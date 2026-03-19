# prm

`prm` is a terminal-first project repository manager built with `ratatui`.
It helps you keep a local index of repositories, manage project TODOs, inspect repo git state, and navigate repo workflows quickly.

## Usage

```bash
# Add the current directory (or pass any path)
prm add .

# Open the TUI
prm
```

## Requirements

- Rust `1.88.0` (if using cargo directly)
- `git` (used for git status/history/release metadata)
- Optional: `lazygit` (for the `g` shortcut)
- Optional: `tmux` (for popup/window integrations)
- Optional: `nix` with flakes enabled

Path behavior:
- Added paths are canonicalized.
- If you add a subdirectory inside a git repo, `prm` stores the git repo root.

## Installation

### Run with Cargo

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

### Install With Homebrew

Install directly from the custom tap:

```bash
brew install bencetotht/prm/prm
```

Or add the tap first and then install:

```bash
brew tap bencetotht/prm
brew install prm
```

### Install From the AUR


Using an aur manager like `yay`:

```bash
yay -S prman-bin
```

Source package also available as `prman`:

```bash
yay -S prman
```

Both AUR packages install the executable as:

```bash
prm
```

### Install From GitHub Releases

Download the asset that matches your platform from the GitHub Releases page.
Current example:

<!-- release-download-example:start -->
```bash
curl -fsSL https://github.com/bencetotht/prm/releases/download/v1.0.2/prm-v1.0.2-aarch64-apple-darwin.tar.gz -o prm.tar.gz
tar -xzf prm.tar.gz
install "./prm-1.0.2-aarch64-apple-darwin/prm" /usr/local/bin/prm
```
<!-- release-download-example:end -->

Release artifacts:

<!-- release-assets:start -->
- `prm-v1.0.2-x86_64-unknown-linux-gnu.tar.gz`
- `prm-v1.0.2-x86_64-apple-darwin.tar.gz`
- `prm-v1.0.2-aarch64-apple-darwin.tar.gz`
- `prm-v1.0.2-checksums.txt`
<!-- release-assets:end -->

### Install via Nix flake
add the following to your `flake.nix`:

```nix
{
  inputs.prm = {
      url = "github:bencetotht/prm";
      inputs.nixpkgs.follows = "nixpkgs";
    };
}
```

and then install by adding this into your system packages:

```nix
inputs.prm.packages.${pkgs.stdenv.hostPlatform.system}.default
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

The main TUI is built on `crossterm` + `ratatui`, so it is not tied to Ghostty. It should behave the same in Kitty, Ghostty, WezTerm, Alacritty, and other terminals.

Theme note:

- `prm` does not query or switch terminal themes directly.
- Instead, it follows the terminal's active ANSI palette and default foreground/background, which is the most portable way for a TUI to track your current color scheme.
- This means a Vim/Neovim colorscheme only affects `prm` indirectly when your terminal palette matches that broader theme setup.
- If your terminal renders colors poorly, set `PRM_THEME=mono` to force monochrome attributes.
- `NO_COLOR=1` or `CLICOLOR=0` also force monochrome mode; `PRM_THEME=color` overrides that.

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

## Data and Configuration

- Default database location is platform data dir under `prm/prm.db`.
  - Example on Linux: `~/.local/share/prm/prm.db`
  - Example on macOS: `~/Library/Application Support/prm/prm.db`
- Override DB path with:

```bash
export PRM_DB_PATH=/custom/path/prm.db
```

## Contributing
Contributions are welcome, but please open an issue first to discuss your proposed changes and ensure they align with the project goals.

## License

Dual-licensed under:

- MIT ([LICENSE-MIT](LICENSE-MIT))
- Apache 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
