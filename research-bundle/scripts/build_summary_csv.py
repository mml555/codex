#!/usr/bin/env python3
"""Compile one CSV row per arm for Runs 5-8 from the bundled record.json
files (plus rollout meta extracted by extract_packets.py).

Output: research-bundle/summary/runs_cost_summary.csv
"""
import csv
import json
from pathlib import Path

BUNDLE = Path("/Users/mendell/codex/research-bundle")
RUNS = BUNDLE / "runs"
OUT_CSV = BUNDLE / "summary" / "runs_cost_summary.csv"

FIELDS = [
    "run",
    "task_id",
    "arm",
    "model",
    "model_provider",
    "git_commit",
    "codex_build_profile",
    "tokens_input",
    "tokens_output",
    "tokens_total",
    "duration_ms",
    "harness_prewarm_ms",
    "turn_count",
    "exec_exit_code",
    "tests_passed",
    "tool_call_count",
    "shell_command_count",
    "discover_command_count",
    "edit_command_count",
    "verify_command_count",
    "file_read_count",
    "run_valid",
    "invalid_reason",
    "warnings",
    "repo_intelligence_enabled",
    "harness_context_visible",
    "intent_changed_files_count",
    "diff_changed_files_count",
    "formatter_changed_files_count",
    "ri_surfaced_edit_targets",
    "ri_surfaced_orientation",
    "thread_id",
]


def first_or_none(d, key):
    v = d.get(key)
    if isinstance(v, list):
        return ";".join(v)
    return v


def main():
    rows = []
    for run_dir in sorted(RUNS.iterdir()):
        if not run_dir.is_dir():
            continue
        for arm in ("vanilla", "repo_intelligence"):
            arm_dir = run_dir / arm
            record_path = arm_dir / "record.json"
            meta_path = arm_dir / "run_meta.json"
            if not record_path.exists():
                continue
            rec = json.loads(record_path.read_text())
            meta = json.loads(meta_path.read_text()) if meta_path.exists() else {}

            rows.append(
                {
                    "run": run_dir.name,
                    "task_id": rec.get("task_id"),
                    "arm": arm,
                    "model": meta.get("model"),
                    "model_provider": meta.get("model_provider"),
                    "git_commit": meta.get("git_commit"),
                    "codex_build_profile": rec.get("codex_build_profile"),
                    "tokens_input": rec.get("tokens_input"),
                    "tokens_output": rec.get("tokens_output"),
                    "tokens_total": rec.get("tokens_total"),
                    "duration_ms": rec.get("duration_ms"),
                    "harness_prewarm_ms": rec.get("harness_prewarm_ms"),
                    "turn_count": rec.get("turn_count"),
                    "exec_exit_code": rec.get("exec_exit_code"),
                    "tests_passed": rec.get("tests_passed"),
                    "tool_call_count": rec.get("tool_call_count"),
                    "shell_command_count": rec.get("shell_command_count"),
                    "discover_command_count": rec.get("discover_command_count"),
                    "edit_command_count": rec.get("edit_command_count"),
                    "verify_command_count": rec.get("verify_command_count"),
                    "file_read_count": rec.get("file_read_count"),
                    "run_valid": rec.get("run_valid"),
                    "invalid_reason": rec.get("invalid_reason"),
                    "warnings": first_or_none(rec, "warnings"),
                    "repo_intelligence_enabled": rec.get("repo_intelligence_enabled"),
                    "harness_context_visible": rec.get("harness_context_visible"),
                    "intent_changed_files_count": len(rec.get("intent_changed_files") or []),
                    "diff_changed_files_count": len(rec.get("diff_changed_files") or []),
                    "formatter_changed_files_count": len(rec.get("formatter_changed_files") or []),
                    "ri_surfaced_edit_targets": first_or_none(rec, "ri_surfaced_edit_targets"),
                    "ri_surfaced_orientation": first_or_none(rec, "ri_surfaced_orientation"),
                    "thread_id": meta.get("thread_id"),
                }
            )

    OUT_CSV.parent.mkdir(parents=True, exist_ok=True)
    with OUT_CSV.open("w", newline="") as fp:
        w = csv.DictWriter(fp, fieldnames=FIELDS)
        w.writeheader()
        w.writerows(rows)

    print(f"wrote {len(rows)} rows -> {OUT_CSV}")
    # quick A/B delta summary
    print()
    print(f"{'run':<32}  {'tokens V/RI':<22}  {'dur V/RI':<18}  {'edits V/RI':<10}")
    by_run = {}
    for r in rows:
        by_run.setdefault(r["run"], {})[r["arm"]] = r
    for run, arms in by_run.items():
        v = arms.get("vanilla", {})
        ri = arms.get("repo_intelligence", {})
        tk = f"{v.get('tokens_total')}/{ri.get('tokens_total')}"
        du = f"{v.get('duration_ms')}/{ri.get('duration_ms')}"
        ed = f"{v.get('intent_changed_files_count')}/{ri.get('intent_changed_files_count')}"
        print(f"{run:<32}  {tk:<22}  {du:<18}  {ed:<10}")


if __name__ == "__main__":
    main()
