use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use iocraft::prelude::*;

use crate::app::ViewKind;
use crate::components::text_input;
use crate::config::keybindings::BuiltinAction;
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
/// ```text
/// let event_channel = hooks.use_state(common::new_event_channel);
/// let (event_tx, event_rx_arc) = event_channel.read().clone();
/// ```
pub fn new_event_channel() -> EventChannel {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    (tx, Arc::new(Mutex::new(rx)))
}

/// Map a `GoTo*` keybinding action to its target `ViewKind`.
///
/// Returns `None` for non-`GoTo*` actions.
pub(crate) fn goto_target(action: BuiltinAction) -> Option<ViewKind> {
    match action {
        BuiltinAction::GoToPrs => Some(ViewKind::Prs),
        BuiltinAction::GoToIssues => Some(ViewKind::Issues),
        BuiltinAction::GoToActions => Some(ViewKind::Actions),
        BuiltinAction::GoToAlerts => Some(ViewKind::Alerts),
        BuiltinAction::GoToNotifications => Some(ViewKind::Notifications),
        BuiltinAction::GoToRepo => Some(ViewKind::Repo),
        _ => None,
    }
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

/// Resolve the final selection list from the current multiselect state.
///
/// If the input buffer is empty, returns the checked items as-is.
/// Otherwise resolves the current suggestion and merges it into the checked set.
fn resolve_selection(
    buf: &str,
    candidates: State<Vec<String>>,
    selection: State<usize>,
    checked: Vec<String>,
) -> Vec<String> {
    if buf.is_empty() {
        return checked;
    }
    let cands = candidates.read();
    let filtered = text_input::filter_suggestions(&cands, buf);
    let item = if filtered.is_empty() {
        buf.to_owned()
    } else {
        let sel = selection.get().min(filtered.len().saturating_sub(1));
        filtered[sel].clone()
    };
    if item.is_empty() {
        return checked;
    }
    let mut all = checked;
    if !all.contains(&item) {
        all.push(item);
    }
    all
}

/// Shared state for a multiselect-with-autocomplete input widget.
pub(crate) struct MultiSelectState {
    pub input_buffer: State<String>,
    pub candidates: State<Vec<String>>,
    pub selection: State<usize>,
    pub selected: State<Vec<String>>,
}

/// Generic keyboard handler for multiselect-with-autocomplete inputs.
///
/// Handles Tab/Down, Up/BackTab, Space toggle, Backspace, Char typing, Esc,
/// and Enter. On Enter the resolved selection is passed to `on_submit`; on both
/// Enter and Esc the `on_dismiss` callback is invoked to reset view-specific
/// state (e.g. `input_mode`).
pub(crate) fn handle_multiselect_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    ms: &mut MultiSelectState,
    on_submit: impl FnOnce(Vec<String>),
    mut on_dismiss: impl FnMut(),
) {
    let mut input_buffer = ms.input_buffer;
    let candidates = ms.candidates;
    let mut selection = ms.selection;
    let mut selected = ms.selected;
    match code {
        KeyCode::Tab | KeyCode::Down => {
            let buf = input_buffer.read().clone();
            let cands = candidates.read();
            let filtered = text_input::filter_suggestions(&cands, &buf);
            if !filtered.is_empty() {
                selection.set((selection.get() + 1) % filtered.len());
            }
        }
        KeyCode::Up | KeyCode::BackTab => {
            let buf = input_buffer.read().clone();
            let cands = candidates.read();
            let filtered = text_input::filter_suggestions(&cands, &buf);
            if !filtered.is_empty() {
                let sel = selection.get();
                selection.set(if sel == 0 {
                    filtered.len() - 1
                } else {
                    sel - 1
                });
            }
        }
        KeyCode::Char(' ') => {
            let buf = input_buffer.read().clone();
            let cands = candidates.read();
            let filtered = text_input::filter_suggestions(&cands, &buf);
            if !filtered.is_empty() {
                let sel = selection.get().min(filtered.len().saturating_sub(1));
                let item = filtered[sel].clone();
                let mut items = selected.read().clone();
                if let Some(pos) = items.iter().position(|s| s == &item) {
                    items.remove(pos);
                } else {
                    items.push(item);
                }
                selected.set(items);
            }
            input_buffer.set(String::new());
            selection.set(0);
        }
        KeyCode::Enter => {
            let buf = input_buffer.read().clone();
            let checked = selected.read().clone();
            let resolved = resolve_selection(&buf, candidates, selection, checked);
            on_submit(resolved);
            input_buffer.set(String::new());
            selection.set(0);
            selected.set(Vec::new());
            on_dismiss();
        }
        KeyCode::Esc => {
            input_buffer.set(String::new());
            selection.set(0);
            selected.set(Vec::new());
            on_dismiss();
        }
        KeyCode::Backspace => {
            let mut buf = input_buffer.read().clone();
            buf.pop();
            input_buffer.set(buf);
            selection.set(0);
        }
        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let mut buf = input_buffer.read().clone();
            buf.push(ch);
            input_buffer.set(buf);
            selection.set(0);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Mouse scroll helpers
// ---------------------------------------------------------------------------

/// Number of rows to scroll per mouse wheel tick.
pub const MOUSE_SCROLL_LINES: isize = 3;

/// Scroll the table viewport by `delta` rows without moving the selection
/// cursor. If the cursor falls outside the new visible window it is clamped to
/// the nearest visible row.
pub fn mouse_scroll_table(
    mut scroll_offset: State<usize>,
    mut cursor: State<usize>,
    total_rows: usize,
    visible_rows: usize,
    delta: isize,
) {
    if total_rows == 0 {
        return;
    }
    let max_offset = total_rows.saturating_sub(visible_rows);
    let current = scroll_offset.get();
    let new_offset = if delta > 0 {
        current
            .saturating_add(delta.cast_unsigned())
            .min(max_offset)
    } else {
        current.saturating_sub((-delta).cast_unsigned())
    };
    scroll_offset.set(new_offset);

    // Clamp cursor into the visible window.
    let cur = cursor.get();
    if cur < new_offset {
        cursor.set(new_offset);
    } else if cur >= new_offset + visible_rows {
        cursor.set((new_offset + visible_rows).saturating_sub(1));
    }
}

/// Scroll the sidebar viewport by `delta` rows.
///
/// Bounds are handled by the sidebar's existing render-time clamping in
/// `compute_visual_layout`, so we only need `saturating_add`/`saturating_sub`
/// here.
pub fn mouse_scroll_sidebar(mut preview_scroll: State<usize>, delta: isize) {
    let current = preview_scroll.get();
    let new_val = if delta > 0 {
        current.saturating_add(delta.cast_unsigned())
    } else {
        current.saturating_sub((-delta).cast_unsigned())
    };
    preview_scroll.set(new_val);
}
