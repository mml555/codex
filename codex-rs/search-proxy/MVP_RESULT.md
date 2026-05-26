# Search Proxy MVP — result

**Branch:** `search-proxy-mvp`
**Date:** 2026-05-26
**Status:** Real positive signal on symbol-heavy discovery tasks (2 clean
wins across 2 crates). No-harm/easy-case behavior not yet tested.

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

Clear token win. **But** the model re-issued every substituted command
(escape-hatch 3/3) — it treated compact evidence as a pointer, not a
satisfying answer. Flagged as the main thing to improve.

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

---

## Conclusion

```
Run2  agent_eval.rs   -54%   top_file ✓   escape-hatch 3/3
Run4  rules.rs        -22%   top_file ✓   escape-hatch 0/2
```

Two clean wins, two different crates, two different symbol vocabularies,
correct top file and correct intent edit both times. **Search Proxy is
promising on symbol-heavy discovery tasks** — the kind where the model
must locate an owner file from concept/symbol language. It both reduces
paid context and avoids the wrong-upfront-hint failure that sank RI.

---

## Caveats / not yet proven

- **No-harm on easy cases (the open question).** Both wins were tasks
  where discovery mattered. We have NOT tested a task where the prompt
  already names the file — the proxy must not tax runs that don't need
  it. This is the next eval.
- **Wall-clock.** +7% (Run2) / flat (Run4). The proxy runs a bounded
  internal `rg` before answering. Acceptable so far; watch it.
- **The result classifier verdict.** The eval still prints
  `ri_worse:faster_wall_clock` because the comparison logic penalizes
  tiny wall-clock deltas and ignores token deltas. Misleading for a
  token-dominant treatment. Fix later with a treatment-aware classifier
  and thresholds (ignore <5% / <5s deltas); raw metrics are clear
  enough for now.
- **Single-run samples.** One A/B per task; no repeat-variance bounds.

---

## Next eval (decided)

**No-harm / easy case.** A task whose prompt names the file directly
(e.g. "in `context-harness/src/renderer.rs`, add a doc comment above
`HARNESS_MARKER` …"). Pass = the proxy stays out of the way:

```
search_proxy_substitutions = 0 or harmless
intent_changed_files same
tokens / duration / tool calls not materially worse
```

If the proxy activates unnecessarily and worsens an easy run, that's a
real problem to fix before going further.

## Out of scope (deliberately, until no-harm is shown)

- Combining Search Proxy with RI.
- Extending interception to `sed` / `cat` / `find` / tests.
- Iterating the compact format (escape-hatch is already trending to 0
  with correct ranking).
- A third symbol-heavy task (enough generalization evidence for now).

---

## Strategic shift

```
RI upfront injection:             mixed / negative (closed)
Search Proxy reactive mediation:  two clean wins on symbol discovery
```

The harness should mediate model-initiated tool use at the moment of
need — not guess context before the model asks.
