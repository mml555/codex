use codex_context_harness::RunMemory;

/// Live per-run state updated by tool lifecycle hooks (extension layer).
#[derive(Debug, Clone, Default)]
pub struct RunMemoryBridge {
    pub memory: RunMemory,
}

impl RunMemoryBridge {
    pub fn record_file_read(&mut self, path: impl Into<String>) {
        self.memory.files_read.push(path.into());
    }

    pub fn record_file_edited(&mut self, path: impl Into<String>) {
        self.memory.files_edited.push(path.into());
    }

    pub fn record_command(&mut self, command: impl Into<String>) {
        self.memory.commands_run.push(command.into());
    }

    pub fn record_failure(&mut self, failure: impl Into<String>) {
        self.memory.failures.push(failure.into());
    }
}
