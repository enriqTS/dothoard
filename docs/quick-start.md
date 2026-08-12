# Quick Start

This guide uses the TUI. It is the recommended first-run path because it lets
you review the repository, namespace, sources, and preview before a backup.

## Prerequisites

- CachyOS or Arch Linux (the currently supported platforms)
- Rust 1.97 or newer
- Git in `PATH`
- A working `systemd --user` session

Install from a source checkout:

```bash
cargo install --path .
dothoard --version
```

## 1. Prepare a dedicated repository

Use a repository that contains only dothoard-managed namespaces and any content
you deliberately maintain alongside them. Do not point dothoard at another
project's repository.

```bash
mkdir ~/dotfiles
cd ~/dotfiles
git init
git remote add origin git@github.com:you/dotfiles.git
```

Alternatively, clone an existing dedicated repository:

```bash
git clone git@github.com:you/dotfiles.git ~/dotfiles
```

Configure SSH or HTTPS credentials before enabling scheduled backups; see
[Authentication](authentication.md).

## 2. Configure in the TUI

Run:

```bash
dothoard
```

1. Open **Repository** and select the repository clone.
2. Create or select a namespace, for example `desktop`. A namespace identifies
   this machine's independent directory in the repository.
3. Open **Sources** and select regular files, directories, or source-root
   symlinks below `$HOME`.
4. Open **Ignore** to add source-relative Git-style exclusions when needed.
5. Open **Preview** and inspect the exact changes and warnings.
6. Start a manual backup only after the preview looks correct.

The TUI never follows a selected symlink's target. A source-root symlink is
stored as a link; a symlink in a source's parent path is rejected.

## 3. Enable automation

After a manual backup succeeds and `dothoard check` reports a ready remote:

```bash
dothoard check
dothoard service install
dothoard service status
```

The timer runs one minute after the user manager starts and again after each
configured interval (five minutes by default).

## Configuration-file alternative

Create `~/.config/dothoard/config.toml`:

```toml
version = 2
repository = "~/dotfiles"
remote = "origin"
namespace = "desktop"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = ["fish_variables", "*.log"]
```

Then validate and back up:

```bash
dothoard check
dothoard backup
```

Continue with the [configuration reference](configuration.md) and [TUI guide](tui.md).
