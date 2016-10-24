use std::mem;
use std::ptr;

#[inline(always)]
fn bits() -> usize {
    mem::size_of::<usize>() * 8
}

pub struct Bitmap {
    data: *mut usize,
    blocks: usize,
    len: usize,
    count: usize,
    head: usize,
}

impl Bitmap {
    pub fn new() -> Self {
        let data = {
            let mut v = Vec::with_capacity(0);
            let ptr = v.as_mut_ptr();
            mem::forget(v);
            ptr
        };
        Bitmap {
            data: data,
            blocks: 0,
            len: 0,
            count: 0,
            head: !0,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn allocate(&mut self) -> usize {
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
            debug_assert!(i < bits());

            let index = b * bits() + i;
            debug_assert!(index < self.len);

            debug_assert!(*self.mask(b) >> i & 1 == 0);
            *self.mask(b) |= 1 << i;

            if *self.mask(b) == !0 && self.head == b {
                let b = *self.next(b);
                if b != !0 {
                    *self.prev(b) = !0;
                }
                self.head = b;
            }

            self.count += 1;
            index
        }
    }

    unsafe fn connect(&mut self, a: usize, b: usize) {
        if a != !0 { *self.next(a) = b; }
        if b != !0 { *self.prev(b) = a; }
    }

    pub unsafe fn free(&mut self, index: usize) {
        let b = index / bits();
        let i = index % bits();

        *self.mask(b) ^= 1 << i;
        self.count -= 1;

        if *self.mask(b) == 0 {
            let head = self.head;
            self.head = b;

            self.connect(!0, b);
            self.connect(b, head);
        }
    }

    pub fn resize(&mut self, len: usize) {
        unsafe {
            assert!(self.len <= len);
            self.len = len;

            let new_blocks = (len + bits() - 1) / bits();
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
    }

    #[inline]
    pub unsafe fn is_allocated(&self, index: usize) -> bool {
        let b = index / bits();
        let i = index % bits();
        unsafe {
            *self.mask(b) >> i & 1 != 0
        }
    }

    pub fn iter(&self) -> Iter {
        Iter {
            bitmap: &self,
            b: 0,
            i: 0,
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
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.data, 0, 2 * self.blocks);
        }
    }
}

struct Iter<'a> {
    bitmap: &'a Bitmap,
    b: usize,
    i: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.b < self.bitmap.blocks {
            let mask = unsafe { *self.bitmap.mask(self.b) };

            if self.i == bits() || mask == 0 {
                self.b += 1;
                self.i = 0;
            } else {
                while self.i < bits() && mask >> self.i & 1 == 0 {
                    self.i += 1;
                }

                if self.i < bits() {
                    let index = self.b * bits() + self.i;
                    self.i += 1;
                    return Some(index);
                }
            }
        }
        None
    }
}
