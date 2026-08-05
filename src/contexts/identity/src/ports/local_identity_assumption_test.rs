use crate::domain::DisplayName;
use crate::domain::events::LocalIdentityInitialized;
use crate::ports::LocalIdentityAssumption;
use crate::test_peers;

fn assumed() -> LocalIdentityAssumption {
    LocalIdentityAssumption::Assumed(LocalIdentityInitialized {
        peer: test_peers::alice(),
        display_name: DisplayName::derived_from(&test_peers::alice()),
    })
}

#[test]
fn both_outcomes_agree_on_the_peer_that_was_assumed() {
    assert_eq!(assumed().peer(), test_peers::alice());
    assert_eq!(
        LocalIdentityAssumption::AlreadyAssumed(test_peers::alice()).peer(),
        test_peers::alice()
    );
}

#[test]
fn only_the_first_assumption_carries_an_event() {
    assert!(assumed().event().is_some());
    assert_eq!(
        LocalIdentityAssumption::AlreadyAssumed(test_peers::alice()).event(),
        None,
        "re-issuing an idempotent bootstrap announces nothing a second time"
    );
}
