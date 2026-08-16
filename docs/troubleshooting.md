# Troubleshooting

## `dothoard check` reports an invalid repository

Choose an existing dedicated Git worktree with a branch and configured remote.
Dothoard does not create or clone repositories. Confirm it manually:

```bash
cd ~/dotfiles
git status
git remote -v
```

## The remote is not accessible

Run `dothoard check` and configure noninteractive SSH or HTTPS credentials. See
[Authentication](authentication.md). Scheduled backups cannot answer password,
passphrase, browser, or host-key prompts.

## A backup is blocked by dirty unmanaged paths

Dothoard only owns the active namespace. Inspect the worktree:

```bash
cd ~/dotfiles
git status --short
```

Commit, stash, or discard unrelated changes yourself. Do not ask dothoard to
clean them: refusing them is a safety boundary.

## A source disappeared

A missing source root fails the run but does not delete its complete existing
backup. Restore the source or remove it deliberately through the TUI or
configuration, then preview the resulting deletion.

## A cron backup fails but a terminal backup works

Cron does not normally load the interactive shell environment. Verify the
absolute executable path, `HOME`, `PATH`, `XDG_RUNTIME_DIR`, and credential
agent variables. Use the minimal-environment test in [Backup
automation](automation.md), and inspect `~/.local/state/dothoard/logs/`.

## Cron automation is stale or refuses installation

Cron intervals must be from 1 through 59 minutes, and the executable and
runtime-directory paths must be safely representable without shell quoting.
Dothoard refuses duplicate, incomplete, or unowned managed markers rather than
risk replacing unrelated crontab content. Inspect with `crontab -l`; repair an
ambiguous marker block manually only after preserving the complete crontab.

Dothoard verifies its block but cannot verify that the cron daemon is active.
Check that separately using the operating system's service tools.

## The systemd timer differs from configuration

Reinstall the generated units after changing the interval:

```bash
dothoard service install
dothoard service status
```

Inspect service logs with:

```bash
journalctl --user -u dothoard-backup.service -f
```

## A rebase conflict occurred

Dothoard aborts the rebase and preserves its local commit. Resolve it manually
in the repository, then run another backup. Follow the detailed
[conflict-recovery instructions](safety.md#manual-conflict-recovery).

## More help

Read [FAQ](faq.md), [Safety model](safety.md), or open an issue with sanitized
versions of `dothoard check`, `git status --short`, and relevant logs. Never
include credentials, tokens, or credential-bearing remote URLs.
