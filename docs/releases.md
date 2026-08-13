# Experimental releases

Dothoard is experimental software. A release means that a specific source
revision has passed the documented quality baseline; it does **not** mean that
backup data is guaranteed safe or that the project is stable.

## Pre-release policy

Until a stable release is explicitly announced, GitHub Releases use a tag such
as `v1.1.0-alpha.1` and are marked **pre-release**. Each release notes:

- the version, commit, and date;
- user-visible changes and known limitations;
- the supported platforms (currently CachyOS and Arch Linux);
- verification performed; and
- upgrade or rollback notes when applicable.

Use a dedicated repository, review backup previews, and read the [safety
model](safety.md) before trying a pre-release.

## Installing a release

Pushing a `v*` tag runs the release workflow, which repeats the CI quality
baseline (formatting, Clippy, and the full test suite) before building an
x86_64 Linux binary and its checksum, so a release is never less verified than
a regular CI-passing commit. The workflow uploads these as a **draft** GitHub
Release with generated notes; a maintainer edits the notes to match the fields
above and publishes it before it is publicly visible or installable. Install
the latest published release with:

```bash
curl -fsSL https://raw.githubusercontent.com/enriqTS/dothoard/main/scripts/install.sh | sh
```

The script verifies the release archive's published SHA-256 checksum before
installing. Set `VERSION` to install a specific tag, or `INSTALL_DIR` to
change the install location; run `cat scripts/install.sh` to review it before
piping it into a shell. Only x86_64 Linux binaries are published, matching the
currently supported platforms; other platforms, or anyone who prefers not to
run prebuilt binaries, should build from source using the [development
guide](development.md).

## AUR packaging

AUR packaging is deliberately deferred until the release workflow above has a
track record of producing and verifying release artifacts consistently across
several releases. It must not imply stable support or bypass the pre-release
policy above.
