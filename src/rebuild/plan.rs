use std::collections::HashSet;

use crate::tree::{Layout, Node, Ratio, SplitDirection};

/// One deterministic Herdr mutation in a rebuild plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Moves the first parked pane into a newly created scratch tab.
    OpenScratch { pane_id: String },
    /// Moves another pane beside the scratch tab's anchor.
    Park {
        pane_id: String,
        target_pane_id: String,
    },
    /// Moves a pane back beside an already placed pane in the original tab.
    Place {
        pane_id: String,
        target_pane_id: String,
        direction: SplitDirection,
        ratio: Ratio,
    },
    /// Restores a split ratio after the topology is complete.
    SetRatio { path: Vec<bool>, ratio: Ratio },
    /// Restores pane focus.
    Focus { pane_id: String },
    /// Restores zoom state.
    Zoom { pane_id: String, zoomed: bool },
}

/// A rebuild plan whose command order depends only on the desired tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Pane that remains in the original tab while all others are parked.
    pub anchor_pane_id: String,
    /// Commands to execute in order.
    pub commands: Vec<Command>,
}

/// Compiles a validated layout into a deterministic command sequence.
///
/// # Errors
///
/// Returns an error for an empty pane ID, a duplicate pane ID, or a focused
/// pane that is absent from the tree.
pub fn compile(layout: &Layout) -> Result<Plan, String> {
    let mut pane_ids = Vec::new();
    collect_panes(&layout.root, &mut pane_ids);
    let mut unique = HashSet::new();
    if pane_ids.iter().any(|id| id.is_empty()) {
        return Err("cannot rebuild a tree containing an empty pane ID".to_owned());
    }
    if pane_ids.iter().any(|id| !unique.insert(id.as_str())) {
        return Err("cannot rebuild a tree containing duplicate pane IDs".to_owned());
    }
    if !unique.contains(layout.focused_pane_id.as_str()) {
        return Err("cannot rebuild a tree whose focused pane is absent".to_owned());
    }

    let anchor_pane_id = pane_ids
        .first()
        .expect("a Node always contains at least one pane")
        .clone();
    let mut commands = Vec::new();
    if let Some(scratch_anchor) = pane_ids.get(1) {
        commands.push(Command::Zoom {
            pane_id: layout.focused_pane_id.clone(),
            zoomed: false,
        });
        commands.push(Command::OpenScratch {
            pane_id: scratch_anchor.clone(),
        });
        for pane_id in &pane_ids[2..] {
            commands.push(Command::Park {
                pane_id: pane_id.clone(),
                target_pane_id: scratch_anchor.clone(),
            });
        }
        plan_placements(&layout.root, &mut commands);
        plan_ratios(&layout.root, &mut Vec::new(), &mut commands);
    }
    commands.push(Command::Focus {
        pane_id: layout.focused_pane_id.clone(),
    });
    commands.push(Command::Zoom {
        pane_id: layout.focused_pane_id.clone(),
        zoomed: layout.zoomed,
    });

    Ok(Plan {
        anchor_pane_id,
        commands,
    })
}

pub(super) fn collect_panes(node: &Node, pane_ids: &mut Vec<String>) {
    match node {
        Node::Pane { id } => pane_ids.push(id.clone()),
        Node::Split { first, second, .. } => {
            collect_panes(first, pane_ids);
            collect_panes(second, pane_ids);
        }
    }
}

fn first_pane(node: &Node) -> &str {
    match node {
        Node::Pane { id } => id,
        Node::Split { first, .. } => first_pane(first),
    }
}

fn plan_placements(node: &Node, commands: &mut Vec<Command>) {
    let Node::Split {
        direction,
        ratio,
        first,
        second,
    } = node
    else {
        return;
    };
    commands.push(Command::Place {
        pane_id: first_pane(second).to_owned(),
        target_pane_id: first_pane(first).to_owned(),
        direction: *direction,
        ratio: *ratio,
    });
    plan_placements(first, commands);
    plan_placements(second, commands);
}

fn plan_ratios(node: &Node, path: &mut Vec<bool>, commands: &mut Vec<Command>) {
    let Node::Split {
        ratio,
        first,
        second,
        ..
    } = node
    else {
        return;
    };
    commands.push(Command::SetRatio {
        path: path.clone(),
        ratio: *ratio,
    });
    path.push(false);
    plan_ratios(first, path, commands);
    path.pop();
    path.push(true);
    plan_ratios(second, path, commands);
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_nested_tree_in_a_stable_order() {
        let layout = sample_layout();
        let plan = compile(&layout).unwrap();

        assert_eq!(plan.anchor_pane_id, "p1");
        assert_eq!(
            plan.commands,
            vec![
                Command::Zoom {
                    pane_id: "p2".into(),
                    zoomed: false,
                },
                Command::OpenScratch {
                    pane_id: "p2".into(),
                },
                Command::Park {
                    pane_id: "p3".into(),
                    target_pane_id: "p2".into(),
                },
                Command::Place {
                    pane_id: "p3".into(),
                    target_pane_id: "p1".into(),
                    direction: SplitDirection::Right,
                    ratio: ratio(0.6),
                },
                Command::Place {
                    pane_id: "p2".into(),
                    target_pane_id: "p1".into(),
                    direction: SplitDirection::Down,
                    ratio: ratio(0.4),
                },
                Command::SetRatio {
                    path: vec![],
                    ratio: ratio(0.6),
                },
                Command::SetRatio {
                    path: vec![false],
                    ratio: ratio(0.4),
                },
                Command::Focus {
                    pane_id: "p2".into(),
                },
                Command::Zoom {
                    pane_id: "p2".into(),
                    zoomed: true,
                },
            ]
        );
    }

    fn sample_layout() -> Layout {
        Layout {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            zoomed: true,
            focused_pane_id: "p2".into(),
            root: Node::Split {
                direction: SplitDirection::Right,
                ratio: ratio(0.6),
                first: Box::new(Node::Split {
                    direction: SplitDirection::Down,
                    ratio: ratio(0.4),
                    first: Box::new(Node::Pane { id: "p1".into() }),
                    second: Box::new(Node::Pane { id: "p2".into() }),
                }),
                second: Box::new(Node::Pane { id: "p3".into() }),
            },
        }
    }

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }
}
