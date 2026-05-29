//! Classify whether a shell command is a *large file read* the proxy
//! should intercept: `cat <file>` or `sed -n '<start>,<end>p' <file>` with
//! a large requested range.
//!
//! Conservative by design (mirrors the search-proxy classifier): anything
//! we can't cleanly reason about — flags, multiple files, chained commands,
//! small ranges — passes through. The only harm the proxy can do is delay a
//! read by one turn (the escape hatch always lets a repeat through), so we
//! prefer false negatives over false positives.

use std::fmt;

/// `sed -n '1,Np'` is only intercepted when the requested span is at least
/// this many lines — reading a few lines is already cheap.
pub const MIN_SED_RANGE_LINES: u32 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyOutcome {
    PassThrough(PassThroughReason),
    Eligible(ClassifiedRead),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassThroughReason {
    /// Not a recognized read command (`cat` / `sed -n <range>p`).
    NotReadCommand,
    /// A shell metacharacter (`;`, `&&`, `|`, `>`, `$(`, …) — chained.
    ShellMetacharacter,
    /// `sed` range is below the large-read threshold; cheap, leave it.
    SmallRange,
    /// `cat`/`sed` with flags or multiple files we don't reason about.
    UnsupportedArgs,
    /// Empty / unparsable.
    UnsupportedShape,
}

impl fmt::Display for PassThroughReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PassThroughReason::NotReadCommand => "not_read_command",
            PassThroughReason::ShellMetacharacter => "shell_metacharacter",
            PassThroughReason::SmallRange => "small_range",
            PassThroughReason::UnsupportedArgs => "unsupported_args",
            PassThroughReason::UnsupportedShape => "unsupported_shape",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTool {
    Cat,
    Sed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedRead {
    pub tool: ReadTool,
    pub path: String,
    /// `(start, end)` requested line range for `sed` (1-based inclusive;
    /// `end` is `u32::MAX` for `1,$p`). `None` for `cat` (whole file).
    pub requested_range: Option<(u32, u32)>,
    /// Canonical text used as the escape-hatch registry key.
    pub normalized: String,
}

pub fn classify_command(raw: &str) -> ClassifyOutcome {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape);
    }

    let outer = match shlex::split(trimmed) {
        Some(t) if !t.is_empty() => t,
        _ => return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape),
    };
    let inner_script = unwrap_shell_script(&outer).unwrap_or_else(|| trimmed.to_string());
    if has_unquoted_shell_metacharacter(&inner_script) {
        return ClassifyOutcome::PassThrough(PassThroughReason::ShellMetacharacter);
    }
    let tokens = match shlex::split(&inner_script) {
        Some(t) if !t.is_empty() => t,
        _ => return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape),
    };

    match tokens.first().map(String::as_str) {
        Some("cat") => classify_cat(&tokens[1..]),
        Some("sed") => classify_sed(&tokens[1..]),
        _ => ClassifyOutcome::PassThrough(PassThroughReason::NotReadCommand),
    }
}

fn classify_cat(args: &[String]) -> ClassifyOutcome {
    // Exactly one positional file, no flags.
    if args.len() != 1 || args[0].starts_with('-') {
        return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedArgs);
    }
    let path = args[0].clone();
    ClassifyOutcome::Eligible(ClassifiedRead {
        tool: ReadTool::Cat,
        normalized: format!("cat {path}"),
        path,
        requested_range: None,
    })
}

fn classify_sed(args: &[String]) -> ClassifyOutcome {
    // Only the exact `-n <range>p <file>` shape.
    if args.len() != 3 || args[0] != "-n" || args[2].starts_with('-') {
        return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedArgs);
    }
    let Some((start, end)) = parse_sed_range(&args[1]) else {
        return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedArgs);
    };
    let span = end.saturating_sub(start).saturating_add(1);
    if span < MIN_SED_RANGE_LINES {
        return ClassifyOutcome::PassThrough(PassThroughReason::SmallRange);
    }
    let path = args[2].clone();
    ClassifyOutcome::Eligible(ClassifiedRead {
        tool: ReadTool::Sed,
        normalized: format!("sed -n {} {path}", args[1]),
        path,
        requested_range: Some((start, end)),
    })
}

/// Parse a `sed` print range like `1,400p`, `10,$p`. Returns `(start, end)`
/// with `end = u32::MAX` for `$`. Rejects anything that isn't a simple
/// `<start>,<end>p` numeric (or `$`) range.
fn parse_sed_range(tok: &str) -> Option<(u32, u32)> {
    let body = tok.strip_suffix('p')?;
    let (a, b) = body.split_once(',')?;
    let start: u32 = a.parse().ok()?;
    let end: u32 = if b == "$" { u32::MAX } else { b.parse().ok()? };
    if end < start {
        return None;
    }
    Some((start, end))
}

fn unwrap_shell_script(outer: &[String]) -> Option<String> {
    if outer.len() < 3 || !is_shell_executable(&outer[0]) {
        return None;
    }
    let mut i = 1;
    while i < outer.len() {
        match outer[i].as_str() {
            "-c" | "-lc" => {
                if i + 1 == outer.len() - 1 {
                    return Some(outer[i + 1].clone());
                }
                return None;
            }
            "-l" | "-i" => i += 1,
            _ => return None,
        }
    }
    None
}

fn is_shell_executable(token: &str) -> bool {
    matches!(
        token,
        "/bin/zsh" | "/bin/bash" | "/bin/sh" | "zsh" | "bash" | "sh"
    )
}

fn has_unquoted_shell_metacharacter(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && !in_single {
            i += 2;
            continue;
        }
        if b == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }
        if in_single || in_double {
            i += 1;
            continue;
        }
        match b {
            b';' | b'|' | b'>' | b'<' | b'&' | b'`' => return true,
            b'$' if bytes.get(i + 1) == Some(&b'(') => return true,
            _ => {}
        }
        i += 1;
    }
    false
}
