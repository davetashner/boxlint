# boxlint

A linter and auto-fixer for Unicode box-drawing diagrams.

[![CI](https://github.com/davetashner/boxlint/actions/workflows/ci.yml/badge.svg)](https://github.com/davetashner/boxlint/actions/workflows/ci.yml)

## Usage

```bash
boxlint lint diagram.txt       # Print diagnostics
boxlint fix diagram.txt        # Auto-fix to stdout
boxlint fix diagram.txt -i     # Auto-fix in place
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
