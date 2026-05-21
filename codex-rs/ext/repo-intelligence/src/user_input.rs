use codex_protocol::user_input::UserInput;

/// Concatenate user text inputs into a single harness task string.
pub fn task_text_from_user_input(input: &[UserInput]) -> Option<String> {
    let mut parts = Vec::new();
    for item in input {
        let UserInput::Text { text, .. } = item else {
            continue;
        };
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::task_text_from_user_input;
    use codex_protocol::user_input::UserInput;

    #[test]
    fn joins_non_empty_text_inputs() {
        let input = vec![
            UserInput::Text {
                text: "  fix calculator  ".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Text {
                text: "keep it minimal".to_string(),
                text_elements: Vec::new(),
            },
        ];
        assert_eq!(
            task_text_from_user_input(&input).as_deref(),
            Some("fix calculator\nkeep it minimal")
        );
    }

    #[test]
    fn ignores_non_text_inputs() {
        let input = vec![UserInput::Skill {
            name: "demo".to_string(),
            path: std::path::PathBuf::from("SKILL.md"),
        }];
        assert_eq!(task_text_from_user_input(&input), None);
    }
}
