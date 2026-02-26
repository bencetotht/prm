# prm

`prm` is a ratatui-based terminal CLI for tracking and jumping between local project repositories.

## Install

### From source (global command)

```bash
cargo install --locked --path .
```

This installs `prm` to `~/.cargo/bin`. Ensure that directory is on your `PATH`.

### With Nix

From this repo:

```bash
nix profile install .
```

From GitHub:

```bash
nix profile install github:bencetotht/prm
```

After install, `prm` is available as a normal shell command.

## Usage

```bash
prm add /path/to/repo
prm
```

## Development

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
```

## Nix

```bash
nix build .
nix run .
```

## Release flow

1. Create a version tag like `v0.1.0`.
2. Push the tag.
3. GitHub Actions builds release binaries for Linux and macOS and uploads archives to the GitHub Release.
