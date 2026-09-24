use super::{Monitor, ScreenRect, choose_monitor, requested_rect};
use crate::config::Settings;
use eframe::egui::{self, ViewportCommand};
use std::{
    fs::{self, File, OpenOptions},
    os::fd::AsRawFd,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
pub fn monitors() -> Vec<Monitor> {
    core_graphics::display::CGDisplay::active_displays()
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            let display = core_graphics::display::CGDisplay::new(id);
            let bounds = display.bounds();
            let rect = ScreenRect {
                left: bounds.origin.x.round() as i32,
                top: bounds.origin.y.round() as i32,
                right: (bounds.origin.x + bounds.size.width).round() as i32,
                bottom: (bounds.origin.y + bounds.size.height).round() as i32,
            };
            Monitor {
                id: format!("display:{id}"),
                name: format!("Экран {id}"),
                rect,
                work: rect,
                scale: 1.0,
                primary: display.is_main(),
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn monitors() -> Vec<Monitor> {
    let Ok(output) = Command::new("xrandr").arg("--listmonitors").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            let geometry = fields.get(2)?;
            let (width, rest) = geometry.split_once('/')?;
            let (_, rest) = rest.split_once('x')?;
            let (height, rest) = rest.split_once('/')?;
            let offset = rest.find(['+', '-'])?;
            let coords = &rest[offset..];
            let split = coords[1..].find(['+', '-'])? + 1;
            let left: i32 = coords[..split].parse().ok()?;
            let top: i32 = coords[split..].parse().ok()?;
            let width: i32 = width.parse().ok()?;
            let height: i32 = height.parse().ok()?;
            let name = fields.last()?.to_string();
            let rect = ScreenRect {
                left,
                top,
                right: left + width,
                bottom: top + height,
            };
            Some(Monitor {
                id: name.clone(),
                name,
                rect,
                work: rect,
                scale: 1.0,
                primary: fields.get(1)?.contains('*'),
            })
        })
        .collect()
}

pub struct DockWindow {
    context: egui::Context,
    last: Option<(ScreenRect, bool)>,
}

impl DockWindow {
    pub fn new(context: egui::Context) -> Self {
        Self {
            context,
            last: None,
        }
    }

    pub fn tick(&mut self, settings: &Settings, list: &[Monitor]) -> Result<(), String> {
        let Some(monitor) = choose_monitor(list, &settings.monitor_id) else {
            return Ok(());
        };
        let rect = requested_rect(monitor, settings);
        if self.last != Some((rect, settings.always_on_top)) {
            self.context
                .send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(
                    rect.left as f32,
                    rect.top as f32,
                )));
            self.context
                .send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(
                    (rect.right - rect.left) as f32,
                    (rect.bottom - rect.top) as f32,
                )));
            self.context.send_viewport_cmd(ViewportCommand::WindowLevel(
                if settings.always_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                },
            ));
            self.last = Some((rect, settings.always_on_top));
        }
        Ok(())
    }
}

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

pub struct SingleInstance {
    _file: File,
}

impl SingleInstance {
    pub fn acquire() -> Option<Self> {
        let path = runtime_dir().join("instance.lock");
        fs::create_dir_all(path.parent()?).ok()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()?;
        (unsafe { flock(file.as_raw_fd(), 2 | 4) } == 0).then_some(Self { _file: file })
    }

    pub fn acquire_waiting(timeout: Duration) -> Option<Self> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(instance) = Self::acquire() {
                return Some(instance);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Не найден домашний каталог".into())
}

fn runtime_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(path).join("mh-sidebar");
    }
    home()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(".local/share/mh-sidebar")
}

pub fn legacy_autostart_exists() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".config"))
        .join("autostart");
    let path = dir.join("mh-sidebar.desktop");
    if !enabled {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let escaped = exe
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('%', "%%");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=MH Sidebar\nExec=\"{escaped}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    fs::write(path, entry).map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let dir = home()?.join("Library/LaunchAgents");
    let path = dir.join("games.midnighthearts.mh-sidebar.plist");
    if !enabled {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let escaped = exe
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    let entry = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>games.midnighthearts.mh-sidebar</string><key>ProgramArguments</key><array><string>{escaped}</string></array><key>RunAtLoad</key><true/></dict></plist>\n"
    );
    fs::write(path, entry).map_err(|error| error.to_string())
}

pub fn is_elevated() -> bool {
    false
}

pub fn relaunch_elevated() -> Result<(), String> {
    Err("На этой платформе запуск с повышенными правами не требуется".into())
}

pub fn install_pawnio() -> Result<(), String> {
    Err("PawnIO доступен только в Windows".into())
}

pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let command = "open";
    #[cfg(target_os = "linux")]
    let command = "xdg-open";
    Command::new(command)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
