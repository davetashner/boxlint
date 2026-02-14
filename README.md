# boxlint

A linter and auto-fixer for Unicode box-drawing diagrams.

[![CI](https://github.com/davetashner/boxlint/actions/workflows/ci.yml/badge.svg)](https://github.com/davetashner/boxlint/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/davetashner/boxlint/graph/badge.svg)](https://codecov.io/gh/davetashner/boxlint)

## Usage

```bash
boxlint lint diagram.txt       # Print diagnostics
boxlint fix diagram.txt        # Auto-fix to stdout
boxlint fix diagram.txt -i     # Auto-fix in place
```

## Example

**Before** — a diagram with alignment issues:

```
┌──────────┐
│  Service │
└──────────┘
       │
       ▼
  ┌────────────┐     ┌──────────┐
  │  Database   │────►│  Cache  │
  └────────────┘     └──────────┘
```

Problems:
- The arrow from "Service" is not centered under the box
- "Database" box edge widths don't match the content
- The horizontal arrow between boxes is misaligned vertically

**After** — the same diagram, corrected:

```
┌──────────┐
│  Service │
└────┬─────┘
     │
     ▼
┌──────────┐     ┌─────────┐
│ Database │────►│  Cache  │
└──────────┘     └─────────┘
```

## What it checks

- **Box corners and edges** — mismatched corner characters (`┌` with `╗`), gaps in edges
- **Box content alignment** — text overflowing box boundaries, inconsistent alignment
- **Arrow connections** — arrows that don't connect to box edges, misaligned segments

## Building

```bash
cargo build
cargo test
```

## License

MIT
