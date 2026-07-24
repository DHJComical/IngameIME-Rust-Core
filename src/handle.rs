//! Opaque handle registry that replaces raw `Box::into_raw` pointers exposed to
//! Java as `jlong` handles.
//!
//! The previous design handed a raw `*mut ImeContext` to Java and reconstructed
//! `&`/`&mut` references from arbitrary `jlong` values. That allowed several
//! classes of undefined behaviour:
//!   - use-after-free when Java kept a stale handle past `destroy`,
//!   - double-free when `destroy` ran twice on the same handle,
//!   - arbitrary-address dereference when Java passed a forged/garbage `jlong`,
//!   - `&mut` aliasing UB when a callback re-entered JNI and produced a second
//!     mutable reference to the same context.
//!
//! Instead we hand out opaque `u64` IDs allocated from a global, never-reused
//! atomic counter. The actual `ImeContext` instances live in a thread-local
//! registry: the Windows TSF (STA thread affinity) and IMM32 (window thread
//! affinity) backends are inherently not `Send`, so a thread-local map matches
//! their affinity without any `unsafe impl Send`. Unknown, stale, or
//! cross-thread IDs simply fail the lookup and become safe no-ops.
//!
//! `with_context` removes the context from the map for the duration of the
//! closure, so a re-entrant call on the same handle finds nothing and returns
//! `None` rather than minting a second `&mut`. If a re-entrant callback destroys
//! the handle while it is borrowed, the destroy is deferred via
//! `pending_destroy` and applied when the borrow is returned, so the context is
//! never leaked nor freed early.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::context::ImeContext;

/// Opaque handle type exposed to Java as a `jlong`.
pub type Handle = u64;

/// Reserved handle meaning "no context" (matches the old null pointer / 0).
pub const INVALID_HANDLE: Handle = 0;

/// Global, monotonically increasing handle allocator. Starts at 1 so that 0
/// stays reserved as `INVALID_HANDLE`. IDs are never reused across the process.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Generic registry core, kept parametric over `T` so its logic can be unit
/// tested with lightweight placeholder types instead of a real `ImeContext`.
struct Registry<T> {
    map: HashMap<Handle, T>,
    borrowed: HashSet<Handle>,
    pending_destroy: HashSet<Handle>,
}

impl<T> Registry<T> {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            borrowed: HashSet::new(),
            pending_destroy: HashSet::new(),
        }
    }

    /// Store a value and return a freshly allocated handle.
    fn insert(&mut self, value: T) -> Handle {
        let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        self.map.insert(id, value);
        id
    }

    /// Remove a handle. Idempotent: a second call returns `false`. If the handle
    /// is currently borrowed by `with_context`, the destroy is deferred and this
    /// returns `true` (the value will be dropped when the borrow is returned).
    fn remove(&mut self, id: Handle) -> bool {
        if self.borrowed.contains(&id) {
            self.pending_destroy.insert(id);
            return true;
        }
        self.map.remove(&id).is_some()
    }

    /// Take a value out of the map for exclusive use, marking it as borrowed.
    fn checkout(&mut self, id: Handle) -> Option<T> {
        let value = self.map.remove(&id)?;
        self.borrowed.insert(id);
        Some(value)
    }

    /// Return a borrowed value. If it was destroyed while borrowed, drop it now;
    /// otherwise put it back into the map.
    fn checkin(&mut self, id: Handle, value: T) {
        self.borrowed.remove(&id);
        if self.pending_destroy.remove(&id) {
            drop(value);
            return;
        }
        self.map.insert(id, value);
    }
}

/// Run `f` against the value behind `id`, taking it out of the registry for the
/// duration so re-entrant lookups on the same `id` fail safely. The registry
/// borrow is released before `f` runs so `f` may re-enter the registry.
fn with_registry<T, R>(
    cell: &RefCell<Registry<T>>,
    id: Handle,
    f: impl FnOnce(&mut T) -> R,
) -> Option<R> {
    let mut value = cell.borrow_mut().checkout(id)?;
    let result = f(&mut value);
    cell.borrow_mut().checkin(id, value);
    Some(result)
}

thread_local! {
    static REGISTRY: RefCell<Registry<ImeContext>> = RefCell::new(Registry::new());
}

/// Register a context and return its opaque handle.
pub fn insert(context: ImeContext) -> Handle {
    REGISTRY.with(|cell| cell.borrow_mut().insert(context))
}

/// Destroy the context behind `id`. Idempotent; returns whether the handle was
/// known (or deferred while borrowed).
pub fn remove(id: Handle) -> bool {
    REGISTRY.with(|cell| cell.borrow_mut().remove(id))
}

/// Run `f` against the context behind `id`, or return `None` if the handle is
/// unknown, stale, from another thread, or currently borrowed (re-entrancy).
pub fn with_context<R>(id: Handle, f: impl FnOnce(&mut ImeContext) -> R) -> Option<R> {
    REGISTRY.with(|cell| with_registry(cell, id, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_returns_nonzero_and_increasing_ids() {
        let cell = RefCell::new(Registry::<u32>::new());
        let a = cell.borrow_mut().insert(10);
        let b = cell.borrow_mut().insert(20);
        assert_ne!(a, INVALID_HANDLE);
        assert_ne!(b, INVALID_HANDLE);
        assert_ne!(a, b);
        assert!(b > a);
    }

    #[test]
    fn with_registry_hits_known_and_misses_unknown() {
        let cell = RefCell::new(Registry::<u32>::new());
        let id = cell.borrow_mut().insert(1);

        let hit = with_registry(&cell, id, |v| {
            *v += 41;
            *v
        });
        assert_eq!(hit, Some(42));

        // Value mutation persisted back into the map.
        let again = with_registry(&cell, id, |v| *v);
        assert_eq!(again, Some(42));

        let miss = with_registry(&cell, 999_999, |v| *v);
        assert_eq!(miss, None);
    }

    #[test]
    fn remove_is_idempotent() {
        let cell = RefCell::new(Registry::<u32>::new());
        let id = cell.borrow_mut().insert(7);

        assert!(cell.borrow_mut().remove(id));
        assert_eq!(with_registry(&cell, id, |v| *v), None);
        // Second remove finds nothing.
        assert!(!cell.borrow_mut().remove(id));
    }

    #[test]
    fn reentrant_with_registry_on_same_id_returns_none() {
        let cell = RefCell::new(Registry::<u32>::new());
        let id = cell.borrow_mut().insert(5);

        let outer = with_registry(&cell, id, |_v| {
            // Same handle is checked out, so a nested borrow must miss.
            with_registry(&cell, id, |v| *v)
        });
        assert_eq!(outer, Some(None));
    }

    #[test]
    fn destroy_while_borrowed_is_deferred_then_applied() {
        let cell = RefCell::new(Registry::<u32>::new());
        let id = cell.borrow_mut().insert(3);

        let outer = with_registry(&cell, id, |_v| {
            // Destroy while the value is borrowed: deferred, reported as known.
            cell.borrow_mut().remove(id)
        });
        assert_eq!(outer, Some(true));

        // After the borrow is returned the handle no longer exists.
        assert_eq!(with_registry(&cell, id, |v| *v), None);
    }
}
