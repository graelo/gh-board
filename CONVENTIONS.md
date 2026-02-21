# Coding Conventions

Prescriptive rules for this codebase. Architecture diagrams and module
descriptions live in `ARCHITECTURE.md`.

---

## 1. View component structure

Each view file (`prs.rs`, `issues.rs`, `notifications.rs`, `repo.rs`) follows
the same internal layout:

```text
1. Column definitions (pure functions returning Vec<Column>)
2. Row builder   (fn foo_to_row(...) -> Row)
3. InputMode enum + PendingAction enum
4. Per-filter state structs (FilterData, FooState)
5. Props struct + #[component] fn
   a. Hook declarations (use_state, use_future for debounce, polling future)
   b. Lazy-fetch / refresh trigger
   c. hooks.use_terminal_events closure  ← keyboard handling lives here
   d. Early return for inactive views
   e. Render body (tabs, table, sidebar, footer, help overlay)
6. Helper functions (get_current_foo_info, build_foo_sidebar_meta, …)
```

---

## 2. Keyboard handler: inline-first

### The rule

Extract a mode handler into a standalone function **only if**:

1. The logic is **shared across multiple view files**, OR
2. It takes **≤ ~12 parameters** AND models a clearly bounded sub-state
   machine (e.g., autocomplete cycling, multi-step confirmation).

If neither condition holds, keep the code inline inside the
`use_terminal_events` closure.

### Why

Extracting a function with many parameters does not reduce complexity — it
spreads it across two locations. iocraft's `State<T>` handles are already the
encapsulation boundary; threading them through function arguments is redundant
ceremony. A function with 33 parameters is harder to read at the call site than
the equivalent inline block.

### Applied to the current modes

| Mode      | Where handled           | Rationale                             |
| --------- | ----------------------- | ------------------------------------- |
| `Normal`  | Inline (~350–430 lines) | Not shared; too many state handles    |
| `Search`  | Inline (~25 lines)      | Trivially small                       |
| `Confirm` | Inline (~20 lines)      | Trivially small                       |
| `Comment` | `handle_text_input`     | Bounded input sub-machine, ≤11 params |
| `Assign`  | `handle_assign_input`   | Autocomplete sub-machine, ≤12 params  |
| `Label`   | `handle_label_input`    | Autocomplete sub-machine, ≤12 params  |

### Target structure inside `use_terminal_events`

```rust
hooks.use_terminal_events({
    move |event| match event {
        TerminalEvent::Key(KeyEvent { code, kind, modifiers, .. })
            if kind != KeyEventKind::Release =>
        {
            if !is_active { return; }
            if help_visible.get() { /* intercept */ return; }

            match input_mode.read().clone() {
                InputMode::Comment  => handle_text_input(…),   // extracted: ≤11 params
                InputMode::Assign   => handle_assign_input(…), // extracted: ≤12 params
                InputMode::Label    => handle_label_input(…),  // extracted: ≤12 params
                InputMode::Confirm(ref pending) => match code { /* inline */ },
                InputMode::Search               => match code { /* inline */ },
                InputMode::Normal => {
                    let engine = engine.as_ref();  // shadow as &-ref for Copy semantics
                    let event_tx = &event_tx_kb;
                    match code {
                        // --- Navigation ---
                        // --- Actions ---
                        // --- Refresh ---
                        // --- Search ---
                        // --- Sidebar tab cycling ---
                        // --- Filter switching ---
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
});
```

Use section comments (`// --- Navigation ---`, etc.) inside the Normal arm to
keep the long match navigable. Prefer consistent section names across all three
view files.

---

## 3. iocraft state binding: always `mut` if `.set()` is called

`State<T>.set()` takes `&mut self`. A state handle that is mutated anywhere
in the component — including inside a `use_terminal_events` closure or a
`use_future` — **must be declared `mut`** at the `hooks.use_state(…)` call
site.

```rust
// Correct
let mut cursor = hooks.use_state(|| 0usize);
let mut input_mode = hooks.use_state(|| InputMode::Normal);

// Wrong — will fail to compile when .set() is called inside a closure
let cursor = hooks.use_state(|| 0usize);
```

Handles that are only read (`.get()`, `.read()`) may be immutable, but `mut`
on a read-only handle is harmless. When in doubt, declare `mut`.

---

## 4. Capturing `EngineHandle` in `'static` futures

`EngineHandle` is `Clone + 'static + Send`. The component receives it as
`Option<&'a EngineHandle>` from props. Clone it once per consumer at the top of
the component function, before any hooks:

```rust
let engine: Option<EngineHandle> = props.engine.cloned();
let engine_for_poll      = engine.clone();
let engine_for_debounce  = engine.clone();
let engine_for_keyboard  = engine.clone();  // moves into use_terminal_events
```

Inside a `use_terminal_events` closure (which requires `'static` captures),
shadow the handle as a shared reference before the inner `match`:

```rust
let engine = engine.as_ref();  // Option<&EngineHandle>, Copy — safe to use multiple times
```

Do **not** use `async_compat::Compat` anywhere in the UI layer.

---

## 5. Engine → UI reply channel

Each view owns exactly one `std::sync::mpsc` channel for engine replies,
created in a `use_state` initialiser so it survives re-renders:

```rust
let event_channel = hooks.use_state(|| {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    (tx, Arc::new(Mutex::new(rx)))
});
let (event_tx, event_rx_arc) = event_channel.read().clone();
```

Events are drained in a 100 ms polling future:

```rust
hooks.use_future(async move {
    loop {
        smol::Timer::after(Duration::from_millis(100)).await;
        let rx = event_rx_arc.lock().unwrap();
        while let Ok(ev) = rx.try_recv() { /* update state */ }
    }
});
```

Pass `event_tx.clone()` as `reply_tx` when sending a `Request` to the engine.
Never share a single channel between views.

---

## 6. Module visibility

- `src/types/` — `pub`; imported by both UI and engine.
- `src/github/` — `pub(crate)`; only `src/engine/github.rs` may import from it.
  UI layers import domain types from `crate::types::*`, never from
  `crate::github::*`.
- `src/engine/interface.rs` — `pub`; defines the contract (`EngineHandle`,
  `Request`, `Event`) consumed by all views.
- `src/components/` — `pub(crate)`; shared TUI widgets, not part of the public
  API.

---

## 7. No speculative complexity

- Do not extract helpers for one-time-use logic.
- Do not add error handling for scenarios that cannot occur.
- Do not design for hypothetical future requirements.
- Three similar lines of code are preferable to a premature abstraction.
- When removing code, remove it completely — do not leave `// removed` comments
  or unused `_`-prefixed variables as backwards-compatibility shims.
