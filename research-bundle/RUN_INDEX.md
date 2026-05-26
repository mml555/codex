# Run index

All four release-mode cloud pairs from the closed RI eval experiment.
Selector verdicts come from `EVAL_REPORT.md`. Numbers below come from
each arm's `record.json`, verified to match the report exactly.

| Run | Task                                | Commit      | Selector  | V tokens | RI tokens | Δ%      | V dur (s) | RI dur (s) | Δs    | Outcome      |
| --- | ----------------------------------- | ----------- | --------- | -------: | --------: | ------: | --------: | ---------: | ----: | ------------ |
| 5   | `convention_add_area_package_alias` | `350d30b9c` | ✓ gold     |  607,040 |   387,083 |   -36%  |    105.0 |     121.1 |  +16  | RI token win |
| 6   | `route_directive_marker` (pre-fix)  | `350d30b9c` | ✗ BUILD.bazel|  882,315 |   864,699 |    -2%  |    278.1 |     319.4 |  +41  | tie          |
| 7   | `route_directive_marker` (post-fix) | `9b1f5a1e2` | ✓ renderer.rs|  246,511 |   903,965 |  +267%  |     35.2 |     393.7 | +358  | RI loss      |
| 8   | `targeted_test_agent_eval_excluded` | `5a45147f5` | ✗ core/README.md|  675,937 | 3,408,162 |  +404%  |    261.7 |     499.8 | +239  | RI loss (worst) |

All four pairs: provider `azure`, model `gpt-5.3-codex`,
`codex_build_profile=release`, `repo_intelligence_enabled=true` on RI
arm, `tests_passed=false` flag in record is the harness self-check
(not the model's verification outcome), `intent_changed_files_count=1`
on both arms — both arms only edited the gold file.

| Run | Artifact dir (in repo)                                                            |
| --- | --------------------------------------------------------------------------------- |
| 5   | `codex-rs/ri-convention-v1-release/convention_add_area_package_alias/`            |
| 6   | `codex-rs/ri-directive-marker-v1/route_directive_marker/`                         |
| 7   | `codex-rs/ri-directive-marker-v2/route_directive_marker/`                         |
| 8   | `codex-rs/ri-agent-eval-excluded-v1/targeted_test_agent_eval_excluded/`           |

Mirrored into this bundle under `runs/run{5,6,7,8}-*/`.

## Per-arm thread IDs and rollout paths

| Run | Arm                | Thread ID                                  | Rollout file                                                            |
| --- | ------------------ | ------------------------------------------ | ----------------------------------------------------------------------- |
| 5   | vanilla            | `019e61a1-2798-7a10-b6e2-c416bfe60979`     | `~/.codex/sessions/2026/05/25/rollout-...20-13-31-019e61a1-...jsonl`    |
| 5   | repo_intelligence  | `019e61a2-cdb4-7b92-a7bf-c457d270e637`     | `~/.codex/sessions/2026/05/25/rollout-...20-15-20-019e61a2-...jsonl`    |
| 6   | vanilla            | `019e61cc-c62e-7db3-8208-07c2a0e9bbd3`     | `~/.codex/sessions/2026/05/25/rollout-...21-01-10-019e61cc-...jsonl`    |
| 6   | repo_intelligence  | `019e61d1-123e-7a72-b474-69ccd1220699`     | `~/.codex/sessions/2026/05/25/rollout-...21-05-52-019e61d1-...jsonl`    |
| 7   | vanilla            | `019e6204-b41c-7913-a9c2-943269b10c60`     | `~/.codex/sessions/2026/05/25/rollout-...22-02-16-019e6204-...jsonl`    |
| 7   | repo_intelligence  | `019e6205-4e42-7a93-a941-6699bedffd3f`     | `~/.codex/sessions/2026/05/25/rollout-...22-02-55-019e6205-...jsonl`    |
| 8   | vanilla            | `019e622d-681e-7b40-acae-fb3cd2a3efe8`     | `~/.codex/sessions/2026/05/25/rollout-...22-46-43-019e622d-...jsonl`    |
| 8   | repo_intelligence  | `019e6231-7507-7610-8645-2ac1eee5eba3`     | `~/.codex/sessions/2026/05/25/rollout-...22-51-08-019e6231-...jsonl`    |

The rollout files are copied into each `runs/<run>/<arm>/rollout_full.jsonl`
so the bundle is self-contained.

## Selector one-line outcomes

- **Run 5**: selector landed `verification/src/rules.rs` correctly.
  RI saw -36% tokens. The clearest win in the experiment.
- **Run 6**: selector landed `context-harness/BUILD.bazel` (wrong).
  Token deltas roughly cancel. Used as the regression case that
  motivated the within-crate ownership + manifest de-prio fix in
  commit `9b1f5a1e`.
- **Run 7**: selector landed `context-harness/src/renderer.rs`
  correctly *after* commit `9b1f5a1e`. RI lost on wall-clock because
  the RI-arm model entered a `just test` verification loop while the
  vanilla arm skipped verification — a model-strategy variance, not
  a selector failure.
- **Run 8**: selector landed `core/README.md` (wrong). Model ignored
  the bad hint and found the right file via `rg`, but the RI arm
  still spent 5× the tokens. This is the strongest evidence the
  report cites for "wrong hint is not free." The fix shape is the
  open follow-up: symbol-aware ownership routing.
