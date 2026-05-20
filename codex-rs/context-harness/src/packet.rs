use serde::Deserialize;
use serde::Serialize;

use crate::decision_log::ContextDecisionLog;

pub const CONTEXT_PACKET_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStage {
    Preflight,
    PostInspection,
    PreEdit,
    PostEdit,
    PostFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemKind {
    TaskSummary,
    RepoRule,
    FileSummary,
    FileSnippet,
    SymbolDefinition,
    CallSite,
    TestFile,
    RecentDiff,
    CommandOutput,
    RunMemory,
}

/// How much of an included item appears in the model-visible prompt fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderLevel {
    #[default]
    Full,
    Compact,
    PathOnly,
    HiddenDebugOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemState {
    Candidate,
    Included,
    Pinned,
    Compressed,
    Dropped,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInfo {
    pub raw: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextItem {
    pub id: String,
    pub kind: ContextItemKind,
    pub state: ContextItemState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub relevance: f64,
    pub confidence: f64,
    pub reason: String,
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<String>,
    #[serde(default)]
    pub render_level: RenderLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedTest {
    pub path: String,
    pub command: String,
    pub reason: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBudgetInfo {
    pub limit: u32,
    pub used_estimate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPacket {
    pub version: u32,
    pub stage: ContextStage,
    pub task: TaskInfo,
    pub items: Vec<ContextItem>,
    pub decision_log: ContextDecisionLog,
    pub selected_tests: Vec<SelectedTest>,
    pub warnings: Vec<String>,
    pub token_budget: TokenBudgetInfo,
}

impl ContextPacket {
    pub fn included_paths(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter(|item| item.state == ContextItemState::Included)
            .filter_map(|item| item.path.as_deref())
            .collect()
    }
}
