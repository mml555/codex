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
RESCORE_ARTIFACTS_DIR=""
SESSION_INJECTION=0
VERBOSE=0
ISOLATED_WORKTREES=0
BASE_REF="HEAD"
BASE_REF_SHA=""
REPO_ROOT=""
OSS_ARGS=()
# Extra args passed verbatim to every `codex exec` call. Used by --cloud-args
# to inject `-c model_provider=azure -m gpt-5.3-codex` (or any other config
# override) without flipping codex's --oss path. Empty unless --cloud-args
# is set.
CLOUD_EXTRA_ARGS=()
# Resilience guards (added after Run 1 / Run 2 post-mortems):
EVAL_CARGO_TARGET_DIR=""       # --cargo-target-dir; default /tmp/codex-ri-eval-cargo-target
SAFE_CODEX_BIN_DIR=""          # holds a write-protected copy of codex; survives target/ wipes
MAX_TARGET_DIR_GB=""           # --max-target-dir-gb N; bail before disk-out
# Worktrees the current run has created. Populated by create_arm_workdir,
# deregistered by cleanup_arm_workdir, and force-cleaned on EXIT.
ACTIVE_WORKTREES=()

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

Resilience guards (apply when --isolated-worktrees is set):
  --cargo-target-dir PATH  Shared CARGO_TARGET_DIR for agent-initiated cargo
                           runs. Default: /tmp/codex-ri-eval-cargo-target.
                           MUST NOT be the source tree's target/ — Run 2
                           died when the agent in one arm wiped the binary
                           via that shared path.
  --max-target-dir-gb N    Abort the run before launching an arm if the
                           cargo target dir already exceeds N GB. Default
                           is unbounded; set this when disk is tight.
  trap EXIT                Always installed. Force-removes every worktree
                           the run registered, on any exit path (crash,
                           Ctrl-C, normal end).

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
    --rescore-artifacts)
      # Re-classify existing record.json files under DIR using the
      # current classifier (validity, visibility, activity) WITHOUT
      # re-running codex. DIR is expected to mirror the artifacts
      # layout: DIR/<task_id>/<arm>/{events.jsonl, record.json}.
      # Use this after instrumentation fixes to backfill artifacts
      # produced by an older runner.
      RESCORE_ARTIFACTS_DIR="$2"
      shift 2
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
    --cargo-target-dir)
      EVAL_CARGO_TARGET_DIR="$2"
      shift 2
      ;;
    --max-target-dir-gb)
      MAX_TARGET_DIR_GB="$2"
      shift 2
      ;;
    --oss)
      OSS_ARGS=(--oss)
      shift
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --codex-bin | --artifacts-dir | --fixture | --run | --session-injection \
            | --isolated-worktrees | --base-ref | --cargo-target-dir | --max-target-dir-gb \
            | --cloud-args \
            | --verbose | -h | --help) break ;;
          *)
            OSS_ARGS+=("$1")
            shift
            ;;
        esac
      done
      ;;
    --cloud-args)
      # Collect every following token until a known flag is hit (same
      # parser shape as --oss, but does NOT prepend --oss itself).
      # Example: --cloud-args -c model_provider=azure -m gpt-5.3-codex
      shift
      while [[ $# -gt 0 ]]; do
        case "$1" in
          --codex-bin | --artifacts-dir | --fixture | --run | --session-injection \
            | --isolated-worktrees | --base-ref | --cargo-target-dir | --max-target-dir-gb \
            | --oss \
            | --verbose | -h | --help) break ;;
          *)
            CLOUD_EXTRA_ARGS+=("$1")
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
      # Register so trap EXIT can sweep us if the run dies mid-arm.
      ACTIVE_WORKTREES+=("${worktree_root}")
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
    rm -rf "${worktree_root}" 2>/dev/null || true
    # Deregister from ACTIVE_WORKTREES (preserve original order for any peers).
    local new=()
    for wt in ${ACTIVE_WORKTREES[@]+"${ACTIVE_WORKTREES[@]}"}; do
      [[ "${wt}" != "${worktree_root}" ]] && new+=("${wt}")
    done
    ACTIVE_WORKTREES=(${new[@]+"${new[@]}"})
  fi
}

# Trap-installed: sweep every worktree the run still has registered. The
# EXIT pseudosignal fires on clean termination, `exit N`, and `set -e`
# propagation. SIGTERM (kill, parent-process death), SIGINT (Ctrl-C), and
# SIGHUP (terminal close) DO NOT fire EXIT by default in bash — they
# terminate the shell before any pending EXIT trap runs. Trap each
# explicitly so a `kill` from outside the script still gets us a clean
# sweep before bash exits.
cleanup_all_active_worktrees() {
  local count=${#ACTIVE_WORKTREES[@]}
  if [[ "${count}" -eq 0 ]]; then
    return
  fi
  echo "trap: force-removing ${count} orphan worktree(s)" >&2
  for wt in ${ACTIVE_WORKTREES[@]+"${ACTIVE_WORKTREES[@]}"}; do
    if [[ -n "${REPO_ROOT}" ]]; then
      git -C "${REPO_ROOT}" worktree remove --force "${wt}" 2>/dev/null || true
    fi
    rm -rf "${wt}" 2>/dev/null || true
  done
  ACTIVE_WORKTREES=()
}
trap cleanup_all_active_worktrees EXIT TERM INT HUP

# Ensure the safe codex binary still exists. If an agent in some arm wiped
# the source tree's target/, the copy under SAFE_CODEX_BIN_DIR survives
# unless the agent also reaches into /tmp. Either way we want to fail fast
# with a clear message before invoking exit-127 ten more times.
ensure_codex_bin_alive() {
  if [[ -x "${CODEX_BIN}" ]]; then
    return 0
  fi
  echo "ERROR: codex binary missing at ${CODEX_BIN}" >&2
  echo "  An agent in a previous arm likely deleted it." >&2
  echo "  Re-run with --codex-bin pointing to a freshly-built binary." >&2
  return 1
}

# Check the shared cargo target dir size against --max-target-dir-gb.
# Returns non-zero (and prints a message) if the threshold would be exceeded.
check_target_dir_size() {
  if [[ -z "${MAX_TARGET_DIR_GB:-}" || -z "${EVAL_CARGO_TARGET_DIR}" ]]; then
    return 0
  fi
  [[ -d "${EVAL_CARGO_TARGET_DIR}" ]] || return 0
  local kb gb
  kb="$(du -sk "${EVAL_CARGO_TARGET_DIR}" 2>/dev/null | cut -f1)"
  gb=$((kb / 1024 / 1024))
  if [[ "${gb}" -ge "${MAX_TARGET_DIR_GB}" ]]; then
    echo "ABORT: ${EVAL_CARGO_TARGET_DIR} is ${gb} GB >= --max-target-dir-gb ${MAX_TARGET_DIR_GB}" >&2
    return 1
  fi
  return 0
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
# Detect whether the harness directive marker reached the model as PROMPT
# context (input message), not as TOOL OUTPUT echoed back from a shell
# grep over codex source files.
#
# The legacy "scan events.jsonl + raw rollout text" approach matched the
# marker anywhere it appeared, including in `function_call_output`
# payloads emitted when agents shell-grep'd the renderer source for the
# `HARNESS_MARKER` constant. That's a false positive: the model saw the
# marker because it searched for it, not because the harness injected it.
#
# Prompt-only rule: the marker only counts when it appears in a
# `response_item` whose `payload.type == "message"` and whose `role` is
# one of {user, developer, system}. Other payload types — including
# function_call_output, function_call, reasoning, custom_tool_call — do
# NOT count.
import json
import sys
from pathlib import Path

NEEDLE = "Harness repo intelligence:"
PROMPT_ROLES = {"user", "developer", "system"}

events_path = Path(sys.argv[1])
codex_home = Path(sys.argv[2])

def _content_carries_needle(payload) -> bool:
    """Return True iff the `content` field of a message payload contains
    the harness directive marker. `content` is either a flat string or
    an array of structured parts (e.g. `{"type":"input_text","text":...}`).
    """
    content = payload.get("content")
    if isinstance(content, str):
        return NEEDLE in content
    if isinstance(content, list):
        for part in content:
            if isinstance(part, str) and NEEDLE in part:
                return True
            if isinstance(part, dict):
                text = part.get("text")
                if isinstance(text, str) and NEEDLE in text:
                    return True
    return False

def thread_id_from_events() -> str:
    if not events_path.exists():
        return ""
    with events_path.open(encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("type") == "thread.started":
                return ev.get("thread_id") or ""
    return ""

def visible_in_rollout_prompt() -> bool:
    thread_id = thread_id_from_events()
    if not thread_id:
        return False
    sessions = codex_home / "sessions"
    if not sessions.is_dir():
        return False
    for rollout in sessions.rglob(f"*{thread_id}*.jsonl"):
        try:
            with rollout.open(encoding="utf-8", errors="ignore") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        ev = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if ev.get("type") != "response_item":
                        continue
                    payload = ev.get("payload") or {}
                    if payload.get("type") != "message":
                        continue
                    role = payload.get("role") or ""
                    if role not in PROMPT_ROLES:
                        continue
                    if _content_carries_needle(payload):
                        return True
        except OSError:
            continue
    return False

print("true" if visible_in_rollout_prompt() else "false")
PY
}

# Extract the "Likely edit targets" and "Orientation only" file lists
# that the RI directive prompt rendered for THIS run. Walks the rollout
# (same lookup as harness_context_visible_for_run), finds user/developer
# message payloads carrying the harness marker, and parses the two
# named sections out of the content. Emits two newline-separated
# blocks separated by ASCII Unit Separator (\x1f):
#   <edit_target_files separated by \n> \x1f <orientation_files separated by \n>
# Either block can be empty. Empty for vanilla arms (no RI directive
# in the prompt) and for pre-split fragments (no `Likely edit targets:`
# header — the legacy `Before editing, inspect these files first:`
# block is intentionally not back-parsed; see the Rust test
# `parse_directive_file_lists_handles_legacy_single_section_fragment`).
extract_ri_surfaced_files() {
  local events="$1"
  local codex_home="${CODEX_HOME:-${HOME}/.codex}"
  python3 - "${events}" "${codex_home}" <<'PY'
import json
import sys
from pathlib import Path

NEEDLE = "Harness repo intelligence:"
EDIT_HEADER = "Likely edit targets:"
ORIENT_HEADER = "Orientation only:"
PROMPT_ROLES = {"user", "developer", "system"}

events_path = Path(sys.argv[1])
codex_home = Path(sys.argv[2])

def thread_id_from_events() -> str:
    if not events_path.exists():
        return ""
    with events_path.open(encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("type") == "thread.started":
                return ev.get("thread_id") or ""
    return ""

def directive_text_from_payload(payload) -> str:
    """Concatenate every text fragment in a message payload's content."""
    content = payload.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        out = []
        for part in content:
            if isinstance(part, str):
                out.append(part)
            elif isinstance(part, dict):
                txt = part.get("text")
                if isinstance(txt, str):
                    out.append(txt)
        return "\n".join(out)
    return ""

def parse_numbered_entry(line: str):
    """Pull `<path>` out of a `N. <path> — <reason>` line; mirrors the
    Rust parser. Returns None on miss."""
    s = line.lstrip()
    if not s or not s[0].isdigit():
        return None
    i = 0
    while i < len(s) and s[i].isdigit():
        i += 1
    if i >= len(s) or s[i] != ".":
        return None
    rest = s[i + 1 :]
    if not rest.startswith(" "):
        return None
    rest = rest[1:]
    # Path is everything up to the em-dash separator (U+2014).
    sep = " — "
    head = rest.split(sep, 1)[0].strip()
    return head or None

def parse_lists(fragment: str):
    if NEEDLE not in fragment:
        return [], []
    edit, orient = [], []
    section = None
    for raw in fragment.splitlines():
        line = raw.rstrip()
        if line == EDIT_HEADER:
            section = edit
            continue
        if line == ORIENT_HEADER:
            section = orient
            continue
        if not line.strip():
            continue
        path = parse_numbered_entry(line)
        if path is not None and section is not None:
            section.append(path)
            continue
        # Anything else closes the section.
        section = None
    return edit, orient

def lists_from_rollout():
    thread_id = thread_id_from_events()
    if not thread_id:
        return [], []
    sessions = codex_home / "sessions"
    if not sessions.is_dir():
        return [], []
    for rollout in sessions.rglob(f"*{thread_id}*.jsonl"):
        try:
            with rollout.open(encoding="utf-8", errors="ignore") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        ev = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if ev.get("type") != "response_item":
                        continue
                    payload = ev.get("payload") or {}
                    if payload.get("type") != "message":
                        continue
                    role = payload.get("role") or ""
                    if role not in PROMPT_ROLES:
                        continue
                    text = directive_text_from_payload(payload)
                    if NEEDLE not in text:
                        continue
                    e, o = parse_lists(text)
                    if e or o:
                        return e, o
        except OSError:
            continue
    return [], []

edit_targets, orientation = lists_from_rollout()
print("\n".join(edit_targets), end="\x1f")
print("\n".join(orientation), end="")
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
    # Three fields separated by ASCII Unit Separator (\x1f):
    #   <valid> \x1f <invalid_reason> \x1f <warnings_csv>
    # warnings_csv is comma-separated, empty when no warnings. We avoid
    # tab here because bash `read -r a b c` with IFS=$'\t' collapses
    # adjacent tabs (tab is IFS-whitespace), so `true\t\twarn` would
    # parse as 2 fields and lose the warning. \x1f is non-whitespace,
    # which preserves empty fields between separators.
    print("false\x1fmissing_events\x1f")
    raise SystemExit(0)

has_turn_completed = False
has_turn_failed = False
turn_failed_messages = []
error_messages = []
non_fatal_messages = []
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
            turn_failed_messages.append(message)
            error_messages.append(message)
    elif kind == "error":
        # `error` events fire mid-stream (e.g. provider_network_error during
        # a Responses stream that codex then auto-reconnects through). They
        # are NOT terminal; only co-occurrence with turn.failed or missing
        # turn.completed promotes them to a hard failure.
        message = event.get("message") or ""
        if message:
            non_fatal_messages.append(message)
            error_messages.append(message)

warnings = []

# Terminal failure dominates: if a turn.failed event carries a provider
# error, classify by that reason. (provider_usage_limit, auth, network)
for message in turn_failed_messages:
    reason = classify_provider_error(message)
    if reason:
        print(f"false\x1f{reason}\x1f")
        raise SystemExit(0)

# Hard runner failures: no turn ever completed.
if has_turn_failed:
    print("false\x1fturn_failed\x1f")
    raise SystemExit(0)
if not has_thread_started or not has_turn_started:
    print("false\x1fmissing_events\x1f")
    raise SystemExit(0)
if not has_turn_completed:
    # Mid-stream error events without recovery → still terminal. Surface
    # the most specific reason we can derive.
    for message in non_fatal_messages:
        reason = classify_provider_error(message)
        if reason:
            print(f"false\x1f{reason}\x1f")
            raise SystemExit(0)
    print("false\x1fmissing_events\x1f")
    raise SystemExit(0)
if exec_exit != 0:
    print("false\x1frunner_error\x1f")
    raise SystemExit(0)

# At this point: thread.started + turn.started + turn.completed + exit 0.
# That's a behaviorally complete run. If any mid-stream `error` event
# fired for a recoverable reason and codex still drove the turn to
# completion, record it as a NON-FATAL warning on an otherwise valid run.
for message in non_fatal_messages:
    reason = classify_provider_error(message)
    if reason == "provider_network_error":
        warnings.append("provider_network_error_recovered")
        break  # one warning per category is enough

print(f"true\x1f\x1f{','.join(warnings)}")
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
  # Worktree metadata + activity/duration: all passed via env to keep the
  # positional surface bounded. `serde(default)` on the Rust side accepts
  # records that lack any of these keys.
  local repo_intel_enabled=false
  local harness_visible
  harness_visible="$(harness_context_visible_for_run "${events}")"
  if [[ "${arm}" == "repo_intelligence" ]]; then
    repo_intel_enabled=true
  fi
  # RI-surfaced file lists (edit targets + orientation) parsed from the
  # rollout's directive prompt. Empty unless the rollout carries the
  # new two-section format. The extractor emits two \n-joined blocks
  # separated by \x1f; we split here so the Python record writer can
  # pass both into the JSON.
  local ri_surfaced
  ri_surfaced="$(extract_ri_surfaced_files "${events}")"
  local ri_edit_targets ri_orientation
  IFS=$'\x1f' read -r ri_edit_targets ri_orientation <<<"${ri_surfaced}"
  mkdir -p "$(dirname "${out}")"
  RECORD_WORKTREE_ISOLATED="${RECORD_WORKTREE_ISOLATED:-false}" \
  RECORD_BASE_REF="${RECORD_BASE_REF:-}" \
  RECORD_WORKTREE_PATH="${RECORD_WORKTREE_PATH:-}" \
  RECORD_DURATION_MS="${RECORD_DURATION_MS:-}" \
  RECORD_TOOL_CALL_COUNT="${RECORD_TOOL_CALL_COUNT:-}" \
  RECORD_SHELL_COMMAND_COUNT="${RECORD_SHELL_COMMAND_COUNT:-}" \
  RECORD_FILE_READ_COUNT="${RECORD_FILE_READ_COUNT:-}" \
  RECORD_DISCOVER_COMMAND_COUNT="${RECORD_DISCOVER_COMMAND_COUNT:-}" \
  RECORD_EDIT_COMMAND_COUNT="${RECORD_EDIT_COMMAND_COUNT:-}" \
  RECORD_VERIFY_COMMAND_COUNT="${RECORD_VERIFY_COMMAND_COUNT:-}" \
  RECORD_WARNINGS="${RECORD_WARNINGS:-}" \
  RECORD_RI_SURFACED_EDIT_TARGETS="${ri_edit_targets}" \
  RECORD_RI_SURFACED_ORIENTATION="${ri_orientation}" \
  RECORD_INTENT_CHANGED_FILES="$(extract_intent_changed_files "${events}")" \
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

def opt_env_int(name):
    raw = os.environ.get(name, "")
    return int(raw) if raw not in ("", "null") else None

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
    "duration_ms": opt_env_int("RECORD_DURATION_MS"),
    "tool_call_count": opt_env_int("RECORD_TOOL_CALL_COUNT"),
    "shell_command_count": opt_env_int("RECORD_SHELL_COMMAND_COUNT"),
    "file_read_count": opt_env_int("RECORD_FILE_READ_COUNT"),
    "discover_command_count": opt_env_int("RECORD_DISCOVER_COMMAND_COUNT"),
    "edit_command_count": opt_env_int("RECORD_EDIT_COMMAND_COUNT"),
    "verify_command_count": opt_env_int("RECORD_VERIFY_COMMAND_COUNT"),
    # Warnings: comma-separated slugs in RECORD_WARNINGS, emitted as a
    # JSON string array. Reviewer-facing only — the validity classifier
    # has already cleared this run as behaviorally valid.
    "warnings": [w for w in os.environ.get("RECORD_WARNINGS", "").split(",") if w],
    # RI-surfaced file lists. Each env var is a \n-joined block from
    # `extract_ri_surfaced_files`. Empty for vanilla arms and for
    # pre-split rollouts that used the legacy single-section header.
    "ri_surfaced_edit_targets": [
        p for p in os.environ.get("RECORD_RI_SURFACED_EDIT_TARGETS", "").split("\n") if p
    ],
    "ri_surfaced_orientation": [
        p for p in os.environ.get("RECORD_RI_SURFACED_ORIENTATION", "").split("\n") if p
    ],
    "worktree_isolated": os.environ.get("RECORD_WORKTREE_ISOLATED") == "true",
    # See bottom of this dict literal — `intent_changed_files`,
    # `diff_changed_files`, `formatter_changed_files` are assembled
    # AFTER the record is built so they can reference `changed_files`
    # and `intent_changed_files` together.
    "base_ref": opt_str(os.environ.get("RECORD_BASE_REF", "")),
    "worktree_path": opt_str(os.environ.get("RECORD_WORKTREE_PATH", "")),
}

# Assemble the intent/diff/formatter triplet. `intent_changed_files`
# is the authoritative model-intent set (from `file_change` events).
# `diff_changed_files` mirrors `changed_files` and is purely
# diagnostic. `formatter_changed_files = diff − intent` so reviewers
# can see at a glance how much collateral a run accumulated.
intent_paths = [
    p for p in os.environ.get("RECORD_INTENT_CHANGED_FILES", "").split("\n") if p
]
diff_paths = list(record["changed_files"])
intent_set = set(intent_paths)

def _norm(p: str) -> str:
    # Mirror Rust normalize_agent_eval_path for the diff side, since
    # the runner emits `codex-rs/`-prefixed paths but intent_paths are
    # already normalized to repo-relative.
    p = p.replace("\\", "/").strip()
    if p.startswith("./"): p = p[2:]
    idx = p.find("/codex-rs/")
    if idx >= 0: return p[idx + len("/codex-rs/"):]
    while p.startswith("codex-rs/"): p = p[len("codex-rs/"):]
    return p

diff_norm = {_norm(p): p for p in diff_paths if p.strip()}
formatter_paths = [orig for norm, orig in sorted(diff_norm.items()) if norm not in intent_set]
record["intent_changed_files"] = intent_paths
record["diff_changed_files"] = diff_paths
record["formatter_changed_files"] = formatter_paths
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

# Tally tool/shell activity + per-phase shell breakdown from events.jsonl.
# Emits six lines (in order):
#   tool_call_count
#   shell_command_count
#   file_read_count        (= read phase)
#   discover_command_count
#   edit_command_count
#   verify_command_count
# Heuristic mirrors `agent_eval::classify_shell_phase` /
# `count_activity_from_exec_jsonl` so the bash-emitted and Rust-parsed
# counts agree on the same events.jsonl. Phase sums equal shell_command
# count minus an unrecorded "other" residual.
# Extract the set of file paths the model INTENTIONALLY edited during
# the run, sourced from `file_change` items in `events.jsonl`. Mirrors
# `intent_changed_files_from_exec_jsonl` on the Rust side. Emits one
# repo-relative path per line (sorted, deduped). Empty stdout when no
# file_change items were observed (which also covers pre-instrumented
# runs).
#
# Critical because `git diff --name-only HEAD` picks up `cargo fmt
# --all` collateral whenever the agent ran `just fmt` — the rate_limit
# v2 rerun showed git diff reporting 9 files when the model only
# authored 2 patches. Scoring against intent eliminates that noise.
extract_intent_changed_files() {
  local events="$1"
  python3 - "${events}" <<'PY'
import json
import sys
from pathlib import Path

events_path = Path(sys.argv[1])

def normalize(p: str) -> str:
    """Mirror `agent_eval::normalize_agent_eval_path`. Strip the
    `/codex-rs/` prefix wherever it appears so absolute worktree paths
    map to repo-relative paths."""
    p = p.strip().replace("\\", "/")
    if not p:
        return ""
    if p.startswith("./"):
        p = p[2:]
    idx = p.find("/codex-rs/")
    if idx >= 0:
        return p[idx + len("/codex-rs/"):]
    while p.startswith("codex-rs/"):
        p = p[len("codex-rs/"):]
    return p

paths = set()
if events_path.exists():
    with events_path.open(encoding="utf-8", errors="ignore") as f:
        for raw in f:
            raw = raw.strip()
            if not raw:
                continue
            try:
                ev = json.loads(raw)
            except json.JSONDecodeError:
                continue
            if ev.get("type") != "item.completed":
                continue
            item = ev.get("item") or {}
            if item.get("type") != "file_change":
                continue
            for ch in (item.get("changes") or []):
                p = ch.get("path") if isinstance(ch, dict) else None
                if not isinstance(p, str):
                    continue
                norm = normalize(p)
                if norm:
                    paths.add(norm)

for p in sorted(paths):
    print(p)
PY
}

count_activity() {
  local events="$1"
  python3 - "${events}" <<'PY'
import json, sys
from pathlib import Path

FILE_READ_COMMANDS = {"cat", "head", "tail", "less", "more"}
SHELL_WRAPPERS = ("/bin/zsh -lc ", "/bin/bash -c ", "zsh -lc ", "bash -c ")

def first_token(raw: str) -> str:
    s = raw.strip()
    for prefix in SHELL_WRAPPERS:
        if s.startswith(prefix):
            s = s[len(prefix):].lstrip()
            break
    s = s.lstrip("'\"")
    parts = s.split()
    return parts[0] if parts else ""

def strip_leading_cd_chains(cmd: str) -> str:
    """Skip every leading `cd <path> && ` segment so we classify the
    *real* command, not the navigation prefix. Mirrors the Rust
    `strip_leading_cd_chains` helper.
    """
    s = cmd.strip()
    for prefix in SHELL_WRAPPERS:
        if s.startswith(prefix):
            s = s[len(prefix):].lstrip()
            break
    s = s.strip("'\"")
    while s.lstrip().startswith("cd "):
        idx = s.find("&&")
        if idx < 0: break
        s = s[idx + 2:].lstrip()
    return s

def _has_joined_grep_context(lc: str) -> bool:
    """Detect joined grep context flags like `-A5`, `-B5`, `-C5`."""
    import re
    return bool(re.search(r" -[abc]\d", lc))

def classify_phase(cmd: str) -> str:
    normalized = strip_leading_cd_chains(cmd)
    first = first_token(normalized)
    if first in ("find", "ls", "tree", "locate", "fd"):
        return "discover"
    if first in FILE_READ_COMMANDS:
        return "read"
    if first in ("grep", "rg", "ack"):
        lc = normalized.lower()
        # Context flags → post-edit verification rather than discovery.
        # Match BOTH spaced (`-A 3`) and joined (`-A3`) variants, plus
        # long forms.
        if any(m in lc for m in (
            " -a ", " -b ", " -c ", " -a=", " -b=", " -c=",
            " --after-context", " --before-context", " --context",
        )) or _has_joined_grep_context(lc):
            return "verify"
        return "discover"
    if first == "sed":
        return "edit" if "-i" in normalized else "other"
    if first == "perl":
        return "edit" if "-i" in normalized else "other"
    if first == "awk":
        if "inplace" in normalized: return "edit"
        if "print" in normalized: return "verify"
        return "other"
    if first in ("echo", "printf"):
        return "edit" if ">" in normalized else "other"
    if first in ("mv", "cp", "rm", "mkdir", "touch", "rmdir", "ln"):
        return "edit"
    if first == "diff":
        return "verify"
    if first == "git":
        lc = normalized.lower()
        if " diff" in lc or " log" in lc or " show" in lc: return "verify"
        if " status" in lc or " ls-files" in lc or " branch" in lc: return "discover"
        return "other"
    return "other"

events_path = Path(sys.argv[1])
tool_calls = 0
shell_cmds = 0
file_reads = 0
discover = 0
edit = 0
verify = 0
if events_path.exists():
    with events_path.open(encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line: continue
            try: ev = json.loads(line)
            except json.JSONDecodeError: continue
            if ev.get("type") != "item.completed": continue
            tool_calls += 1
            item = ev.get("item") or {}
            item_type = item.get("type")
            # Frontier models edit via the `apply_patch` custom tool, which
            # codex surfaces as `file_change` items (NOT as command_execution
            # shells). Without this branch the cost-table `edit` column
            # reads e=0 even when multiple files changed. Count file_change
            # as an edit but do NOT inflate shell_commands — that counter
            # is reserved for command_execution.
            if item_type == "file_change":
                edit += 1
                continue
            if item_type != "command_execution": continue
            shell_cmds += 1
            cmd = item.get("command")
            if isinstance(cmd, list): cmd = " ".join(cmd)
            elif not isinstance(cmd, str): cmd = ""
            # Derive every phase counter (including file_reads) from
            # classify_phase so they all agree on `cd $X && <real>` chains.
            phase = classify_phase(cmd)
            if phase == "discover": discover += 1
            elif phase == "read":     file_reads += 1
            elif phase == "edit":     edit += 1
            elif phase == "verify":   verify += 1
            # "other" implicit.

print(tool_calls)
print(shell_cmds)
print(file_reads)
print(discover)
print(edit)
print(verify)
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
  # The repo_intelligence arm needs features.repo_intelligence=true regardless
  # of provider mode. Without it the extension early-returns on the gate
  # check and the RI arm reduces to vanilla — invalidating the entire A/B.
  local feature_args=()
  if [[ "${arm}" == "repo_intelligence" ]]; then
    feature_args=(-c features.repo_intelligence=true)
  fi
  # `${arr[@]+"${arr[@]}"}` is the empty-array-safe expansion under `set -u`
  # (bash 3.x on macOS treats an empty `${arr[@]}` as unbound).
  local start_ms end_ms duration_ms
  start_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  set +e
  "${CODEX_BIN}" exec ${OSS_ARGS[@]+"${OSS_ARGS[@]}"} ${CLOUD_EXTRA_ARGS[@]+"${CLOUD_EXTRA_ARGS[@]}"} ${feature_args[@]+"${feature_args[@]}"} -s workspace-write \
    --dangerously-bypass-approvals-and-sandbox \
    --json \
    "${prompt}" </dev/null >"${events}" 2>/dev/null
  local exec_exit=$?
  set -e
  end_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  duration_ms=$((end_ms - start_ms))

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
  local tool_call_count shell_command_count file_read_count
  local discover_command_count edit_command_count verify_command_count
  {
    read -r tool_call_count
    read -r shell_command_count
    read -r file_read_count
    read -r discover_command_count
    read -r edit_command_count
    read -r verify_command_count
  } < <(count_activity "${events}")
  local validity
  validity="$(classify_run_validity "${events}" "${exec_exit}")"
  # classify_run_validity emits THREE fields delimited by ASCII Unit
  # Separator (\x1f):
  #   <run_valid> \x1f <invalid_reason> \x1f <warnings_csv>
  # Fix #1 added the third field. We use \x1f (not \t) because bash
  # `read -r a b c` with IFS=$'\t' treats tab as IFS-whitespace and
  # collapses adjacent tabs — `true\t\twarn` would parse as 2 fields
  # and drop the warning. \x1f is non-whitespace and preserves empties.
  local run_valid invalid_reason warnings_csv
  IFS=$'\x1f' read -r run_valid invalid_reason warnings_csv <<<"${validity}"
  if [[ "${run_valid}" == "true" ]]; then
    invalid_reason=""
  fi
  RECORD_WORKTREE_ISOLATED="${worktree_isolated}" \
  RECORD_BASE_REF="${base_ref}" \
  RECORD_WORKTREE_PATH="${workdir}" \
  RECORD_DURATION_MS="${duration_ms}" \
  RECORD_TOOL_CALL_COUNT="${tool_call_count}" \
  RECORD_SHELL_COMMAND_COUNT="${shell_command_count}" \
  RECORD_FILE_READ_COUNT="${file_read_count}" \
  RECORD_DISCOVER_COMMAND_COUNT="${discover_command_count}" \
  RECORD_EDIT_COMMAND_COUNT="${edit_command_count}" \
  RECORD_VERIFY_COMMAND_COUNT="${verify_command_count}" \
  RECORD_WARNINGS="${warnings_csv}" \
  write_record "${ARTIFACTS_DIR}/${task_id}/${arm}/record.json" "${arm}" "${task_id}" \
    "${changed_json}" "${tests_passed}" "${turns}" "${exec_exit}" "${events}" \
    "${run_valid}" "${invalid_reason}" "${tokens_input}" "${tokens_output}" "${tokens_total}"
  cd "${CODEX_RS_ROOT}"
  cleanup_arm_workdir "${workdir_kind}" "${workdir}" "${worktree_isolated}"
  local target_size="(n/a)"
  if [[ -n "${EVAL_CARGO_TARGET_DIR}" && -d "${EVAL_CARGO_TARGET_DIR}" ]]; then
    target_size="$(du -sh "${EVAL_CARGO_TARGET_DIR}" 2>/dev/null | cut -f1)"
  fi
  # Format duration in seconds for the log line; one-decimal precision.
  local duration_s
  duration_s="$(python3 -c "print(f'{${duration_ms}/1000:.1f}')")"
  log "arm=${arm} task=${task_id} workdir=${workdir} isolated=${worktree_isolated} exit=${exec_exit} duration=${duration_s}s shell=${shell_command_count} (d=${discover_command_count}/r=${file_read_count}/e=${edit_command_count}/v=${verify_command_count}) cargo_target=${target_size}"
}

# Re-classify a single existing record using the live classifier.
# Reads events.jsonl + record.json from a record_dir, runs validity,
# visibility, and activity classifiers, then rewrites record.json in
# place — overwriting only the fields that the classifier owns:
#   run_valid, invalid_reason, warnings, harness_context_visible,
#   tool_call_count, shell_command_count, file_read_count,
#   discover_command_count, edit_command_count, verify_command_count
# All other fields (changed_files, tokens_*, duration_ms, worktree_*,
# tests_passed, turn_count, exec_exit_code, repo_intelligence_enabled,
# arm, task_id) are preserved exactly. Side effect-free if events.jsonl
# is absent.
rescore_record() {
  local record_dir="$1"
  local record_path="${record_dir}/record.json"
  local events_path="${record_dir}/events.jsonl"
  if [[ ! -f "${record_path}" || ! -f "${events_path}" ]]; then
    return 0
  fi
  # Pull exec_exit from the existing record so the validity classifier
  # has the same signal it had at original write time.
  local exec_exit
  exec_exit="$(python3 -c "import json,sys; r=json.load(open('${record_path}',encoding='utf-8')); print(r.get('exec_exit_code') if r.get('exec_exit_code') is not None else 0)")"
  local validity
  validity="$(classify_run_validity "${events_path}" "${exec_exit}")"
  local run_valid invalid_reason warnings_csv
  IFS=$'\x1f' read -r run_valid invalid_reason warnings_csv <<<"${validity}"
  if [[ "${run_valid}" == "true" ]]; then
    invalid_reason=""
  fi
  local harness_visible
  harness_visible="$(harness_context_visible_for_run "${events_path}")"
  local tool_call_count shell_command_count file_read_count
  local discover_command_count edit_command_count verify_command_count
  {
    read -r tool_call_count
    read -r shell_command_count
    read -r file_read_count
    read -r discover_command_count
    read -r edit_command_count
    read -r verify_command_count
  } < <(count_activity "${events_path}")
  # RI-surfaced file lists from the rollout (empty for vanilla arms or
  # pre-split fragments). Threaded via env vars to keep the positional
  # surface bounded.
  local ri_surfaced
  ri_surfaced="$(extract_ri_surfaced_files "${events_path}")"
  local ri_edit_targets ri_orientation
  IFS=$'\x1f' read -r ri_edit_targets ri_orientation <<<"${ri_surfaced}"
  RECORD_RI_SURFACED_EDIT_TARGETS="${ri_edit_targets}" \
  RECORD_RI_SURFACED_ORIENTATION="${ri_orientation}" \
  RECORD_INTENT_CHANGED_FILES="$(extract_intent_changed_files "${events_path}")" \
  python3 - "${record_path}" "${run_valid}" "${invalid_reason}" "${warnings_csv}" "${harness_visible}" \
    "${tool_call_count}" "${shell_command_count}" "${file_read_count}" \
    "${discover_command_count}" "${edit_command_count}" "${verify_command_count}" <<'PY'
import json, os, sys
(
    path, run_valid, invalid_reason, warnings_csv, harness_visible,
    tool_call, shell_cmd, file_read, discover, edit, verify,
) = sys.argv[1:12]
with open(path, encoding="utf-8") as f:
    rec = json.load(f)
rec["run_valid"] = run_valid == "true"
rec["invalid_reason"] = invalid_reason or None
rec["warnings"] = [w for w in warnings_csv.split(",") if w]
rec["harness_context_visible"] = harness_visible == "true"
def _i(v):
    return int(v) if v not in ("", "null") else None
rec["tool_call_count"] = _i(tool_call)
rec["shell_command_count"] = _i(shell_cmd)
rec["file_read_count"] = _i(file_read)
rec["discover_command_count"] = _i(discover)
rec["edit_command_count"] = _i(edit)
rec["verify_command_count"] = _i(verify)
rec["ri_surfaced_edit_targets"] = [
    p for p in os.environ.get("RECORD_RI_SURFACED_EDIT_TARGETS", "").split("\n") if p
]
rec["ri_surfaced_orientation"] = [
    p for p in os.environ.get("RECORD_RI_SURFACED_ORIENTATION", "").split("\n") if p
]
# Intent / diff / formatter triplet. Mirrors write_record's assembly.
intent_paths = [
    p for p in os.environ.get("RECORD_INTENT_CHANGED_FILES", "").split("\n") if p
]
diff_paths = list(rec.get("changed_files", []))
intent_set = set(intent_paths)
def _norm(p: str) -> str:
    p = p.replace("\\", "/").strip()
    if p.startswith("./"): p = p[2:]
    idx = p.find("/codex-rs/")
    if idx >= 0: return p[idx + len("/codex-rs/"):]
    while p.startswith("codex-rs/"): p = p[len("codex-rs/"):]
    return p
diff_norm = {_norm(p): p for p in diff_paths if p.strip()}
formatter_paths = [orig for norm, orig in sorted(diff_norm.items()) if norm not in intent_set]
rec["intent_changed_files"] = intent_paths
rec["diff_changed_files"] = diff_paths
rec["formatter_changed_files"] = formatter_paths
with open(path, "w", encoding="utf-8") as f:
    json.dump(rec, f, indent=2)
    f.write("\n")
PY
}

# Re-classify every record.json under DIR. Layout assumption matches the
# in-tree write_record path: DIR/<task_id>/<arm>/{events.jsonl,record.json}.
rescore_artifacts_tree() {
  local root="$1"
  if [[ ! -d "${root}" ]]; then
    echo "rescore: directory not found: ${root}" >&2
    return 2
  fi
  local count=0
  while IFS= read -r -d '' record; do
    rescore_record "$(dirname "${record}")"
    count=$((count + 1))
  done < <(find "${root}" -mindepth 3 -maxdepth 3 -name record.json -print0)
  log "rescored ${count} record(s) under ${root}"
}

if [[ -n "${RESCORE_ARTIFACTS_DIR}" ]]; then
  RESCORE_ARTIFACTS_DIR="$(cd "${RESCORE_ARTIFACTS_DIR}" && pwd)"
  rescore_artifacts_tree "${RESCORE_ARTIFACTS_DIR}"
  echo "rescored: ${RESCORE_ARTIFACTS_DIR}"
  exit 0
fi

resolve_codex_bin
resolve_isolation_base
if [[ -z "${ARTIFACTS_DIR}" ]]; then
  ARTIFACTS_DIR="$(mktemp -d)"
fi
# Resolve ARTIFACTS_DIR to an absolute path BEFORE any `cd` happens in run_arm.
# Otherwise a relative `--artifacts-dir ./foo` resolves inside each isolated
# worktree at write_record time and `git worktree remove --force` deletes the
# records along with the worktree.
mkdir -p "${ARTIFACTS_DIR}"
ARTIFACTS_DIR="$(cd "${ARTIFACTS_DIR}" && pwd)"
log "artifacts: ${ARTIFACTS_DIR}"
if [[ "${ISOLATED_WORKTREES}" -eq 1 ]]; then
  log "isolated worktrees enabled; base_ref=${BASE_REF} (${BASE_REF_SHA}) repo=${REPO_ROOT}"

  # Copy the codex binary to a stable safe location BEFORE any arm runs.
  # Run 2 died because an agent in some arm wiped target/ — taking the codex
  # binary with it. With a copy outside any path the agent normally
  # touches, even a destructive `cargo clean` in a worktree leaves the
  # runner intact.
  SAFE_CODEX_BIN_DIR="$(mktemp -d -t codex-ri-eval-bin-XXXXXX)"
  cp "${CODEX_BIN}" "${SAFE_CODEX_BIN_DIR}/codex"
  chmod +x "${SAFE_CODEX_BIN_DIR}/codex"
  # Read-only flags raise the bar against accidental deletion; an agent with
  # --dangerously-bypass-approvals-and-sandbox can still force-remove this
  # path, but everyday `rm` / `cargo clean` no longer suffices.
  chmod 0555 "${SAFE_CODEX_BIN_DIR}/codex" 2>/dev/null || true
  CODEX_BIN="${SAFE_CODEX_BIN_DIR}/codex"
  log "safe codex binary: ${CODEX_BIN}"

  # CARGO_TARGET_DIR must NOT point at the source tree's target/. If an
  # agent in any arm runs `cargo clean`, that command rm -rf's the dir —
  # taking the codex binary with it if we share the path. Default to a
  # disposable /tmp dir; --cargo-target-dir overrides.
  if [[ -z "${EVAL_CARGO_TARGET_DIR}" ]]; then
    EVAL_CARGO_TARGET_DIR="/tmp/codex-ri-eval-cargo-target"
  fi
  mkdir -p "${EVAL_CARGO_TARGET_DIR}"
  export CARGO_TARGET_DIR="${EVAL_CARGO_TARGET_DIR}"
  log "shared CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
  if [[ -n "${MAX_TARGET_DIR_GB:-}" ]]; then
    log "max-target-dir-gb guard: ${MAX_TARGET_DIR_GB} GB"
  fi
fi

if [[ "${RUN_AGENT}" -eq 1 ]]; then
  # Use ASCII Unit Separator (\x1f), not tab, so an empty verify_command
  # does not cause adjacent IFS whitespace to collapse and shift fields
  # (which makes workdir_kind silently default to "calculator" and turns
  # the next task's id into a stray `eval` argument).
  while IFS=$'\x1f' read -r id task verify workdir_kind || [[ -n "${id:-}" ]]; do
    [[ -z "${id}" ]] && break
    workdir_kind="${workdir_kind:-calculator}"
    # Pre-arm guards: bail loudly rather than emit 30 exit=127 records.
    if ! ensure_codex_bin_alive; then
      echo "stopping after $(( ${#ACTIVE_WORKTREES[@]} )) in-flight worktree(s)" >&2
      exit 1
    fi
    if ! check_target_dir_size; then
      exit 1
    fi
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
