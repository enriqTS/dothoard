# Frequently Asked Questions

## Is dothoard a dotfile manager?

No. It is a one-way backup and Git synchronization tool. It does not create
symlinks into your home directory or automatically restore files.

## Can it back up files outside `$HOME`?

No. Paths outside `$HOME` and parent traversal are rejected.

## Can several machines use the same repository?

Yes, when each selects a different [namespace](namespaces.md). Each machine
owns only its own namespace.

## Does dothoard create a repository or clone one?

No. You must provide an existing dedicated Git clone.

## Does it encrypt my backups?

No. Encryption before committing is deferred. Treat the repository as readable
by anyone with access to its Git history and use ignore rules for sensitive
files.

## Can I schedule backups without systemd?

Yes. Select the managed `cron` backend, or have an external scheduler run the
absolute `dothoard backup` path. See [Backup automation](automation.md) for
switching, cron timing, status, and environment requirements.

## What happens offline?

A completed local commit is retained. A later backup retries synchronization,
even when no new files changed.

## What happens if I accidentally commit a secret?

Rotate it immediately. Ignoring or deleting the file does not remove older Git
history. See [Safety model](safety.md#git-secret-history).

## Why does dothoard refuse a dirty repository?

It blocks dirty paths outside the active namespace to avoid staging, modifying,
or committing unrelated work. This is intentional.

## Where is restore support?

Restore is deferred. Use standard Git and filesystem commands deliberately;
see [Safety model](safety.md#backup-only--no-restore).
