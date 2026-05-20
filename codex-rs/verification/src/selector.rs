use codex_repo_index::RepoMap;

use crate::planner::VerificationPlanner;

/// Legacy command shape kept for older call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationCommand {
    pub command: String,
    pub reason: String,
    pub confidence: f64,
}

pub struct TestSelector;

impl TestSelector {
    pub fn select(changed_paths: &[String], map: &RepoMap) -> Vec<VerificationCommand> {
        let plan = VerificationPlanner::plan(changed_paths, map);
        plan.commands
            .into_iter()
            .map(|cmd| VerificationCommand {
                command: cmd.command,
                reason: cmd.reason,
                confidence: cmd.confidence,
            })
            .collect()
    }
}
