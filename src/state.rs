//! Local state management for delta tracking.
//!
//! Persists addressed comment IDs in `.rug/` so that subsequent
//! `rug status` calls only return new/unresolved comments.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// Tracked state for a single PR, persisted as JSON in `.rug/`.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct State {
    /// Comment IDs that have been addressed by the agent.
    pub addressed: HashSet<u64>,
    /// The head SHA when we last processed this PR.
    pub head_sha: Option<String>,
}

impl State {
    /// Path to the state file for a given PR key.
    fn state_path(rug_dir: &Path, key: &str) -> PathBuf {
        rug_dir.join(format!("{key}.json"))
    }

    /// Load state from disk, or return default if no state exists.
    pub fn load(rug_dir: &Path, key: &str) -> Result<Self> {
        let path = Self::state_path(rug_dir, key);
        if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read state file: {}", path.display()))?;
            let state: State = serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse state file: {}", path.display()))?;
            Ok(state)
        } else {
            Ok(State::default())
        }
    }

    /// Save state to disk, creating the .rug directory if needed.
    pub fn save(&self, rug_dir: &Path, key: &str) -> Result<()> {
        std::fs::create_dir_all(rug_dir)
            .with_context(|| format!("Failed to create directory: {}", rug_dir.display()))?;
        let path = Self::state_path(rug_dir, key);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write state file: {}", path.display()))?;
        Ok(())
    }

    /// Mark the given comment IDs as addressed.
    pub fn mark_addressed(&mut self, ids: &[u64]) {
        self.addressed.extend(ids);
    }

    /// Check if a comment ID has been addressed.
    pub fn is_addressed(&self, id: u64) -> bool {
        self.addressed.contains(&id)
    }

    /// Delete the state file for a given PR.
    pub fn delete(rug_dir: &Path, key: &str) -> Result<()> {
        let path = Self::state_path(rug_dir, key);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to delete state file: {}", path.display()))?;
        }
        Ok(())
    }
}

/// Get the .rug directory path (in the current working directory).
pub fn rug_dir() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok(cwd.join(".rug"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_state() {
        let state = State::default();
        assert!(state.addressed.is_empty());
        assert!(state.head_sha.is_none());
    }

    #[test]
    fn test_load_missing() {
        let dir = tempdir().unwrap();
        let state = State::load(dir.path(), "test-pr").unwrap();
        assert!(state.addressed.is_empty());
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let mut state = State::default();
        state.mark_addressed(&[1001, 1002]);
        state.head_sha = Some("abc123".to_string());
        state.save(dir.path(), "test-pr").unwrap();

        let loaded = State::load(dir.path(), "test-pr").unwrap();
        assert!(loaded.is_addressed(1001));
        assert!(loaded.is_addressed(1002));
        assert!(!loaded.is_addressed(9999));
        assert_eq!(loaded.head_sha.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_mark_addressed_additive() {
        let dir = tempdir().unwrap();
        let mut state = State::default();
        state.mark_addressed(&[1001]);
        state.save(dir.path(), "test-pr").unwrap();

        let mut state = State::load(dir.path(), "test-pr").unwrap();
        state.mark_addressed(&[1002]);
        state.save(dir.path(), "test-pr").unwrap();

        let loaded = State::load(dir.path(), "test-pr").unwrap();
        assert!(loaded.is_addressed(1001));
        assert!(loaded.is_addressed(1002));
    }

    #[test]
    fn test_delete() {
        let dir = tempdir().unwrap();
        let state = State::default();
        state.save(dir.path(), "test-pr").unwrap();
        assert!(dir.path().join("test-pr.json").exists());

        State::delete(dir.path(), "test-pr").unwrap();
        assert!(!dir.path().join("test-pr.json").exists());
    }

    #[test]
    fn test_delete_missing() {
        let dir = tempdir().unwrap();
        State::delete(dir.path(), "nonexistent").unwrap();
    }
}
