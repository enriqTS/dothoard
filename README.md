# dothoard

Automatically back up your dotfiles to a Git repository.

dothoard watches selected files and directories under `$HOME`, mirrors them
into a dedicated Git repo, and commits + pushes on a schedule — all without
interactive prompts. A Ratatui TUI lets you configure sources, preview
changes, and monitor status.

## Status

Work in progress. The headless backend (backup, check, Git sync) is functional.
The TUI and systemd automation are next.

## How it works

1. You point dothoard at a dedicated Git clone and list the paths you want
   backed up (e.g. `.config/fish`, `.config/hypr`, `.bashrc`).
2. A `systemd --user` timer runs `dothoard backup` after login and every few
   minutes (configurable).
3. Each run mirrors source files into the repo's `home/` directory, commits
   the diff, and pushes to the remote.

No symlinks are followed. No files outside the managed namespace are touched.
Failed runs are retried automatically; desktop notifications alert you only
when something breaks (and again when it recovers).

## Commands

```
dothoard                 Open the TUI
dothoard backup          Run one backup immediately
dothoard check           Validate configuration and repository
dothoard service install Install and enable the user timer
dothoard service remove  Disable and remove the user timer
dothoard service status  Show automation status
```

## Configuration

```toml
# ~/.config/dothoard/config.toml

version = 1
repository = "~/dotfiles"
remote = "origin"
interval_minutes = 5

[[sources]]
path = ".config/fish"
ignore = ["fish_variables", "*.log"]

[[sources]]
path = ".config/hypr"

[[sources]]
path = ".bashrc"
```

## Building

Requires Rust (stable toolchain).

```bash
cargo build --release
```

The binary is at `target/release/dothoard`.

## Design principles

- **Backup only** — restoring is a manual `git checkout` for now.
- **No shell interpolation** — all external commands use direct argument arrays.
- **Noninteractive Git** — no prompts, no credential popups; works unattended.
- **Safety boundaries** — never follows symlinks during traversal, never touches
  files outside the repo's managed namespace, never stages unmanaged paths.
- **Least surprise** — identical inputs produce identical plans; the planner is
  stateless and deterministic.

## Targets

CachyOS and Arch Linux. Other systemd-based distros likely work but are
untested.

## License

TBD
