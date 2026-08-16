# Backup Automation

Dothoard is not a persistent daemon. Every scheduled run is a short-lived
invocation of:

```bash
/absolute/path/to/dothoard backup
```

The backup command provides the same validation, exclusive locking,
noninteractive Git operation, network timeouts, state history, run logs, and
failure/recovery notification attempts whether it is started manually or by a
scheduler.

## Managed systemd automation

Systemd user timers are currently the only automation backend that dothoard
installs, removes, and inspects itself:

```bash
dothoard service install
dothoard service status
dothoard service remove
```

The timer starts one minute after the user manager starts and schedules the
next run after the previous backup becomes inactive. The CLI, health check, and
TUI use a scheduler-neutral automation layer, which currently selects the
systemd backend.

## Using cron manually

Cron can invoke the backup command today even though dothoard does not yet
manage crontabs. First find the absolute executable path:

```bash
command -v dothoard
```

Run `crontab -e` and add an entry using that absolute path. Replace the example
home, executable, numeric user ID, and agent socket with values from the target
account:

```text
HOME=/home/alice
PATH=/home/alice/.local/bin:/usr/local/bin:/usr/bin:/bin
XDG_RUNTIME_DIR=/run/user/1000
SSH_AUTH_SOCK=/run/user/1000/ssh-agent.socket

*/5 * * * * /home/alice/.local/bin/dothoard backup
```

Do not place passwords, access tokens, or credential-bearing URLs in the
crontab. Use an SSH agent or a credential helper as described in [Git
Authentication](authentication.md).

A cron-launched command does not read an interactive shell profile by default.
Confirm that all paths are absolute and that `HOME`, `PATH`, and any credential
agent variables match the cron environment. `XDG_RUNTIME_DIR` should identify
the account's existing private runtime directory so manual, TUI, and scheduled
runs use the same lock. Desktop failure notifications additionally require the
appropriate session variables, commonly `DBUS_SESSION_BUS_ADDRESS`, `DISPLAY`,
or `WAYLAND_DISPLAY`; missing notification access does not prevent state and
run-log persistence.

Before saving the schedule, approximate cron's minimal environment and verify
configuration and authentication. Preserve any variables required by your
credential setup:

```bash
env -i \
  HOME="$HOME" \
  PATH="/home/alice/.local/bin:/usr/local/bin:/usr/bin:/bin" \
  XDG_RUNTIME_DIR="/run/user/1000" \
  SSH_AUTH_SOCK="/run/user/1000/ssh-agent.socket" \
  /home/alice/.local/bin/dothoard check
```

The current check will show the non-fatal warning `systemd user timer: warning:
automation not installed` when cron is used without systemd. Explicit cron
backend selection and cron-aware health status are planned next.

### Cron behavior differences

- `*/5` uses fixed wall-clock boundaries. It does not wait five minutes after a
  backup completes as the systemd timer does.
- Traditional cron does not replay runs missed while the machine was powered
  off or suspended.
- If a run is still active at the next boundary, dothoard's exclusive lock
  makes the second process exit with code 2 without changing backup content.
- Cron may mail command output or send it to its own logs. Dothoard separately
  writes per-run logs beneath `~/.local/state/dothoard/logs/` and records recent
  results for the TUI. Configure cron output only after confirming the local
  cron implementation's behavior.
- Cron environment and desktop-session access vary by implementation and login
  method. Test after reboot and after a normal login before relying on it.

## Other schedulers

`fcron`, BSD cron, OpenRC systems using a cron daemon, and similar schedulers
can use the same absolute command. `fcron` may be preferable when missed-run
handling is required. `anacron` is generally suited to hourly or daily work,
not dothoard's default five-minute interval.

For runit, s6, dinit, or another service supervisor, prefer its native periodic
or timer facility when available. Avoid an unsupervised shell loop. Dothoard
intentionally remains a short-lived process and does not provide a built-in
scheduling daemon.

Regardless of scheduler, test `dothoard check` and one manual `dothoard backup`
under the intended environment before enabling unattended runs.
