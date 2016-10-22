use std::marker::PhantomData;
use std::mem;
use std::ops::{Index, IndexMut};
use std::ptr;

// TODO: Handle ZST differently
// TODO: Test ZST, make a ZST implementing Drop

#[inline(always)]
fn bits() -> usize {
    std::mem::size_of::<usize>() * 8
}

// TODO: Move into VecArena?
#[inline(always)]
fn num_blocks(cap: usize) -> usize {
    (cap + bits() - 1) / bits()
}

pub struct VecArena<T> {
    elems: *const T,
    meta: *mut usize,
    cap: usize,
    head: usize,
    count: usize,
    marker: PhantomData<T>,
}

impl<T> VecArena<T> {
    #[inline(always)]
    unsafe fn get_alive(&self, block: usize) -> *mut usize {
        self.meta.offset(block as isize)
    }

    #[inline(always)]
    unsafe fn get_next(&self, block: usize) -> *mut usize {
        self.meta.offset((num_blocks(self.cap) + block) as isize)
    }

    pub fn new() -> Self {
        let elems = {
            let mut v = Vec::with_capacity(0);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        let meta = {
            let mut v = Vec::with_capacity(0);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        VecArena {
            elems: elems,
            meta: meta,
            cap: 0,
            head: !0,
            count: 0,
            marker: PhantomData,
        }
    }

    pub fn push(&mut self, value: T) -> usize {
        unsafe {
            if self.count == self.cap {
                self.grow();
            }
            while self.head != !0 && *self.get_alive(self.head) == !0 {
                self.head = *self.get_next(self.head);
            }
            if self.head == !0 {
                self.grow();
            }

            let i = (!*self.get_alive(self.head)).trailing_zeros() as usize;
            let index = self.head * bits() + i;

            unsafe {
                ptr::write(self.elems.offset(index as isize) as *mut T, value);
            }
            let block = self.head;
            *self.get_alive(block) |= 1 << i;
            self.count += 1;
            index
        }
    }

    pub fn take(&mut self, index: usize) -> T {
        self.validate_index(index);

        let b = index / bits();
        let i = index % bits();

        unsafe {
            self.count -= 1;
            *self.get_alive(b) ^= 1 << i;

            if *self.get_alive(b) == 0 {
                *self.get_next(b) = self.head;
                self.head = b;
            }

            ptr::read(self.elems.offset(index as isize) as *mut T)
        }
    }

    unsafe fn grow(&mut self) {
        let new_cap = if self.cap == 0 { 4 } else { self.cap * 2 };
        let blocks = num_blocks(self.cap);
        let new_blocks = num_blocks(new_cap);

        let new_elems = {
            let mut v = Vec::with_capacity(new_cap);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        ptr::copy_nonoverlapping(self.elems, new_elems, self.cap);
        Vec::from_raw_parts(self.elems as *mut T, 0, self.cap);
        self.elems = new_elems;

        let new_meta = {
            let mut v = Vec::from_raw_parts(self.meta, 2 * blocks, 2 * blocks);
            v.reserve_exact(new_blocks * 2 - blocks * 2);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        ptr::write_bytes(new_meta.offset(blocks as isize), 0, new_blocks - blocks);

        for i in blocks .. new_blocks {
            ptr::write(new_meta.offset((new_blocks + i) as isize), i.wrapping_sub(1));
        }

        self.meta = new_meta;

        self.cap = new_cap;
        self.head = new_blocks - 1;
    }

    #[inline]
    fn validate_index(&self, index: usize) {
        let b = index / bits();
        let i = index % bits();
        unsafe {
            if index >= self.cap || *self.get_alive(b) >> i & 1 == 0 {
                self.panic_invalid_index(index);
            }
        }
    }

    #[inline(never)]
    fn panic_invalid_index(&self, index: usize) {
        if index >= self.cap {
            panic!("index out of bounds: the cap is {} but the index is {}", self.cap, index);
        }
        panic!("uninitialized memory: the index is {} but it's not allocated", index);
    }
}

impl<T> Drop for VecArena<T> {
    fn drop(&mut self) {
        unsafe {
            for b in 0 .. num_blocks(self.cap) {
                let alive = *self.get_alive(b);
                if alive != 0 {
                    for i in 0 .. bits() {
                        if alive & (1 << i) != 0 {
                            let index = b * bits() + i;
                            ptr::drop_in_place(self.elems.offset(index as isize) as *mut T);
                        }
                    }
                }
            }

            let blocks = num_blocks(self.cap);
            Vec::from_raw_parts(self.elems as *mut T, 0, self.cap);
            Vec::from_raw_parts(self.meta, 0, 2 * blocks);
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

// TODO: impl Default

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut arena = VecArena::new();
        arena.alloc(1);
    }
}
