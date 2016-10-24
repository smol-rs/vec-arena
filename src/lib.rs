use bitmap::Bitmap;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

mod bitmap;

// TODO: Handle ZST differently
// TODO: Test ZST, make a ZST implementing Drop
// TODO: check for overflow

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

    pub fn insert(&mut self, value: T) -> usize {
        let len = self.bitmap.len();
        let count = self.bitmap.count();

        if count == len {
            let new_len = if len == 0 { 4 } else { 2 * len };
            self.resize(new_len);
        }

        let index = self.bitmap.allocate();
        unsafe {
            ptr::write(self.elems.offset(index as isize) as *mut T, value);
        }
        index
    }

    pub fn remove(&mut self, index: usize) -> T {
        self.validate_index(index);
        unsafe {
            self.bitmap.free(index);
            ptr::read(self.elems.offset(index as isize) as *mut T)
        }
    }

    #[cold]
    fn resize(&mut self, new_len: usize) {
        let new_elems = unsafe {
            let mut vec = Vec::with_capacity(new_len);
            let ptr = vec.as_mut_ptr();
            mem::forget(vec);

            let len = self.bitmap.len();
            ptr::copy_nonoverlapping(self.elems, ptr, len);
            Vec::from_raw_parts(self.elems as *mut T, 0, len);

            ptr
        };

        self.elems = new_elems;
        self.bitmap.resize(new_len);
    }

    #[inline]
    fn validate_index(&self, index: usize) {
        unsafe {
            if index >= self.bitmap.len() || !self.bitmap.is_allocated(index) {
                self.panic_invalid_index(index);
            }
        }
    }

    #[cold]
    #[inline(never)]
    unsafe fn panic_invalid_index(&self, index: usize) {
        assert!(index < self.bitmap.len(),
                "index out of bounds: the cap is {} but the index is {}", self.bitmap.len(), index);

        panic!("uninitialized memory at index {}", index);
    }
}

impl<T> Drop for VecArena<T> {
    fn drop(&mut self) {
        unsafe {
            for index in self.bitmap.iter() {
                ptr::drop_in_place(self.elems.offset(index as isize) as *mut T);
            }
            Vec::from_raw_parts(self.elems as *mut T, 0, self.bitmap.len());
        }
    }
}

impl<T> Index<usize> for VecArena<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.validate_index(index);
        unsafe {
            &*self.elems.offset(index as isize)
        }
    }
}

impl<T> IndexMut<usize> for VecArena<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.validate_index(index);
        unsafe {
            &mut *(self.elems.offset(index as isize) as *mut T)
        }
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
