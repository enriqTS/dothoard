# Contributing

Thanks for considering a contribution. Dothoard is experimental and has strict
filesystem and Git safety boundaries.

## Before opening a change

1. Read `AGENTS.md`, `PLAN.md`, `DEVELOPMENT_PLAN.md`, and `MEMORY.md`.
2. Check that the proposed work fits the active plan; open an issue first for a
   larger change.
3. Keep the smallest complete change and avoid unrelated refactors.
4. Add tests with implementation. Bug fixes need a regression test that fails
   before the fix.
5. Run the full [verification baseline](development.md#verification).

## Pull requests

Explain the user-visible behavior, safety implications, tests run, and any
manual validation. Do not include private paths, repositories, credentials,
tokens, or generated build artifacts. Keep commits focused and use concise
conventional messages such as `fix: preserve local commit on sync failure`.

## Reporting bugs

Provide reproducible steps, expected and actual behavior, dothoard version,
platform, and sanitized diagnostics. Never report secrets or complete URLs that
contain credentials. Security-sensitive reports should follow the repository's
future security policy rather than a public issue.
