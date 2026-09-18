use std::{env, path::PathBuf};

use crate::{
    herdr::client::Client,
    rebuild,
    tree::{Node, SplitDirection},
};

pub(super) fn run() -> Result<(), String> {
    let pane_id = env::var("HERDR_PANE_ID")
        .map_err(|_| "required environment variable HERDR_PANE_ID is not valid UTF-8".to_owned())?;
    let socket_path = env::var_os("HERDR_SOCKET_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| "required environment variable HERDR_SOCKET_PATH is missing".to_owned())?;
    let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "required environment variable HERDR_PLUGIN_STATE_DIR is missing".to_owned()
        })?;

    let mut client = Client::new(socket_path);
    let original = client
        .export_layout(&pane_id)
        .map_err(|error| format!("export layout for pane {pane_id}: {error}"))?;
    let mut desired = original.clone();
    if !toggle_parent(&mut desired.root, &pane_id) {
        return Ok(());
    }
    rebuild::apply(&mut client, &state_dir, &original, &desired)
}

fn toggle_parent(node: &mut Node, pane_id: &str) -> bool {
    let Node::Split {
        direction,
        first,
        second,
        ..
    } = node
    else {
        return false;
    };

    if matches!(&**first, Node::Pane { id } if id == pane_id)
        || matches!(&**second, Node::Pane { id } if id == pane_id)
    {
        *direction = match direction {
            SplitDirection::Right => SplitDirection::Down,
            SplitDirection::Down => SplitDirection::Right,
        };
        return true;
    }

    toggle_parent(first, pane_id) || toggle_parent(second, pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Layout, Ratio};

    #[test]
    fn toggles_only_the_focused_panes_direct_parent() {
        for (focused_pane_id, original_direction, expected_direction) in [
            ("p1", SplitDirection::Right, SplitDirection::Down),
            ("p2", SplitDirection::Down, SplitDirection::Right),
        ] {
            let mut layout = nested_layout(focused_pane_id, original_direction);
            assert!(toggle_parent(&mut layout.root, focused_pane_id));

            let Node::Split {
                direction: root_direction,
                ratio: root_ratio,
                first,
                second,
            } = &layout.root
            else {
                panic!("root must remain a split");
            };
            assert_eq!(*root_direction, SplitDirection::Down);
            assert_eq!(root_ratio.get(), 0.7);
            assert_eq!(**second, Node::Pane { id: "p3".into() });

            let Node::Split {
                direction,
                ratio,
                first,
                second,
            } = &**first
            else {
                panic!("focused pane parent must remain a split");
            };
            assert_eq!(*direction, expected_direction);
            assert_eq!(ratio.get(), 0.3);
            assert_eq!(**first, Node::Pane { id: "p1".into() });
            assert_eq!(**second, Node::Pane { id: "p2".into() });
            assert_eq!(layout.focused_pane_id, focused_pane_id);
            assert!(layout.zoomed);
        }
    }

    #[test]
    fn single_pane_is_unchanged() {
        let mut root = Node::Pane { id: "p1".into() };
        assert!(!toggle_parent(&mut root, "p1"));
        assert_eq!(root, Node::Pane { id: "p1".into() });
    }

    fn nested_layout(focused_pane_id: &str, direction: SplitDirection) -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: focused_pane_id.into(),
            root: Node::Split {
                direction: SplitDirection::Down,
                ratio: Ratio::new(0.7).unwrap(),
                first: Box::new(Node::Split {
                    direction,
                    ratio: Ratio::new(0.3).unwrap(),
                    first: Box::new(Node::Pane { id: "p1".into() }),
                    second: Box::new(Node::Pane { id: "p2".into() }),
                }),
                second: Box::new(Node::Pane { id: "p3".into() }),
            },
        }
    }
}
