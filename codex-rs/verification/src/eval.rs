use std::path::Path;

use codex_repo_index::RepoMap;

use crate::rules::PlanContext;
use crate::rules::build_verification_plan;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEvalFixture {
    pub changed_files: Vec<String>,
    pub expected_commands: Vec<String>,
    #[serde(default)]
    pub forbidden_substrings: Vec<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub repo_map: Option<RepoMap>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEvalResult {
    pub changed_files: Vec<String>,
    pub planned_commands: Vec<String>,
    pub accuracy: f64,
    pub matched_expected: Vec<String>,
    pub missed_expected: Vec<String>,
    pub forbidden_hits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEvalReport {
    pub fixture_count: usize,
    pub avg_accuracy: f64,
    pub fixtures: Vec<PlanEvalResult>,
}

pub fn load_plan_fixtures(path: &Path) -> anyhow::Result<Vec<PlanEvalFixture>> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn run_plan_eval(fixtures: &[PlanEvalFixture], map: &RepoMap) -> PlanEvalReport {
    let mut results = Vec::new();
    let mut accuracy_sum = 0.0;

    for fixture in fixtures {
        let eval_map = fixture.repo_map.as_ref().unwrap_or(map);
        let ctx = PlanContext {
            task: fixture.task.clone(),
            changed_paths: fixture.changed_files.clone(),
            packet: None,
        };
        let plan = build_verification_plan(eval_map, &ctx);
        let result = score_plan_fixture(fixture, &plan);
        accuracy_sum += result.accuracy;
        results.push(result);
    }

    let count = fixtures.len().max(1);
    PlanEvalReport {
        fixture_count: fixtures.len(),
        avg_accuracy: accuracy_sum / count as f64,
        fixtures: results,
    }
}

pub fn score_plan_fixture(
    fixture: &PlanEvalFixture,
    plan: &crate::planner::VerificationPlan,
) -> PlanEvalResult {
    let planned_commands: Vec<String> = plan.commands.iter().map(|c| c.command.clone()).collect();
    let mut matched_expected = Vec::new();
    let mut missed_expected = Vec::new();

    for expected in &fixture.expected_commands {
        if planned_commands
            .iter()
            .any(|planned| planned == expected || planned.contains(expected))
        {
            matched_expected.push(expected.clone());
        } else {
            missed_expected.push(expected.clone());
        }
    }

    let accuracy = if fixture.expected_commands.is_empty() {
        1.0
    } else {
        matched_expected.len() as f64 / fixture.expected_commands.len() as f64
    };

    let mut forbidden_hits = Vec::new();
    for forbidden in &fixture.forbidden_substrings {
        if planned_commands
            .iter()
            .any(|planned| planned.contains(forbidden))
        {
            forbidden_hits.push(forbidden.clone());
        }
    }

    PlanEvalResult {
        changed_files: fixture.changed_files.clone(),
        planned_commands,
        accuracy,
        matched_expected,
        missed_expected,
        forbidden_hits,
    }
}

pub fn render_plan_eval_summary(report: &PlanEvalReport) -> String {
    format!(
        "fixture_count: {}\navg_accuracy: {:.2}",
        report.fixture_count, report.avg_accuracy,
    )
}
