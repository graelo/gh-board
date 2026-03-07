use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use iocraft::prelude::*;

use crate::engine::Event;

/// Type alias for the event channel pair used by every view.
///
/// The `Sender` is cloned into each `Request` so the engine can reply.
/// The `Arc<Mutex<Receiver>>` is polled every 100ms in a `use_future` hook.
pub type EventChannel = (Sender<Event>, Arc<Mutex<std::sync::mpsc::Receiver<Event>>>);

/// Create a per-view event channel and unpack it into its two halves.
///
/// Call this exactly once per view component, inside a `hooks.use_state`:
///
/// ```ignore
/// let event_channel = hooks.use_state(common::new_event_channel);
/// let (event_tx, event_rx_arc) = event_channel.read().clone();
/// ```
pub fn new_event_channel() -> EventChannel {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    (tx, Arc::new(Mutex::new(rx)))
}

/// Mark a filter index as in-flight (or clear it).
///
/// This is the canonical way to update `filter_in_flight` state — it avoids
/// the repetitive clone-mutate-set dance that appears dozens of times across
/// the view layer.
pub fn set_in_flight(state: &mut State<Vec<bool>>, idx: usize, value: bool) {
    let mut v = state.read().clone();
    if idx < v.len() {
        v[idx] = value;
    }
    state.set(v);
}
