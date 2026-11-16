//! Desktop shim over the extracted collaboration runtime.
//!
//! The runtime itself lives in `op-collab-host`, which is host-agnostic: it
//! knows neither winit nor `op-host-services`. This module supplies the two
//! desktop-specific pieces it asks for — the async → sync bridge and the
//! event-loop wake notifier — and keeps the `DesktopCollabRuntime` name the
//! rest of the binary already uses.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use op_collab_host::{BlockingExecutor, CollabWakeNotifier};
use winit::event_loop::EventLoopProxy;

use crate::DesktopEvent;

pub(crate) type DesktopCollabRuntime = op_collab_host::CollabRuntime;

/// Routes the runtime's relay-bridge and JWKS futures to the workspace's
/// single sanctioned async → sync bridge.
struct ServicesBlockingExecutor;

impl BlockingExecutor for ServicesBlockingExecutor {
    fn block_on_erased(&self, future: Pin<&mut (dyn Future<Output = ()> + '_)>) {
        op_host_services::chat_runtime::block_on_anywhere(future);
    }
}

/// Install the process-wide blocking executor. Idempotent — the first install
/// wins, so calling it from more than one entry point is safe.
pub(crate) fn install_blocking_executor() {
    let _ = op_collab_host::install_blocking_executor(Arc::new(ServicesBlockingExecutor));
}

/// Wrap the winit event-loop proxy as the runtime's wake notifier.
pub(crate) fn wake_notifier(proxy: EventLoopProxy<DesktopEvent>) -> CollabWakeNotifier {
    Arc::new(move || {
        let _ = proxy.send_event(DesktopEvent::CollabWake);
    })
}

/// The notifier every runtime this process builds is given. Each open document
/// can own a runtime, and a runtime that is not on screen still has to wake the
/// event loop when its network worker has something for it.
static WAKE_NOTIFIER: OnceLock<CollabWakeNotifier> = OnceLock::new();

/// Install the notifier [`new_runtime`] hands out. Call before the first
/// runtime is built; the first install wins. Tests install none: a runtime
/// without a notifier simply wakes nobody.
pub(crate) fn install_wake_notifier(notifier: CollabWakeNotifier) {
    let _ = WAKE_NOTIFIER.set(notifier);
}

/// A fresh idle runtime wired to the event loop. The ONE place a desktop
/// runtime is built: a document that starts a session keeps its own runtime,
/// so every runtime must carry the notifier or its events would sit unread
/// until something else woke the loop.
pub(crate) fn new_runtime() -> DesktopCollabRuntime {
    let mut runtime = DesktopCollabRuntime::new();
    if let Some(notifier) = WAKE_NOTIFIER.get() {
        runtime.set_wake_notifier(Arc::clone(notifier));
    }
    runtime
}

#[cfg(test)]
mod collab_host_tests;
