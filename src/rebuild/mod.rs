//! Deterministic, journaled reconstruction of a live-pane BSP tree.

mod plan;
mod state;

use std::path::Path;

pub use plan::{Command, Plan, compile};

use crate::{
    herdr::client::{Client, MovedPane},
    tree::{Layout, Ratio, SplitDirection},
};
use plan::collect_panes;
use state::{Journal, OperationLock, journal_path, load, remove, save};

/// Rebuilds `desired` while retaining all live panes from `original`.
///
/// The journal remains if a command fails or the final export differs from `desired`.
///
/// # Errors
///
/// Returns an error for incompatible layouts, lock contention, journal I/O,
/// a failed Herdr command, or a mismatched final layout.
pub fn apply(
    client: &mut Client,
    state_dir: &Path,
    original: &Layout,
    desired: &Layout,
) -> Result<(), String> {
    let _lock = OperationLock::acquire(state_dir)?;
    apply_locked(client, state_dir, original, desired)
}

pub(crate) fn lock(state_dir: &Path) -> Result<OperationLock, String> {
    OperationLock::acquire(state_dir)
}

pub(crate) fn apply_locked(
    client: &mut Client,
    state_dir: &Path,
    original: &Layout,
    desired: &Layout,
) -> Result<(), String> {
    ensure_recovered(state_dir)?;
    let journal_path = journal_path(state_dir);
    validate_compatible(original, desired)?;
    let plan = compile(desired)?;
    let mut journal = Journal {
        original: original.clone(),
        active_anchor: plan.anchor_pane_id.clone(),
        completed_step: 0,
    };
    save(&journal_path, &journal)?;
    execute(client, &plan, &mut journal, &journal_path)?;
    verify_layout(client, desired)?;
    remove(&journal_path)
}

pub(crate) fn ensure_recovered(state_dir: &Path) -> Result<(), String> {
    let journal_path = journal_path(state_dir);
    if journal_path.exists() {
        return Err("an interrupted rebuild must be recovered first".to_owned());
    }
    Ok(())
}

/// Restores the original tree from an interrupted rebuild, if one exists.
///
/// Repeated calls after successful recovery are no-ops.
///
/// # Errors
///
/// Returns an error for lock contention, an invalid journal, journal I/O, or
/// any failed Herdr command. A failed recovery keeps the journal intact.
pub fn recover(client: &mut Client, state_dir: &Path) -> Result<(), String> {
    let _lock = OperationLock::acquire(state_dir)?;
    let journal_path = journal_path(state_dir);
    let Some(mut journal) = load(&journal_path)? else {
        return Ok(());
    };
    let plan = compile(&journal.original)?;

    if journal.active_anchor != plan.anchor_pane_id {
        client
            .zoom(&journal.active_anchor, false)
            .map_err(|error| format!("disable zoom before recovery: {error}"))?;
        let moved = client
            .move_to_tab(
                &plan.anchor_pane_id,
                &journal.original.tab_id,
                &journal.active_anchor,
                SplitDirection::Right,
                Ratio::new(0.5).expect("0.5 is a valid ratio"),
            )
            .map_err(|error| format!("prepare recovery anchor: {error}"))?;
        if !moved.changed && moved.reason.as_deref() != Some("same_tab") {
            return Err(format!(
                "prepare recovery anchor: pane move made no change ({})",
                moved.reason.as_deref().unwrap_or("unknown reason")
            ));
        }
        journal.active_anchor = plan.anchor_pane_id.clone();
        journal.completed_step = 0;
        save(&journal_path, &journal)?;
    }

    execute(client, &plan, &mut journal, &journal_path)?;
    verify_layout(client, &journal.original)?;
    remove(&journal_path)
}

fn verify_layout(client: &mut Client, desired: &Layout) -> Result<(), String> {
    let actual = client
        .export_layout(&desired.focused_pane_id)
        .map_err(|error| format!("verify rebuilt layout: {error}"))?;
    if actual != *desired {
        return Err(
            "rebuilt layout differs from the desired tree; recovery journal retained".into(),
        );
    }
    Ok(())
}

fn execute(
    client: &mut Client,
    plan: &Plan,
    journal: &mut Journal,
    journal_path: &Path,
) -> Result<(), String> {
    let mut scratch_tab_id = None;
    for (index, command) in plan.commands.iter().enumerate() {
        match command {
            Command::OpenScratch { pane_id } => {
                let moved = client
                    .move_to_new_tab(pane_id, &journal.original.workspace_id)
                    .map_err(|error| format!("park pane {pane_id}: {error}"))?;
                require_move(&moved, &format!("park pane {pane_id}"))?;
                scratch_tab_id = Some(moved.tab_id);
            }
            Command::Park {
                pane_id,
                target_pane_id,
            } => {
                let tab_id = scratch_tab_id.as_deref().ok_or_else(|| {
                    "rebuild plan parks a pane before opening the scratch tab".to_owned()
                })?;
                let moved = client
                    .move_to_tab(
                        pane_id,
                        tab_id,
                        target_pane_id,
                        SplitDirection::Right,
                        Ratio::new(0.5).expect("0.5 is a valid ratio"),
                    )
                    .map_err(|error| format!("park pane {pane_id}: {error}"))?;
                require_move(&moved, &format!("park pane {pane_id}"))?;
            }
            Command::Place {
                pane_id,
                target_pane_id,
                direction,
                ratio,
            } => {
                let moved = client
                    .move_to_tab(
                        pane_id,
                        &journal.original.tab_id,
                        target_pane_id,
                        *direction,
                        *ratio,
                    )
                    .map_err(|error| format!("place pane {pane_id}: {error}"))?;
                require_move(&moved, &format!("place pane {pane_id}"))?;
            }
            Command::SetRatio { path, ratio } => client
                .set_split_ratio(&journal.original.tab_id, path, *ratio)
                .map_err(|error| format!("restore split ratio at {path:?}: {error}"))?,
            Command::Focus { pane_id } => client
                .focus(pane_id)
                .map_err(|error| format!("restore focus to {pane_id}: {error}"))?,
            Command::Zoom { pane_id, zoomed } => client
                .zoom(pane_id, *zoomed)
                .map_err(|error| format!("restore zoom for {pane_id}: {error}"))?,
        }
        journal.completed_step = index + 1;
        save(journal_path, journal)?;
    }
    Ok(())
}

fn require_move(moved: &MovedPane, operation: &str) -> Result<(), String> {
    moved.changed.then_some(()).ok_or_else(|| {
        format!(
            "{operation}: pane move made no change ({})",
            moved.reason.as_deref().unwrap_or("unknown reason")
        )
    })
}

fn validate_compatible(original: &Layout, desired: &Layout) -> Result<(), String> {
    if original.workspace_id != desired.workspace_id || original.tab_id != desired.tab_id {
        return Err("original and desired layouts must address the same tab".to_owned());
    }
    let mut original_panes = Vec::new();
    let mut desired_panes = Vec::new();
    collect_panes(&original.root, &mut original_panes);
    collect_panes(&desired.root, &mut desired_panes);
    original_panes.sort();
    desired_panes.sort();
    if original_panes != desired_panes {
        return Err("original and desired layouts must contain the same panes".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    use serde_json::json;

    use super::*;
    use crate::tree::Node;

    #[test]
    fn recovery_restores_the_original_tree_after_every_interrupted_step() {
        let original = two_pane_layout();
        let step_count = compile(&original).unwrap().commands.len();

        for completed_step in 0..=step_count {
            let state_dir = test_directory();
            fs::create_dir_all(&state_dir).unwrap();
            state::save(
                &state::journal_path(&state_dir),
                &Journal {
                    original: original.clone(),
                    active_anchor: "w1:p2".into(),
                    completed_step,
                },
            )
            .unwrap();
            let (socket, server) = serve_recovery(&state_dir);
            let mut client = Client::new(socket);

            recover(&mut client, &state_dir).unwrap();
            server.join().unwrap();
            assert!(!state::journal_path(&state_dir).exists());
            recover(&mut client, &state_dir).unwrap();
            fs::remove_dir_all(state_dir).unwrap();
        }
    }

    #[test]
    fn concurrent_apply_fails_before_contacting_herdr_or_writing_a_journal() {
        let state_dir = test_directory();
        let _lock = state::OperationLock::acquire(&state_dir).unwrap();
        let layout = two_pane_layout();

        let error = apply(
            &mut Client::new(state_dir.join("missing.sock")),
            &state_dir,
            &layout,
            &layout,
        )
        .unwrap_err();

        assert!(error.contains("already running"));
        assert!(!state::journal_path(&state_dir).exists());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn successful_commands_with_a_wrong_final_layout_keep_the_journal() {
        let state_dir = test_directory();
        fs::create_dir_all(&state_dir).unwrap();
        let mut original = two_pane_layout();
        original.root = Node::Pane { id: "w1:p1".into() };
        let socket = state_dir.join("mismatch.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            for (method, result) in [
                ("pane.focus", json!({"type": "pane_info"})),
                ("pane.zoom", json!({"type": "pane_zoom"})),
                (
                    "layout.export",
                    json!({"type": "layout_export", "layout": {
                        "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": false,
                        "focused_pane_id": "w1:p1",
                        "root": {"type": "pane", "pane_id": "w1:p1"}
                    }}),
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["method"], method);
                serde_json::to_writer(&mut stream, &json!({"id": request["id"], "result": result}))
                    .unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let error = apply(&mut Client::new(socket), &state_dir, &original, &original).unwrap_err();
        server.join().unwrap();
        assert!(error.contains("differs from the desired tree"));
        assert_eq!(
            state::load(&state::journal_path(&state_dir))
                .unwrap()
                .unwrap()
                .original,
            original
        );
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn failed_rebuild_keeps_the_original_journal() {
        let state_dir = test_directory();
        fs::create_dir_all(&state_dir).unwrap();
        let original = two_pane_layout();
        let mut desired = original.clone();
        desired.root = Node::Split {
            direction: SplitDirection::Down,
            ratio: ratio(0.5),
            first: Box::new(Node::Pane { id: "w1:p2".into() }),
            second: Box::new(Node::Pane { id: "w1:p1".into() }),
        };
        let socket = state_dir.join("failed.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                let id = request["id"].as_str().unwrap();
                if index == 0 {
                    assert_eq!(request["method"], "pane.zoom");
                    serde_json::to_writer(
                        &mut stream,
                        &json!({"id": id, "result": {"type": "pane_zoom"}}),
                    )
                    .unwrap();
                } else {
                    assert_eq!(request["method"], "pane.move");
                    assert_eq!(request["params"]["destination"]["type"], "new_tab");
                    serde_json::to_writer(
                        &mut stream,
                        &json!({
                            "id": id,
                            "result": {
                                "type": "pane_move",
                                "move_result": {
                                    "changed": false,
                                    "reason": "zoomed_tab",
                                    "pane": {"pane_id": "w1:p1", "tab_id": "w1:t1"}
                                }
                            }
                        }),
                    )
                    .unwrap();
                }
                stream.write_all(b"\n").unwrap();
            }
        });

        let error = apply(&mut Client::new(socket), &state_dir, &original, &desired).unwrap_err();
        server.join().unwrap();
        assert!(error.contains("zoomed_tab"));
        let journal = state::load(&state::journal_path(&state_dir))
            .unwrap()
            .unwrap();
        assert_eq!(journal.original, original);
        assert_eq!(journal.completed_step, 1);
        fs::remove_dir_all(state_dir).unwrap();
    }

    fn two_pane_layout() -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "w1:t1".into(),
            zoomed: true,
            focused_pane_id: "w1:p1".into(),
            root: Node::Split {
                direction: SplitDirection::Right,
                ratio: ratio(0.6),
                first: Box::new(Node::Pane { id: "w1:p1".into() }),
                second: Box::new(Node::Pane { id: "w1:p2".into() }),
            },
        }
    }

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    fn test_directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "herdr-tiling-rebuild-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn serve_recovery(state_dir: &Path) -> (PathBuf, thread::JoinHandle<()>) {
        let socket = state_dir.join("test.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let methods = [
                "pane.zoom",
                "pane.move",
                "pane.zoom",
                "pane.move",
                "pane.move",
                "layout.set_split_ratio",
                "pane.focus",
                "pane.zoom",
                "layout.export",
            ];
            for (index, expected_method) in methods.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                assert_eq!(request["method"], expected_method);
                match index {
                    3 => {
                        assert_eq!(request["params"]["destination"]["type"], "new_tab");
                        assert_eq!(request["params"]["destination"]["workspace_id"], "w1");
                    }
                    4 => {
                        assert_eq!(request["params"]["destination"]["type"], "tab");
                        assert_eq!(request["params"]["destination"]["tab_id"], "w1:t1");
                    }
                    5 => assert_eq!(request["params"]["path"], json!([])),
                    6 => assert_eq!(request["params"]["pane_id"], "w1:p1"),
                    7 => assert_eq!(request["params"]["mode"], "on"),
                    _ => {}
                }
                let id = request["id"].as_str().unwrap();
                let result = match index {
                    0 | 2 | 7 => json!({"type": "pane_zoom"}),
                    1 => json!({
                        "type": "pane_move",
                        "move_result": {"changed": true, "reason": null, "pane": {"pane_id": "w1:p1", "tab_id": "w1:t1"}}
                    }),
                    3 => json!({
                        "type": "pane_move",
                        "move_result": {"changed": true, "reason": null, "pane": {"pane_id": "w1:p2", "tab_id": "w1:t2"}}
                    }),
                    4 => json!({
                        "type": "pane_move",
                        "move_result": {"changed": true, "reason": null, "pane": {"pane_id": "w1:p2", "tab_id": "w1:t1"}}
                    }),
                    5 => json!({"type": "layout_split_ratio_set"}),
                    6 => json!({"type": "pane_info"}),
                    8 => json!({"type": "layout_export", "layout": {
                        "workspace_id": "w1", "tab_id": "w1:t1", "zoomed": true,
                        "focused_pane_id": "w1:p1", "root": {
                            "type": "split", "direction": "right", "ratio": 0.6,
                            "first": {"type": "pane", "pane_id": "w1:p1"},
                            "second": {"type": "pane", "pane_id": "w1:p2"}
                        }
                    }}),
                    _ => unreachable!(),
                };
                serde_json::to_writer(&mut stream, &json!({"id": id, "result": result})).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        (socket, server)
    }
}
