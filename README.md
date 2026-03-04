# prm

`prm` is a terminal-first project repository manager built with `ratatui`.
It helps you keep a local index of repositories, manage project TODOs, inspect repo git state, and jump into repo workflows quickly from one TUI.

## Requirements

- Rust `1.88.0` (if using cargo directly)
- `git` (used for git status/history/release metadata)
- Optional: `lazygit` (for the `g` shortcut)
- Optional: `tmux` (for popup/window integrations)
- Optional: `nix` with flakes enabled

## Quick Start

```bash
# Add the current directory (or pass any path)
prm add .

# Open the TUI
prm
```

Path behavior:
- Added paths are canonicalized.
- If you add a subdirectory inside a git repo, `prm` stores the git repo root.

## Run With Cargo

Run without installing:

```bash
cargo run -- add /path/to/repo
cargo run --
```

Install globally from this repository:

```bash
cargo install --locked --path .
```

Make sure to add the `~/.cargo/bin` to your PATH!
```bash
export PATH="~/.cargo/bin:$PATH"
```

## Run With Nix

Run directly from this repository:

```bash
nix run . -- add /path/to/repo
nix run .
```

Install into your profile:

```bash
nix profile install .
```

Install directly from GitHub:

```bash
nix profile install github:bencetotht/prm
```

## Development

Using cargo directly:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

Using nix dev shell:

```bash
nix develop
cargo run --
```

Nix CI-equivalent checks:

```bash
nix flake check
```

## Features

- Project registry:
  - Add repositories from CLI (`prm add`) or from the TUI (`a` in Projects pane)
  - Rename, archive/unarchive, delete, and filter projects
- Git-aware project list:
  - Per-project git badges (`CHG`, `PUSH`, `COMMIT`, `OK`, `BEHIND`, `DIVERGED`, etc.)
  - Latest reachable tag shown in the project row
- Todo management:
  - Per-project TODO list with add/edit/toggle/reorder/delete
  - Supports two storage modes per project:
    - `db` mode (default): todos in `prm` SQLite DB
    - `markdown` mode: todos in `<project>/TODO.md` using markdown checkbox lines
  - Switch storage mode with `m` in the Projects pane
  - Note: switching mode does not migrate todos between DB and `TODO.md`; each source is managed independently
- Local TODO.md integration:
  - In markdown mode, edits are written directly to the repository `TODO.md`
  - External `TODO.md` edits are reflected after refresh/reload
- AGENTS.md viewer:
  - Shows `<project>/AGENTS.md` content in a dedicated pane
- Git history pane:
  - Shows recent commits and release distance from nearest tag
- External tool shortcuts:
  - `g` opens `lazygit` for selected project
  - `t` opens a new terminal context for selected project via tmux window
- Terminal-adaptive UI theming:
  - `prm` avoids hard-coded background colors and leans on your terminal's default foreground/background
  - Selection and focus are primarily expressed with `reverse`, `bold`, `dim`, and underline attributes
  - Git badges still use the standard ANSI palette so they inherit your terminal theme's color definitions
- Auto-refresh behavior:
  - Git status/history refresh every 60 seconds
  - Database external-change detection every 2 seconds
  - Manual full refresh with `f`

### Terminal Compatibility

The main TUI is built on `crossterm` + `ratatui`, so it is not tied to Ghostty. It should behave the same in Kitty, Ghostty, WezTerm, Alacritty, and other terminals with normal alternate-screen, mouse, and ANSI support.

Theme note:

- `prm` does not query or switch terminal themes directly.
- Instead, it follows the terminal's active ANSI palette and default foreground/background, which is the most portable way for a TUI to track your current color scheme.
- This means a Vim/Neovim colorscheme only affects `prm` indirectly when your terminal palette matches that broader theme setup.

### tmux Notes

Some features rely on tmux and may not behave as expected outside tmux:

- `t` (open terminal window) requires an active tmux session.
- `g` (lazygit) prefers a tmux popup when running inside tmux; without tmux it falls back to fullscreen lazygit.
- If tmux popup fails, `prm` falls back to fullscreen lazygit.

## Hotkey Guide

Global:

| Key | Action |
| --- | --- |
| `Q` | Quit `prm` |
| `?` | Toggle help dialog |
| `/` | Open project filter input |
| `f` | Fetch now (refresh DB + git + pane caches) |
| `g` | Open `lazygit` for selected project |
| `t` | Open tmux terminal window for selected project |
| `Tab` / `Shift+Tab` | Cycle pane focus forward/backward |
| `h` / `l` | Cycle pane focus left/right |
| `Left` / `Right` | Cycle pane focus |
| `1` / `2` / `3` / `4` | Focus Projects / Todos / AGENTS.md / Git history pane |
| `j` / `k` or `Up` / `Down` | Move selection (lists) or scroll (text panes) |
| Mouse click | Focus/select item in pane under cursor |
| Mouse wheel | Scroll pane under cursor |

Projects pane:

| Key | Action |
| --- | --- |
| `a` | Add project modal (path + optional name) |
| `r` | Rename selected project |
| `x` | Archive/unarchive selected project |
| `d` | Delete selected project (with confirmation modal) |
| `A` | Toggle showing archived projects |
| `m` | Toggle todo storage mode (`db` / `TODO.md`) |

Todos pane:

| Key | Action |
| --- | --- |
| `n` | Add todo |
| `e` or `Enter` | Edit selected todo |
| `Space` | Toggle done state |
| `d` then `d` | Delete selected todo (double press safety) |
| `J` / `K` | Reorder selected todo down/up |

Filter mode:

| Key | Action |
| --- | --- |
| Type text | Live filter projects by name/path |
| `Backspace` | Remove filter characters |
| `Enter` | Apply and close filter mode |
| `Esc` or `q` | Close filter mode |

Help dialog:

| Key | Action |
| --- | --- |
| `Esc`, `q`, or `?` | Close help dialog |

Modal dialogs:

| Key | Action |
| --- | --- |
| `Enter` | Submit current modal |
| `Esc` | Cancel modal |
| In confirm modal: `y` / `n` | Confirm / cancel |
| In add-project modal: `Tab` | Switch active field |

## Data & Configuration

- Default database location is platform data dir under `prm/prm.db`.
  - Example on Linux: `~/.local/share/prm/prm.db`
  - Example on macOS: `~/Library/Application Support/prm/prm.db`
- Override DB path with:

```bash
export PRM_DB_PATH=/custom/path/prm.db
```

## Contributing

1. Fork and create a feature branch.
2. Run:
   - `cargo fmt --all -- --check`
   - `cargo clippy --locked --all-targets --all-features -- -D warnings`
   - `cargo test --locked --all-targets`
3. Add/adjust tests for behavior changes.
4. Open a pull request.

## Release Flow

1. Create and push a semver tag like `v0.1.0`.
2. GitHub Actions builds release binaries for Linux and macOS targets.
3. Artifacts are attached to the GitHub Release.

## License

Dual-licensed under:

- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

You may choose either license.
