use codex_repo_index::RepoMap;

use crate::ownership::ResolvedOwnership;
use crate::ownership::file_in_area_scope;
use crate::task_terms::TaskTerms;
use crate::task_terms::path_matches_term;

/// How broadly a task may pull files outside the primary owning area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskScope {
    /// Only the primary crate area.
    SingleArea,
    /// Primary area plus CLI/command entrypoints.
    AreaPlusBridge,
    /// Multiple feature areas (extension, app-server, core, etc.).
    CrossArea,
    /// Primary area plus CLI bridges and `core/` prompt/session integration files.
    CoreIntegration,
}

const CLI_BRIDGE_PATH: &str = "cli/src/context_cmd.rs";

const CORE_INTEGRATION_PATHS: &[(&str, &[&str])] = &[
    (
        "core/src/prompt_debug.rs",
        &["diff", "prompt", "debug", "self"],
    ),
    (
        "core/src/session/mod.rs",
        &["session", "assembly", "prompt", "extension", "intelligence"],
    ),
];

const CROSS_AREA_ROOTS: &[(&str, &[&str])] = &[
    ("ext/repo-intelligence/", &["intelligence", "extension"]),
    ("app-server/", &["server", "extension", "assembly"]),
    ("core/", &["session", "assembly", "prompt"]),
];

pub fn infer_task_scope(task: &str, terms: &TaskTerms, ownership: &ResolvedOwnership) -> TaskScope {
    let lower = task.to_ascii_lowercase();

    if terms
        .phrases
        .iter()
        .any(|p| p == "intelligence" || p == "extension")
        && (lower.contains("session") || lower.contains("assembly") || lower.contains("wire"))
    {
        return TaskScope::CrossArea;
    }

    if lower.contains("diff-prompt")
        || lower.contains("diff prompt")
        || (lower.contains("prompt") && lower.contains("self-contained"))
        || (terms.phrases.iter().any(|p| p == "prompt") && lower.contains("debug"))
    {
        return TaskScope::CoreIntegration;
    }

    if ownership.matched_command.is_some()
        || lower.contains("context")
            && terms
                .phrases
                .iter()
                .any(|p| matches!(p.as_str(), "command" | "eval" | "diff"))
    {
        return TaskScope::AreaPlusBridge;
    }

    TaskScope::SingleArea
}

pub fn extend_scoped_paths_for_scope(
    scope: TaskScope,
    task: &str,
    terms: &TaskTerms,
    ownership: &mut ResolvedOwnership,
    map: &RepoMap,
) {
    ownership.task_scope = scope;
    ownership.bridge_paths.clear();

    if let Some(entry) = map.commands.iter().find(|c| {
        ownership
            .matched_command
            .as_ref()
            .is_some_and(|cmd| cmd == &c.command)
    }) {
        ownership.scoped_paths.insert(entry.entrypoint.clone());
        ownership.bridge_paths.insert(entry.entrypoint.clone());
        for path in &entry.related_files {
            if is_bridge_path(path, ownership.primary_area.as_deref()) {
                ownership.bridge_paths.insert(path.clone());
            }
            ownership.scoped_paths.insert(path.clone());
        }
    }

    match scope {
        TaskScope::SingleArea => {}
        TaskScope::AreaPlusBridge => {
            ownership.scoped_paths.insert(CLI_BRIDGE_PATH.to_string());
            ownership.bridge_paths.insert(CLI_BRIDGE_PATH.to_string());
        }
        TaskScope::CoreIntegration => {
            ownership.scoped_paths.insert(CLI_BRIDGE_PATH.to_string());
            ownership.bridge_paths.insert(CLI_BRIDGE_PATH.to_string());
            for (path, required_terms) in CORE_INTEGRATION_PATHS {
                if task_mentions_any(task, terms, required_terms) {
                    ownership.scoped_paths.insert((*path).to_string());
                    ownership.bridge_paths.insert((*path).to_string());
                }
            }
        }
        TaskScope::CrossArea => {
            for (root, required_terms) in CROSS_AREA_ROOTS {
                if task_mentions_any(task, terms, required_terms) {
                    ownership.cross_area_roots.push((*root).to_string());
                }
            }
            for (path, required_terms) in CORE_INTEGRATION_PATHS {
                if task_mentions_any(task, terms, required_terms) {
                    ownership.scoped_paths.insert((*path).to_string());
                    ownership.bridge_paths.insert((*path).to_string());
                }
            }
            if let Some(area) = ownership
                .primary_area
                .as_ref()
                .and_then(|id| map.area_map_for_id(id))
            {
                for path in &area.related_cli_paths {
                    ownership.scoped_paths.insert(path.clone());
                    ownership.bridge_paths.insert(path.clone());
                }
            }
        }
    }
}

pub fn is_bridge_path(path: &str, primary_area: Option<&str>) -> bool {
    if path == CLI_BRIDGE_PATH {
        return true;
    }
    if path.starts_with("cli/") {
        return true;
    }
    if let Some(area) = primary_area {
        if path.starts_with(area) {
            return false;
        }
    }
    false
}

pub fn is_core_integration_path(path: &str, terms: &TaskTerms, task: &str) -> bool {
    CORE_INTEGRATION_PATHS
        .iter()
        .any(|(candidate, required)| *candidate == path && task_mentions_any(task, terms, required))
}

pub fn file_in_selection_scope(
    path: &str,
    area: Option<&codex_repo_index::AreaMap>,
    ownership: &ResolvedOwnership,
    _map: &RepoMap,
    terms: &TaskTerms,
    task: &str,
) -> bool {
    if area.is_some_and(|a| file_in_area_scope(path, a, &ownership.scoped_paths)) {
        return true;
    }
    if ownership.scoped_paths.contains(path) {
        return true;
    }
    match ownership.task_scope {
        TaskScope::SingleArea => false,
        TaskScope::AreaPlusBridge => {
            ownership.bridge_paths.contains(path)
                || is_bridge_path(path, ownership.primary_area.as_deref())
        }
        TaskScope::CoreIntegration => {
            ownership.bridge_paths.contains(path)
                || is_bridge_path(path, ownership.primary_area.as_deref())
                || is_core_integration_path(path, terms, task)
        }
        TaskScope::CrossArea => {
            ownership.bridge_paths.contains(path)
                || ownership
                    .cross_area_roots
                    .iter()
                    .any(|root| path.starts_with(root))
                || is_core_integration_path(path, terms, task)
        }
    }
}

pub fn area_lock_strict(scope: TaskScope) -> bool {
    matches!(scope, TaskScope::SingleArea)
}

pub fn should_use_global_fallback(scope: TaskScope, area_candidate_count: usize) -> bool {
    match scope {
        TaskScope::CrossArea | TaskScope::CoreIntegration => area_candidate_count < 1,
        TaskScope::AreaPlusBridge | TaskScope::SingleArea => area_candidate_count < 2,
    }
}

fn task_mentions_any(task: &str, terms: &TaskTerms, required_terms: &[&str]) -> bool {
    let lower = task.to_ascii_lowercase();
    required_terms.iter().any(|term| {
        lower.contains(term)
            || terms.phrases.iter().any(|p| p == *term)
            || path_matches_term(task, term)
    })
}
