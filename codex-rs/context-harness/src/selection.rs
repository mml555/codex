/// Hard caps and thresholds for context selection (M11 relevance tuning).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionCaps {
    pub max_included_files: usize,
    pub max_file_summaries: usize,
    pub max_tests: usize,
    pub max_dropped_logged: usize,
    pub max_prompt_included_files: usize,
    pub max_prompt_full_files: usize,
    pub max_prompt_compact_files: usize,
    pub max_prompt_tests: usize,
    pub max_prompt_warnings: usize,
    /// Top-K relevance-scored files that get marked as "likely edit targets"
    /// in the directive prompt. Files ranked below K render under
    /// "Orientation only". K=1 by default: the first cloud batch's
    /// `rate_limit` failure showed that listing every relevant file as an
    /// edit candidate made the model broaden scope.
    pub max_edit_targets: usize,
    pub include_relevance_min: f64,
    pub drop_confidence_below: f64,
}

impl Default for SelectionCaps {
    fn default() -> Self {
        Self {
            max_included_files: 8,
            max_file_summaries: 6,
            max_tests: 5,
            max_dropped_logged: 12,
            max_prompt_included_files: 8,
            max_prompt_full_files: 4,
            max_prompt_compact_files: 4,
            max_prompt_tests: 5,
            max_prompt_warnings: 3,
            max_edit_targets: 1,
            include_relevance_min: 0.45,
            drop_confidence_below: 0.25,
        }
    }
}

impl SelectionCaps {
    pub fn truncate_dropped<T>(&self, entries: &mut Vec<T>) {
        entries.truncate(self.max_dropped_logged);
    }
}
