# Release checklist

Muxboard V1 ships GitHub-first. Crates.io publishing stays disabled until we intentionally make that a separate release decision.

## Preflight

Run from a clean tree:

```bash
git status --short
just public-assets
just release-check
cargo package --locked --list
```

Review the package list for local-only files. It should include source, fixtures,
public docs, demo SVG/GIF assets, social-preview PNG, and license files. It should not
include `.hermes`, `target`, downloaded agent source drops, or local identity
files.

## Public repository

The current V1 release path is GitHub-first. Before any public push, tag, or release, verify the repo wiring without changing anything:

```bash
just github-preflight
```

That check intentionally fails until `origin` points at `aanari/muxboard`, the GitHub repo exists, the repo is public, and the default branch is available.

Keep the repository homepage pointed at GitHub unless the public Pages route has
been verified end to end. A green Pages build is not enough; owner-level custom
domains can redirect `github.io` project pages somewhere else.

Create the public repo only after the release check is green and the current branch is the branch you want as `main`:

```bash
gh repo create aanari/muxboard --public --source . --remote origin --push
```

If the repo already exists, attach it explicitly:

```bash
git remote add origin git@github.com:aanari/muxboard.git
git push -u origin main
```

## Install paths

Source install from the public repo:

```bash
cargo install --git https://github.com/aanari/muxboard --locked
```

Local install from a checkout:

```bash
cargo install --path . --locked
```

The binary needs `tmux` on the same host where `muxboard` runs.

## GitHub release

Tag only after `main` is pushed and green:

```bash
git tag -a v1.0.0 -m "muxboard 1.0.0"
git push origin v1.0.0
```

The tag workflow builds Linux and macOS binaries, uploads checksums, and creates or updates the GitHub release.

## Crates.io

`publish = false` is intentional for V1. Remove it only when we are ready to own the crate namespace and support `cargo install muxboard` as an official distribution path.
