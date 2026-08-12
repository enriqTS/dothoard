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
model](safety.md) before trying a pre-release. Release binaries are not yet
published; build from source using the [development guide](development.md).

## AUR packaging

AUR packaging is deliberately deferred until the release workflow can produce
and verify release artifacts consistently. It must not imply stable support or
bypass the pre-release policy above.
