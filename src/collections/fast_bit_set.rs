use std::fmt;
use std::iter::FusedIterator;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Sub, SubAssign};

/// A fast set of integers between 0 and 63, backed by a single `u64`.
///
/// Iteration is always in ascending order. The Scala `FastBitSet` was inconsistent here
/// (`toList`, `map`, `flatMap` and `withFilter` yielded descending order, `iter` ascending);
/// where the original relied on that, call sites use `.iter().rev()`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FastBitSet(u64);

impl FastBitSet {
    pub const EMPTY: Self = Self(0);

    /// The set containing only `num`.
    #[inline]
    pub const fn single(num: usize) -> Self {
        debug_assert!(num < 64);
        Self(1 << num)
    }

    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    #[inline]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The number of elements contained.
    #[inline]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn contains(self, num: usize) -> bool {
        num < 64 && self.0 & (1 << num) != 0
    }

    /// The smallest element, if any.
    #[inline]
    pub const fn first(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.trailing_zeros() as usize)
        }
    }

    /// The largest element, if any.
    #[inline]
    pub const fn last(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some(63 - self.0.leading_zeros() as usize)
        }
    }

    /// Whether the set contains only `num`.
    #[inline]
    pub const fn is_exactly(self, num: usize) -> bool {
        self.0 == Self::single(num).0
    }

    /// This set with `num` added.
    #[inline]
    pub const fn with(self, num: usize) -> Self {
        Self(self.0 | Self::single(num).0)
    }

    /// This set with `num` removed.
    #[inline]
    pub const fn without(self, num: usize) -> Self {
        Self(self.0 & !Self::single(num).0)
    }

    /// Adds `num`, returning whether it was newly inserted.
    #[inline]
    pub fn insert(&mut self, num: usize) -> bool {
        let had = self.contains(num);
        *self = self.with(num);
        !had
    }

    /// Removes `num`, returning whether it was present.
    #[inline]
    pub fn remove(&mut self, num: usize) -> bool {
        let had = self.contains(num);
        *self = self.without(num);
        had
    }

    /// Elements in either set.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Elements in both sets.
    #[inline]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Elements in this set that aren't in the other.
    #[inline]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The subset of elements satisfying the predicate.
    pub fn filter(self, mut cond: impl FnMut(usize) -> bool) -> Self {
        self.iter().filter(|&n| !cond(n)).fold(self, Self::without)
    }

    /// Returns `alternative()` if this set is empty, otherwise this set.
    #[inline]
    pub fn when_empty(self, alternative: impl FnOnce() -> Self) -> Self {
        if self.is_empty() { alternative() } else { self }
    }

    #[inline]
    pub const fn iter(self) -> Iter {
        Iter(self.0)
    }
}

/// Iterator over the elements of a [`FastBitSet`], ascending.
#[derive(Clone)]
pub struct Iter(u64);

impl Iterator for Iter {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let bit = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1;
        Some(bit)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count_ones() as usize;
        (n, Some(n))
    }
}

impl DoubleEndedIterator for Iter {
    #[inline]
    fn next_back(&mut self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        let bit = 63 - self.0.leading_zeros() as usize;
        self.0 &= !(1 << bit);
        Some(bit)
    }
}

impl ExactSizeIterator for Iter {}
impl FusedIterator for Iter {}

impl IntoIterator for FastBitSet {
    type Item = usize;
    type IntoIter = Iter;

    fn into_iter(self) -> Iter {
        self.iter()
    }
}

impl IntoIterator for &FastBitSet {
    type Item = usize;
    type IntoIter = Iter;

    fn into_iter(self) -> Iter {
        self.iter()
    }
}

impl FromIterator<usize> for FastBitSet {
    fn from_iter<I: IntoIterator<Item = usize>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

impl Extend<usize> for FastBitSet {
    fn extend<I: IntoIterator<Item = usize>>(&mut self, iter: I) {
        for n in iter {
            self.insert(n);
        }
    }
}

impl<const N: usize> From<[usize; N]> for FastBitSet {
    fn from(nums: [usize; N]) -> Self {
        nums.into_iter().collect()
    }
}

impl BitOr for FastBitSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitAnd for FastBitSet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

impl Sub for FastBitSet {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.difference(rhs)
    }
}

impl BitOrAssign for FastBitSet {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl BitAndAssign for FastBitSet {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}

impl SubAssign for FastBitSet {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl fmt::Display for FastBitSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FastBitSet(")?;
        for (i, n) in self.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{n}")?;
        }
        write!(f, ")")
    }
}

impl fmt::Debug for FastBitSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_membership() {
        let set = FastBitSet::from([0, 5, 63]);
        assert_eq!(set.len(), 3);
        assert!(set.contains(0) && set.contains(5) && set.contains(63));
        assert!(!set.contains(1));
        assert!(!set.contains(64));
        assert_eq!(set.first(), Some(0));
        assert_eq!(set.last(), Some(63));
    }

    #[test]
    fn empty_set() {
        let set = FastBitSet::EMPTY;
        assert!(set.is_empty());
        assert_eq!(set.first(), None);
        assert_eq!(set.last(), None);
        assert_eq!(set.iter().count(), 0);
    }

    #[test]
    fn is_exactly() {
        assert!(FastBitSet::single(7).is_exactly(7));
        assert!(!FastBitSet::single(7).is_exactly(6));
        assert!(!FastBitSet::from([6, 7]).is_exactly(7));
        assert!(!FastBitSet::EMPTY.is_exactly(0));
    }

    #[test]
    fn iterates_both_directions() {
        let set = FastBitSet::from([3, 1, 40, 2]);
        assert_eq!(set.iter().collect::<Vec<_>>(), [1, 2, 3, 40]);
        assert_eq!(set.iter().rev().collect::<Vec<_>>(), [40, 3, 2, 1]);
        assert_eq!(set.iter().len(), 4);

        let mut it = set.iter();
        assert_eq!(it.next(), Some(1));
        assert_eq!(it.next_back(), Some(40));
        assert_eq!(it.collect::<Vec<_>>(), [2, 3]);
    }

    #[test]
    fn set_algebra() {
        let a = FastBitSet::from([1, 2, 3]);
        let b = FastBitSet::from([3, 4]);
        assert_eq!(a | b, FastBitSet::from([1, 2, 3, 4]));
        assert_eq!(a & b, FastBitSet::from([3]));
        assert_eq!(a - b, FastBitSet::from([1, 2]));
        assert_eq!(a.with(9).without(1), FastBitSet::from([2, 3, 9]));
    }

    #[test]
    fn insert_and_remove_report_changes() {
        let mut set = FastBitSet::EMPTY;
        assert!(set.insert(4));
        assert!(!set.insert(4));
        assert!(set.remove(4));
        assert!(!set.remove(4));
        assert!(set.is_empty());
    }

    #[test]
    fn filter_keeps_matching() {
        let set = FastBitSet::from([1, 2, 3, 4, 5]);
        assert_eq!(set.filter(|n| n % 2 == 1), FastBitSet::from([1, 3, 5]));
        assert_eq!(set.filter(|_| false), FastBitSet::EMPTY);
    }

    #[test]
    fn when_empty_only_replaces_empty() {
        let alt = || FastBitSet::from([9]);
        assert_eq!(FastBitSet::EMPTY.when_empty(alt), FastBitSet::from([9]));
        assert_eq!(FastBitSet::from([1]).when_empty(alt), FastBitSet::from([1]));
    }

    #[test]
    fn formats_like_scala() {
        assert_eq!(
            FastBitSet::from([2, 10, 7]).to_string(),
            "FastBitSet(2,7,10)"
        );
        assert_eq!(FastBitSet::EMPTY.to_string(), "FastBitSet()");
    }
}
