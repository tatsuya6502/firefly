use alloc::alloc::{AllocError, Allocator, Global, Layout};
use core::cell::UnsafeCell;
use core::mem;
use core::ptr::{self, NonNull};

use firefly_alloc::heap::Heap;

use crate::term::Term;

pub struct ProcessHeap {
    range: *mut [u8],
    top: UnsafeCell<*mut u8>,
}
impl ProcessHeap {
    const DEFAULT_HEAP_SIZE: usize = 4 * 1024;

    pub fn new() -> Self {
        let layout =
            Layout::from_size_align(Self::DEFAULT_HEAP_SIZE, mem::align_of::<Term>()).unwrap();
        let nonnull = Global.allocate(layout).unwrap();
        Self {
            range: nonnull.as_ptr(),
            top: UnsafeCell::new(nonnull.as_non_null_ptr().as_ptr()),
        }
    }
}
impl Drop for ProcessHeap {
    fn drop(&mut self) {
        let size = ptr::metadata(self.range) as usize;
        let layout = Layout::from_size_align(size, mem::align_of::<Term>()).unwrap();
        unsafe { Global.deallocate(NonNull::new_unchecked(self.range.cast()), layout) }
    }
}
unsafe impl Allocator for ProcessHeap {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let layout = layout.pad_to_align();
        let size = layout.size();

        // Calculate the base pointer of the allocation at the desired alignment,
        // then offset that pointer by the desired size to give us the new top
        let top = unsafe { *self.top.get() };
        let offset = top.align_offset(layout.align());
        let base = unsafe { top.add(offset) };
        let new_top = unsafe { base.add(size) } as *const u8;

        // Make sure the requested allocation fits within the fragment
        let start = self.range.as_mut_ptr() as *const u8;
        let heap_size = self.range.len();
        let range = start..(unsafe { start.add(heap_size) });
        if range.contains(&new_top) {
            unsafe {
                self.top.get().write(new_top as *mut u8);
            }
            Ok(unsafe { NonNull::new_unchecked(ptr::from_raw_parts_mut(base.cast(), size)) })
        } else {
            Err(AllocError)
        }
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {}
    unsafe fn grow(
        &self,
        _ptr: NonNull<u8>,
        _old_layout: Layout,
        _new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        Err(AllocError)
    }
    unsafe fn grow_zeroed(
        &self,
        _ptr: NonNull<u8>,
        _old_layout: Layout,
        _new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        Err(AllocError)
    }
    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        _old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
    }
}
impl Heap for ProcessHeap {
    #[inline]
    fn heap_start(&self) -> *mut u8 {
        self.range.as_mut_ptr()
    }

    #[inline]
    fn heap_top(&self) -> *mut u8 {
        unsafe { *self.top.get() }
    }

    #[inline]
    fn heap_end(&self) -> *mut u8 {
        unsafe { self.heap_start().add(self.range.len()) }
    }
}
