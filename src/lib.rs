use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

// TODO: Handle ZST differently
// TODO: Test ZST, make a ZST implementing Drop
// TODO: check for overflow

#[inline(always)]
fn bits() -> usize {
    std::mem::size_of::<usize>() * 8
}

struct Bitmap {
    data: *mut usize,
    cap: usize,
    blocks: usize,
    head: usize,
}

impl Bitmap {
    fn new() -> Self {
        let data = {
            let mut v = Vec::with_capacity(0);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        Bitmap {
            data: data,
            cap: 0,
            blocks: 0,
            head: !0,
        }
    }

    #[inline(always)]
    unsafe fn mask(&self, b: usize) -> *mut usize {
        self.data.offset(b as isize)
    }

    #[inline(always)]
    unsafe fn next(&self, b: usize) -> *mut usize {
        self.data.offset((self.blocks + b) as isize)
    }

    #[inline(always)]
    unsafe fn prev(&self, b: usize) -> *mut usize {
        self.data.offset((2 * self.blocks + b) as isize)
    }

    fn allocate(&mut self) -> usize {
        assert!(self.blocks > 0);

        let b = if self.head == !0 {
            self.blocks - 1
        } else {
            self.head
        };

        unsafe {
            let i = {
                let mask = *self.mask(b);
                if self.head == !0 {
                    assert!(mask != !0);
                }
                (!mask).trailing_zeros() as usize
            };

            let index = b * bits() + i;

            debug_assert!(0 <= index && index < self.cap);
            debug_assert!(0 <= i && i < bits());
            debug_assert!(*self.mask(b) >> i & 1 == 0);

            *self.mask(b) |= 1 << i;

            if *self.mask(b) == !0 && self.head == b {
                let b = *self.next(b);
                if b != !0 {
                    *self.prev(b) = !0;
                }
                self.head = b;
            }

            index
        }
    }

    unsafe fn resize(&mut self, cap: usize) {
        assert!(self.cap <= cap);
        self.cap = cap;

        let new_blocks = (cap + bits() - 1) / bits();
        assert!(self.blocks <= new_blocks);

        let diff = new_blocks - self.blocks;
        if diff == 0 {
            return;
        }

        if self.blocks > 0 && *self.mask(self.blocks - 1) != !0 {
            *self.next(self.blocks - 1) = self.head;
            *self.prev(self.blocks - 1) = !0;
            self.head = self.blocks - 1;
        }

        let new_data = {
            let mut v = Vec::with_capacity(3 * new_blocks);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };

        for i in 0..3 {
            ptr::copy_nonoverlapping(
                self.data.offset((self.blocks * i) as isize),
                new_data.offset((new_blocks * i) as isize),
                self.blocks);
        }
        Vec::from_raw_parts(self.data, 0, 3 * self.blocks);

        ptr::write_bytes(new_data.offset(self.blocks as isize), 0, new_blocks - self.blocks);

        let old_blocks = self.blocks;
        self.data = new_data;
        self.blocks = new_blocks;

        if diff >= 2 {
            for b in old_blocks .. new_blocks - 2 {
                *self.next(b) = b + 1;
            }
            *self.next(new_blocks - 2) = self.head;

            for b in old_blocks + 1 .. new_blocks - 1 {
                *self.prev(b) = b - 1;
            }
            *self.prev(old_blocks) = !0;

            if self.head != !0 {
                *self.prev(self.head) = new_blocks - 2;
            }
            self.head = old_blocks;
        }
    }

    #[inline]
    unsafe fn is_allocated(&self, index: usize) -> bool {
        let b = index / bits();
        let i = index % bits();
        unsafe {
            *self.mask(b) >> i & 1 != 0
        }
    }
}

pub struct VecArena<T> {
    elems: *const T,
    count: usize,
    cap: usize,
    bitmap: Bitmap,
    marker: PhantomData<T>,
}

impl<T> VecArena<T> {
    pub fn new() -> Self {
        let elems = {
            let mut v = Vec::with_capacity(0);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        VecArena {
            elems: elems,
            count: 0,
            cap: 0,
            bitmap: Bitmap::new(),
            marker: PhantomData,
        }
    }

    pub fn insert(&mut self, value: T) -> usize {
        if self.count == self.cap {
            let new_cap = if self.cap == 0 { 4 } else { self.cap * 2 };
            self.resize(new_cap);
        }

        unsafe {
            let index = self.bitmap.allocate();
            ptr::write(self.elems.offset(index as isize) as *mut T, value);
            self.count += 1;
            index
        }
    }

    // pub fn remove(&mut self, index: usize) -> T {
    //     self.validate_index(index);
    //
    //     let b = index / bits();
    //     let i = index % bits();
    //
    //     unsafe {
    //         self.count -= 1;
    //         *self.get_alive(b) ^= 1 << i;
    //
    //         if *self.get_alive(b) == 0 {
    //             *self.get_next(b) = self.head;
    //             self.head = b;
    //         }
    //
    //         ptr::read(self.elems.offset(index as isize) as *mut T)
    //     }
    // }

    #[cold]
    fn resize(&mut self, new_cap: usize) {
        unsafe {
            let new_elems = {
                let mut v = Vec::with_capacity(new_cap);
                let ptr = v.as_mut_ptr();
                mem::forget(v);
                ptr
            };
            ptr::copy_nonoverlapping(self.elems, new_elems, self.cap);
            Vec::from_raw_parts(self.elems as *mut T, 0, self.cap);

            self.elems = new_elems;
            self.cap = new_cap;
            self.bitmap.resize(new_cap);
        }
    }

    #[inline]
    fn validate_index(&self, index: usize) {
        unsafe {
            if index >= self.cap || !self.bitmap.is_allocated(index) {
                self.panic_invalid_index(index);
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn panic_invalid_index(&self, index: usize) {
        if index >= self.cap {
            panic!("index out of bounds: the cap is {} but the index is {}", self.cap, index);
        }
        panic!("uninitialized memory: the index is {} but it's not allocated", index);
    }
}

// impl<T> Drop for VecArena<T> {
//     fn drop(&mut self) {
//         unsafe {
//             for b in 0 .. self.num_blocks() {
//                 let alive = *self.get_alive(b);
//                 if alive != 0 {
//                     for i in 0 .. bits() {
//                         if alive & (1 << i) != 0 {
//                             let index = b * bits() + i;
//                             ptr::drop_in_place(self.elems.offset(index as isize) as *mut T);
//                         }
//                     }
//                 }
//             }
//
//             let blocks = self.num_blocks();
//             Vec::from_raw_parts(self.elems as *mut T, 0, self.cap);
//             Vec::from_raw_parts(self.meta, 0, 2 * blocks);
//         }
//     }
// }

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
