# OSS Python harness E2E (manual runbook)

Repo-local manual procedure for exercising the harness loop with Ollama via `codex --oss`.
This skill is **not** auto-invoked by product routing; use it when validating the harness on a disposable Python repo.

## Automated deterministic loop (run first)

From `codex-rs/`:

```bash
./scripts/harness-e2e-oss-python.sh --verbose
```

Optional: `--codex-bin /path/to/codex`, `--skip-run` (plan-only).

The script copies `context-harness/tests/fixtures/e2e_python_calculator/` to a temp directory and checks:

- `context build` includes `calculator.py` and `test_calculator`
- `verification plan` emits `python -m pytest tests/test_calculator.py` (no `cargo test`)
- If `python -m pytest --version` works: `verification run` fails on the broken calculator, then `context build --with-verification-report` renders a compact post-failure fragment

## Prerequisites (user config only)

Set in `~/.codex/config.toml` (not project `.codex/config.toml`):

```toml
oss_provider = "ollama"
model = "qwen2.5-coder:1.5b"

[features]
repo_intelligence = true
```

Install Ollama and pull a small model:

```bash
ollama pull qwen2.5-coder:1.5b
ollama run qwen2.5-coder:1.5b "Say ok"
```

## Known gotchas

- Use `codex-rs/target/debug/codex` (or `cargo build -p codex-cli` first). The npm/global `codex` may lack `context` / `verification` subcommands.
- Run harness commands from the **temp fixture repo** (`$TMP_REPO`), not from `codex-rs/`.
- Do not paste shell lines that start with `#` into zsh; they run as commands and fail.
- If you run pytest manually in the fixture, `tests/__pycache__/*.pyc` may appear; the planner ignores `.pyc` and `__pycache__` when pairing targets.
- `verification run` exits **1** on expected test failures but still writes `--json-out` report.
- Local Ollama/Codex may **narrate** patches without changing disk; always verify with `git diff` and `cat src/calculator.py`. Treat unreliable OSS tool execution as a separate investigation—not a harness blocker.

## Deterministic E2E status

**Complete** when `./scripts/harness-e2e-oss-python.sh --verbose` passes (plan → guarded run → post-failure context).

Local Ollama repair is an **optional manual research** task (scorecard below), not required for harness sign-off.

## Manual OSS smoke (optional, after script passes)

```bash
TMP_REPO="$(mktemp -d)"
cp -R codex-rs/context-harness/tests/fixtures/e2e_python_calculator/. "$TMP_REPO/"
cd "$TMP_REPO"
git init -q && git add -A && git commit -q -m initial

CODEX=/path/to/codex-rs/target/debug/codex   # or export PATH after cargo build -p codex-cli

"$CODEX" --oss --local-provider ollama -m qwen2.5-coder:7b \
  "Fix the failing calculator test. Keep the change minimal."
```

Use interactive `codex --oss` first; use `codex exec` only if your build exposes it.

After the session, verify disk state:

```bash
git diff
cat src/calculator.py
python3 -m pytest tests/test_calculator.py -q
```

Re-run planning/verification on the edited tree with the **debug** binary:

```bash
"$CODEX" verification plan --changed src/calculator.py --cwd .
"$CODEX" verification run --changed src/calculator.py --cwd . --yes --json-out /tmp/report.json
"$CODEX" context build "Fix the failing calculator test." \
  --with-verification-report /tmp/report.json \
  --changed src/calculator.py \
  --prompt-fragment
```

Optional one-shot OSS debug (tool-forcing; stop if transcript claims success but `git diff` is empty):

```bash
codex --oss --local-provider ollama -m qwen2.5-coder:7b \
  "Edit src/calculator.py. Replace 'return a - b' with 'return a + b'. After editing, run git diff and show the diff."
```

## Scorecard (5–10 real failures)

| Run | failure_type OK | likely_files OK | relevant_output OK | prompt compact | model used failure |
| --- | --------------- | --------------- | ------------------ | -------------- | ------------------ |
| 1   |                 |                 |                    |                |                    |

## Explicitly deferred

- Session-triggered verification
- Auto-repair / retry loops
- Broad test escalation
- AST extractors / embeddings
- TUI harness panels

## Tuning triggers (change code only after patterns)

1. Failure output too noisy → summarization in `verification/src/output.rs`
2. Wrong `repair_hint` type → `context-harness/src/repair_hint.rs` classifier order
3. Missing file paths in output → path parsers in `repair_hint.rs`
4. Post-failure too thin/thick → `context-harness/src/post_failure.rs`
