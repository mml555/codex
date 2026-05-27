# Search Proxy — Merge-Candidate Assessment

**Branch:** `search-proxy-hardening` (off `search-proxy-mvp`)
**Question:** Is Search Proxy ready to propose as a feature-gated harness capability?
**Short answer:** Yes as a **default-off, feature-gated experiment** — it is safe, read-only, and has one real attributable win plus a no-harm case. **Not yet** as a **default-on, proven** improvement: the outcome bar wants more creditable symbol-task wins.

## What it is

Reactive `rg` mediation. When the model issues a simple `rg`, the harness runs a bounded internal `rg`, returns compact ranked evidence (likely owner file + a few sample lines) in place of raw output, and lets the model repeat the exact command to get raw output.

## Implementation readiness — MET

| Criterion | Status |
| --- | --- |
| Feature-gated, default off (`features.search_proxy`) | ✓ |
| Read-only (runs `rg`, returns text; no writes, no arbitrary exec) | ✓ |
| Escape hatch (exact repeat → raw `rg`) | ✓ |
| Clean metrics in record/report path | ✓ (treatment-aware table + `proxy_state` column) |
| Minimal core hook, both `shell.rs` and `unified_exec` | ✓ |
| Strong tests | ✓ (crate unit tests, record serde round-trip, cli tests green) |

## Outcome evidence — PARTIAL

Hardened scorecard — treatment-aware classifier, wall-clock excluded from the verdict, 5% token materiality threshold:

| Run | File | Proxy state | Top file | Verdict |
| --- | --- | --- | --- | --- |
| Run4 | `verification/src/rules.rs` | active (subs=2, 0 bypass) | correct | **`search_proxy_better:token_efficiency` (−22%)** |
| Run2 | `context-harness/src/agent_eval.rs` | bypassed_all (subs=3 = repeats=3) | correct | `search_proxy_inconclusive:model_strategy_variance` (−54% withheld) |
| Run5 | `context-harness/src/renderer.rs` | inert (subs=0) | n/a | `search_proxy_inert:no_eligible_search` (no-harm) |

- **1 creditable token win** (Run4): same intent edit, no patch regression.
- **1 inconclusive** (Run2): the proxy surfaced the correct top file and the edit matched, but the model bypassed every substitution, so the −54% is run variance, not attributable to the proxy.
- **1 no-harm** (Run5): proxy stayed inert; the arm's cost delta is confounded by unrelated verification behavior.

## Upstream-readiness bar (for default-on)

- **≥3 symbol-heavy tasks:** top file correct, same/better intent edit, **≥2 of 3 creditable token wins**, no patch-attempt regression.
- **≥2 no-harm tasks:** proxy inert/harmless, no proxy-attributable overhead.

## Gap vs bar

- Symbol tasks: 2 distinct crates exercised (`rules.rs`, `agent_eval.rs`), both top-file-correct with matching edits. **Creditable wins: 1 of the desired ≥2** — Run2 is inconclusive (bypass), not a win.
- No-harm: **1 of 2** (Run5).

## Recommendation

1. **Land now as feature-gated experimental** (default off, read-only, escape hatch, metrics). It is safe and has one real attributable win — worth shipping behind the flag so others can opt in and we gather more data without risk.
2. **To justify default-on:** one more symbol-heavy cloud pair (gold file *not* named in the prompt) to turn 1 → 2 creditable wins, plus one more no-harm case. That run is the only remaining cloud spend and is gated on an explicit go.

**Honest claim today:** Search Proxy reliably surfaces the right file on symbol-heavy discovery and is no-harm when no eligible search occurs; it produced one clean token win and one inconclusive (model-bypassed) case. Strong enough to land behind a flag; one more creditable symbol win would justify a default-on proposal.
