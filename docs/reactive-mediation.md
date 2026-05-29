# Reactive Mediation (Search Proxy + Large Read Proxy)

Reactive Mediation is an opt-in, default-off pair of intercept layers that
sit in front of the model's shell tool calls and replace expensive raw
output (`rg`, `cat`/`sed` on large files) with compact, ranked evidence the
model can use directly. The goal is to reduce **fresh, model-visible
tool-output cost** without harming task outcomes.

> **Status:** default-off, opt-in. Validated on a small internal suite of
> search/read coding tasks (reduced delivered tool-output bytes and
> cost-weighted token cost with no edit regressions); kept default-off
> pending broader validation. See `docs/reactive-mediation-review.md` for
> the reviewer's map.

## What it does

- **Search Proxy** (`features.search_proxy`) — when the model issues a
  simple `rg <pattern>` invocation, Codex runs a bounded internal `rg`,
  picks a likely owner file (ranked by definition-symbol overlap with the
  query), and returns up to a few sample match lines in `rg`-native
  `path:line:col:text` form. Raw output is not sent. Repeating the exact
  same command bypasses the proxy and returns raw output (escape hatch).
  Requires the system `rg` binary on PATH. The internal `rg` runs under a
  wall-clock timeout (default 5s); if it expires the proxy passes through —
  the model's own command runs normally, so the turn is never blocked.

- **Large Read Proxy** (`features.large_read_proxy`) — when the model
  issues `cat <large file>` or `sed -n '1,Np'` with a wide span on a file
  ≥120 lines, Codex returns line-numbered slices (file header + public
  definitions, or windows around hinted symbols) instead of the raw dump.
  Repeating the same command bypasses to raw output.

- **Composition** — enable both flags and the model's `search → read`
  workflow is mediated end-to-end: search localizes the file, then a
  follow-up over-read of that file is compacted before it reaches the
  model.

Neither proxy mutates anything. They are read-only and intercept only
their respective tool shapes — anything they can't classify cleanly is
passed through untouched.

## Enabling

Both proxies are opt-in via the features system. **The validated path is
composed mode** — enable BOTH. In `~/.codex/config.toml`:

```toml
[features]
search_proxy = true       # opt in to Search Proxy
large_read_proxy = true   # opt in to Large Read Proxy
```

Or per-invocation:

```bash
codex exec -c features.search_proxy=true -c features.large_read_proxy=true ...
```

Enabling only one is supported but partial — the model's search→read
workflow is mediated only on the enabled phase. `codex doctor`
explicitly flags one-enabled configs as "composed mode not enabled" so
this is visible. There is intentionally no single umbrella flag; the two
proxies are independent and gated separately.

## What you'll see when it's working

At the end of each turn, Codex emits a one-paragraph telemetry summary to
stderr describing what the proxies did:

```
[reactive mediation] session summary:
  search-proxy: 3 substituted, 1 bypassed (model re-ran), 0 pass-through;
                saved ~193 KB (compact 2 KB vs raw 200 KB)
  large-read-proxy: inert (no eligible cat/sed occurred)
```

The line distinguishes three states:

- **inert** — feature enabled but no eligible search/read occurred.
- **N substituted / N bypassed / N pass-through** — the proxy fired.
  `bypassed` means the model re-ran the exact same command; `pass-through`
  means the proxy ran internally but the result didn't justify substituting.

If the proxy is not enabled, no summary is emitted (zero noise for
non-opted-in users).

For per-event detail, structured tracing events are also emitted at INFO
level under `target = "search_proxy"` / `target = "large_read_proxy"` —
useful when diagnosing why a particular command was or wasn't intercepted.

## `codex doctor`

`codex doctor` includes a `reactive-mediation` check that reports the
current flag state (search_proxy, large_read_proxy, composed mode) and
cross-references the `runtime.search` check (which is where `rg`
availability is actually verified — Search Proxy depends on it).

## When to disable

- **You're debugging a tool-call shape** the proxy might be intercepting.
  Disable temporarily to see raw output.
- **You're not opted in** — both default off, so this is the safe state.
- **Your `rg` binary is unusual** — Search Proxy relies on `rg` for its
  internal search; if `codex doctor`'s `runtime.search` row is failing,
  disable Search Proxy until it's fixed.

## Known limitations

- **Large Read Proxy is most useful in composition.** Frontier models
  often self-target reads on well-specified tasks, so LRP standalone is
  frequently inert; its value shows when Search Proxy localizes a file the
  model then over-reads.
- **Cost, not quality.** Validation measured tool-output cost; edits were
  preserved, not improved. No quality claim is made.
- **Narrow validation surface.** Validated on a small fixed suite of
  search/read tasks — hence default-off. Broaden deliberately before
  considering default-on.
- **Ranking is best-effort.** Search Proxy's owner ranking is heuristic;
  low-confidence results are rendered non-directively ("do NOT trust this
  path") and the model is steered to the match lines, never forced onto a
  single guessed file.

## See also

- `docs/reactive-mediation-review.md` — reviewer's map (diff surface, hot
  paths, safety model, test matrix).
