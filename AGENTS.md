# Agent Instructions

## Project Overview

**boxlint** is a Rust CLI that lints and auto-fixes Unicode box-drawing diagrams. It detects misaligned corners, broken edges, overflowing text, and disconnected arrows.

- Language: Rust (edition 2021, MSRV 1.75)
- Build: `cargo build --all-targets`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt --all --check`

## CI Requirements — Read Before Committing

Pull requests are required. All CI jobs must pass before merge. Run these locally before pushing:

```bash
cargo build --all-targets
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

### Commit Messages

Every commit must follow **conventional commits** format. CI rejects non-conforming messages on PRs.

```
<type>(<optional scope>): <description>

Valid types: feat, fix, chore, docs, test, refactor, style, perf, ci, build, revert
```

Examples:
- `feat: add box detection parser`
- `fix(linter): correct off-by-one in corner alignment check`
- `test: add edge cases for nested box detection`
- `chore: update dependencies`

### Clippy

CI runs `cargo clippy -- -D warnings` — all warnings are errors. Common pitfalls:
- Use `if let` instead of single-arm `match`
- Avoid `clone()` when a reference suffices
- Use `unwrap_or_else` instead of `unwrap_or` with expensive defaults
- Prefer `is_empty()` over `len() == 0`
- Don't leave unused imports, variables, or dead code

### Formatting

CI runs `cargo fmt --all --check`. Do not mix formatting changes with functional changes. If reformatting is needed, make it a separate `style:` commit.

### MSRV Compatibility

Code must compile on Rust 1.75. Avoid features stabilized after 1.75. If unsure, check the [Rust release notes](https://releases.rs/).

## Workflow

### Branch Strategy

Always work on a feature branch, never commit directly to `main`.

```bash
git checkout -b feat/my-feature origin/main
# ... work ...
git push -u origin feat/my-feature
gh pr create
```

### Issue Tracking (beads)

This project uses **bd** (beads) for issue tracking.

```bash
bd ready                              # Find available work
bd show <id>                          # View issue details
bd update <id> --status in_progress   # Claim work
bd close <id> --reason "..."          # Complete work
bd sync                               # Sync with git
```

Reference beads issue IDs in commit messages: `feat: add parser [boxlint-9dc]`

### Pull Requests

All changes go through PRs. Your job is not done until CI passes and the PR is merged.

1. Create a PR with `gh pr create`
2. Wait for CI — check with `gh pr checks <number>`
3. If CI fails, fix and push again. CI must pass.
4. Merge with `gh pr merge <number> --squash --delete-branch`
5. Clean up the local branch: `git checkout main && git pull && git branch -d <branch>`

Never leave stale PRs or branches. If a PR is abandoned, close it and delete the branch.

### Secrets

**NEVER commit secrets to git.** No API keys, tokens, passwords, or credentials in any file, ever. Check for secrets before every push. Use environment variables or external secret stores.

### Session Completion

When ending a work session, you MUST complete ALL steps below. Work is NOT complete until PRs are merged and branches are cleaned up.

1. **Run quality gates** — `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
2. **File issues** for remaining work with `bd create`
3. **Update issue status** — close finished work, update in-progress items
4. **Push, wait for CI, merge** — do not leave open PRs behind
5. **Clean up** — delete merged branches locally and remotely
6. **Verify** — `git status` on main, up to date with origin, no stale branches

## Project Structure

```
src/
  main.rs       # CLI entry point
  parser.rs     # Grid model and diagram IR (planned)
  linter.rs     # Lint rule engine (planned)
  fixer.rs      # Auto-fix engine (planned)
examples/
  demo-flow-diagram.txt   # Reference test diagram
.beads/                   # Issue tracking database
.github/workflows/ci.yml  # CI pipeline
```
