#!/usr/bin/env bash
# Vanilla vs harness-context agent eval on shared tasks (optional OSS runs + deterministic score).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CODEX_RS_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_SRC="${CODEX_RS_ROOT}/context-harness/tests/fixtures/e2e_python_calculator"
CALCULATOR_TASK_FIXTURE="${CODEX_RS_ROOT}/context-harness/tests/fixtures/agent_eval_tasks.json"
# v1 RI fixture: 15 codex_rs tasks across the five RI categories. Requires
# --isolated-worktrees because every task edits the live codex-rs tree.
CODEX_SESSION_TASK_FIXTURE="${CODEX_RS_ROOT}/context-harness/tests/fixtures/agent_eval_tasks_ri_v1.json"
TASK_FIXTURE="${CALCULATOR_TASK_FIXTURE}"
FIXTURE_EXPLICIT=0

CODEX_BIN=""
ARTIFACTS_DIR=""
RUN_AGENT=0
SESSION_INJECTION=0
VERBOSE=0
ISOLATED_WORKTREES=0
BASE_REF="HEAD"
BASE_REF_SHA=""
REPO_ROOT=""
OSS_ARGS=()

usage() {
  cat <<'EOF'
Usage: harness-agent-eval.sh [--codex-bin PATH] [--artifacts-dir DIR] [--fixture PATH]
                             [--run] [--session-injection] [--isolated-worktrees] [--base-ref REF]
                             [--verbose] [--oss ...]

Compares vanilla Codex vs treatment Codex on the same tasks.

Modes:
  default: vanilla vs harness, where the harness arm prepends a manual
    `codex context build` prompt fragment.
  --session-injection: vanilla vs repo_intelligence, where the treatment arm
    runs `codex exec -c features.repo_intelligence=true` and relies on session
    context injection instead of a manual prompt prefix. This mode scores
    repo_intelligence artifacts as the treatment arm.

Fixtures:
  default: context-harness/tests/fixtures/agent_eval_tasks.json
    Uses workdir "calculator", which copies the Python calculator fixture into
    a fresh temporary git repo for each arm.
  --session-injection default: context-harness/tests/fixtures/agent_eval_tasks_ri_v1.json
    (15 codex_rs tasks across file_routing / bridge_wiring / test_targeting /
    local_convention / cross_module_ownership), unless --fixture is set.
  Task fixtures may set "workdir": "codex_rs" to run the task directly in this
    codex-rs checkout instead of a copied temp fixture. Without
    --isolated-worktrees, both arms share ${CODEX_RS_ROOT} (their edits and
    git diffs leak across arms — only safe on a disposable checkout).

Isolation:
  --isolated-worktrees   For "codex_rs" workdirs, give each arm a fresh
                         `git worktree add --detach` rooted at --base-ref and
                         clean it up after artifact capture. Calculator-mode
                         arms are already isolated by `mktemp -d` + `git init`.
  --base-ref REF         Resolved by `git rev-parse REF` against the codex-rs
                         repo root before any arms run. Default: HEAD.

Without --run: scores existing artifacts only (requires record.json per task/arm).
With --run: executes both arms via `codex exec --json` (needs a working model provider).

Artifacts layout:
  ARTIFACTS_DIR/<task_id>/vanilla/record.json
  ARTIFACTS_DIR/<task_id>/harness/record.json   (manual prefix arm)
  ARTIFACTS_DIR/<task_id>/repo_intelligence/record.json   (--session-injection)

Metrics: harness_context_visible, target files (gold ∪ bridge), tests pass,
turn count, unnecessary files, token usage (input/output/total per arm).
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
    --isolated-worktrees)
      ISOLATED_WORKTREES=1
      shift
      ;;
    --base-ref)
      BASE_REF="$2"
      shift 2
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

resolve_isolation_base() {
  if [[ "${ISOLATED_WORKTREES}" -ne 1 ]]; then
    return
  fi
  REPO_ROOT="$(git -C "${CODEX_RS_ROOT}" rev-parse --show-toplevel 2>/dev/null || true)"
  if [[ -z "${REPO_ROOT}" ]]; then
    echo "--isolated-worktrees requires a git repo; ${CODEX_RS_ROOT} is not inside one" >&2
    exit 2
  fi
  BASE_REF_SHA="$(git -C "${REPO_ROOT}" rev-parse "${BASE_REF}" 2>/dev/null || true)"
  if [[ -z "${BASE_REF_SHA}" ]]; then
    echo "--base-ref ${BASE_REF} could not be resolved in ${REPO_ROOT}" >&2
    exit 2
  fi
}

# Create the per-arm workdir. Echoes a single line "<workdir>|<isolated>|<base_ref>" so the
# caller can record it; non-isolated calculator runs report isolated=true,base_ref="" because
# `mktemp -d` + `git init` already guarantees per-arm isolation.
create_arm_workdir() {
  local workdir_kind="$1"
  local workdir worktree_root
  local isolated=true
  local base_ref=""
  if [[ "${workdir_kind}" == "codex_rs" ]]; then
    if [[ "${ISOLATED_WORKTREES}" -eq 1 ]]; then
      worktree_root="$(mktemp -d -t codex-arm-XXXXXX)"
      # `git worktree add` insists on a path that does not yet exist.
      rmdir "${worktree_root}"
      git -C "${REPO_ROOT}" worktree add --detach -q "${worktree_root}" "${BASE_REF_SHA}"
      workdir="${worktree_root}/codex-rs"
      base_ref="${BASE_REF_SHA}"
    else
      workdir="${CODEX_RS_ROOT}"
      isolated=false
    fi
  else
    workdir="$(mktemp -d)"
    cp -R "${FIXTURE_SRC}/." "${workdir}/"
    (
      cd "${workdir}"
      git init -q
      git add -A
      git commit -q -m initial
    )
  fi
  printf '%s|%s|%s\n' "${workdir}" "${isolated}" "${base_ref}"
}

# Remove an isolated worktree. No-op for shared codex_rs runs and for
# calculator mktemp dirs (those are cleaned later via OS tmp reaping).
cleanup_arm_workdir() {
  local workdir_kind="$1"
  local workdir="$2"
  local isolated="$3"
  if [[ "${workdir_kind}" == "codex_rs" && "${isolated}" == "true" ]]; then
    local worktree_root
    worktree_root="$(dirname "${workdir}")"
    git -C "${REPO_ROOT}" worktree remove --force "${worktree_root}" 2>/dev/null || true
  fi
}

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

needle = "Harness repo intelligence:"
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

classify_run_validity() {
  local events="$1"
  local exec_exit="$2"
  python3 - "${events}" "${exec_exit}" <<'PY'
import json
import sys
from pathlib import Path

events_path = Path(sys.argv[1])
exec_exit = int(sys.argv[2])

def classify_provider_error(message: str):
    m = (message or "").lower()
    if any(token in m for token in ["usage limit", "quota", "rate limit"]):
        return "provider_usage_limit"
    if any(token in m for token in ["unauthorized", "forbidden", "invalid api key", "authentication", "auth"]):
        return "provider_auth_error"
    if any(token in m for token in ["network", "connection", "timeout", "timed out", "dns", "tls", "socket"]):
        return "provider_network_error"
    return None

if not events_path.exists():
    print("false\tmissing_events")
    raise SystemExit(0)

has_turn_completed = False
has_turn_failed = False
error_messages = []
has_thread_started = False
has_turn_started = False

for raw in events_path.read_text(encoding="utf-8", errors="ignore").splitlines():
    line = raw.strip()
    if not line:
        continue
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    kind = event.get("type")
    if kind == "thread.started":
        has_thread_started = True
    elif kind == "turn.started":
        has_turn_started = True
    elif kind == "turn.completed":
        has_turn_completed = True
    elif kind == "turn.failed":
        has_turn_failed = True
        message = ((event.get("error") or {}).get("message")) or ""
        if message:
            error_messages.append(message)
    elif kind == "error":
        message = event.get("message") or ""
        if message:
            error_messages.append(message)

for message in error_messages:
    reason = classify_provider_error(message)
    if reason:
        print(f"false\t{reason}")
        raise SystemExit(0)

if has_turn_failed:
    print("false\tturn_failed")
elif not has_thread_started or not has_turn_started:
    print("false\tmissing_events")
elif not has_turn_completed:
    print("false\tmissing_events")
elif exec_exit != 0:
    print("false\trunner_error")
else:
    print("true\t")
PY
}

write_record() {
  local out="$1"
  local arm="$2"
  local task_id="$3"
  local changed_json="$4"
  local tests_passed="$5"
  local turn_count="$6"
  local exec_exit="$7"
  local events="$8"
  local run_valid="$9"
  local invalid_reason="${10}"
  local tokens_input="${11}"
  local tokens_output="${12}"
  local tokens_total="${13}"
  # Worktree metadata: passed via env (RECORD_WORKTREE_ISOLATED /
  # RECORD_BASE_REF / RECORD_WORKTREE_PATH) to keep the positional surface
  # bounded. `serde(default)` on the Rust side accepts records that lack
  # these keys.
  local repo_intel_enabled=false
  local harness_visible
  harness_visible="$(harness_context_visible_for_run "${events}")"
  if [[ "${arm}" == "repo_intelligence" ]]; then
    repo_intel_enabled=true
  fi
  mkdir -p "$(dirname "${out}")"
  RECORD_WORKTREE_ISOLATED="${RECORD_WORKTREE_ISOLATED:-false}" \
  RECORD_BASE_REF="${RECORD_BASE_REF:-}" \
  RECORD_WORKTREE_PATH="${RECORD_WORKTREE_PATH:-}" \
  python3 - "${out}" "${arm}" "${task_id}" "${changed_json}" "${tests_passed}" "${turn_count}" "${exec_exit}" "${repo_intel_enabled}" "${harness_visible}" "${run_valid}" "${invalid_reason}" "${tokens_input}" "${tokens_output}" "${tokens_total}" <<'PY'
import json, os, sys
(
    out,
    arm,
    task_id,
    changed_json,
    tests_passed,
    turn_count,
    exec_exit,
    repo_intel,
    harness_visible,
    run_valid,
    invalid_reason,
    tokens_input,
    tokens_output,
    tokens_total,
) = sys.argv[1:15]

def opt_int(value):
    return int(value) if value not in ("", "null") else None

def opt_str(value):
    return value if value else None

record = {
    "arm": arm,
    "task_id": task_id,
    "changed_files": json.loads(changed_json),
    "tests_passed": tests_passed == "true",
    "turn_count": opt_int(turn_count),
    "exec_exit_code": opt_int(exec_exit),
    "repo_intelligence_enabled": repo_intel == "true",
    "harness_context_visible": harness_visible == "true",
    "run_valid": run_valid == "true",
    "invalid_reason": (invalid_reason or None),
    "tokens_input": opt_int(tokens_input),
    "tokens_output": opt_int(tokens_output),
    "tokens_total": opt_int(tokens_total),
    "worktree_isolated": os.environ.get("RECORD_WORKTREE_ISOLATED") == "true",
    "base_ref": opt_str(os.environ.get("RECORD_BASE_REF", "")),
    "worktree_path": opt_str(os.environ.get("RECORD_WORKTREE_PATH", "")),
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(record, f, indent=2)
    f.write("\n")
PY
}

# Sum input/output tokens across turn.completed events. Emits three lines
# (tokens_input, tokens_output, tokens_total) — each is an integer if at
# least one turn.completed event was seen, otherwise "null". `null` here
# means "no completed turn observed", which is distinct from a completed
# turn that reported zero usage.
count_tokens() {
  local events="$1"
  python3 - "${events}" <<'PY'
import json, sys
from pathlib import Path

events_path = Path(sys.argv[1])
seen_completed = False
input_total = 0
output_total = 0

if events_path.exists():
    with events_path.open(encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("type") != "turn.completed":
                continue
            seen_completed = True
            usage = ev.get("usage") or {}
            input_total += max(int(usage.get("input_tokens") or 0), 0)
            output_total += max(int(usage.get("output_tokens") or 0), 0)

if seen_completed:
    print(input_total)
    print(output_total)
    print(input_total + output_total)
else:
    print("null")
    print("null")
    print("null")
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
  local workdir_kind="${5:-calculator}"
  local workdir_info workdir worktree_isolated base_ref
  workdir_info="$(create_arm_workdir "${workdir_kind}")"
  workdir="${workdir_info%%|*}"
  workdir_info="${workdir_info#*|}"
  worktree_isolated="${workdir_info%%|*}"
  base_ref="${workdir_info#*|}"
  cd "${workdir}"

  local prompt="${task_text}"
  if [[ "${arm}" == "harness" ]]; then
    local fragment
    fragment="$("${CODEX_BIN}" context build "${task_text}" \
      --changed src/calculator.py --cwd . --prompt-fragment 2>/dev/null || true)"
    prompt="${fragment}

${task_text}"
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
  # Capture both modified tracked files (vs HEAD / base_ref) and untracked
  # files honoring .gitignore. `git diff --name-only` alone misses new files,
  # which are the common shape of "add a regression test" tasks and would
  # zero out target_files_hit in the score.
  changed_json="$( { git diff --name-only HEAD; git ls-files --others --exclude-standard; } | python3 -c 'import json,sys; print(json.dumps(sorted(set(l for l in sys.stdin.read().splitlines() if l.strip()))))')"
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
  local tokens_input tokens_output tokens_total
  {
    read -r tokens_input
    read -r tokens_output
    read -r tokens_total
  } < <(count_tokens "${events}")
  local validity
  validity="$(classify_run_validity "${events}" "${exec_exit}")"
  local run_valid
  run_valid="${validity%%$'\t'*}"
  local invalid_reason
  invalid_reason="${validity#*$'\t'}"
  if [[ "${run_valid}" == "true" ]]; then
    invalid_reason=""
  fi
  RECORD_WORKTREE_ISOLATED="${worktree_isolated}" \
  RECORD_BASE_REF="${base_ref}" \
  RECORD_WORKTREE_PATH="${workdir}" \
  write_record "${ARTIFACTS_DIR}/${task_id}/${arm}/record.json" "${arm}" "${task_id}" \
    "${changed_json}" "${tests_passed}" "${turns}" "${exec_exit}" "${events}" \
    "${run_valid}" "${invalid_reason}" "${tokens_input}" "${tokens_output}" "${tokens_total}"
  cd "${CODEX_RS_ROOT}"
  cleanup_arm_workdir "${workdir_kind}" "${workdir}" "${worktree_isolated}"
  log "arm=${arm} task=${task_id} workdir=${workdir} isolated=${worktree_isolated} exit=${exec_exit}"
}

resolve_codex_bin
resolve_isolation_base
if [[ -z "${ARTIFACTS_DIR}" ]]; then
  ARTIFACTS_DIR="$(mktemp -d)"
  log "artifacts: ${ARTIFACTS_DIR}"
fi
if [[ "${ISOLATED_WORKTREES}" -eq 1 ]]; then
  log "isolated worktrees enabled; base_ref=${BASE_REF} (${BASE_REF_SHA}) repo=${REPO_ROOT}"
fi

if [[ "${RUN_AGENT}" -eq 1 ]]; then
  # Use ASCII Unit Separator (\x1f), not tab, so an empty verify_command
  # does not cause adjacent IFS whitespace to collapse and shift fields
  # (which makes workdir_kind silently default to "calculator" and turns
  # the next task's id into a stray `eval` argument).
  while IFS=$'\x1f' read -r id task verify workdir_kind || [[ -n "${id:-}" ]]; do
    [[ -z "${id}" ]] && break
    workdir_kind="${workdir_kind:-calculator}"
    if [[ "${SESSION_INJECTION}" -eq 1 ]]; then
      run_arm vanilla "${id}" "${task}" "${verify}" "${workdir_kind}"
      run_arm repo_intelligence "${id}" "${task}" "${verify}" "${workdir_kind}"
    else
      run_arm vanilla "${id}" "${task}" "${verify}" "${workdir_kind}"
      run_arm harness "${id}" "${task}" "${verify}" "${workdir_kind}"
    fi
  done < <(python3 - "${TASK_FIXTURE}" <<'PY'
import json, sys
tasks = json.load(open(sys.argv[1], encoding="utf-8"))
for t in tasks:
    print(
        t["id"],
        t["task"],
        t.get("verify_command") or "",
        t.get("workdir", "calculator"),
        sep="\x1f",
    )
PY
)
fi

SCORE_ARGS=(--fixture "${TASK_FIXTURE}" --artifacts-dir "${ARTIFACTS_DIR}" --human)
if [[ "${SESSION_INJECTION}" -eq 1 ]]; then
  SCORE_ARGS+=(--treatment-arm repo_intelligence)
fi

"${CODEX_BIN}" context agent-eval score "${SCORE_ARGS[@]}"

echo "artifacts_dir: ${ARTIFACTS_DIR}"
