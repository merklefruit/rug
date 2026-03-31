//! PR reference parsing and resolution.
//!
//! Supports GitHub PR URLs, short refs (`owner/repo#123`), and
//! automatic resolution from the current git branch via `gh pr view`.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::types::PrRef;

/// Parse a PR reference from various formats:
/// - https://github.com/owner/repo/pull/123
/// - owner/repo#123
pub fn parse_pr_ref(input: &str) -> Result<PrRef> {
    if let Some(pr) = parse_url(input) {
        return Ok(pr);
    }
    if let Some(pr) = parse_short_ref(input) {
        return Ok(pr);
    }
    bail!(
        "Cannot parse PR reference: {input}\nExpected: URL (https://github.com/owner/repo/pull/123) or owner/repo#123"
    )
}

fn parse_url(input: &str) -> Option<PrRef> {
    let input = input.trim().trim_end_matches('/');
    let parts: Vec<&str> = input.split('/').collect();
    if parts.len() >= 7 && parts[5] == "pull" {
        let number = parts[6].parse().ok()?;
        return Some(PrRef { owner: parts[3].to_string(), repo: parts[4].to_string(), number });
    }
    None
}

fn parse_short_ref(input: &str) -> Option<PrRef> {
    let (repo_part, num_part) = input.split_once('#')?;
    let (owner, repo) = repo_part.split_once('/')?;
    let number = num_part.parse().ok()?;
    Some(PrRef { owner: owner.to_string(), repo: repo.to_string(), number })
}

/// Resolve the PR for the current branch using `gh pr view`.
pub fn resolve_from_branch() -> Result<PrRef> {
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "number,url", "-q", ".url"])
        .output()
        .context("Failed to run `gh pr view`. Is gh CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("No PR found for current branch: {stderr}");
    }

    let url = String::from_utf8(output.stdout).context("Invalid UTF-8 from gh")?.trim().to_string();

    parse_pr_ref(&url)
}

/// Resolve a PrRef from user input or current branch.
pub fn resolve_pr(input: Option<&str>) -> Result<PrRef> {
    match input {
        Some(s) => parse_pr_ref(s),
        None => resolve_from_branch(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_url() {
        let pr = parse_pr_ref("https://github.com/octocat/hello-world/pull/42").unwrap();
        assert_eq!(pr.owner, "octocat");
        assert_eq!(pr.repo, "hello-world");
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn test_parse_full_url_trailing_slash() {
        let pr = parse_pr_ref("https://github.com/octocat/hello-world/pull/42/").unwrap();
        assert_eq!(pr.number, 42);
    }

    #[test]
    fn test_parse_short_ref() {
        let pr = parse_pr_ref("octocat/hello-world#99").unwrap();
        assert_eq!(pr.owner, "octocat");
        assert_eq!(pr.repo, "hello-world");
        assert_eq!(pr.number, 99);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_pr_ref("not-a-pr-ref").is_err());
    }

    #[test]
    fn test_parse_invalid_number() {
        assert!(parse_pr_ref("owner/repo#abc").is_err());
    }

    #[test]
    fn test_state_key() {
        let pr = parse_pr_ref("octocat/hello-world#42").unwrap();
        assert_eq!(pr.state_key(), "octocat-hello-world-42");
    }
}
