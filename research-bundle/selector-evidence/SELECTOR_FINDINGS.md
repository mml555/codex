# Selector evidence

Three regression tests document specific selector failure modes that
WERE fixed during the branch. Each test names the task prompt, the
prior wrong target, and the expected gold target. They run against
the live `codex-rs` `RepoMap` and lock in the fix.

The Run 8 failure (`agent-eval-excluded`) is the UNRESOLVED follow-up
the report identifies as the next 1–2 day project: symbol-aware
ownership routing.

## Locked-in cases (regression tests pass)

| Test                                              | Task shape                                     | Wrong before fix             | Gold target                              | Commit |
| ------------------------------------------------- | ---------------------------------------------- | ---------------------------- | ---------------------------------------- | ------ |
| `ri_packet_for_area_package_alias_picks_verification_rules` | mentions `"cli"` in a backticked **example** that should NOT route to CLI crate | `cli/...` files              | `verification/src/rules.rs`              | `f56a8c34` (quote-aware tokenizer) |
| `ri_packet_for_directive_marker_picks_renderer_not_bazel` | "directive ... fragment ... marker" inside context-harness | `context-harness/BUILD.bazel` | `context-harness/src/renderer.rs`        | `9b1f5a1e` (within-crate owners + manifest de-prio) |
| `ri_packet_for_pytest_target_picks_python_rules`  | "pytest target" helper inside verification     | `verification/src/command_exec.rs` | `verification/src/python_rules.rs` | `227e682f` (within-crate owners for verification) |

These tests live at `ri_packet_regressions.rs` (copied into this
bundle for reference). The same selector chain runs in production
under `score_file_for_task` in `assembler.rs`.

The three corresponding rendered packets show up in
`ri-packets/run5-area-package-alias.txt` (selector ✓) and
`ri-packets/run7-directive-marker-postfix.txt` (selector ✓ after
fix). `ri-packets/run6-directive-marker-prefix.txt` shows the
SAME task before the within-crate fix — selector picked
`context-harness/BUILD.bazel`. That regression is now locked in by
test #2 above.

## Unresolved case (Run 8)

```
Task: "Add a unit test inside the #[cfg(test)] mod tests block of
       the agent_eval module that asserts classify_result returns an
       AgentEvalResult::Excluded variant whenever
       valid_for_comparison=false..."

Gold target:    codex-rs/context-harness/src/agent_eval.rs
Selector said:  core/README.md (edit target)
                core/config.schema.json (orientation)
                core/gpt-5.1-codex-max_prompt.md (orientation)
                core/gpt-5.2-codex_prompt.md (orientation)
                core/gpt_5_1_prompt.md (orientation)

Likely area:   core
```

Why the selector failed (from `EVAL_REPORT.md`):

> The area-inference path checks `task_targets_crate(strong, "context-harness")`
> which requires both "context" AND "harness" as tokens. Tasks that
> mention only `agent_eval`, `classify_result`, etc. don't match.
> General-area scoring then picks whichever area had the most
> generic-token hits — in Run 8, that was `core`.

The fix shape the report recommends is **symbol-aware ownership routing**:

> Add a signal that, for each task token matching `[a-z][a-z0-9_]+`
> (snake_case identifier) or `[A-Z][a-zA-Z0-9]+` (UpperCamel),
> looks up which files in the RepoMap define or heavily reference
> that symbol, and boosts area inference toward those files' crate.

The Run 8 task names four identifiers that are uniquely owned by
`codex-rs/context-harness/src/agent_eval.rs`:

- `agent_eval` (module name)
- `classify_result` (function defined here)
- `AgentEvalResult::Excluded` (enum defined here)
- `valid_for_comparison` (field defined here)

A symbol → defining-file index would route this task to the right
crate without needing the task to spell out `"context-harness"`.

## Cost of getting it wrong (Run 8 ledger)

Even though the model ignored the bad directive and ran `rg` to
find `agent_eval.rs` itself, the RI arm spent:

- **3,408k tokens** (vs vanilla 676k = **+404%**)
- **499s wall-clock** (vs vanilla 261s = +239s)
- 7 edit commands (vs vanilla's single patch flow)

Bad routing hint did not save the model time; it appears to have
*added* token spend in reconciling the wrong hint with search
results. This is the strongest evidence in the bundle for the
report's "wrong hint is not free" claim.

## Selector code anchors

The selector chain that produced the packets above:

- `codex-rs/context-harness/src/assembler.rs`
  - `score_file_for_task` (top-level file scorer)
  - `area_affinity_adjustment` (per-area boost/penalty by `ownership.primary_area`)
  - `within_crate_owner_match` + `WITHIN_CRATE_OWNERS` table
  - `task_targets_crate` (requires both crate tokens to appear)
- `codex-rs/context-harness/src/task_terms.rs`
  - `build_task_terms` (tokenizer; emits `phrases`, `strong_phrases`,
    `task_outside_quotes_lower`, etc.)
  - The quote-aware path that strips backticked / double-quoted
    spans from `strong_phrases` (commit `f56a8c34`)
- `codex-rs/context-harness/src/renderer.rs`
  - `render_prompt_fragment_with_caps` (final directive rendering)
  - The `HARNESS_MARKER` constant ("Harness repo intelligence:")
- `codex-rs/repo-index/...`
  - `RepoMap` ownership + the `primary_area` heuristic
