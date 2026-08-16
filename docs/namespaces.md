# Multiple-Machine Namespaces

Several computers can share one dothoard repository when each uses a distinct,
explicit namespace. A namespace is not inferred from the hostname.

```text
repository/
|-- desktop/
|   |-- home/
|   `-- .dothoard-manifest.toml
`-- notebook/
    |-- home/
    `-- .dothoard-manifest.toml
```

A machine owns only `<namespace>/home/` and
`<namespace>/.dothoard-manifest.toml`. It cannot write, delete, stage, clean,
or claim sibling namespaces. Root-level legacy `home/` data and manifests are
unmanaged and are never adopted or migrated automatically.

## Set up another machine

1. Clone the same dedicated repository.
2. Start dothoard. First-run setup opens repository selection directly.
3. Choose the clone, then select the existing namespace for this machine or
   explicitly create a new one. Dothoard never silently chooses `desktop` or a
   hostname-derived name.
4. When an owned namespace is selected, dothoard validates its manifest and
   restores its source paths and ignore rules into local configuration. Review
   the selections in **Sources**; paths absent on the fresh machine remain
   selected and are reported as missing without deleting their existing backup.
5. Run `dothoard check`, then run a reviewed manual backup before enabling the
   timer.

Selecting a new namespace clears source selections so settings from the
previous active namespace cannot leak into it. Switching between owned
namespaces reloads each namespace's own manifest settings.

Each machine commits a different directory tree, so normal synchronization
works when namespaces differ. Git rebase conflicts are still possible if users
manually create conflicting repository history; dothoard aborts the rebase and
preserves its local commit for manual resolution.

## Lifecycle operations

The TUI can create, select, rename, and delete namespaces. Selecting an owned
namespace also loads that namespace's source and ignore configuration. It shows the
affected path and requires confirmation for ownership-sensitive or destructive
steps. Creation rejects ambiguous content and collisions. Rename and delete
operate only on the active namespace; you must select or create another usable
namespace before deleting the active one.

Do not manually migrate a legacy root-level layout while dothoard is running.
If needed, perform and commit that migration manually in an otherwise clean
worktree.
