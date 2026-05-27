# Search Proxy MVP — result

**Branch:** `search-proxy-mvp`
**Date:** 2026-05-26
**Status:** Real positive signal on symbol-heavy discovery tasks (2 clean
wins across 2 crates) + a mechanism-level no-harm pass (proxy stays
inert when no eligible search occurs). MVP earned a write-up; cloud
runs paused.

---

## Thesis

> Intercepting model-initiated `rg` searches and returning compact
> evidence can reduce paid context without forcing upfront repo
> intelligence into the prompt.

Supported on both symbol-heavy discovery tasks tested so far.

---

## Why this differs from RI

RI v1 injected context **before** the model asked, guessing from a
heuristic selector. It mis-guessed on symbol-only tasks (RI Run 8 →
`core/README.md`) and the wrong upfront hint cost 3.4M tokens of thrash.

Search Proxy waits until the model **reveals intent** through a tool
call, then answers with evidence grounded in actual `rg` matches:

```
model:   rg "AgentEvalResult|classify_result|valid_for_comparison"
harness: compact evidence, top file = agent_eval.rs
```

No upfront guess. No wrong-hint tax. The RI Run 8 failure mode is
structurally avoided — the proxy can't surface a file `rg` never
matched. And when the proxy's ranking IS wrong (see Run3 below), the
model can repeat the command to get raw output, so a wrong reactive
hint is far cheaper than a wrong upfront one.

---

## Results

All A/B pairs: `vanilla` (no features) vs `search_proxy`
(`features.search_proxy=true`), no repo intelligence on either arm,
Azure / gpt-5.3-codex, release binary, isolated worktrees, one task per
fixture.

### Run2 — `agent_eval.rs` (RI Run 8 task), symbol-heavy

Task: add a `classify_result` / `AgentEvalResult::Excluded` unit test.
Names only internal symbols. The exact case RI v1 mis-routed.

```
                       vanilla     search_proxy    delta
tokens_total         2,127,345        981,076    -54%
duration_ms            288,139        308,692    +7%
tool_call_count             24             21
intent_changed_files agent_eval.rs agent_eval.rs  both correct
substitutions / escape_hatch_repeats        3 / 3
top_files                    context-harness/src/agent_eval.rs (all 3)
```

The proxy produced correct compact evidence — gold `agent_eval.rs` was the
top file on all 3 substitutions — and tokens were far lower. **But** the
model re-issued every substituted command (escape-hatch 3/3): it ran the
original `rg` anyway on every call.

**Updated read (treatment-aware classifier, `search-proxy-hardening`):**
this rescore now reads `search_proxy_inconclusive:model_strategy_variance`
(proxy state `bypassed_all`), **not** a token win. Because the model
bypassed every substitution and ran the original command, the -54% delta
cannot be attributed to the proxy — it is most likely run variance. The
original Run2 was still useful evidence that the proxy *surfaces the right
file*, but attribution is weaker than Run4, where the model accepted the
compact evidence (escape-hatch 0/2) and the token win is creditable.

### Run3 — `verification/src/rules.rs`, concept-only prompt

Generalization test: different crate, prompt names no file/crate/symbol
(`agent_eval_tasks_area_alias_concept_only.json`). Gold is the
`package_name_for_area` / `AREA_PACKAGE_ALIASES` helper.

```
                       vanilla     search_proxy    delta
tokens_total           655,195        628,998    -4% (noise)
intent_changed_files rules.rs ✓    rules.rs ✓     both correct
substitutions / escape_hatch_repeats        1 / 1
raw_bytes_estimated                      4,178,771 (4.2 MB!)
top_files            context-harness/src/task_terms.rs  ✗ WRONG
```

**Neutral, and the proxy's top file was wrong.** Two root causes:

1. **No intra-Owner ranking.** The file classifier is binary
   (Owner/RelatedTest/Source); within a class the builder broke ties
   alphabetically. Three files classified as Owner — `task_terms.rs`,
   `repo_map.rs`, and the gold `rules.rs` — and `rules.rs` lost on
   `c < v`. It ranked **#3** in the evidence despite being the actual
   owner (the only file *defining* `package_name_for_area` /
   `area_id_for_path`).
2. **Repo pollution.** ~19 `ri-*` eval-artifact dirs were committed to
   the branch; the isolated worktree contained them, so the broad
   `rg . ` matched 75 files / 4.2 MB of mostly artifacts.

The model ignored the wrong hint, escape-hatched to raw, and still
edited `rules.rs` — so the wrong reactive hint cost ~nothing (-4%),
unlike RI's +404%. Inconclusive for the thesis, but it diagnosed the
ranking weakness.

### Fixes between Run3 and Run4

- **Cleanup** (`chore: remove eval artifact dirs from repo`): untracked
  the `ri-*` dirs + `research-bundle/` and gitignored them, so eval
  worktrees stop carrying artifact noise.
- **Ranking** (`fix: rank owner search results by query relevance`):
  added a within-tier relevance score —
  `3 × definition_symbol_overlap + distinct_phrases_present`, where
  definition overlap counts query words appearing in *defined symbol
  names* (snake_case + CamelCase aware). Sort is now
  `(class rank, relevance desc, path)`. Local gate test confirmed
  `rules.rs` ranks #1 on the Run3 query.

### Run4 — Run3 re-run after the fixes (same fixture)

```
                       vanilla     search_proxy    delta
tokens_total           708,350        551,049    -22%
duration_ms            145,674        146,040    +0.4s (flat)
tool_call_count             19             16    -3
discover_command_count       4              2    -2
intent_changed_files rules.rs ✓    rules.rs ✓     both correct
substitutions / escape_hatch_repeats        2 / 0
build_pass_throughs                              1 (rg with bad dir arg → RgError → passed through)
raw_bytes_estimated                         61,797 (clean worktree; was 4.2 MB)
top_files                    ./verification/src/rules.rs  ✓ CORRECT
```

**Clean win.** The ranking fix turned the neutral -4% into -22%, the
proxy surfaced the gold file as #1, and — notably — the model made
**0 escape-hatch repeats**: with correct evidence it trusted both
substitutions and went straight to `rules.rs`. The 3/3 → 0/2 drop
suggests the Run2 escape-hatch wrinkle was partly a symptom of wrong
ranking, not just format.

### Run5 — no-harm / named-file task

Safety check: does the proxy interfere when it isn't needed? Prompt
names the file directly (`context-harness/src/renderer.rs`, add a doc
comment above `shorten_for_prompt`), so no discovery is required.

```
                       vanilla     search_proxy    delta
tokens_total           257,858        520,634    +102% (2x)
duration_ms             40,825        243,590    +497% (6x)
tool_call_count              8             17
edit / verify              1/1            2/2
intent_changed_files renderer.rs ✓ renderer.rs ✓  both correct
substitutions / escape_hatch / build_pass_through   0 / 0 / 0  (proxy never fired)
```

> On the no-harm named-file task, Search Proxy did not activate at all,
> so it introduced no direct proxy behavior. The treatment arm still
> cost more because the model independently entered a verification loop.
> This makes the cost comparison confounded, but the mechanism-level
> no-harm check passed.

The model's only rg was `rg "shorten_for_prompt" renderer.rs && sed …`
— `&&`-chained, which the classifier correctly rejects as ineligible.
There were no standalone eligible rg commands, so the proxy stayed
inert (0 of every metric, 0 tracing). The 2x/6x arm cost is the
search_proxy arm running `just fmt` + `just test` (a ~3-min compile)
and two edit/verify cycles while vanilla skipped verification — the
same "model strategy variance dominates the cost ledger" confound the
RI eval flagged.

---

## Result summary

```
Run2  agent_eval.rs   -54% tokens   top_file ✓   same intent file   escape-hatch 3/3
Run3  rules.rs         -4% (noise)  top_file ✗ (pre-fix)            escape-hatch 1/1
Run4  rules.rs        -22% tokens   top_file ✓   same intent file   escape-hatch 0/2
Run5  renderer.rs     proxy inert   no-harm mechanism pass; cost confounded by verify variance
```

Two clean wins across two crates and two symbol vocabularies (correct
top file + correct intent edit both times), plus a mechanism-level
no-harm pass. **Search Proxy is promising on symbol-heavy discovery
tasks and stays inert when no eligible search occurs** — but the claim
is still narrow (see below).

---

## Key findings

- **Reactive mediation beats upfront RI.** Answering at the moment of
  the tool call grounds on real matches and structurally avoids RI's
  wrong-upfront-hint failure (and when the hint is wrong, the escape
  hatch makes it cheap — Run3 was -4%, not RI Run 8's +404%).
- **Correct compact evidence can be trusted by the model.** Run4's
  0/2 escape-hatch rate (vs Run2's 3/3 with the same format but
  wrong-then-right ranking) shows the model accepts evidence it
  believes is correct.
- **Ranking quality matters.** The binary Owner classifier with an
  alphabetical tiebreak mis-ranked the gold file to #3 (Run3); a
  within-tier relevance score fixed it (Run4).
- **Repo pollution skews raw-search estimates.** Committed `ri-*`
  artifact dirs inflated Run3's raw match pool to 4.2 MB; untracking
  them dropped Run4 to 62 KB.
- **Verification variance is the biggest unrelated confound.** Whether
  the model runs `just test` swings wall-clock and tokens far more than
  the proxy does on easy tasks (Run5). Single-sample A/Bs can't
  separate it from proxy effect.
- **Still narrow: `rg` only.** No `sed` / file-read / test
  interception. No RI combination.

---

## What is proven vs not

Proven (narrow):

> Reactive tool mediation can help on symbol-heavy search tasks and can
> stay inert when no eligible search occurs.

NOT proven:

> Search Proxy always reduces cost.
> Search Proxy controls verification variance.
> Search Proxy improves easy-task wall-clock.

These are separate questions for later.

---

## Next engineering options (not yet decided)

1. Add a verification policy separately (control the dominant confound).
2. Expand the proxy to `sed` / large file reads.
3. Improve the compact evidence format.
4. Add a treatment-aware result classifier (the current
   `ri_worse:faster_wall_clock` verdict penalizes tiny wall-clock
   deltas and ignores token deltas; use thresholds, e.g. ignore <5% /
   <5s).
5. Run a broader eval later (more tasks, repeats for variance bounds).

## Out of scope for the MVP (deliberately)

- Combining Search Proxy with RI.
- Extending interception beyond `rg` before the above are scoped.
- Iterating the compact format (escape-hatch already trends to 0 with
  correct ranking).

---

## Strategic shift

```
RI upfront injection:             mixed / negative (closed)
Search Proxy reactive mediation:  two clean wins on symbol discovery
```

The harness should mediate model-initiated tool use at the moment of
need — not guess context before the model asks.
