//! Parser for `rg --json` output.
//!
//! `rg --json` emits one JSON object per line with a `type` of
//! `begin`, `match`, `end`, `context`, or `summary`. We only care
//! about `begin` (to learn each file's path) and `match` (to learn
//! line numbers and matched text). Other event types are ignored.
//!
//! See <https://docs.rs/grep-printer/latest/grep_printer/struct.JSON.html>
//! for the full schema; we model only what the MVP needs.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One file's worth of raw matches as `rg --json` reported them.
/// Hits preserve `rg`'s order so renderers can show "top N lines"
/// deterministically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFileHits {
    pub path: String,
    pub hits: Vec<ParsedHit>,
}

/// A single line-level match event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHit {
    pub line: u32,
    pub column: Option<u32>,
    /// The literal matched line as `rg` returned it, with no
    /// trimming or length capping. The evidence builder cleans it
    /// up.
    pub line_text: String,
}

/// Parse `rg --json` output bytes into per-file hit lists. Lines
/// that don't parse as JSON are skipped (this matches `rg`'s own
/// tolerant behaviour when something interrupts its stream).
///
/// Returns the files in first-seen order, with each file's hits in
/// `rg`'s emission order (= file scan order, normally).
pub fn parse_rg_json(bytes: &[u8]) -> Vec<ParsedFileHits> {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut by_path: BTreeMap<String, ParsedFileHits> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut current_path: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: RgEvent = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };
        match event {
            RgEvent::Begin { data } => {
                let Some(path) = data.path.text() else {
                    continue;
                };
                current_path = Some(path.clone());
                if !by_path.contains_key(&path) {
                    order.push(path.clone());
                    by_path.insert(
                        path.clone(),
                        ParsedFileHits {
                            path,
                            hits: Vec::new(),
                        },
                    );
                }
            }
            RgEvent::Match { data } => {
                let Some(path) = data.path.text().or_else(|| current_path.clone()) else {
                    continue;
                };
                let entry = by_path.entry(path.clone()).or_insert_with(|| {
                    order.push(path.clone());
                    ParsedFileHits {
                        path: path.clone(),
                        hits: Vec::new(),
                    }
                });
                let column = data.submatches.first().map(|sm| (sm.start as u32) + 1);
                let snippet = data.lines.text().unwrap_or_default();
                entry.hits.push(ParsedHit {
                    line: data.line_number,
                    column,
                    line_text: snippet,
                });
            }
            RgEvent::Other => {}
        }
    }

    order
        .into_iter()
        .filter_map(|p| by_path.remove(&p))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RgEvent {
    Begin {
        data: BeginData,
    },
    Match {
        data: MatchData,
    },
    /// `End`, `Context`, and `Summary` events carry payload data we
    /// don't currently consume; they exist as variants only so serde
    /// recognizes them instead of failing to deserialize.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct BeginData {
    path: RgText,
}

#[derive(Debug, Deserialize)]
struct MatchData {
    path: RgText,
    lines: RgText,
    line_number: u32,
    #[serde(default)]
    submatches: Vec<RgSubmatch>,
}

#[derive(Debug, Deserialize)]
struct RgSubmatch {
    start: u64,
}

/// `rg --json` represents text fields as either `{"text": "..."}` for
/// UTF-8 paths/snippets or `{"bytes": "..."}` for non-UTF-8. We only
/// support the UTF-8 form for now — bytes events are dropped (returns
/// `None`).
#[derive(Debug, Deserialize)]
struct RgText {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    bytes: Option<String>,
}

impl RgText {
    fn text(&self) -> Option<String> {
        self.text.clone()
    }
}
