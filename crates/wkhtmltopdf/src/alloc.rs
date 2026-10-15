//! Global allocator of the static musl build (pure Rust).
//!
//! musl's `malloc` takes a process-wide lock and returns memory to the system eagerly, which
//! made it 15–20 % of a conversion's CPU time, and it serialises the threads that style a
//! document and build its text layouts. This allocator has three tiers:
//!
//! - **small** blocks (at most [`MAX_SMALL`] bytes, the bulk of the DOM, style and text
//!   layout allocations) are taken from and returned to a per-thread cache of free blocks,
//!   one list per dlmalloc chunk size, without any lock. A thread refills an empty list, or
//!   gives back half of a full one, in a batch under the lock of the shared heap;
//! - **medium** blocks come from that shared [`dlmalloc`] heap, under its lock;
//! - **large** blocks (at least [`MIN_LARGE`] bytes: content streams, display-list chunks,
//!   script sources) are mapped from the system on their own and unmapped when freed, so the
//!   heap keeps no hole the size of the largest buffer a conversion ever had; `mremap` grows
//!   them without a copy on Linux.
//!
//! The vendored dlmalloc gives the whole pages of the chunks freed in it back to the system
//! (`vendor/dlmalloc`, `purge_freed`): plain dlmalloc only returns the top of its segments,
//! and kept the peak RSS of script-heavy pages 10 % above musl's.
//!
//! A block of a small class is exactly the chunk dlmalloc would have used for the request,
//! so the classes add no padding. A cached block can be reused only by the thread that holds
//! it; each list is capped ([`cache_cap`]), so a thread holds at most a few hundred KiB that
//! other threads cannot use. Caches of exited threads are not given back (the conversion's
//! threads live as long as the process).

use std::alloc::{GlobalAlloc, Layout};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::Mutex;

/// dlmalloc's alignment: requests with a larger one bypass the caches.
const ALIGN: usize = 16;
/// Largest small request. dlmalloc's chunk for `n` bytes is `(n + 8)` rounded up to 16
/// (at least 32); the usable part of a chunk of `16 * k` bytes is `16 * k - 8`.
pub const MAX_SMALL: usize = 1024 - 8;
/// Number of small classes: chunks of 32, 48, … 1024 bytes.
const CLASSES: usize = 1024 / 16 - 1;
/// Requests of this size or more are mapped on their own.
pub const MIN_LARGE: usize = 128 * 1024;
const PAGE: usize = 4096;

/// The class of a small request of `size` bytes (`size <= MAX_SMALL`).
#[inline]
fn class_of(size: usize) -> usize {
    // Chunk size / 16 - 2, chunks being at least 32 bytes.
    ((size + 8 + 15) >> 4).max(2) - 2
}

/// The usable size of the chunks of class `c`.
#[inline]
fn class_size(c: usize) -> usize {
    (c + 2) * 16 - 8
}

/// How many free blocks of class `c` a thread keeps: 4 KiB worth, between 4 and 64 (twice
/// that measured the same, for 0.2-0.3 MiB more peak RSS on small documents).
#[inline]
fn cache_cap(c: usize) -> usize {
    (4096 / (class_size(c) + 8)).clamp(4, 64)
}

/// Blocks of class `c` taken from the heap at once by an empty list.
#[inline]
fn refill_count(c: usize) -> usize {
    (cache_cap(c) / 2).max(1)
}

struct Heap(UnsafeCell<dlmalloc::Dlmalloc>);
// SAFETY: only accessed under `HEAP`'s mutex.
unsafe impl Send for Heap {}

static HEAP: Mutex<Heap> = Mutex::new(Heap(UnsafeCell::new(dlmalloc::Dlmalloc::new())));

fn with_heap<R>(f: impl FnOnce(&mut dlmalloc::Dlmalloc) -> R) -> R {
    let guard = HEAP.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the guard gives exclusive access.
    f(unsafe { &mut *guard.0.get() })
}

/// A free list of one class: blocks linked through their first word.
#[derive(Clone, Copy)]
struct FreeList {
    head: *mut u8,
    len: usize,
}

struct Cache {
    lists: [FreeList; CLASSES],
}

thread_local! {
    // No destructor and a constant initialiser: accessing it never allocates.
    static CACHE: UnsafeCell<Cache> = const {
        UnsafeCell::new(Cache { lists: [FreeList { head: ptr::null_mut(), len: 0 }; CLASSES] })
    };
}

/// The allocator. See the module documentation.
#[derive(Clone, Copy)]
pub struct WkAlloc;

impl WkAlloc {
    #[inline]
    unsafe fn alloc_small(c: usize) -> *mut u8 {
        let p = CACHE
            .try_with(|cache| {
                // SAFETY: the cache is only touched by its thread, never re-entrantly (the
                // code below does not allocate).
                let list = unsafe { &mut (*cache.get()).lists[c] };
                if list.head.is_null() {
                    unsafe { Self::refill(list, c) };
                }
                let p = list.head;
                if !p.is_null() {
                    list.head = unsafe { *(p as *mut *mut u8) };
                    list.len -= 1;
                }
                p
            })
            .unwrap_or(ptr::null_mut());
        if p.is_null() {
            // No cache (thread being torn down) or the heap is exhausted.
            with_heap(|h| unsafe { h.malloc(class_size(c), ALIGN) })
        } else {
            p
        }
    }

    /// Fill an empty list with a batch of blocks from the heap.
    #[cold]
    unsafe fn refill(list: &mut FreeList, c: usize) {
        let size = class_size(c);
        with_heap(|h| {
            for _ in 0..refill_count(c) {
                let p = unsafe { h.malloc(size, ALIGN) };
                if p.is_null() {
                    break;
                }
                unsafe { *(p as *mut *mut u8) = list.head };
                list.head = p;
                list.len += 1;
            }
        });
    }

    #[inline]
    unsafe fn dealloc_small(p: *mut u8, c: usize) {
        let cached = CACHE
            .try_with(|cache| {
                let list = unsafe { &mut (*cache.get()).lists[c] };
                if list.len >= cache_cap(c) {
                    unsafe { Self::drain(list, c) };
                }
                unsafe { *(p as *mut *mut u8) = list.head };
                list.head = p;
                list.len += 1;
            })
            .is_ok();
        if !cached {
            with_heap(|h| unsafe { h.free(p, class_size(c), ALIGN) });
        }
    }

    /// Give half of a full list back to the heap.
    #[cold]
    unsafe fn drain(list: &mut FreeList, c: usize) {
        let size = class_size(c);
        let keep = cache_cap(c) / 2;
        with_heap(|h| {
            while list.len > keep {
                let p = list.head;
                list.head = unsafe { *(p as *mut *mut u8) };
                list.len -= 1;
                unsafe { h.free(p, size, ALIGN) };
            }
        });
    }
}

/// Bytes mapped for a large block of `size` bytes.
#[inline]
fn map_len(size: usize) -> usize {
    (size + PAGE - 1) & !(PAGE - 1)
}

unsafe fn map(size: usize) -> *mut u8 {
    let p = unsafe {
        libc::mmap(
            ptr::null_mut(),
            map_len(size),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED { ptr::null_mut() } else { p.cast() }
}

unsafe fn unmap(p: *mut u8, size: usize) {
    unsafe { libc::munmap(p.cast(), map_len(size)) };
}

#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe fn remap(p: *mut u8, old: usize, new: usize) -> *mut u8 {
    let q = unsafe { libc::mremap(p.cast(), map_len(old), map_len(new), libc::MREMAP_MAYMOVE) };
    if q == libc::MAP_FAILED { ptr::null_mut() } else { q.cast() }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
unsafe fn remap(p: *mut u8, old: usize, new: usize) -> *mut u8 {
    if map_len(old) == map_len(new) {
        return p;
    }
    let q = unsafe { map(new) };
    if !q.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(p, q, old.min(new));
            unmap(p, old);
        }
    }
    q
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tier {
    Small(usize),
    Medium,
    Large,
}

#[inline]
fn tier(size: usize, align: usize) -> Tier {
    if align <= ALIGN && size <= MAX_SMALL {
        Tier::Small(class_of(size))
    } else if align <= PAGE && size >= MIN_LARGE {
        Tier::Large
    } else {
        Tier::Medium
    }
}

unsafe impl GlobalAlloc for WkAlloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match tier(layout.size(), layout.align()) {
            Tier::Small(c) => unsafe { Self::alloc_small(c) },
            Tier::Large => unsafe { map(layout.size()) },
            Tier::Medium => with_heap(|h| unsafe { h.malloc(layout.size(), layout.align()) }),
        }
    }

    #[inline]
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        match tier(layout.size(), layout.align()) {
            Tier::Small(c) => unsafe { Self::dealloc_small(p, c) },
            Tier::Large => unsafe { unmap(p, layout.size()) },
            Tier::Medium => with_heap(|h| unsafe { h.free(p, layout.size(), layout.align()) }),
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match tier(layout.size(), layout.align()) {
            Tier::Small(c) => {
                let p = unsafe { Self::alloc_small(c) };
                if !p.is_null() {
                    unsafe { ptr::write_bytes(p, 0, layout.size()) };
                }
                p
            }
            // Fresh mappings are zero-filled.
            Tier::Large => unsafe { map(layout.size()) },
            Tier::Medium => with_heap(|h| unsafe { h.calloc(layout.size(), layout.align()) }),
        }
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let (old, new) = (tier(layout.size(), layout.align()), tier(new_size, layout.align()));
        match (old, new) {
            (Tier::Small(a), Tier::Small(b)) if a == b => p,
            (Tier::Large, Tier::Large) => unsafe { remap(p, layout.size(), new_size) },
            (Tier::Medium, Tier::Medium) => {
                with_heap(|h| unsafe { h.realloc(p, layout.size(), layout.align(), new_size) })
            }
            _ => {
                // SAFETY: the caller guarantees `new_size` with this alignment is a valid layout.
                let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
                let q = unsafe { self.alloc(new_layout) };
                if !q.is_null() {
                    unsafe {
                        ptr::copy_nonoverlapping(p, q, layout.size().min(new_size));
                        self.dealloc(p, layout);
                    }
                }
                q
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_match_dlmalloc_chunks() {
        assert_eq!(class_of(0), 0);
        assert_eq!(class_of(24), 0);
        assert_eq!(class_of(25), 1);
        assert_eq!(class_of(40), 1);
        assert_eq!(class_of(MAX_SMALL), CLASSES - 1);
        for size in 0..=MAX_SMALL {
            let c = class_of(size);
            assert!(class_size(c) >= size);
            assert!(c == 0 || class_size(c - 1) < size, "{size}");
        }
    }

    #[test]
    fn alloc_realloc_free_across_tiers_and_threads() {
        let a = WkAlloc;
        let sizes =
            [1, 8, 24, 25, 100, 500, MAX_SMALL, MAX_SMALL + 1, 5000, MIN_LARGE - 1, MIN_LARGE, 300_000];
        let blocks: Vec<(usize, usize)> = std::thread::spawn(move || {
            let mut out = Vec::new();
            for (i, &s) in sizes.iter().enumerate() {
                for align in [1, 8, 16, 64, 4096] {
                    let l = Layout::from_size_align(s, align).unwrap();
                    let p = unsafe { if i % 2 == 0 { a.alloc(l) } else { a.alloc_zeroed(l) } };
                    assert!(!p.is_null());
                    assert_eq!(p as usize % align, 0);
                    if i % 2 == 1 {
                        assert!(unsafe { std::slice::from_raw_parts(p, s) }.iter().all(|&b| b == 0));
                    }
                    unsafe { ptr::write_bytes(p, (s % 251) as u8, s) };
                    out.push((p as usize, s * 8192 + align));
                }
            }
            out
        })
        .join()
        .unwrap();
        // Grow and shrink every block on another thread, checking the contents survive.
        for (p, key) in blocks {
            let (s, align) = (key / 8192, key % 8192);
            let l = Layout::from_size_align(s, align).unwrap();
            let mut p = p as *mut u8;
            let (mut cur, mut valid) = (s, s);
            for new in [s * 3 + 7, (s / 2).max(1), MIN_LARGE * 2, 3, s] {
                let q = unsafe { a.realloc(p, Layout::from_size_align(cur, align).unwrap(), new) };
                assert!(!q.is_null());
                assert_eq!(q as usize % align, 0);
                valid = valid.min(new);
                let bytes = unsafe { std::slice::from_raw_parts(q, valid) };
                assert!(bytes.iter().all(|&b| b == (s % 251) as u8), "{s} {align} {new}");
                p = q;
                cur = new;
            }
            assert_eq!(cur, s);
            unsafe { a.dealloc(p, l) };
        }
        // Enough small frees on one thread to drain its lists.
        let l = Layout::from_size_align(40, 8).unwrap();
        let ps: Vec<usize> = (0..10_000).map(|_| unsafe { a.alloc(l) } as usize).collect();
        for p in ps {
            unsafe { a.dealloc(p as *mut u8, l) };
        }
    }
}
