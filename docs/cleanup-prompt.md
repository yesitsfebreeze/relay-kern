# Execute cleanup-audit.md

Read `docs/cleanup-audit.md` end-to-end before touching any file. Each
finding has a Location, Evidence, Recommendation, Risk. Honour Risk
ordering: low → medium. Do **not** batch unrelated changes into one
commit.

## Operating rules

- Run `cargo build --workspace` and `cargo test --workspace` after each
  finding. If either breaks, revert the finding's diff and surface the
  failure before moving on.
- Use `split` MCP tools (`open_source`, `read_body`, `write_body`,
  `find_large`, `search_bodies`) for navigation and edits over Read/Edit
  whenever feasible. Bodies stitch back to `.rs` automatically.
- One commit per finding. Conventional Commits format. Reference the
  finding ID in the body (e.g. `refactor(journal): remove duplicate
  history+entry — audit A1`).
- Skip any finding marked **OBSOLETE** if the verification table is still
  present. (Already pruned from current audit.)
- For each finding, before editing:
  1. Re-verify Evidence via `read_body` / `git status` / `Glob`. If
     evidence no longer matches, mark `cleanup-audit.md` and skip.
  2. State (in chat) the planned diff in 1–2 sentences.
  3. Apply.
  4. Run targeted tests (the affected crate's `cargo test -p <crate>`).
  5. Commit.

## Phase order (override audit's stated order; this is concrete)

### Phase 1 — pure subtraction (low risk)

1. **A1** — delete `src/bin/agnt/src/journal/history.rs` and
   `src/bin/agnt/src/journal/entry.rs`. Re-export from
   `journal::*` (the shared crate). Update `agnt::journal` mod tree.
   Verify: `cargo build -p agnt` + journal tests still pass.

2. **#17** — decide on `src/bin/agnt/src/goal.rs`: read it; if wired in,
   commit; if orphan, delete.

3. **A3** — locate canonical home for `run_turn_with_retry`. If `.fs` is
   the only copy, rename to `.rs` and let split re-derive. Add
   `*.fs` (outside `.split/`) to `.gitignore`.

4. **#18** — add `docker/out/` to `.gitignore`.

5. **#19** — checkpoint commit of in-flight modifications listed in
   `git status` (`LAYOUT.md`, theme files, etc.) **before** any further
   refactor lands. Surface to user first; do not auto-commit unfamiliar
   diffs.

### Phase 2 — targeted dedup (low risk)

6. **#1** — `register_inproc` / `spawn_stdio` tail. Extract
   `fn install_transport(&mut self, id, transport: Box<dyn Transport>) ->
   Result<&LiveServer>`. Replace `Entry::Occupied(_) => unreachable!()`
   with a real error variant. Update both callers.

7. **#2** — drop `tee::abi_version` override (rely on default trait
   impl).

8. **#16** — fix `unreachable!()` outside tests:
   - `run_turn_with_retry`: convert to `while attempt < MAX { ... }` +
     trailing `TurnComplete { fork_id: None, result: Err("retry attempts
     exhausted".into()) }`.
   - `auth/interactive/wizard.rs:49`: replace with `debug_assert!` plus
     real default arm. Match arms `chat..background_agent` are exhaustive
     for `role_labels[idx]` already; the `_` arm is dead — collapse the
     match into a lookup table.
   - `registry::{register_inproc, spawn_stdio}`: covered by #1.

9. **#4** — inline `parse_head` and `is_slash` into `run_slash`. Keep
   `canonicalise`. Add unit test on `run_slash`.

10. **#8** — drop `/thread`, `/plugins`, `/capabilities`, `/state` from
    `repl::commands::builtin`. Don't `cfg!(debug_assertions)` them — just
    remove. They reappear when implemented.

11. **#9** — wrap `cmd_peers` `println!` in
    `unimplemented_subcommand("federation")` helper.

12. **A5** — extract `fn info(name, summary) -> PluginInfo` shared helper
    for `kern_plugin`, `fs_plugin`, `fs_inproc`, `dev_plugin`.

### Phase 3 — bigger collapses (medium risk)

13. **A2** — JSON-RPC envelope unification. Three sub-steps:
    a. Add `pub fn run_stdio_server<S: Server>(server: S)` to
       `shared/trnsprt`. Inline the `main` body shared across plugins.
       Add `pub fn write_frame`, `parse_frames`, `ok_response`,
       `error_response` if not already exposed.
    b. Replace each plugin `main()` (`fs`, `intro`, `relay-plugin`,
       `ask-bubble`, `clock`, `echo`, `search`) with a 3-line
       `fn main() { trnsprt::run_stdio_server(MyServer::new()) }`. Drop
       per-plugin `dispatch` if it adds nothing beyond the canonical
       handshake/list/call switch.
    c. Replace `agnt::mcp_server::serve` and `repl::mcp_server::serve`
       with calls into the shared driver, parameterised on
       `handle_tools_call`.
    d. Delete `kern::mcp::ok` and `kern::mcp::err_resp` once kern's MCP
       server uses the shared helpers.

    Test each sub-step independently. Do **not** roll all four into one
    commit.

14. **A4** — extract `FieldRecorder { message, fallback }` with
    `record_*` impls into `shared/journal`. `trace_bridge` composes it.

15. **#10** — ABI mismatch warning. Two options; **prefer hard error**
    (no external plugins yet). Keep test by flipping its assertion to
    expect an `Err`. If user prefers soft path, drop the warning and the
    `abi_version` field entirely.

### Phase 4 — oversized fns (opportunistic)

16. **#11** `edit_area::handle_input` — only attempt if `LAYOUT.md`
    work has settled. Split into `handle_form_active`,
    `handle_list_active`, `handle_normal_key`.

17. **#13** `repl::commands::builtin` — hoist each lambda to named fn.
    Body collapses to a flat `cmd!()` list.

18. **#14** `textarea::form::render` — pull `FormField::{Input,Picker}`
    arms into helpers.

19. **#15** `agnt::fs_inproc::list_tools` — extract const tool table.
    Same shape as #6.

20. **#6** `kern::mcp::tools::tool_definitions` — extract `ToolSpec`
    table or migrate to `schemars`. Lower priority.

21. **#12** kern algorithmic fns (`do_cluster`, `expand`, `forward_graph`,
    `run_server`) — read each in full first; only split when step
    boundaries are obvious. Bench before/after on `do_cluster` and
    `expand`.

22. **A6** `dev_plugin` `handle_*` table — only refactor if the parsing
    surface is uniform. If signatures differ, leave alone.

## Stop conditions

- Stop the run if `cargo build` breaks and the cause is unclear after one
  attempt to revert.
- Stop if a finding's Evidence no longer matches the tree (mark obsolete
  in `cleanup-audit.md`, move on).
- Stop after Phase 2 and surface a status report before entering Phase 3
  — Phase 3 changes touch every binary.

## Reporting

After each phase:
- List findings completed with commit SHAs.
- LoC delta (`git diff --stat <phase-start>..HEAD`).
- Test-suite pass/fail summary.
- Any findings demoted to OBSOLETE during verification.
