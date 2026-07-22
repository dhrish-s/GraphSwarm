# Contributing to GraphSwarm

Thank you for considering contributing to GraphSwarm!

## Quick Start

```bash
git clone https://github.com/YOUR_USERNAME/graphswarm.git
cd graphswarm
git checkout -b feat/my-feature
cargo test
# make changes ...
git push origin feat/my-feature
# open a Pull Request
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` and fix all warnings
- Write doc comments on every public item
- Add tests for new functionality

## Commit Messages

Use imperative mood, 72 chars max first line:

```
Add Go parser support

- Implement tree-sitter Go bindings
- Add extraction for Go functions and types
- Write unit tests

Fixes #42
```

## Branch Naming

- `feat/description` - new feature
- `fix/description` - bug fix
- `docs/description` - documentation
- `refactor/description` - refactor

## Pull Request Checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo fmt` applied
- [ ] Doc comments on public APIs
- [ ] Related issue linked

## Areas We Need Help With

- Multi-language parser support (Go, Rust, TypeScript)
- Performance optimizations
- Better error messages and diagnostics
- Documentation and tutorials

## Questions?

Open a GitHub issue or start a discussion.
