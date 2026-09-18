mod layout;
mod move_window;
mod recover;
mod toggle_split;

use crate::action::Action;

pub(crate) fn run(action: Action) -> Result<(), String> {
    match action {
        Action::Move(direction) => move_window::run(direction),
        Action::ToggleSplit => toggle_split::run(),
        Action::Layout(preset) => layout::run(Some(preset)),
        Action::Cycle => layout::run(None),
        Action::Recover => recover::run(),
    }
}
