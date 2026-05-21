#!/usr/bin/env bash
# Vanilla vs harness-context agent eval on shared tasks (optional OSS runs + deterministic score).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CODEX_RS_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_SRC="${CODEX_RS_ROOT}/context-harness/tests/fixtures/e2e_python_calculator"
CALCULATOR_TASK_FIXTURE="${CODEX_RS_ROOT}/context-harness/tests/fixtures/agent_eval_tasks.json"
CODEX_SESSION_TASK_FIXTURE="${CODEX_RS_ROOT}/context-harness/tests/fixtures/agent_eval_tasks_codex_session.json"
TASK_FIXTURE="${CALCULATOR_TASK_FIXTURE}"
FIXTURE_EXPLICIT=0

CODEX_BIN=""
ARTIFACTS_DIR=""
RUN_AGENT=0
SESSION_INJECTION=0
VERBOSE=0
OSS_ARGS=()

usage() {
  cat <<'EOF'
Usage: harness-agent-eval.sh [--codex-bin PATH] [--artifacts-dir DIR] [--fixture PATH] [--run] [--session-injection] [--verbose] [--oss ...]

Compares vanilla Codex vs treatment Codex on the same tasks.

Default fixture: agent_eval_tasks.json (calculator sandbox).
--session-injection: vanilla vs repo_intelligence (-c features.repo_intelligence=true)
  with agent_eval_tasks_codex_session.json unless --fixture is set.
Without --session-injection: vanilla vs harness (manual context build prefix).

Without --run: scores existing artifacts only (requires record.json per task/arm).
With --run: executes both arms via `codex exec --json` (needs a working model provider).

Artifacts layout:
  ARTIFACTS_DIR/<task_id>/vanilla/record.json
  ARTIFACTS_DIR/<task_id>/harness/record.json   (manual prefix arm)
  ARTIFACTS_DIR/<task_id>/repo_intelligence/record.json   (--session-injection)

Metrics: harness_context_visible, correct file, tests pass, bridge files touched,
turn count, unnecessary files, failure recovery (when requires_post_failure).
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --codex-bin)
      CODEX_BIN="$2"
      shift 2
      ;;
    --artifacts-dir)
      ARTIFACTS_DIR="$2"
      shift 2
      ;;
    --fixture)
      TASK_FIXTURE="$2"
      FIXTURE_EXPLICIT=1
      shift 2
      ;;
    --run)
      RUN_AGENT=1
      shift
      ;;
    --session-injection)
      SESSION_INJECTION=1
      shift
      ;;
    --verbose)
      VERBOSE=1
      shift
      ;;
    --oss)
      OSS_ARGS=(--oss)
      shift
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --codex-bin | --artifacts-dir | --fixture | --run | --session-injection | --verbose | -h | --help) break ;;
          *)
            OSS_ARGS+=("$1")
            shift
            ;;
        esac
      done
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

if [[ "${SESSION_INJECTION}" -eq 1 && "${FIXTURE_EXPLICIT}" -eq 0 ]]; then
  TASK_FIXTURE="${CODEX_SESSION_TASK_FIXTURE}"
fi

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
  (cd "${CODEX_RS_ROOT}" && cargo build -p codex-cli -q)
  CODEX_BIN="${built}"
}

harness_context_visible_for_run() {
  local events="$1"
  local codex_home="${CODEX_HOME:-${HOME}/.codex}"
  python3 - "${events}" "${codex_home}" <<'PY'
import json
import sys
from pathlib import Path

needle = "Harness repo context:"
events_path = Path(sys.argv[1])
codex_home = Path(sys.argv[2])

def visible_in_events() -> bool:
    try:
        with events_path.open(encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    ev = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if needle in json.dumps(ev, ensure_ascii=False):
                    return True
    except FileNotFoundError:
        pass
    return False

def visible_in_rollout() -> bool:
    thread_id = None
    try:
        with events_path.open(encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    ev = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if ev.get("type") == "thread.started":
                    thread_id = ev.get("thread_id")
                    break
    except FileNotFoundError:
        return False
    if not thread_id:
        return False
    sessions = codex_home / "sessions"
    if not sessions.is_dir():
        return False
    for rollout in sessions.rglob(f"*{thread_id}*.jsonl"):
        try:
            text = rollout.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        if needle in text:
            return True
    return False

print("true" if visible_in_events() or visible_in_rollout() else "false")
PY
}

write_record() {
  local out="$1"
  local arm="$2"
  local task_id="$3"
  local changed_json="$4"
  local tests_passed="$5"
  local turn_count="$6"
  local used_post_failure="$7"
  local exec_exit="$8"
  local events="$9"
  local repo_intel_enabled=false
  local harness_visible
  harness_visible="$(harness_context_visible_for_run "${events}")"
  if [[ "${arm}" == "repo_intelligence" ]]; then
    repo_intel_enabled=true
  fi
  mkdir -p "$(dirname "${out}")"
  python3 - "${out}" "${arm}" "${task_id}" "${changed_json}" "${tests_passed}" "${turn_count}" "${used_post_failure}" "${exec_exit}" "${repo_intel_enabled}" "${harness_visible}" <<'PY'
import json, sys
out, arm, task_id, changed_json, tests_passed, turn_count, used_pf, exec_exit, repo_intel, harness_visible = sys.argv[1:11]
record = {
    "arm": arm,
    "task_id": task_id,
    "changed_files": json.loads(changed_json),
    "tests_passed": tests_passed == "true",
    "turn_count": int(turn_count) if turn_count not in ("", "null") else None,
    "used_post_failure": used_pf == "true",
    "exec_exit_code": int(exec_exit) if exec_exit not in ("", "null") else None,
    "repo_intelligence_enabled": repo_intel == "true",
    "harness_context_visible": harness_visible == "true",
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(record, f, indent=2)
    f.write("\n")
PY
}

count_turns() {
  local events="$1"
  python3 - "${events}" <<'PY'
import json, sys
count = 0
with open(sys.argv[1], encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") in ("turn.completed", "turn.failed"):
            count += 1
print(count)
PY
}

run_arm() {
  local arm="$1"
  local task_id="$2"
  local task_text="$3"
  local verify_cmd="$4"
  local requires_pf="$5"
  local workdir_kind="${6:-calculator}"
  local workdir
  if [[ "${workdir_kind}" == "codex_rs" ]]; then
    workdir="${CODEX_RS_ROOT}"
    cd "${workdir}"
  else
    workdir="$(mktemp -d)"
    cp -R "${FIXTURE_SRC}/." "${workdir}/"
    cd "${workdir}"
    git init -q
    git add -A
    git commit -q -m initial
  fi

  local prompt="${task_text}"
  local used_post_failure=false
  if [[ "${arm}" == "repo_intelligence" ]]; then
    prompt="${task_text}"
  elif [[ "${arm}" == "harness" ]]; then
    if [[ "${requires_pf}" == "true" ]]; then
      set +e
      "${CODEX_BIN}" verification run --changed src/calculator.py --cwd . --yes --json-out /tmp/report.json >/dev/null 2>&1
      set -e
      local fragment
      fragment="$("${CODEX_BIN}" context build "${task_text}" \
        --with-verification-report /tmp/report.json \
        --changed src/calculator.py --cwd . --prompt-fragment 2>/dev/null || true)"
      prompt="${fragment}

${task_text}"
      used_post_failure=true
    else
      local fragment
      fragment="$("${CODEX_BIN}" context build "${task_text}" \
        --changed src/calculator.py --cwd . --prompt-fragment 2>/dev/null || true)"
      prompt="${fragment}

${task_text}"
    fi
  fi

  local events="${ARTIFACTS_DIR}/${task_id}/${arm}/events.jsonl"
  mkdir -p "$(dirname "${events}")"
  set +e
  if ((${#OSS_ARGS[@]} > 0)); then
    "${CODEX_BIN}" exec "${OSS_ARGS[@]}" -s workspace-write \
      --dangerously-bypass-approvals-and-sandbox \
      --json \
      "${prompt}" </dev/null >"${events}" 2>/dev/null
  else
    if [[ "${arm}" == "repo_intelligence" ]]; then
      "${CODEX_BIN}" exec -c features.repo_intelligence=true -s workspace-write \
        --dangerously-bypass-approvals-and-sandbox \
        --json \
        "${prompt}" </dev/null >"${events}" 2>/dev/null
    else
      "${CODEX_BIN}" exec -s workspace-write \
        --dangerously-bypass-approvals-and-sandbox \
        --json \
        "${prompt}" </dev/null >"${events}" 2>/dev/null
    fi
  fi
  local exec_exit=$?
  set -e

  local changed_json
  changed_json="$(python3 -c 'import json,sys; print(json.dumps([l for l in sys.stdin.read().splitlines() if l.strip()]))' <<<"$(git diff --name-only)")"
  local tests_passed=false
  if [[ -n "${verify_cmd}" ]]; then
    set +e
    eval "${verify_cmd}"
    if [[ $? -eq 0 ]]; then
      tests_passed=true
    fi
    set -e
  fi
  local turns
  turns="$(count_turns "${events}")"
  write_record "${ARTIFACTS_DIR}/${task_id}/${arm}/record.json" "${arm}" "${task_id}" \
    "${changed_json}" "${tests_passed}" "${turns}" "${used_post_failure}" "${exec_exit}" "${events}"
  log "arm=${arm} task=${task_id} workdir=${workdir} exit=${exec_exit}"
}

resolve_codex_bin
if [[ -z "${ARTIFACTS_DIR}" ]]; then
  ARTIFACTS_DIR="$(mktemp -d)"
  log "artifacts: ${ARTIFACTS_DIR}"
fi

if [[ "${RUN_AGENT}" -eq 1 ]]; then
  python3 - "${TASK_FIXTURE}" <<'PY' | while IFS=$'\t' read -r id task verify requires_pf workdir_kind || [[ -n "${id:-}" ]]; do
import json, sys
tasks = json.load(open(sys.argv[1], encoding="utf-8"))
for t in tasks:
    print(
        t["id"],
        t["task"],
        t.get("verify_command") or "",
        str(t.get("requires_post_failure", False)).lower(),
        t.get("workdir", "calculator"),
        sep="\t",
    )
PY
    [[ -z "${id}" ]] && break
    requires_pf="${requires_pf:-false}"
    workdir_kind="${workdir_kind:-calculator}"
    if [[ "${SESSION_INJECTION}" -eq 1 ]]; then
      run_arm vanilla "${id}" "${task}" "${verify}" "${requires_pf}" "${workdir_kind}"
      run_arm repo_intelligence "${id}" "${task}" "${verify}" "${requires_pf}" "${workdir_kind}"
    else
      run_arm vanilla "${id}" "${task}" "${verify}" "${requires_pf}" "${workdir_kind}"
      run_arm harness "${id}" "${task}" "${verify}" "${requires_pf}" "${workdir_kind}"
    fi
  done
fi

SCORE_ARGS=(--fixture "${TASK_FIXTURE}" --artifacts-dir "${ARTIFACTS_DIR}" --human)
if [[ "${SESSION_INJECTION}" -eq 1 ]]; then
  SCORE_ARGS+=(--treatment-arm repo_intelligence)
fi

"${CODEX_BIN}" context agent-eval score "${SCORE_ARGS[@]}"

echo "artifacts_dir: ${ARTIFACTS_DIR}"
