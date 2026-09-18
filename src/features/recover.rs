use std::{env, path::PathBuf};

use crate::{herdr::client::Client, rebuild};

pub(super) fn run() -> Result<(), String> {
    let socket_path = env::var_os("HERDR_SOCKET_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| "required environment variable HERDR_SOCKET_PATH is missing".to_owned())?;
    let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "required environment variable HERDR_PLUGIN_STATE_DIR is missing".to_owned()
        })?;

    rebuild::recover(&mut Client::new(socket_path), &state_dir)
}
