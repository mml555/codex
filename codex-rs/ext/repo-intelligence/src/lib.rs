//! Harness-native repo intelligence via [`ContextContributor`].

mod extension;
mod run_memory_bridge;
mod user_input;

pub use extension::RepoIntelligenceExtension;
pub use extension::RepoIntelligenceExtensionConfig;
pub use extension::install;
pub use extension::narrow_verification_hint;
pub use run_memory_bridge::RunMemoryBridge;
