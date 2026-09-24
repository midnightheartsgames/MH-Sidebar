use mh_sidebar::{
    config::{Settings, Side},
    platform::{self, DockWindow},
};
use std::{ptr::null_mut, time::Duration};
use windows_sys::Win32::{
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};
fn pause() {
    unsafe {
        let mut msg = std::mem::zeroed();
        for _ in 0..4 {
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
fn main() {
    unsafe {
        assert!(SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0);
        let class: Vec<u16> = "MHSidebarDockProbe".encode_utf16().chain(Some(0)).collect();
        let module = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: module,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        assert!(RegisterClassW(&wc) != 0);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            280,
            500,
            null_mut(),
            null_mut(),
            module,
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        let before = platform::monitors();
        let monitor = if std::env::args().any(|arg| arg == "--primary") {
            before.iter().find(|monitor| monitor.primary).unwrap()
        } else {
            platform::choose_monitor(&before, "").unwrap()
        }
        .clone();
        let mut settings = Settings {
            monitor_id: monitor.id.clone(),
            reserve_space: true,
            always_on_top: false,
            ..Default::default()
        };
        let mut window = DockWindow::new(hwnd);
        window.apply(&settings, &before).unwrap();
        pause();
        let current = platform::monitors();
        let after = platform::choose_monitor(&current, &monitor.id).unwrap();
        println!("before {:?}, reserved {:?}", monitor.work, after.work);
        assert!(
            after.work.right < monitor.work.right,
            "reservation must reduce usable width"
        );
        settings.side = Side::Left;
        window.apply(&settings, &current).unwrap();
        pause();
        let current = platform::monitors();
        let left = platform::choose_monitor(&current, &monitor.id).unwrap();
        println!("left {:?}", left.work);
        assert!(left.work.left > monitor.work.left);
        assert_eq!(left.work.right, monitor.work.right);
        settings.visible = false;
        window.apply(&settings, &current).unwrap();
        pause();
        let current = platform::monitors();
        assert_eq!(
            platform::choose_monitor(&current, &monitor.id)
                .unwrap()
                .work,
            monitor.work,
            "hiding must release work area"
        );
        settings.visible = true;
        window.apply(&settings, &current).unwrap();
        pause();
        drop(window);
        pause();
        let current = platform::monitors();
        assert_eq!(
            platform::choose_monitor(&current, &monitor.id)
                .unwrap()
                .work,
            monitor.work,
            "Drop must restore work area"
        );
        DestroyWindow(hwnd);
        UnregisterClassW(class.as_ptr(), module);
        println!("AppBar right/left/hide/show/drop checks passed");
    }
}
