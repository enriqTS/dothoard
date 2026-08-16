# Quick Start

This guide uses the TUI. It is the recommended first-run path because it lets
you review the repository, namespace, sources, and preview before a backup.

## Prerequisites

- CachyOS or Arch Linux (the currently supported platforms)
- Git in `PATH`
- A working `systemd --user` session, a compatible user `crontab`, or another externally configured scheduler
- Rust 1.97 or newer, only if building from source

## Install

Download a prebuilt binary from the latest [GitHub Release](https://github.com/enriqTS/dothoard/releases)
(pre-release; see [Experimental releases](releases.md)):

```bash
curl -fsSL https://raw.githubusercontent.com/enriqTS/dothoard/main/scripts/install.sh | sh
dothoard --version
```

The script installs to `~/.local/bin` by default; set `INSTALL_DIR` to change
that, or `VERSION` to pin a specific release tag. It only supports x86_64
Linux; other platforms must build from source.

Or build from a source checkout:

```bash
cargo install --path .
dothoard --version
```

## 1. Prepare a dedicated repository

Use a repository that contains only dothoard-managed namespaces and any content
you deliberately maintain alongside them. Do not point dothoard at another
project's repository. You may prepare a local clone yourself or let first-run
setup clone an existing remote into a new local path.

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

1. **Repository:** choose **Use an existing clone** to browse to a dedicated
   worktree, or **Clone from a Git URL** and enter both the remote URL and a new
   local destination. The destination's parent must already exist. Cloning and
   validation run in the background; authentication, transport, timeout,
   existing-destination, and repository-validation errors remain visible for
   correction or retry. Credential-bearing URLs are redacted from diagnostics.
2. **Namespace:** select an existing namespace or explicitly create one, for
   example `desktop`. Dothoard does not choose a default. Selecting an owned
   namespace restores its manifest's source paths and ignore rules.
3. **Automation:** choose systemd, cron, or external and set the interval in
   minutes. Cron accepts 1–59 minutes. This records the choice but does not
   install automation yet.
4. **Theme:** move through the complete list with Up/Down or `j`/`k`. Each
   highlighted theme applies immediately; press Enter to keep the highlighted
   theme and open the main tabs.
5. Open **Sources** and review the restored selections or select regular files,
   directories, or source-root symlinks below `$HOME` for a new namespace.
6. Open **Ignore** to add source-relative Git-style exclusions when needed.
7. Open **Preview** and inspect the exact changes and warnings.
8. Start a manual backup only after the preview looks correct.

If the application exits after namespace selection but before setup finishes,
the next launch resumes at Automation instead of exposing a partially completed
main interface.

The TUI never follows a selected symlink's target. A source-root symlink is
stored as a link; a symlink in a source's parent path is rejected.

## 3. Enable automation

After a manual backup succeeds and `dothoard check` reports a ready remote:

```bash
dothoard check
dothoard service install
dothoard service status
```

The default systemd timer runs one minute after the user manager starts and
again after each configured interval (five minutes by default). To use cron,
remove installed systemd automation first, then select and install cron:

```bash
dothoard service remove
dothoard service select cron
dothoard service install
dothoard service status
```

For another scheduler implementation, select the external backend and copy its
invocation into that scheduler yourself:

```bash
dothoard service remove
dothoard service select external
dothoard service print-command
```

You can also press `b` in the TUI's Automation screen while no selected managed
backend is installed. See [Backup automation](automation.md) for scheduler
environment, timing, and status limitations.

## Configuration-file alternative

Create `~/.config/dothoard/config.toml`:

```toml
version = 2
repository = "~/dotfiles"
remote = "origin"
namespace = "desktop"
interval_minutes = 5
automation_backend = "systemd"
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
