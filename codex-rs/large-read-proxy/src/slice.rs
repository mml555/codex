//! Build compact slices from a large file. Pure: given the file content
//! (+ optional symbol hints), pick up to `max_slices` windows of at most
//! `max_lines_per_slice` each, total under `max_total_bytes` — favoring the
//! header, public definitions, and a test module, or windows around hinted
//! symbols when the caller supplies them (e.g. from prior search-proxy
//! evidence in the session).

/// One contiguous window of the file. Line numbers are 1-based inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice {
    pub start: u32,
    pub end: u32,
    pub reason: String,
    /// The actual file lines `[start, end]`, joined with `\n` (no trailing
    /// newline). v2: the renderer emits this content (line-numbered) so the
    /// model sees real code, not just a pointer to a line range — a pointer
    /// reliably triggers the bypass-to-raw behavior we are trying to avoid.
    pub text: String,
}

impl Slice {
    pub fn line_count(&self) -> u32 {
        self.end.saturating_sub(self.start).saturating_add(1)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SliceOptions {
    pub max_slices: usize,
    pub max_lines_per_slice: u32,
    pub max_total_bytes: usize,
}

impl Default for SliceOptions {
    fn default() -> Self {
        Self {
            max_slices: 3,
            max_lines_per_slice: 30,
            max_total_bytes: 6144,
        }
    }
}

pub fn build_slices(content: &str, hints: &[String], opts: &SliceOptions) -> Vec<Slice> {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len() as u32;
    if total == 0 {
        return Vec::new();
    }

    let mut slices: Vec<Slice> = Vec::new();
    let mut used_bytes = 0usize;
    const CTX_BEFORE: u32 = 2;

    // 1. File header — always the first slice.
    let header_end = total.min(opts.max_lines_per_slice);
    try_push(
        &mut slices,
        &mut used_bytes,
        &lines,
        1,
        header_end,
        "file header / imports".to_string(),
        opts,
    );

    // 2. Anchors: hinted symbol matches if provided, else public
    //    definitions + a test module.
    let mut anchors: Vec<(u32, String)> = Vec::new();
    if hints.is_empty() {
        for (i, line) in lines.iter().enumerate() {
            if let Some(reason) = pub_def_reason(line) {
                anchors.push((i as u32 + 1, reason));
            }
        }
        if let Some(idx) = lines
            .iter()
            .position(|l| l.contains("mod tests") || l.trim() == "#[cfg(test)]")
        {
            anchors.push((idx as u32 + 1, "test module".to_string()));
        }
    } else {
        for hint in hints {
            if let Some(idx) = lines.iter().position(|l| l.contains(hint.as_str())) {
                anchors.push((idx as u32 + 1, format!("match: {hint}")));
            }
        }
    }

    for (anchor, reason) in anchors {
        if slices.len() >= opts.max_slices {
            break;
        }
        let start = anchor.saturating_sub(CTX_BEFORE).max(1);
        let end = (start + opts.max_lines_per_slice - 1).min(total);
        try_push(&mut slices, &mut used_bytes, &lines, start, end, reason, opts);
    }

    slices.sort_by_key(|s| s.start);
    slices
}

/// Add `[start, end]` as a slice unless it would exceed `max_slices`,
/// overlap an existing slice, or blow the byte budget.
fn try_push(
    slices: &mut Vec<Slice>,
    used_bytes: &mut usize,
    lines: &[&str],
    start: u32,
    end: u32,
    reason: String,
    opts: &SliceOptions,
) {
    if slices.len() >= opts.max_slices || start > end {
        return;
    }
    if slices.iter().any(|s| start <= s.end && s.start <= end) {
        return;
    }
    let window = &lines[(start as usize - 1)..(end as usize)];
    let bytes: usize = window.iter().map(|l| l.len() + 1).sum();
    if *used_bytes + bytes > opts.max_total_bytes {
        return;
    }
    *used_bytes += bytes;
    let text = window.join("\n");
    slices.push(Slice {
        start,
        end,
        reason,
        text,
    });
}

/// If `line` declares a public item (or a free `fn`), return a short reason
/// naming it. Heuristic, language-agnostic enough for Rust/TS/Python.
fn pub_def_reason(line: &str) -> Option<String> {
    let t = line.trim_start();
    const STARTS: [&str; 9] = [
        "pub fn ",
        "pub struct ",
        "pub enum ",
        "pub trait ",
        "pub const ",
        "pub type ",
        "pub mod ",
        "pub async fn ",
        "fn ",
    ];
    if STARTS.iter().any(|p| t.starts_with(p)) {
        let snippet: String = t.trim_end().chars().take(60).collect();
        Some(format!("definition: {snippet}"))
    } else {
        None
    }
}
