# Plan: Executor subagents use Claude instead of OpenAI

> **Parked** with the board. Code paths below reference the in-tree crate
> name `cranyum/`; the project-level name is `board`.

## Context

The board has two LLM roles:
- **Brain / orchestrator** — board-watching agent (`agent/orchestrator/brain.rs`). Already has its own override via `OrchestratorConfig` in `config.rs`.
- **Executors / subagents** — spawned per ticket by `start_task` / `HarnessAdapter::spawn_for_task` (`agent/orchestrator/manager.rs`, `agent/harness/adapter.rs`). They currently inherit the top-level `CranyumConfig.provider` / `model`.

Goal: executors always start tasks on Claude (Anthropic), independent of what the orchestrator or top-level config selects. If the user sets `CRANYUM_PROVIDER=openai`, the brain may use OpenAI but executors still use Claude — unless explicitly overridden.

## Current call chain (verified)

1. `MCP core.rs:415` or `main.rs:359` → `Orchestrator::start_task(...)` (`manager.rs:95`)
2. `start_task_inner` (`manager.rs:115`) builds a `HarnessAdapter` from `HarnessConfig::from_env()` → `from_config(&CranyumConfig::load())` (`adapter.rs:33`).
3. `HarnessConfig` reads top-level `cfg.provider_name()`, `cfg.api_key()`, `cfg.model_name()` — no executor-specific layer.
4. Inside thread: picks `AnthropicProvider` vs `OpenAiProvider` from the `Provider` enum (`adapter.rs:~109`).

`config.rs:153` already defaults non-OpenAI to `claude-sonnet-4-20250514`, and the fallback `Default` for `HarnessConfig` is Anthropic. The only thing that routes executors to OpenAI is a user-set `provider = "openai"` at the top level.

## Design

Mirror the existing `OrchestratorConfig` with an `ExecutorConfig` sub-block so executors can be configured (or defaulted) independently.

```rust
// config.rs
pub struct CranyumConfig {
    ...
    pub orchestrator: Option<OrchestratorConfig>,
    pub executor: Option<ExecutorConfig>,  // NEW
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ExecutorConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
}
```

Executor resolution (new methods on `CranyumConfig`):
- `executor_provider_name()` → `executor.provider` | **"anthropic" (hard default, NOT top-level)**
- `executor_model_name()` → `executor.model` | provider-specific default (`claude-sonnet-4-20250514` for anthropic)
- `executor_max_tokens()` → `executor.max_tokens` | top-level `max_tokens` | 8192
- `executor_api_key()` → same key table as `api_key()` but keyed off `executor_provider_name()`

Key behavioural change: **executor defaults do not fall back to top-level `provider`**. That's the whole point — top-level can be OpenAI, executor stays Claude until the user explicitly opts in via `executor.provider = "openai"` or `CRANYUM_EXECUTOR_PROVIDER=openai`.

Env vars (parallel to orchestrator):
- `CRANYUM_EXECUTOR_PROVIDER`
- `CRANYUM_EXECUTOR_MODEL`
- `CRANYUM_EXECUTOR_MAX_TOKENS`

## Changes

### 1. `cranyum/config.rs`
- Add `ExecutorConfig` struct + field on `CranyumConfig`.
- Add to `from_env` block, mirroring `orchestrator` handling.
- Add to `merge`, mirroring orchestrator merge.
- Add `executor_provider_name / executor_model_name / executor_max_tokens / executor_api_key` methods.

### 2. `cranyum/agent/harness/adapter.rs`
- Add constructor: `HarnessConfig::from_config_executor(cfg: &CranyumConfig)`.
- It reads `cfg.executor_*()` instead of the top-level getters.
- Leave `from_config` / `from_env` alone (still used by any non-executor code path, tests).

### 3. `cranyum/agent/orchestrator/manager.rs`
- In `start_task_inner` (`manager.rs:115`), switch the `HarnessAdapter` construction site to use `HarnessConfig::from_config_executor(&cfg)`.
- Confirm no other call sites spawn executors; `main.rs:359` and `mcp/core.rs:415` both go through `start_task`, so this one change covers both.

### 4. `cranyum/agent/orchestrator/brain.rs` — unchanged
Brain already resolves via `OrchestratorConfig` → falls back to top-level. Leaves current behaviour intact.

### 5. Docs
- Add `ExecutorConfig` + env vars to `AGENTS.md` / README config section.
- State the new invariant: **executors default to Anthropic/Claude even when top-level provider is OpenAI.**

## Tests

- Unit test in `config.rs`: with `provider = "openai"` at top level and no `executor` block → `executor_provider_name() == "anthropic"` and `executor_model_name() == "claude-sonnet-4-20250514"`.
- Unit test: `executor.provider = "openai"` override → executor resolves openai + `gpt-4o` default.
- Unit test: env var `CRANYUM_EXECUTOR_PROVIDER=openai` wins over JSON.
- Existing brain tests should keep passing untouched.

## Risks / open questions

- **API keys:** if the top-level was OpenAI-only, the user may not have `ANTHROPIC_API_KEY` set. `executor_api_key()` will error at `start_task_inner`. Acceptable: surface the error clearly ("executor uses anthropic by default — set ANTHROPIC_API_KEY or override executor.provider"). Don't silently fall through to the top-level provider, that defeats the goal.
- **Migration:** existing users with `provider = "openai"` at top level will see executors flip to Claude on next run. This is the intended behaviour, but worth a release note.
- **Naming:** `executor` vs `subagent` vs `worker`. Codebase uses "executor" (AGENTS.md goal section). Go with `executor` / `ExecutorConfig` for consistency.

## Non-goals

- Not changing the brain provider.
- Not removing OpenAI support.
- Not touching `llm.rs` provider implementations.
