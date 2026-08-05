use crate::format::hex_bytes::{self, KEY_BYTES};

#[test]
fn round_trips_every_byte_value() {
    let mut bytes = [0u8; KEY_BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(7).wrapping_add(3);
    }

    let text = hex_bytes::encode(&bytes);

    assert_eq!(text.len(), KEY_BYTES * 2);
    assert_eq!(hex_bytes::decode(&text), Some(bytes));
}

#[test]
fn renders_lowercase() {
    let bytes = [0xabu8; KEY_BYTES];

    assert_eq!(hex_bytes::encode(&bytes), "ab".repeat(KEY_BYTES));
}

#[test]
fn accepts_uppercase_on_the_way_in() {
    let bytes = [0xcdu8; KEY_BYTES];

    assert_eq!(hex_bytes::decode(&"CD".repeat(KEY_BYTES)), Some(bytes));
}

#[test]
fn rejects_the_wrong_length() {
    assert_eq!(hex_bytes::decode(&"ab".repeat(KEY_BYTES - 1)), None);
    assert_eq!(hex_bytes::decode(&"ab".repeat(KEY_BYTES + 1)), None);
    assert_eq!(hex_bytes::decode(""), None);
}

#[test]
fn rejects_a_non_hex_character() {
    let mut text = "00".repeat(KEY_BYTES);
    text.replace_range(5..6, "z");

    assert_eq!(hex_bytes::decode(&text), None);
}

#[test]
fn rejects_multi_byte_characters_without_panicking() {
    // A multi-byte scalar makes the byte length right and the character count
    // wrong; slicing by bytes would panic on the boundary.
    let text = format!("é{}", "0".repeat(KEY_BYTES * 2 - 2));

    assert_eq!(hex_bytes::decode(&text), None);
}
