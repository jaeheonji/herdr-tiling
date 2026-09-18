use std::{collections::HashSet, fs, path::Path};

use serde::Deserialize;

use crate::preset::Preset;

const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) cycle_layouts: Vec<Preset>,
    pub(crate) main_ratio: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cycle_layouts: Preset::ALL.to_vec(),
            main_ratio: 0.5,
        }
    }
}

impl Config {
    pub(crate) fn load(config_dir: &Path) -> Result<Self, String> {
        let path = config_dir.join(CONFIG_FILE);
        let config = match fs::read(&path) {
            Ok(contents) => serde_json::from_slice(&contents)
                .map_err(|error| format!("read layout config {}: {error}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                return Err(format!("read layout config {}: {error}", path.display()));
            }
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.cycle_layouts.is_empty() {
            return Err("layout config cycle_layouts must not be empty".to_owned());
        }
        let mut unique = HashSet::new();
        if self
            .cycle_layouts
            .iter()
            .any(|preset| !unique.insert(*preset))
        {
            return Err("layout config cycle_layouts must not contain duplicates".to_owned());
        }
        if !self.main_ratio.is_finite() || !(0.1..=0.9).contains(&self.main_ratio) {
            return Err("layout config main_ratio must be between 0.1 and 0.9".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    #[test]
    fn loads_defaults_and_validates_overrides() {
        let directory = test_directory();
        fs::create_dir_all(&directory).unwrap();
        let defaults = Config::load(&directory).unwrap();
        assert_eq!(defaults.cycle_layouts, Preset::ALL);
        assert_eq!(defaults.main_ratio, 0.5);

        let path = directory.join(CONFIG_FILE);
        fs::write(
            &path,
            br#"{"cycle_layouts":["tiled","even-horizontal"],"main_ratio":0.7}"#,
        )
        .unwrap();
        let config = Config::load(&directory).unwrap();
        assert_eq!(
            config.cycle_layouts,
            [Preset::Tiled, Preset::EvenHorizontal]
        );
        assert_eq!(config.main_ratio, 0.7);

        for invalid in [
            "not json",
            r#"{"cycle_layouts":[]}"#,
            r#"{"cycle_layouts":["tiled","tiled"]}"#,
            r#"{"main_ratio":1.0}"#,
            r#"{"unknown":true}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(Config::load(&directory).is_err(), "{invalid}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    fn test_directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "herdr-tiling-config-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
