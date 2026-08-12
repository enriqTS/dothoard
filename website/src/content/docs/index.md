---
title: dothoard
description: Safe, Git-native dotfile backups for Linux.
---

> **Experimental:** dothoard is under active development. Use a dedicated
> repository, review previews, and do not rely on it for important data without
> understanding its [safety model](./safety/).

## What it is for

Dothoard is a focused backup and synchronization tool for Linux users who want
unattended dotfile backups without giving up control of Git. It is not a restore
manager, cloud backup service, persistent daemon, or general-purpose repository
manager.

## Start in five minutes

1. Install Rust 1.97+, Git, and use a systemd user session.
2. Create or clone a dedicated Git repository.
3. Install and open dothoard:

   ```bash
   cargo install --path .
   dothoard
   ```

4. In the TUI, select the repository, create or select a namespace, choose
   sources, inspect the preview, and run the first backup.
5. Once that works, enable scheduled backups with:

   ```bash
   dothoard service install
   ```

Continue to the [Quick start](./quick-start/) for the full walkthrough.

## Safety highlights

- It does not follow symlinks during traversal.
- Writes, deletion, staging, and commits stay inside the active namespace.
- Sibling namespaces and unmanaged repository paths remain untouched.
- Scheduled Git operations are noninteractive and timeout-bounded.
- A failed sync preserves the local commit for a later retry.
- Preview calls out likely secrets; ignore rules cannot erase Git history.

Read the [Safety model and limitations](./safety/) before use.
