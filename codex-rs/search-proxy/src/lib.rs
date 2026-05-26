//! Intent-aware tool proxy for codex.
//!
//! MVP scope: classify model-initiated `rg` invocations so the
//! interceptor can decide whether to substitute compact evidence or
//! pass the command through to the normal shell handler.
//!
//! Other tool families (sed, find, cargo test, apply_patch) are out
//! of scope for this MVP. See branch `search-proxy-mvp`.
//!
//! Commit 1 ships only the classifier. The compact evidence builder
//! (Commit 2) and the shell-handler interception hook (Commit 3) are
//! follow-ups that consume `ClassifiedRg` from this crate.

mod command_classifier;

pub use command_classifier::ClassifiedRg;
pub use command_classifier::ClassifyOutcome;
pub use command_classifier::PassThroughReason;
pub use command_classifier::RgFlags;
pub use command_classifier::classify_command;

#[cfg(test)]
mod command_classifier_tests;
