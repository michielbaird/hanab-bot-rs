//! Port of `test/lib/CertainMap.scala`.

use hanab_bot::collections::{CertainMapEntry, FastBitSet};

#[test]
fn basic_operations() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(10, Some(2));

    assert_eq!(entry.len(), 1);

    assert!(entry.contains_order(10));
    assert!(entry.contains(9, Some(2)));

    assert!(!entry.contains_order(2));
    assert!(!entry.contains(2, Some(1)));
}

#[test]
fn overwrites_with_the_same_order() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(10, Some(2));
    entry.update(10, None);

    assert!(!entry.contains_order(9));
    assert_eq!(entry.len(), 1);
}

#[test]
fn adds_a_new_slot_with_a_different_order() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(10, None);
    entry.update(7, None);

    assert!(entry.contains_order(10));
    assert!(entry.contains_order(7));
    assert_eq!(entry.len(), 2);
}

#[test]
fn manages_three_slots() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(10, Some(1));
    entry.update(7, Some(2));

    assert_eq!(entry.len(), 2);

    entry.update(10, None);

    assert_eq!(entry.len(), 2);

    entry.update(3, None);

    assert_eq!(entry.len(), 3);

    entry.update(7, None);

    assert_eq!(entry.len(), 3);
    assert!(entry.contains_order(10));
    assert!(entry.contains_order(7));
    assert!(entry.contains_order(3));
}

#[test]
fn converts_to_a_fast_bit_set() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(10, Some(1));
    entry.update(7, Some(2));

    assert_eq!(entry.orders(), FastBitSet::from([10, 7]));
}

// Beyond what the Scala tests cover.

#[test]
fn round_trips_every_slot_field() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(63, Some(5));
    entry.update(0, None);
    entry.update(31, Some(0));

    assert_eq!(
        entry.iter().collect::<Vec<_>>(),
        [(63, Some(5)), (0, None), (31, Some(0))]
    );
    assert_eq!(entry.to_string(), "CertainMapEntry(63/5, 0/-1, 31/0)");
}

#[test]
fn overwriting_one_slot_leaves_the_others() {
    let mut entry = CertainMapEntry::EMPTY;
    entry.update(1, Some(0));
    entry.update(2, Some(1));
    entry.update(3, Some(2));
    entry.update(2, None);

    assert_eq!(
        entry.iter().collect::<Vec<_>>(),
        [(1, Some(0)), (2, None), (3, Some(2))]
    );
}

#[test]
fn holds_more_than_three_slots() {
    let mut entry = CertainMapEntry::EMPTY;
    for order in 0..6 {
        entry.update(order, Some(order % 6));
    }
    assert_eq!(entry.len(), 6);
    assert_eq!(entry.orders(), FastBitSet::from([0, 1, 2, 3, 4, 5]));
}

#[test]
#[should_panic(expected = "full")]
fn panics_rather_than_corrupting_when_full() {
    let mut entry = CertainMapEntry::EMPTY;
    for order in 0..7 {
        entry.update(order, None);
    }
}
