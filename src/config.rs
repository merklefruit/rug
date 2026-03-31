//! Configuration from `rug.toml`.

use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

/// Configuration loaded from an optional `rug.toml` file in the repo root.
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Only process comments from these authors. `None` means all comments are actionable.
    pub review_bots: Option<Vec<String>>,
    /// Seconds to wait after all checks settle before final status check (used by the skill).
    #[serde(default = "default_settle_window")]
    #[allow(dead_code)]
    pub settle_window: u64,
    /// Max fix loops before the skill should stop (used by the skill).
    #[serde(default = "default_max_loops")]
    #[allow(dead_code)]
    pub max_loops: u32,
}

fn default_settle_window() -> u64 {
    60
}

fn default_max_loops() -> u32 {
    5
}

impl Default for Config {
    fn default() -> Self {
        Self {
            review_bots: None,
            settle_window: default_settle_window(),
            max_loops: default_max_loops(),
        }
    }
}

impl Config {
    /// Load config from rug.toml in the given directory, or return defaults.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("rug.toml");
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.review_bots.is_none());
        assert_eq!(config.settle_window, 60);
        assert_eq!(config.max_loops, 5);
    }

    #[test]
    fn test_load_missing_file() {
        let dir = tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.review_bots.is_none());
        assert_eq!(config.settle_window, 60);
    }

    #[test]
    fn test_load_full_config() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("rug.toml"),
            r#"
review_bots = ["devin-ai[bot]", "cursor[bot]"]
settle_window = 30
max_loops = 10
"#,
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(
            config.review_bots.as_deref(),
            Some(&["devin-ai[bot]".to_string(), "cursor[bot]".to_string()][..])
        );
        assert_eq!(config.settle_window, 30);
        assert_eq!(config.max_loops, 10);
    }

    #[test]
    fn test_load_partial_config() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("rug.toml"), "settle_window = 120\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.review_bots.is_none());
        assert_eq!(config.settle_window, 120);
        assert_eq!(config.max_loops, 5);
    }
}
