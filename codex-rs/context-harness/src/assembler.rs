use codex_repo_index::AreaMap;
use codex_repo_index::RepoFileEntry;
use codex_repo_index::RepoMap;

use crate::classifier::ClassifiedTask;
use crate::classifier::TaskType;
use crate::decision_log::DecisionEntry;
use crate::ownership::ResolvedOwnership;
use crate::ownership::area_owned_relevance_boost;
use crate::ownership::file_in_area_scope;
use crate::ownership::path_in_negative_area;
use crate::ownership::resolve_ownership;
use crate::packet::ContextItem;
use crate::packet::ContextItemKind;
use crate::packet::ContextItemState;
use crate::packet::RenderLevel;
use crate::packet::SelectedTest;
use crate::run_memory::RunMemory;
use crate::selection::SelectionCaps;
use crate::task_scope::area_lock_strict;
use crate::task_scope::file_in_selection_scope;
use crate::task_scope::should_use_global_fallback;
use crate::task_terms::TaskTerms;
use crate::task_terms::build_task_terms;
use crate::task_terms::count_term_matches;
use crate::task_terms::is_penalty_path;
use crate::task_terms::path_matches_term;
use crate::task_terms::task_mentions_path_marker;
use crate::test_select::select_tests_for_task;

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub candidates: Vec<ContextItem>,
    pub selected_tests: Vec<SelectedTest>,
    pub warnings: Vec<String>,
    pub dropped: Vec<DecisionEntry>,
    pub low_confidence: Vec<DecisionEntry>,
    pub likely_area: Option<String>,
}

pub struct ContextAssembler;

impl ContextAssembler {
    pub fn assemble_preflight(
        task: &str,
        classified: &ClassifiedTask,
        map: &RepoMap,
        run_memory: &RunMemory,
        caps: SelectionCaps,
    ) -> AssembledContext {
        let _ = run_memory;
        let terms = build_task_terms(task, map);
        let ownership = resolve_ownership(task, map, &terms);
        let likely_area = ownership
            .primary_area
            .clone()
            .or_else(|| terms.likely_areas.first().cloned());
        let area_map = likely_area
            .as_ref()
            .and_then(|area_id| map.area_map_for_id(area_id));
        let area_lock_active = likely_area.is_some()
            && (area_lock_strict(ownership.task_scope)
                || ownership.matched_command.is_some()
                || area_map.is_some_and(|area| area.confidence >= 0.8));
        let mut passing = Vec::new();
        let mut dropped = Vec::new();
        let mut low_confidence = Vec::new();
        let mut warnings = Vec::new();
        let mut area_candidate_count = 0usize;

        if let Some(area) = &likely_area {
            warnings.push(format!("Likely area: {area}"));
        }
        if let Some(command) = &ownership.matched_command {
            warnings.push(format!("Matched CLI command: {command}"));
        }
        warnings.push(format!("Task scope: {:?}", ownership.task_scope));

        if map.agents_md.is_some() {
            passing.push(ContextItem {
                id: "rule:agents_md".to_string(),
                kind: ContextItemKind::RepoRule,
                state: ContextItemState::Pinned,
                path: None,
                relevance: 1.0,
                confidence: 0.9,
                reason: "Project AGENTS.md instructions".to_string(),
                evidence: vec!["source:agents_md".to_string()],
                presentation: Some("summary".to_string()),
                render_level: RenderLevel::Full,
            });
        }

        for file in &map.files {
            if file.signals.tags.iter().any(|tag| tag == "test") {
                continue;
            }

            let id = format!("file:{}", file.path);
            let in_selection_scope = area_map.is_some_and(|area| {
                file_in_selection_scope(&file.path, Some(area), &ownership, map, &terms, task)
            });
            if let Some(area) = area_map {
                if area_lock_active
                    && path_in_negative_area(&file.path, area)
                    && !in_selection_scope
                {
                    push_dropped(
                        &mut dropped,
                        DecisionEntry {
                            id: id.clone(),
                            path: Some(file.path.clone()),
                            reason: format!("Excluded by area negative_paths for {}", area.area_id),
                            evidence: vec![format!("area:{}", area.area_id)],
                            relevance: None,
                            confidence: Some(file.signals.confidence),
                        },
                    );
                    continue;
                }
                if area_lock_active
                    && !in_selection_scope
                    && !should_use_global_fallback(ownership.task_scope, area_candidate_count)
                {
                    push_dropped(
                        &mut dropped,
                        DecisionEntry {
                            id: id.clone(),
                            path: Some(file.path.clone()),
                            reason: "Outside resolved area scope (area-first selection)"
                                .to_string(),
                            evidence: vec![format!("area:{}", area.area_id)],
                            relevance: None,
                            confidence: Some(file.signals.confidence),
                        },
                    );
                    continue;
                }
            }

            let targets_harness = task_targets_crate(&terms, "context-harness");
            let targets_cli = terms.phrases.iter().any(|p| p == "command")
                && terms.phrases.iter().any(|p| p == "eval");
            let in_target_crate = file.path.starts_with("context-harness/") && targets_harness
                || file.path.starts_with("cli/") && targets_cli;
            let area_relaxed = in_selection_scope && area_map.is_some();
            if file.signals.confidence < caps.drop_confidence_below
                && !in_target_crate
                && !area_relaxed
            {
                push_dropped(
                    &mut dropped,
                    DecisionEntry {
                        id: id.clone(),
                        path: Some(file.path.clone()),
                        reason: "File signal confidence below threshold".to_string(),
                        evidence: file.signals.evidence.clone(),
                        relevance: None,
                        confidence: Some(file.signals.confidence),
                    },
                );
                continue;
            }

            if is_manifest_path(&file.path) && !task_mentions_manifest(&file.path, &terms) {
                push_dropped(
                    &mut dropped,
                    DecisionEntry {
                        id: id.clone(),
                        path: Some(file.path.clone()),
                        reason: "Build manifest omitted unless task targets it".to_string(),
                        evidence: file.signals.evidence.clone(),
                        relevance: None,
                        confidence: Some(file.signals.confidence),
                    },
                );
                continue;
            }

            let score = score_file_for_task(
                file,
                &terms,
                classified.task_type,
                map,
                area_map,
                &ownership,
            );
            let phrase_hits = count_term_matches(&file.path, &terms.phrases);
            let expanded_hits = count_term_matches(&file.path, &terms.expanded);
            let in_likely_area = terms.likely_areas.iter().any(|area| {
                file.path.starts_with(area) || file.path.to_ascii_lowercase().contains(area)
            });
            let mut has_strong_match =
                phrase_hits >= 2 || expanded_hits >= 2 || (in_likely_area && phrase_hits >= 1);
            if in_target_crate || in_selection_scope {
                has_strong_match |= phrase_hits >= 1 || expanded_hits >= 1;
            }
            if in_selection_scope {
                has_strong_match |= phrase_hits >= 1
                    || expanded_hits >= 1
                    || score.relevance >= caps.include_relevance_min;
            }
            let relevance_min = if in_target_crate || in_selection_scope {
                caps.include_relevance_min * 0.75
            } else {
                caps.include_relevance_min
            };
            if !has_strong_match || score.relevance < relevance_min {
                push_dropped(
                    &mut dropped,
                    DecisionEntry {
                        id,
                        path: Some(file.path.clone()),
                        reason: score.drop_reason,
                        evidence: score.evidence,
                        relevance: Some(score.relevance),
                        confidence: Some(score.confidence),
                    },
                );
                if score.confidence < caps.drop_confidence_below {
                    push_low_confidence(
                        &mut low_confidence,
                        &file.path,
                        score.confidence,
                        &file.signals.evidence,
                    );
                }
                continue;
            }

            let presentation = if score.relevance > 0.8 {
                "summary+snippets"
            } else {
                "summary"
            };

            if in_selection_scope {
                area_candidate_count += 1;
            }
            passing.push(ContextItem {
                id: format!("file:{}", file.path),
                kind: ContextItemKind::FileSummary,
                state: ContextItemState::Candidate,
                path: Some(file.path.clone()),
                relevance: score.relevance,
                confidence: score.confidence,
                reason: score.include_reason,
                evidence: score.evidence,
                presentation: Some(presentation.to_string()),
                render_level: RenderLevel::Full,
            });
        }

        passing.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.path
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.path.as_deref().unwrap_or(""))
                })
        });

        let mut pinned_rules = Vec::new();
        let mut file_candidates = Vec::new();
        for item in passing {
            if item.kind == ContextItemKind::RepoRule {
                pinned_rules.push(item);
            } else {
                file_candidates.push(item);
            }
        }

        let prioritized = prioritize_for_task(&terms, file_candidates);
        let mut candidates = pinned_rules;
        let mut file_slots = 0usize;
        let mut summary_slots = 0usize;
        for mut item in prioritized {
            if file_slots >= caps.max_included_files {
                item.state = ContextItemState::Dropped;
                push_dropped(
                    &mut dropped,
                    DecisionEntry {
                        id: item.id.clone(),
                        path: item.path.clone(),
                        reason: "Excluded by max_included_files cap".to_string(),
                        evidence: item.evidence.clone(),
                        relevance: Some(item.relevance),
                        confidence: Some(item.confidence),
                    },
                );
                continue;
            }
            if summary_slots >= caps.max_file_summaries {
                item.state = ContextItemState::Dropped;
                push_dropped(
                    &mut dropped,
                    DecisionEntry {
                        id: item.id.clone(),
                        path: item.path.clone(),
                        reason: "Excluded by max_file_summaries cap".to_string(),
                        evidence: item.evidence.clone(),
                        relevance: Some(item.relevance),
                        confidence: Some(item.confidence),
                    },
                );
                continue;
            }
            file_slots += 1;
            summary_slots += 1;
            candidates.push(item);
        }

        let candidate_sources: Vec<String> = candidates
            .iter()
            .filter_map(|item| item.path.clone())
            .collect();
        let selected_tests =
            select_tests_for_task(map, &terms, &ownership, &candidate_sources, caps);
        caps.truncate_dropped(&mut dropped);

        if classified.task_type == TaskType::BugFix && selected_tests.is_empty() {
            warnings.push("No likely tests identified for bug-fix task".to_string());
        }

        AssembledContext {
            candidates,
            selected_tests,
            warnings,
            dropped,
            low_confidence,
            likely_area,
        }
    }
}

fn push_dropped(dropped: &mut Vec<DecisionEntry>, entry: DecisionEntry) {
    dropped.push(entry);
}

fn push_low_confidence(
    low_confidence: &mut Vec<DecisionEntry>,
    path: &str,
    confidence: f64,
    evidence: &[String],
) {
    low_confidence.push(DecisionEntry {
        id: format!("file:{path}"),
        path: Some(path.to_string()),
        reason: "low confidence path heuristic".to_string(),
        evidence: evidence.to_vec(),
        relevance: None,
        confidence: Some(confidence),
    });
}

struct FileScore {
    relevance: f64,
    confidence: f64,
    include_reason: String,
    drop_reason: String,
    evidence: Vec<String>,
}

fn score_file_for_task(
    file: &RepoFileEntry,
    terms: &TaskTerms,
    task_type: TaskType,
    map: &RepoMap,
    area_map: Option<&AreaMap>,
    ownership: &ResolvedOwnership,
) -> FileScore {
    let path_lower = file.path.to_ascii_lowercase();
    let mut relevance = file.signals.confidence * 0.2;
    let mut evidence = file.signals.evidence.clone();
    let mut matched_terms = 0usize;

    if is_penalty_path(&file.path, terms) && !task_mentions_path_marker(&file.path, terms) {
        relevance = 0.05;
        evidence.push("path:penalty_segment".to_string());
    }
    // Manifest de-prioritization: BUILD.bazel, Cargo.toml, package.json
    // should almost never be edit targets unless the task explicitly
    // names manifest vocabulary. The route-directive-marker packet
    // check showed `context-harness/BUILD.bazel` winning the
    // within-crate tiebreak over `renderer.rs` for a task that had
    // nothing to do with the build system.
    //
    // The penalty is heavier than the generic `.toml/.json` penalty
    // (×0.20 vs ×0.35) because manifests are stronger false
    // positives — they pick up area/crate tokens from their paths
    // (`context-harness/BUILD.bazel` matches "context", "harness")
    // without being plausible edit targets. When the task DOES name
    // manifest terms ("Cargo.toml", "BUILD.bazel", "manifest",
    // "dependency", "bazel target"), the penalty stays out of the
    // way and an explicit manifest task can still route correctly.
    let manifest_unannounced =
        is_manifest_path(&file.path) && !task_explicitly_names_manifest(terms);
    if manifest_unannounced {
        relevance *= 0.20;
        evidence.push("path:manifest_unannounced".to_string());
    } else if path_lower.ends_with(".json")
        || path_lower.ends_with(".toml")
        || path_lower.contains("schema/json")
    {
        relevance *= 0.35;
        evidence.push("path:config_or_schema".to_string());
    }

    let term_hits = count_term_matches(&file.path, &terms.expanded);
    if term_hits >= 2 {
        relevance += 0.25;
        evidence.push(format!("task:multi_match:{term_hits}"));
    }

    for term in &terms.expanded {
        if !crate::task_terms::is_scoring_term(term) {
            continue;
        }
        if path_matches_term(&file.path, term) {
            matched_terms += 1;
            relevance += if terms.phrases.iter().any(|p| p == term) {
                0.22
            } else {
                0.12
            };
            evidence.push(format!("task:{term}"));
        }
    }

    // Use the OWNERSHIP-resolved primary area (the signal that
    // already drives the displayed "Likely area: <X>" warning), not
    // `terms.likely_areas.first()`. The two can disagree:
    // `infer_area_from_maps` (the ownership path) takes commands and
    // declared area_maps into account, while `terms.likely_areas` is
    // pure term-frequency over the repo. The pytest-target packet
    // check showed both verification files getting the
    // `area:outside_likely` penalty because `terms.likely_areas.first()`
    // returned something other than "verification" even when ownership
    // correctly resolved verification as the primary area.
    let (area_delta, area_evidence) =
        area_affinity_adjustment(&file.path, ownership.primary_area.as_deref());
    relevance += area_delta;
    if let Some(e) = area_evidence {
        evidence.push(e);
    }

    // Within-crate ownership boost is applied AFTER the [0,1] clamp
    // further down — applying it here would be wasted, since multiple
    // in-area files already saturate at 1.0 from term matches + area
    // boost + signals. See the `// Within-crate ownership boost
    // (post-clamp)` block immediately after the clamp.

    if file.signals.tags.iter().any(|t| t == "route") {
        relevance += 0.1;
        evidence.push("tag:route".to_string());
    }
    if let Some(churn) = file.signals.git_churn_30d {
        relevance += (churn.min(10) as f64) * 0.015;
        evidence.push(format!("git_churn:{churn}"));
    }

    for test in &map.tests {
        if test
            .related_paths
            .iter()
            .any(|related| related == &file.path)
        {
            relevance += 0.15;
            evidence.push(format!("test_pairing:{}", test.path));
        }
    }
    for entry in &map.test_map {
        if entry.source_path == file.path {
            relevance += entry.confidence * 0.2;
            evidence.push(format!("test_map:{}", entry.test_paths.len()));
        }
    }
    if ownership.bridge_paths.contains(&file.path) {
        relevance += 0.42;
        evidence.push("scope:bridge".to_string());
    }
    if let Some(area) = area_map {
        if file_in_area_scope(&file.path, area, &ownership.scoped_paths) {
            relevance +=
                area_owned_relevance_boost(&file.path, area, &ownership.scoped_paths, terms);
            evidence.push(format!("area_owned:{}", area.area_id));
        } else if path_in_negative_area(&file.path, area) {
            relevance = relevance.min(0.08);
            evidence.push(format!("area_negative:{}", area.area_id));
        }
    }

    if task_type == TaskType::BugFix && path_lower.contains("/tests/") {
        relevance += 0.05;
    }
    if task_type == TaskType::Review && path_lower.contains("/protocol/") {
        relevance += 0.05;
    }

    if path_lower.contains("legacy") && !terms.expanded.iter().any(|t| t == "legacy") {
        relevance = relevance.min(0.12);
        evidence.push("path:legacy_penalty".to_string());
    }

    if task_targets_crate(terms, "context-harness") && file.path.starts_with("context-harness/") {
        let hits = count_term_matches(&file.path, &terms.expanded);
        if hits >= 1 {
            relevance = relevance.max(0.55);
            evidence.push(format!("crate:context-harness-boost:{hits}"));
        }
    }
    if terms.phrases.iter().any(|p| p == "eval") && path_lower.ends_with("eval.rs") {
        relevance += 0.2;
        evidence.push("task:eval_file".to_string());
    }
    if terms.phrases.iter().any(|p| p == "metrics") && path_lower.ends_with("metrics.rs") {
        relevance += 0.2;
        evidence.push("task:metrics_file".to_string());
    }
    relevance = relevance.clamp(0.0, 1.0);

    // Within-crate ownership boost (post-clamp): when the inferred
    // area only narrows routing to a crate (e.g. `verification/`),
    // the ranker still has to pick a specific file. The pytest-target
    // packet check exposed this: every verification file saturated at
    // relevance=1.0 from area boost + term matches + import signals,
    // so the +0.30 ownership boost was lost to clamping, and
    // `command_exec.rs` won the alphabetical tiebreaker over
    // `python_rules.rs`. Applying the boost after the clamp lets
    // owners exceed 1.0 specifically when the task semantics name
    // them — the only files that ever exceed the [0,1] range are
    // confirmed within-crate owners, so downstream consumers (which
    // sort by relevance descending) see a clean ordering.
    //
    // Triggers and table see `WITHIN_CRATE_OWNERS`. Boost magnitude
    // (+0.30) chosen so an owner at clamped-base 1.0 lands at 1.30,
    // safely above any tied non-owner. Doesn't affect file inclusion
    // (the threshold check happens earlier) — purely a tiebreaker.
    if let Some(owner_evidence) = within_crate_owner_match(&file.path, terms) {
        relevance += WITHIN_CRATE_OWNER_BOOST;
        evidence.push(owner_evidence.to_string());
    }

    let confidence = file.signals.confidence.clamp(0.0, 1.0);

    let include_reason = if matched_terms > 0 {
        format!("Path matches {matched_terms} task term(s)")
    } else if evidence
        .iter()
        .any(|e| e.starts_with("area:") && !e.contains("outside"))
    {
        "Likely repo area match".to_string()
    } else {
        "Related repo signal".to_string()
    };

    let drop_reason = if matched_terms == 0 {
        "No task term match and below relevance threshold".to_string()
    } else {
        "Below relevance threshold".to_string()
    };

    FileScore {
        relevance,
        confidence,
        include_reason,
        drop_reason,
        evidence,
    }
}

fn task_targets_crate(terms: &TaskTerms, crate_name: &str) -> bool {
    crate::task_terms::task_targets_crate(&terms.phrases, crate_name)
}

/// Crate-affinity adjustment to `score_file_for_task`'s relevance
/// score. Returns `(delta, evidence_label)` so the caller can append
/// the evidence string uniformly.
///
/// Previously three hardcoded special cases boosted `context-harness`
/// and `cli` files when the task mentioned their crate names. Every
/// other crate (verification, features, repo-index, ext/*, app-server,
/// core) got no crate-level boost at all — so a literal example string
/// like ``"`cli`"`` in a task about the verification crate could leak
/// CLI files past actual verification files.
///
/// The fix: use the inferred top `likely_area` instead. The inference
/// already runs against the full RepoMap and produces a ranked list
/// (top-3 entries above the `> 0.5` confidence threshold in
/// `infer_likely_areas`). A non-empty list is itself the confidence
/// guard — if no area cleared the threshold, no boost is applied.
///
/// The +0.40 boost is asymmetric with the -0.30 outside-area penalty:
/// inflated term-match scores from example-string mentions of OTHER
/// crates can stack to ~0.24 (2 hits × +0.12), so the boost must be
/// larger than the symmetric penalty to overcome them.
pub(crate) const AREA_AFFINITY_BOOST: f64 = 0.40;
pub(crate) const AREA_OUTSIDE_PENALTY: f64 = 0.30;

pub(crate) fn area_affinity_adjustment(
    path: &str,
    primary_area: Option<&str>,
) -> (f64, Option<String>) {
    let Some(top_area) = primary_area else {
        return (0.0, None);
    };
    let prefix = format!("{top_area}/");
    if path.starts_with(&prefix) {
        (AREA_AFFINITY_BOOST, Some(format!("area:{top_area}")))
    } else {
        (
            -AREA_OUTSIDE_PENALTY,
            Some("area:outside_likely".to_string()),
        )
    }
}

/// Boost magnitude applied to a file that owns the task's
/// responsibility within its crate. Sits between the area boost
/// (+0.40) and a single term match (+0.12) so it decisively breaks
/// within-crate ties without overriding the area signal.
pub(crate) const WITHIN_CRATE_OWNER_BOOST: f64 = 0.30;

/// Static within-crate ownership table. Each row maps a specific
/// file path suffix to the set of task-term triggers that indicate
/// that file is the actual owner of the task's intent.
///
/// Scoped narrowly on purpose: each rule's triggers are the actual
/// term forms the tokenizer emits after singularization (`commands`
/// → `command`, etc.), so we don't need fuzzy matching.
///
/// The matcher checks `terms.phrases` and `terms.expanded`. Phrases
/// are the raw tokens from the task; `expanded` adds synonyms from
/// the `task_terms::SYNONYMS` table.
const WITHIN_CRATE_OWNERS: &[(&str, &[&str], &str)] = &[
    // ---- verification crate (added in commit 227e682f5) ----
    //
    // `python_rules.rs` owns the python/pytest validation rules.
    // The pytest-target packet check failed when this file lost a
    // tie to `command_exec.rs` despite the task being explicitly
    // about pytest.
    (
        "verification/src/python_rules.rs",
        &["python", "pytest"],
        "owner:python_rules",
    ),
    // `command_exec.rs` owns shell command parsing / execution
    // safety. Triggers cover both noun ("command") and verb
    // ("exec", "parse") forms a task description might use.
    (
        "verification/src/command_exec.rs",
        &["command", "exec", "execution", "shell", "parse", "parser", "safe", "safety"],
        "owner:command_exec",
    ),
    // `planner.rs` owns verification-plan assembly.
    (
        "verification/src/planner.rs",
        &["plan", "planner"],
        "owner:planner",
    ),
    // `rules.rs` owns the area-to-cargo-package map and other static
    // rules tables. Triggers cover both the literal "rules" mention
    // and the area-package-alias task's vocabulary ("cargo",
    // "package", "alias", "area").
    (
        "verification/src/rules.rs",
        &["rule", "rules", "cargo", "package", "alias", "area"],
        "owner:rules",
    ),
    //
    // ---- context-harness crate (this commit) ----
    //
    // `renderer.rs` owns the directive prompt rendering pipeline,
    // including the `HARNESS_MARKER` sentinel and the
    // edit-targets/orientation section headers. The route-directive-
    // marker packet check failed when this file lost the within-
    // crate tiebreak to `BUILD.bazel`.
    (
        "context-harness/src/renderer.rs",
        &[
            "renderer", "render", "directive", "marker", "sentinel", "fragment", "prompt",
        ],
        "owner:harness_renderer",
    ),
    // `agent_eval.rs` owns the AgentRunRecord/AgentRunScore schemas,
    // the validity classifier, the result classifier, and most
    // diagnostic fields the cloud-eval reports surface. Trigger
    // terms come from field names and field semantics, not
    // generic verbs.
    (
        "context-harness/src/agent_eval.rs",
        &[
            "scorer", "record", "warning", "warnings", "invalid", "duration", "validity",
            "classify",
        ],
        "owner:agent_eval",
    ),
    // `assembler.rs` owns context-packet assembly: file scoring,
    // area affinity, within-crate ownership (this file itself),
    // packet emission. "packet" is the most discriminative term.
    (
        "context-harness/src/assembler.rs",
        &["assembler", "packet", "orientation"],
        "owner:assembler",
    ),
    // `task_terms.rs` owns task tokenization and the quote-aware
    // strong-phrase machinery added in commit f56a8c344.
    (
        "context-harness/src/task_terms.rs",
        &[
            "tokenize", "synonym", "synonyms", "backtick", "phrase", "phrases",
        ],
        "owner:task_terms",
    ),
    // `selection.rs` owns SelectionCaps (max_inspect_files,
    // max_edit_targets, the include-relevance thresholds).
    (
        "context-harness/src/selection.rs",
        &["caps", "cap", "budget", "inspect", "limit"],
        "owner:selection",
    ),
];

/// If `path` is a known within-crate owner AND any of its trigger
/// terms appears LITERALLY in the task, return the evidence label.
/// The caller adds the +`WITHIN_CRATE_OWNER_BOOST` to the file's
/// relevance. Returns None for unknown paths or non-matching tasks.
///
/// Matches only against `terms.phrases` (the raw tokenized vocabulary
/// of the task) — NOT `terms.expanded`. The synonym table
/// (`task_terms::SYNONYMS`) expands "harness" to include "assembler"
/// and "packet", which would otherwise fire the `owner:assembler`
/// rule for any context-harness task, including
/// `route_directive_marker` where the actual owner is `renderer.rs`.
/// Synonym-based ownership matching is too loose for this routing
/// signal — direct phrase mentions only.
pub(crate) fn within_crate_owner_match(path: &str, terms: &TaskTerms) -> Option<&'static str> {
    let has = |term: &str| terms.phrases.iter().any(|p| p == term);
    for (suffix, triggers, evidence) in WITHIN_CRATE_OWNERS {
        if !path.ends_with(suffix) {
            continue;
        }
        if triggers.iter().copied().any(has) {
            return Some(*evidence);
        }
    }
    None
}

fn is_manifest_path(path: &str) -> bool {
    path.ends_with("BUILD.bazel") || path.ends_with("Cargo.toml") || path.ends_with("package.json")
}

/// True when the task's *outside-quotes* text explicitly names
/// manifest-related vocabulary — "Cargo.toml", "BUILD.bazel",
/// "manifest", "dependency"/"dependencies", "bazel", or "crate
/// package". Stricter than the existing `task_mentions_manifest`
/// helper (which fires on bare "cargo" or "bazel" tokens and over-
/// matches tasks like `area-package-alias` that say "Cargo package
/// names" purely in prose). Drives the manifest de-prioritization
/// in `score_file_for_task`.
///
/// Substring matching (not tokenization) so that `Cargo.toml` survives
/// the tokenizer's split on `.` — the tokenizer would otherwise
/// produce `cargo` and `toml` separately, neither of which is
/// discriminative on its own.
///
/// Reads `terms.task_outside_quotes_lower` so a quoted example like
/// "`Cargo.toml`" does NOT count as a manifest mention — matches the
/// area-inference contract added in commit f56a8c344.
pub(crate) fn task_explicitly_names_manifest(terms: &TaskTerms) -> bool {
    let text = &terms.task_outside_quotes_lower;
    text.contains("cargo.toml")
        || text.contains("build.bazel")
        || text.contains("package.json")
        || text.contains("manifest")
        || text.contains("dependency")
        || text.contains("dependencies")
        || text.contains("crate package")
        // `bazel` as a standalone token (not just inside a file path
        // like "context-harness/BUILD.bazel" appearing in some other
        // context). The quote-elided text never contains a "BUILD.bazel"
        // literal — that would mean the task said it in prose.
        || text.contains(" bazel ")
        || text.starts_with("bazel ")
        || text.ends_with(" bazel")
}

fn task_mentions_manifest(path: &str, terms: &TaskTerms) -> bool {
    let path_lower = path.to_ascii_lowercase();
    terms
        .expanded
        .iter()
        .any(|term| term == "bazel" || term == "cargo" || path_lower.contains(term))
}

fn prioritize_for_task(terms: &TaskTerms, items: Vec<ContextItem>) -> Vec<ContextItem> {
    let reserve_harness = task_targets_crate(terms, "context-harness");
    let reserve_cli =
        terms.phrases.iter().any(|p| p == "command") && terms.phrases.iter().any(|p| p == "eval");
    let mut high_signal = Vec::new();
    let mut preferred = Vec::new();
    let mut rest = Vec::new();
    for item in items {
        let Some(path) = item.path.as_deref() else {
            rest.push(item);
            continue;
        };
        if path.ends_with("eval.rs")
            || path.ends_with("metrics.rs")
            || path.ends_with("context_cmd.rs")
        {
            high_signal.push(item);
        } else if (reserve_harness && path.starts_with("context-harness/"))
            || (reserve_cli && path.starts_with("cli/"))
        {
            preferred.push(item);
        } else {
            rest.push(item);
        }
    }
    let mut ordered = high_signal;
    ordered.append(&mut preferred);
    ordered.append(&mut rest);
    ordered
}

#[cfg(test)]
mod tests {
    use codex_repo_index::RepoFileEntry;
    use codex_repo_index::RepoMap;
    use codex_repo_index::RepoSignals;

    use super::AREA_AFFINITY_BOOST;
    use super::AREA_OUTSIDE_PENALTY;
    use super::ContextAssembler;
    use super::area_affinity_adjustment;
    use super::task_explicitly_names_manifest;
    use super::within_crate_owner_match;
    use crate::RunMemory;
    use crate::TaskClassifier;
    use crate::selection::SelectionCaps;
    use crate::task_terms::build_task_terms;

    #[test]
    fn area_affinity_boosts_files_under_primary_area() {
        // Primary area = "verification" → verification/* gets +0.40
        // boost. The cli/Cargo.toml that previously won the first
        // packet check (because the task said "`cli`" as a quoted
        // example) now gets -0.30 instead — a net swing of 0.70 in
        // favor of the right crate.
        let (boost, evidence) =
            area_affinity_adjustment("verification/src/rules.rs", Some("verification"));
        assert_eq!(boost, AREA_AFFINITY_BOOST);
        assert_eq!(evidence.as_deref(), Some("area:verification"));

        let (penalty, evidence) = area_affinity_adjustment("cli/Cargo.toml", Some("verification"));
        assert_eq!(penalty, -AREA_OUTSIDE_PENALTY);
        assert_eq!(evidence.as_deref(), Some("area:outside_likely"));
    }

    #[test]
    fn area_affinity_skips_boost_when_no_area_resolved() {
        // None → no signal → zero adjustment. `ownership.primary_area`
        // is None when ownership resolution didn't find a command
        // match, area_map match, or term-driven area inference. Keep
        // the ranker neutral instead of penalizing.
        let (delta, evidence) = area_affinity_adjustment("any/file.rs", None);
        assert_eq!(delta, 0.0);
        assert!(evidence.is_none());
    }

    #[test]
    fn area_affinity_requires_exact_path_prefix_not_substring() {
        // The OLD code's area block used `path.starts_with(area) ||
        // path_lower.contains(area)`. The substring branch matched any
        // path with the area name as a substring — e.g. with
        // primary_area="cli", `client.rs` would be treated as "in area",
        // and with primary_area="core", anything containing "core"
        // would. The fix anchors on the `<area>/` prefix exactly.
        let (boost, _) = area_affinity_adjustment("cli/src/main.rs", Some("cli"));
        assert_eq!(boost, AREA_AFFINITY_BOOST, "exact prefix match");

        let (delta, _) = area_affinity_adjustment("client.rs", Some("cli"));
        assert_eq!(
            delta, -AREA_OUTSIDE_PENALTY,
            "substring match must NOT count as in-area"
        );
        let (delta, _) = area_affinity_adjustment("foo/cli/bar.rs", Some("cli"));
        assert_eq!(
            delta, -AREA_OUTSIDE_PENALTY,
            "area only matches as path PREFIX, not anywhere mid-path"
        );
    }

    #[test]
    fn area_affinity_handles_nested_area_paths() {
        // Some area_maps use nested ids like `ext/repo-intelligence`.
        // The `<area>/` prefix rule must still work — the entire
        // nested path is the prefix.
        let area = Some("ext/repo-intelligence");
        let (boost, _) = area_affinity_adjustment("ext/repo-intelligence/src/lib.rs", area);
        assert_eq!(boost, AREA_AFFINITY_BOOST);

        let (delta, _) = area_affinity_adjustment("ext/other/src/lib.rs", area);
        assert_eq!(delta, -AREA_OUTSIDE_PENALTY);
    }

    /// Build TaskTerms with an empty RepoMap so the ownership tests
    /// don't depend on the live codex-rs tree. `infer_likely_areas`
    /// will return an empty vec, which is fine — the ownership match
    /// reads from `terms.phrases` / `terms.expanded` only.
    fn task_terms_for(task: &str) -> crate::task_terms::TaskTerms {
        let map = RepoMap {
            version: 2,
            repo_id: "t".to_string(),
            root: "/t".to_string(),
            files: Vec::new(),
            tests: Vec::new(),
            areas: Vec::new(),
            packages: Vec::new(),
            area_maps: Vec::new(),
            commands: Vec::new(),
            test_map: Vec::new(),
            agents_md: None,
            warnings: Vec::new(),
        };
        build_task_terms(task, &map)
    }

    #[test]
    fn within_crate_owner_matches_python_rules_for_pytest_task() {
        // The packet check 2 failure: pytest-target-check picked
        // command_exec.rs over python_rules.rs. The owner table must
        // route any task carrying "python" or "pytest" to
        // python_rules.rs.
        let terms = task_terms_for(
            "In the verification crate, find the helper that returns true \
             when a path matches the narrow single-file pytest target shape",
        );
        assert_eq!(
            within_crate_owner_match("verification/src/python_rules.rs", &terms),
            Some("owner:python_rules"),
        );
        // The same task must NOT activate command_exec ownership —
        // its triggers (command, exec, parse, shell) are absent here.
        assert_eq!(
            within_crate_owner_match("verification/src/command_exec.rs", &terms),
            None,
        );
    }

    #[test]
    fn within_crate_owner_matches_command_exec_for_command_safety_task() {
        // The complementary direction: a task explicitly about
        // shell command parsing/execution must route to
        // command_exec.rs, NOT to python_rules.rs (even though both
        // are in `verification/`).
        let terms = task_terms_for(
            "Add a helper to the verification crate that parses shell commands \
             and rejects unsafe execution patterns",
        );
        assert_eq!(
            within_crate_owner_match("verification/src/command_exec.rs", &terms),
            Some("owner:command_exec"),
        );
        assert_eq!(
            within_crate_owner_match("verification/src/python_rules.rs", &terms),
            None,
        );
    }

    #[test]
    fn within_crate_owner_matches_planner_for_verification_plan_task() {
        // Planning tasks must route to planner.rs — and must NOT
        // collide with command_exec or python_rules even when the
        // task happens to mention verification or rules.
        let terms = task_terms_for(
            "Update the verification planner to produce a narrower plan when \
             only one file changed",
        );
        assert_eq!(
            within_crate_owner_match("verification/src/planner.rs", &terms),
            Some("owner:planner"),
        );
        assert_eq!(
            within_crate_owner_match("verification/src/command_exec.rs", &terms),
            None,
        );
        assert_eq!(
            within_crate_owner_match("verification/src/python_rules.rs", &terms),
            None,
        );
    }

    #[test]
    fn within_crate_owner_returns_none_for_files_outside_known_owners() {
        // A verification file that isn't in the owners table (or any
        // file in another crate without matching triggers) gets
        // None — the ranker falls back to area boost + term matching
        // only. Keeps the table's scope narrow and predictable.
        let terms = task_terms_for("any task about pytest verification");
        assert_eq!(
            within_crate_owner_match("verification/src/lib.rs", &terms),
            None,
            "lib.rs is not an owner — must not get the boost"
        );
        // context-harness/agent_eval.rs IS now an owner, so it
        // matches when terms (`scorer`, `record`, etc.) are present.
        // For the pytest task above, no agent_eval triggers fire,
        // so the match is still None.
        assert_eq!(
            within_crate_owner_match("context-harness/src/agent_eval.rs", &terms),
            None,
            "no agent_eval triggers in this pytest task"
        );
    }

    #[test]
    fn within_crate_owner_matches_renderer_for_directive_marker_task() {
        // The route-directive-marker packet check failed when
        // `context-harness/BUILD.bazel` won the within-crate tiebreak
        // over `renderer.rs`. The renderer entry in the owners table
        // must fire on this task's vocabulary ("directive",
        // "fragment", "sentinel", "prompt").
        let terms = task_terms_for(
            "The constant whose literal text starts every model-visible \
             repo-intelligence prompt fragment lives in exactly one file \
             in the context-harness crate. Add a Sentinel doc comment \
             above its definition.",
        );
        assert_eq!(
            within_crate_owner_match("context-harness/src/renderer.rs", &terms),
            Some("owner:harness_renderer"),
        );
        // Manifests in the same crate must NOT fire the renderer
        // ownership boost.
        assert_eq!(
            within_crate_owner_match("context-harness/BUILD.bazel", &terms),
            None,
        );
        assert_eq!(
            within_crate_owner_match("context-harness/Cargo.toml", &terms),
            None,
        );
        // Critical negative case: `assembler.rs` must NOT fire on
        // this task. The first attempt at this fix matched
        // `terms.expanded`, which includes "assembler" and "packet"
        // as synonyms of "harness" — that produced a false positive
        // on assembler.rs for any context-harness task. Phrase-only
        // matching prevents that.
        assert_eq!(
            within_crate_owner_match("context-harness/src/assembler.rs", &terms),
            None,
            "synonym-driven 'assembler' must NOT fire owner:assembler \
             when the task is actually about the renderer/directive"
        );
    }

    #[test]
    fn within_crate_owner_matches_task_terms_for_tokenizer_task() {
        // Tasks about the tokenizer / quote-aware machinery must
        // route to task_terms.rs, not to assembler.rs or renderer.rs.
        let terms = task_terms_for(
            "Adjust the tokenizer's quote handling so backticked example \
             phrases are downweighted during area inference",
        );
        assert_eq!(
            within_crate_owner_match("context-harness/src/task_terms.rs", &terms),
            Some("owner:task_terms"),
        );
        assert_eq!(
            within_crate_owner_match("context-harness/src/assembler.rs", &terms),
            None,
        );
        assert_eq!(
            within_crate_owner_match("context-harness/src/renderer.rs", &terms),
            None,
        );
    }

    #[test]
    fn within_crate_owner_matches_assembler_for_packet_task() {
        // The assembler entry should win for tasks describing context-
        // packet assembly. Should NOT collide with renderer or
        // agent_eval ownership for the same task.
        let terms = task_terms_for(
            "Update the assembler to emit a new evidence label when a \
             file enters the orientation section of the packet",
        );
        assert_eq!(
            within_crate_owner_match("context-harness/src/assembler.rs", &terms),
            Some("owner:assembler"),
        );
    }

    #[test]
    fn manifest_paths_unannounced_get_de_prioritization_evidence() {
        // Direct test of the manifest helper: a task that doesn't
        // name manifest vocabulary leaves manifest paths unannounced.
        let plain = task_terms_for(
            "The constant whose literal text starts every model-visible \
             prompt fragment lives in the context-harness crate.",
        );
        assert!(
            !task_explicitly_names_manifest(&plain),
            "directive-marker task must NOT mention manifests; got phrases={:?}",
            plain.phrases
        );

        // Control: a task that explicitly names BUILD.bazel DOES
        // count as a manifest task. The penalty must stay out of
        // the way so an explicit manifest edit can route correctly.
        let bazel_task = task_terms_for(
            "Update the context-harness BUILD.bazel target to add a new \
             rust_test rule for the regression suite.",
        );
        assert!(
            task_explicitly_names_manifest(&bazel_task),
            "explicit BUILD.bazel mention must count as manifest task; got: {:?}",
            bazel_task.task_outside_quotes_lower
        );

        // Control 2: "Cargo package names" prose (the area-package-alias
        // task) MUST NOT trigger the manifest path — bare "cargo" is too
        // common to count.
        let area_package = task_terms_for(
            "Inside the verification crate there is a static array that \
             maps area-id prefixes to Cargo package names. Add a new entry.",
        );
        assert!(
            !task_explicitly_names_manifest(&area_package),
            "bare 'Cargo package' prose must NOT count as manifest mention"
        );

        // Control 3: explicit "Cargo.toml" or "manifest" or
        // "dependency" each count.
        for variant in [
            "Bump the codex-cli Cargo.toml version to 0.2",
            "Add a new dev-dependency to the manifest",
            "Replace a dependency entry with a newer revision",
        ] {
            let t = task_terms_for(variant);
            assert!(
                task_explicitly_names_manifest(&t),
                "variant `{variant}` should trigger manifest task; outside_lower={:?}",
                t.task_outside_quotes_lower
            );
        }
    }

    #[test]
    fn legacy_paths_are_dropped_for_restaurant_task() {
        let map = RepoMap {
            version: 2,
            repo_id: "t".to_string(),
            root: "/t".to_string(),
            files: vec![
                RepoFileEntry {
                    path: "backend/routes/legacy_restaurants.py".to_string(),
                    signals: RepoSignals::new(0.4).with_tag("route"),
                },
                RepoFileEntry {
                    path: "backend/routes/restaurants.py".to_string(),
                    signals: RepoSignals::new(0.78).with_tag("route"),
                },
            ],
            tests: Vec::new(),
            areas: Vec::new(),
            packages: Vec::new(),
            area_maps: Vec::new(),
            commands: Vec::new(),
            test_map: Vec::new(),
            agents_md: None,
            warnings: Vec::new(),
        };
        let classified = TaskClassifier::classify("fix restaurant search pagination");
        let assembled = ContextAssembler::assemble_preflight(
            "fix restaurant search pagination",
            &classified,
            &map,
            &RunMemory::default(),
            SelectionCaps::default(),
        );
        assert!(!assembled.candidates.iter().any(|item| {
            item.path
                .as_deref()
                .is_some_and(|path| path.contains("legacy_restaurants"))
        }));
        assert!(assembled.dropped.iter().any(|entry| {
            entry
                .path
                .as_deref()
                .is_some_and(|path| path.contains("legacy_restaurants"))
        }));
    }
}
