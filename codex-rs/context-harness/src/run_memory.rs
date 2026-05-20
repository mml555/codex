use serde::Deserialize;
use serde::Serialize;

/// Per-run state from the current agent session (schema only in harness).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMemory {
    pub files_read: Vec<String>,
    pub files_edited: Vec<String>,
    pub commands_run: Vec<String>,
    pub failures: Vec<String>,
    pub open_questions: Vec<String>,
}
