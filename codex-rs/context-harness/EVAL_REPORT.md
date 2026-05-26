# Repo Intelligence Eval — Result Report

**Branch:** `harness-core-pr1` (53 commits ahead of `main`)
**Date:** 2026-05-25
**Status:** Experiment closed. No more cloud runs planned on this branch.

---

## Headline

> Native repo intelligence is mechanically feasible, but the current
> selector is too brittle and the cost/time wins do not hold across
> frontier-model evals.

The eval harness built during this work is the most reusable artifact.
The selector / context-injection design needs symbol-aware routing
before it's worth measuring at this scale again.

---

## Verdict matrix

| Claim                                       | Status               |
| ------------------------------------------- | -------------------- |
| Native RI injection works mechanically      | proven               |
| Eval harness produces trustworthy metrics   | proven               |
| Intent-file scoring works                   | proven               |
| Validity / warning classification works     | proven               |
| Release-mode + cached-map prewarm works     | proven               |
| Selector routes correctly on most tasks     | partial              |
| Selector routes correctly on all task forms | **NOT proven**       |
| RI reduces tokens reliably                  | **NOT proven**       |
| RI reduces wall-clock reliably              | **NOT proven**       |
| RI improves discovery reliably              | **NOT proven**       |
| Bad routing hint is harmless                | **NOT proven** (Run 8 disaster)|

---

## What was built (the durable contribution)

The eval machinery is independent of whether RI itself is shippable.
Each component below is tested, used by the eval, and could be reused
or upstreamed individually.

### Eval harness (`scripts/harness-agent-eval.sh`)

- Vanilla vs RI A/B runner with isolated `git worktree` arms.
- `--rescore-artifacts` mode for reclassifying existing records without
  re-running codex.
- Shared repo-index prewarm + cache (`codex context dump-repo-index`)
  threaded into each arm via `CODEX_REPO_INTELLIGENCE_CACHED_MAP`.
- Release-binary resolution by default; debug-mode amplification
  measured at 5–7× on `build_context_packet` and tracked via the
  `codex_build_profile` record field.

### Record schema (`AgentRunRecord` in `context-harness/src/agent_eval.rs`)

Per-arm fields useful for any future agent eval, RI-related or not:

- `intent_changed_files` — paths the model authored via `file_change`
  / `apply_patch`, NOT `git diff`. The original `git diff`-based scoring
  was contaminated by `cargo fmt --all` collateral; intent-file scoring
  isolates real model intent.
- `diff_changed_files`, `formatter_changed_files` — diagnostic split
  so reviewers can see how much collateral a run accumulated.
- `ri_surfaced_edit_targets`, `ri_surfaced_orientation` — what the
  RI directive proposed, parsed back out of the rollout's user message.
- `harness_prewarm_ms` — local one-time cost amortized across the batch.
- `codex_build_profile` — prevents debug-vs-release wall-clock mixing.
- `warnings` — non-fatal observations like
  `provider_network_error_recovered`.
- `verification_required` (on `AgentEvalTask`) — fixture-level intent
  for splitting verification-required from verification-optional tasks
  in reports.

### Validity classifier

Three-tier validity model in `scripts/harness-agent-eval.sh` and
`AgentRunInvalidReason`:

- Behaviorally valid (terminal `turn.completed`, `exit 0`)
- Valid with warnings (e.g. `provider_network_error_recovered` —
  recovered network blip mid-stream)
- Invalid (workspace_limit, auth_error, turn_failed, missing_events)

### Phase instrumentation

`classify_shell_phase` heuristic + matching bash mirror. Each shell
command classified as discover / read / edit / verify / other.
`apply_patch` / `file_change` items count as edit-phase activity
(otherwise frontier models that use `apply_patch` instead of `sed -i`
render as `e=0`).

### Selector chain (the part that's *not yet ready*)

Six commits worth of file ranking improvements, each fixing a
specific failure exposed by a packet check:

1. `intent_changed_files` (commit `704ae82`) — scoring uses model
   intent, not `git diff` noise.
2. Edit-targets / orientation split in the directive (commit `24d3931`)
   — gives the model a single "primary" file plus context.
3. Generalized crate-affinity boost (commit `93f9cac7`) — replaces
   the hardcoded `+0.45` context-harness and `+0.35` cli special
   cases with `area_affinity_adjustment` keyed on
   `ownership.primary_area`.
4. Within-crate ownership table for verification (commit `227e682f`)
   — fixes `pytest-target` picking `command_exec.rs` over
   `python_rules.rs`.
5. Quote-aware area inference (commit `f56a8c34`) — `terms.strong_phrases`
   and `task_outside_quotes_lower` so backticked example strings
   don't pollute area selection.
6. Within-crate ownership for context-harness + manifest
   de-prioritization (commit `9b1f5a1e`) — fixes `directive-marker`
   picking `BUILD.bazel` over `renderer.rs`.

All six have unit tests + live-map regression tests in
`context-harness/tests/ri_packet_regressions.rs`.

---

## What was tested (release-mode pairs)

Eight cloud pairs total against Azure / gpt-5.3-codex. The first
four pairs used a debug binary and uncached / partially-cached
index; their wall-clock numbers were 5–7× inflated by harness cost
and aren't directly comparable. The four release-mode + cached
pairs are the experimental result.

```
Run  Task                              Selector  Tokens V/RI       Δ       Wall-clock V/RI   Δ
─── ────────────────────────────────  ────────  ───────────────  ─────── ───────────────── ───────
 5   area-package-alias                ✓ gold    607k / 387k       -36%    105s / 121s        +16s
 6   directive-marker (BUILD.bazel)    ✗ wrong   882k / 865k       -2%     278s / 319s        +41s
 7   directive-marker (renderer.rs)    ✓ gold    247k / 904k       +267%   35s / 394s         +358s
 8   agent-eval-excluded (core/...)    ✗ wrong   676k / 3,408k     +404%   261s / 500s        +239s
```

**One RI token win** (Run 5), **one effective tie** (Run 6),
**two RI losses** (Runs 7 + 8). The token deltas range from −36%
to **+404%** on otherwise similar tasks — sample variance is
larger than any directional signal.

---

## Findings

### 1. Selector works in easier cases, fails on internal symbols

The selector lands the right file when the task names the crate
or strong path tokens:

```
"Inside the verification crate ..."         → verification/* ✓
"Add a `// Sentinel ...` doc above its def" → renderer.rs ✓
```

It fails when the task names only **internal symbols** that exist
inside a crate:

```
"agent_eval module ... classify_result ... AgentEvalResult::Excluded"
                                          → core/README.md ✗
```

The area-inference path checks `task_targets_crate(strong, "context-harness")`
which requires both "context" AND "harness" as tokens. Tasks that
mention only `agent_eval`, `classify_result`, etc. don't match.
General-area scoring then picks whichever area had the most
generic-token hits — in Run 8, that was `core`.

This is the SAME shape of bug as the verification within-crate
issue (commit 227e682f) and the quote-pollution bug (commit
f56a8c34), but it operates one level up: the AREA inference itself
needs to understand that `agent_eval` is a context-harness module.

### 2. Bad routing hint can harm, not just be neutral

Run 8 is the clearest evidence: RI surfaced `core/README.md` for a
task about `agent_eval.rs`. The model ignored the directive and
found `agent_eval.rs` anyway via `rg`. But then the RI arm spent
**3.4M tokens** thrashing through 8 patch attempts, vs vanilla's
single patch and 676k tokens.

The model's loop with the bad directive present produced a
different — worse — workflow. Mechanism unclear (perhaps the
model spent extra reasoning reconciling the wrong hint with its
search results), but the outcome is reproducible.

**Implication:** RI must be conservative. A wrong hint isn't free
— it changes downstream model behavior in costly ways.

### 3. Model strategy variance dominates the cost ledger

Run 7 (correct selector) made the model run `just test`, hit a
regression, and spend 4 minutes in a fix-test cycle. Vanilla
skipped verification entirely on the same task and finished in 35s.
The 11× wall-clock difference was driven by which arm chose to
enter the test cycle, not by what RI did.

The `verification_required` field was added to fixtures to control
for this, but tagging is intent metadata — it doesn't change model
behavior. The model decides independently whether to verify, and
the decision interacts with the RI directive in ways we don't yet
understand.

### 4. The eval harness is the strongest deliverable

The instrumentation work paid off repeatedly:

- The intent-file fix retired the entire "RI broadens scope to 7
  files" narrative — that failure was `cargo fmt --all` collateral,
  not real model behavior.
- The harness-init / model-loop / contribute() timing split
  pinpointed the wall-clock penalty as harness setup, not model
  deliberation.
- The release-mode binary fix dropped wall-clock penalties by ~70s.
- Three regression tests in `ri_packet_regressions.rs` lock in
  specific selector behaviors.

These are durable improvements regardless of what happens to RI
itself.

---

## What's worth carrying forward

### Strong candidates for keeping or upstreaming separately

- **Eval harness ideas.** The intent-file / diff-file / formatter-file
  triplet, validity-with-warnings, phase classification, three-number
  wall-clock split (`harness_prewarm_ms` / `duration_ms` / model loop),
  and the prewarmed-index pattern.
- **Intent-file scoring.** The single biggest correction during the
  experiment — should be the default for any future agent eval, RI
  or not.
- **Validity classifier with recovered-warning tier.** Distinguishes
  behaviorally valid runs that survived transient network blips from
  hard failures.
- **Narrow verification safety.** The `narrow_verification_hint`
  scaffolding is small, optional, and well-bounded.

### Not ready to propose

- **RI session injection as a Codex feature.** The selector is
  brittle on tasks that name internal symbols. Wrong hints cost
  more than no hints. Cost/time wins don't generalize.

### Open follow-up if the project continues

The clearest engineering project is **symbol-aware ownership routing**.
Add a signal that, for each task token matching `[a-z][a-z0-9_]+`
(snake_case identifier) or `[A-Z][a-zA-Z0-9]+` (UpperCamel),
looks up which files in the RepoMap define or heavily reference
that symbol, and boosts area inference toward those files' crate.

That's a 1–2 day project on top of the existing infrastructure.
It would address the Run 8 failure mode directly. It is **NOT**
done in this branch.

---

## Where things live

| Component                        | Path                                                                |
| -------------------------------- | ------------------------------------------------------------------- |
| Eval runner                      | `scripts/harness-agent-eval.sh`                                     |
| Record schema                    | `context-harness/src/agent_eval.rs::AgentRunRecord`                 |
| Score schema                     | `context-harness/src/agent_eval.rs::AgentRunScore`                  |
| Selector / ranker                | `context-harness/src/assembler.rs::score_file_for_task`             |
| Within-crate owners              | `context-harness/src/assembler.rs::WITHIN_CRATE_OWNERS`             |
| Quote-aware tokenizer            | `context-harness/src/task_terms.rs::build_task_terms`               |
| Directive renderer               | `context-harness/src/renderer.rs::render_prompt_fragment_with_caps` |
| RI extension                     | `ext/repo-intelligence/src/extension.rs`                            |
| Cache-loading env var            | `CODEX_REPO_INTELLIGENCE_CACHED_MAP`                                |
| Per-stage contribute() bench    | `ext/repo-intelligence/examples/bench_contribute.rs`                |
| Live-map regression tests        | `context-harness/tests/ri_packet_regressions.rs`                    |
| Single-task gated fixtures       | `context-harness/tests/fixtures/agent_eval_tasks_*_only.json`       |

---

## Final status

```
Mechanism:    proven
Eval harness: proven (durable contribution)
Selector:     brittle (works on crate-name tasks, fails on symbol-name tasks)
Economics:    not proven (token/time deltas dominated by variance)
Cloud result: mixed/negative across 4 release-mode pairs
Next step:    write-up complete; no more cloud runs planned
```

If work continues, the next engineering project is **symbol-aware
ownership routing**. Until that lands, RI session injection should
not be proposed as a Codex feature — wrong routing hints
demonstrably cost more than no hints (Run 8).
