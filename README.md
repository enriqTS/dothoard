# dothoard

Automatically back up your dotfiles to a Git repository.

dothoard watches selected files and directories under `$HOME`, mirrors them
into a dedicated Git repo, and commits + pushes on a schedule — all without
interactive prompts. A Ratatui TUI lets you configure sources, preview
changes, and monitor status.

## Features

- Mirror selected paths from `$HOME` into a dedicated Git repository.
- Scheduled backups via `systemd --user` timer (default: every 5 minutes).
- Noninteractive Git commits and pushes — works unattended after login.
- Per-source `.gitignore`-style ignore rules.
- Desktop notifications on failure and recovery.
- Interactive TUI for configuration, preview, and monitoring.
- Secret detection warnings for private keys, credentials, and tokens.
- Safe by design: never follows symlinks, never touches unmanaged files.

## Requirements

- **OS:** CachyOS, Arch Linux, or any systemd-based Linux distribution.
- **Rust:** stable toolchain (1.85+).
- **Git:** installed and accessible in `$PATH`.
- **systemd:** user session support (`systemd --user`).
- **notify-send** (optional): for desktop failure notifications.

## Installation

### From source (recommended)

Install the Rust toolchain if you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then build and install dothoard:

```bash
cargo install --path .
```

Or build a release binary manually:

```bash
cargo build --release
cp target/release/dothoard ~/.local/bin/
```

### From a Git clone

```bash
git clone https://github.com/henrique/dothoard.git
cd dothoard
cargo install --path .
```

Verify the installation:

```bash
dothoard --version
dothoard --help
```

## Quick Start

### 1. Prepare a dedicated Git repository

dothoard needs a dedicated clone — it won't share a repo with other projects.

```bash
mkdir ~/dotfiles
cd ~/dotfiles
git init
git remote add origin git@github.com:you/dotfiles.git
```

Or clone an existing one:

```bash
git clone git@github.com:you/dotfiles.git ~/dotfiles
```

### 2. Create the configuration

Create `~/.config/dothoard/config.toml`:

```toml
version = 1
repository = "~/dotfiles"
remote = "origin"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = [
  "fish_variables",
  "*.log",
]

[[sources]]
path = ".config/hypr"
ignore = [
  "*token*",
  "*.cache",
]

[[sources]]
path = ".bashrc"
```

### 3. Validate and run your first backup

```bash
# Check that configuration and repository are valid
dothoard check

# Run one backup manually
dothoard backup
```

### 4. Install the systemd timer

```bash
dothoard service install
```

This creates and enables `dothoard-backup.timer` and `dothoard-backup.service`
under `~/.config/systemd/user/`. The timer fires 1 minute after login and then
every `interval_minutes` (default: 5) after each completed run.

### 5. Open the TUI

```bash
dothoard
```

The TUI provides tabs for: Dashboard, Repository, Sources, Ignore, Preview,
Automation, and History.

## Commands

```
dothoard                 Open the TUI
dothoard backup          Run one backup immediately
dothoard check           Validate configuration and repository
dothoard service install Install and enable the user timer
dothoard service remove  Disable and remove the user timer
dothoard service status  Show automation status
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0    | Success (or no changes needed) |
| 1    | Backup or operation failed |
| 2    | Another backup is already running |
| 3    | Configuration is invalid or missing |

## Configuration Reference

The configuration file lives at `~/.config/dothoard/config.toml`.

```toml
# Schema version (required, must be 1)
version = 1

# Path to the dedicated Git repository (required)
# Supports ~ expansion
repository = "~/dotfiles"

# Git remote name for push/pull (default: "origin")
remote = "origin"

# Minutes between scheduled backups (default: 5, minimum: 1)
interval_minutes = 5

# Timeout in seconds for network Git operations (default: 120)
network_timeout_seconds = 120

# One or more source entries
[[sources]]
# Path relative to $HOME (required)
path = ".config/fish"
# Optional ignore patterns (gitignore syntax, rooted at the source)
ignore = [
  "fish_variables",
  "*.log",
  "cache/",
]
```

### Source path rules

- Paths are relative to `$HOME` — absolute paths are rejected.
- Parent traversal (`..`) is not allowed.
- Symlinks in parent components between `$HOME` and the source are rejected.
- A source root that is itself a symlink is preserved as a link (not followed).
- Sources must not overlap each other.
- Sources must not contain or be contained by the repository.

### Ignore pattern syntax

Patterns follow `.gitignore` semantics, rooted at the configured source:

- `*.log` — match files ending in `.log` at any depth.
- `/build` — match only `build` at the source root.
- `cache/` — match directories named `cache`.
- `!important.log` — re-include a previously ignored file.
- `\#comment` — escape a leading `#`.

The last matching rule wins. A child cannot be re-included while its parent
directory remains excluded. Nested `.git` directories and special files
(sockets, devices, FIFOs) are always excluded and cannot be negated.

## Systemd Automation

### Install and enable

```bash
dothoard service install
```

This is idempotent — safe to run multiple times. It will:
1. Generate `dothoard-backup.service` and `dothoard-backup.timer`.
2. Write them atomically to `~/.config/systemd/user/`.
3. Run `systemctl --user daemon-reload`.
4. Enable and start the timer.

### Check status

```bash
dothoard service status

# Or directly via systemctl:
systemctl --user status dothoard-backup.timer
systemctl --user status dothoard-backup.service
```

### View logs

```bash
journalctl --user -u dothoard-backup.service -f
```

### Remove

```bash
dothoard service remove
```

### Timer behavior

- Fires 1 minute after user-manager startup (first login).
- Fires again `interval_minutes` after each completed backup.
- Does not fire per-login if the user manager is already running.
- The service has a timeout = `network_timeout_seconds` + 60s as a safeguard.

### Updating the interval

Change `interval_minutes` in your config and reinstall:

```bash
dothoard service install
```

Or use the Automation tab in the TUI — it regenerates and restarts the timer
automatically.

## Repository Layout

```
repository/
|-- home/
|   |-- .config/
|   |   |-- fish/
|   |   |   |-- config.fish
|   |   |   `-- functions/
|   |   `-- hypr/
|   |       `-- hyprland.conf
|   `-- .bashrc
`-- .dothoard-manifest.toml
```

The application owns the `home/` namespace and `.dothoard-manifest.toml`.
Everything else in the repository is untouched.

## Backup Workflow

Each `dothoard backup` run:

1. Acquires an exclusive lock (prevents concurrent runs).
2. Loads and validates the configuration.
3. Validates the Git repository (branch, remote, no rebase/merge in progress).
4. Rejects dirty unmanaged paths (staged/unstaged/untracked outside `home/`).
5. Mirrors each configured source into `repository/home/...`.
6. Updates the manifest.
7. Stages only managed paths using literal Git pathspecs.
8. Commits if anything changed (message: `backup(<hostname>): <timestamp>`).
9. Pulls with rebase from the remote.
10. Pushes local commits.
11. Persists the result for the TUI and notifications.

If any source or manifest step fails, no Git operations are performed for that
run. If the network is unavailable, the local commit is preserved and pushed
on the next successful run.

## Authentication

dothoard runs Git noninteractively — it will never prompt for passwords or
passphrases. You must configure credential access before the timer can push.

See [docs/authentication.md](docs/authentication.md) for complete setup
instructions covering SSH agents, HTTPS credential helpers, host keys, and
troubleshooting.

Quick check:

```bash
dothoard check
```

This verifies that `git ls-remote` succeeds against your configured remote
without any interaction.

## Building from Source

```bash
# Development build
cargo build

# Run tests
cargo test --all-targets --all-features

# Full quality check
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features

# Release build (optimized, stripped)
cargo build --release
```

## Environment Variables

| Variable | Effect |
|----------|--------|
| `RUST_LOG` | Controls log verbosity (e.g. `info`, `debug`, `dothoard=trace`) |
| `XDG_CONFIG_HOME` | Overrides config location (default: `~/.config`) |
| `XDG_STATE_HOME` | Overrides state location (default: `~/.local/state`) |
| `XDG_RUNTIME_DIR` | Location for the exclusive lock file |

## Targets

CachyOS and Arch Linux. Other systemd-based distros likely work but are
untested.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) for the full text.
