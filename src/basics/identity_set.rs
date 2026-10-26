use std::fmt;
use std::iter::FusedIterator;
use std::ops::{BitAnd, BitOr, Sub};

use super::card::Identity;
use super::state::State;

/// A set of [`Identity`]s (at most 30: 6 suits of 5 ranks), packed into a `u64` by ordinal.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IdentitySet(u64);

impl IdentitySet {
    pub const EMPTY: Self = Self(0);

    #[inline]
    pub const fn single(id: Identity) -> Self {
        Self(1 << id.to_ord())
    }

    /// The set of identities with ordinals below `max_ids` that satisfy the condition.
    pub fn create(mut cond: impl FnMut(Identity) -> bool, max_ids: usize) -> Self {
        (0..max_ids)
            .map(Identity::from_ord)
            .filter(|&id| cond(id))
            .collect()
    }

    #[inline]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The number of identities contained.
    #[inline]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn contains(self, id: Identity) -> bool {
        self.0 & (1 << id.to_ord()) != 0
    }

    /// The identity with the lowest ordinal, if any.
    #[inline]
    pub fn first(self) -> Option<Identity> {
        (self.0 != 0).then(|| Identity::from_ord(self.0.trailing_zeros() as usize))
    }

    /// Whether the set contains only this identity.
    #[inline]
    pub fn is_exactly(self, id: Identity) -> bool {
        self == Self::single(id)
    }

    #[inline]
    pub const fn with(self, id: Identity) -> Self {
        Self(self.0 | Self::single(id).0)
    }

    #[inline]
    pub const fn without(self, id: Identity) -> Self {
        Self(self.0 & !Self::single(id).0)
    }

    #[inline]
    pub fn insert(&mut self, id: Identity) {
        *self = self.with(id);
    }

    #[inline]
    pub fn remove(&mut self, id: Identity) {
        *self = self.without(id);
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[inline]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The subset of identities satisfying the predicate.
    pub fn filter(self, mut cond: impl FnMut(Identity) -> bool) -> Self {
        self.iter().filter(|&id| cond(id)).collect()
    }

    /// Returns `alternative()` if this set is empty, otherwise this set.
    pub fn when_empty(self, alternative: impl FnOnce() -> Self) -> Self {
        if self.is_empty() { alternative() } else { self }
    }

    /// Ascending by ordinal.
    #[inline]
    pub const fn iter(self) -> Iter {
        Iter(self.0)
    }

    /// The identities as `r1,r2,...`, using the state's suit abbreviations.
    pub fn fmt(self, state: &State) -> String {
        self.iter()
            .map(|id| state.log_id(id))
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Clone)]
pub struct Iter(u64);

impl Iterator for Iter {
    type Item = Identity;

    #[inline]
    fn next(&mut self) -> Option<Identity> {
        if self.0 == 0 {
            return None;
        }
        let bit = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1;
        Some(Identity::from_ord(bit))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count_ones() as usize;
        (n, Some(n))
    }
}

impl DoubleEndedIterator for Iter {
    #[inline]
    fn next_back(&mut self) -> Option<Identity> {
        if self.0 == 0 {
            return None;
        }
        let bit = 63 - self.0.leading_zeros() as usize;
        self.0 &= !(1 << bit);
        Some(Identity::from_ord(bit))
    }
}

impl ExactSizeIterator for Iter {}
impl FusedIterator for Iter {}

impl IntoIterator for IdentitySet {
    type Item = Identity;
    type IntoIter = Iter;
    fn into_iter(self) -> Iter {
        self.iter()
    }
}

impl FromIterator<Identity> for IdentitySet {
    fn from_iter<I: IntoIterator<Item = Identity>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

impl Extend<Identity> for IdentitySet {
    fn extend<I: IntoIterator<Item = Identity>>(&mut self, iter: I) {
        for id in iter {
            self.insert(id);
        }
    }
}

impl From<Identity> for IdentitySet {
    fn from(id: Identity) -> Self {
        Self::single(id)
    }
}

impl BitOr for IdentitySet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitAnd for IdentitySet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersect(rhs)
    }
}

impl Sub for IdentitySet {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.difference(rhs)
    }
}

impl fmt::Debug for IdentitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.iter().map(|id| (id.suit_index, id.rank)))
            .finish()
    }
}

/// An `Option<IdentitySet>`: "no set" is distinct from "the empty set".
///
/// (The Scala version packed this into a `Long` with `-1` as the sentinel; a real [`Option`]
/// is the same size here since [`IdentitySet`] only uses 30 bits.)
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct IdentitySetOpt(Option<IdentitySet>);

impl IdentitySetOpt {
    pub const NONE: Self = Self(None);

    #[inline]
    pub const fn is_defined(self) -> bool {
        self.0.is_some()
    }

    #[inline]
    pub const fn get(self) -> Option<IdentitySet> {
        self.0
    }

    pub fn map(self, f: impl FnOnce(IdentitySet) -> IdentitySet) -> Self {
        Self(self.0.map(f))
    }

    /// Whether the set exists and satisfies the predicate.
    pub fn exists(self, f: impl FnOnce(IdentitySet) -> bool) -> bool {
        self.0.is_some_and(f)
    }

    /// Whether the set doesn't exist or satisfies the predicate.
    pub fn forall(self, f: impl FnOnce(IdentitySet) -> bool) -> bool {
        self.0.is_none_or(f)
    }
}

impl From<IdentitySet> for IdentitySetOpt {
    fn from(set: IdentitySet) -> Self {
        Self(Some(set))
    }
}

impl From<Option<IdentitySet>> for IdentitySetOpt {
    fn from(set: Option<IdentitySet>) -> Self {
        Self(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: usize, r: usize) -> Identity {
        Identity::new(s, r)
    }

    #[test]
    fn set_operations() {
        let a: IdentitySet = [id(0, 1), id(0, 2), id(4, 5)].into_iter().collect();
        let b: IdentitySet = [id(0, 2), id(1, 1)].into_iter().collect();
        assert_eq!(a.len(), 3);
        assert!(a.contains(id(4, 5)) && !a.contains(id(1, 1)));
        assert_eq!((a & b).iter().collect::<Vec<_>>(), [id(0, 2)]);
        assert_eq!((a - b).len(), 2);
        assert_eq!((a | b).len(), 4);
        assert_eq!(a.first(), Some(id(0, 1)));
        assert!(IdentitySet::single(id(2, 3)).is_exactly(id(2, 3)));
    }

    #[test]
    fn iterates_by_ordinal() {
        let s: IdentitySet = [id(1, 1), id(0, 5), id(0, 1)].into_iter().collect();
        assert_eq!(s.iter().collect::<Vec<_>>(), [id(0, 1), id(0, 5), id(1, 1)]);
        assert_eq!(s.iter().next_back(), Some(id(1, 1)));
    }

    #[test]
    fn create_respects_max() {
        let s = IdentitySet::create(|i| i.rank == 1, 10);
        assert_eq!(s.iter().collect::<Vec<_>>(), [id(0, 1), id(1, 1)]);
    }

    #[test]
    fn opt_distinguishes_none_from_empty() {
        assert!(!IdentitySetOpt::NONE.is_defined());
        assert!(IdentitySetOpt::from(IdentitySet::EMPTY).is_defined());
        assert!(IdentitySetOpt::NONE.forall(|_| false));
        assert!(!IdentitySetOpt::NONE.exists(|_| true));
    }
}
