//! A simple object arena.
//!
//! `Arena<T>` is basically just a `Vec<Option<T>>`, which allows you to:
//!
//! * Insert an object (reuse an existing `None` element, or append to the end).
//! * Remove object at a specified index.
//! * Access object at a specified index.
//!
//! # Examples
//!
//! Some data structures built using `Arena<T>`:
//!
//! * [Doubly linked list](https://github.com/stjepang/vec-arena/blob/master/examples/linked_list.rs)
//! * [Splay tree](https://github.com/stjepang/vec-arena/blob/master/examples/splay_tree.rs)

extern crate unreachable;

use std::fmt;
use std::iter;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;
use std::slice;
use std::vec;

use unreachable::unreachable;

/// A slot, which is either vacant or occupied.
///
/// Vacant slots in arena are linked together into a singly linked list. This allows the arena to
/// efficiently find a vacant slot before inserting a new object, or reclaiming a slot after
/// removing an object.
#[derive(Clone)]
enum Slot<T> {
    /// Vacant slot, containing index to the next slot in the linked list.
    Vacant(usize),

    /// Occupied slot, containing a value.
    Occupied(T),
}

/// An object arena.
///
/// `Arena<T>` holds an array of slots for storing objects.
/// Every slot is always in one of two states: occupied or vacant.
///
/// Essentially, this is equivalent to `Vec<Option<T>>`.
///
/// # Insert and remove
///
/// When inserting a new object into arena, a vacant slot is found and then the object is placed
/// into the slot. If there are no vacant slots, the array is reallocated with bigger capacity.
/// The cost of insertion is amortized `O(1)`.
///
/// When removing an object, the slot containing it is marked as vacant and the object is returned.
/// The cost of removal is `O(1)`.
///
/// ```
/// use vec_arena::Arena;
///
/// let mut arena = Arena::new();
/// let a = arena.insert(10);
/// let b = arena.insert(20);
///
/// assert_eq!(a, 0); // 10 was placed at index 0
/// assert_eq!(b, 1); // 20 was placed at index 1
///
/// assert_eq!(arena.remove(a), Some(10));
/// assert_eq!(arena.get(a), None); // slot at index 0 is now vacant
///
/// assert_eq!(arena.insert(30), 0); // slot at index 0 is reused
/// ```
///
/// # Indexing
///
/// You can also access objects in an arena by index, just like you would in a `Vec`.
/// However, accessing a vacant slot by index or using an out-of-bounds index will result in panic.
///
/// ```
/// use vec_arena::Arena;
///
/// let mut arena = Arena::new();
/// let a = arena.insert(10);
/// let b = arena.insert(20);
///
/// assert_eq!(arena[a], 10);
/// assert_eq!(arena[b], 20);
///
/// arena[a] += arena[b];
/// assert_eq!(arena[a], 30);
/// ```
///
/// To access slots without fear of panicking, use `get` and `get_mut`, which return `Option`s.
pub struct Arena<T> {
    /// Slots in which objects are stored.
    slots: Vec<Slot<T>>,

    /// Number of occupied slots in the arena.
    len: usize,

    /// Index of the first vacant slot in the linked list.
    head: usize,
}

impl<T> Arena<T> {
    /// Constructs a new, empty arena.
    ///
    /// The arena will not allocate until objects are inserted into it.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena: Arena<i32> = Arena::new();
    /// ```
    #[inline]
    pub fn new() -> Self {
        Arena {
            slots: Vec::new(),
            len: 0,
            head: !0,
        }
    }

    /// Constructs a new, empty arena with the specified capacity (number of slots).
    ///
    /// The arena will be able to hold exactly `capacity` objects without reallocating.
    /// If `capacity` is 0, the arena will not allocate.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::with_capacity(10);
    ///
    /// assert_eq!(arena.len(), 0);
    /// assert_eq!(arena.capacity(), 10);
    ///
    /// // These inserts are done without reallocating...
    /// for i in 0..10 {
    ///     arena.insert(i);
    /// }
    /// assert_eq!(arena.capacity(), 10);
    ///
    /// // ... but this one will reallocate.
    /// arena.insert(11);
    /// assert!(arena.capacity() > 10);
    /// ```
    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Arena {
            slots: Vec::with_capacity(cap),
            len: 0,
            head: !0,
        }
    }

    /// Returns the number of slots in the arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let arena: Arena<i32> = Arena::with_capacity(10);
    /// assert_eq!(arena.capacity(), 10);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    /// Returns the number of occupied slots in the arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// assert_eq!(arena.len(), 0);
    ///
    /// for i in 0..10 {
    ///     arena.insert(());
    ///     assert_eq!(arena.len(), i + 1);
    /// }
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if all slots are vacant.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// assert!(arena.is_empty());
    ///
    /// arena.insert(1);
    /// assert!(!arena.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the index of the slot that next `insert` will use if no other
    /// mutating calls take place in between.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    ///
    /// let a = arena.next_vacant();
    /// let b = arena.insert(1);
    /// assert_eq!(a, b);
    /// let c = arena.next_vacant();
    /// let d = arena.insert(2);
    /// assert_eq!(c, d);
    /// ```
    #[inline]
    pub fn next_vacant(&mut self) -> usize {
        if self.head == !0 {
            self.len
        } else {
            self.head
        }
    }

    /// Inserts an object into the arena and returns the slot index it was stored in.
    /// The arena will reallocate if it's full.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    ///
    /// let a = arena.insert(1);
    /// let b = arena.insert(2);
    /// assert!(a != b);
    /// ```
    #[inline]
    pub fn insert(&mut self, object: T) -> usize {
        self.len += 1;

        if self.head == !0 {
            self.slots.push(Slot::Occupied(object));
            self.len - 1
        } else {
            let index = self.head;
            match self.slots[index] {
                Slot::Vacant(next) => {
                    self.head = next;
                    self.slots[index] = Slot::Occupied(object);
                },
                Slot::Occupied(_) => unreachable!(),
            }
            index
        }
    }

    /// Removes the object stored at `index` from the arena and returns it.
    ///
    /// `None` is returned in case the slot is vacant, or `index` is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let a = arena.insert("hello");
    ///
    /// assert_eq!(arena.len(), 1);
    /// assert_eq!(arena.remove(a), Some("hello"));
    ///
    /// assert_eq!(arena.len(), 0);
    /// assert_eq!(arena.remove(a), None);
    /// ```
    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<T> {
        match self.slots.get_mut(index) {
            None => None,
            Some(&mut Slot::Vacant(_)) => None,
            Some(slot @ &mut Slot::Occupied(_)) => {
                if let Slot::Occupied(object) = mem::replace(slot, Slot::Vacant(self.head)) {
                    self.head = index;
                    self.len -= 1;
                    Some(object)
                } else {
                    unreachable!();
                }
            }
        }
    }

    /// Clears the arena, removing and dropping all objects it holds. Keeps the allocated memory
    /// for reuse.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// for i in 0..10 {
    ///     arena.insert(i);
    /// }
    ///
    /// assert_eq!(arena.len(), 10);
    /// arena.clear();
    /// assert_eq!(arena.len(), 0);
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.slots.clear();
        self.len = 0;
        self.head = !0;
    }

    /// Returns a reference to the object stored at `index`.
    ///
    /// If `index` is out of bounds or the slot is vacant, `None` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let index = arena.insert("hello");
    ///
    /// assert_eq!(arena.get(index), Some(&"hello"));
    /// arena.remove(index);
    /// assert_eq!(arena.get(index), None);
    /// ```
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        match self.slots.get(index) {
            None => None,
            Some(&Slot::Vacant(_)) => None,
            Some(&Slot::Occupied(ref object)) => Some(object),
        }
    }

    /// Returns a mutable reference to the object stored at `index`.
    ///
    /// If `index` is out of bounds or the slot is vacant, `None` is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let index = arena.insert(7);
    ///
    /// assert_eq!(arena.get_mut(index), Some(&mut 7));
    /// *arena.get_mut(index).unwrap() *= 10;
    /// assert_eq!(arena.get_mut(index), Some(&mut 70));
    /// ```
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        match self.slots.get_mut(index) {
            None => None,
            Some(&mut Slot::Vacant(_)) => None,
            Some(&mut Slot::Occupied(ref mut object)) => Some(object),
        }
    }

    /// Returns a reference to the object stored at `index`.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if `index` is out of bounds or the slot is vacant.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let index = arena.insert("hello");
    ///
    /// unsafe { assert_eq!(&*arena.get_unchecked(index), &"hello") }
    /// ```
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        match self.slots.get(index) {
            None => unreachable(),
            Some(&Slot::Vacant(_)) => unreachable(),
            Some(&Slot::Occupied(ref object)) => object,
        }
    }

    /// Returns a mutable reference to the object stored at `index`.
    ///
    /// # Safety
    ///
    /// Behavior is undefined if `index` is out of bounds or the slot is vacant.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let index = arena.insert("hello");
    ///
    /// unsafe { assert_eq!(&*arena.get_unchecked_mut(index), &"hello") }
    /// ```
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        match self.slots.get_mut(index) {
            None => unreachable(),
            Some(&mut Slot::Vacant(_)) => unreachable(),
            Some(&mut Slot::Occupied(ref mut object)) => object,
        }
    }

    /// Swaps two objects in the arena.
    ///
    /// The two indices are `a` and `b`.
    ///
    /// # Panics
    ///
    /// Panics if any of the indices is out of bounds or any of the slots is vacant.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// let a = arena.insert(7);
    /// let b = arena.insert(8);
    ///
    /// arena.swap(a, b);
    /// assert_eq!(arena.get(a), Some(&8));
    /// assert_eq!(arena.get(b), Some(&7));
    /// ```
    #[inline]
    pub fn swap(&mut self, a: usize, b: usize) {
        unsafe {
            let fst = self.get_mut(a).unwrap() as *mut _;
            let snd = self.get_mut(b).unwrap() as *mut _;
            if a != b {
                ptr::swap(fst, snd);
            }
        }
    }

    /// Reserves capacity for at least `additional` more objects to be inserted. The arena may
    /// reserve more space to avoid frequent reallocations.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// arena.insert("hello");
    ///
    /// arena.reserve(10);
    /// assert!(arena.capacity() >= 11);
    /// ```
    pub fn reserve(&mut self, additional: usize) {
        let vacant = self.slots.len() - self.len;
        if additional > vacant {
            self.slots.reserve(additional - vacant);
        }
    }

    /// Reserves the minimum capacity for exactly `additional` more objects to be inserted.
    ///
    /// Note that the allocator may give the arena more space than it requests.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// arena.insert("hello");
    ///
    /// arena.reserve_exact(10);
    /// assert!(arena.capacity() >= 11);
    /// ```
    pub fn reserve_exact(&mut self, additional: usize) {
        let vacant = self.slots.len() - self.len;
        if additional > vacant {
            self.slots.reserve_exact(additional - vacant);
        }
    }

    /// Returns an iterator over occupied slots.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// arena.insert(1);
    /// arena.insert(2);
    /// arena.insert(4);
    ///
    /// let mut iterator = arena.iter();
    /// assert_eq!(iterator.next(), Some((0, &1)));
    /// assert_eq!(iterator.next(), Some((1, &2)));
    /// assert_eq!(iterator.next(), Some((2, &4)));
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<T> {
        Iter { slots: self.slots.iter().enumerate() }
    }

    /// Returns an iterator that returns mutable references to objects.
    ///
    /// # Examples
    ///
    /// ```
    /// use vec_arena::Arena;
    ///
    /// let mut arena = Arena::new();
    /// arena.insert("zero".to_string());
    /// arena.insert("one".to_string());
    /// arena.insert("two".to_string());
    ///
    /// for (index, object) in arena.iter_mut() {
    ///     *object = index.to_string() + " " + object;
    /// }
    ///
    /// let mut iterator = arena.iter();
    /// assert_eq!(iterator.next(), Some((0, &"0 zero".to_string())));
    /// assert_eq!(iterator.next(), Some((1, &"1 one".to_string())));
    /// assert_eq!(iterator.next(), Some((2, &"2 two".to_string())));
    /// ```
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<T> {
        IterMut { slots: self.slots.iter_mut().enumerate() }
    }
}

impl<T> fmt::Debug for Arena<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Arena {{ ... }}")
    }
}

impl<T> Index<usize> for Arena<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("vacant slot at `index`")
    }
}

impl<T> IndexMut<usize> for Arena<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("vacant slot at `index`")
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena::new()
    }
}

impl<T: Clone> Clone for Arena<T> {
    fn clone(&self) -> Self {
        Arena {
            slots: self.slots.clone(),
            len: self.len,
            head: self.head,
        }
    }
}

/// An iterator over the occupied slots in a `Arena`.
pub struct IntoIter<T> {
    slots: iter::Enumerate<vec::IntoIter<Slot<T>>>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = (usize, T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, slot)) = self.slots.next() {
            if let Slot::Occupied(object) = slot {
                return Some((index, object));
            }
        }
        None
    }
}

impl<T> IntoIterator for Arena<T> {
    type Item = (usize, T);
    type IntoIter = IntoIter<T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        IntoIter { slots: self.slots.into_iter().enumerate() }
    }
}

impl<T> iter::FromIterator<T> for Arena<T> {
    fn from_iter<U: IntoIterator<Item=T>>(iter: U) -> Arena<T> {
        let iter = iter.into_iter();
        let mut arena = Arena::with_capacity(iter.size_hint().0);
        for i in iter {
            arena.insert(i);
        }
        arena
    }
}

impl<T> fmt::Debug for IntoIter<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IntoIter {{ ... }}")
    }
}

/// An iterator over references to the occupied slots in a `Arena`.
pub struct Iter<'a, T: 'a> {
    slots: iter::Enumerate<slice::Iter<'a, Slot<T>>>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (usize, &'a T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, slot)) = self.slots.next() {
            if let Slot::Occupied(ref object) = *slot {
                return Some((index, object));
            }
        }
        None
    }
}

impl<'a, T> fmt::Debug for Iter<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Iter {{ ... }}")
    }
}

/// An iterator over mutable references to the occupied slots in a `Arena`.
pub struct IterMut<'a, T: 'a> {
    slots: iter::Enumerate<slice::IterMut<'a, Slot<T>>>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (usize, &'a mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((index, slot)) = self.slots.next() {
            if let Slot::Occupied(ref mut object) = *slot {
                return Some((index, object));
            }
        }
        None
    }
}

impl<'a, T> fmt::Debug for IterMut<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IterMut {{ ... }}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        let arena = Arena::<i32>::new();
        assert!(arena.is_empty());
        assert_eq!(arena.len(), 0);
        assert_eq!(arena.capacity(), 0);
    }

    #[test]
    fn insert() {
        let mut arena = Arena::new();

        for i in 0..10 {
            assert_eq!(arena.insert(i * 10), i);
            assert_eq!(arena[i], i * 10);
        }
        assert!(!arena.is_empty());
        assert_eq!(arena.len(), 10);
    }

    #[test]
    fn with_capacity() {
        let mut arena = Arena::with_capacity(10);
        assert_eq!(arena.capacity(), 10);

        for _ in 0..10 {
            arena.insert(());
        }
        assert_eq!(arena.len(), 10);
        assert_eq!(arena.capacity(), 10);

        arena.insert(());
        assert_eq!(arena.len(), 11);
        assert!(arena.capacity() > 10);
    }

    #[test]
    fn remove() {
        let mut arena = Arena::new();

        assert_eq!(arena.insert(0), 0);
        assert_eq!(arena.insert(10), 1);
        assert_eq!(arena.insert(20), 2);
        assert_eq!(arena.insert(30), 3);
        assert_eq!(arena.len(), 4);

        assert_eq!(arena.remove(1), Some(10));
        assert_eq!(arena.remove(2), Some(20));
        assert_eq!(arena.len(), 2);

        assert!(arena.insert(-1) < 4);
        assert!(arena.insert(-1) < 4);
        assert_eq!(arena.len(), 4);

        assert_eq!(arena.remove(0), Some(0));
        assert!(arena.insert(-1) < 4);
        assert_eq!(arena.len(), 4);

        assert_eq!(arena.insert(400), 4);
        assert_eq!(arena.len(), 5);
    }

    #[test]
    fn invalid_remove() {
        let mut arena = Arena::new();
        for i in 0..10 {
            arena.insert(i.to_string());
        }

        assert_eq!(arena.remove(7), Some("7".to_string()));
        assert_eq!(arena.remove(5), Some("5".to_string()));

        assert_eq!(arena.remove(!0), None);
        assert_eq!(arena.remove(10), None);
        assert_eq!(arena.remove(11), None);

        assert_eq!(arena.remove(5), None);
        assert_eq!(arena.remove(7), None);
    }

    #[test]
    fn clear() {
        let mut arena = Arena::new();
        arena.insert(10);
        arena.insert(20);

        assert!(!arena.is_empty());
        assert_eq!(arena.len(), 2);

        let cap = arena.capacity();
        arena.clear();

        assert!(arena.is_empty());
        assert_eq!(arena.len(), 0);
        assert_eq!(arena.capacity(), cap);
    }

    #[test]
    fn indexing() {
        let mut arena = Arena::new();

        let a = arena.insert(10);
        let b = arena.insert(20);
        let c = arena.insert(30);

        arena[b] += arena[c];
        assert_eq!(arena[a], 10);
        assert_eq!(arena[b], 50);
        assert_eq!(arena[c], 30);
    }

    #[test]
    #[should_panic]
    fn indexing_vacant() {
        let mut arena = Arena::new();

        let _ = arena.insert(10);
        let b = arena.insert(20);
        let _ = arena.insert(30);

        arena.remove(b);
        arena[b];
    }

    #[test]
    #[should_panic]
    fn invalid_indexing() {
        let mut arena = Arena::new();

        arena.insert(10);
        arena.insert(20);
        arena.insert(30);

        arena[100];
    }

    #[test]
    fn get() {
        let mut arena = Arena::new();

        let a = arena.insert(10);
        let b = arena.insert(20);
        let c = arena.insert(30);

        *arena.get_mut(b).unwrap() += *arena.get(c).unwrap();
        assert_eq!(arena.get(a), Some(&10));
        assert_eq!(arena.get(b), Some(&50));
        assert_eq!(arena.get(c), Some(&30));

        arena.remove(b);
        assert_eq!(arena.get(b), None);
        assert_eq!(arena.get_mut(b), None);
    }

    #[test]
    fn reserve() {
        let mut arena = Arena::new();
        arena.insert(1);
        arena.insert(2);

        arena.reserve(10);
        assert!(arena.capacity() >= 11);
    }

    #[test]
    fn reserve_exact() {
        let mut arena = Arena::new();
        arena.insert(1);
        arena.insert(2);
        arena.reserve(10);
        assert!(arena.capacity() >= 11);
    }

    #[test]
    fn iter() {
        let mut arena = Arena::new();
        let a = arena.insert(10);
        let b = arena.insert(20);
        let c = arena.insert(30);
        let d = arena.insert(40);

        arena.remove(b);

        let mut it = arena.iter();
        assert_eq!(it.next(), Some((a, &10)));
        assert_eq!(it.next(), Some((c, &30)));
        assert_eq!(it.next(), Some((d, &40)));
        assert_eq!(it.next(), None);
    }

    #[test]
    fn iter_mut() {
        let mut arena = Arena::new();
        let a = arena.insert(10);
        let b = arena.insert(20);
        let c = arena.insert(30);
        let d = arena.insert(40);

        arena.remove(b);

        {
            let mut it = arena.iter_mut();
            assert_eq!(it.next(), Some((a, &mut 10)));
            assert_eq!(it.next(), Some((c, &mut 30)));
            assert_eq!(it.next(), Some((d, &mut 40)));
            assert_eq!(it.next(), None);
        }

        for (index, value) in arena.iter_mut() {
            *value += index;
        }

        let mut it = arena.iter_mut();
        assert_eq!(*it.next().unwrap().1, 10 + a);
        assert_eq!(*it.next().unwrap().1, 30 + c);
        assert_eq!(*it.next().unwrap().1, 40 + d);
        assert_eq!(it.next(), None);
    }

    #[test]
    fn from_iter() {
        let arena: Arena<_> = [10, 20, 30, 40].iter().cloned().collect();

        let mut it = arena.iter();
        assert_eq!(it.next(), Some((0, &10)));
        assert_eq!(it.next(), Some((1, &20)));
        assert_eq!(it.next(), Some((2, &30)));
        assert_eq!(it.next(), Some((3, &40)));
        assert_eq!(it.next(), None);
    }
}
