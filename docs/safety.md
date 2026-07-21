# Safety Model and Limitations

This document explains dothoard's safety boundaries, what it does not do,
and what requires manual intervention.

## Backup only — no restore

dothoard is a one-way backup tool. It copies files from `$HOME` into the
repository. It never writes back into your home directory.

To restore files, use standard Git:

```bash
cd ~/dotfiles
git log --oneline -- home/.config/fish/config.fish   # find the version
git show HEAD:home/.config/fish/config.fish > ~/.config/fish/config.fish
```

Or check out an entire source tree:

```bash
cp -r ~/dotfiles/home/.config/fish/ ~/.config/fish/
```

Automated restore (with conflict detection and selective recovery) is deferred
to a future version.

## Symlink safety

dothoard never follows symlinks during traversal:

- **Source parents:** If any directory component between `$HOME` and a source
  root is a symlink, that source is rejected. This prevents an attacker or
  misconfiguration from redirecting the backup to arbitrary locations.

- **Source root:** A source path that is itself a symlink is preserved as a
  link — the raw target path is stored, but the target is never read or
  entered.

- **Destination:** Before writing or deleting inside `repository/home/`,
  dothoard verifies that no parent component in the path is a symlink. This
  prevents symlink-based escapes that could write outside the repository.

- **Traversal:** Directory walking never enters symlinked directories. Files
  that are symlinks are recorded as links (target path only).

## Filesystem boundaries

- All writes and deletions are confined to `repository/home/` and
  `.dothoard-manifest.toml`.
- dothoard never modifies, stages, discards, or commits files outside those
  paths.
- Dirty (staged, unstaged, or untracked) paths outside the managed namespace
  cause a safe failure — dothoard refuses to run rather than risk committing
  unrelated changes.

## Git secret history

**Ignoring a file does not remove it from Git history.**

If a secret (private key, token, API key) was previously backed up and you
later add it to the ignore list:

1. dothoard removes the file from the working tree and stages the deletion.
2. The file is gone from the current commit onward.
3. **The secret remains in older commits** accessible via `git log`.

### What to do if a secret was committed

1. **Rotate the credential immediately.** This is the only reliable fix.
   Assume it was compromised the moment it entered a remote repository.

2. Optionally rewrite history to remove the file:
   ```bash
   cd ~/dotfiles
   git filter-repo --invert-paths --path home/.config/app/secret.key
   git push --force
   ```
   History rewriting requires `git-filter-repo` (installable via your package
   manager) and a force push to the remote.

3. If the remote is GitHub/GitLab, also invalidate their cached copies
   (support request or repository re-creation may be needed).

dothoard warns about likely secrets in the preview screen to help prevent
accidental commits, but it cannot guarantee that sensitive files are never
added — that responsibility lies with the ignore rules you configure.

## Single-writer expectation

dothoard expects to be the **sole writer** to the managed namespace
(`home/` and `.dothoard-manifest.toml`). Concurrent or external modifications
cause predictable failures:

| Scenario | Behavior |
|----------|----------|
| Two dothoard instances on the same machine | Exclusive lock prevents overlap; second instance reports "already running" and exits |
| Manual edits inside `repository/home/` | Detected as dirty managed paths; dothoard normalizes them on next run (overwrites with source content) |
| External commits touching `home/` | If they conflict with dothoard's next commit, rebase fails and dothoard preserves its local commit |
| Different machine pushing to the same remote | Works if commits don't conflict; conflicts require manual resolution |

### Multi-machine usage

dothoard supports multiple machines pushing to the same remote repository.
Each machine's backup is committed independently. This works well when:

- Each machine backs up different files (no overlap in `home/` paths).
- Conflicts are rare because files change on only one machine at a time.

When two machines modify the same file between syncs, a rebase conflict
occurs. See "Manual conflict recovery" below.

## Manual conflict recovery

When `git pull --rebase` encounters a conflict that cannot be auto-resolved:

1. dothoard **aborts the rebase** to avoid leaving the repository in a
   broken state.
2. The **local commit is preserved** intact in the branch history.
3. dothoard reports the conflict and marks the run as failed.
4. The failure appears in `dothoard check`, the TUI History tab, and as a
   desktop notification.

### Resolving the conflict

```bash
cd ~/dotfiles

# See what happened
git status
git log --oneline -5

# Pull and resolve manually
git pull --rebase origin main

# If conflicts appear, resolve them:
# Edit the conflicted files, then:
git add <resolved-files>
git rebase --continue

# Or abort and accept the remote version:
git rebase --abort
git pull origin main   # merge instead
```

After manual resolution, the next `dothoard backup` run will operate normally.

### Preventing conflicts

- Use one dothoard instance per machine (each with its own commit identity).
- Avoid manually editing files inside `repository/home/` — let dothoard
  manage that namespace.
- If two machines back up the same paths, coordinate changes or accept
  occasional manual resolution.

## What dothoard will NOT do

| Action | Reason |
|--------|--------|
| Follow symlinks during traversal | Prevents escaping intended boundaries |
| Stage files outside `home/` or the manifest | Prevents accidental commits of unrelated work |
| Commit when staging verification fails | Safety boundary: all staged paths must be managed |
| Delete an entire backup when a source root disappears | Preserves data; reports an error instead |
| Continue after a source or manifest failure | All-or-nothing: partial failures prevent Git operations |
| Sign commits | Avoids GPG pinentry blocking unattended runs |
| Run through a login shell | Avoids shell interpolation and injection |
| Log credentials or full remote URLs | Redacted before logging/persisting |
| Install real systemd units from tests | Test isolation is preserved |

## Failure recovery

dothoard is designed to be self-healing across runs:

- **Interrupted mirror:** Partially written files in the working tree are
  normalized by the next run (the planner is stateless and the executor is
  idempotent).
- **Network failure:** Local commits are preserved and pushed on the next
  successful run.
- **Lock contention:** A second instance exits cleanly; the timer retries
  on its next cycle.
- **Rebase conflict:** Aborted safely; local commit preserved; manual
  resolution needed once, then normal operation resumes.

## Notifications

| Event | Notification |
|-------|-------------|
| Successful scheduled backup | Silent (no notification) |
| First failure after success | Critical notification with error summary |
| Recovery after failure | Normal notification confirming recovery |
| notify-send unavailable | Silently skipped; failure is still persisted for the TUI |

## Data the application stores

| Location | Content |
|----------|---------|
| `~/.config/dothoard/config.toml` | Configuration (no secrets) |
| `~/.local/state/dothoard/` | Run history, status (JSON) |
| `$XDG_RUNTIME_DIR/dothoard.lock` | Exclusive lock (empty file) |
| Repository `home/` | Backed-up files |
| Repository `.dothoard-manifest.toml` | Source mapping and ignore config |

None of these contain credentials. Remote URLs stored in state are redacted
if they contain embedded credentials.
