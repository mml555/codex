use std::path::Path;

use codex_repo_index::RepoMap;

use crate::build_context_packet;
use crate::metrics::ContextMetrics;
use crate::metrics::EvalDiagnostics;
use crate::metrics::EvalLabels;
use crate::metrics::Metrics;
use crate::pipeline::BuildPacketOptions;
use crate::run_memory::RunMemory;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvalTaskFixture {
    pub task: String,
    #[serde(alias = "gold_files")]
    pub relevant_files: Vec<String>,
    #[serde(alias = "gold_tests", default)]
    pub relevant_tests: Vec<String>,
    #[serde(default)]
    pub bridge_files: Vec<String>,
    #[serde(default)]
    pub danger_zones: Vec<String>,
    #[serde(default)]
    pub repo_map: Option<RepoMap>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvalTaskResult {
    pub task: String,
    pub metrics: ContextMetrics,
    pub diagnostics: EvalDiagnostics,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvalReport {
    pub task_count: usize,
    pub avg_recall: f64,
    pub avg_waste: f64,
    pub avg_test_accuracy: f64,
    pub avg_token_estimate: u32,
    pub avg_bridge_file_recall: Option<f64>,
    pub tasks: Vec<EvalTaskResult>,
}

pub fn load_eval_fixtures(path: &Path) -> anyhow::Result<Vec<EvalTaskFixture>> {
    let bytes = std::fs::read(path)?;
    let fixtures: Vec<EvalTaskFixture> = serde_json::from_slice(&bytes)?;
    Ok(fixtures)
}

pub fn run_eval(
    fixtures: &[EvalTaskFixture],
    map: &RepoMap,
    options: BuildPacketOptions,
) -> EvalReport {
    let mut tasks = Vec::new();
    let mut recall_sum = 0.0;
    let mut waste_sum = 0.0;
    let mut test_sum = 0.0;
    let mut token_sum = 0u32;
    let mut bridge_sum = 0.0;
    let mut bridge_count = 0usize;

    for fixture in fixtures {
        let eval_map = fixture.repo_map.as_ref().unwrap_or(map);
        let packet = build_context_packet(
            &fixture.task,
            eval_map,
            &RunMemory::default(),
            options.clone(),
        );
        let labels = EvalLabels {
            relevant_files: fixture.relevant_files.clone(),
            relevant_tests: fixture.relevant_tests.clone(),
            bridge_files: fixture.bridge_files.clone(),
        };
        let metrics = Metrics::evaluate(&packet, &labels);
        let diagnostics = Metrics::diagnose(&packet, &labels);
        recall_sum += metrics.relevant_file_recall;
        waste_sum += metrics.context_waste;
        test_sum += metrics.test_selection_accuracy;
        token_sum += metrics.token_estimate;
        if let Some(bridge) = metrics.bridge_file_recall {
            bridge_sum += bridge;
            bridge_count += 1;
        }
        tasks.push(EvalTaskResult {
            task: fixture.task.clone(),
            metrics,
            diagnostics,
        });
    }

    let count = fixtures.len().max(1);
    EvalReport {
        task_count: fixtures.len(),
        avg_recall: recall_sum / count as f64,
        avg_waste: waste_sum / count as f64,
        avg_test_accuracy: test_sum / count as f64,
        avg_token_estimate: token_sum / count as u32,
        avg_bridge_file_recall: if bridge_count == 0 {
            None
        } else {
            Some(bridge_sum / bridge_count as f64)
        },
        tasks,
    }
}

pub fn render_eval_summary(report: &EvalReport) -> String {
    let bridge_line = report
        .avg_bridge_file_recall
        .map(|bridge| format!("\navg_bridge_file_recall: {bridge:.2}"))
        .unwrap_or_default();
    format!(
        "task_count: {}\navg_recall: {:.2}\navg_waste: {:.2}\navg_test_accuracy: {:.2}\navg_token_estimate: {}{bridge_line}",
        report.task_count,
        report.avg_recall,
        report.avg_waste,
        report.avg_test_accuracy,
        report.avg_token_estimate,
    )
}

pub fn render_eval_human(report: &EvalReport) -> String {
    let mut lines = vec![render_eval_summary(report), String::new()];
    for task in &report.tasks {
        lines.push(format!("Task: {}", task.task));
        lines.push(format!(
            "  Recall: {:.2}",
            task.metrics.relevant_file_recall
        ));
        lines.push(format!("  Waste: {:.2}", task.metrics.context_waste));
        lines.push(format!(
            "  Tests: {:.2}",
            task.metrics.test_selection_accuracy
        ));
        lines.push(format!("  Tokens: {}", task.metrics.token_estimate));
        if let Some(bridge) = task.metrics.bridge_file_recall {
            lines.push(format!("  Bridge recall: {bridge:.2}"));
        }
        if !task.diagnostics.missed_gold_files.is_empty() {
            lines.push("  Missed gold files:".to_string());
            for path in &task.diagnostics.missed_gold_files {
                lines.push(format!("    - {path}"));
            }
        }
        if !task.diagnostics.missed_bridge_files.is_empty() {
            lines.push("  Missed bridge files:".to_string());
            for path in &task.diagnostics.missed_bridge_files {
                lines.push(format!("    - {path}"));
            }
        }
        if !task.diagnostics.extra_included_files.is_empty() {
            lines.push("  Extra included files:".to_string());
            for path in &task.diagnostics.extra_included_files {
                lines.push(format!("    - {path}"));
            }
        }
        if !task.diagnostics.missed_gold_tests.is_empty() {
            lines.push("  Missed gold tests:".to_string());
            for path in &task.diagnostics.missed_gold_tests {
                lines.push(format!("    - {path}"));
            }
        }
        if !task.diagnostics.extra_selected_tests.is_empty() {
            lines.push("  Extra selected tests:".to_string());
            for path in &task.diagnostics.extra_selected_tests {
                lines.push(format!("    - {path}"));
            }
        }
        lines.push(String::new());
    }
    lines.join("\n")
}
