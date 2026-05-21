#!/usr/bin/env bash
# Deterministic harness E2E for a tiny Python calculator repo (no LLM / no OpenAI).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CODEX_RS_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_SRC="${CODEX_RS_ROOT}/context-harness/tests/fixtures/e2e_python_calculator"
PROMPT='Fix the failing calculator test. Keep the change minimal.'

CODEX_BIN=""
SKIP_RUN=0
VERBOSE=0

usage() {
  cat <<'EOF'
Usage: harness-e2e-oss-python.sh [--codex-bin PATH] [--skip-run] [--verbose]

Runs the deterministic harness loop on a copy of the Python calculator fixture:
  context build -> verification plan -> (optional) verification run

Does not invoke codex --oss or Ollama. See .codex/skills/test-harness-e2e-oss/SKILL.md
for the manual OSS smoke step.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --codex-bin)
      CODEX_BIN="$2"
      shift 2
      ;;
    --skip-run)
      SKIP_RUN=1
      shift
      ;;
    --verbose)
      VERBOSE=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

log() {
  if [[ "${VERBOSE}" -eq 1 ]]; then
    echo "$@"
  fi
}

resolve_codex_bin() {
  if [[ -n "${CODEX_BIN}" ]]; then
    return
  fi
  local built="${CODEX_RS_ROOT}/target/debug/codex"
  if [[ -x "${built}" ]]; then
    CODEX_BIN="${built}"
    return
  fi
  if command -v codex >/dev/null 2>&1; then
    local on_path
    on_path="$(command -v codex)"
    if "${on_path}" context --help >/dev/null 2>&1; then
      CODEX_BIN="${on_path}"
      return
    fi
  fi
  echo "building codex-cli (debug)..." >&2
  (cd "${CODEX_RS_ROOT}" && cargo build -p codex-cli -q)
  CODEX_BIN="${built}"
}

pytest_available() {
  python -m pytest --version >/dev/null 2>&1
}

TMP_REPO="$(mktemp -d)"
TMP_ARTIFACTS="$(mktemp -d)"
cleanup() {
  rm -rf "${TMP_REPO}" "${TMP_ARTIFACTS}"
}
trap cleanup EXIT

cp -R "${FIXTURE_SRC}/." "${TMP_REPO}/"
cd "${TMP_REPO}"

resolve_codex_bin
log "using codex: ${CODEX_BIN}"

CONTEXT_FRAGMENT="${TMP_ARTIFACTS}/context_fragment.txt"
"${CODEX_BIN}" context build "${PROMPT}" --cwd . --prompt-fragment >"${CONTEXT_FRAGMENT}"
python - "${CONTEXT_FRAGMENT}" <<'PY'
import sys
text = open(sys.argv[1]).read()
for needle in ("calculator.py", "test_calculator"):
    assert needle in text, f"missing {needle!r} in context fragment"
PY

PLAN_JSON="${TMP_ARTIFACTS}/plan.json"
"${CODEX_BIN}" verification plan --changed src/calculator.py --cwd . --json-out "${PLAN_JSON}"
python - "${PLAN_JSON}" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
commands = [c["command"] for c in data["commands"]]
assert any("python -m pytest tests/test_calculator.py" in c for c in commands), commands
assert not any("cargo test" in c for c in commands), commands
assert not any(c.strip() in ("pytest", "python -m pytest") for c in commands), commands
PY

if [[ "${SKIP_RUN}" -eq 1 ]]; then
  echo "harness-e2e-oss-python: plan checks passed (--skip-run)"
  exit 0
fi

if ! pytest_available; then
  echo "harness-e2e-oss-python: plan checks passed; skipping run/post-failure (python -m pytest unavailable)" >&2
  exit 0
fi

REPORT_JSON="${TMP_ARTIFACTS}/report.json"
set +e
"${CODEX_BIN}" verification run \
  --changed src/calculator.py \
  --cwd . \
  --yes \
  --json-out "${REPORT_JSON}"
run_exit=$?
set -e
if [[ "${run_exit}" -ne 1 ]]; then
  echo "expected verification run exit code 1 (failed tests), got ${run_exit}" >&2
  exit "${run_exit}"
fi

python - "${REPORT_JSON}" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
assert data["status"] == "failed", data.get("status")
PY

echo "harness-e2e-oss-python: all deterministic checks passed"
