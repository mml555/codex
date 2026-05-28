use crate::PassThroughReason;
use crate::ReadDecision;
use crate::decide_read;
use std::collections::HashSet;

#[test]
fn first_read_substitutable_then_repeat_bypasses() {
    let mut reg = HashSet::new();
    let normalized = match decide_read("cat core/src/lib.rs", &reg) {
        ReadDecision::Substitutable(c) => c.normalized,
        other => panic!("expected Substitutable, got {other:?}"),
    };
    // The caller registers the normalized key only after a successful
    // substitution; simulate that here.
    reg.insert(normalized);
    assert!(matches!(
        decide_read("cat core/src/lib.rs", &reg),
        ReadDecision::Bypass { .. }
    ));
}

#[test]
fn non_read_and_small_range_pass_through() {
    let reg = HashSet::new();
    assert!(matches!(
        decide_read("rg foo", &reg),
        ReadDecision::PassThrough(PassThroughReason::NotReadCommand)
    ));
    assert!(matches!(
        decide_read("sed -n '1,20p' f.rs", &reg),
        ReadDecision::PassThrough(PassThroughReason::SmallRange)
    ));
}

#[test]
fn small_file_passthrough_does_not_pre_register() {
    // A read that classifies eligible but whose file turns out small is
    // registered by the caller ONLY on a real substitution, so the registry
    // stays empty and a later (now-large) read of the same path can still be
    // substituted. decide_read itself never mutates the registry.
    let reg = HashSet::new();
    assert!(matches!(
        decide_read("cat core/src/lib.rs", &reg),
        ReadDecision::Substitutable(_)
    ));
    assert!(reg.is_empty());
}
