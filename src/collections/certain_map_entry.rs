use std::fmt;

use super::FastBitSet;

const ORDER_BITS: u32 = 6;
const UNKNOWN_BITS: u32 = 3;
const SLOT_BITS: u32 = 10;
const SIZE_BITS: u32 = 3;

const UNKNOWN_MASK: u64 = (1 << UNKNOWN_BITS) - 1;
const ORDER_MASK: u64 = ((1 << ORDER_BITS) - 1) << UNKNOWN_BITS;
const SIZE_MASK: u64 = (1 << SIZE_BITS) - 1;
const SLOT_MASK: u64 = (1 << SLOT_BITS) - 1;

/// The most slots an entry can hold. The Scala version packed into 32 bits and silently
/// corrupted on a 4th slot; 64 bits leaves room for 6 and we fail loudly beyond that.
pub const MAX_SLOTS: usize = ((64 - SIZE_BITS) / SLOT_BITS) as usize;

/// A bit-packed list of `(order, unknown_to)` pairs.
///
/// `order` is a card order (< 64). `unknown_to` is the player who doesn't know the card is
/// this identity, or `None` if it is known to everyone (stored as the all-ones sentinel).
///
/// Layout, from the LSB: `[size][[unknown_to_1][order_1]][[unknown_to_2][order_2]]...`.
/// Slots are never removed, only added or overwritten.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CertainMapEntry(u64);

impl CertainMapEntry {
    pub const EMPTY: Self = Self(0);

    pub const fn len(self) -> usize {
        (self.0 & SIZE_MASK) as usize
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    fn encode_unknown(unknown_to: Option<usize>) -> u64 {
        match unknown_to {
            None => UNKNOWN_MASK,
            Some(u) => {
                assert!((u as u64) < UNKNOWN_MASK, "player index {u} too large");
                u as u64
            }
        }
    }

    fn make_slot(order: usize, unknown_to: Option<usize>) -> u64 {
        assert!(order < 1 << ORDER_BITS, "order {order} too large");
        ((order as u64) << UNKNOWN_BITS) | Self::encode_unknown(unknown_to)
    }

    fn slot(self, i: usize) -> (usize, Option<usize>) {
        let bits = self.0 >> (SIZE_BITS + i as u32 * SLOT_BITS);
        let order = ((bits & ORDER_MASK) >> UNKNOWN_BITS) as usize;
        let unknown = match bits & UNKNOWN_MASK {
            UNKNOWN_MASK => None,
            u => Some(u as usize),
        };
        (order, unknown)
    }

    fn overwrite_slot(&mut self, i: usize, slot: u64) {
        let shift = SIZE_BITS + i as u32 * SLOT_BITS;
        self.0 = (self.0 & !(SLOT_MASK << shift)) | (slot << shift);
    }

    /// The `(order, unknown_to)` pairs, in insertion order.
    pub fn iter(self) -> impl Iterator<Item = (usize, Option<usize>)> {
        (0..self.len()).map(move |i| self.slot(i))
    }

    /// Records that the card with this order is certain, unknown to `unknown_to`.
    /// Overwrites the existing slot if the order is already present.
    pub fn update(&mut self, order: usize, unknown_to: Option<usize>) {
        let slot = Self::make_slot(order, unknown_to);

        match self.iter().position(|(o, _)| o == order) {
            Some(i) => self.overwrite_slot(i, slot),
            None => {
                let size = self.len();
                assert!(size < MAX_SLOTS, "CertainMapEntry is full");
                self.overwrite_slot(size, slot);
                self.0 = (self.0 & !SIZE_MASK) | (size as u64 + 1);
            }
        }
    }

    /// Whether a slot has this order.
    pub fn contains_order(self, order: usize) -> bool {
        self.iter().any(|(o, _)| o == order)
    }

    /// Whether a slot has this order, *or* is unknown to `unknown_to`.
    ///
    /// (This mirrors the Scala `contains(order, unknownTo)`, whose condition is a disjunction.)
    pub fn contains(self, order: usize, unknown_to: Option<usize>) -> bool {
        self.iter().any(|(o, u)| o == order || u == unknown_to)
    }

    /// The set of orders stored.
    pub fn orders(self) -> FastBitSet {
        self.iter().map(|(o, _)| o).collect()
    }
}

impl fmt::Display for CertainMapEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CertainMapEntry(")?;
        for (i, (order, unknown)) in self.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            match unknown {
                Some(u) => write!(f, "{order}/{u}")?,
                None => write!(f, "{order}/-1")?,
            }
        }
        write!(f, ")")
    }
}

impl fmt::Debug for CertainMapEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
