# Installing rug

## Prerequisites

- [Rust toolchain](https://rustup.rs/) (for building from source)
- [GitHub CLI (`gh`)](https://cli.github.com/) — must be installed and authenticated

Verify gh is set up:
```bash
gh auth status
```

## Install from source

```bash
cargo install --git https://github.com/<owner>/review-until-green
```

This installs the `rug` binary to `~/.cargo/bin/`.

## Install the Claude Code skill

Copy the skill file to your Claude Code skills directory:

```bash
cp skill/review-until-green.md ~/.claude/skills/
```

Or, if you use a project-level skills directory:

```bash
mkdir -p .claude/skills
cp skill/review-until-green.md .claude/skills/
```

## Verify

```bash
rug --help
```

## Usage

From a branch with an open PR:

```
/review-until-green
```

Or with a specific PR:

```
/review-until-green https://github.com/owner/repo/pull/123
```

## Configuration (optional)

Create `rug.toml` in your repo root:

```toml
# Only fix comments from specific authors (default: all)
# review_bots = ["devin-ai[bot]"]

# Seconds to wait after checks settle (default: 60)
settle_window = 60

# Max fix loops (default: 5)
max_loops = 5
```

Add `.rug/` to your `.gitignore` — it stores local state for delta tracking.
