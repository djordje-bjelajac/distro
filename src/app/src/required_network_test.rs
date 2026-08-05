use crate::required_network::Verdict;

// The escalation is checked here rather than in the node smoke tests, because
// on a machine with networking those never take the skip branch at all — so the
// only thing that would ever exercise it is the machine where it is broken.
// `Verdict` is pure, so the decision can be asserted without setting a process
// environment variable out from under a parallel test.

#[test]
fn an_unset_variable_allows_the_skip() {
    assert_eq!(Verdict::of(None), Verdict::Skip);
}

#[test]
fn an_empty_or_zero_value_allows_the_skip() {
    // A shell that exports the variable empty must not turn every sandbox red,
    // and `=0` has to mean what a reader expects it to mean.
    assert_eq!(Verdict::of(Some("")), Verdict::Skip);
    assert_eq!(Verdict::of(Some("0")), Verdict::Skip);
    assert_eq!(Verdict::of(Some("   ")), Verdict::Skip);
}

#[test]
fn any_other_value_refuses_the_skip() {
    for setting in ["1", "true", "yes", "on", " 1 "] {
        assert_eq!(
            Verdict::of(Some(setting)),
            Verdict::Fail,
            "`{setting}` must be taken as a promise that this machine has networking"
        );
    }
}
