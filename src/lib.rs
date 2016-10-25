use bitmap::Bitmap;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

mod bitmap;

// TODO: check for overflow
// TODO: clone

// TODO: fn drain
// TODO: fn iter
// TODO: fn iter_mut
// TODO: fn into_iter
// TODO: fn clear

pub struct VecArena<T> {
    elems: *const T,
    bitmap: Bitmap,
    marker: PhantomData<T>,
}

impl<T> VecArena<T> {
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

    pub fn with_capacity(cap: usize) -> Self {
        let mut arena = Self::new();
        arena.reserve_exact(cap);
        arena
    }

    pub fn capacity(&self) -> usize {
        self.bitmap.len()
    }

    pub fn occupied(&self) -> usize {
        self.bitmap.occupied()
    }

    pub fn insert(&mut self, value: T) -> usize {
        if self.bitmap.occupied() == self.bitmap.len() {
            self.double();
        }

        let index = self.bitmap.acquire();
        unsafe {
            ptr::write(self.elems.offset(index as isize) as *mut T, value);
        }
        index
    }

    pub fn remove(&mut self, index: usize) -> T {
        self.validate_index(index);
        unsafe {
            self.bitmap.release(index);
            ptr::read(self.elems.offset(index as isize) as *mut T)
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        let len = self.bitmap.len();
        self.bitmap.reserve(additional);
        self.reallocate(len);
    }

    pub fn reserve_exact(&mut self, additional: usize) {
        let len = self.bitmap.len();
        self.bitmap.reserve_exact(additional);
        self.reallocate(len);
    }

    fn reallocate(&mut self, old_len: usize) {
        let new_len = self.bitmap.len();

        unsafe {
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
    }

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
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        &*self.elems.offset(index as isize)
    }

    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        &mut *(self.elems.offset(index as isize) as *mut T)
    }

    #[inline]
    fn validate_index(&self, index: usize) {
        // This will also panic if the index is out of bounds.
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

    fn index(&self, index: usize) -> &T {
        self.validate_index(index);
        unsafe { self.get_unchecked(index) }
    }
}

impl<T> IndexMut<usize> for VecArena<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.validate_index(index);
        unsafe { self.get_unchecked_mut(index) }
    }
}

impl<T> Default for VecArena<T> {
    fn default() -> Self {
        VecArena::new()
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
