# dothoard

> Safe, Git-native dotfile backups for Linux — unattended when you want them,
> reviewable when you need them.

[![Status: experimental](https://img.shields.io/badge/status-experimental-orange)](#project-status)
[![Rust CI](https://github.com/enriqTS/dothoard/actions/workflows/ci.yml/badge.svg)](https://github.com/enriqTS/dothoard/actions/workflows/ci.yml)
[![License: GPL--3.0--or--later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)

**[Documentation website](https://enriqts.github.io/dothoard/)**

![dothoard quick start walkthrough](assets/screenshots/quickstart.gif)

`dothoard` copies selected files and directories from your home directory into a
dedicated Git repository, then commits and pushes them on demand or through a
`systemd --user` timer. Its keyboard-first terminal UI configures sources,
ignore rules, namespaces, previews, and automation.

**It is for Linux users who want a focused backup and synchronization tool for
their dotfiles without giving up control of the Git repository.** It is not a
restore manager, cloud backup service, persistent daemon, or general-purpose
repository manager.

## Project status

**Experimental.** The backend is extensively tested, but the TUI usability and
visual-design work is still in progress. Review previews and use a dedicated
repository before relying on it for important data. Supported and manually
smoke-tested platforms are CachyOS and Arch Linux; other systemd-based Linux
distributions may work but are not yet supported.

## Why dothoard?

![dothoard Dashboard](assets/screenshots/dashboard.png)

- **Safe boundaries:** it never follows symlinks while traversing, and every
  write or deletion stays inside the active namespace in the repository.
- **Git-native:** changes are ordinary Git commits that you can inspect,
  review, and synchronize with your usual tools.
- **Multi-machine aware:** each computer owns a named namespace and cannot
  stage, alter, or clean a sibling namespace.
- **Unattended by design:** scheduled Git operations are noninteractive and
  timeout-bounded; failed network sync keeps the local commit for a later run.
- **Review before backup:** preview exact additions, changes, deletions,
  exclusions, and likely-secret warnings before a manual backup.

Read the complete [safety model](docs/safety.md) before use. Ignore rules and
secret warnings reduce risk, but a committed secret remains in Git history and
must be rotated.

## Five-minute quick start

1. **Install prerequisites:** Rust 1.97+, Git, and a systemd user session.
2. **Create or clone a dedicated Git repository**—do not share it with another
   project.
3. **Install dothoard and open the TUI:**

   ```bash
   cargo install --path .
   dothoard
   ```

4. In **Repository**, choose the clone and create/select a namespace such as
   `desktop`.
5. In **Sources**, select files or directories below `$HOME`; inspect **Preview**
   and run the first backup.
6. When the manual flow is working, install automation:

   ```bash
   dothoard service install
   ```

For commands, authentication, and a configuration-file workflow, see the
[quick-start guide](docs/quick-start.md).

![dothoard source browser](assets/screenshots/sources.png)

## Documentation

Visit the documentation website at
[enriqts.github.io/dothoard](https://enriqts.github.io/dothoard/) or use the
repository guides below.

| I want to… | Read |
|---|---|
| Install and make a first backup | [Quick start](docs/quick-start.md) |
| Use the terminal UI and keyboard controls | [TUI guide](docs/tui.md) |
| Edit the configuration file | [Configuration reference](docs/configuration.md) |
| Exclude files safely | [Ignore rules](docs/ignore-rules.md) |
| Use one repository from several machines | [Namespaces](docs/namespaces.md) |
| Configure SSH or HTTPS for unattended Git | [Authentication](docs/authentication.md) |
| Understand limits, safety, and recovery | [Safety model](docs/safety.md) |
| Solve a common problem | [Troubleshooting](docs/troubleshooting.md) and [FAQ](docs/faq.md) |
| Build or contribute | [Development](docs/development.md) and [Contributing](docs/contributing.md) |

## Commands

```text
dothoard                 Open the TUI
dothoard backup          Run one backup immediately
dothoard check           Validate configuration and repository
dothoard service install Install and enable the user timer
dothoard service remove  Disable and remove the user timer
dothoard service status  Show automation status
```

| Exit code | Meaning |
|---:|---|
| 0 | Success or no changes needed |
| 1 | Backup or operation failed |
| 2 | Another backup is already running |
| 3 | Configuration is missing or invalid |

## What a backup does

A run validates the configuration, repository, active namespace, and source
paths; blocks dirty changes outside that namespace; mirrors sources under
`<namespace>/home/`; updates the namespace manifest; stages only that
namespace; and commits, rebases, and pushes noninteractively. A source or
manifest failure prevents Git publication for that run. A network failure keeps
the local commit so a later run can push it.

![dothoard backup preview](assets/screenshots/preview.png)

## Repository layout

```text
repository/
|-- desktop/
|   |-- home/
|   |   `-- .config/...
|   `-- .dothoard-manifest.toml
|-- notebook/
|   |-- home/
|   `-- .dothoard-manifest.toml
`-- other repository content (untouched)
```

Each configured machine owns only its selected namespace. Root-level legacy
`home/` data and sibling namespaces are unmanaged and untouched. See
[Namespaces](docs/namespaces.md) for setup and lifecycle details.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features -- --test-threads=1
```

The test suite uses temporary directories and must not touch real dotfiles,
repositories, systemd user units, or desktop notifications. See the
[development guide](docs/development.md).

## Repository metadata

GitHub repository description, topics, and social-preview settings are managed
in the GitHub repository settings. Recommended description:

> Safe, Git-backed dotfile backups for Linux, with unattended sync and a
> keyboard-driven TUI.

Recommended topics: `dotfiles`, `backup`, `git`, `rust`, `linux`, `ratatui`,
`systemd`, `dotfile-manager`.

## Releases and security

Dothoard releases remain experimental and are clearly marked as pre-releases;
see [Experimental releases](docs/releases.md). To report a vulnerability,
follow the private reporting process in [SECURITY.md](SECURITY.md), not a public
issue.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
