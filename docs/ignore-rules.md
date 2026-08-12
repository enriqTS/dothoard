# Ignore Rules

Ignore rules belong to one configured source and use `.gitignore` semantics
rooted at that source. They are evaluated in order; the last matching rule
wins.

```toml
[[sources]]
path = ".config/fish"
ignore = [
  "fish_variables",
  "*.log",
  "/cache/",
  "!cache/keep.log",
]
```

## Examples

| Pattern | Meaning |
|---|---|
| `*.log` | Match files ending in `.log` at any depth. |
| `/build` | Match `build` only at the source root. |
| `cache/` | Match directories named `cache`. |
| `!important.log` | Re-include a previous match when its parent is included. |
| `\#comment` | Match a literal name beginning with `#`. |

A child cannot be re-included while its parent directory remains excluded.
Rules in `.gitignore` files inside sources are copied as ordinary files; they
are not loaded as dothoard rules.

Nested `.git` directories and unsupported special files are always excluded and
cannot be negated. Hidden files are included unless a rule excludes them.

## Secrets and tracked files

Use ignore rules to prevent new secrets from entering the worktree. The Preview
and Ignore screens warn about likely private keys, credentials, tokens, and
cookies. Ignoring a file already committed does not remove it from Git history:
rotate exposed credentials, then use Git tooling manually if history rewriting
is appropriate. See [Safety model](safety.md).
