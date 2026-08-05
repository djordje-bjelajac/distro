use crate::domain::{DeliveryFailure, DeliveryState, DeliveryStateError};

const REASON: DeliveryFailure = DeliveryFailure::NoRelayAvailable;

// ------------------------------------------------- the direct-message path

#[test]
fn a_direct_message_starts_pending() {
    let state = DeliveryState::Pending;

    assert!(state.is_pending());
    assert!(!state.is_terminal());
    assert_eq!(state.failure_reason(), None);
}

#[test]
fn a_pending_message_becomes_delivered() {
    assert_eq!(
        DeliveryState::Pending.mark_delivered(),
        Ok(DeliveryState::Delivered)
    );
}

#[test]
fn a_pending_message_becomes_failed_with_a_stated_reason() {
    // AC11/D10: silent loss is not a state — a failure always names its cause.
    let failed = DeliveryState::Pending
        .mark_failed(REASON)
        .expect("pending may fail");

    assert_eq!(failed, DeliveryState::Failed(REASON));
    assert_eq!(failed.failure_reason(), Some(REASON));
    assert!(failed.is_terminal());
}

#[test]
fn every_failure_reason_is_reachable_from_pending_and_kept_verbatim() {
    for reason in DeliveryFailure::ALL {
        assert_eq!(
            DeliveryState::Pending.mark_failed(reason),
            Ok(DeliveryState::Failed(reason))
        );
    }
}

// ------------------------------------------------------- terminal states

#[test]
fn the_transition_table_rejects_every_move_out_of_a_terminal_state() {
    let table = [
        (DeliveryState::Delivered, DeliveryState::Delivered),
        (DeliveryState::Failed(REASON), DeliveryState::Delivered),
        (DeliveryState::Published, DeliveryState::Delivered),
    ];

    for (from, to) in table {
        assert_eq!(
            from.mark_delivered(),
            Err(DeliveryStateError::InvalidTransition { from, to })
        );
    }

    for from in [
        DeliveryState::Delivered,
        DeliveryState::Failed(REASON),
        DeliveryState::Published,
    ] {
        assert_eq!(
            from.mark_failed(REASON),
            Err(DeliveryStateError::InvalidTransition {
                from,
                to: DeliveryState::Failed(REASON),
            })
        );
    }
}

#[test]
fn a_failed_message_cannot_be_re_failed_with_a_different_reason() {
    let failed = DeliveryState::Failed(DeliveryFailure::PeerUnreachable);

    assert_eq!(
        failed.mark_failed(DeliveryFailure::SessionClosed),
        Err(DeliveryStateError::InvalidTransition {
            from: failed,
            to: DeliveryState::Failed(DeliveryFailure::SessionClosed),
        })
    );
}

// ------------------------------------------------------- the broadcast path

#[test]
fn a_broadcast_message_is_published_and_stays_published() {
    let state = DeliveryState::Published;

    assert!(!state.is_pending());
    assert!(state.is_terminal());
    assert_eq!(state.failure_reason(), None);
    assert!(state.mark_delivered().is_err());
}

// ------------------------------------------------------------- rendering

#[test]
fn states_render_for_the_user_interface() {
    assert_eq!(DeliveryState::Pending.to_string(), "pending");
    assert_eq!(DeliveryState::Delivered.to_string(), "delivered");
    assert_eq!(DeliveryState::Published.to_string(), "published");
    assert_eq!(
        DeliveryState::Failed(DeliveryFailure::NoRelayAvailable).to_string(),
        "failed: no peer is available to relay to the recipient"
    );
}

#[test]
fn a_rejected_transition_names_both_ends() {
    let error = DeliveryStateError::InvalidTransition {
        from: DeliveryState::Delivered,
        to: DeliveryState::Delivered,
    };

    assert_eq!(
        error.to_string(),
        "delivery state cannot move from delivered to delivered"
    );
}
