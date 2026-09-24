# Usability v0.3

Continuation of the approved roadmap, in the current workspace on top of v0.2.

Profiles store presentation and block selections; applying one preserves monitor placement,
autostart, hotkeys and visibility. Three built-in presets and named user profiles operate
on the settings draft. Full JSON import also replaces only the draft; export explicitly
exports that draft through a native Save dialog. Schema 3 protects these fields from
older applications and backs up the source schema before migration.

Density changes vertical spacing. Blocks can be dragged by a handle or moved by arrows.
Hotkey recording uses keyboard events, with global registrations suspended during capture
and restored when capture ends or the settings window closes. Escape cancels capture.

- [x] Test and implement profiles, schema migration and bounded import.
- [x] Add profiles/import/export UI and native file dialogs.
- [x] Implement density, drag ordering and shortcut recording.
- [x] Run tests, lint/build, independent review and package v0.3.

## Outcome — 2026-09-23

The automated suite passed 58 tests, with five hardware tests left opt-in. Clippy with
warnings denied, debug and release builds passed. A normal desktop smoke process exited
with exit code 0 and a tray-icon removal diagnostic on stderr. Own-framebuffer screenshots of the profiles settings and sidebar were
inspected. The independent review found a lost Win modifier in hotkey capture; this was
fixed and the reviewer confirmed the fix. Native file dialogs and mouse drag gestures
have not been exercised interactively. No commit or existing user process was changed.
