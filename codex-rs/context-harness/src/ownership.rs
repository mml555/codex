use std::collections::BTreeSet;

use codex_repo_index::AreaMap;
use codex_repo_index::RepoMap;
use codex_repo_index::match_command_from_task;

use crate::task_scope::TaskScope;
use crate::task_scope::extend_scoped_paths_for_scope;
use crate::task_scope::infer_task_scope;
use crate::task_terms::TaskTerms;
use crate::task_terms::count_term_matches;
use crate::task_terms::task_targets_crate;

/// Resolved task ownership used for area-first file selection.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedOwnership {
    pub primary_area: Option<String>,
    pub matched_command: Option<String>,
    pub scoped_paths: BTreeSet<String>,
    pub task_scope: TaskScope,
    pub bridge_paths: BTreeSet<String>,
    pub cross_area_roots: Vec<String>,
}

pub fn resolve_ownership(task: &str, map: &RepoMap, terms: &TaskTerms) -> ResolvedOwnership {
    let mut scoped_paths = BTreeSet::new();
    let command_entry = match_command_from_task(task, &map.commands);
    let matched_command = command_entry.map(|entry| {
        scoped_paths.insert(entry.entrypoint.clone());
        for path in &entry.related_files {
            scoped_paths.insert(path.clone());
        }
        entry.command.clone()
    });

    let primary_area = command_entry
        .map(|entry| entry.implementation_area.clone())
        .or_else(|| infer_area_from_maps(task, map, terms))
        .or_else(|| terms.likely_areas.first().cloned());

    if let Some(area_id) = &primary_area {
        if let Some(area) = map.area_map_for_id(area_id) {
            extend_area_scope(area, &mut scoped_paths);
        }
    }

    let mut ownership = ResolvedOwnership {
        primary_area,
        matched_command,
        scoped_paths,
        task_scope: TaskScope::SingleArea,
        bridge_paths: BTreeSet::new(),
        cross_area_roots: Vec::new(),
    };
    let scope = infer_task_scope(task, terms, &ownership);
    extend_scoped_paths_for_scope(scope, task, terms, &mut ownership, map);
    ownership
}

pub fn infer_area_from_maps(_task: &str, map: &RepoMap, terms: &TaskTerms) -> Option<String> {
    if map.area_maps.is_empty() {
        return None;
    }

    // All term and raw-text checks below consult the QUOTE-AWARE
    // signals: `terms.strong_phrases` (terms present outside any
    // backtick / double-quote span) and `terms.task_outside_quotes_lower`
    // (the raw task text with quoted regions stripped). A quoted
    // example like ``"`cli`"`` inside a task about the verification
    // crate must NOT score the CLI area.
    let strong = &terms.strong_phrases;
    let task_lower = &terms.task_outside_quotes_lower;

    if task_targets_crate(strong, "context-harness")
        && map.area_map_for_id("context-harness").is_some()
    {
        return Some("context-harness".to_string());
    }
    if strong
        .iter()
        .any(|p| p == "intelligence" || p == "extension")
        && map.area_map_for_id("ext/repo-intelligence").is_some()
    {
        return Some("ext/repo-intelligence".to_string());
    }

    let mut best: Option<(String, f64)> = None;

    for area in &map.area_maps {
        let mut score = area.confidence * 0.2;
        let short_id = area
            .area_id
            .rsplit('/')
            .next()
            .unwrap_or(area.area_id.as_str());
        if task_lower.contains(&area.area_id)
            || task_lower.contains(short_id)
            || task_lower.contains(&short_id.replace('-', " "))
        {
            score += 0.85;
        }
        let area_tokens: Vec<String> = area
            .area_id
            .split(&['/', '-'][..])
            .filter(|t| t.len() >= 3)
            .map(str::to_string)
            .collect();
        if task_targets_crate(strong, &area.area_id)
            || area_tokens
                .iter()
                .all(|token| strong.iter().any(|p| p == token))
        {
            score += 0.5;
        }
        // Synonym pass still uses the full `expanded` set (which
        // includes weak terms) because area_id substring matches are
        // already heavily gated by the +0.85 prose-mention check
        // above; the +0.15 increment is a small tiebreaker and
        // gating it on strong-only terms would over-correct.
        for term in &terms.expanded {
            if area.area_id.contains(term) || term.contains(&area.area_id) {
                score += 0.15;
            }
        }
        if score > best.as_ref().map(|(_, s)| *s).unwrap_or(0.45) {
            best = Some((area.area_id.clone(), score));
        }
    }

    best.map(|(id, _)| id)
}

fn extend_area_scope(area: &AreaMap, scoped_paths: &mut BTreeSet<String>) {
    for path in &area.owned_files {
        scoped_paths.insert(path.clone());
    }
    for path in &area.related_cli_paths {
        scoped_paths.insert(path.clone());
    }
}

pub fn path_in_negative_area(path: &str, area: &AreaMap) -> bool {
    area.negative_paths
        .iter()
        .any(|negative| path.starts_with(negative))
}

fn is_build_manifest(path: &str) -> bool {
    path.ends_with("Cargo.toml") || path.ends_with("BUILD.bazel") || path.ends_with("package.json")
}

pub fn file_in_area_scope(path: &str, area: &AreaMap, scoped_paths: &BTreeSet<String>) -> bool {
    if is_build_manifest(path) {
        return false;
    }
    if scoped_paths.contains(path) {
        return true;
    }
    area.root_paths.iter().any(|root| path.starts_with(root))
}

pub fn area_owned_relevance_boost(
    path: &str,
    area: &AreaMap,
    scoped_paths: &BTreeSet<String>,
    terms: &TaskTerms,
) -> f64 {
    let mut boost = 0.0;
    if scoped_paths.contains(path) {
        boost += 0.35;
    } else if area.root_paths.iter().any(|root| path.starts_with(root)) {
        boost += 0.25;
    }
    if area.owned_files.iter().any(|owned| owned == path) {
        boost += 0.2;
    }
    if count_term_matches(path, &terms.expanded) >= 1 {
        boost += 0.1;
    }
    boost
}

#[cfg(test)]
mod live_map_tests {
    use super::*;
    use crate::task_terms::build_task_terms;
    use codex_repo_index::RepoMapBuilder;

    #[test]
    #[ignore = "slow: indexes full codex-rs tree"]
    fn codex_rs_map_resolves_context_harness_area() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("codex-rs workspace root");
        let map = RepoMapBuilder::build(root).expect("build map");
        let manifest_paths: Vec<_> = map
            .files
            .iter()
            .filter(|f| f.path.ends_with("Cargo.toml"))
            .take(5)
            .map(|f| f.path.as_str())
            .collect();
        assert!(
            !map.area_maps.is_empty(),
            "area_maps empty; manifest samples={manifest_paths:?}; file_count={}",
            map.files.len()
        );
        let task = "add codex context-harness eval command with fixture metrics";
        let terms = build_task_terms(task, &map);
        let ownership = resolve_ownership(task, &map, &terms);
        assert_eq!(
            ownership.primary_area.as_deref(),
            Some("context-harness"),
            "likely_areas={:?}",
            terms.likely_areas
        );
    }
}
