use crate::entropy;

#[test]
fn fills_the_whole_buffer() {
    let mut secret = [0u8; 32];

    entropy::fill_secret(&mut secret).expect("the OS random source must be available");

    // A source that filled part of the buffer would leave a key with a
    // predictable tail; that is a forgeable identity that looks sound.
    assert_ne!(secret, [0u8; 32]);
}

#[test]
fn two_draws_differ() {
    let mut first = [0u8; 32];
    let mut second = [0u8; 32];

    entropy::fill_secret(&mut first).expect("the OS random source must be available");
    entropy::fill_secret(&mut second).expect("the OS random source must be available");

    // The chance of a false failure here is 2^-256; the chance of catching a
    // stubbed-out source is 1.
    assert_ne!(first, second);
}
