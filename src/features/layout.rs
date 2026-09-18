use std::{
    env, fs,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    herdr::client::Client,
    preset::Preset,
    rebuild,
    tree::{Layout, Node, Ratio, SplitDirection},
};

const STATE_FILE: &str = "layouts.json";

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    last_layouts: Vec<LastLayout>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LastLayout {
    socket_path: PathBuf,
    tab_id: String,
    preset: Preset,
}

impl State {
    fn load(state_dir: &Path) -> Result<Self, String> {
        let path = state_dir.join(STATE_FILE);
        match fs::read(&path) {
            Ok(contents) => serde_json::from_slice(&contents)
                .map_err(|error| format!("read layout state {}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("read layout state {}: {error}", path.display())),
        }
    }

    fn get(&self, socket_path: &Path, tab_id: &str) -> Option<Preset> {
        self.last_layouts
            .iter()
            .find(|entry| entry.socket_path == socket_path && entry.tab_id == tab_id)
            .map(|entry| entry.preset)
    }

    fn set(&mut self, socket_path: &Path, tab_id: &str, preset: Preset) {
        if let Some(entry) = self
            .last_layouts
            .iter_mut()
            .find(|entry| entry.socket_path == socket_path && entry.tab_id == tab_id)
        {
            entry.preset = preset;
        } else {
            self.last_layouts.push(LastLayout {
                socket_path: socket_path.to_owned(),
                tab_id: tab_id.to_owned(),
                preset,
            });
        }
    }

    fn save(&self, state_dir: &Path) -> Result<(), String> {
        let path = state_dir.join(STATE_FILE);
        let temporary = path.with_extension("json.tmp");
        let contents =
            serde_json::to_vec(self).map_err(|error| format!("serialize layout state: {error}"))?;
        let mut file = File::create(&temporary)
            .map_err(|error| format!("create layout state {}: {error}", temporary.display()))?;
        file.write_all(&contents)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("write layout state {}: {error}", temporary.display()))?;
        fs::rename(&temporary, &path)
            .map_err(|error| format!("publish layout state {}: {error}", path.display()))
    }
}

pub(super) fn run(requested: Option<Preset>) -> Result<(), String> {
    let tab_id = env::var("HERDR_TAB_ID")
        .map_err(|_| "required environment variable HERDR_TAB_ID is not valid UTF-8".to_owned())?;
    let socket_path = required_path("HERDR_SOCKET_PATH")?;
    let state_dir = required_path("HERDR_PLUGIN_STATE_DIR")?;
    let config_dir = required_path("HERDR_PLUGIN_CONFIG_DIR")?;
    let config = Config::load(&config_dir)?;

    let _lock = rebuild::lock(&state_dir)?;
    rebuild::ensure_recovered(&state_dir)?;
    let mut state = State::load(&state_dir)?;
    let preset = requested
        .unwrap_or_else(|| next_preset(&config.cycle_layouts, state.get(&socket_path, &tab_id)));
    let mut client = Client::new(&socket_path);
    let original = client
        .export_tab_layout(&tab_id)
        .map_err(|error| format!("export layout for tab {tab_id}: {error}"))?;
    let desired = transform(&original, preset, config.main_ratio);
    if desired.root != original.root {
        rebuild::apply_locked(&mut client, &state_dir, &original, &desired)?;
    }
    state.set(&socket_path, &tab_id, preset);
    state.save(&state_dir)
}

fn required_path(name: &str) -> Result<PathBuf, String> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("required environment variable {name} is missing"))
}

fn next_preset(cycle: &[Preset], current: Option<Preset>) -> Preset {
    current
        .and_then(|preset| cycle.iter().position(|candidate| *candidate == preset))
        .map(|index| cycle[(index + 1) % cycle.len()])
        .unwrap_or(cycle[0])
}

fn transform(original: &Layout, preset: Preset, main_ratio: f32) -> Layout {
    let mut pane_ids = Vec::new();
    collect_panes(&original.root, &mut pane_ids);
    let mut desired = original.clone();
    desired.root = build(preset, pane_ids, main_ratio);
    desired
}

fn collect_panes(node: &Node, pane_ids: &mut Vec<String>) {
    match node {
        Node::Pane { id } => pane_ids.push(id.clone()),
        Node::Split { first, second, .. } => {
            collect_panes(first, pane_ids);
            collect_panes(second, pane_ids);
        }
    }
}

fn build(preset: Preset, pane_ids: Vec<String>, main_ratio: f32) -> Node {
    let mut panes = pane_ids.into_iter().map(pane).collect::<Vec<_>>();
    if panes.len() == 1 {
        return panes.pop().expect("a layout contains at least one pane");
    }
    match preset {
        Preset::EvenHorizontal => even(panes, SplitDirection::Right),
        Preset::EvenVertical => even(panes, SplitDirection::Down),
        Preset::MainHorizontal => main(panes, SplitDirection::Down, main_ratio),
        Preset::MainVertical => main(panes, SplitDirection::Right, main_ratio),
        Preset::Tiled => tiled(panes),
    }
}

fn pane(id: String) -> Node {
    Node::Pane { id }
}

fn main(mut panes: Vec<Node>, direction: SplitDirection, main_ratio: f32) -> Node {
    let others = panes.split_off(1);
    split(
        direction,
        main_ratio,
        panes.pop().expect("main layout has a pane"),
        even(
            others,
            match direction {
                SplitDirection::Right => SplitDirection::Down,
                SplitDirection::Down => SplitDirection::Right,
            },
        ),
    )
}

fn tiled(panes: Vec<Node>) -> Node {
    let pane_count = panes.len();
    let mut rows = 1;
    let mut columns = 1;
    while rows * columns < pane_count {
        rows += 1;
        if rows * columns < pane_count {
            columns += 1;
        }
    }
    let mut panes = panes.into_iter();
    let rows = (0..rows)
        .map(|_| {
            even(
                panes.by_ref().take(columns).collect(),
                SplitDirection::Right,
            )
        })
        .collect();
    even(rows, SplitDirection::Down)
}

fn even(mut nodes: Vec<Node>, direction: SplitDirection) -> Node {
    if nodes.len() == 1 {
        return nodes.pop().expect("an even group contains a node");
    }
    let count = nodes.len();
    let second = nodes.split_off(count.div_ceil(2));
    let ratio = nodes.len() as f32 / count as f32;
    split(
        direction,
        ratio,
        even(nodes, direction),
        even(second, direction),
    )
}

fn split(direction: SplitDirection, ratio: f32, first: Node, second: Node) -> Node {
    Node::Split {
        direction,
        ratio: Ratio::new(ratio).expect("preset ratios are between zero and one"),
        first: Box::new(first),
        second: Box::new(second),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::tree::Rect;

    #[test]
    fn builds_all_presets_in_tree_order() {
        let original = layout(5);
        for preset in Preset::ALL {
            let desired = transform(&original, preset, 0.6);
            let mut actual = Vec::new();
            collect_panes(&desired.root, &mut actual);
            assert_eq!(actual, ["p1", "p2", "p3", "p4", "p5"]);
            assert_eq!(desired.focused_pane_id, original.focused_pane_id);
            assert_eq!(desired.zoomed, original.zoomed);
        }

        let main_horizontal = transform(&original, Preset::MainHorizontal, 0.6);
        assert!(matches!(
            main_horizontal.root,
            Node::Split {
                direction: SplitDirection::Down,
                ratio,
                ..
            } if ratio.get() == 0.6
        ));
        let main_vertical = transform(&original, Preset::MainVertical, 0.6);
        assert!(matches!(
            main_vertical.root,
            Node::Split {
                direction: SplitDirection::Right,
                ratio,
                ..
            } if ratio.get() == 0.6
        ));

        let single = layout(1);
        for preset in Preset::ALL {
            assert_eq!(transform(&single, preset, 0.5), single);
        }
    }

    #[test]
    fn even_layouts_stay_even_without_small_split_ratios() {
        for preset in [Preset::EvenHorizontal, Preset::EvenVertical] {
            for pane_count in 2..=24 {
                let desired = transform(&layout(pane_count), preset, 0.5);
                assert_ratios_in_range(&desired.root);
                let rects = desired.root.pane_rects(Rect {
                    x: 0,
                    y: 0,
                    width: 240,
                    height: 240,
                });
                let sizes = rects
                    .iter()
                    .map(|(_, rect)| match preset {
                        Preset::EvenHorizontal => rect.width,
                        Preset::EvenVertical => rect.height,
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>();
                assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
            }
        }
    }

    #[test]
    fn tiled_uses_tmux_row_and_column_growth() {
        let desired = transform(&layout(5), Preset::Tiled, 0.5);
        let rects = desired.root.pane_rects(Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 90,
        });
        assert_eq!(
            rects[0].1,
            Rect {
                x: 0,
                y: 0,
                width: 60,
                height: 30
            }
        );
        assert_eq!(
            rects[1].1,
            Rect {
                x: 60,
                y: 0,
                width: 60,
                height: 30
            }
        );
        assert_eq!(
            rects[4].1,
            Rect {
                x: 0,
                y: 60,
                width: 120,
                height: 30
            }
        );
    }

    #[test]
    fn cycle_uses_configured_order_and_falls_back_to_the_first_layout() {
        let cycle = [Preset::Tiled, Preset::EvenHorizontal];
        assert_eq!(next_preset(&cycle, None), Preset::Tiled);
        assert_eq!(
            next_preset(&cycle, Some(Preset::Tiled)),
            Preset::EvenHorizontal
        );
        assert_eq!(
            next_preset(&cycle, Some(Preset::EvenVertical)),
            Preset::Tiled
        );
    }

    #[test]
    fn state_is_scoped_by_socket_and_tab() {
        let directory = test_directory();
        fs::create_dir_all(&directory).unwrap();
        let mut state = State::default();
        state.set(Path::new("/one.sock"), "t1", Preset::Tiled);
        state.set(Path::new("/two.sock"), "t1", Preset::EvenVertical);
        state.save(&directory).unwrap();
        let state = State::load(&directory).unwrap();
        assert_eq!(state.get(Path::new("/one.sock"), "t1"), Some(Preset::Tiled));
        assert_eq!(
            state.get(Path::new("/two.sock"), "t1"),
            Some(Preset::EvenVertical)
        );
        assert_eq!(state.get(Path::new("/one.sock"), "t2"), None);
        fs::remove_dir_all(directory).unwrap();
    }

    fn assert_ratios_in_range(node: &Node) {
        if let Node::Split {
            ratio,
            first,
            second,
            ..
        } = node
        {
            assert!((0.1..=0.9).contains(&ratio.get()));
            assert_ratios_in_range(first);
            assert_ratios_in_range(second);
        }
    }

    fn layout(count: usize) -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: "p2".into(),
            root: even(
                (1..=count).map(|index| pane(format!("p{index}"))).collect(),
                SplitDirection::Right,
            ),
        }
    }

    fn test_directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        env::temp_dir().join(format!(
            "herdr-tiling-layout-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
