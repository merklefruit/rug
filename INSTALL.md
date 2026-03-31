# Installing review-until-green

Two things to install: the `rug` CLI and the `/review-until-green` skill.

## Prerequisites

- [Rust toolchain](https://rustup.rs/)
- [GitHub CLI (`gh`)](https://cli.github.com/), authenticated (`gh auth login`)

## 1. Install the CLI

```bash
cargo install --git https://github.com/merklefruit/review-until-green
```

Verify: `rug --help`

## 2. Install the skill

### Via skills.sh (recommended)

```bash
npx skills add merklefruit/review-until-green
```

### Manual (curl)

Global install (all projects):

```bash
mkdir -p ~/.claude/skills
curl -fsSL https://raw.githubusercontent.com/merklefruit/review-until-green/main/skills/review-until-green/SKILL.md \
  -o ~/.claude/skills/review-until-green.md
```

Project-level (current repo only):

```bash
mkdir -p .claude/skills
curl -fsSL https://raw.githubusercontent.com/merklefruit/review-until-green/main/skills/review-until-green/SKILL.md \
  -o .claude/skills/review-until-green.md
```

## 3. Add `.rug/` to your .gitignore

```bash
echo '.rug/' >> .gitignore
```

## Usage

```
/review-until-green
/review-until-green https://github.com/owner/repo/pull/123
```

## Configuration (optional)

Download the example config:

```bash
curl -fsSL https://raw.githubusercontent.com/merklefruit/review-until-green/main/rug.example.toml \
  -o rug.toml
```

Edit `rug.toml` to customize `review_bots`, `settle_window`, and `max_loops`.
