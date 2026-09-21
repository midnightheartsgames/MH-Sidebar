# Independent correctness review

Reviewed the approved v0.1 spec and application, configuration, docking, controls, sensors, and UI code. Findings below are based on source inspection; interactive reproduction remains with the main agent. Line references are from the reviewed revision, before formatting/fixes.

1. **[P1] Reinstalling Controls disconnects every recovery action after a hotkey change** — `src/app.rs:98`, `src/controls.rs` event-handler installation. Change any hotkey in Settings and click Apply; then try a tray item, tray left click, or the new shortcut. `global-hotkey` 0.8, `muda` 0.20, and `tray-icon` 0.25 all store their handlers in `OnceCell` and silently ignore subsequent `set_event_handler` calls. The first handlers still capture the old command sender (whose receiver was dropped), and menu/hotkey handlers additionally retain the old IDs/bindings. The new controls therefore cannot deliver commands. If the panel is hidden or click-through this loses the required recovery route. Keep a stable handler/channel and update registrations in the existing controls thread, or route the one-time handlers through replaceable shared state. Validate changing shortcuts twice, then tray visibility/settings/exit and both retained/new hotkeys.

2. **[P2] Failed WDDM memory query becomes a fabricated zero capacity** — `src/sensors/gpu.rs:124`, `src/sensors.rs:363`. `KMTQAITYPE_GETSEGMENTSIZE` failure is collapsed with `map_or(0, ...)`, and the sidebar emits that zero as `Some(0.0)` GiB. A driver that supports adapter enumeration but rejects this query displays a supposedly valid zero and cannot explain the missing sensor, contrary to the explicit unavailable-data contract. Preserve `Option<u64>` through `AdapterInfo` and the reading, while retaining an actual successful zero for integrated adapters. Validate with an injected failed segment query and a successful zero-memory result.

No additional high-confidence docking/persistence defects were established during this pass. This is not a substitute for the planned Windows interaction and Explorer-restart smoke checks.

## Resolution by implementer

- P1 fixed: Controls stays alive; a message reconfigures OS hotkey registration on its owning thread, and the receiver dispatches through the current binding list. Regression test sends native WM_HOTKEY to this process's manager, changes bindings twice, verifies removed bindings do not dispatch and new bindings reach the original command receiver. Passed.
- P2 fixed: AdapterInfo preserves Option<u64>; a failed segment query renders unavailable, while a successful zero is retained.
- Native dock_probe passed outside the isolated sandbox against real Explorer: right reservation, transfer to left, hide/show and RAII restoration. Working area restored exactly.
- Actual egui framebuffer screenshots of sidebar and settings inspected. Additional manual hot-plug, live DPI and Explorer restart tests remain documented limitations.
- Native GUI smoke passed (Reset/Cancel, reopen with header button, Reset/Apply, hide). Background context-menu hit area is registered before child controls so it cannot cover their hit targets. Test clicks verify the target window first and bring the test window above any existing topmost sidebar.
