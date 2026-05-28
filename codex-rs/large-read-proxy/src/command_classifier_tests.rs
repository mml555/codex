use crate::ClassifiedRead;
use crate::ClassifyOutcome;
use crate::PassThroughReason;
use crate::ReadTool;
use crate::classify_command;
use pretty_assertions::assert_eq;

fn eligible(cmd: &str) -> ClassifiedRead {
    match classify_command(cmd) {
        ClassifyOutcome::Eligible(c) => c,
        other => panic!("expected Eligible for {cmd:?}, got {other:?}"),
    }
}

fn passthrough(cmd: &str) -> PassThroughReason {
    match classify_command(cmd) {
        ClassifyOutcome::PassThrough(r) => r,
        other => panic!("expected PassThrough for {cmd:?}, got {other:?}"),
    }
}

#[test]
fn cat_single_file_is_eligible() {
    let c = eligible("cat context-harness/src/agent_eval.rs");
    assert_eq!(c.tool, ReadTool::Cat);
    assert_eq!(c.path, "context-harness/src/agent_eval.rs");
    assert_eq!(c.requested_range, None);
    assert_eq!(c.normalized, "cat context-harness/src/agent_eval.rs");
}

#[test]
fn sed_large_range_is_eligible() {
    let c = eligible("sed -n '1,400p' core/src/lib.rs");
    assert_eq!(c.tool, ReadTool::Sed);
    assert_eq!(c.requested_range, Some((1, 400)));
    assert_eq!(c.normalized, "sed -n 1,400p core/src/lib.rs");
}

#[test]
fn sed_whole_file_dollar_is_eligible() {
    let c = eligible("sed -n '1,$p' core/src/lib.rs");
    assert_eq!(c.requested_range, Some((1, u32::MAX)));
}

#[test]
fn sed_small_range_passes_through() {
    assert_eq!(
        passthrough("sed -n '1,40p' f.rs"),
        PassThroughReason::SmallRange
    );
}

#[test]
fn cat_with_flag_or_multiple_files_passes_through() {
    assert_eq!(
        passthrough("cat -n f.rs"),
        PassThroughReason::UnsupportedArgs
    );
    assert_eq!(
        passthrough("cat a.rs b.rs"),
        PassThroughReason::UnsupportedArgs
    );
}

#[test]
fn chained_or_piped_passes_through() {
    assert_eq!(
        passthrough("cat f.rs | head"),
        PassThroughReason::ShellMetacharacter
    );
    assert_eq!(
        passthrough("cat f.rs && echo done"),
        PassThroughReason::ShellMetacharacter
    );
}

#[test]
fn non_read_command_passes_through() {
    assert_eq!(
        passthrough("rg AgentEvalResult"),
        PassThroughReason::NotReadCommand
    );
    assert_eq!(passthrough("just test"), PassThroughReason::NotReadCommand);
}

#[test]
fn shell_wrapper_is_unwrapped() {
    let c = eligible("bash -lc \"cat core/src/lib.rs\"");
    assert_eq!(c.tool, ReadTool::Cat);
    assert_eq!(c.path, "core/src/lib.rs");
}

#[test]
fn empty_is_unsupported() {
    assert_eq!(passthrough("   "), PassThroughReason::UnsupportedShape);
}

/// C4 edge case: `sed -n '<N>,<M>p'` with a non-1-based start is still
/// a paging read — eligibility depends on the span (`M-N+1 >= 200`), not
/// the starting line. The model commonly issues these when zoom-paging
/// into a known region. Was implicitly covered by the eligible-large-range
/// test using `1,300p`; this nails down the non-1-based start case.
#[test]
fn sed_non_one_based_start_with_large_span_is_eligible() {
    let c = eligible("sed -n '100,400p' foo.rs");
    assert_eq!(c.tool, ReadTool::Sed);
    assert_eq!(c.path, "foo.rs");
}

/// C4 edge case: a quoted file path is unwrapped by shlex and the proxy
/// classifies the read against the unquoted path. The escape-hatch
/// registry uses the normalized form so a re-issue under different quoting
/// still hits the bypass.
#[test]
fn quoted_path_is_unwrapped_and_normalizes_stably() {
    let a = eligible(r#"cat "core/src/lib.rs""#);
    let b = eligible("cat core/src/lib.rs");
    assert_eq!(a.path, "core/src/lib.rs");
    assert_eq!(a.normalized, b.normalized);
}

/// C4 stress: classifying the same simple `cat <file>` 10k times must not
/// drift in semantics. Pure-function property; mirrors the equivalent SP
/// stress test.
#[test]
fn classifier_is_stable_under_repeated_invocation() {
    let cmd = "cat rollout/src/recorder.rs";
    let baseline = eligible(cmd);
    for _ in 0..10_000 {
        let again = eligible(cmd);
        assert_eq!(again.normalized, baseline.normalized);
        assert_eq!(again.path, baseline.path);
    }
}
