use super::*;

#[test]
fn insertion_index_on_an_empty_slice_is_zero() {
    assert_eq!(insertion_index(&[], 7), 0);
}

#[test]
fn insertion_index_after_every_lower_rank() {
    assert_eq!(insertion_index(&[0, 1, 2], 7), 3);
}

#[test]
fn insertion_index_between_a_lower_and_a_higher_rank() {
    assert_eq!(insertion_index(&[0, 9], 7), 1);
}

#[test]
fn insertion_index_of_a_duplicate_rank_precedes_the_existing_entry() {
    // A duplicate rank must not silently swap places with what is already
    // there: it always resolves to the same side (before), never an
    // arbitrary one that would depend on call order.
    assert_eq!(insertion_index(&[7], 7), 0);
}

#[test]
fn insertion_index_replays_shuffled_removal_and_reinsertion_in_sorted_order() {
    // Property: for any subset of 0..15, feeding its members through
    // `insertion_index` in any order reproduces the sorted subset with a
    // strictly increasing rank sequence. This is the guarantee the whole
    // rank-preserving scheme in `StatusBar` depends on -- an item removed
    // and later reinserted at its original rank must land back in the same
    // slot no matter what else was toggled in between.
    let shuffles: [&[usize]; 5] = [
        &[3, 1, 4, 0, 2],
        &[14, 0, 7, 13, 1, 6],
        &[5, 12, 2, 9],
        &[10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
        &[0, 14, 1, 13, 2, 12, 3, 11],
    ];

    for shuffle in shuffles {
        let mut ranks: Vec<usize> = Vec::new();
        for &rank in shuffle {
            let index = insertion_index(&ranks, rank);
            ranks.insert(index, rank);
        }

        let mut expected = shuffle.to_vec();
        expected.sort_unstable();
        assert_eq!(ranks, expected);

        for pair in ranks.windows(2) {
            assert!(
                pair[0] < pair[1],
                "rank sequence must stay strictly increasing, got {ranks:?}"
            );
        }
    }
}
