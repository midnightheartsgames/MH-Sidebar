# MH Sidebar Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for independent components and integration in this session.

**Goal:** Deliver a usable native Windows sidebar and MH-style settings with real telemetry.
**Architecture:** Single Cargo package with core config/model/history, platform Win32, background sensors, and egui UI modules. No reference project dependency.
**Tech Stack:** Rust 2024, eframe/egui 0.36 glow, windows-sys 0.61, sysinfo, NVML, tray-icon, global-hotkey, serde.
**Spec:** ../specs/2026-09-19-mh-sidebar-design.md

## Global Constraints
- Windows, normal user process, no bundled driver/service or PresentMon.
- Use local assets/fonts and MH palette; preserve reference directory.
- APPDATA/MH Sidebar/settings.json, schema_version 1; invalid/future config must not be silently overwritten.
- Test rates using actual elapsed time and counter resets; test docking with negative coordinates and DPI.

## Task 1: Core and persistent settings
Files: Cargo.toml, src/lib.rs, src/config.rs, src/model.rs, src/history.rs, tests/core.rs.
- [x] Add tests for validation, duplicate/missing block repair, history expiry, reset-safe rates and corrupt config preservation; run to establish failures.
- [x] Implement Settings, Block/Metric selection, Sample snapshots, History and atomic save/load.
- [x] Run cargo test --lib --test core.

## Task 2: Windows integration
Files: src/platform.rs, src/controls.rs, assets/app.manifest, build.rs.
Interface: Monitor { id, name, rect, work, scale, primary }; monitors() -> Vec<Monitor>; DockWindow::new(HWND); apply(&Settings, &[Monitor]); tick(&Settings, &[Monitor]); Drop removes reservation and subclass.
- [x] Test pure docking on both sides, negative coordinates, scaled width and missing monitor fallback.
- [x] Implement monitor identity, AppBar registration/query/set/remove and event-driven reposition; tray/hotkeys and opt-in registry autostart.
- [x] Verify Windows compilation and cleanup behavior.

## Task 3: Background telemetry
Files: src/sensors.rs. Interface Sampler::new(), sample() -> Snapshot. Each Snapshot contains Vec<Section>, each Section { id: Block, device, rows: Vec<Reading> }; Reading { key, label, value: Option<f64>, text, unit, reason }.
- [x] Add rate/reset tests and first-sample unknown checks.
- [x] Collect CPU/RAM, NVML GPU, disk/network counters; elapsed-time rates and periodic device refresh, reconnect failed providers.
- [x] Probe actual hardware, keep errors per field and return truthful unavailable values.

## Task 4: Native UI and integration
Files: src/main.rs, src/app.rs, src/theme.rs, src/sidebar.rs, src/settings_ui.rs.
- [x] Wire periodic background sampling with bounded history and egui repaint on new samples; ensure worker cancellation.
- [x] Build MH settings cards, live preview, sections, display picker, appearance, shortcuts, About cover and apply/cancel.
- [x] Render sidebar, graphs, tray actions, persistence and first-run settings. Build and launch debug executable for inspection.

## Task 5: Delivery and review
Files: README.md, tools/build-release.ps1, .github/workflows/ci.yml, LICENSE, THIRD-PARTY.md.
- [x] Run fmt, tests, clippy and sensor probe; resolve review findings.
- [x] Build portable release executable with manifest/version information and SHA256; document unavailable sensors and manual display/DPI tests.

## Execution ledger
Ruling: Work in the user's dedicated new-project directory. It has an unborn branch and untracked reference/assets only; a worktree has no committed source to isolate. No reference files will be edited or committed incidentally.
Ruling: Approved design is the specification; proceed without repeating approval gates or offering execution-method choices.
