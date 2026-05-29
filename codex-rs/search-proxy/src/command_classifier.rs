//! Classify whether a shell command is a simple, safe `rg` invocation
//! that the search proxy can intercept.
//!
//! The classifier is intentionally conservative: any command shape we
//! cannot reason about cleanly returns
//! [`ClassifyOutcome::PassThrough`] so the normal shell handler runs.
//! Adding a flag to the allow-list is a deliberate decision — see
//! [`classify_long_flag`] and [`classify_short_flag_token`].

use std::collections::BTreeSet;
use std::fmt;

/// Outcome of classifying a model-issued shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyOutcome {
    /// The proxy should NOT touch this command. The shell handler
    /// runs it unchanged. The carried reason is for metrics only.
    PassThrough(PassThroughReason),
    /// The command is a simple `rg` invocation the proxy can intercept.
    Eligible(ClassifiedRg),
}

/// Why the classifier declined to intercept. Surfaced through the
/// per-session metrics so we can tell whether the proxy is firing too
/// rarely vs the model's actual search pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassThroughReason {
    /// First token after wrapper stripping is not `rg`.
    NotRg,
    /// Found a shell metacharacter the classifier refuses to reason
    /// about (`;`, `&&`, `||`, `|`, `>`, `<`, `$(`, backtick, etc.).
    ShellMetacharacter,
    /// `rg` was invoked with a flag we don't have an explicit
    /// allow-rule for. Conservative default: pass through.
    UnknownFlag,
    /// `rg` invocation with no positional query (e.g. `rg --files`).
    /// The MVP only handles content searches.
    NoQuery,
    /// `rg` invoked in an output mode the proxy can't faithfully compact
    /// (`-l`/`--files-with-matches`, `-c`/`--count`) — these change the
    /// output shape (filenames / counts), not just its volume.
    OutputModeUnsupported,
    /// Outer wrapper was not parseable, command failed shlex, or
    /// other structural problems.
    UnsupportedShape,
}

impl fmt::Display for PassThroughReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PassThroughReason::NotRg => "not_rg",
            PassThroughReason::ShellMetacharacter => "shell_metacharacter",
            PassThroughReason::UnknownFlag => "unknown_flag",
            PassThroughReason::NoQuery => "no_query",
            PassThroughReason::OutputModeUnsupported => "output_mode_unsupported",
            PassThroughReason::UnsupportedShape => "unsupported_shape",
        };
        f.write_str(s)
    }
}

/// A simple `rg <query> [paths...]` invocation. Downstream commits
/// build compact evidence from this struct and use `normalized` as the
/// escape-hatch registry key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedRg {
    /// The literal pattern argument the model is searching for.
    pub query: String,
    /// Optional positional path arguments restricting the search.
    /// Sorted alphabetically in `normalized` so different argument
    /// orders collapse to one registry key.
    pub target_paths: Vec<String>,
    /// Parsed flag state.
    pub flags: RgFlags,
    /// Canonical text used by the interceptor to detect a repeat call.
    /// Two commands with the same semantics but different shell quoting
    /// or flag ordering produce the same `normalized` string.
    pub normalized: String,
}

/// Parsed `rg` flags relevant to the MVP. Flags we recognize but that
/// only affect output formatting (`-n`, `--no-heading`, `--color=*`)
/// are accepted without being mirrored here — they don't change which
/// files match and the compact-evidence builder picks its own format.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RgFlags {
    pub ignore_case: bool,
    pub smart_case: bool,
    pub word_regexp: bool,
    pub fixed_strings: bool,
    pub files_only: bool, // `-l` / `--files-with-matches`
    pub count_only: bool, // `-c` / `--count`
    pub multiline: bool,
    pub max_count: Option<u32>,
    pub context_before: Option<u32>, // `-B N` / `--before-context=N`
    pub context_after: Option<u32>,  // `-A N` / `--after-context=N`
    pub context_around: Option<u32>, // `-C N` / `--context=N`
    /// `-t TYPE` filters in original order. Multiple `-t` flags
    /// accumulate.
    pub type_filters: Vec<String>,
    /// `-g GLOB` filters in original order.
    pub glob_filters: Vec<String>,
}

/// Strip a single leading `pwd &&` no-op prefix from the inner script.
///
/// The model frequently prepends `pwd &&` to a search to print the working
/// directory first (e.g. `pwd && rg "foo" -S .`). `pwd` has no effect on what
/// rg searches — it does NOT change the search root the way `cd <dir>` would —
/// so the command is semantically equivalent to the bare `rg ...` for
/// interception. Stripping it lets the metacharacter guard see a clean single
/// command instead of passing the whole thing through on the `&&`.
///
/// Deliberately conservative:
/// - only `pwd` is recognized (never `cd <dir> &&`, which changes the search
///   root and must keep passing through);
/// - only as a LEADING prefix, and only ONE occurrence — a second `&&`, a
///   trailing chain, or a pipe is left intact so the metacharacter guard still
///   passes it through;
/// - `pwd` must be a standalone token (`pwdx`, `pwd_foo` are not stripped).
fn strip_leading_pwd_and(script: &str) -> &str {
    let s = script.trim_start();
    let rest = match s.strip_prefix("pwd") {
        Some(rest) => rest,
        None => return s,
    };
    // The char immediately after `pwd` must be whitespace or the start of the
    // `&&` operator (e.g. `pwd&&`); otherwise it's a different token.
    if !(rest.starts_with(char::is_whitespace) || rest.starts_with("&&")) {
        return s;
    }
    match rest.trim_start().strip_prefix("&&") {
        Some(after) => after.trim_start(),
        None => s,
    }
}

/// Quote-aware scanner for shell metacharacters.
///
/// Returns `true` iff any of `; | > < & $( backtick` appears outside
/// single- and double-quoted spans. Backslash escapes the next
/// character outside single quotes (matching POSIX shell behavior for
/// the unquoted and double-quoted regions we care about here).
///
/// Tokens like `xargs` are NOT in the metacharacter set: in a
/// well-formed pipeline the `|` immediately before them already
/// triggers the metacharacter check, and a bare `xargs foo` invocation
/// is not a shell metacharacter — it's just a different executable
/// that lands at [`PassThroughReason::NotRg`] via the executable check.
fn has_unquoted_shell_metacharacter(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let b = bytes[i];

        // Backslash escapes the next byte outside single-quoted spans.
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

/// Classify a raw shell command (the literal `cmd` argument the
/// model passed to the shell tool) into a proxy decision.
///
/// The classifier does **not** execute anything. It does not read the
/// filesystem. It is pure function-of-string.
pub fn classify_command(raw_cmd: &str) -> ClassifyOutcome {
    let trimmed = raw_cmd.trim();
    if trimmed.is_empty() {
        return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape);
    }

    // First pass: tokenize the outer command. shlex applies one layer
    // of shell unescaping — what bash/sh would do when interpreting
    // the model-emitted command string before invoking any inner
    // shell.
    let outer_tokens = match shlex::split(trimmed) {
        Some(t) if !t.is_empty() => t,
        _ => return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape),
    };

    // If the outer command is a `<shell> -lc "<script>"` wrapper,
    // pull the script out as a single string. Otherwise the trimmed
    // raw command is itself the script.
    let inner_script_raw =
        unwrap_shell_script(&outer_tokens).unwrap_or_else(|| trimmed.to_string());

    // A leading `pwd &&` is a no-op prefix the model commonly prepends to
    // echo the working directory before searching. Strip one such prefix so
    // the metacharacter guard below sees a clean single command instead of
    // passing the whole thing through on the `&&`. See `strip_leading_pwd_and`.
    let inner_script = strip_leading_pwd_and(&inner_script_raw);

    // Quote-aware metacharacter check on the inner script. This is
    // the string the inner shell would parse, so unquoted `;`, `|`,
    // redirect, command-substitution, etc. would chain rg to other
    // commands.
    if has_unquoted_shell_metacharacter(inner_script) {
        return ClassifyOutcome::PassThrough(PassThroughReason::ShellMetacharacter);
    }

    let tokens = match shlex::split(inner_script) {
        Some(t) if !t.is_empty() => t,
        _ => return ClassifyOutcome::PassThrough(PassThroughReason::UnsupportedShape),
    };

    let exe = tokens.first().map(String::as_str).unwrap_or("");
    if !is_rg_executable(exe) {
        return ClassifyOutcome::PassThrough(PassThroughReason::NotRg);
    }

    classify_rg_tokens(&tokens[1..])
}

fn is_rg_executable(token: &str) -> bool {
    token == "rg" || token.ends_with("/rg") || token.ends_with("\\rg") // Windows path form, just in case.
}

/// Detect a `<shell> -c <script>` (or `-lc`) wrapper in
/// already-shlex-split tokens and return the inner script. Returns
/// `None` if the tokens don't match a wrapper shape.
fn unwrap_shell_script(outer_tokens: &[String]) -> Option<String> {
    if outer_tokens.len() < 3 {
        return None;
    }
    if !is_shell_executable(&outer_tokens[0]) {
        return None;
    }
    let mut i = 1;
    while i < outer_tokens.len() {
        match outer_tokens[i].as_str() {
            "-c" | "-lc" => {
                // Expect the script as the single next token, and
                // nothing after it (codex emits this exact shape).
                if i + 1 == outer_tokens.len() - 1 {
                    return Some(outer_tokens[i + 1].clone());
                }
                return None;
            }
            // Skip benign shell-mode toggles before `-c`.
            "-l" | "-i" => {
                i += 1;
            }
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

/// Classify an already-tokenized `rg` argv (i.e. tokens AFTER the
/// `rg` executable token).
fn classify_rg_tokens(args: &[String]) -> ClassifyOutcome {
    let mut flags = RgFlags::default();
    let mut positional: Vec<String> = Vec::new();
    // Track which canonical short flags fired so the normalized form
    // is order-independent.
    let mut canonical_flags: BTreeSet<&'static str> = BTreeSet::new();

    let mut i = 0;
    while i < args.len() {
        let tok = args[i].as_str();
        // Sentinel: anything after `--` is positional.
        if tok == "--" {
            positional.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(after_dashes) = tok.strip_prefix("--") {
            // Long flag. Two forms: `--name=value` and `--name value`.
            let (name, value_inline) = match after_dashes.find('=') {
                Some(eq) => (&after_dashes[..eq], Some(&after_dashes[eq + 1..])),
                None => (after_dashes, None),
            };
            match classify_long_flag(name, value_inline, &args[i + 1..], &mut flags) {
                LongFlagOutcome::Ok { consumed, canon } => {
                    canonical_flags.insert(canon);
                    i += consumed;
                    continue;
                }
                LongFlagOutcome::Pure { canon } => {
                    canonical_flags.insert(canon);
                    i += 1;
                    continue;
                }
                LongFlagOutcome::Unknown => {
                    return ClassifyOutcome::PassThrough(PassThroughReason::UnknownFlag);
                }
            }
        }
        if tok.starts_with('-') && tok.len() > 1 {
            // Short flag (single or clustered).
            match classify_short_flag_token(tok, &args[i + 1..], &mut flags) {
                ShortFlagOutcome::Ok { consumed, canons } => {
                    for c in canons {
                        canonical_flags.insert(c);
                    }
                    i += consumed;
                    continue;
                }
                ShortFlagOutcome::Unknown => {
                    return ClassifyOutcome::PassThrough(PassThroughReason::UnknownFlag);
                }
            }
        }
        positional.push(tok.to_string());
        i += 1;
    }

    if positional.is_empty() {
        return ClassifyOutcome::PassThrough(PassThroughReason::NoQuery);
    }

    // `-l`/`--files-with-matches` and `-c`/`--count` change rg's OUTPUT SHAPE
    // (filenames-only / per-file counts) rather than emitting match lines.
    // The proxy only compacts match-line output — it runs its own rg without
    // forwarding these flags and the renderer always emits match lines, so
    // substituting here would change the command's semantics, not just
    // compact it. Pass through and let the model's own command run.
    if flags.files_only || flags.count_only {
        return ClassifyOutcome::PassThrough(PassThroughReason::OutputModeUnsupported);
    }

    let query = positional.remove(0);
    let mut target_paths = positional;
    target_paths.sort();

    let normalized = render_normalized(
        &query,
        &target_paths,
        &flags,
        &canonical_flags.into_iter().collect::<Vec<_>>(),
    );

    ClassifyOutcome::Eligible(ClassifiedRg {
        query,
        target_paths,
        flags,
        normalized,
    })
}

enum LongFlagOutcome {
    /// Flag consumed `consumed` tokens (including itself).
    Ok {
        consumed: usize,
        canon: &'static str,
    },
    /// Flag is recognized but takes no argument. Caller advances by 1.
    Pure {
        canon: &'static str,
    },
    Unknown,
}

enum ShortFlagOutcome {
    Ok {
        consumed: usize,
        canons: Vec<&'static str>,
    },
    Unknown,
}

/// Recognized long flags. `value_inline` is `Some` for `--flag=value`,
/// `None` for `--flag value` (in which case `lookahead` carries the
/// next token).
fn classify_long_flag(
    name: &str,
    value_inline: Option<&str>,
    lookahead: &[String],
    flags: &mut RgFlags,
) -> LongFlagOutcome {
    // Boolean long flags (no arg).
    let boolean = |canon: &'static str, set: &mut bool| {
        *set = true;
        LongFlagOutcome::Pure { canon }
    };

    match name {
        "ignore-case" => return boolean("-i", &mut flags.ignore_case),
        "smart-case" => return boolean("-S", &mut flags.smart_case),
        "word-regexp" => return boolean("-w", &mut flags.word_regexp),
        "fixed-strings" => return boolean("-F", &mut flags.fixed_strings),
        "files-with-matches" => return boolean("-l", &mut flags.files_only),
        "count" => return boolean("-c", &mut flags.count_only),
        "multiline" => {
            flags.multiline = true;
            return LongFlagOutcome::Pure {
                canon: "--multiline",
            };
        }
        // Pure formatting / harmless toggles — accepted, not mirrored.
        "line-number" | "no-line-number" | "no-heading" | "heading" | "no-color" | "json" => {
            return LongFlagOutcome::Pure { canon: "--fmt" };
        }
        _ => {}
    }

    // Long flags that take a value.
    let value = match value_inline {
        Some(v) => Some(v.to_string()),
        None => lookahead.first().cloned(),
    };
    let consumed = if value_inline.is_some() { 1 } else { 2 };

    let parsed_u32 = || value.as_deref().and_then(|s| s.parse::<u32>().ok());

    match name {
        "max-count" => {
            let n = match parsed_u32() {
                Some(n) => n,
                None => return LongFlagOutcome::Unknown,
            };
            flags.max_count = Some(n);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-m",
            }
        }
        "after-context" => {
            let n = match parsed_u32() {
                Some(n) => n,
                None => return LongFlagOutcome::Unknown,
            };
            flags.context_after = Some(n);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-A",
            }
        }
        "before-context" => {
            let n = match parsed_u32() {
                Some(n) => n,
                None => return LongFlagOutcome::Unknown,
            };
            flags.context_before = Some(n);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-B",
            }
        }
        "context" => {
            let n = match parsed_u32() {
                Some(n) => n,
                None => return LongFlagOutcome::Unknown,
            };
            flags.context_around = Some(n);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-C",
            }
        }
        "type" => {
            let v = match value {
                Some(v) => v,
                None => return LongFlagOutcome::Unknown,
            };
            flags.type_filters.push(v);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-t",
            }
        }
        "glob" => {
            let v = match value {
                Some(v) => v,
                None => return LongFlagOutcome::Unknown,
            };
            flags.glob_filters.push(v);
            LongFlagOutcome::Ok {
                consumed,
                canon: "-g",
            }
        }
        "color" => {
            // `--color=never|auto|always` is accepted; the proxy doesn't care.
            if value.is_none() {
                return LongFlagOutcome::Unknown;
            }
            LongFlagOutcome::Ok {
                consumed,
                canon: "--fmt",
            }
        }
        _ => LongFlagOutcome::Unknown,
    }
}

/// Recognize a short-flag token of the form `-x`, `-xyz` (clustered
/// booleans), or `-mN` (flag with attached numeric value).
fn classify_short_flag_token(
    tok: &str,
    lookahead: &[String],
    flags: &mut RgFlags,
) -> ShortFlagOutcome {
    let body = &tok[1..];
    let mut canons: Vec<&'static str> = Vec::new();

    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'i' => {
                flags.ignore_case = true;
                canons.push("-i");
            }
            'S' => {
                flags.smart_case = true;
                canons.push("-S");
            }
            'w' => {
                flags.word_regexp = true;
                canons.push("-w");
            }
            'F' => {
                flags.fixed_strings = true;
                canons.push("-F");
            }
            'l' => {
                flags.files_only = true;
                canons.push("-l");
            }
            'c' => {
                flags.count_only = true;
                canons.push("-c");
            }
            'n' | 'N' | 'H' => {
                // -n / --line-number, -N / --no-line-number, -H / --with-filename:
                // formatting-only, accepted.
                canons.push("--fmt");
            }
            'm' | 'A' | 'B' | 'C' | 't' | 'g' => {
                // Flag with an argument. Either attached (`-m20`) or
                // next token (`-m 20`).
                let arg_str: String = chars.clone().collect();
                let (value_str, consumes_lookahead) = if arg_str.is_empty() {
                    match lookahead.first() {
                        Some(v) => (v.clone(), true),
                        None => return ShortFlagOutcome::Unknown,
                    }
                } else {
                    (arg_str, false)
                };
                match c {
                    'm' => {
                        let n: u32 = match value_str.parse() {
                            Ok(n) => n,
                            Err(_) => return ShortFlagOutcome::Unknown,
                        };
                        flags.max_count = Some(n);
                        canons.push("-m");
                    }
                    'A' => {
                        let n: u32 = match value_str.parse() {
                            Ok(n) => n,
                            Err(_) => return ShortFlagOutcome::Unknown,
                        };
                        flags.context_after = Some(n);
                        canons.push("-A");
                    }
                    'B' => {
                        let n: u32 = match value_str.parse() {
                            Ok(n) => n,
                            Err(_) => return ShortFlagOutcome::Unknown,
                        };
                        flags.context_before = Some(n);
                        canons.push("-B");
                    }
                    'C' => {
                        let n: u32 = match value_str.parse() {
                            Ok(n) => n,
                            Err(_) => return ShortFlagOutcome::Unknown,
                        };
                        flags.context_around = Some(n);
                        canons.push("-C");
                    }
                    't' => {
                        flags.type_filters.push(value_str);
                        canons.push("-t");
                    }
                    'g' => {
                        flags.glob_filters.push(value_str);
                        canons.push("-g");
                    }
                    _ => unreachable!(),
                }
                let consumed = if consumes_lookahead { 2 } else { 1 };
                return ShortFlagOutcome::Ok { consumed, canons };
            }
            _ => return ShortFlagOutcome::Unknown,
        }
    }
    ShortFlagOutcome::Ok {
        consumed: 1,
        canons,
    }
}

/// Render a deterministic string that uniquely identifies the
/// (query, flags, targets) tuple. Used as the registry key for the
/// repeat-command escape hatch. Always starts with `rg ` so it's
/// easy to read in logs.
fn render_normalized(
    query: &str,
    target_paths: &[String],
    flags: &RgFlags,
    canonical_flags: &[&str],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push("rg".to_string());

    // Stable, deterministic flag rendering: boolean canonicals
    // sorted, then valued canonicals with their parsed value.
    let mut bool_canons: Vec<&str> = canonical_flags
        .iter()
        .copied()
        .filter(|c| !matches!(*c, "-m" | "-A" | "-B" | "-C" | "-t" | "-g" | "--fmt"))
        .collect();
    bool_canons.sort();
    bool_canons.dedup();
    for c in bool_canons {
        parts.push(c.to_string());
    }

    if let Some(n) = flags.max_count {
        parts.push(format!("-m{n}"));
    }
    if let Some(n) = flags.context_after {
        parts.push(format!("-A{n}"));
    }
    if let Some(n) = flags.context_before {
        parts.push(format!("-B{n}"));
    }
    if let Some(n) = flags.context_around {
        parts.push(format!("-C{n}"));
    }

    let mut type_filters = flags.type_filters.clone();
    type_filters.sort();
    for t in type_filters {
        parts.push(format!("-t{t}"));
    }

    let mut glob_filters = flags.glob_filters.clone();
    glob_filters.sort();
    for g in glob_filters {
        parts.push(format!("-g{g}"));
    }

    // Quote the query so it round-trips even when it contains spaces.
    parts.push(format!("{query:?}"));

    for p in target_paths {
        parts.push(p.clone());
    }
    parts.join(" ")
}
