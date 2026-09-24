#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod analytics_ui;
mod app;
#[cfg(debug_assertions)]
mod capture;
mod controls;
mod settings_files;
mod settings_ui;
mod sidebar;
mod theme;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("MH Sidebar: {error}");
        let text = format!("MH Sidebar не удалось запустить: {error}");
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                wide(&text).as_ptr(),
                wide("MH Sidebar").as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}
