# Development

## Setup

On CachyOS or Arch Linux:

```bash
sudo pacman -Syu --needed base-devel git rustup
rustup default stable
git clone https://github.com/enriqTS/dothoard.git
cd dothoard
cargo build
```

The minimum supported Rust version is 1.97.

## Verification

Run the complete baseline before submitting a change:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features -- --test-threads=1
```

`make check` runs formatting and Clippy; `make test` runs the serialized test
suite. GitHub Actions runs this same baseline for pushes and pull requests.
Tests must use temporary homes, repositories, remotes, runtime directories,
unit locations, and notification tooling. They must not mutate real user state.

## Architecture and safety

The short-lived headless commands and Ratatui TUI call shared backend services.
The backend must not depend on the TUI. Read `AGENTS.md`, `PLAN.md`,
`DEVELOPMENT_PLAN.md`, and `MEMORY.md` before changing behavior.

Safety invariants—including no-follow traversal, active-namespace ownership,
restricted staging, and noninteractive process execution—are defined in
`PLAN.md`. Regression fixes require a test that reproduces the problem first.

See [Contributing](contributing.md) for the contribution workflow and
[Experimental releases](releases.md) for the pre-release and AUR policy.
