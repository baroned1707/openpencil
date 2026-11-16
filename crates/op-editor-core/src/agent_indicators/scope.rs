//! Per-document scoping of the indicator registry.
//!
//! The registry used to be one process-wide [`AgentIndicators`], so a run begun
//! in one document cleared the indicators of another. It now holds one
//! `AgentIndicators` per *scope* (the desktop uses the open document's id).
//! Two rules keep every existing call site working unchanged:
//!
//! - Calls that carry an epoch (`add_node`, `finish_if_epoch`, ...) find the
//!   scope that owns that epoch. Epochs come from ONE process-wide counter, so
//!   an epoch names exactly one run in exactly one scope, and a worker thread
//!   registering for a run in a parked document lands in that document's
//!   indicators whatever is on screen.
//! - Calls without an epoch (`begin`, `snapshot`, `active_epoch`, ...) address
//!   the calling thread's *current scope*. It is `0` until a host sets it, which
//!   is the old single-registry behaviour (web, FFI, daemon). The desktop UI
//!   thread sets it to the document on screen, and to a parked document for the
//!   duration of pumping that document's run ([`enter_scope`]).

use super::AgentIndicators;
use std::cell::Cell;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Every scope's indicators.
#[derive(Default)]
pub(super) struct Registry {
    scopes: HashMap<u64, AgentIndicators>,
}

pub(super) static REGISTRY: LazyLock<Mutex<Registry>> =
    LazyLock::new(|| Mutex::new(Registry::default()));

/// Source of run epochs. `0` is "never begun", so the first epoch is `1`.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// A fresh epoch that no run has held or will hold again.
pub(super) fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

thread_local! {
    static CURRENT_SCOPE: Cell<u64> = const { Cell::new(0) };
}

/// The scope epoch-less calls on this thread address.
pub fn current_scope() -> u64 {
    CURRENT_SCOPE.with(Cell::get)
}

/// Point this thread's epoch-less calls at `scope` (the document on screen).
pub fn set_current_scope(scope: u64) {
    CURRENT_SCOPE.with(|current| current.set(scope));
}

/// Restores the previous current scope when dropped.
#[must_use = "the previous scope returns when this guard is dropped"]
pub struct ScopeGuard {
    previous: u64,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        set_current_scope(self.previous);
    }
}

/// Address `scope` until the returned guard drops.
pub fn enter_scope(scope: u64) -> ScopeGuard {
    let previous = current_scope();
    set_current_scope(scope);
    ScopeGuard { previous }
}

/// Forget a scope's indicators (its document was closed).
pub fn drop_scope(scope: u64) {
    REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .scopes
        .remove(&scope);
}

/// One scope's indicators, held under the registry lock.
pub(super) struct Locked<'a> {
    guard: MutexGuard<'a, Registry>,
    scope: u64,
}

impl Deref for Locked<'_> {
    type Target = AgentIndicators;

    fn deref(&self) -> &AgentIndicators {
        &self.guard.scopes[&self.scope]
    }
}

impl DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut AgentIndicators {
        self.guard
            .scopes
            .get_mut(&self.scope)
            .expect("scope was inserted when the lock was taken")
    }
}

fn lock_registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap()
}

/// The current scope's indicators.
pub(super) fn lock_current() -> Locked<'static> {
    let scope = current_scope();
    let mut guard = lock_registry();
    guard.scopes.entry(scope).or_default();
    Locked { guard, scope }
}

/// The indicators of the run holding `epoch`, or `None` when no scope holds it
/// (the run was retired or replaced). The current scope is tried first, which
/// is what keeps the never-begun epoch `0` addressing the current scope.
pub(super) fn lock_epoch(epoch: u64) -> Option<Locked<'static>> {
    let current = current_scope();
    let guard = lock_registry();
    let scope = if guard
        .scopes
        .get(&current)
        .is_some_and(|ind| ind.epoch == epoch)
    {
        current
    } else if epoch != 0 {
        guard
            .scopes
            .iter()
            .find_map(|(scope, ind)| (ind.epoch == epoch).then_some(*scope))?
    } else {
        return None;
    };
    Some(Locked { guard, scope })
}

/// The confirmed agent of the live run `epoch`, wherever that run lives. For
/// worker threads, whose epoch-less calls would address the wrong scope.
pub fn cursor_agent_for_epoch(epoch: u64) -> Option<super::AgentTag> {
    let ind = lock_epoch(epoch)?;
    if ind.run_active {
        ind.cursor_agent.clone()
    } else {
        None
    }
}
