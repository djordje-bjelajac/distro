use crate::rng::SeededRng;

#[test]
fn the_same_seed_produces_the_same_sequence() {
    let mut first = SeededRng::from_seed(0xDEAD_BEEF);
    let mut second = SeededRng::from_seed(0xDEAD_BEEF);

    let left: Vec<u64> = (0..64).map(|_| first.next_u64()).collect();
    let right: Vec<u64> = (0..64).map(|_| second.next_u64()).collect();

    assert_eq!(left, right);
}

#[test]
fn different_seeds_produce_different_sequences() {
    let mut first = SeededRng::from_seed(1);
    let mut second = SeededRng::from_seed(2);

    let left: Vec<u64> = (0..16).map(|_| first.next_u64()).collect();
    let right: Vec<u64> = (0..16).map(|_| second.next_u64()).collect();

    assert_ne!(left, right);
}

#[test]
fn the_sequence_is_pinned_so_a_recorded_trace_keeps_matching() {
    // Pinning the algorithm, not merely its reproducibility: a change to the
    // constants would silently invalidate every trace recorded against this
    // crate, and this test is what makes that a failing build instead.
    let mut rng = SeededRng::from_seed(0);

    assert_eq!(
        [rng.next_u64(), rng.next_u64(), rng.next_u64()],
        [
            0xE220_A839_7B1D_CDAF,
            0x6E78_9E6A_A1B9_65F4,
            0x06C4_5D18_8009_454F,
        ]
    );
}

#[test]
fn a_label_gives_each_named_peer_its_own_stable_stream() {
    let alice = SeededRng::for_label(7, "alice");
    let alice_again = SeededRng::for_label(7, "alice");
    let bob = SeededRng::for_label(7, "bob");
    let alice_other_seed = SeededRng::for_label(8, "alice");

    assert_eq!(alice, alice_again);
    assert_ne!(alice, bob);
    assert_ne!(alice, alice_other_seed);
}

#[test]
fn below_stays_inside_its_bound() {
    let mut rng = SeededRng::from_seed(99);

    for _ in 0..1_000 {
        assert!(rng.below(7) < 7);
    }
}

#[test]
fn below_zero_is_zero_rather_than_a_division_by_zero() {
    let mut rng = SeededRng::from_seed(1);

    assert_eq!(rng.below(0), 0);
}

#[test]
fn below_one_is_always_zero() {
    let mut rng = SeededRng::from_seed(1);

    for _ in 0..32 {
        assert_eq!(rng.below(1), 0);
    }
}

#[test]
fn below_reaches_every_value_in_its_range() {
    let mut rng = SeededRng::from_seed(4);
    let mut seen = [false; 5];

    for _ in 0..500 {
        seen[usize::try_from(rng.below(5)).expect("bounded by 5")] = true;
    }

    assert_eq!(seen, [true; 5]);
}

#[test]
fn filled_bytes_are_reproducible_and_cover_the_whole_slice() {
    let mut first = SeededRng::from_seed(5);
    let mut second = SeededRng::from_seed(5);
    let mut left = [0u8; 33];
    let mut right = [0u8; 33];

    first.fill_bytes(&mut left);
    second.fill_bytes(&mut right);

    assert_eq!(left, right);
    assert!(left.iter().any(|byte| *byte != 0), "slice stayed empty");
    assert_ne!(left[32], 0, "the trailing partial word was not written");
}

#[test]
fn shuffle_is_a_permutation_and_is_reproducible() {
    let mut first = SeededRng::from_seed(11);
    let mut second = SeededRng::from_seed(11);
    let mut left: Vec<u32> = (0..12).collect();
    let mut right: Vec<u32> = (0..12).collect();

    first.shuffle(&mut left);
    second.shuffle(&mut right);

    assert_eq!(left, right);

    let mut sorted = left.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..12).collect::<Vec<u32>>());
    assert_ne!(
        left, sorted,
        "a 12-element shuffle left the order untouched"
    );
}

#[test]
fn shuffling_nothing_or_one_thing_is_a_no_op() {
    let mut rng = SeededRng::from_seed(3);
    let mut empty: [u8; 0] = [];
    let mut single = [42u8];

    rng.shuffle(&mut empty);
    rng.shuffle(&mut single);

    assert_eq!(single, [42]);
}
