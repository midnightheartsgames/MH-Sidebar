# Windows integration task
Implement only src/platform.rs. Do not edit shared Cargo/lib/config files. Dependencies already include windows-sys 0.61.2. Reference modules may be inspected and selectively adapted with MIT attribution, but do not depend on reference Cargo packages. No drivers, service, elevation.

Required public API (consumed by root agent UI):
```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenRect { pub left:i32, pub top:i32, pub right:i32, pub bottom:i32 }
#[derive(Debug, Clone)]
pub struct Monitor { pub id:String, pub name:String, pub rect:ScreenRect, pub work:ScreenRect, pub scale:f32, pub primary:bool }
pub fn monitors() -> Vec<Monitor>;
pub fn choose_monitor<'a>(list:&'a [Monitor], id:&str) -> Option<&'a Monitor>;
pub struct DockWindow; // owns subclass/AppBar registration
impl DockWindow {
  pub fn new(hwnd: windows_sys::Win32::Foundation::HWND) -> Self;
  pub fn apply(&mut self, settings:&crate::config::Settings, monitors:&[Monitor]) -> Result<(),String>;
  pub fn tick(&mut self, settings:&crate::config::Settings, monitors:&[Monitor]) -> Result<(),String>;
}
pub fn set_autostart(enabled:bool) -> Result<(),String>;
pub struct SingleInstance; impl SingleInstance { pub fn acquire() -> Option<Self>; }
```
Settings fields: monitor_id:String (empty selects secondary then primary), side: config::Side enum Left/Right, width:f32 (logical px), reserve_space:bool, visible:bool, always_on_top:bool, locked:bool. Defaults width=280, visible=true, reserve_space=false, always_on_top=false, locked=false.

AppBar must use real sidebar HWND, query/set positions with chosen monitor physical bounds; remove reservation when hidden/off/Drop and restore on show, reserve correct DPI width; subclass callbacks mark dirty for ABN_POSCHANGED, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE and TaskbarCreated, no reentrant position loops. Position actual native HWND; use toolwindow and mouse passthrough styles, respect topmost false. Stable display ID from display device identity or QueryDisplayConfig, not enumerate index; human monitor label. Off-monitor fallback. Autostart only per-user Run key own value (quoted current exe), no invocation during tests. Single instance named Local\\MH-Sidebar with RAII.

Test pure geometry with negative X/Y and 125/150% widths, missing monitor selection. Run appropriate tests/compile (coordinate Cargo lock with root). Read installed SDK Rust bindings as authority. No commit. Report exact limitations and tests in docs/platform-report.md, then a short completion message.
