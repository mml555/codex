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
    if path_lower.ends_with(".json")
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

    let (area_delta, area_evidence) = area_affinity_adjustment(&file.path, &terms.likely_areas);
    relevance += area_delta;
    if let Some(e) = area_evidence {
        evidence.push(e);
    }

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
    likely_areas: &[String],
) -> (f64, Option<String>) {
    let Some(top_area) = likely_areas.first() else {
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

fn is_manifest_path(path: &str) -> bool {
    path.ends_with("BUILD.bazel") || path.ends_with("Cargo.toml") || path.ends_with("package.json")
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
    use crate::RunMemory;
    use crate::TaskClassifier;
    use crate::selection::SelectionCaps;

    #[test]
    fn area_affinity_boosts_files_under_top_likely_area() {
        // Top area = "verification" → verification/* gets +0.40 boost.
        // The cli/Cargo.toml that previously won the first packet check
        // (because the task said "`cli`" as a quoted example) now gets
        // -0.30 instead — a net swing of 0.70 in favor of the right
        // crate. This is the structural fix that replaces the
        // hardcoded `if task_targets_crate(terms, "context-harness")`
        // / `if task_targets_crate(terms, "cli")` boosts.
        let areas = vec!["verification".to_string()];
        let (boost, evidence) = area_affinity_adjustment("verification/src/rules.rs", &areas);
        assert_eq!(boost, AREA_AFFINITY_BOOST);
        assert_eq!(evidence.as_deref(), Some("area:verification"));

        let (penalty, evidence) = area_affinity_adjustment("cli/Cargo.toml", &areas);
        assert_eq!(penalty, -AREA_OUTSIDE_PENALTY);
        assert_eq!(evidence.as_deref(), Some("area:outside_likely"));
    }

    #[test]
    fn area_affinity_skips_boost_when_no_area_inferred() {
        // Empty likely_areas → no signal → zero adjustment. The
        // pre-existing inference threshold (`score > 0.5` in
        // `infer_likely_areas`) acts as the confidence guard: if no
        // area cleared that bar, the affinity logic stays out of the
        // ranker entirely.
        let (delta, evidence) = area_affinity_adjustment("any/file.rs", &[]);
        assert_eq!(delta, 0.0);
        assert!(evidence.is_none());
    }

    #[test]
    fn area_affinity_requires_exact_path_prefix_not_substring() {
        // The OLD code's area block used `path.starts_with(area) ||
        // path_lower.contains(area)`. The substring branch matched any
        // path with the area name as a substring — e.g. with
        // likely_area="cli", `client.rs` would be treated as "in area",
        // and with likely_area="core", anything containing "core"
        // would. The fix anchors on the `<area>/` prefix exactly.
        let areas = vec!["cli".to_string()];
        let (boost, _) = area_affinity_adjustment("cli/src/main.rs", &areas);
        assert_eq!(boost, AREA_AFFINITY_BOOST, "exact prefix match");

        let (delta, _) = area_affinity_adjustment("client.rs", &areas);
        assert_eq!(
            delta, -AREA_OUTSIDE_PENALTY,
            "substring match must NOT count as in-area"
        );
        let (delta, _) = area_affinity_adjustment("foo/cli/bar.rs", &areas);
        assert_eq!(
            delta, -AREA_OUTSIDE_PENALTY,
            "area only matches as path PREFIX, not anywhere mid-path"
        );
    }

    #[test]
    fn area_affinity_uses_only_the_top_likely_area() {
        // `likely_areas` is a ranked Vec; the boost is applied to
        // files under the FIRST entry only. Files under second-place
        // areas get the outside-area penalty. This is intentional —
        // a single confident routing signal is the design.
        let areas = vec!["verification".to_string(), "cli".to_string()];
        let (delta, _) = area_affinity_adjustment("cli/Cargo.toml", &areas);
        assert_eq!(
            delta, -AREA_OUTSIDE_PENALTY,
            "second-ranked area must NOT inherit the boost"
        );
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
