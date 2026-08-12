# Configuration Reference

The configuration file is `~/.config/dothoard/config.toml`. It is written
atomically by the application.

```toml
version = 2
repository = "~/dotfiles"
remote = "origin"
namespace = "desktop"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = ["fish_variables", "*.log", "cache/"]
```

## Fields

| Field | Required | Meaning |
|---|---:|---|
| `version` | Yes | Must be `2`. Version 1 has no automatic migration. |
| `repository` | Yes | Existing dedicated Git worktree; `~` is supported. |
| `remote` | No | Git remote name; defaults to `origin`. |
| `namespace` | Yes | Explicit portable name for this machine. |
| `interval_minutes` | No | Timer interval; defaults to 5 and must be at least 1. |
| `network_timeout_seconds` | No | Timeout for network Git work; defaults to 120. |
| `sources` | No | Home-relative files or directories to back up. |

## Source paths

A source path is relative to `$HOME`. Absolute paths, `..`, overlapping sources,
repository recursion, and symlinked parent components are rejected. A selected
source may itself be a symlink, which is stored without following its target.

Sources mirror beneath `<repository>/<namespace>/home/` with their path
relative to `$HOME`. Empty directories do not create commits.

## Namespaces

A namespace is a nonempty portable ASCII path component using only letters,
digits, `.`, `_`, and `-`. It cannot be `.` or `..`, absolute, or contain a path
separator. The local configuration is authoritative; a repository manifest is
an ownership marker, not configuration to apply automatically.

See [Namespaces](namespaces.md) for lifecycle behavior.

## Ignore rules

Each source can have `ignore = [...]`. Rules use Git-style semantics rooted at
that source. See [Ignore rules](ignore-rules.md) for examples and limitations.

## Theme preference

`~/.config/dothoard/theme.toml` stores the TUI's selected theme, separately
from `config.toml` so a theme can be chosen before a repository is
configured:

```toml
theme = "catppuccin-mocha"
```

It is written when a theme is confirmed from the theme picker (`Ctrl+T`) and
is optional; a missing or unrecognized file falls back to Catppuccin Mocha.
See [TUI Guide](tui.md#themes) for the full list of built-in themes.
