use crate::EnvelopeSignature;

#[test]
fn wraps_and_exposes_its_sixty_four_bytes() {
    let mut bytes = [0u8; 64];
    bytes[0] = 0xab;
    bytes[63] = 0xcd;

    let signature = EnvelopeSignature::new(bytes);
    assert_eq!(signature.as_bytes(), &bytes);
}

#[test]
fn equality_is_by_signature_bytes() {
    let a = EnvelopeSignature::new([1u8; 64]);
    let b = EnvelopeSignature::new([1u8; 64]);
    let c = EnvelopeSignature::new([2u8; 64]);

    assert_eq!(a, b);
    assert_ne!(a, c);
}
