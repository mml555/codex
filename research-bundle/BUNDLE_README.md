# RI eval experiment — research evidence bundle

Snapshot of every artifact needed to do a strategic / external-research
pass on the closed Repo Intelligence (RI) session-injection experiment.
Closure status, verdict, and follow-up are documented in
[EVAL_REPORT.md](EVAL_REPORT.md). This bundle gathers the underlying
evidence so a research pass can reason from the raw data, not the summary.

## Provenance

- Branch: `harness-core-pr1`
- Bundle generated at codex commit: see `code-snapshot/SNAPSHOT_COMMIT.txt`
- Each run records its own `git_commit` (which differs across runs as
  selector fixes landed). See `RUN_INDEX.md`.
- All four release-mode cloud pairs are included. The earlier
  debug-binary / uncached pairs are NOT in this bundle because the
  eval report excludes them — debug-mode wall-clock was 5–7×
  inflated by harness cost.

## Layout

```
research-bundle/
├── BUNDLE_README.md            ← this file
├── EVAL_REPORT.md              ← the closure report (source of truth)
├── RUN_INDEX.md                ← run → task → commit → outcome table
│
├── runs/                       ← one dir per release-mode pair
│   ├── run5-area-package-alias/
│   │   ├── vanilla/
│   │   │   ├── record.json              ← canonical metrics
│   │   │   ├── events.jsonl             ← model action stream
│   │   │   ├── rollout_full.jsonl       ← raw session rollout
│   │   │   ├── prompt_messages.md       ← human-readable system/user msgs
│   │   │   ├── ri_directive.txt         ← (empty for vanilla)
│   │   │   ├── ri_directive_trimmed.txt ← (empty for vanilla)
│   │   │   ├── run_meta.json            ← model, provider, commit, thread_id
│   │   │   └── codex_exec.stderr.log    ← contribute() timing log
│   │   └── repo_intelligence/  (same files; ri_directive*.txt populated)
│   ├── run6-directive-marker-prefix/
│   ├── run7-directive-marker-postfix/
│   └── run8-agent-eval-excluded/
│
├── ri-packets/                 ← top-level mirror of RI directives for
│                                 quick side-by-side comparison
│
├── selector-evidence/          ← SELECTOR_FINDINGS.md + the live-map
│                                 regression tests (ri_packet_regressions.rs)
│
├── fixtures/                   ← task fixtures used by Runs 5–8
│   ├── agent_eval_tasks_ri_hard_v1.json
│   ├── agent_eval_tasks_area_package_alias_only.json
│   ├── agent_eval_tasks_directive_marker_only.json
│   └── agent_eval_tasks_agent_eval_excluded_only.json
│
├── code-snapshot/              ← canonical source at SNAPSHOT_COMMIT
│   ├── SNAPSHOT_COMMIT.txt
│   ├── context-harness/src/{assembler,task_terms,renderer,agent_eval}.rs
│   ├── context-harness/tests/ri_packet_regressions.rs
│   ├── ext/repo-intelligence/src/extension.rs
│   └── scripts/harness-agent-eval.sh
│
├── summary/
│   └── runs_cost_summary.csv   ← one row per arm × Runs 5–8
│
└── scripts/                    ← the extractors used to build this bundle
    ├── extract_packets.py
    └── build_summary_csv.py
```

## How the bundle was assembled

Two scripts under `scripts/` rebuild the derived data deterministically
from the source artifacts:

1. `extract_packets.py` — for each Run 5–8 arm: finds the rollout file
   in `~/.codex/sessions/`, copies it verbatim, extracts every
   prompt-role message into `prompt_messages.md`, then isolates the
   `Harness repo intelligence:` block into `ri_directive*.txt` and
   `ri-packets/<run>.txt`.

2. `build_summary_csv.py` — reads every `record.json` and `run_meta.json`,
   emits one CSV row per arm with the full cost ledger.

Both scripts hard-code the rollout paths and read from the live `runs/`
directory copies. They are idempotent.

## How to use this bundle for research

Start from [EVAL_REPORT.md](EVAL_REPORT.md) for the closure verdict and
the report's framing of the four findings.

Then for any claim in that report, the underlying evidence is here:

- **"selector works on Run 5/7, fails on Run 6/8"** →
  `ri-packets/*.txt` — read the four directives side by side.
- **"wrong hint is not free (Run 8)"** →
  `summary/runs_cost_summary.csv` — tokens row for run8 RI vs vanilla;
  `runs/run8-agent-eval-excluded/repo_intelligence/events.jsonl` for the
  model's actual command stream.
- **"selector fix for directive-marker landed"** →
  `ri-packets/run6-directive-marker-prefix.txt` (wrong before fix) vs
  `ri-packets/run7-directive-marker-postfix.txt` (correct after fix),
  plus `selector-evidence/ri_packet_regressions.rs` for the locked-in
  regression test.
- **"intent-file scoring corrected the formatter contamination"** →
  `runs/*/repo_intelligence/record.json` → compare
  `intent_changed_files` vs `diff_changed_files` vs
  `formatter_changed_files`. All four runs: intent = 1, diff = many,
  formatter = (diff − intent − gold).
- **"verification loop in Run 7 explains the wall-clock penalty"** →
  `runs/run7-directive-marker-postfix/repo_intelligence/events.jsonl`
  for the `just test` invocations vs the vanilla arm's events.jsonl.
- **"harness prewarm is the dominant local cost"** →
  `runs/*/repo_intelligence/codex_exec.stderr.log` for the
  `contribute()` timing line, and each record's `harness_prewarm_ms`.

## What's intentionally NOT in this bundle

- The four debug-binary / uncached cloud pairs (Runs 1–4). EVAL_REPORT
  explicitly excludes them from the experimental result because the
  debug profile inflated wall-clock 5–7×. Their artifact dirs still
  exist on disk under `codex-rs/ri-*/`.
- The full `codex-rs` source tree at this commit. Only the
  selector-chain files referenced in the report are mirrored under
  `code-snapshot/`. Pull the rest from `git checkout` if needed.
- The `RepoMap` data used to build packets. The selector regression
  tests rebuild it on demand from the live workspace via
  `RepoMapBuilder::build(...)`.
- Cargo-fmt collateral diffs. They are summarized via the
  `formatter_changed_files` array in each record.

## Verifying numbers against the report

The CSV at `summary/runs_cost_summary.csv` reproduces the
EVAL_REPORT.md table exactly:

```
Run  Tokens V/RI       Wall V/RI
 5   607k / 387k       105s / 121s
 6   882k / 865k       278s / 319s
 7   247k / 904k        35s / 394s
 8   676k / 3,408k     262s / 500s
```
