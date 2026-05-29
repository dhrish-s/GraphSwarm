# Pushing GraphSwarm to GitHub

## 1. Create the repo on GitHub

Go to https://github.com/new and create **graphswarm** (public, no template files — they're already here).

## 2. Initialize and push

```bash
cd graphswarm
git init
git add .
git commit -m "Initial project scaffolding"
git branch -M main
git remote add origin git@github.com:dhrish-s/graphswarm.git
git push -u origin main
```

## 3. Verify

Visit https://github.com/dhrish-s/graphswarm and confirm all files appear.

## 4. Optional: enable CI

Create `.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --verbose
      - run: cargo clippy -- -D warnings
      - run: cargo fmt -- --check
```
