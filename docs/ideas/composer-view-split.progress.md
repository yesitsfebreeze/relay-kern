# Composer/View Split — Execution Progress

Worktree: `.worktrees/composer-view-split`
Branch: `composer-view-split`
Baseline commit: `e734f7a` (gitignore)

| Task | Status | SHA | Notes |
|------|--------|-----|-------|
| 1. View trait + AgentScope + LayoutSplit | ✅ done | `cfd1b1b` | 2/2 tests; `use input::Key;` + NaN comment accepted from review |
| 2. Focus FSM | ✅ done | `d0be095` | 3/3 tests; fallback-arm comment + ladder-reset assert + focus-preserve assert added |
| 3. SubSession | ✅ done | `3a81cce` | 2/2 tests; Vec vs VecDeque comment + `==` assertion tightening |
| 4. Tab | ✅ done | `0ff628f` | 1/1 test; owned-String contract comment added |
| 5. WorkerRegistry | ✅ done | `fa3aecb` | 2 integration tests; `Debug` derive + `Option<WorkerId>` return on `forget` |
| 6. InboxView stub | ✅ done | `fc30f26` | 1/1 test; full lib suite 120 pass |
| 7. HomeView scaffold | ✅ done | `04909ed` | 1/1 test; combined spec+quality review passed |
| 8. Host shell + focus dispatch | in-flight | | |
| 9. SurfApp → HomeView migration | pending | | Heavy — opus + baseline test count |
| 10. Slash routing `/inbox` `/home` `/tab new` | pending | | |
| 11. Prompt hint + focus indicator | pending | | |
| 12. Wire binary entrypoint | pending | | |

**Divergence note:** main tree completed `kernd` → `kern` rename on 2026-04-25. Worktree still on pre-rename state. Not a correctness blocker (repl crate isn't renamed). Rebase/merge conflicts expected at integration time.
