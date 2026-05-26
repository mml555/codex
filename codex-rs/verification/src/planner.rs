use serde::Deserialize;
use serde::Serialize;

use crate::rules::PlanContext;
use crate::rules::build_verification_plan;

/// Scope of a planned verification command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanScope {
    Narrow,
    Broad,
}

/// A recommended verification command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlannedCommand {
    pub command: String,
    pub reason: String,
    pub scope: PlanScope,
    pub confidence: f64,
}

/// A verification command intentionally omitted from the plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkippedCommand {
    pub command: String,
    pub reason: String,
}

/// Overall risk estimate for the proposed verification scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerificationRisk {
    Low,
    Medium,
    High,
}

/// Deterministic verification plan (no execution).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationPlan {
    pub commands: Vec<PlannedCommand>,
    pub skipped: Vec<SkippedCommand>,
    pub risk: VerificationRisk,
}

/// Inputs used to build a [`VerificationPlan`].
#[derive(Debug, Clone, Default)]
pub struct PlanRequest {
    pub task: Option<String>,
    pub changed_paths: Vec<String>,
}

pub struct VerificationPlanner;

impl VerificationPlanner {
    pub fn plan(changed_paths: &[String], map: &codex_repo_index::RepoMap) -> VerificationPlan {
        Self::plan_with_request(
            map,
            &PlanRequest {
                changed_paths: changed_paths.to_vec(),
                task: None,
            },
        )
    }

    pub fn plan_with_request(
        map: &codex_repo_index::RepoMap,
        request: &PlanRequest,
    ) -> VerificationPlan {
        let ctx = PlanContext {
            task: request.task.clone(),
            changed_paths: request.changed_paths.clone(),
            packet: None,
        };
        build_verification_plan(map, &ctx)
    }

    pub fn plan_with_context(
        map: &codex_repo_index::RepoMap,
        request: &PlanRequest,
        packet: &codex_context_harness::ContextPacket,
    ) -> VerificationPlan {
        let ctx = PlanContext {
            task: request.task.clone(),
            changed_paths: request.changed_paths.clone(),
            packet: Some(packet.clone()),
        };
        build_verification_plan(map, &ctx)
    }

}
