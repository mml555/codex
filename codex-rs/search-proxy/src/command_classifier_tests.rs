use pretty_assertions::assert_eq;

use super::ClassifyOutcome;
use super::PassThroughReason;
use super::classify_command;

fn expect_eligible(cmd: &str) -> super::ClassifiedRg {
    match classify_command(cmd) {
        ClassifyOutcome::Eligible(r) => r,
        other => panic!("expected Eligible, got {other:?} for cmd={cmd}"),
    }
}

fn expect_pass_through(cmd: &str) -> PassThroughReason {
    match classify_command(cmd) {
        ClassifyOutcome::PassThrough(r) => r,
        other => panic!("expected PassThrough, got {other:?} for cmd={cmd}"),
    }
}

#[test]
fn plain_rg_query_is_eligible() {
    let r = expect_eligible("rg AgentEvalResult");
    assert_eq!(r.query, "AgentEvalResult");
    assert!(r.target_paths.is_empty(), "no targets: {r:?}");
    assert!(!r.flags.ignore_case);
}

#[test]
fn rg_with_target_path_is_eligible() {
    let r = expect_eligible("rg classify_result context-harness");
    assert_eq!(r.query, "classify_result");
    assert_eq!(r.target_paths, vec!["context-harness".to_string()]);
}

#[test]
fn double_quoted_query_is_unquoted_by_shlex() {
    let r = expect_eligible("rg \"AgentEvalResult\"");
    assert_eq!(r.query, "AgentEvalResult");
}

#[test]
fn single_quoted_query_is_unquoted_by_shlex() {
    let r = expect_eligible("rg 'classify_result'");
    assert_eq!(r.query, "classify_result");
}

#[test]
fn shlex_dequotes_inner_double_quoted_regex_with_pipes() {
    // The Run 8 vanilla command shape after wrapper stripping. The `|`
    // chars are inside a double-quoted regex and must NOT trigger the
    // shell-metacharacter check.
    let cmd = r#"rg -n "mod agent_eval|classify_result|AgentEvalResult|\#\[cfg\(test\)\] mod tests" -S ."#;
    let r = expect_eligible(cmd);
    assert!(
        r.query.contains("classify_result"),
        "query should include the alternation: {r:?}"
    );
    assert_eq!(r.target_paths, vec![".".to_string()]);
    assert!(r.flags.smart_case, "-S should set smart_case: {r:?}");
}

#[test]
fn zsh_wrapper_is_stripped() {
    let r = expect_eligible("/bin/zsh -lc \"rg AgentEvalResult\"");
    assert_eq!(r.query, "AgentEvalResult");
}

#[test]
fn bash_wrapper_is_stripped() {
    let r = expect_eligible("/bin/bash -lc \"rg classify_result core/src\"");
    assert_eq!(r.query, "classify_result");
    assert_eq!(r.target_paths, vec!["core/src".to_string()]);
}

#[test]
fn short_lc_wrapper_with_single_quotes_works() {
    let r = expect_eligible("bash -c 'rg HARNESS_MARKER'");
    assert_eq!(r.query, "HARNESS_MARKER");
}

#[test]
fn non_rg_executable_passes_through_as_not_rg() {
    assert_eq!(
        expect_pass_through("grep -r foo context-harness"),
        PassThroughReason::NotRg
    );
    assert_eq!(
        expect_pass_through("/bin/zsh -lc \"sed -n 1,80p file.rs\""),
        PassThroughReason::NotRg
    );
    assert_eq!(expect_pass_through("just test"), PassThroughReason::NotRg);
    assert_eq!(expect_pass_through("cat foo"), PassThroughReason::NotRg);
}

#[test]
fn unquoted_pipe_is_shell_metacharacter() {
    assert_eq!(
        expect_pass_through("rg foo | head -5"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn semicolon_chaining_is_shell_metacharacter() {
    assert_eq!(
        expect_pass_through("rg foo ; rm -rf /tmp/x"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn and_chaining_is_shell_metacharacter() {
    assert_eq!(
        expect_pass_through("rg foo && echo done"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn leading_pwd_and_prefix_is_stripped_and_eligible() {
    // The model commonly prefixes a search with `pwd &&` to echo the cwd.
    // `pwd` does not change the search root, so the command is equivalent to
    // the bare `rg ...` and should be intercepted, not passed through on `&&`.
    let r = expect_eligible(r#"pwd && rg -n "foo|bar" -S ."#);
    assert_eq!(r.query, "foo|bar");
    assert_eq!(r.target_paths, vec![".".to_string()]);
    assert!(r.flags.smart_case);
}

#[test]
fn leading_pwd_and_prefix_in_zsh_wrapper_is_eligible() {
    // The exact shape that went inert in the Track-D Slot-1 cloud A/B:
    // `/bin/zsh -lc 'pwd && rg -n "..." -S .'` produced a 432 KB whole-tree
    // search that SP passed through on the `&&`. With the prefix stripped it
    // is intercepted and the broad search can be compacted.
    let r =
        expect_eligible(r#"/bin/zsh -lc 'pwd && rg -n "rollout|jsonl|resume|reopen|record" -S .'"#);
    assert_eq!(r.query, "rollout|jsonl|resume|reopen|record");
    assert_eq!(r.target_paths, vec![".".to_string()]);
}

#[test]
fn leading_pwd_and_normalizes_same_as_bare_rg() {
    // The `pwd &&` prefix must not leak into the escape-hatch registry key,
    // so a repeat of either shape collapses to the same normalized command.
    let with_pwd = expect_eligible(r#"pwd && rg "foo" ."#);
    let bare = expect_eligible(r#"rg "foo" ."#);
    assert_eq!(with_pwd.normalized, bare.normalized);
}

#[test]
fn cd_prefix_still_passes_through() {
    // `cd <dir> &&` changes the search root, so it must NOT be stripped —
    // intercepting it would search the wrong directory.
    assert_eq!(
        expect_pass_through("cd rollout && rg foo"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn pwd_with_trailing_chain_still_passes_through() {
    // Only ONE leading `pwd &&` is stripped; a further chained command keeps
    // the metacharacter guard engaged.
    assert_eq!(
        expect_pass_through("pwd && rg foo && echo done"),
        PassThroughReason::ShellMetacharacter
    );
    assert_eq!(
        expect_pass_through("pwd && rg foo | head"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn pwd_prefix_token_must_be_standalone() {
    // `pwdx` is a different token; the `&&` it chains to must still trip the
    // metacharacter guard rather than being mistaken for a `pwd` prefix.
    assert_eq!(
        expect_pass_through("pwdx && rg foo"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn redirect_is_shell_metacharacter() {
    assert_eq!(
        expect_pass_through("rg foo > out.txt"),
        PassThroughReason::ShellMetacharacter
    );
    assert_eq!(
        expect_pass_through("rg foo < in.txt"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn command_substitution_is_shell_metacharacter() {
    assert_eq!(
        expect_pass_through("rg $(date) ."),
        PassThroughReason::ShellMetacharacter
    );
    assert_eq!(
        expect_pass_through("rg `date` ."),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn metachars_inside_double_quotes_are_ignored() {
    // The Run 8 case: alternation inside a quoted query is fine.
    let r = expect_eligible(r#"rg "foo|bar" ."#);
    assert_eq!(r.query, "foo|bar");
}

#[test]
fn metachars_inside_single_quotes_are_ignored() {
    let r = expect_eligible("rg 'foo|bar;baz' .");
    assert_eq!(r.query, "foo|bar;baz");
}

#[test]
fn escaped_pipe_outside_quotes_is_ignored() {
    // `rg foo\|bar` — backslash-escapes the pipe, so it's a literal arg
    // to rg, not a shell pipe. Should be Eligible.
    let r = expect_eligible(r"rg foo\|bar");
    // After shlex, the backslash is removed; the query is `foo|bar`.
    assert_eq!(r.query, "foo|bar");
}

#[test]
fn rg_files_flag_is_unknown_in_mvp_scope() {
    // `--files` (list files rg would search, no query) is out of MVP
    // scope. Pass through as UnknownFlag rather than fabricate a
    // recognized-but-empty intercept.
    assert_eq!(
        expect_pass_through("rg --files"),
        PassThroughReason::UnknownFlag
    );
}

#[test]
fn bare_rg_is_no_query() {
    assert_eq!(expect_pass_through("rg"), PassThroughReason::NoQuery);
}

#[test]
fn unknown_long_flag_passes_through() {
    assert_eq!(
        expect_pass_through("rg --completely-fake-flag query"),
        PassThroughReason::UnknownFlag
    );
}

#[test]
fn unknown_short_flag_passes_through() {
    // `-z` is not in the allow-list. Conservative: pass through.
    assert_eq!(
        expect_pass_through("rg -z query"),
        PassThroughReason::UnknownFlag
    );
}

#[test]
fn empty_command_is_unsupported() {
    assert_eq!(expect_pass_through(""), PassThroughReason::UnsupportedShape);
    assert_eq!(
        expect_pass_through("   "),
        PassThroughReason::UnsupportedShape
    );
}

#[test]
fn ignore_case_short_long_both_set_flag() {
    let short = expect_eligible("rg -i query");
    assert!(short.flags.ignore_case);
    let long = expect_eligible("rg --ignore-case query");
    assert!(long.flags.ignore_case);
}

#[test]
fn clustered_short_flags() {
    let r = expect_eligible("rg -iwn query");
    assert!(r.flags.ignore_case);
    assert!(r.flags.word_regexp);
    // -n is formatting-only; flag state doesn't need to mirror it.
    assert_eq!(r.query, "query");
}

#[test]
fn max_count_short_attached() {
    let r = expect_eligible("rg -m20 query");
    assert_eq!(r.flags.max_count, Some(20));
}

#[test]
fn max_count_short_separate() {
    let r = expect_eligible("rg -m 20 query");
    assert_eq!(r.flags.max_count, Some(20));
}

#[test]
fn max_count_long_equals() {
    let r = expect_eligible("rg --max-count=20 query");
    assert_eq!(r.flags.max_count, Some(20));
}

#[test]
fn max_count_long_separate() {
    let r = expect_eligible("rg --max-count 20 query");
    assert_eq!(r.flags.max_count, Some(20));
}

#[test]
fn context_flags_parse() {
    let r = expect_eligible("rg -A 3 -B 2 -C 5 query");
    assert_eq!(r.flags.context_after, Some(3));
    assert_eq!(r.flags.context_before, Some(2));
    assert_eq!(r.flags.context_around, Some(5));
}

#[test]
fn type_filter_short() {
    let r = expect_eligible("rg -t rust query");
    assert_eq!(r.flags.type_filters, vec!["rust".to_string()]);
}

#[test]
fn type_filter_long_equals() {
    let r = expect_eligible("rg --type=rust query");
    assert_eq!(r.flags.type_filters, vec!["rust".to_string()]);
}

#[test]
fn type_filter_repeated() {
    let r = expect_eligible("rg -t rust -t python query");
    assert_eq!(
        r.flags.type_filters,
        vec!["rust".to_string(), "python".to_string()]
    );
}

#[test]
fn glob_filter_short() {
    let r = expect_eligible("rg -g *.rs query");
    assert_eq!(r.flags.glob_filters, vec!["*.rs".to_string()]);
}

#[test]
fn files_only_flag() {
    let r = expect_eligible("rg -l query");
    assert!(r.flags.files_only);
}

#[test]
fn count_only_flag() {
    let r = expect_eligible("rg -c query");
    assert!(r.flags.count_only);
}

#[test]
fn formatting_flags_are_accepted() {
    // -n / --no-heading / --color=never should NOT cause UnknownFlag.
    let r1 = expect_eligible("rg -n query");
    let r2 = expect_eligible("rg --no-heading query");
    let r3 = expect_eligible("rg --color=never query");
    assert_eq!(r1.query, "query");
    assert_eq!(r2.query, "query");
    assert_eq!(r3.query, "query");
}

#[test]
fn double_dash_terminator_yields_positional_args() {
    // `rg -- --weird-looking-query .` — after `--`, anything is
    // positional, even if it looks like a flag.
    let r = expect_eligible("rg -- --weird-looking-query .");
    assert_eq!(r.query, "--weird-looking-query");
    assert_eq!(r.target_paths, vec![".".to_string()]);
}

#[test]
fn normalized_is_order_independent_for_flags() {
    let a = expect_eligible("rg -i -w query");
    let b = expect_eligible("rg -w -i query");
    assert_eq!(a.normalized, b.normalized);
}

#[test]
fn normalized_is_order_independent_for_target_paths() {
    let a = expect_eligible("rg query path2 path1");
    let b = expect_eligible("rg query path1 path2");
    assert_eq!(a.normalized, b.normalized);
}

#[test]
fn normalized_collapses_short_and_long_flags() {
    let a = expect_eligible("rg -i query");
    let b = expect_eligible("rg --ignore-case query");
    assert_eq!(a.normalized, b.normalized);
}

#[test]
fn normalized_collapses_attached_and_separate_value_flags() {
    let a = expect_eligible("rg -m20 query");
    let b = expect_eligible("rg -m 20 query");
    let c = expect_eligible("rg --max-count=20 query");
    let d = expect_eligible("rg --max-count 20 query");
    assert_eq!(a.normalized, b.normalized);
    assert_eq!(b.normalized, c.normalized);
    assert_eq!(c.normalized, d.normalized);
}

#[test]
fn normalized_distinguishes_different_queries() {
    let a = expect_eligible("rg AgentEvalResult");
    let b = expect_eligible("rg classify_result");
    assert_ne!(a.normalized, b.normalized);
}

#[test]
fn normalized_distinguishes_different_targets() {
    let a = expect_eligible("rg query core/src");
    let b = expect_eligible("rg query context-harness");
    assert_ne!(a.normalized, b.normalized);
}

#[test]
fn normalized_distinguishes_different_max_counts() {
    let a = expect_eligible("rg -m 10 query");
    let b = expect_eligible("rg -m 20 query");
    assert_ne!(a.normalized, b.normalized);
}

#[test]
/// C4 stress: classifying the same simple `rg` shape 10k times must not
/// drift in semantics and must not allocate unboundedly. The point isn't
/// throughput — it's that the classifier is a pure, stateless function
/// of its input string and can be re-entered safely under model traffic.
#[test]
fn classifier_is_stable_under_repeated_invocation() {
    let cmd = r#"rg -S "AgentEvalResult|classify_result" context-harness/src"#;
    let baseline = expect_eligible(cmd);
    for _ in 0..10_000 {
        let again = expect_eligible(cmd);
        assert_eq!(again.normalized, baseline.normalized);
        assert_eq!(again.query, baseline.query);
        assert_eq!(again.target_paths, baseline.target_paths);
    }
}

/// C4 weird-shape: a long alternation query (50 branches) must classify
/// cleanly without overflow or panic. Models occasionally synthesize
/// these when grasping at a concept; the classifier must not be where
/// the model gets surprised.
#[test]
fn long_alternation_query_classifies_cleanly() {
    let branches: Vec<String> = (0..50).map(|i| format!("branch_{i}")).collect();
    let pattern = branches.join("|");
    let cmd = format!(r#"rg -S "{pattern}" ."#);
    let r = expect_eligible(&cmd);
    assert!(r.query.contains("branch_0"));
    assert!(r.query.contains("branch_49"));
    assert!(r.normalized.starts_with("rg "));
}

/// C4 weird-shape: a regex containing escaped backslashes (`\\b` for
/// word boundary, `\\.` for a literal dot) survives shlex unescaping and
/// remains classified as eligible. The query body is what `rg` actually
/// receives — its exact form may vary by quoting path but the shape stays
/// a single-rg invocation.
#[test]
fn regex_with_escaped_backslashes_is_eligible() {
    // Single-quoted in the shell so the backslashes reach rg literally.
    let cmd = r"rg '\bAgents?\.md\b' core/src";
    let r = expect_eligible(cmd);
    // The query passed through shlex single-quote handling — must contain
    // the meaningful regex tokens.
    assert!(r.query.contains("Agents"));
    assert!(r.query.contains(".md"));
    assert_eq!(r.target_paths, vec!["core/src".to_string()]);
}

fn run8_actual_command_is_eligible_and_normalizes_stably() {
    // Verbatim form of the first rg command Run 8's vanilla arm fired,
    // after wrapping. Verifies the wrapper strip + quote-aware
    // metacharacter scanner + flag parser all line up.
    let cmd1 = r#"/bin/zsh -lc "rg -n \"AgentEvalResult\" -S ."#.to_string() + "\"";
    let cmd2 = "/bin/zsh -lc 'rg -nS \"AgentEvalResult\" .'";
    let cmd3 = "rg \"AgentEvalResult\" -S .";
    let a = expect_eligible(&cmd1);
    let b = expect_eligible(cmd2);
    let c = expect_eligible(cmd3);
    assert_eq!(a.query, "AgentEvalResult");
    assert_eq!(a.normalized, b.normalized);
    assert_eq!(b.normalized, c.normalized);
    assert!(a.flags.smart_case);
    assert_eq!(a.target_paths, vec![".".to_string()]);
}
