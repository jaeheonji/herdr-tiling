use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::tree::Layout;

const JOURNAL_FILE: &str = "rebuild.json";
const LOCK_FILE: &str = "rebuild.lock";

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Journal {
    pub(super) original: Layout,
    pub(super) active_anchor: String,
    pub(super) completed_step: usize,
}

#[derive(Debug)]
pub(crate) struct OperationLock {
    _file: File,
}

impl OperationLock {
    pub(crate) fn acquire(state_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(state_dir)
            .map_err(|error| format!("create plugin state directory: {error}"))?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(state_dir.join(LOCK_FILE))
            .map_err(|error| format!("open rebuild lock: {error}"))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => {
                Err("another herdr-tiling operation is already running".to_owned())
            }
            Err(TryLockError::Error(error)) => Err(format!("lock rebuild operation: {error}")),
        }
    }
}

pub(super) fn journal_path(state_dir: &Path) -> PathBuf {
    state_dir.join(JOURNAL_FILE)
}

pub(super) fn load(path: &Path) -> Result<Option<Journal>, String> {
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| format!("read recovery journal {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read recovery journal {}: {error}", path.display())),
    }
}

pub(super) fn save(path: &Path, journal: &Journal) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let contents = serde_json::to_vec(journal)
        .map_err(|error| format!("serialize recovery journal: {error}"))?;
    let mut file = File::create(&temporary)
        .map_err(|error| format!("create recovery journal {}: {error}", temporary.display()))?;
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("write recovery journal {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("publish recovery journal {}: {error}", path.display()))
}

pub(super) fn remove(path: &Path) -> Result<(), String> {
    fs::remove_file(path)
        .map_err(|error| format!("remove recovery journal {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::tree::{Node, Ratio, SplitDirection};

    use super::*;

    #[test]
    fn rejects_lock_contention_and_recovers_after_release() {
        let state_dir = test_directory();
        let first = OperationLock::acquire(&state_dir).unwrap();
        assert!(
            OperationLock::acquire(&state_dir)
                .unwrap_err()
                .contains("already running")
        );
        drop(first);
        assert!(OperationLock::acquire(&state_dir).is_ok());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn journal_round_trips_and_missing_journal_is_a_no_op() {
        let state_dir = test_directory();
        fs::create_dir_all(&state_dir).unwrap();
        let path = journal_path(&state_dir);
        let journal = Journal {
            original: sample_layout(),
            active_anchor: "p1".into(),
            completed_step: 3,
        };

        save(&path, &journal).unwrap();
        let restored = load(&path).unwrap().unwrap();
        assert_eq!(restored.original, journal.original);
        assert_eq!(restored.active_anchor, "p1");
        assert_eq!(restored.completed_step, 3);
        remove(&path).unwrap();
        assert!(load(&path).unwrap().is_none());
        fs::remove_dir_all(state_dir).unwrap();
    }

    fn sample_layout() -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: "p2".into(),
            root: Node::Split {
                direction: SplitDirection::Right,
                ratio: Ratio::new(0.6).unwrap(),
                first: Box::new(Node::Pane { id: "p1".into() }),
                second: Box::new(Node::Pane { id: "p2".into() }),
            },
        }
    }

    fn test_directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "herdr-tiling-state-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
