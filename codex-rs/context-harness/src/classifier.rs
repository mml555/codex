use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    BugFix,
    Feature,
    Refactor,
    Review,
    Unknown,
}

impl TaskType {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskType::BugFix => "bug_fix",
            TaskType::Feature => "feature",
            TaskType::Refactor => "refactor",
            TaskType::Review => "review",
            TaskType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedTask {
    pub task_type: TaskType,
    pub confidence: f64,
    pub evidence: Vec<String>,
}

pub struct TaskClassifier;

impl TaskClassifier {
    pub fn classify(task: &str) -> ClassifiedTask {
        let lower = task.to_ascii_lowercase();
        let mut evidence = Vec::new();

        let task_type = if contains_keyword(&lower, "review") || contains_keyword(&lower, "audit") {
            evidence.push("keyword:review".to_string());
            TaskType::Review
        } else if contains_keyword(&lower, "refactor") {
            evidence.push("keyword:refactor".to_string());
            TaskType::Refactor
        } else if contains_keyword(&lower, "fix")
            || contains_keyword(&lower, "bug")
            || contains_keyword(&lower, "broken")
            || contains_keyword(&lower, "failing")
            || contains_keyword(&lower, "regression")
        {
            evidence.push("keyword:bug_fix".to_string());
            TaskType::BugFix
        } else if contains_keyword(&lower, "add")
            || contains_keyword(&lower, "implement")
            || contains_keyword(&lower, "create")
            || contains_phrase(&lower, "new feature")
        {
            evidence.push("keyword:feature".to_string());
            TaskType::Feature
        } else {
            evidence.push("keyword:none".to_string());
            TaskType::Unknown
        };

        let confidence = match task_type {
            TaskType::Unknown => 0.45,
            _ => 0.71,
        };

        ClassifiedTask {
            task_type,
            confidence,
            evidence,
        }
    }
}

fn contains_keyword(text: &str, keyword: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|token| token == keyword)
}

fn contains_phrase(text: &str, phrase: &str) -> bool {
    text.contains(phrase)
}
