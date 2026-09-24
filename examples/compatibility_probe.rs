#[cfg(windows)]
use mh_sidebar::platform;
#[cfg(windows)]
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

#[cfg(windows)]
fn main() {
    assert!(
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } != 0
    );
    let monitors = platform::monitors();
    println!("Active monitors: {}", monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        println!(
            "{index}: primary={} scale={:.2} bounds={:?} work={:?} name={}",
            monitor.primary, monitor.scale, monitor.rect, monitor.work, monitor.name
        );
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Этот пример предназначен для Windows");
}
