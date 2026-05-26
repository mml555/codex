#!/usr/bin/env python3
"""Pull rollout context for each Run 5-8 arm and write three files per run/arm:

- rollout_full.jsonl       : raw rollout copied verbatim
- prompt_messages.md       : human-readable system/developer/user messages (= what the model saw)
- ri_directive.txt         : just the "Harness repo intelligence:" block (empty for vanilla)

We also write run_meta.json with the resolved provider/model and the rollout path,
because the per-arm record.json does not carry that.
"""
import json
import shutil
from pathlib import Path

NEEDLE = "Harness repo intelligence:"
BUNDLE = Path("/Users/mendell/codex/research-bundle")
RUNS = BUNDLE / "runs"

# Mapping: bundle run dir -> arm -> rollout path
ROLLOUTS = {
    "run5-area-package-alias": {
        "vanilla":           "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T20-13-31-019e61a1-2798-7a10-b6e2-c416bfe60979.jsonl",
        "repo_intelligence": "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T20-15-20-019e61a2-cdb4-7b92-a7bf-c457d270e637.jsonl",
    },
    "run6-directive-marker-prefix": {
        "vanilla":           "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T21-01-10-019e61cc-c62e-7db3-8208-07c2a0e9bbd3.jsonl",
        "repo_intelligence": "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T21-05-52-019e61d1-123e-7a72-b474-69ccd1220699.jsonl",
    },
    "run7-directive-marker-postfix": {
        "vanilla":           "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T22-02-16-019e6204-b41c-7913-a9c2-943269b10c60.jsonl",
        "repo_intelligence": "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T22-02-55-019e6205-4e42-7a93-a941-6699bedffd3f.jsonl",
    },
    "run8-agent-eval-excluded": {
        "vanilla":           "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T22-46-43-019e622d-681e-7b40-acae-fb3cd2a3efe8.jsonl",
        "repo_intelligence": "/Users/mendell/.codex/sessions/2026/05/25/rollout-2026-05-25T22-51-08-019e6231-7507-7610-8645-2ac1eee5eba3.jsonl",
    },
}


def flatten_content(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        out = []
        for part in content:
            if isinstance(part, str):
                out.append(part)
            elif isinstance(part, dict):
                t = part.get("text")
                if isinstance(t, str):
                    out.append(t)
        return "\n".join(out)
    return ""


def first_directive_text(payloads):
    """Return the first user/developer/system message whose text contains the RI marker."""
    for role, text in payloads:
        if role in {"user", "developer", "system"} and NEEDLE in text:
            return text
    return None


def trim_to_directive(text: str) -> str:
    """Cut the message at the end of the RI directive so we don't ship the full
    AGENTS.md tail. The directive ends right after the `Likely area: X` line."""
    if not text or NEEDLE not in text:
        return ""
    start = text.find(NEEDLE)
    body = text[start:]
    marker = "\nLikely area:"
    idx = body.find(marker)
    if idx < 0:
        return body.rstrip() + "\n"
    line_end = body.find("\n", idx + 1)
    if line_end < 0:
        return body.rstrip() + "\n"
    return body[: line_end].rstrip() + "\n"


def collect_payloads(rollout_path: Path):
    """Yield (role, text) for every prompt-role message_response item.

    Returns also a meta dict with thread_id, model, provider if discoverable
    in `session_meta` / `turn_meta` records."""
    meta = {"rollout_path": str(rollout_path)}
    out = []
    with rollout_path.open(encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            t = ev.get("type")
            if t in {"session_meta", "session.start", "session.started"}:
                payload = ev.get("payload") or {}
                for k in ("model_provider", "originator", "cli_version", "thread_source"):
                    if k in payload:
                        meta.setdefault(k, payload[k])
                meta.setdefault("thread_id", payload.get("id"))
                git = payload.get("git") or {}
                if git.get("commit_hash"):
                    meta.setdefault("git_commit", git["commit_hash"])
            elif t == "turn_context":
                payload = ev.get("payload") or {}
                for k in ("model", "reasoning_effort", "reasoning_summary"):
                    if k in payload:
                        meta.setdefault(k, payload[k])
            elif t == "thread.started":
                meta.setdefault("thread_id", ev.get("thread_id"))
            elif t == "response_item":
                payload = ev.get("payload") or {}
                if payload.get("type") == "message":
                    role = payload.get("role") or ""
                    text = flatten_content(payload.get("content"))
                    if text:
                        out.append((role, text))
    return out, meta


def render_prompt_md(payloads, header):
    lines = [f"# {header}", ""]
    for i, (role, text) in enumerate(payloads):
        lines.append(f"## [{i}] role={role}")
        lines.append("")
        lines.append("```")
        lines.append(text.rstrip())
        lines.append("```")
        lines.append("")
    return "\n".join(lines)


def main():
    for run, arms in ROLLOUTS.items():
        for arm, rollout_str in arms.items():
            src = Path(rollout_str)
            dst_dir = RUNS / run / arm
            dst_dir.mkdir(parents=True, exist_ok=True)

            # 1) Raw rollout copy.
            shutil.copyfile(src, dst_dir / "rollout_full.jsonl")

            # 2) Pretty prompt extract.
            payloads, meta = collect_payloads(src)
            (dst_dir / "prompt_messages.md").write_text(
                render_prompt_md(payloads, f"{run} / {arm}")
            )

            # 3) RI directive extract (if present). Two flavors:
            #    - ri_directive.txt: raw message that carried the directive
            #      (= directive + AGENTS.md tail, exactly what the model saw)
            #    - ri_directive_trimmed.txt: just the harness-authored block,
            #      cut at the trailing "Likely area:" line
            full = first_directive_text(payloads)
            (dst_dir / "ri_directive.txt").write_text(full or "")
            (dst_dir / "ri_directive_trimmed.txt").write_text(
                trim_to_directive(full or "")
            )

            # Mirror trimmed directive into the top-level ri-packets dir for
            # easy side-by-side comparison.
            if full:
                top = BUNDLE / "ri-packets" / f"{run}.txt"
                top.write_text(trim_to_directive(full))

            # 4) Per-arm meta.
            (dst_dir / "run_meta.json").write_text(
                json.dumps(meta, indent=2, sort_keys=True)
            )

            n_msgs = len(payloads)
            has_directive = bool(full)
            print(f"{run}/{arm}: msgs={n_msgs} ri_directive={has_directive} meta_keys={sorted(meta.keys())}")


if __name__ == "__main__":
    main()
