//! Harness-native repo intelligence via [`ContextContributor`].

mod extension;
mod run_memory_bridge;

pub use extension::RepoIntelligenceExtension;
pub use extension::RepoIntelligenceExtensionConfig;
pub use extension::install;
pub use run_memory_bridge::RunMemoryBridge;
