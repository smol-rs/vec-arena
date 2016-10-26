//! A fast general-purpose object arena.
//!
//! This crate contains `VecArena`, which is an allocator that can hold objects of only one type.
//! It can allocate space for new objects and reclaim space upon removal of objects. The amount
//! of space to hold objects is dynamically expanded as needed. Inserting objects into an arena
//! is amortized `O(1)`, and removing is `O(1)`.
// TODO: An example of a doubly linked list?

use bitmap::Bitmap;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

mod bitmap;

// TODO: fn insert_near / nearest_vacant
// TODO: DoubleEndedIterator

/// An arena that can insert and remove objects of a single type.
///
/// `VecArena<T>` is in many ways like `Vec<Option<T>>` because it holds an array of slots for
/// storing objects. A slot can be either occupied or vacant. Inserting a new object into an arena
/// involves finding a vacant slot and placing the object into the slot. Removing an object means
/// taking it out of the slot and marking it as vacant.
///
/// Internally, a bitmap is used instead of `Option`s to conserve space and improve cache
/// performance. Every object access makes sure that the accessed object really exists, otherwise
/// it panics.
// TODO: a bunch of examples, see the docs for Vec for inspiration.
pub struct VecArena<T> {
    // TODO: Docs for these fields
    slots: *const T,
    bitmap: Bitmap,
    marker: PhantomData<T>,
}

impl<T> VecArena<T> {
    /// Constructs a new, empty arena.
    ///
    /// The arena will not allocate until objects are inserted into it.
    pub fn new() -> Self {
        let slots = {
            let mut vec = Vec::with_capacity(0);
            let ptr = vec.as_mut_ptr();
            mem::forget(vec);
            ptr
        };
        VecArena {
            slots: slots,
            bitmap: Bitmap::new(),
            marker: PhantomData,
        }
    }

    /// Constructs a new, empty arena with the specified capacity (number of slots).
    ///
    /// The arena will be able to hold exactly `capacity` objects without reallocating.
    /// If `capacity` is 0, the arena will not allocate.
    pub fn with_capacity(cap: usize) -> Self {
        let mut arena = Self::new();
        arena.reserve_exact(cap);
        arena
    }

    /// Returns the number of objects the arena can hold without reallocating.
    /// In other words, this is the number of slots.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.bitmap.len()
    }

    /// Returns the number of objects in the arena.
    /// In other words, this is the number of occupied slots.
    #[inline]
    pub fn len(&self) -> usize {
        self.bitmap.occupied()
    }

    /// Returns `true` if the arena holds no objects.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the slot at `index` is vacant.
    #[inline]
    pub fn is_vacant(&self, index: usize) -> bool {
        !self.bitmap.is_occupied(index)
    }

    /// Returns `true` if the slot at `index` is occupied.
    #[inline]
    pub fn is_occupied(&self, index: usize) -> bool {
        self.bitmap.is_occupied(index)
    }

    /// Inserts an object into the arena and returns the slot index in which it was stored.
    /// The arena will reallocate if it's full.
    pub fn insert(&mut self, object: T) -> usize {
        if self.bitmap.occupied() == self.bitmap.len() {
            self.double();
        }

        let index = self.bitmap.acquire();
        unsafe {
            ptr::write(self.slots.offset(index as isize) as *mut T, object);
        }
        index
    }

    /// Inserts an object into the vacant slot at `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds or the slot is already occupied.
    pub fn insert_at(&mut self, index: usize, object: T) -> usize {
        self.bitmap.acquire_at(index);
        unsafe {
            ptr::write(self.slots.offset(index as isize) as *mut T, object);
        }
        index
    }

    /// Removes the object stored at `index` from the arena and returns it.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds or the slot is already vacant.
    pub fn remove(&mut self, index: usize) -> T {
        // `release` will panic if the index is out of bounds or the slot is already vacant.
        self.bitmap.release(index);

        unsafe {
            ptr::read(self.slots.offset(index as isize) as *mut T)
        }
    }

    /// Clears the arena, removing and dropping all objects it holds. Keeps the allocated memory
    /// for reuse.
    pub fn clear(&mut self) {
        let mut arena = VecArena::with_capacity(self.capacity());
        mem::swap(self, &mut arena);
    }

    /// Returns a reference to the object stored at `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.guard_index(index);
        if self.is_occupied(index) {
            unsafe {
                Some(self.get_unchecked(index))
            }
        } else {
            None
        }
    }

    /// Returns a mutable reference to the object stored at `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.guard_index(index);
        if self.is_occupied(index) {
            unsafe {
                Some(self.get_unchecked_mut(index))
            }
        } else {
            None
        }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted. The arena may
    /// reserve more space to avoid frequent reallocations.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    pub fn reserve(&mut self, additional: usize) {
        let len = self.bitmap.len();
        self.bitmap.reserve(additional);
        unsafe {
            self.reallocate(len);
        }
    }

    /// Reserves the minimum capacity for exactly `additional` more elements to be inserted.
    ///
    /// Note that the allocator may give the arena more space than it requests.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    pub fn reserve_exact(&mut self, additional: usize) {
        let len = self.bitmap.len();
        self.bitmap.reserve_exact(additional);
        unsafe {
            self.reallocate(len);
        }
    }

    /// Reallocates the object array because the bitmap was resized.
    unsafe fn reallocate(&mut self, old_len: usize) {
        let new_len = self.bitmap.len();

        // Allocate a new array.
        let mut vec = Vec::with_capacity(new_len);
        let ptr = vec.as_mut_ptr();
        mem::forget(vec);

        // Copy data into the new array.
        ptr::copy_nonoverlapping(self.slots, ptr, old_len);

        // Deallocate the old array.
        Vec::from_raw_parts(self.slots as *mut T, 0, old_len);

        self.slots = ptr;
    }

    /// Doubles the capacity of the arena.
    #[cold]
    #[inline(never)]
    fn double(&mut self) {
        let len = self.bitmap.len();
        let elem_size = mem::size_of::<T>();

        let new_len = if len == 0 {
            if elem_size.checked_mul(4).is_some() {
                4
            } else {
                1
            }
        } else {
            len.checked_mul(2).expect("len overflow")
        };

        self.reserve_exact(new_len - len);
    }

    #[inline]
    pub fn iter(&self) -> Iter<T> {
        Iter {
            arena: self,
            index: 0,
        }
    }

    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<T> {
        IterMut {
            slots: self.slots as *mut T,
            bitmap: &self.bitmap,
            index: 0,
            marker: PhantomData,
        }
    }

    #[inline]
    pub fn drain(&mut self) -> Drain<T> {
        Drain {
            arena: self,
            index: 0,
        }
    }

    /// Returns a reference to the object at `index`, without bounds checking nor checking that
    /// the object exists.
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        &*self.slots.offset(index as isize)
    }

    /// Returns a mutable reference to the object at `index`, without bounds checking nor checking
    /// that the object exists.
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        &mut *(self.slots.offset(index as isize) as *mut T)
    }

    /// Panics if `index` is out of bounds.
    #[inline]
    fn guard_index(&self, index: usize) {
        assert!(index < self.bitmap.len(), "`index` out of bounds");
    }

    /// Panics if `index` is out of bounds or the slot is vacant.
    #[inline]
    fn guard_occupied(&self, index: usize) {
        // `is_occupied` will panic if the index is out of bounds.
        assert!(self.bitmap.is_occupied(index), "vacant slot at `index`");
    }
}

impl<T> Drop for VecArena<T> {
    fn drop(&mut self) {
        unsafe {
            // Drop all objects in the arena.
            for index in self.bitmap.iter() {
                ptr::drop_in_place(self.slots.offset(index as isize) as *mut T);
            }

            // Deallocate the old array.
            Vec::from_raw_parts(self.slots as *mut T, 0, self.bitmap.len());
        }
    }
}

impl<T> Index<usize> for VecArena<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &T {
        self.guard_occupied(index);
        unsafe {
            self.get_unchecked(index)
        }
    }
}

impl<T> IndexMut<usize> for VecArena<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.guard_occupied(index);
        unsafe {
            self.get_unchecked_mut(index)
        }
    }
}

impl<T> Default for VecArena<T> {
    fn default() -> Self {
        VecArena::new()
    }
}

impl<T: Clone> Clone for VecArena<T> {
    fn clone(&self) -> Self {
        let mut arena = VecArena::with_capacity(self.capacity());
        for index in self.bitmap.iter() {
            let clone = unsafe {
                self.get_unchecked(index).clone()
            };
            arena.insert_at(index, clone);
        }
        arena
    }
}

pub struct IntoIter<T> {
    slots: *const T,
    bitmap: Bitmap,
    index: usize,
    marker: PhantomData<T>,
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        while self.next().is_some() {}
    }
}

impl<T> Iterator for IntoIter<T> {
    type Item = (usize, T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.bitmap.next_occupied(self.index).map(|index| {
            self.index = index + 1;
            unsafe {
                self.bitmap.release(index);
                (index, ptr::read(self.slots.offset(index as isize)))
            }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let lower = 0;
        let upper = self.bitmap.len() - self.index;
        (lower, Some(upper))
    }
}

impl<T> IntoIterator for VecArena<T> {
    type Item = (usize, T);
    type IntoIter = IntoIter<T>;

    fn into_iter(mut self) -> Self::IntoIter {
        let iter = IntoIter {
            slots: self.slots,
            bitmap: mem::replace(&mut self.bitmap, Bitmap::new()),
            index: 0,
            marker: PhantomData,
        };
        mem::forget(self);
        iter
    }
}

pub struct Iter<'a, T: 'a> {
    arena: &'a VecArena<T>,
    index: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (usize, &'a T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.arena.bitmap.next_occupied(self.index).map(|index| {
            self.index = index + 1;
            unsafe {
                (index, self.arena.get_unchecked(index))
            }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let lower = 0;
        let upper = self.arena.capacity() - self.index;
        (lower, Some(upper))
    }
}

pub struct IterMut<'a, T: 'a> {
    slots: *mut T,
    bitmap: &'a Bitmap,
    index: usize,
    marker: PhantomData<&'a mut T>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (usize, &'a mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.bitmap.next_occupied(self.index).map(|index| {
            self.index = index + 1;
            unsafe {
                (index, &mut *self.slots.offset(index as isize))
            }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let lower = 0;
        let upper = self.bitmap.len() - self.index;
        (lower, Some(upper))
    }
}

pub struct Drain<'a, T: 'a> {
    arena: &'a mut VecArena<T>,
    index: usize,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = (usize, T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.arena.bitmap.next_occupied(self.index).map(|index| {
            self.index = index + 1;
            unsafe {
                self.arena.bitmap.release(index);
                (index, ptr::read(self.arena.slots.offset(index as isize)))
            }
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let lower = 0;
        let upper = self.arena.capacity() - self.index;
        (lower, Some(upper))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: test variance: https://github.com/rust-lang/rust/pull/30998/files

    #[test]
    fn it_works() {
        let mut arena = VecArena::new();
        for i in 0..10 {
            assert_eq!(arena.insert(()), i);
            assert!(arena[i] == ());
        }
        for i in 0..10 {
            assert!(arena[i] == ());
        }
    }
}
