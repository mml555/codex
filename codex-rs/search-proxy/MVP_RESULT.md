# Search Proxy MVP — interim result

**Branch:** `search-proxy-mvp`
**Date:** 2026-05-26
**Status:** First positive signal. One task, one valid A/B. Not yet generalized.

---

## Thesis

> Intercepting model-initiated `rg` searches and returning compact
> evidence can reduce paid context without forcing upfront repo
> intelligence into the prompt.

Supported on the one task tested so far.

---

## The result (one A/B pair)

Task: `targeted_test_agent_eval_excluded` — the exact Run 8 task from
the closed RI experiment (add a `classify_result` /
`AgentEvalResult::Excluded` unit test to `agent_eval.rs`). The task
names only internal symbols, so discovery is non-trivial — this is the
case RI v1 mis-routed to `core/README.md`.

Arms: `vanilla` (no features) vs `search_proxy`
(`features.search_proxy=true`). No repo intelligence on either arm.
Provider Azure / gpt-5.3-codex, release binary, isolated worktrees.

```
                          vanilla        search_proxy     delta
tokens_total            2,127,345          981,076       -1,146,269 (-54%)
duration_ms               288,139          308,692          +20,553 (+7%)
tool_call_count                24               21
discover/read/edit/verify  3/0/1/1          4/1/1/1
intent_changed_files    agent_eval.rs    agent_eval.rs    same gold file
run_valid                    true             true
```

Search-proxy interception metrics:

```
substitutions:              3
escape_hatch_repeats:       3
build_pass_throughs:        0
compact_bytes:          3,389
raw_bytes_estimated:   41,218   (compact saves 92% per substituted call)
top_files:  context-harness/src/agent_eval.rs  (all 3 substitutions)
```

The three intercepted searches:

```
1. rg -g*.rs "mod agent_eval|classify_result|AgentEvalResult"   8 files, 87 hits
2. rg "classify_result|AgentEvalResult|mod tests|valid_for_comparison" \
     context-harness/src/agent_eval.rs                          1 file, 46 hits
3. rg "mod tests" context-harness/src/agent_eval.rs             1 file,  1 hit
```

All three top-ranked `agent_eval.rs`, the gold owner.

---

## Why this differs from RI

RI v1 injected context **before** the model asked, guessing from a
heuristic selector. It mis-guessed on symbol-only tasks (Run 8 →
`core/README.md`) and the wrong hint cost 3.4M tokens of thrash.

Search Proxy waits until the model **reveals intent** through a tool
call, then answers with evidence grounded in actual `rg` matches:

```
model:   rg "AgentEvalResult|classify_result|valid_for_comparison"
harness: compact evidence, top file = agent_eval.rs
```

No upfront guess. No wrong-hint tax. The Run 8 failure mode is
structurally avoided — the proxy can't surface `core/README.md` for
this task because `rg` never matched it.

---

## The wrinkle

The model repeated **every** intercepted command (substitutions=3,
escape_hatch_repeats=3). It did not fully trust the compact evidence —
each substitution was followed by a re-issue to get raw `rg` output.

The A/B still won by 54% tokens, because:
- the compact form (3.4 KB total) is far smaller than raw (41 KB), so
  even the "teaser then raw" pattern moves less text on the first pass;
- the compact evidence shaped what the model grepped for next (the
  searches narrow from repo-wide to `agent_eval.rs`-scoped).

But the 3/3 repeat rate is the **main thing to improve**: compact
evidence is currently treated as a pointer, not a satisfying answer.

---

## What is NOT proven

- **Generalization.** One task. Need at least one more symbol-heavy
  task to confirm the win isn't task-specific.
- **No-harm on easy cases.** Haven't tested a task where the model
  wouldn't have wasted search effort anyway — the proxy must not make
  those worse.
- **Wall-clock.** +7% here (the proxy runs a bounded internal `rg`
  before answering). Acceptable at this token delta, but worth watching.
- **The classifier verdict.** The eval's comparison logic scored this
  pair `ri_worse:faster_wall_clock` — it penalizes the +20s and ignores
  the -54% tokens. The verdict heuristic predates a token-dominant
  treatment and should be revisited.

---

## Next steps (in order)

1. ~~Fix the `search_proxy` treatment arm in the scorer~~ (done — the
   score subcommand now renders + prints a Search Proxy section).
2. This note.
3. One more cloud pair on a **different** symbol-heavy task — does the
   win generalize?
4. Only if that also looks positive: iterate the compact format to
   reduce escape-hatch repeats (more raw lines? line ranges? a short
   definition snippet inline?).

## Out of scope (deliberately)

- Combining Search Proxy with RI.
- Extending interception to `sed` / `cat` / `find` / tests.
- Optimizing the compact format before generalization is shown.

---

## Strategic shift

```
RI upfront injection:        mixed / negative (closed)
Search Proxy reactive mediation:  first strong positive
```

The harness should mediate model-initiated tool use at the moment of
need — not guess context before the model asks.
