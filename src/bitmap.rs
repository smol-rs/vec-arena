//! Metadata used by `VecArena`.
//!
//! It keeps track of occupied and vacant slots in the arena. Useful methods are provided for e.g.
//! iterating over all occupied slots, or reserving additional slots.

use std::mem;
use std::ptr;

/// Returns the number of bits in one integer of type `usize`.
#[inline(always)]
fn bits() -> usize {
    mem::size_of::<usize>() * 8
}

/// Returns the number of integers required to store `len` bits.
#[inline(always)]
fn blocks_for(len: usize) -> usize {
    // Divide `len` by `bits()` and round up.
    (len + bits() - 1) / bits()
}

/// Metadata for keeping track of occupied and vacant slots in `VecArena`.
///
/// It's implemented as an array of blocks, where every block keeps information for a small
/// contiguous chunk of `log2(usize::MAX + 1)` slots. That's the number of bits in one `usize`.
///
/// A block consists of:
///
/// * a bit mask, in which zeros are for vacant slots and ones for occupied slots
/// * index of the next block in the linked list
/// * index of the previous block in the linked list
///
/// All blocks which are not fully occupied, except the last one, are linked together to form a
/// doubly-linked list. This list allows finding a vacant slot to acquire in `O(1)` and releasing
/// an occupied slot in `O(1)`, assuming the bitmap doesn't grow to accommodate more slots.
///
/// The last block is tricky to handle because it might not have the same number of slots as other
/// blocks, so it gets special treatment in the implementation.
pub struct Bitmap {
    /// Storage for the following sequences, in this order:
    ///
    /// * bit masks
    /// * indices to next node
    /// * indices to previous node
    ///
    /// All three sequences are stored in this single contiguous array.
    ///
    /// The most common and costly operation during the lifetime of a bitmap is testing
    /// whether a slot is occupied. Storing bit masks close together improves cache
    /// performance.
    data: *mut usize,

    /// Number of reserved slots.
    len: usize,

    /// Number of occupied slots.
    count: usize,

    /// Index of the first block in the linked list, or `!0` if the list is empty.
    head: usize,
}

impl Bitmap {
    /// Constructs a new `Bitmap` with zero slots.
    pub fn new() -> Self {
        let data = {
            let mut vec = Vec::with_capacity(0);
            let ptr = vec.as_mut_ptr();
            mem::forget(vec);
            ptr
        };
        Bitmap {
            data: data,
            len: 0,
            count: 0,
            head: !0,
        }
    }

    /// Returns the number of reserved slots in the bitmap.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the number of occupied slots in the bitmap.
    #[inline]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Finds a vacant slot, marks as occupied, and returns its index.
    ///
    /// # Panics
    ///
    /// Panics if all slots are occupied.
    pub fn acquire(&mut self) -> usize {
        assert!(self.count < self.len, "no vacant slots to acquire, len = {}", self.len);

        let num_blocks = blocks_for(self.len);

        let block = if self.head == !0 {
            // The list is empty, so try the last block.
            num_blocks - 1
        } else {
            // The list has a head. Take a vacant slot from the head block.
            self.head
        };

        unsafe {
            // Find the rightmost zero bit in the mask. Taking the rightmost zero is always ok,
            // even if this is the last block.
            let offset = (!*self.mask(block)).trailing_zeros() as usize;
            debug_assert!(offset < bits());

            // Use the block index and offset within it to calculate the actual slot index.
            let index = block * bits() + offset;
            debug_assert!(index < self.len);

            // Mark the slot as occupied in the block's bit mask.
            debug_assert!(*self.mask(block) >> offset & 1 == 0);
            *self.mask(block) ^= 1 << offset;

            if block == self.head {
                // If the block has just become fully occupied, remove it from the list.
                if *self.mask(block) == !0 {
                    let next = *self.next(block);
                    self.link_blocks(!0, next);
                    self.head = next;
                }
            }

            self.count += 1;
            index
        }
    }

    /// Releases the occupied slot at `index`.
    ///
    /// # Panics
    ///
    /// Panics if the `index` is out of bounds, or if the slot is vacant.
    pub fn release(&mut self, index: usize) {
        assert!(index < self.len);

        let block = index / bits();
        let offset = index % bits();

        unsafe {
            self.count -= 1;

            // If the block is fully occupied, insert it into the list because now it won't be.
            if *self.mask(block) == !0 {
                let head = self.head;
                self.link_blocks(!0, block);
                self.link_blocks(block, head);
                self.head = block;
            }

            // Mark the slot as vacant in the block's bit mask.
            assert!(*self.mask(block) >> offset & 1 == 1);
            *self.mask(block) ^= 1 << offset;
        }
    }

    pub fn resize(&mut self, new_len: usize) {
        unsafe {
            let old_blocks = blocks_for(self.len);

            assert!(self.len <= new_len);
            self.len = new_len;

            let new_blocks = blocks_for(new_len);
            assert!(old_blocks <= new_blocks);

            let diff = new_blocks - old_blocks;
            if diff == 0 {
                return;
            }

            if old_blocks > 0 && *self.mask(old_blocks - 1) != !0 {
                *self.next(old_blocks - 1) = self.head;
                *self.prev(old_blocks - 1) = !0;
                self.head = old_blocks - 1;
            }

            let new_data = {
                let mut vec = Vec::with_capacity(3 * new_blocks);
                let ptr = vec.as_mut_ptr();
                mem::forget(vec);
                ptr
            };

            for i in 0..3 {
                ptr::copy_nonoverlapping(
                    self.data.offset((old_blocks * i) as isize),
                    new_data.offset((new_blocks * i) as isize),
                    old_blocks);
            }
            Vec::from_raw_parts(self.data, 0, 3 * old_blocks);

            ptr::write_bytes(new_data.offset(old_blocks as isize), 0, new_blocks - old_blocks);

            self.data = new_data;

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
    pub unsafe fn is_occupied(&self, index: usize) -> bool {
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

    #[inline]
    unsafe fn link_blocks(&mut self, a: usize, b: usize) {
        if a != !0 { *self.next(a) = b; }
        if b != !0 { *self.prev(b) = a; }
    }

    #[inline(always)]
    unsafe fn mask(&self, b: usize) -> *mut usize {
        self.data.offset(b as isize)
    }

    #[inline(always)]
    unsafe fn next(&self, b: usize) -> *mut usize {
        self.data.offset((blocks_for(self.len) + b) as isize)
    }

    #[inline(always)]
    unsafe fn prev(&self, b: usize) -> *mut usize {
        self.data.offset((2 * blocks_for(self.len) + b) as isize)
    }

    #[cfg(debug_assertions)]
    pub fn check_invariants(&self) {
        // TODO
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.data, 0, 2 * blocks_for(self.len));
        }
    }
}

pub struct Iter<'a> {
    bitmap: &'a Bitmap,
    b: usize,
    i: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.b < blocks_for(self.bitmap.len) {
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
