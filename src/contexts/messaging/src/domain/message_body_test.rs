use crate::domain::{MessageBody, MessageBodyError};

/// A four-byte scalar, so byte length and character count cannot be confused.
const FOUR_BYTE_CHAR: char = '𝄞';

fn body_of_bytes(bytes: usize) -> String {
    "a".repeat(bytes)
}

#[test]
fn a_body_keeps_its_text() {
    let body = MessageBody::new("hello").expect("non-empty text");

    assert_eq!(body.as_str(), "hello");
    assert_eq!(body.len_bytes(), 5);
}

#[test]
fn surrounding_whitespace_is_trimmed_before_anything_else() {
    let body = MessageBody::new("  hello \n").expect("non-empty after trim");

    assert_eq!(body.as_str(), "hello");
}

#[test]
fn an_empty_body_is_rejected() {
    assert_eq!(MessageBody::new(""), Err(MessageBodyError::Empty));
}

#[test]
fn a_whitespace_only_body_is_rejected_because_trimming_comes_first() {
    for text in [" ", "\t\n", "   \r\n  "] {
        assert_eq!(MessageBody::new(text), Err(MessageBodyError::Empty));
    }
}

#[test]
fn a_single_character_is_a_valid_body() {
    assert_eq!(
        MessageBody::new("x").map(|body| body.len_bytes()),
        Ok(MessageBody::MIN_BYTES)
    );
}

#[test]
fn a_body_of_exactly_the_maximum_size_is_accepted() {
    let text = body_of_bytes(MessageBody::MAX_BYTES);

    assert_eq!(
        MessageBody::new(&text).map(|body| body.len_bytes()),
        Ok(MessageBody::MAX_BYTES)
    );
}

#[test]
fn one_byte_over_the_maximum_is_rejected() {
    let text = body_of_bytes(MessageBody::MAX_BYTES + 1);

    assert_eq!(
        MessageBody::new(&text),
        Err(MessageBodyError::TooLong {
            bytes: MessageBody::MAX_BYTES + 1
        })
    );
}

#[test]
fn the_limit_counts_bytes_not_characters() {
    // 4096 four-byte scalars are 16384 bytes: at the limit, though only a
    // quarter of the characters a same-sized ASCII body would hold.
    let at_limit: String =
        std::iter::repeat_n(FOUR_BYTE_CHAR, MessageBody::MAX_BYTES / 4).collect();
    assert_eq!(at_limit.len(), MessageBody::MAX_BYTES);
    assert_eq!(
        MessageBody::new(&at_limit).map(|body| body.len_bytes()),
        Ok(MessageBody::MAX_BYTES)
    );

    let over_limit = format!("{at_limit}{FOUR_BYTE_CHAR}");
    assert_eq!(
        MessageBody::new(&over_limit),
        Err(MessageBodyError::TooLong {
            bytes: MessageBody::MAX_BYTES + 4
        })
    );
}

#[test]
fn an_oversized_body_becomes_acceptable_once_its_padding_is_trimmed() {
    // Trimming happens before measuring, so whitespace never costs a sender
    // their message.
    let padded = format!("  {}  ", body_of_bytes(MessageBody::MAX_BYTES));

    assert_eq!(
        MessageBody::new(&padded).map(|body| body.len_bytes()),
        Ok(MessageBody::MAX_BYTES)
    );
}

#[test]
fn interior_whitespace_and_newlines_survive() {
    let body = MessageBody::new(" line one\nline two ").expect("non-empty");

    assert_eq!(body.as_str(), "line one\nline two");
}

#[test]
fn errors_render_their_cause() {
    assert_eq!(
        MessageBodyError::Empty.to_string(),
        "a message body is empty once trimmed"
    );
    assert_eq!(
        MessageBodyError::TooLong { bytes: 20_000 }.to_string(),
        "a message body of 20000 bytes exceeds the 16384-byte limit"
    );
}
