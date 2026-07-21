# Git Authentication for dothoard

dothoard runs Git commands noninteractively — no password prompts, no
passphrase dialogs, no host-key confirmations. If your remote requires
authentication, you must configure credential access so it works without
user input.

This document covers the supported authentication methods.

## How dothoard disables prompts

dothoard sets the following environment for every Git operation:

| Variable | Value | Effect |
|----------|-------|--------|
| `GIT_TERMINAL_PROMPT` | `0` | Prevents git from opening a terminal prompt |
| `GIT_ASKPASS` | (empty) | Disables the askpass password dialog |
| `SSH_ASKPASS` | (empty) | Disables SSH askpass GUI |
| `SSH_ASKPASS_REQUIRE` | `never` | Prevents SSH from using askpass even if set |
| `GCM_INTERACTIVE` | `Never` | Disables Git Credential Manager interactive mode |
| `GIT_SSH_COMMAND` | `ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new` | No SSH password/passphrase/host-key prompts |

If authentication cannot proceed without user interaction, dothoard reports
a failure rather than hanging.

## SSH Authentication (recommended)

SSH with a key loaded in an agent is the simplest setup for noninteractive
access.

### 1. Generate an SSH key (if you don't have one)

```bash
ssh-keygen -t ed25519 -C "dothoard@$(hostname)"
```

You can use a passphrase — as long as an agent holds the unlocked key at
runtime.

### 2. Add the key to your SSH agent

Most desktop environments start `ssh-agent` or a keyring (GNOME Keyring,
KDE Wallet, gcr-ssh-agent) automatically on login.

Verify your agent is running:

```bash
ssh-add -l
```

If it shows your key, you're set. If not:

```bash
# Start the agent (if not auto-started by your session):
eval "$(ssh-agent -s)"

# Add your key:
ssh-add ~/.ssh/id_ed25519
```

For persistence across reboots, configure your desktop environment or
`~/.config/environment.d/` to start the agent and add the key at login.

**systemd-based agent (Arch/CachyOS):**

Create `~/.config/systemd/user/ssh-agent.service`:

```ini
[Unit]
Description=SSH key agent

[Service]
Type=simple
Environment=SSH_AUTH_SOCK=%t/ssh-agent.socket
ExecStart=/usr/bin/ssh-agent -D -a $SSH_AUTH_SOCK

[Install]
WantedBy=default.target
```

Then:

```bash
systemctl --user enable --now ssh-agent
echo 'export SSH_AUTH_SOCK="$XDG_RUNTIME_DIR/ssh-agent.socket"' >> ~/.profile
```

After login, `ssh-add` once and the key remains available for the entire
session (including dothoard's timer).

### 3. Add the public key to your Git host

```bash
cat ~/.ssh/id_ed25519.pub
```

Add the output to:
- **GitHub:** Settings > SSH and GPG keys > New SSH key
- **GitLab:** Preferences > SSH Keys
- **Gitea/Forgejo:** Settings > SSH / GPG Keys

### 4. Accept the host key

dothoard uses `StrictHostKeyChecking=accept-new`, which means:
- A **new** host key is automatically accepted and saved on first connection.
- A **changed** host key (potential MITM) is rejected.

To pre-accept the host key manually:

```bash
ssh -T git@github.com       # answer "yes" when prompted
ssh -T git@gitlab.com
```

Or add it directly:

```bash
ssh-keyscan github.com >> ~/.ssh/known_hosts
```

### 5. Verify noninteractive access

```bash
dothoard check
```

The check command tests `git ls-remote` against your configured remote and
reports whether it responds without interaction.

Or test manually:

```bash
GIT_TERMINAL_PROMPT=0 \
SSH_ASKPASS="" \
GIT_SSH_COMMAND="ssh -o BatchMode=yes" \
git ls-remote origin
```

If this prints refs, dothoard can push and pull.

## HTTPS Authentication

HTTPS remotes require a credential helper that provides tokens without
prompting.

### Git Credential Manager (GCM)

GCM stores tokens in a system keyring. After an initial interactive login,
subsequent operations are noninteractive.

```bash
# Install (Arch)
paru -S git-credential-manager

# Configure git to use it
git config --global credential.helper manager

# Do one interactive auth to store the token
git push
```

After this, dothoard's `GCM_INTERACTIVE=Never` tells GCM to use the stored
credential without opening a browser or prompt.

### git-credential-store (plaintext file)

Stores credentials in `~/.git-credentials` as plaintext. Simple but less
secure.

```bash
git config --global credential.helper store
# Next interactive push stores the credential
git push
```

### git-credential-libsecret (GNOME Keyring)

Uses the GNOME Keyring (or any libsecret-compatible store):

```bash
git config --global credential.helper /usr/lib/git-core/git-credential-libsecret
```

### Personal access tokens

Generate a token on your Git host and store it via your credential helper.
For GitHub:

1. Go to Settings > Developer settings > Personal access tokens > Fine-grained tokens.
2. Create a token with `Contents: Read and write` permission on your dotfiles repo.
3. Use the token as your password on the next push (the credential helper saves it).

### Verify HTTPS access

```bash
dothoard check
```

Or manually:

```bash
GIT_TERMINAL_PROMPT=0 \
GCM_INTERACTIVE=Never \
git ls-remote origin
```

## Troubleshooting

### "remote not accessible" in `dothoard check`

1. **SSH:** Verify `ssh-add -l` shows your key. If empty, your agent isn't
   running or the key isn't loaded.
2. **HTTPS:** Run `git credential fill` to check if a stored credential
   exists for your remote host.
3. **Host key:** If the remote host key changed, remove the old entry from
   `~/.ssh/known_hosts` and reconnect.

### Timer runs fail but manual works

The systemd user service inherits environment from the user manager, not from
your shell session. Common issues:

- `SSH_AUTH_SOCK` not set in the systemd environment. Fix:
  ```bash
  systemctl --user import-environment SSH_AUTH_SOCK
  ```
  Or use `environment.d` for persistence:
  ```bash
  echo 'SSH_AUTH_SOCK="${XDG_RUNTIME_DIR}/ssh-agent.socket"' > \
    ~/.config/environment.d/ssh-agent.conf
  ```

- GNOME Keyring / KDE Wallet not unlocked. These typically unlock on login;
  if using auto-login without a display manager, the keyring may remain
  locked.

### "connection timed out"

- Check network connectivity.
- Check `network_timeout_seconds` in your config (default: 120s).
- dothoard preserves local commits when the network is unavailable and
  pushes on the next successful run.

### GPG passphrase prompts

dothoard does not sign commits by default. If your global `git config` has
`commit.gpgsign=true`, dothoard will still work because it uses its own
committer identity and environment. However, if you've configured signing
per-repository, consider disabling it for your dotfiles repo:

```bash
cd ~/dotfiles
git config commit.gpgsign false
```

## Summary of requirements

| Method | What must be ready at timer runtime |
|--------|-------------------------------------|
| SSH + agent | Agent running, key loaded, host key known |
| HTTPS + GCM | Token stored in keyring, keyring unlocked |
| HTTPS + store | Credential in `~/.git-credentials` |
| HTTPS + libsecret | Token in GNOME Keyring, session unlocked |
