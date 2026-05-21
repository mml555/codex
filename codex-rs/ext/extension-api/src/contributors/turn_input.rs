use codex_protocol::user_input::UserInput;

use crate::ExtensionData;

/// Contributor invoked before prompt context assembly for a turn.
///
/// Hosts call this with the pending user input so extensions can prime thread-scoped
/// state (for example harness task text) before [`super::ContextContributor`] runs.
pub trait TurnInputContributor: Send + Sync {
    /// Prepare extension-owned thread state from the user input about to be recorded.
    fn prepare_turn_input(&self, thread_store: &ExtensionData, input: &[UserInput]);
}
