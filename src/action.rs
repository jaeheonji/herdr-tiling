use std::ffi::OsString;

use crate::{preset::Preset, tree::Direction};

const USAGE: &str = "\
usage: herdr-tiling <action>

actions:
  move-left | move-right | move-up | move-down | toggle-split
  even-horizontal | even-vertical | main-horizontal | main-vertical | tiled | cycle
  recover";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Move(Direction),
    ToggleSplit,
    Layout(Preset),
    Cycle,
    Recover,
}

impl Action {
    pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let command = args.next().ok_or_else(|| USAGE.to_owned())?;

        if args.next().is_some() {
            return Err(USAGE.to_owned());
        }

        match command.to_str() {
            Some("move-left") => Ok(Self::Move(Direction::Left)),
            Some("move-right") => Ok(Self::Move(Direction::Right)),
            Some("move-up") => Ok(Self::Move(Direction::Up)),
            Some("move-down") => Ok(Self::Move(Direction::Down)),
            Some("toggle-split") => Ok(Self::ToggleSplit),
            Some("even-horizontal") => Ok(Self::Layout(Preset::EvenHorizontal)),
            Some("even-vertical") => Ok(Self::Layout(Preset::EvenVertical)),
            Some("main-horizontal") => Ok(Self::Layout(Preset::MainHorizontal)),
            Some("main-vertical") => Ok(Self::Layout(Preset::MainVertical)),
            Some("tiled") => Ok(Self::Layout(Preset::Tiled)),
            Some("cycle") => Ok(Self::Cycle),
            Some("recover") => Ok(Self::Recover),
            _ => Err(USAGE.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_action_and_rejects_invalid_arguments() {
        let cases = [
            ("move-left", Action::Move(Direction::Left)),
            ("move-right", Action::Move(Direction::Right)),
            ("move-up", Action::Move(Direction::Up)),
            ("move-down", Action::Move(Direction::Down)),
            ("toggle-split", Action::ToggleSplit),
            ("even-horizontal", Action::Layout(Preset::EvenHorizontal)),
            ("even-vertical", Action::Layout(Preset::EvenVertical)),
            ("main-horizontal", Action::Layout(Preset::MainHorizontal)),
            ("main-vertical", Action::Layout(Preset::MainVertical)),
            ("tiled", Action::Layout(Preset::Tiled)),
            ("cycle", Action::Cycle),
            ("recover", Action::Recover),
        ];

        for (command, expected) in cases {
            assert_eq!(Action::parse([command.into()]), Ok(expected));
        }

        assert!(Action::parse([]).is_err());
        assert!(Action::parse(["unknown".into()]).is_err());
        assert!(Action::parse(["recover".into(), "extra".into()]).is_err());
    }
}
