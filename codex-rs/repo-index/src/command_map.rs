use std::path::Path;

use crate::repo_map::AreaMap;
use crate::repo_map::CommandMapEntry;

const CONTEXT_CMD_PATH: &str = "cli/src/context_cmd.rs";

/// Subcommand stem → primary implementation area and related source hints.
const CONTEXT_SUBCOMMANDS: &[(&str, &str, &[&str])] = &[
    (
        "build",
        "context-harness",
        &[
            "context-harness/src/assembler.rs",
            "context-harness/src/pipeline.rs",
            "context-harness/src/renderer.rs",
        ],
    ),
    (
        "diff-prompt",
        "context-harness",
        &[
            "context-harness/src/prompt_paths.rs",
            "core/src/prompt_debug.rs",
        ],
    ),
    (
        "eval",
        "context-harness",
        &[
            "context-harness/src/eval.rs",
            "context-harness/src/metrics.rs",
        ],
    ),
    (
        "agent-eval",
        "context-harness",
        &[
            "context-harness/src/agent_eval.rs",
            "cli/src/context_agent_eval_cmd.rs",
            "scripts/harness-agent-eval.sh",
        ],
    ),
    (
        "smoke",
        "context-harness",
        &["context-harness/src/prompt_visibility.rs"],
    ),
];

pub fn build_command_maps(root: &Path, area_maps: &[AreaMap]) -> Vec<CommandMapEntry> {
    let mut commands = Vec::new();
    let context_cmd = root.join(CONTEXT_CMD_PATH);
    if !context_cmd.is_file() {
        return commands;
    }

    for (subcommand, area_id, related_hints) in CONTEXT_SUBCOMMANDS {
        let mut related_files: Vec<String> = related_hints
            .iter()
            .filter(|path| root.join(path).is_file())
            .map(|path| (*path).to_string())
            .collect();

        if let Some(area) = area_maps.iter().find(|a| a.area_id == *area_id) {
            for owned in &area.owned_files {
                let stem = Path::new(owned)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if related_hints.iter().any(|hint| hint.contains(stem)) {
                    related_files.push(owned.clone());
                }
            }
        }

        related_files.sort();
        related_files.dedup();

        commands.push(CommandMapEntry {
            command: format!("context {subcommand}"),
            entrypoint: CONTEXT_CMD_PATH.to_string(),
            implementation_area: area_id.to_string(),
            related_files,
        });
    }

    commands.sort_by(|a, b| a.command.cmp(&b.command));
    commands
}

pub fn match_command_from_task<'a>(
    task: &str,
    commands: &'a [CommandMapEntry],
) -> Option<&'a CommandMapEntry> {
    let lower = task.to_ascii_lowercase();
    let mut best: Option<(&CommandMapEntry, usize)> = None;
    for entry in commands {
        let cmd = entry.command.to_ascii_lowercase();
        if lower.contains(&cmd) {
            let len = cmd.len();
            if best.is_none_or(|(_, best_len)| len > best_len) {
                best = Some((entry, len));
            }
            continue;
        }
        let mut parts = cmd.split_whitespace();
        let Some(context) = parts.next() else {
            continue;
        };
        let Some(subcommand) = parts.next() else {
            continue;
        };
        if lower.contains(context) && lower.contains(subcommand) {
            let len = subcommand.len();
            if best.is_none_or(|(_, best_len)| len > best_len) {
                best = Some((entry, len));
            }
            continue;
        }
        // `agent-eval` tasks often omit the `context` qualifier but still
        // clearly refer to the command family.
        if subcommand.contains('-') && lower.contains(subcommand) {
            let len = subcommand.len();
            if best.is_none_or(|(_, best_len)| len > best_len) {
                best = Some((entry, len));
            }
        }
    }
    best.map(|(entry, _)| entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(command: &str) -> CommandMapEntry {
        CommandMapEntry {
            command: command.to_string(),
            entrypoint: CONTEXT_CMD_PATH.to_string(),
            implementation_area: "context-harness".to_string(),
            related_files: Vec::new(),
        }
    }

    #[test]
    fn matches_agent_eval_without_context_prefix() {
        let commands = vec![command("context eval"), command("context agent-eval")];
        let matched =
            match_command_from_task("extend agent-eval scoring output", &commands).unwrap();
        assert_eq!(matched.command, "context agent-eval");
    }

    #[test]
    fn prefers_longer_specific_command_match() {
        let commands = vec![command("context eval"), command("context agent-eval")];
        let matched = match_command_from_task("run context agent-eval score", &commands).unwrap();
        assert_eq!(matched.command, "context agent-eval");
    }
}
