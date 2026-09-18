use std::ffi::OsString;

use crate::action::Action;

pub(crate) fn validate(action: &Action) -> Result<(), String> {
    validate_with(action, |name| std::env::var_os(name))
}

fn validate_with(
    action: &Action,
    mut lookup: impl FnMut(&str) -> Option<OsString>,
) -> Result<(), String> {
    require("HERDR_SOCKET_PATH", &mut lookup)?;
    require("HERDR_PLUGIN_STATE_DIR", &mut lookup)?;

    match action {
        Action::Recover => Ok(()),
        Action::Move(_) | Action::ToggleSplit => require("HERDR_PANE_ID", &mut lookup),
        Action::Layout(_) | Action::Cycle => {
            require("HERDR_TAB_ID", &mut lookup)?;
            require("HERDR_PLUGIN_CONFIG_DIR", &mut lookup)
        }
    }
}

fn require(name: &str, lookup: &mut impl FnMut(&str) -> Option<OsString>) -> Result<(), String> {
    match lookup(name) {
        Some(value) if !value.is_empty() => Ok(()),
        _ => Err(format!("required environment variable {name} is missing")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::tree::Direction;

    use super::*;

    #[test]
    fn validates_environment_for_pane_actions_and_recovery() {
        let pane = Action::Move(Direction::Left);
        let recover = Action::Recover;
        let layout = Action::Cycle;
        let values = HashMap::from([
            ("HERDR_SOCKET_PATH", OsString::from("/tmp/herdr.sock")),
            ("HERDR_PANE_ID", OsString::from("w1:p1")),
            ("HERDR_PLUGIN_STATE_DIR", OsString::from("/tmp/state")),
            ("HERDR_PLUGIN_CONFIG_DIR", OsString::from("/tmp/config")),
            ("HERDR_TAB_ID", OsString::from("w1:t1")),
        ]);

        assert!(validate_with(&pane, |name| values.get(name).cloned()).is_ok());
        assert!(validate_with(&recover, |name| values.get(name).cloned()).is_ok());
        assert!(validate_with(&layout, |name| values.get(name).cloned()).is_ok());
        assert!(validate_with(&pane, |_| None).is_err());
        assert!(
            validate_with(&pane, |name| match name {
                "HERDR_SOCKET_PATH" => Some("/tmp/s".into()),
                "HERDR_PANE_ID" => Some("w1:p1".into()),
                _ => None,
            })
            .is_err()
        );
        assert!(
            validate_with(&pane, |name| (name == "HERDR_SOCKET_PATH")
                .then(|| "/tmp/s".into()))
            .is_err()
        );
        assert!(
            validate_with(&recover, |name| (name == "HERDR_SOCKET_PATH")
                .then(|| "/tmp/s".into()))
            .is_err()
        );
    }
}
