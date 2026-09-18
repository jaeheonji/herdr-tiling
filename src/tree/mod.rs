//! Pure types for the BSP layout tree.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// A direction in which to find and move a pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl fmt::Display for Direction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        };
        formatter.write_str(name)
    }
}

/// The axis and ordering of a split in Herdr's exported layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Right,
    Down,
}

/// A finite split ratio supplied by Herdr.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Ratio(f32);

impl Ratio {
    /// Creates a ratio, rejecting values outside Herdr's `0.0..=1.0` range.
    pub fn new(value: f32) -> Result<Self, &'static str> {
        (value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(Self(value))
            .ok_or("split ratio must be finite and between 0 and 1")
    }

    /// Returns the underlying ratio.
    pub fn get(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Ratio {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A pane or binary split in the active tab.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Pane {
        /// Herdr's stable pane identifier.
        id: String,
    },
    Split {
        /// The split axis and second-child direction.
        direction: SplitDirection,
        /// The first child's share of the split.
        ratio: Ratio,
        /// The first child in layout order.
        first: Box<Node>,
        /// The second child in layout order.
        second: Box<Node>,
    },
}

/// A validated export of one Herdr tab.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Layout {
    /// Workspace containing the tab.
    pub workspace_id: String,
    /// Tab described by the tree.
    pub tab_id: String,
    /// Whether Herdr had the focused pane zoomed.
    pub zoomed: bool,
    /// Pane to focus after rebuilding.
    pub focused_pane_id: String,
    /// BSP root containing each pane exactly once.
    pub root: Node,
}

/// A pane rectangle in terminal cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
pub struct Rect {
    /// Horizontal offset from the layout origin.
    pub x: u16,
    /// Vertical offset from the layout origin.
    pub y: u16,
    /// Width in terminal cells.
    pub width: u16,
    /// Height in terminal cells.
    pub height: u16,
}

impl Rect {
    /// Matches Herdr's terminal-cell rounding at each split.
    pub(crate) fn split(self, direction: SplitDirection, ratio: Ratio) -> (Self, Self) {
        let mut first = self;
        let mut second = self;
        match direction {
            SplitDirection::Right => {
                first.width = (f32::from(self.width) * ratio.get()).round() as u16;
                second.x = self.x.saturating_add(first.width);
                second.width -= first.width;
            }
            SplitDirection::Down => {
                first.height = (f32::from(self.height) * ratio.get()).round() as u16;
                second.y = self.y.saturating_add(first.height);
                second.height -= first.height;
            }
        }
        (first, second)
    }
}

impl Node {
    /// Computes unzoomed leaf rectangles in tree order, including hidden panes.
    pub(crate) fn pane_rects(&self, area: Rect) -> Vec<(&str, Rect)> {
        match self {
            Self::Pane { id } => vec![(id, area)],
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_area, second_area) = area.split(*direction, *ratio);
                let mut panes = first.pane_rects(first_area);
                panes.extend(second.pane_rects(second_area));
                panes
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_rectangles_round_each_split_like_herdr() {
        let root = Node::Split {
            direction: SplitDirection::Right,
            ratio: Ratio::new(0.5).unwrap(),
            first: Box::new(Node::Pane { id: "p1".into() }),
            second: Box::new(Node::Split {
                direction: SplitDirection::Down,
                ratio: Ratio::new(0.5).unwrap(),
                first: Box::new(Node::Pane { id: "p2".into() }),
                second: Box::new(Node::Pane { id: "p3".into() }),
            }),
        };
        assert_eq!(
            root.pane_rects(Rect {
                x: 3,
                y: 7,
                width: 185,
                height: 93
            }),
            vec![
                (
                    "p1",
                    Rect {
                        x: 3,
                        y: 7,
                        width: 93,
                        height: 93
                    }
                ),
                (
                    "p2",
                    Rect {
                        x: 96,
                        y: 7,
                        width: 92,
                        height: 47
                    }
                ),
                (
                    "p3",
                    Rect {
                        x: 96,
                        y: 54,
                        width: 92,
                        height: 46
                    }
                ),
            ]
        );
    }

    #[test]
    fn ratio_requires_a_value_in_the_protocol_range() {
        assert_eq!(Ratio::new(0.5).map(Ratio::get), Ok(0.5));
        assert_eq!(Ratio::new(0.0).map(Ratio::get), Ok(0.0));
        assert_eq!(Ratio::new(1.0).map(Ratio::get), Ok(1.0));
        assert!(Ratio::new(-0.1).is_err());
        assert!(Ratio::new(1.1).is_err());
        assert!(Ratio::new(f32::NAN).is_err());
        assert!(Ratio::new(f32::INFINITY).is_err());
    }
}
