//! Durable-ish structural understanding of a repository.
//!
//! Produces [`RepoMap`] only. Consumers such as `codex-context-harness` must not
//! depend on how the map was built.

mod area_map;
mod builder;
mod cache;
mod churn;
mod command_map;
pub mod extractors;
mod repo_map;
mod signals;
mod test_map;
mod walk;

pub use builder::RepoMapBuilder;
pub use builder::RepoMapBuilderOptions;
pub use cache::RepoIndexCache;
pub use command_map::match_command_from_task;
pub use repo_map::AreaMap;
pub use repo_map::CommandMapEntry;
pub use repo_map::RepoArea;
pub use repo_map::RepoFileEntry;
pub use repo_map::RepoMap;
pub use repo_map::RepoPackage;
pub use repo_map::RepoTestEntry;
pub use repo_map::TestMapEntry;
pub use signals::RepoSignals;
