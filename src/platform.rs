use crate::config::{Settings, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[derive(Debug, Clone)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub rect: ScreenRect,
    pub work: ScreenRect,
    pub scale: f32,
    pub primary: bool,
}

pub fn choose_monitor<'a>(list: &'a [Monitor], id: &str) -> Option<&'a Monitor> {
    list.iter()
        .find(|m| !id.is_empty() && m.id == id)
        .or_else(|| list.iter().find(|m| !m.primary))
        .or_else(|| list.iter().find(|m| m.primary))
        .or_else(|| list.first())
}

fn dock_rect(bounds: ScreenRect, logical_width: f32, scale: f32, side: Side) -> ScreenRect {
    let width = if logical_width.is_finite() {
        logical_width.max(1.0)
    } else {
        280.0
    };
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = ((width * scale).round() as i32).clamp(1, (bounds.right - bounds.left).max(1));
    match side {
        Side::Left => ScreenRect {
            right: bounds.left + width,
            ..bounds
        },
        Side::Right => ScreenRect {
            left: bounds.right - width,
            ..bounds
        },
    }
}

fn requested_rect(monitor: &Monitor, settings: &Settings) -> ScreenRect {
    let mut bounds = monitor.rect;
    // Keep the physical monitor edge, without overlapping a top/bottom taskbar.
    // The shell does not trim side AppBars against the taskbar, so do it for both modes.
    bounds.top = monitor.work.top.max(bounds.top);
    bounds.bottom = monitor.work.bottom.min(bounds.bottom);
    dock_rect(bounds, settings.width, monitor.scale, settings.side)
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        cell::Cell,
        marker::PhantomData,
        mem::size_of,
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
        rc::Rc,
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Registry::*, Threading::*},
        UI::{HiDpi::*, Shell::*, WindowsAndMessaging::*},
    };
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn string(s: &[u16]) -> String {
        String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
    }
    fn rect(r: RECT) -> ScreenRect {
        ScreenRect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
    fn win_rect(r: ScreenRect) -> RECT {
        RECT {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
    fn error(context: &str) -> String {
        format!("{context}: {}", std::io::Error::last_os_error())
    }

    pub fn monitors() -> Vec<Monitor> {
        unsafe extern "system" fn visit(
            handle: HMONITOR,
            _: HDC,
            _: *mut RECT,
            data: LPARAM,
        ) -> i32 {
            // Windows invokes this synchronously with the Vec pointer passed below.
            unsafe {
                let mut info: MONITORINFOEXW = std::mem::zeroed();
                info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
                if GetMonitorInfoW(handle, &mut info.monitorInfo) == 0 {
                    return 1;
                }
                let mut device: DISPLAY_DEVICEW = std::mem::zeroed();
                device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
                // EDD_GET_DEVICE_INTERFACE_NAME returns the persistent monitor interface identity.
                let found = EnumDisplayDevicesW(info.szDevice.as_ptr(), 0, &mut device, 1) != 0;
                let id = if found {
                    string(&device.DeviceID)
                } else {
                    String::new()
                };
                let fallback = string(&info.szDevice);
                let name = if found {
                    string(&device.DeviceString)
                } else {
                    fallback.clone()
                };
                let (mut x, mut y) = (96, 96);
                if GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut x, &mut y) < 0 {
                    x = 96;
                }
                (&mut *(data as *mut Vec<Monitor>)).push(Monitor {
                    id: if id.is_empty() { fallback.clone() } else { id },
                    name: format!("{} ({})", name, fallback),
                    rect: rect(info.monitorInfo.rcMonitor),
                    work: rect(info.monitorInfo.rcWork),
                    scale: x as f32 / 96.0,
                    primary: info.monitorInfo.dwFlags & 1 != 0,
                });
                1
            }
        }
        let mut result = Vec::new();
        unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(visit),
                &mut result as *mut _ as isize,
            );
        }
        result
    }

    struct CallbackState {
        dirty: Cell<bool>,
        applying: Cell<bool>,
        destroyed: Cell<bool>,
        shell_restart: Cell<bool>,
        activation: Cell<Option<bool>>,
        position_changed: Cell<bool>,
        callback: u32,
        taskbar_created: u32,
    }
    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        id: usize,
        data: usize,
    ) -> LRESULT {
        unsafe {
            let state = &*(data as *const CallbackState);
            if msg == WM_NCDESTROY {
                state.destroyed.set(true);
                RemoveWindowSubclass(hwnd, Some(subclass), id);
            } else if msg == state.taskbar_created {
                state.shell_restart.set(true);
                state.dirty.set(true);
            } else if msg == WM_ACTIVATE {
                state
                    .activation
                    .set(Some((w & 0xffff) != WA_INACTIVE as usize));
            } else if msg == WM_WINDOWPOSCHANGED {
                state.position_changed.set(true);
            } else if !state.applying.get()
                && ((msg == state.callback && w as u32 == ABN_POSCHANGED)
                    || matches!(msg, WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE))
            {
                state.dirty.set(true);
            }
            DefSubclassProc(hwnd, msg, w, l)
        }
    }
    /// Owns a GUI-thread-only subclass and the shell's registration of the real window.
    pub struct DockWindow {
        hwnd: HWND,
        state: Box<CallbackState>,
        registered: bool,
        registration_target: Option<(String, Side)>,
        installed: bool,
        last: Option<(ScreenRect, bool, bool, bool, bool)>,
        _gui_thread: PhantomData<Rc<()>>,
    }
    impl DockWindow {
        /// # Safety
        /// `hwnd` must be a live window owned by this thread. The returned wrapper
        /// must be dropped on this same thread before its owner is released.
        pub unsafe fn new(hwnd: HWND) -> Self {
            let state = Box::new(CallbackState {
                dirty: Cell::new(true),
                applying: Cell::new(false),
                destroyed: Cell::new(false),
                shell_restart: Cell::new(false),
                activation: Cell::new(None),
                position_changed: Cell::new(false),
                callback: unsafe { RegisterWindowMessageW(wide("MH-Sidebar.AppBar").as_ptr()) },
                taskbar_created: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
            });
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(subclass),
                    0x4d485342,
                    &*state as *const _ as usize,
                ) != 0
            };
            Self {
                hwnd,
                state,
                registered: false,
                registration_target: None,
                installed,
                last: None,
                _gui_thread: PhantomData,
            }
        }
        fn appbar(&self) -> APPBARDATA {
            APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                uCallbackMessage: self.state.callback,
                ..Default::default()
            }
        }
        fn remove(&mut self) {
            if self.registered {
                unsafe {
                    SHAppBarMessage(ABM_REMOVE, &mut self.appbar());
                }
                self.registered = false;
            }
            self.registration_target = None;
        }
        pub fn apply(&mut self, settings: &Settings, list: &[Monitor]) -> Result<(), String> {
            if !self.installed || self.state.callback == 0 || self.state.taskbar_created == 0 {
                return Err("Cannot install sidebar window callbacks".into());
            }
            if self.state.destroyed.get() {
                return Err("Sidebar window has been destroyed".into());
            }
            if self.state.shell_restart.replace(false) {
                self.registered = false;
                self.registration_target = None;
            }
            self.state.applying.set(true);
            let result = self.apply_inner(settings, list);
            self.state.applying.set(false);
            self.state.dirty.set(result.is_err());
            result
        }
        fn apply_inner(&mut self, settings: &Settings, list: &[Monitor]) -> Result<(), String> {
            unsafe {
                if !settings.visible || !settings.reserve_space {
                    self.remove();
                }
                let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                let mut wanted = (style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
                if settings.locked {
                    wanted |= (WS_EX_TRANSPARENT | WS_EX_LAYERED) as isize;
                } else {
                    wanted &= !(WS_EX_TRANSPARENT as isize);
                }
                if wanted != style {
                    SetLastError(0);
                    if SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, wanted) == 0 && GetLastError() != 0
                    {
                        return Err(error("Set sidebar styles"));
                    }
                }
                if settings.locked && SetLayeredWindowAttributes(self.hwnd, 0, 255, LWA_ALPHA) == 0
                {
                    return Err(error("Enable mouse passthrough"));
                }
                if !settings.visible {
                    ShowWindow(self.hwnd, SW_HIDE);
                    self.last = None;
                    return Ok(());
                }
                let monitor =
                    choose_monitor(list, &settings.monitor_id).ok_or("No monitor is available")?;
                let mut target = requested_rect(monitor, settings);
                if settings.reserve_space {
                    let destination = (monitor.id.clone(), settings.side);
                    if self.registration_target.as_ref() != Some(&destination) {
                        // Release the previous monitor/edge before negotiating the new one.
                        self.remove();
                    }
                    if !self.registered {
                        if SHAppBarMessage(ABM_NEW, &mut self.appbar()) == 0 {
                            return Err("Shell rejected sidebar AppBar registration".into());
                        }
                        self.registered = true;
                        self.registration_target = Some(destination);
                    }
                    let mut bar = self.appbar();
                    bar.uEdge = match settings.side {
                        Side::Left => ABE_LEFT,
                        Side::Right => ABE_RIGHT,
                    };
                    bar.rc = win_rect(target);
                    SHAppBarMessage(ABM_QUERYPOS, &mut bar);
                    let width = target.right - target.left;
                    match settings.side {
                        Side::Left => bar.rc.right = bar.rc.left + width,
                        Side::Right => bar.rc.left = bar.rc.right - width,
                    }
                    SHAppBarMessage(ABM_SETPOS, &mut bar);
                    target = rect(bar.rc);
                }
                let z = if settings.always_on_top {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };
                if SetWindowPos(
                    self.hwnd,
                    z,
                    target.left,
                    target.top,
                    target.right - target.left,
                    target.bottom - target.top,
                    SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
                ) == 0
                {
                    return Err(error("Position sidebar"));
                }
                self.last = Some((
                    requested_rect(monitor, settings),
                    settings.reserve_space,
                    settings.visible,
                    settings.always_on_top,
                    settings.locked,
                ));
                Ok(())
            }
        }
        pub fn tick(&mut self, settings: &Settings, list: &[Monitor]) -> Result<(), String> {
            if self.registered && !self.state.destroyed.get() && !self.state.shell_restart.get() {
                self.state.applying.set(true);
                let mut bar = self.appbar();
                unsafe {
                    if let Some(active) = self.state.activation.take() {
                        bar.lParam = isize::from(active);
                        SHAppBarMessage(ABM_ACTIVATE, &mut bar);
                    }
                    if self.state.position_changed.replace(false) {
                        SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut bar);
                    }
                }
                self.state.applying.set(false);
            } else {
                self.state.activation.set(None);
                self.state.position_changed.set(false);
            }
            let desired = if settings.visible {
                choose_monitor(list, &settings.monitor_id).map(|m| {
                    (
                        requested_rect(m, settings),
                        settings.reserve_space,
                        settings.visible,
                        settings.always_on_top,
                        settings.locked,
                    )
                })
            } else {
                None
            };
            if self.state.dirty.get() {
                // A display event may precede the UI's cached monitor list refresh.
                self.apply(settings, &monitors())
            } else if desired != self.last {
                self.apply(settings, list)
            } else {
                Ok(())
            }
        }
    }
    impl Drop for DockWindow {
        fn drop(&mut self) {
            self.state.applying.set(true);
            self.remove();
            if self.installed && !self.state.destroyed.get() {
                unsafe {
                    RemoveWindowSubclass(self.hwnd, Some(subclass), 0x4d485342);
                }
            }
        }
    }
    pub fn set_autostart(enabled: bool) -> Result<(), String> {
        // This function is only called after an explicit setting change, never by tests.
        let value = wide("MH-Sidebar");
        let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let mut key = null_mut();
        unsafe {
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                null(),
                0,
                KEY_SET_VALUE,
                null(),
                &mut key,
                null_mut(),
            );
            if status != ERROR_SUCCESS {
                return Err(format!("Open per-user startup key: {status}"));
            }
            let result = if enabled {
                match std::env::current_exe() {
                    Ok(exe) => {
                        let command: Vec<u16> = Some('"' as u16)
                            .into_iter()
                            .chain(exe.as_os_str().encode_wide())
                            .chain(['"' as u16, 0])
                            .collect();
                        RegSetValueExW(
                            key,
                            value.as_ptr(),
                            0,
                            REG_SZ,
                            command.as_ptr().cast(),
                            (command.len() * 2) as u32,
                        )
                    }
                    Err(e) => {
                        RegCloseKey(key);
                        return Err(format!("Find application executable: {e}"));
                    }
                }
            } else {
                RegDeleteValueW(key, value.as_ptr())
            };
            RegCloseKey(key);
            if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
                Ok(())
            } else {
                Err(format!("Change per-user startup entry: {result}"))
            }
        }
    }
    pub struct SingleInstance(HANDLE);
    impl SingleInstance {
        pub fn acquire() -> Option<Self> {
            unsafe {
                let handle = CreateMutexW(null(), 0, wide("Local\\MH-Sidebar").as_ptr());
                if handle.is_null() {
                    return None;
                }
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    CloseHandle(handle);
                    None
                } else {
                    Some(Self(handle))
                }
            }
        }
    }
    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl SingleInstance {
        /// Used after an elevated relaunch: the previous instance is still shutting down.
        pub fn acquire_waiting(timeout: std::time::Duration) -> Option<Self> {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                if let Some(instance) = Self::acquire() {
                    return Some(instance);
                }
                if std::time::Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }

    /// True when this process runs with an administrator token.
    pub fn is_elevated() -> bool {
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
        };
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut size = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }
    fn shell_execute(verb: &str, file: &str, parameters: &str) -> Result<(), String> {
        let result = unsafe {
            ShellExecuteW(
                null_mut(),
                wide(verb).as_ptr(),
                wide(file).as_ptr(),
                wide(parameters).as_ptr(),
                null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW reports success as a value greater than 32.
        if result as isize > 32 {
            Ok(())
        } else {
            Err(error(file))
        }
    }
    /// Starts a new elevated copy with the same arguments and the settings window open.
    /// The caller must exit so the new copy can take the single-instance lock.
    pub fn relaunch_elevated() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut arguments: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| a != "--settings" && a != "--wait-instance")
            .collect();
        arguments.extend(["--settings".into(), "--wait-instance".into()]);
        let parameters = arguments
            .iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(" ");
        shell_execute("runas", &exe.to_string_lossy(), &parameters)
    }
    /// Installs the PawnIO driver with winget in a visible console; winget asks for UAC itself.
    pub fn install_pawnio() -> Result<(), String> {
        shell_execute(
            "open",
            "cmd.exe",
            "/c winget install --id namazso.PawnIO --exact --accept-source-agreements --accept-package-agreements & pause",
        )
    }
    pub fn open_url(url: &str) -> Result<(), String> {
        shell_execute("open", url, "")
    }
}
#[cfg(windows)]
pub use native::*;

#[cfg(test)]
mod tests {
    use super::*;
    fn monitor(id: &str, primary: bool) -> Monitor {
        Monitor {
            id: id.into(),
            name: id.into(),
            rect: ScreenRect::default(),
            work: ScreenRect::default(),
            scale: 1.0,
            primary,
        }
    }
    #[test]
    fn negative_coordinates_and_fractional_dpi() {
        let bounds = ScreenRect {
            left: -1920,
            top: -1080,
            right: 0,
            bottom: 0,
        };
        assert_eq!(
            dock_rect(bounds, 280.0, 1.25, Side::Left),
            ScreenRect {
                right: -1570,
                ..bounds
            }
        );
        assert_eq!(
            dock_rect(bounds, 280.0, 1.5, Side::Right),
            ScreenRect {
                left: -420,
                ..bounds
            }
        );
    }
    #[test]
    fn missing_monitor_falls_back_to_secondary_then_primary() {
        let list = [monitor("primary", true), monitor("secondary", false)];
        assert_eq!(choose_monitor(&list, "missing").unwrap().id, "secondary");
        assert_eq!(choose_monitor(&list, "primary").unwrap().id, "primary");
        assert_eq!(choose_monitor(&list[..1], "missing").unwrap().id, "primary");
        assert!(choose_monitor(&[], "missing").is_none());
    }
    #[test]
    fn geometry_clamps_invalid_and_oversized_width() {
        let bounds = ScreenRect {
            left: 100,
            top: 50,
            right: 300,
            bottom: 450,
        };
        assert_eq!(dock_rect(bounds, 10000.0, 1.5, Side::Left), bounds);
        assert_eq!(dock_rect(bounds, -1.0, 1.0, Side::Right).left, 299);
        assert_eq!(dock_rect(bounds, f32::NAN, f32::NAN, Side::Left), bounds);
    }

    #[test]
    fn dock_respects_vertical_work_area_at_physical_monitor_edge() {
        let mut display = monitor("secondary", false);
        display.rect = ScreenRect {
            left: -1920,
            top: -1080,
            right: 0,
            bottom: 0,
        };
        // A horizontal inset may be our own previous reservation; do not reuse it.
        display.work = ScreenRect {
            left: -1600,
            top: -1040,
            right: 0,
            bottom: -48,
        };
        display.scale = 1.25;
        let settings = Settings {
            side: Side::Left,
            width: 280.0,
            reserve_space: false,
            ..Default::default()
        };
        assert_eq!(
            requested_rect(&display, &settings),
            ScreenRect {
                left: -1920,
                top: -1040,
                right: -1570,
                bottom: -48
            }
        );
        let settings = Settings {
            reserve_space: true,
            ..settings
        };
        assert_eq!(
            requested_rect(&display, &settings),
            ScreenRect {
                left: -1920,
                top: -1040,
                right: -1570,
                bottom: -48
            }
        );
    }
}
