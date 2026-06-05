# kern Claude Code hooks

Three Claude Code hooks drive kern's automatic memory. They are plain Node ESM
scripts with no dependencies, and all **fail open** — any error exits 0 and the
session proceeds untouched.

| Hook | Event | What it does |
|------|-------|--------------|
| `kern-capture.mjs` | `Stop` | Extracts the new conversation delta from the transcript and writes it to `<cwd>/.kern/capture/`. The daemon drains and distills it. |
| `kern-recall.mjs` | `SessionStart` | Reads `<cwd>/.kern/digest.md` and injects it into the new session as context. |
| `kern-recall-prompt.mjs` | `UserPromptSubmit` | Demand-driven semantic recall: runs `kern search <prompt>` against `<cwd>/.kern` and injects the top scored thoughts (score ≥ `MIN_SCORE`) as context for that prompt. Hard-bounded by `TIMEOUT_MS`. |

All three are **project-scoped by a guard**: each no-ops in any directory
without a `.kern/` folder, so a single global registration is safe across every
project — only directories where a kern is (or has been) active get touched.
`kern-recall-prompt` embeds the prompt every turn (Ollama), so it fails open on
timeout and injects nothing rather than blocking the prompt.

The hooks are also packaged as a Claude **plugin** (`.claude-plugin/plugin.json`
+ `hooks/hooks.json`); enabling the plugin registers all three via
`${CLAUDE_PLUGIN_ROOT}` with no manual `settings.json` editing.

## Install

The repo is a self-contained Claude **plugin** and **marketplace** — install it
straight from GitHub. From any Claude Code session:

```
/plugin marketplace add yesitsfebreeze/relay-kern
/plugin install kern@kern
```

That registers all three hooks (via `${CLAUDE_PLUGIN_ROOT}` — no machine paths)
and the kern MCP server. Restart Claude Code to load them.

**Requirements**

- The `kern` CLI on `PATH` (the hooks and the MCP server both shell out to it).
- A running embedding endpoint for `kern-recall-prompt` (Ollama by default).
- `node` on `PATH` for hook execution.

## How it behaves

The hooks are **project-scoped by a guard**: each no-ops in any directory
without a `.kern/` folder, so the single global registration is safe across
every project — only directories where a kern is (or has been) active get
touched.

- `kern-recall` injects the `<cwd>/.kern/digest.md` at `SessionStart`.
- `kern-recall-prompt` embeds each prompt (Ollama) and injects semantic hits;
  it fails open on timeout and injects nothing rather than blocking the prompt.
- `kern-capture` spools each session's transcript delta to `<cwd>/.kern/capture/`
  on `Stop`; the daemon drains and distills it.
