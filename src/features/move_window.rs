use std::{env, path::PathBuf};

use crate::{
    herdr::client::Client,
    rebuild,
    tree::{Direction, Layout, Node, Ratio, Rect, SplitDirection},
};

pub(super) fn run(direction: Direction) -> Result<(), String> {
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
    let area = client
        .layout_area(&pane_id, &original.tab_id)
        .map_err(|error| format!("read layout area for pane {pane_id}: {error}"))?;
    let Some(desired) = transform(&original, &pane_id, area, direction)? else {
        return Ok(());
    };
    rebuild::apply(&mut client, &state_dir, &original, &desired)
}

fn transform(
    original: &Layout,
    pane_id: &str,
    area: Rect,
    direction: Direction,
) -> Result<Option<Layout>, String> {
    let panes = original.root.pane_rects(area);
    let (_, source) = panes
        .iter()
        .find(|(id, _)| *id == pane_id)
        .ok_or_else(|| format!("pane {pane_id:?} is absent from the layout"))?;
    if panes.len() == 1 {
        return Ok(None);
    }
    if source.width == 0 || source.height == 0 {
        return Err(format!("pane {pane_id:?} has an empty layout rectangle"));
    }
    let focal = focal_point(*source, direction);

    let mut desired = original.clone();
    let override_direction = remove_pane(&mut desired.root, pane_id, direction)
        .expect("the source is present in a tree with at least two panes");
    // Hyprland resolves the target after collapsing the source's parent.
    // preserve_split=true keeps the remaining axes, but their areas change.
    let remaining = desired.root.pane_rects(area);
    let (target, target_rect) = remaining
        .iter()
        .filter(|(_, rect)| rect.width > 0 && rect.height > 0)
        .min_by(|(_, a), (_, b)| {
            distance_squared(*a, focal).total_cmp(&distance_squared(*b, focal))
        })
        .ok_or_else(|| "remaining layout has no nonempty pane rectangle".to_owned())?;
    let insertion = if override_direction {
        direction
    } else if target_rect.width > target_rect.height {
        if focal.0 < f64::from(target_rect.x) + f64::from(target_rect.width) / 2.0 {
            Direction::Left
        } else {
            Direction::Right
        }
    } else if focal.1 < f64::from(target_rect.y) + f64::from(target_rect.height) / 2.0 {
        Direction::Up
    } else {
        Direction::Down
    };
    let target = (*target).to_owned();
    insert_beside(&mut desired.root, &target, pane_id, insertion);
    Ok((desired != *original).then_some(desired))
}

fn focal_point(rect: Rect, direction: Direction) -> (f64, f64) {
    let x = f64::from(rect.x);
    let y = f64::from(rect.y);
    let width = f64::from(rect.width);
    let height = f64::from(rect.height);
    match direction {
        Direction::Left => (x - 1.0, y + height / 2.0),
        Direction::Right => (x + width + 1.0, y + height / 2.0),
        Direction::Up => (x + width / 2.0, y - 1.0),
        Direction::Down => (x + width / 2.0, y + height + 1.0),
    }
}

fn distance_squared(rect: Rect, (x, y): (f64, f64)) -> f64 {
    let left = f64::from(rect.x);
    let top = f64::from(rect.y);
    let dx = x - x.clamp(left, left + f64::from(rect.width));
    let dy = y - y.clamp(top, top + f64::from(rect.height));
    dx * dx + dy * dy
}

/// Returns whether moving across a direct leaf sibling forces the insertion direction.
fn remove_pane(node: &mut Node, pane_id: &str, movement: Direction) -> Option<bool> {
    let Node::Split {
        direction,
        first,
        second,
        ..
    } = node
    else {
        return None;
    };

    if matches!(&**first, Node::Pane { id } if id == pane_id) {
        let override_direction = matches!(&**second, Node::Pane { .. })
            && matches!(
                (*direction, movement),
                (SplitDirection::Right, Direction::Right) | (SplitDirection::Down, Direction::Down)
            );
        *node = (**second).clone();
        Some(override_direction)
    } else if matches!(&**second, Node::Pane { id } if id == pane_id) {
        let override_direction = matches!(&**first, Node::Pane { .. })
            && matches!(
                (*direction, movement),
                (SplitDirection::Right, Direction::Left) | (SplitDirection::Down, Direction::Up)
            );
        *node = (**first).clone();
        Some(override_direction)
    } else {
        remove_pane(first, pane_id, movement).or_else(|| remove_pane(second, pane_id, movement))
    }
}

fn insert_beside(
    node: &mut Node,
    target_pane_id: &str,
    pane_id: &str,
    direction: Direction,
) -> bool {
    if matches!(node, Node::Pane { id } if id == target_pane_id) {
        let target = Box::new(node.clone());
        let moved = Box::new(Node::Pane {
            id: pane_id.to_owned(),
        });
        let (split_direction, first, second) = match direction {
            Direction::Left => (SplitDirection::Right, moved, target),
            Direction::Right => (SplitDirection::Right, target, moved),
            Direction::Up => (SplitDirection::Down, moved, target),
            Direction::Down => (SplitDirection::Down, target, moved),
        };
        *node = Node::Split {
            direction: split_direction,
            ratio: Ratio::new(0.5).expect("0.5 is a valid ratio"),
            first,
            second,
        };
        return true;
    }

    match node {
        Node::Pane { .. } => false,
        Node::Split { first, second, .. } => {
            insert_beside(first, target_pane_id, pane_id, direction)
                || insert_beside(second, target_pane_id, pane_id, direction)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_adjacent_pane_in_every_direction() {
        for (direction, axis, source, first_id, second_id) in [
            (Direction::Left, SplitDirection::Right, "p2", "p2", "p1"),
            (Direction::Right, SplitDirection::Right, "p1", "p2", "p1"),
            (Direction::Up, SplitDirection::Down, "p2", "p2", "p1"),
            (Direction::Down, SplitDirection::Down, "p1", "p2", "p1"),
        ] {
            let mut original = adjacent_layout();
            original.root = split(axis, 0.8, pane("p1"), pane("p2"));
            original.focused_pane_id = source.into();
            // The sibling override wins even when the area prefers the other axis.
            let area = match axis {
                SplitDirection::Right => area(40, 200),
                SplitDirection::Down => area(200, 40),
            };
            let desired = transform(&original, source, area, direction)
                .unwrap()
                .unwrap();

            assert_eq!(
                desired.root,
                split(axis, 0.5, pane(first_id), pane(second_id))
            );
            assert_eq!(desired.workspace_id, original.workspace_id);
            assert_eq!(desired.tab_id, original.tab_id);
            assert_eq!(desired.focused_pane_id, source);
            assert!(desired.zoomed);
        }
    }

    #[test]
    fn moves_between_nested_positions_without_changing_other_ratios() {
        let original = nested_layout();
        let desired = transform(&original, "p2", area(200, 100), Direction::Right)
            .unwrap()
            .unwrap();

        assert_eq!(
            desired.root,
            split(
                SplitDirection::Right,
                0.6,
                pane("p1"),
                split(
                    SplitDirection::Down,
                    0.7,
                    split(SplitDirection::Right, 0.5, pane("p2"), pane("p3")),
                    split(SplitDirection::Right, 0.4, pane("p4"), pane("p5"))
                )
            )
        );
        assert_eq!(desired.focused_pane_id, original.focused_pane_id);
        assert_eq!(desired.zoomed, original.zoomed);
    }

    #[test]
    fn moves_only_the_middle_leaf_left_using_the_target_aspect_ratio() {
        for (ratio, axis) in [
            (0.6, SplitDirection::Right),
            (0.4, SplitDirection::Down),
            (0.5, SplitDirection::Down),
        ] {
            let mut original = adjacent_layout();
            original.root = split(
                SplitDirection::Right,
                ratio,
                pane("p1"),
                split(SplitDirection::Right, 0.3, pane("p2"), pane("p3")),
            );
            original.focused_pane_id = "p2".into();
            let desired = transform(&original, "p2", area(200, 100), Direction::Left)
                .unwrap()
                .unwrap();
            assert_eq!(
                desired.root,
                split(
                    SplitDirection::Right,
                    ratio,
                    split(axis, 0.5, pane("p1"), pane("p2")),
                    pane("p3")
                )
            );
            assert_eq!(desired.focused_pane_id, "p2");
            assert!(desired.zoomed);
        }
    }

    #[test]
    fn moves_between_subtrees_in_all_four_directions() {
        for (movement, axis, area, source_on_first_side) in [
            (Direction::Left, SplitDirection::Right, area(240, 40), false),
            (Direction::Right, SplitDirection::Right, area(240, 40), true),
            (Direction::Up, SplitDirection::Down, area(40, 240), false),
            (Direction::Down, SplitDirection::Down, area(40, 240), true),
        ] {
            let left_nested = split(
                axis,
                0.5,
                split(axis, 0.5, pane("p1"), pane("p2")),
                pane("p3"),
            );
            let right_nested = split(
                axis,
                0.5,
                pane("p1"),
                split(axis, 0.5, pane("p2"), pane("p3")),
            );
            let (root, expected) = if source_on_first_side {
                (left_nested, right_nested)
            } else {
                (right_nested, left_nested)
            };
            let mut original = adjacent_layout();
            original.root = root;
            let desired = transform(&original, "p2", area, movement).unwrap().unwrap();
            assert_eq!(desired.root, expected, "{movement}");
        }
    }

    #[test]
    fn removing_a_root_leaf_reselects_target_in_the_expanded_subtree() {
        let mut original = adjacent_layout();
        original.root = split(
            SplitDirection::Right,
            0.5,
            pane("p1"),
            split(SplitDirection::Right, 0.3, pane("p2"), pane("p3")),
        );
        // Before removal p2 is the right neighbor. After removal the focal point
        // is inside p3, so Hyprland splits p3 instead.
        let desired = transform(&original, "p1", area(200, 80), Direction::Right)
            .unwrap()
            .unwrap();
        assert_eq!(
            desired.root,
            split(
                SplitDirection::Right,
                0.3,
                pane("p2"),
                split(SplitDirection::Right, 0.5, pane("p1"), pane("p3"))
            )
        );
    }

    #[test]
    fn moving_outward_at_the_tab_edge_still_reinserts() {
        let mut original = adjacent_layout();
        original.root = split(SplitDirection::Down, 0.8, pane("p1"), pane("p2"));
        let desired = transform(&original, "p1", area(200, 80), Direction::Left)
            .unwrap()
            .unwrap();
        assert_eq!(
            desired.root,
            split(SplitDirection::Right, 0.5, pane("p1"), pane("p2"))
        );
        assert_eq!(
            transform(&desired, "p1", area(200, 80), Direction::Left),
            Ok(None)
        );
    }

    #[test]
    fn single_pane_is_a_no_op_and_missing_or_empty_source_is_rejected() {
        let mut original = adjacent_layout();
        assert!(transform(&original, "missing", area(200, 80), Direction::Left).is_err());
        original.root = split(SplitDirection::Right, 0.0, pane("p1"), pane("p2"));
        assert!(transform(&original, "p1", area(200, 80), Direction::Left).is_err());
        original.root = pane("p1");
        for direction in [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ] {
            assert_eq!(
                transform(&original, "p1", area(200, 80), direction),
                Ok(None)
            );
        }
    }

    fn area(width: u16, height: u16) -> Rect {
        Rect {
            x: 3,
            y: 7,
            width,
            height,
        }
    }

    fn adjacent_layout() -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: "p1".into(),
            root: split(SplitDirection::Down, 0.8, pane("p1"), pane("p2")),
        }
    }

    fn nested_layout() -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: "p2".into(),
            root: split(
                SplitDirection::Right,
                0.6,
                split(SplitDirection::Down, 0.3, pane("p1"), pane("p2")),
                split(
                    SplitDirection::Down,
                    0.7,
                    pane("p3"),
                    split(SplitDirection::Right, 0.4, pane("p4"), pane("p5")),
                ),
            ),
        }
    }

    fn pane(id: &str) -> Node {
        Node::Pane { id: id.into() }
    }

    fn split(direction: SplitDirection, ratio: f32, first: Node, second: Node) -> Node {
        Node::Split {
            direction,
            ratio: Ratio::new(ratio).unwrap(),
            first: Box::new(first),
            second: Box::new(second),
        }
    }
}
