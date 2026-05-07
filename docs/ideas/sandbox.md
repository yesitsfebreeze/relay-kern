# OS-Level Sandbox Plan for `relay`

Defense-in-depth beneath the Rust capability gates. Assumes the harness is
already compromised; these layers still hold.

## What the process legitimately needs

- **Read:** `~/.relay/auth.toml`, `<repo>/src/recipes/`, `<repo>` working tree.
- **Read-write:** `<relay data dir>` (default `~/.relay/data/`), `$TMPDIR`.
- **Net out:** `api.anthropic.com:443`, `127.0.0.1:<relay port>`, MCP bridge sockets (unix or loopback).
- **Deny everything else** — other home dotfiles, `~/.ssh`, `~/.aws`, browser profiles, `/etc/shadow`-equivalents, arbitrary egress.

---

## 1. Linux — systemd user unit (primary) + bubblewrap (ad-hoc)

`~/.config/systemd/user/relay.service`:

```ini
[Unit]
Description=relay chat REPL (sandboxed)

[Service]
ExecStart=%h/.cargo/bin/relay
WorkingDirectory=%h/dev/relay

# Filesystem
ProtectSystem=strict
ProtectHome=read-only
ReadOnlyPaths=%h/.relay/auth.toml %h/dev/relay/src/recipes
ReadWritePaths=%h/.relay/data %h/dev/relay/target
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes

# Privileges
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes

# Syscalls
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallFilter=~@privileged @resources @mount @debug @module @raw-io @reboot @swap @cpu-emulation

# Network: only Anthropic + loopback (relay). Resolve Anthropic IP range via
# IPAddressAllow; systemd uses BPF, no DNS hook needed if you pin resolver to
# systemd-resolved on loopback.
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
IPAddressDeny=any
IPAddressAllow=127.0.0.0/8
IPAddressAllow=::1/128
# Anthropic fronted by Cloudflare — tighten to current API IPs or keep
# egress via a local proxy (see fallback) to avoid CF churn.
IPAddressAllow=104.16.0.0/12
```

Ad-hoc `bwrap` (no systemd):

```bash
bwrap --unshare-all --share-net \
  --ro-bind ~/.relay/auth.toml ~/.relay/auth.toml \
  --ro-bind ~/dev/relay/src/recipes ~/dev/relay/src/recipes \
  --bind   ~/.relay/data ~/.relay/data \
  --tmpfs /tmp --dev /dev --proc /proc \
  --die-with-parent --new-session \
  -- ~/.cargo/bin/relay
```

Landlock is a reasonable addition for in-process enforcement; systemd's FS gates already cover the same ground at unit start.

## 2. macOS — `sandbox-exec`

`~/dev/relay/ops/relay.sb`:

```scheme
(version 1)
(deny default)
(allow process-fork process-exec)
(allow mach-lookup)
(allow file-read* (subpath (param "REPO")))
(allow file-read* (literal (string-append (param "HOME") "/.relay/auth.toml")))
(allow file-read-write* (subpath (string-append (param "HOME") "/.relay/data")))
(allow file-read-write* (subpath "/private/tmp"))
(allow network-outbound (remote tcp "127.0.0.1:*"))
(allow network-outbound (remote tcp "*:443"))  ; tighten with a proxy
(deny network-outbound (remote tcp "*:22"))
```

Launch: `sandbox-exec -D HOME="$HOME" -D REPO="$HOME/dev/relay" -f ops/relay.sb relay`.

App Sandbox entitlements only matter if you ship a notarized `.app` — skip until distribution.

## 3. Windows — AppContainer + firewall + Job Object

**AppContainer profile** (PowerShell, once):

```powershell
New-AppContainerProfile -Name relay-ac -DisplayName "relay" `
  -Description "relay chat sandbox" `
  -Capabilities @('internetClient','privateNetworkClientServer')
```

Launch relay inside it via `CreateProcessAsUser` with a `SECURITY_CAPABILITIES` struct (wrap in a small launcher exe, or use `Start-Process -AppContainer` via the Sandboxing API). Grant FS ACLs to the container SID for exactly:

```
icacls "%USERPROFILE%\.relay\auth.toml"   /grant *S-1-15-2-...:R
icacls "%USERPROFILE%\.relay\data"        /grant *S-1-15-2-...:(OI)(CI)M
icacls "%USERPROFILE%\dev\relay\src\recipes" /grant *S-1-15-2-...:(OI)(CI)R
```

**Firewall egress allowlist** (elevated):

```cmd
netsh advfirewall firewall add rule name="relay-block-all-out" ^
  dir=out program="%USERPROFILE%\.cargo\bin\relay.exe" action=block
netsh advfirewall firewall add rule name="relay-allow-anthropic" ^
  dir=out program="%USERPROFILE%\.cargo\bin\relay.exe" action=allow ^
  remoteip=104.16.0.0/12 remoteport=443 protocol=tcp
netsh advfirewall firewall add rule name="relay-allow-loopback" ^
  dir=out program="%USERPROFILE%\.cargo\bin\relay.exe" action=allow ^
  remoteip=127.0.0.1 protocol=tcp
```

Block rules win over allow; invert the order by setting the block rule's `remoteip` to `any` and excluding via a higher-priority allow — or simpler, omit the block and set outbound default to block for that program via a WFP filter. Easiest MVP: the three rules above plus Windows Firewall outbound default = Block for this program (set via `wf.msc` → Outbound Rules → New → Program).

**Job Object** (launcher sets): `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `ActiveProcessLimit=8`, `ProcessMemoryLimit=2GB`, `JOB_OBJECT_UILIMIT_HANDLES`. Prevents runaway forks and handle theft.

## 4. Cross-platform in-process fallback

If the user won't configure OS gates:

- **HTTP allowlist:** wrap `reqwest::Client` in a `connect` interceptor that rejects any host not in `{api.anthropic.com, 127.0.0.1}`. Put it in the `llm` plugin and the relay client; fail closed.
- **FS allowlist:** a thin `relay_fs` wrapper around `std::fs` that canonicalizes every path and rejects anything outside `{repo, ~/.relay/data, $TMPDIR}`. Compile-time ban direct `std::fs` via clippy `disallowed-methods`.
- **Secret loading:** read `auth.toml` once at startup into a `SecretString` (zeroize on drop), then close the fd. Plugins receive a handle, never the path.
- **Env scrub:** on launch, retain only `PATH`, `HOME`, `USER`, `TERM`, `RELAY_*`; drop `AWS_*`, `GITHUB_TOKEN`, `SSH_AUTH_SOCK`, etc.

## 5. What each layer does NOT protect

- **OS sandboxes** don't stop exfil via an *allowed* channel — a malicious plugin can still POST your auth.toml contents to `api.anthropic.com` if it gets the bytes in-process. Secret scoping (fallback #3) is the only mitigation.
- **Firewall rules by remote IP** drift when Anthropic changes Cloudflare ranges. A local egress proxy (mitmproxy/tinyproxy bound to 127.0.0.1) that enforces SNI allowlist is more durable — relay talks only to the proxy.
- **AppContainer** does not sandbox child processes you spawn outside the container (e.g. `git`, shell commands). Launch children inside the same container or deny `CreateProcess` via Job Object.
- **seccomp/Landlock** are process-wide — they won't help against a plugin running in a separate helper you spawn with looser policy.
- **None of these** stop a plugin that reads from stdin/pastes you feed it and reasons about secrets you show it. Human-in-the-loop is its own layer.

## 6. MVP for today on Windows (≤ 1 hour)

1. Run `wf.msc`, create outbound rule for `relay.exe`: block all, then one allow rule for TCP 443 to `api.anthropic.com` resolved IPs (or keep wide 443 and rely on SNI — good enough for now).
2. Add a loopback-only allow rule for 127.0.0.1.
3. In `src/bin/relay/chat/src/http_server.rs` and the llm plugin: add a `reqwest` host allowlist (hard-coded `["api.anthropic.com", "127.0.0.1"]`). Reject on mismatch.
4. Move `~/.relay/auth.toml` to `icacls` read-only for your user; load once at startup, zeroize.
5. Defer AppContainer launcher and Linux systemd unit to when you actually ship.

This gets you 80% of the value — egress allowlist + secret scoping — without touching AppContainer plumbing.
