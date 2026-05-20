use crate::packet::ContextPacket;
use crate::packet::RenderLevel;

#[derive(Debug, Clone, Default)]
pub struct EvalLabels {
    pub relevant_files: Vec<String>,
    pub relevant_tests: Vec<String>,
    pub bridge_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMetrics {
    pub relevant_file_recall: f64,
    pub context_waste: f64,
    pub test_selection_accuracy: f64,
    pub token_estimate: u32,
    pub bridge_file_recall: Option<f64>,
    pub dropped_count: usize,
    pub budget_exhausted_count: usize,
    pub low_confidence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDiagnostics {
    pub missed_gold_files: Vec<String>,
    pub extra_included_files: Vec<String>,
    pub missed_gold_tests: Vec<String>,
    pub extra_selected_tests: Vec<String>,
    pub missed_bridge_files: Vec<String>,
    pub extra_bridge_rendered: Vec<String>,
}

pub struct Metrics;

impl Metrics {
    pub fn evaluate(packet: &ContextPacket, labels: &EvalLabels) -> ContextMetrics {
        let included_paths: Vec<String> = packet
            .included_paths()
            .into_iter()
            .map(str::to_string)
            .collect();

        let relevant_file_recall = recall(&included_paths, &labels.relevant_files);
        let context_waste = waste(&included_paths, &labels.relevant_files);

        let selected: Vec<String> = packet
            .selected_tests
            .iter()
            .map(|t| t.path.clone())
            .collect();
        let test_selection_accuracy = recall(&selected, &labels.relevant_tests);

        let bridge_file_recall = if labels.bridge_files.is_empty() {
            None
        } else {
            Some(recall(&included_paths, &labels.bridge_files))
        };

        ContextMetrics {
            relevant_file_recall,
            context_waste,
            test_selection_accuracy,
            token_estimate: packet.token_budget.used_estimate,
            bridge_file_recall,
            dropped_count: packet.decision_log.dropped.len(),
            budget_exhausted_count: packet.decision_log.budget_exhausted.len(),
            low_confidence_count: packet.decision_log.low_confidence.len(),
        }
    }

    pub fn diagnose(packet: &ContextPacket, labels: &EvalLabels) -> EvalDiagnostics {
        let included_paths: Vec<String> = packet
            .included_paths()
            .into_iter()
            .map(str::to_string)
            .collect();
        let selected: Vec<String> = packet
            .selected_tests
            .iter()
            .map(|t| t.path.clone())
            .collect();

        let bridge_rendered: Vec<String> = packet
            .items
            .iter()
            .filter(|item| {
                item.path
                    .as_ref()
                    .is_some_and(|path| labels.bridge_files.iter().any(|bridge| bridge == path))
                    && item.render_level != RenderLevel::HiddenDebugOnly
            })
            .filter_map(|item| item.path.clone())
            .collect();

        EvalDiagnostics {
            missed_gold_files: labels
                .relevant_files
                .iter()
                .filter(|path| !included_paths.iter().any(|s| s == *path))
                .cloned()
                .collect(),
            extra_included_files: included_paths
                .iter()
                .filter(|path| !labels.relevant_files.iter().any(|g| g == *path))
                .cloned()
                .collect(),
            missed_gold_tests: labels
                .relevant_tests
                .iter()
                .filter(|path| !selected.iter().any(|s| s == *path))
                .cloned()
                .collect(),
            extra_selected_tests: selected
                .iter()
                .filter(|path| !labels.relevant_tests.iter().any(|g| g == *path))
                .cloned()
                .collect(),
            missed_bridge_files: labels
                .bridge_files
                .iter()
                .filter(|path| !included_paths.iter().any(|s| s == *path))
                .cloned()
                .collect(),
            extra_bridge_rendered: bridge_rendered
                .iter()
                .filter(|path| !labels.bridge_files.iter().any(|g| g == *path))
                .cloned()
                .collect(),
        }
    }
}

fn recall(selected: &[String], gold: &[String]) -> f64 {
    if gold.is_empty() {
        return 1.0;
    }
    let hits = gold
        .iter()
        .filter(|path| selected.iter().any(|s| s == *path))
        .count();
    hits as f64 / gold.len() as f64
}

fn waste(selected: &[String], gold: &[String]) -> f64 {
    if selected.is_empty() {
        return 0.0;
    }
    let irrelevant = selected
        .iter()
        .filter(|path| !gold.iter().any(|g| g == *path))
        .count();
    irrelevant as f64 / selected.len() as f64
}
