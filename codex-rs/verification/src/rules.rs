use std::collections::BTreeMap;
use std::collections::BTreeSet;

use codex_context_harness::ContextPacket;
use codex_context_harness::TaskScope;
use codex_context_harness::build_task_terms;
use codex_context_harness::ownership::resolve_ownership;
use codex_repo_index::RepoMap;

use crate::planner::PlanScope;
use crate::planner::PlannedCommand;
use crate::planner::SkippedCommand;
use crate::planner::VerificationPlan;
use crate::planner::VerificationRisk;

/// Cargo package names for codex-rs area roots (path → `cargo test -p` name).
const AREA_PACKAGE_ALIASES: &[(&str, &str)] = &[
    ("app-server", "codex-app-server"),
    ("cli", "codex-cli"),
    ("context-harness", "codex-context-harness"),
    ("core", "codex-core"),
    ("ext/repo-intelligence", "codex-repo-intelligence-extension"),
    ("repo-index", "codex-repo-index"),
    ("tui", "codex-tui"),
    ("verification", "codex-verification"),
];

#[derive(Debug, Clone, Default)]
pub struct PlanContext {
    pub task: Option<String>,
    pub changed_paths: Vec<String>,
    pub packet: Option<ContextPacket>,
}

pub fn build_verification_plan(map: &RepoMap, ctx: &PlanContext) -> VerificationPlan {
    let changed: Vec<String> = ctx
        .changed_paths
        .iter()
        .map(|path| path.replace('\\', "/"))
        .filter(|path| !is_ignored_path(path))
        .collect();

    let python_partial = if crate::python_rules::is_python_repo(map, &changed) {
        Some(crate::python_rules::build_python_verification(
            map, &changed,
        ))
    } else {
        None
    };

    if let Some(python) = python_partial.as_ref()
        && crate::python_rules::changed_paths_are_python_only(&changed)
    {
        let risk = assess_risk(&python.commands, &changed);
        return VerificationPlan {
            commands: python.commands.clone(),
            skipped: python.skipped.clone(),
            risk,
        };
    }

    let ownership = ctx.task.as_ref().map(|task| {
        let terms = build_task_terms(task, map);
        resolve_ownership(task, map, &terms)
    });

    let mut packages: BTreeSet<String> = BTreeSet::new();
    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    let mut confidences: BTreeMap<String, f64> = BTreeMap::new();

    for path in &changed {
        if path.ends_with(".py") {
            continue;
        }
        if let Some(area_id) = area_id_for_path(path, map) {
            if let Some(package) = package_name_for_area(&area_id) {
                insert_package(
                    &mut packages,
                    &mut reasons,
                    &mut confidences,
                    &package,
                    format!("changed file `{path}` belongs to `{area_id}`"),
                    0.86,
                );
            }
        }

        if is_rust_test_path(path) {
            if let Some(area_id) = area_id_for_path(path, map) {
                if let Some(package) = package_name_for_area(&area_id) {
                    insert_package(
                        &mut packages,
                        &mut reasons,
                        &mut confidences,
                        &package,
                        format!("changed test file `{path}` in `{area_id}`"),
                        0.9,
                    );
                }
            }
        }
    }

    if changed
        .iter()
        .any(|path| path == "cli/src/context_cmd.rs" || path.starts_with("cli/"))
    {
        insert_package(
            &mut packages,
            &mut reasons,
            &mut confidences,
            "codex-cli",
            "changed CLI command wiring".to_string(),
            0.88,
        );
        if changed.iter().any(|path| path.contains("context")) || ctx.task.is_some() {
            insert_package(
                &mut packages,
                &mut reasons,
                &mut confidences,
                "codex-context-harness",
                "context CLI commands are implemented in context-harness".to_string(),
                0.84,
            );
        }
    }

    if changed
        .iter()
        .any(|path| path.contains("prompt_debug") || path.contains("core/src/session"))
    {
        insert_package(
            &mut packages,
            &mut reasons,
            &mut confidences,
            "codex-core",
            "changed core prompt/session integration file".to_string(),
            0.87,
        );
    }

    if changed
        .iter()
        .any(|path| path.starts_with("ext/repo-intelligence/"))
    {
        insert_package(
            &mut packages,
            &mut reasons,
            &mut confidences,
            "codex-repo-intelligence-extension",
            "changed repo-intelligence extension crate".to_string(),
            0.88,
        );
        if changed
            .iter()
            .any(|path| path.contains("extension") || path.contains("app-server"))
        {
            insert_package(
                &mut packages,
                &mut reasons,
                &mut confidences,
                "codex-app-server",
                "extension wiring touches app-server registration".to_string(),
                0.8,
            );
        }
    }

    if changed.iter().any(|path| path.starts_with("tui/")) {
        insert_package(
            &mut packages,
            &mut reasons,
            &mut confidences,
            "codex-tui",
            "changed TUI crate file".to_string(),
            0.82,
        );
    }

    if let (Some(packet), Some(ownership)) = (&ctx.packet, &ownership) {
        for test in &packet.selected_tests {
            if let Some(area_id) = ownership.primary_area.as_ref() {
                if test.path.starts_with(area_id) {
                    if let Some(package) = package_name_for_area(area_id) {
                        insert_package(
                            &mut packages,
                            &mut reasons,
                            &mut confidences,
                            &package,
                            format!(
                                "context packet selected likely test `{}` for primary area",
                                test.path
                            ),
                            test.confidence.min(0.85),
                        );
                    }
                }
            }
        }
    }

    if let Some(ownership) = &ownership {
        match ownership.task_scope {
            TaskScope::AreaPlusBridge | TaskScope::CoreIntegration => {
                if !changed.iter().any(|path| path.starts_with("cli/")) {
                    for bridge in &ownership.bridge_paths {
                        if bridge.starts_with("cli/") {
                            insert_package(
                                &mut packages,
                                &mut reasons,
                                &mut confidences,
                                "codex-cli",
                                format!(
                                    "task scope {:?} includes CLI bridge `{bridge}`",
                                    ownership.task_scope
                                ),
                                0.75,
                            );
                        }
                    }
                }
            }
            TaskScope::CrossArea => {
                for root in &ownership.cross_area_roots {
                    if let Some(area_id) = root.strip_suffix('/') {
                        if let Some(package) = package_name_for_area(area_id) {
                            if changed.iter().any(|path| path.starts_with(root)) {
                                insert_package(
                                    &mut packages,
                                    &mut reasons,
                                    &mut confidences,
                                    &package,
                                    format!("cross-area task touched `{root}`"),
                                    0.78,
                                );
                            }
                        }
                    }
                }
            }
            TaskScope::SingleArea => {}
        }
    }

    let mut commands: Vec<PlannedCommand> = packages
        .into_iter()
        .map(|package| {
            let reason = reasons
                .get(&package)
                .cloned()
                .unwrap_or_else(|| format!("narrow tests for package {package}"));
            let confidence = confidences.get(&package).copied().unwrap_or(0.8);
            PlannedCommand {
                command: format!("cargo test -p {package}"),
                reason,
                scope: PlanScope::Narrow,
                confidence,
            }
        })
        .collect();

    if let Some(python) = &python_partial {
        commands.extend(python.commands.clone());
    }

    commands.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.command.cmp(&b.command))
    });

    let mut skipped = build_skipped(&changed, &commands, map, ownership.as_ref());
    if let Some(python) = python_partial {
        skipped.extend(python.skipped);
        skipped.sort_by(|a, b| a.command.cmp(&b.command));
        skipped.dedup_by(|a, b| a.command == b.command);
    }
    let risk = assess_risk(&commands, &changed);

    VerificationPlan {
        commands,
        skipped,
        risk,
    }
}

fn insert_package(
    packages: &mut BTreeSet<String>,
    reasons: &mut BTreeMap<String, String>,
    confidences: &mut BTreeMap<String, f64>,
    package: &str,
    reason: String,
    confidence: f64,
) {
    packages.insert(package.to_string());
    reasons.entry(package.to_string()).or_insert(reason);
    confidences
        .entry(package.to_string())
        .and_modify(|existing| *existing = existing.max(confidence))
        .or_insert(confidence);
}

fn build_skipped(
    changed: &[String],
    commands: &[PlannedCommand],
    map: &RepoMap,
    ownership: Option<&codex_context_harness::ownership::ResolvedOwnership>,
) -> Vec<SkippedCommand> {
    let mut skipped = Vec::new();
    let planned_packages: BTreeSet<String> = commands
        .iter()
        .filter_map(|cmd| package_from_cargo_test(&cmd.command))
        .collect();

    skipped.push(SkippedCommand {
        command: "cargo test --workspace".to_string(),
        reason: "workspace-wide test run is broader than needed for localized changes".to_string(),
    });

    if !changed.iter().any(|path| path.starts_with("core/")) {
        skipped.push(SkippedCommand {
            command: "cargo test -p codex-core".to_string(),
            reason: "no core files changed; core may appear as path-only bridge in context"
                .to_string(),
        });
    }

    for (area_id, package) in AREA_PACKAGE_ALIASES {
        if planned_packages.contains(*package) {
            continue;
        }
        let touched = changed.iter().any(|path| path.starts_with(area_id));
        if !touched {
            if let Some(area) = map.area_map_for_id(area_id) {
                if ownership.is_some_and(|o| o.primary_area.as_deref() == Some(area_id)) {
                    continue;
                }
                if area.owned_files.len() > 20 {
                    skipped.push(SkippedCommand {
                        command: format!("cargo test -p {package}"),
                        reason: format!(
                            "area `{area_id}` not touched by changed files ({} owned files)",
                            area.owned_files.len()
                        ),
                    });
                }
            }
        }
    }

    skipped.sort_by(|a, b| a.command.cmp(&b.command));
    skipped.dedup_by(|a, b| a.command == b.command);
    skipped
}

fn assess_risk(commands: &[PlannedCommand], changed: &[String]) -> VerificationRisk {
    if commands.is_empty() {
        return VerificationRisk::High;
    }
    if commands.len() <= 2 && changed.len() <= 3 {
        return VerificationRisk::Low;
    }
    if commands.len() <= 4 {
        return VerificationRisk::Medium;
    }
    VerificationRisk::High
}

fn area_id_for_path(path: &str, map: &RepoMap) -> Option<String> {
    let mut best: Option<(String, usize)> = None;
    for area in &map.area_maps {
        let root = area
            .root_paths
            .first()
            .map(|r| r.trim_end_matches('/'))
            .unwrap_or(area.area_id.as_str());
        if path == root || path.starts_with(&format!("{root}/")) {
            let len = root.len();
            if best.as_ref().is_none_or(|(_, best_len)| len > *best_len) {
                best = Some((area.area_id.clone(), len));
            }
        }
    }
    if best.is_some() {
        return best.map(|(id, _)| id);
    }
    path.split('/').next().map(str::to_string)
}

fn package_name_for_area(area_id: &str) -> Option<String> {
    if let Some((_, package)) = AREA_PACKAGE_ALIASES
        .iter()
        .find(|(root, _)| *root == area_id)
    {
        return Some((*package).to_string());
    }
    if !area_id.contains('/') {
        return Some(format!("codex-{area_id}"));
    }
    None
}

fn package_from_cargo_test(command: &str) -> Option<String> {
    let rest = command.strip_prefix("cargo test -p ")?;
    Some(rest.split_whitespace().next()?.to_string())
}

fn is_ignored_path(path: &str) -> bool {
    path.contains("/target/")
        || path.contains("/node_modules/")
        || path.ends_with(".md")
        || (path.ends_with(".json") && !path.contains("/tests/"))
}

fn is_rust_test_path(path: &str) -> bool {
    path.contains("/tests/") || path.ends_with("_test.rs") || path.contains("/test/")
}
