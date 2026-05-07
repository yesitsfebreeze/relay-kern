# auth — provider credentials and role bindings

Central source of truth for API keys used by `kern`, `repl`, and
every sub-agent. Replaces scattered `std::env::var("*_API_KEY")` reads
with a single typed table loaded at startup.

## Layout

```
$XDG_CONFIG_HOME/relay/       (unix default: ~/.config/relay/)
  config.toml                 non-secret app config
  auth.toml                   providers + role bindings, chmod 600
```

Windows: `%APPDATA%\relay\`. Override both with `$RELAY_CONFIG_DIR` for
tests and alternate installs.

Resolution order (first hit wins):

1. `$RELAY_CONFIG_DIR`
2. `$XDG_CONFIG_HOME/relay`
3. `$HOME/.config/relay`
4. `%APPDATA%\relay`

## auth.toml

```toml
[providers.anthropic]
api_key = "sk-ant-..."
# base_url defaults to https://api.anthropic.com

[providers.openai]
api_key_cmd = "pass show openai/api"
trust_hash  = "3f2a...b7"    # SHA-256 of the cmd string at trust-time

[providers.ollama]
api_key_env = "OLLAMA_KEY"    # read env var at resolve-time
base_url    = "http://localhost:11434"

[roles.repl]
provider = "anthropic"
model    = "claude-opus-4-7"

[roles.embedding]
provider = "openai"
model    = "text-embedding-3-large"

[roles.orchestrator]
provider = "anthropic"
model    = "claude-sonnet-4-6"

[roles.ingestion]
provider = "openai"
model    = "gpt-4o-mini"

[roles.harness]
provider = "anthropic"
model    = "claude-haiku-4-5"

[roles.background_agent]
provider = "anthropic"
model    = "claude-haiku-4-5"
```

## Roles

Each role binds to exactly one `(provider, model)` pair. Adding a role
= one variant in `auth::Role`. No free-string role lookup at runtime —
typos caught at compile time.

| Role               | Used by                                       |
|--------------------|-----------------------------------------------|
| `repl`              | REPL turns                                    |
| `embedding`        | Ingest + retrieval vectors                    |
| `orchestrator`     | Goal-node reasoning in `agnt`                |
| `ingestion`        | Summarization / extraction workers            |
| `harness`          | Sub-agent turns under recipes                 |
| `background_agent` | Merge / consolidation loop inside `kern`     |

## Key sources

Exactly one per provider; parser rejects combinations.

- `api_key` — plain literal. Simplest, lowest friction.
- `api_key_cmd` — shell-out. Stdout trimmed → key. 5 s timeout. Requires
  `trust_hash` matching SHA-256 of the command string; mismatched hash
  refuses to execute until re-trusted.
- `api_key_env` — read a named env var at resolve-time. Useful for CI.

## Security

- File mode `0600` on write; load refuses anything wider on unix.
- Parent dir `0700`.
- Write refuses if the target path lives inside a git worktree
  (walks parents for `.git`).
- `Secret` newtype: `Debug`/`Display` → `"***"`, zeroized on drop.
- No key material ever enters error messages or logs — errors name
  providers/roles only.
- Shell-out captures stderr into a failure message; stdout trimmed,
  empty output is a load failure.

Windows ACL enforcement is TODO — the default `%APPDATA%` ACLs are
user-profile scoped, but this should still be tightened explicitly.

## Interfaces

### Rust

```rust
let a = auth::Auth::load()?;
let r = a.resolve(auth::Role::Chat)?;
// r.provider, r.model, r.base_url, r.api_key.expose()
```

### Shell

```
kern auth login   <provider>                      # masked prompt, save
kern auth list                                    # show providers + roles
kern auth logout  <provider>
kern auth set-role <role> <provider> <model>
```

### In-repl slash

```
/login  <provider> <key>     # inline save (echo'd — prefer CLI)
/login  <provider>           # explain both paths
/logout <provider>
/models                      # list providers + role bindings
```

The CLI is the safe path for key entry; rpassword masks TTY input.
`/login` inline form exists for when shelling out is inconvenient —
keys typed there are not logged to the journal, but repl input history
is visible on-screen until the next paint. A proper TUI masked modal
is planned.

## Migration

Env vars still work as a fallback during migration:

- Harness: `[roles.repl]` → `ANTHROPIC_API_KEY` fallback.
- Kern loader: `[roles.embedding]`, `[roles.orchestrator]` → then
  `OPENAI_*` / `KERN_*` env vars.

Planned removal: once every install has an `auth.toml`, strip
`apply_env`'s `OPENAI_*` / `KERN_{EMBED,REASON}_*` reads and make
auth.toml required.

## Non-goals

- **Not a secret store.** Use `api_key_cmd` + a real keychain (pass,
  gopass, 1password CLI, macOS Keychain via `security find-generic-password`)
  if you need one.
- **Not an OAuth broker.** Bearer tokens only for v1. Future providers
  needing OAuth add their own flow behind the `KeySource` enum.
- **No per-workspace overrides.** Single user-scope file. Workspace
  ergonomics belong in `config.toml`, not auth.
