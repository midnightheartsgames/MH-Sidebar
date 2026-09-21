# MH Sidebar v0.1

User approved the first-version scope on 2026-09-19.

Native Windows Rust application, egui/glow, Cuprum Regular/Bold and the MH Monitoring palette. Independent project in the current empty repository; reference files remain untouched. A borderless sidebar docks to either edge of a selected monitor. Optional Windows AppBar reserves desktop space. Handle monitor changes, negative coordinates, DPI changes and Explorer restart. Settings use three columns: navigation, editable cards, live preview; apply/cancel semantics.

Display real CPU load/frequency/logical processors, GPU telemetry through user-mode interfaces, RAM, disk capacity/transfer rates, network rates and clock. Missing sensors display a reason, never fabricated values. Reorder and enable blocks and individual metrics; bounded histories and configurable polling. Tray and global shortcuts provide recovery from hidden/click-through mode. Persist versioned validated settings atomically under APPDATA; preserve corrupt files. Autostart is opt-in.

No administrator requirement, bundled sensor driver, PresentMon, ETW capture or service in v0.1. CPU thermal/power sensors deferred with user approval. Existing unused reference assets are not packaged. Cover used on About page, not stretched behind telemetry. Scope includes compiled portable executable and documented checks/limitations.

Verification: regression tests for monitor geometry, counter rates/reset, history, config validation/recovery; cargo fmt/check/test/clippy; real sensor probe; release build and interactive Windows smoke check when available.
