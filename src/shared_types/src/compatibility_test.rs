use crate::{Compatibility, ProtocolVersion};

fn evaluate(
    (received_major, received_minor): (u16, u16),
    (supported_major, supported_minor): (u16, u16),
) -> Compatibility {
    Compatibility::evaluate(
        ProtocolVersion::new(received_major, received_minor),
        ProtocolVersion::new(supported_major, supported_minor),
    )
}

/// One truth-table row: (received, supported) → expected decision.
type Row = ((u16, u16), (u16, u16), Compatibility);

/// Full truth table for the S2 wire-compatibility rule (drives AC14).
#[test]
fn truth_table() {
    let table: &[Row] = &[
        // Same major, same minor → Accept.
        ((1, 0), (1, 0), Compatibility::Accept),
        ((1, 5), (1, 5), Compatibility::Accept),
        // Same major, received minor lower → Accept.
        ((1, 0), (1, 1), Compatibility::Accept),
        ((1, 3), (1, 9), Compatibility::Accept),
        // Same major, received minor higher → Tolerate (ignore unknown
        // fields/kinds, count in local diagnostics).
        ((1, 1), (1, 0), Compatibility::Tolerate),
        ((1, 9), (1, 3), Compatibility::Tolerate),
        // Different major → Reject, in both directions, regardless of minor.
        ((2, 0), (1, 0), Compatibility::Reject),
        ((0, 0), (1, 0), Compatibility::Reject),
        ((2, 0), (1, 9), Compatibility::Reject),
        ((1, 9), (2, 0), Compatibility::Reject),
        ((3, 0), (1, 0), Compatibility::Reject),
    ];

    for (received, supported, expected) in table {
        assert_eq!(
            evaluate(*received, *supported),
            *expected,
            "received {received:?} vs supported {supported:?}",
        );
    }
}

#[test]
fn current_version_accepts_itself() {
    assert_eq!(
        Compatibility::evaluate(ProtocolVersion::CURRENT, ProtocolVersion::CURRENT),
        Compatibility::Accept
    );
}
