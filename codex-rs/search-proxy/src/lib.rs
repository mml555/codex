//! Intent-aware tool proxy for codex.
//!
//! MVP scope: classify model-initiated `rg` invocations
//! ([`classify_command`]) and, for the eligible ones, run a bounded
//! internal `rg` and produce a compact-evidence summary
//! ([`build_proxy_response`]) the shell handler can substitute in
//! place of raw `rg` output.
//!
//! Other tool families (sed, find, cargo test, apply_patch) are out
//! of scope for this MVP. See branch `search-proxy-mvp`.
//!
//! Commits:
//!   1. classifier (this commit, landed earlier)
//!   2. evidence builder + renderer + runner (this commit)
//!   3. shell-handler interception hook (follow-up)
//!   4. eval fixture + cloud A/B (follow-up)

mod command_classifier;
mod evidence;
mod evidence_builder;
mod evidence_renderer;
mod file_class;
mod relevance;
mod rg_json;
mod rg_runner;

pub use command_classifier::ClassifiedRg;
pub use command_classifier::ClassifyOutcome;
pub use command_classifier::PassThroughReason;
pub use command_classifier::RgFlags;
pub use command_classifier::classify_command;

pub use evidence::CompactEvidence;
pub use evidence::EvidenceOptions;
pub use evidence::FileClass;
pub use evidence::FileEvidence;
pub use evidence::HitLine;

pub use evidence_builder::ProxyOutcome;
pub use evidence_builder::ProxyPassThroughReason;
pub use evidence_builder::build_proxy_response;

pub use rg_runner::RawSearchOutput;
pub use rg_runner::RgExitStatus;
pub use rg_runner::RipgrepRunner;
pub use rg_runner::SearchRunner;
pub use rg_runner::SearchRunnerError;
pub use rg_runner::StaticRunner;

#[cfg(test)]
mod command_classifier_tests;
#[cfg(test)]
mod evidence_builder_tests;
#[cfg(test)]
mod rg_json_tests;
