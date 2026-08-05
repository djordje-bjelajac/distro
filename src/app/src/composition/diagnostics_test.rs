//! The two external-address lists, which are the only part of [`Diagnostics`]
//! that decides anything at all — whether a confirmation answers something the
//! operator asked for (D6).
//!
//! The counters above them are `fetch_add` and are not worth a test; this is.

use crate::composition::Diagnostics;

#[test]
fn a_launch_that_asserted_nothing_reports_nothing() {
    let diagnostics = Diagnostics::default();

    assert!(
        diagnostics.external_addresses_supplied().is_empty(),
        "the ordinary launch passes no --external-address and must not look \
         like one that did"
    );
    assert!(diagnostics.external_addresses_in_effect().is_empty());
}

#[test]
fn a_supplied_address_is_reported_as_supplied_before_anything_confirms_it() {
    // The whole of D6 in one assertion: these are two facts from two sources,
    // and the interesting failure — the flag was set and did not take — is the
    // state where the first is non-empty and the second is not.
    let diagnostics = Diagnostics::default();

    diagnostics.record_supplied_external_addresses(&["/ip4/203.0.113.7/tcp/4001".to_owned()]);

    assert_eq!(
        diagnostics.external_addresses_supplied(),
        vec!["/ip4/203.0.113.7/tcp/4001".to_owned()]
    );
    assert!(
        diagnostics.external_addresses_in_effect().is_empty(),
        "supplying an address must not report itself as having taken effect — \
         that is the whole distinction (D6, S4)"
    );
}

#[test]
fn several_supplied_addresses_are_reported_in_the_order_they_were_given() {
    let diagnostics = Diagnostics::default();

    diagnostics.record_supplied_external_addresses(&[
        "/ip4/203.0.113.7/tcp/4001".to_owned(),
        "/ip4/203.0.113.8/udp/4001/quic-v1".to_owned(),
    ]);

    assert_eq!(
        diagnostics.external_addresses_supplied(),
        vec![
            "/ip4/203.0.113.7/tcp/4001".to_owned(),
            "/ip4/203.0.113.8/udp/4001/quic-v1".to_owned(),
        ]
    );
}

#[test]
fn a_confirmed_supplied_address_is_reported_as_in_effect() {
    let diagnostics = Diagnostics::default();
    diagnostics.record_supplied_external_addresses(&["/ip4/203.0.113.7/tcp/4001".to_owned()]);

    diagnostics.record_confirmed_external_address("/ip4/203.0.113.7/tcp/4001");

    assert_eq!(
        diagnostics.external_addresses_in_effect(),
        vec!["/ip4/203.0.113.7/tcp/4001".to_owned()]
    );
    assert_eq!(
        diagnostics.external_addresses_supplied(),
        vec!["/ip4/203.0.113.7/tcp/4001".to_owned()],
        "taking effect does not change what was asked for"
    );
}

#[test]
fn the_same_confirmation_twice_is_reported_once() {
    let diagnostics = Diagnostics::default();
    diagnostics.record_supplied_external_addresses(&["/ip4/203.0.113.7/tcp/4001".to_owned()]);

    diagnostics.record_confirmed_external_address("/ip4/203.0.113.7/tcp/4001");
    diagnostics.record_confirmed_external_address("/ip4/203.0.113.7/tcp/4001");

    assert_eq!(diagnostics.external_addresses_in_effect().len(), 1);
}

#[test]
fn a_confirmation_nobody_asked_for_is_not_an_override_in_effect() {
    // Piece 1's corroborated observation and piece 2's probe confirm addresses
    // through the same event. Reporting one of those as an override would say
    // "the flag took effect" on a launch that passed no flag.
    let diagnostics = Diagnostics::default();

    diagnostics.record_confirmed_external_address("/ip4/203.0.113.7/tcp/4001");

    assert!(diagnostics.external_addresses_in_effect().is_empty());
    assert!(diagnostics.external_addresses_supplied().is_empty());
}

#[test]
fn one_supplied_address_taking_effect_says_nothing_about_another() {
    // Two forwarded ports, one of which is wrong. The report has to show the
    // half that worked without covering for the half that did not.
    let diagnostics = Diagnostics::default();
    diagnostics.record_supplied_external_addresses(&[
        "/ip4/203.0.113.7/tcp/4001".to_owned(),
        "/ip4/203.0.113.8/tcp/4001".to_owned(),
    ]);

    diagnostics.record_confirmed_external_address("/ip4/203.0.113.8/tcp/4001");

    assert_eq!(diagnostics.external_addresses_supplied().len(), 2);
    assert_eq!(
        diagnostics.external_addresses_in_effect(),
        vec!["/ip4/203.0.113.8/tcp/4001".to_owned()]
    );
}

#[test]
fn a_supplied_address_is_matched_by_what_is_advertised_rather_than_by_what_was_typed() {
    // libp2p renders an address in its own canonical form, and that is what the
    // confirmation carries. Comparing spellings verbatim would report an
    // override that did take effect as one that did not — a false alarm on the
    // one screen a user consults when nothing else has worked.
    let diagnostics = Diagnostics::default();
    diagnostics
        .record_supplied_external_addresses(&["/ip6/2001:db8:0:0:0:0:0:1/tcp/4001".to_owned()]);

    diagnostics.record_confirmed_external_address("/ip6/2001:db8::1/tcp/4001");

    assert_eq!(
        diagnostics.external_addresses_in_effect(),
        vec!["/ip6/2001:db8::1/tcp/4001".to_owned()]
    );
}
