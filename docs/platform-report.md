# Windows platform implementation report

Implemented `src/platform.rs` using the installed windows-sys 0.61.2 bindings directly; no reference code or dependencies copied.

- Monitor enumeration exposes Windows monitor device-interface IDs, human display names, physical screen/work bounds, effective DPI and primary flag. Requested missing monitor falls back to secondary then primary.
- Real sidebar HWND is registered as AppBar only while visible and reservation is enabled. Query/set geometry uses selected monitor physical coordinates and DPI-scaled width. Hide, disabling reservation, and Drop remove registration. Explorer restart causes registration to be renewed.
- Stable boxed GUI-thread-only subclass state defers AppBar/display/DPI/settings notifications to `tick`, suppressing callbacks caused by its own positioning. Window destruction removes the subclass; Drop unregisters and releases state.
- Native position, toolwindow style, layered mouse passthrough for lock, and explicit HWND_NOTOPMOST when always-on-top is disabled.
- Per-user startup modifies only HKCU Run value MH-Sidebar with the quoted current executable. Named Local\MH-Sidebar mutex closes its handle with RAII.

## Verification

`rustfmt --edition 2024 src/platform.rs` completed.

Because config.rs was still a stub during this subtask, compiled the complete platform module using an interface-only temporary harness under ignored target/platform-test.rs against the actual project's windows-sys rlib:

```
rustc --edition 2024 --test target/platform-test.rs --extern windows_sys=target/debug/deps/libwindows_sys-2dac7e6f1a00fc8e.rlib -L dependency=target/debug/deps -o target/platform-test.exe
.\target\platform-test.exe
```

Compilation succeeded. All 3 tests passed: negative X/Y with 125% and 150% widths; configured/missing/empty monitor selection; invalid and excessive width clamping. No startup registry modification or named mutex invocation was performed by tests. Root agent must run full Cargo integration after its config implementation is present.

## Limitations / live checks remaining

- AppBar reservation/restore, Explorer restart, actual click-through, taskbar coexistence and multi-monitor unplug/DPI changes require desktop validation; unit tests verify geometry and API compilation only.
- Normal display IDs use monitor device-interface identity. If Windows exposes no monitor interface (some virtual/remote displays), the GDI device name is used as a degraded fallback; Windows may renumber that fallback after topology changes.
- Effective DPI assumes the host window/process is per-monitor DPI aware (eframe/winit and application manifest responsibility).
- DockWindow must be created, ticked and dropped on its owning window thread; its type prevents Send/Sync.
- No second-instance activation IPC is included; acquire returns None for existing instance or OS mutex failure, as required by the supplied interface.
