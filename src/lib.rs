//! A fast allocator of objects of a single type.
//!
//! This crate contains `VecArena`, which is an allocator that can hold objects of only one type.
//! It can allocate space for new objects and reclaim space upon removal of objects. The amount
//! of space to hold objects is dynamically expanded as needed. Inserting objects into an arena
//! is amortized `O(1)`, and removal is `O(1)`.
// TODO: An example of a doubly linked list?

use bitmap::Bitmap;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

mod bitmap;

// TODO: fn drain
// TODO: fn iter
// TODO: fn iter_mut
// TODO: fn into_iter

/// An arena that can insert and remove objects of a single type.
///
/// `VecArena<T>` is a a lot like `Vec<Option<T>>` because it holds a sequence of slots for storing
/// objects. A slot can be either occupied or vacant. Inserting a new object into an arena
/// involves finding a vacant slot to store the object. To remove an object from a slot, the
/// object is taken out of the slot, and the slot is marked as vacant.
///
/// Internally, a bitmap is used instead of `Option`s to conserve space and improve cache
/// performance. Every object access makes sure that the accessed object really exists, otherwise
/// it panics.
// TODO: a bunch of examples, see the docs for Vec for inspiration.
pub struct VecArena<T> {
    elems: *const T,
    bitmap: Bitmap,
    marker: PhantomData<T>,
}

impl<T> VecArena<T> {
    /// Constructs a new, empty arena.
    ///
    /// The arena will not allocate until objects are inserted into it.
    pub fn new() -> Self {
        let elems = {
            let mut vec = Vec::with_capacity(0);
            let ptr = vec.as_mut_ptr();
            mem::forget(vec);
            ptr
        };
        VecArena {
            elems: elems,
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
            ptr::write(self.elems.offset(index as isize) as *mut T, object);
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
            ptr::write(self.elems.offset(index as isize) as *mut T, object);
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
            ptr::read(self.elems.offset(index as isize) as *mut T)
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
        ptr::copy_nonoverlapping(self.elems, ptr, old_len);

        // Deallocate the old array.
        Vec::from_raw_parts(self.elems as *mut T, 0, old_len);

        self.elems = ptr;
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

    /// Returns a reference to the object at `index`, without bounds checking nor checking that
    /// the object exists.
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        &*self.elems.offset(index as isize)
    }

    /// Returns a mutable reference to the object at `index`, without bounds checking nor checking
    /// that the object exists.
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        &mut *(self.elems.offset(index as isize) as *mut T)
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
                ptr::drop_in_place(self.elems.offset(index as isize) as *mut T);
            }

            // Deallocate the old array.
            Vec::from_raw_parts(self.elems as *mut T, 0, self.bitmap.len());
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

#[cfg(test)]
mod tests {
    use super::*;

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
