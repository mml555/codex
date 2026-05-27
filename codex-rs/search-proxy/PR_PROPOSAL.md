# PR proposal — feature-gated Search Proxy (experimental)

Draft PR description for landing Search Proxy behind `Feature::SearchProxy`
(default off). Deeper readiness analysis: `MERGE_CANDIDATE.md`.

## Summary

This adds a feature-gated search-mediation layer for `rg` calls. It is
read-only, default-off, escape-hatch protected, and instrumented, and it has
early evidence of reducing token cost on symbol-heavy discovery tasks. It is
**not** proposed as a default-on or proven-across-the-board improvement — the
ask is to land it behind a flag for continued controlled evaluation.

## Design

- Intercepts the first eligible `rg` invocation the model issues.
- Runs a bounded internal `rg` and returns compact, ranked evidence (likely
  owner file + a few sample lines) instead of raw output.
- Exact repeat of the same command bypasses the proxy → raw `rg` output.
- Read-only; feature-gated on `Feature::SearchProxy` (default off).
- One interception point, wired in both `shell.rs` and
  `unified_exec/exec_command.rs`.

## Safety properties

- Default off.
- No writes; no arbitrary command execution — the proxy only runs `rg`.
- Unsafe / chained commands (shell metacharacters, pipes, `&&`, redirects)
  pass through untouched.
- Exact-repeat bypass guarantees the model can always reach raw output.
- Both tool surfaces covered — the cloud/Azure path uses unified_exec, not
  just `shell.rs`.

## Metrics (per arm, on `AgentRunRecord`)

- substitutions
- escape-hatch repeats
- build pass-throughs (+ reasons)
- compact bytes
- estimated raw bytes
- top files

## Evidence (early, honest)

| Run | File | Result |
| --- | --- | --- |
| Run4 | `verification/src/rules.rs` | attributable **−22% token win**, correct top file, no bypass |
| Run2 | `context-harness/src/agent_eval.rs` | correct top file, −54% tokens, but **attribution weaker** — the model bypassed every substitution |
| Run5 | `context-harness/src/renderer.rs` | **no-harm** mechanism pass; proxy stayed inert when no eligible search occurred |

The stricter treatment-aware classifier deliberately withholds Run2 as a
fully attributable win (the model bypassed all substitutions, so the −54% is
run variance). **Run4 is the cleanest win.**

## Not claimed

- Not default-on ready.
- Not proven across broad task classes.
- Not combined with Verification Policy or Large Read Proxy.
- Not a replacement for raw `rg`.

## Merge ask

Merge behind `Feature::SearchProxy` for continued controlled evaluation.
**Do not enable by default yet.** A default-on argument would need one more
symbol-heavy win (gold file not named in the prompt) plus one more no-harm
run.

## Landing note

Recommend **squash-merge**. An early branch commit
(`enhance search proxy metrics and evaluation integration`) inadvertently
vendored a 1.35 MB `research-bundle.tar.gz` + eval-artifact snapshots; a
later commit removed them and added `.gitignore` rules. The net branch diff
carries none of it, and squash-merge collapses the add-then-remove churn so
the landed commit is clean. Suggested squashed message:

```
feat(codex-rs): add feature-gated Search Proxy (rg mediation, default off)

Classify rg → compact ranked evidence → core hook (shell + unified_exec)
→ eval path + metrics + treatment-aware reporting. Read-only, escape-hatch
protected, default off. Early evidence: one attributable token win on
symbol-heavy discovery (rules.rs, -22%) + a no-harm inert case.
```
