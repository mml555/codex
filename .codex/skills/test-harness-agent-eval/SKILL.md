# Harness agent evaluation (artifact scoring)

Optional **evaluation harness** for comparing vanilla vs harness-context agent runs on the
same tasks. This is not proof that harness improves Codex—only a structured way to score
recorded outcomes. Evidence is early; OSS execution is unreliable.

**Depends on PR 1** (deterministic `context` / `verification` tooling). Not part of PR 1 review.

## Core contract (offline, no model)

`codex context agent-eval score` reads JSON artifacts only. No Codex model, no Ollama, no
repo mutation, no network.

```bash
codex context agent-eval score \
  --fixture codex-rs/context-harness/tests/fixtures/agent_eval_tasks.json \
  --artifacts-dir /tmp/harness-agent-eval \
  --human
```

### Artifact layout

```text
{artifacts_dir}/{task_id}/vanilla/record.json
{artifacts_dir}/{task_id}/harness/record.json          # manual context prefix arm
{artifacts_dir}/{task_id}/repo_intelligence/record.json # session injection arm
```

Each `record.json`:

```json
{
  "arm": "vanilla",
  "task_id": "calculator_fix",
  "changed_files": ["src/calculator.py"],
  "tests_passed": true,
  "turn_count": 2,
  "used_post_failure": false,
  "exec_exit_code": 0,
  "harness_context_visible": true,
  "repo_intelligence_enabled": true
}
```

Populate `changed_files` from `git diff --name-only`. Set `tests_passed` from the task’s
`verify_command` exit code. Set `turn_count` by counting `turn.completed` and `turn.failed`
lines in `codex exec --json` JSONL (optional `events.jsonl` beside `record.json`).

## Metrics (same tasks, both arms)

| Dimension | Definition |
| --------- | ---------- |
| Correct file touched | `changed_files` ∩ `gold_files` |
| Tests pass | `tests_passed` |
| Turn count | `turn_count` |
| Unnecessary files changed | `changed_files` − `gold_files` |
| Failure recovery quality | For `requires_post_failure` tasks: `not_applicable` / `failed` / `partial` / `good` |
| Harness context visible | `harness_context_visible` (rollout probe; must be true for repo_intelligence arms) |
| Bridge files touched | `bridge_files` from fixture ∩ `changed_files` |

Tasks:
- Calculator sandbox: `context-harness/tests/fixtures/agent_eval_tasks.json`
- Real codex-rs tasks: `context-harness/tests/fixtures/agent_eval_tasks_codex_session.json`

## Optional runner (manual / OSS)

Not required for CI or review. May narrate patches without changing disk—always verify artifacts.

```bash
cd codex-rs
./scripts/harness-agent-eval.sh --verbose \
  --artifacts-dir /tmp/harness-agent-eval --run \
  --oss --local-provider ollama -m qwen2.5-coder:7b
```

Session injection (vanilla vs `repo_intelligence=true`, scores automatically):

```bash
cd codex-rs
./scripts/harness-agent-eval.sh --verbose --run --session-injection \
  --artifacts-dir /tmp/harness-agent-eval-session-real
```

Without `--run`, the script only scores existing artifacts (same as `agent-eval score`).
Treatment arm auto-detects `repo_intelligence` vs `harness` from artifact layout.

## Scorecard (5–10 runs)

| Run | correct file (V/H) | tests pass (V/H) | turns (V/H) | extra files (V/H) | recovery (H) |
| --- | ------------------ | ---------------- | ----------- | ----------------- | ------------ |
| 1   |                    |                  |             |                   |              |

## Explicitly out of scope

- Proving harness improves production Codex
- Auto-running agents in deterministic E2E CI
- Session wiring or auto-repair loops
