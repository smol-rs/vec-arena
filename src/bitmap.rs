//! The internals of `VecArena`.
//!
//! `Bitmap` tracks occupied and vacant slots in `VecArena` and provides useful methods for e.g.
//! iterating over all occupied slots, or reserving additional slots.

use std::cmp;
use std::mem;
use std::ptr;

/// Returns the number of bits in one `usize` integer.
#[inline]
fn bits() -> usize {
    mem::size_of::<usize>() * 8
}

/// Returns the number of `usize` integers required to store `len` bits.
#[inline]
fn blocks_for(len: usize) -> usize {
    // Divide `len` by `bits()` and round up.
    len / bits() + ((len % bits() > 0) as usize)
}

/// Given a valid `block` index and `offset` within it, returns the index of that slot as within
/// the whole bitmap.
#[inline]
fn slot_index(block: usize, offset: usize) -> usize {
    block * bits() + offset
}

/// Given a slot `index` in the bitmap, returns it's block index and offset within the block.
#[inline]
fn block_and_offset(index: usize) -> (usize, usize) {
    (index / bits(), index % bits())
}

/// Keeps track of occupied and vacant slots in `VecArena`.
///
/// It's implemented as an array of blocks, where every block tracks only a small contiguous chunk
/// of slots. More precisely: as many slots as there are bits in one `usize`.
///
/// All blocks which are not fully occupied, except the last one, are linked together to form a
/// doubly-linked list. This list allows finding a vacant slot to acquire in `O(1)` and releasing
/// an occupied slot in `O(1)`, assuming the bitmap doesn't grow to accommodate more slots.
///
/// A block consists of:
///
/// * a bit mask (one `usize`), in which zeros are for vacant slots and ones for occupied slots
/// * index of the successor block in the linked list
/// * index of the predecessor block in the linked list
///
/// The last block is tricky to handle because it might not have the same number of slots as other
/// blocks, so it gets special treatment in the implementation.
pub struct Bitmap {
    /// Storage for the following sequences, in this order:
    ///
    /// * bit masks
    /// * indices to successor nodes
    /// * indices to predecessor nodes
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
    occupied: usize,

    /// Index of the first block in the linked list, or `!0` if the list is empty.
    head: usize,
}

impl Bitmap {
    /// Constructs a new `Bitmap` with zero slots.
    #[inline]
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
            occupied: 0,
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
    pub fn occupied(&self) -> usize {
        self.occupied
    }

    /// Finds a vacant slot, marks as occupied, and returns its index.
    ///
    /// # Panics
    ///
    /// Panics if all slots are occupied.
    pub fn acquire(&mut self) -> usize {
        assert!(self.occupied < self.len, "no vacant slots to acquire, len = {}", self.len);

        let block = if self.head == !0 {
            // The list is empty, so try the last block.
            blocks_for(self.len) - 1
        } else {
            // The list has a head. Take a vacant slot from the head block.
            self.head
        };

        let index = unsafe {
            // Find the rightmost zero bit in the mask. Taking the rightmost zero is always ok,
            // even if this is the last block.
            let offset = (!*self.mask(block)).trailing_zeros() as usize;
            debug_assert!(offset < bits());

            slot_index(block, offset)
        };
        self.acquire_at(index);
        index
    }

    /// Marks the vacant slot at `index` as occupied.
    ///
    /// # Panics
    ///
    /// Panics if the slot is already occupied.
    pub fn acquire_at(&mut self, index: usize) {
        assert!(index < self.len, "`index` out of bounds");

        let (block, offset) = block_and_offset(index);
        unsafe {
            // Mark the slot as occupied in the block's bit mask.
            assert!(*self.mask(block) >> offset & 1 == 0, "occupied slot at `index`");
            *self.mask(block) ^= 1 << offset;

            // If the block has just become fully occupied, remove it from the list.
            if block == self.head && *self.mask(block) == !0 {
                let succ = *self.successor(block);
                self.link_blocks(!0, succ);
                self.head = succ;
            }

            self.occupied += 1;
        }
    }

    /// Releases the occupied slot at `index`.
    ///
    /// # Panics
    ///
    /// Panics if the `index` is out of bounds or the slot is vacant.
    pub fn release(&mut self, index: usize) {
        assert!(index < self.len, "`index` out of bounds");

        let (block, offset) = block_and_offset(index);
        unsafe {
            // Make sure we're not releasing a vacant slot.
            assert!(*self.mask(block) >> offset & 1 == 1, "releasing a vacant slot");

            self.occupied -= 1;

            // If the block is fully occupied, insert it back into the list.
            if *self.mask(block) == !0 {
                let head = self.head;
                self.link_blocks(!0, block);
                self.link_blocks(block, head);
                self.head = block;
            }

            // Mark the slot as vacant in the block's bit mask.
            *self.mask(block) ^= 1 << offset;
        }
    }

    /// Given the required minimal additional number of slots to reserve, returns the number of
    /// slots to reserve in order to keep amortized time complexity.
    fn amortized_reserve(&self, additional: usize) -> usize {
        let len = self.len();
        let required_len = len.checked_add(additional).expect("len overflow");
        let double_len = len.checked_mul(2).expect("len overflow");
        cmp::max(required_len, double_len) - len
    }

    /// Reserves at least `additional` more slots for new elements to be inserted. The arena may
    /// reserve more space to avoid frequent reallocations.
    ///
    /// # Panics
    ///
    /// Panics if the new number of slots overflows `usize`.
    pub fn reserve(&mut self, additional: usize) {
        let amortized = self.amortized_reserve(additional);
        self.reserve_exact(amortized);
    }

    /// Reserves exactly `additional` more slots for new elements to be inserted.
    ///
    /// Note that the allocator may give the bitmap more space than it requests.
    ///
    /// # Panics
    ///
    /// Panics if the new number of slots overflows `usize`.
    pub fn reserve_exact(&mut self, additional: usize) {
        let old_blocks = blocks_for(self.len);
        self.len = self.len.checked_add(additional).expect("len overflow");
        let new_blocks = blocks_for(self.len);

        if new_blocks == old_blocks {
            // We can simply return.
            // The higher unused bits of the last block are always zero anyway.
            return;
        }

        unsafe {
            // If the last block had some unused or vacant slots, push it into the linked list.
            if old_blocks > 0 && *self.mask(old_blocks - 1) != !0 {
                let head = self.head;
                let last = old_blocks - 1;
                self.link_blocks(last, head);
                self.link_blocks(!0, last);
                self.head = last;
            }

            // Allocate a new array.
            let new_data = {
                // Every block contains three `usize` integers, so we need `3 * new_blocks` of
                // space.
                let mut vec = Vec::with_capacity(3 * new_blocks);
                let ptr = vec.as_mut_ptr();
                mem::forget(vec);
                ptr
            };

            // Copy the three old subarrays (bit masks, successor indices, predecessor indices)
            // into the new array.
            for i in 0..3 {
                ptr::copy_nonoverlapping(
                    self.data.offset((old_blocks * i) as isize),
                    new_data.offset((new_blocks * i) as isize),
                    old_blocks);
            }

            // Deallocate the old array.
            Vec::from_raw_parts(self.data, 0, 3 * old_blocks);

            // Set the new bit masks to zero.
            ptr::write_bytes(new_data.offset(old_blocks as isize), 0, new_blocks - old_blocks);

            // Set the new data now because we're about to push blocks into the linked list.
            self.data = new_data;

            // If there are at least two new blocks, that means there is at least one new block
            // that must be pushed into the linked list.
            if old_blocks + 2 <= new_blocks {
                let head = self.head;
                self.link_blocks(new_blocks - 2, head);
                self.link_blocks(!0, old_blocks);
                self.head = old_blocks;

                for block in old_blocks .. new_blocks - 2 {
                    self.link_blocks(block, block + 1);
                }
            }
        }
    }

    /// Returns `true` if the slot at `index` is occupied.
    #[inline]
    pub fn is_occupied(&self, index: usize) -> bool {
        assert!(index < self.len, "`index` out of bounds");

        let (block, offset) = block_and_offset(index);
        unsafe {
            *self.mask(block) >> offset & 1 != 0
        }
    }

    /// Returns an iterator over occupied slots.
    #[inline]
    pub fn iter(&self) -> Iter {
        Iter {
            bitmap: self,
            index: 0,
        }
    }

    #[cfg(debug_assertions)]
    pub fn check_invariants(&self) {
        // TODO
    }

    /// Links together blocks `a` and `b` so that `a` comes before `b` in the linked list.
    #[inline]
    unsafe fn link_blocks(&mut self, a: usize, b: usize) {
        if a != !0 { *self.successor(a) = b; }
        if b != !0 { *self.predecessor(b) = a; }
    }

    /// Returns the pointer to the bit mask of `block`.
    #[inline]
    unsafe fn mask(&self, block: usize) -> *mut usize {
        self.data.offset(block as isize)
    }

    /// Returns the pointer to the index of the successor of `block`.
    #[inline]
    unsafe fn successor(&self, block: usize) -> *mut usize {
        self.data.offset((blocks_for(self.len) + block) as isize)
    }

    /// Returns the pointer to the index of the predecessor of `block`.
    #[inline]
    unsafe fn predecessor(&self, block: usize) -> *mut usize {
        self.data.offset((2 * blocks_for(self.len) + block) as isize)
    }

    /// Returns the next occupied slot starting from (and including) the specified `index`.
    pub fn next_occupied(&self, mut index: usize) -> Option<usize> {
        while index < self.len() {
            let (block, offset) = block_and_offset(index);
            let mask = unsafe { *self.mask(block) };

            if mask != 0 {
                for off in offset .. bits() {
                    if mask >> off & 1 == 1 {
                        return Some(slot_index(block, off));
                    }
                }
            }

            index = slot_index(block + 1, 0);
        }
        None
    }

    /// Returns the previous occupied slot before the specified `index`.
    pub fn previous_occupied(&self, mut index: usize) -> Option<usize> {
        index = cmp::min(index, self.len());
        loop {
            let (block, offset) = block_and_offset(index);
            let mask = unsafe { *self.mask(block) };

            if mask != 0 {
                for off in (0 .. offset).rev() {
                    if mask >> off & 1 == 1 {
                        return Some(slot_index(block, off));
                    }
                }
            }

            if block == 0 {
                break;
            }
            index = slot_index(block - 1, 0);
        }
        None
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            Vec::from_raw_parts(self.data, 0, 3 * blocks_for(self.len));
        }
    }
}

/// Iterates over all occupied slots.
pub struct Iter<'a> {
    /// The bitmap to iterate over.
    bitmap: &'a Bitmap,

    /// Index of the current slot.
    index: usize,
}

impl<'a> Iterator for Iter<'a> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.bitmap.next_occupied(self.index).map(|index| {
            self.index = index + 1;
            index
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let low = 0;
        let high = self.bitmap.len() - self.index;
        (low, Some(high))
    }

    #[inline]
    fn count(self) -> usize {
        self.bitmap.occupied
    }
}
