//! KDE Plasma в сеансе Wayland.
//!
//! В Wayland приложение не может само выбрать позицию окна, слой «поверх всех» или скрыть
//! себя с панели задач. В Plasma это делает KWin: панель загружает в него небольшой скрипт
//! через D-Bus (`org.kde.KWin /Scripting`), а скрипт ставит окно к краю рабочей области
//! нужного экрана с учётом панелей Plasma. Список экранов берётся из `kscreen-doctor -j`.

use super::{Monitor, ScreenRect};
use crate::config::Side;
use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

const SCRIPT_NAME: &str = "mh-sidebar-dock";
pub const WINDOW_TITLE: &str = "MH Sidebar";

/// Сеанс Wayland под KDE Plasma (winit в нём выбирает Wayland, а не XWayland).
pub fn wayland_session() -> bool {
    let var = |name| std::env::var(name).unwrap_or_default();
    is_kde_wayland(&var("WAYLAND_DISPLAY"), &var("XDG_CURRENT_DESKTOP"))
}

pub fn is_kde_wayland(wayland_display: &str, current_desktop: &str) -> bool {
    !wayland_display.is_empty()
        && current_desktop
            .split(':')
            .any(|desktop| desktop.eq_ignore_ascii_case("kde"))
}

pub fn monitors() -> Vec<Monitor> {
    let Ok(output) = Command::new("kscreen-doctor").arg("-j").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_kscreen(&String::from_utf8_lossy(&output.stdout))
}

/// Экраны из JSON `kscreen-doctor -j` в логических координатах Wayland.
pub fn parse_kscreen(text: &str) -> Vec<Monitor> {
    let Some(start) = text.find('{') else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<Value>(&text[start..]) else {
        return Vec::new();
    };
    let Some(outputs) = json.get("outputs").and_then(Value::as_array) else {
        return Vec::new();
    };
    let size_of = |value: &Value| {
        Some((
            value.get("width")?.as_f64()?,
            value.get("height")?.as_f64()?,
        ))
    };
    let mut monitors: Vec<Monitor> = outputs
        .iter()
        .filter(|output| {
            output.get("enabled").and_then(Value::as_bool) != Some(false)
                && output.get("connected").and_then(Value::as_bool) != Some(false)
        })
        .filter_map(|output| {
            let name = output.get("name")?.as_str()?.to_owned();
            let pos = output.get("pos")?;
            let x = pos.get("x")?.as_f64()?;
            let y = pos.get("y")?.as_f64()?;
            let mode_id = |id: &Value| match id {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let current = output.get("currentModeId").map(mode_id);
            let mode = current.and_then(|current| {
                output
                    .get("modes")?
                    .as_array()?
                    .iter()
                    .find(|mode| mode.get("id").is_some_and(|id| mode_id(id) == current))
                    .and_then(|mode| size_of(mode.get("size")?))
            });
            let (mut width, mut height) = mode.or_else(|| size_of(output.get("size")?))?;
            // Rotation: 1 — нет, 2 — влево, 4 — вверх ногами, 8 — вправо.
            if matches!(output.get("rotation").and_then(Value::as_i64), Some(2 | 8)) {
                std::mem::swap(&mut width, &mut height);
            }
            let scale = output
                .get("scale")
                .and_then(Value::as_f64)
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(1.0);
            let primary = output.get("priority").and_then(Value::as_i64) == Some(1)
                || output.get("primary").and_then(Value::as_bool) == Some(true);
            let rect = ScreenRect {
                left: x.round() as i32,
                top: y.round() as i32,
                right: (x + width / scale).round() as i32,
                bottom: (y + height / scale).round() as i32,
            };
            Some(Monitor {
                id: name.clone(),
                name,
                rect,
                work: rect,
                // Координаты уже логические, ширину панели масштабировать не нужно.
                scale: 1.0,
                primary,
            })
        })
        .collect();
    monitors.sort_by_key(|m| (m.rect.left, m.rect.top));
    monitors
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Имя выхода (`DP-1`), на котором стоит панель.
    pub output: String,
    /// Прямоугольник на случай, если KWin не отдал рабочую область экрана.
    pub fallback: ScreenRect,
    pub width: f32,
    pub side: Side,
    pub keep_above: bool,
    pub visible: bool,
}

/// Скрипт KWin (ES5, чтобы работал и в Plasma 5, и в Plasma 6).
pub fn script(placement: &Placement, pid: u32) -> String {
    let output = serde_json::to_string(&placement.output).unwrap_or_else(|_| "\"\"".into());
    let title = serde_json::to_string(WINDOW_TITLE).unwrap_or_default();
    let width = if placement.width.is_finite() {
        placement.width.max(1.0).round() as i32
    } else {
        280
    };
    let rect = placement.fallback;
    format!(
        r#"// MH Sidebar: положение панели в сеансе Wayland. Скрипт выгружается при выходе из панели.
(function () {{
    var PID = {pid};
    var TITLE = {title};
    var OUTPUT = {output};
    var WIDTH = {width};
    var LEFT = {left_side};
    var KEEP_ABOVE = {keep_above};
    var VISIBLE = {visible};
    var FALLBACK = {{ x: {x}, y: {y}, width: {w}, height: {h} }};

    function target() {{
        try {{
            if (typeof KWin === "undefined" || !workspace.screens) {{
                return FALLBACK;
            }}
            var output = null;
            for (var i = 0; i < workspace.screens.length; i++) {{
                if (workspace.screens[i].name === OUTPUT) {{
                    output = workspace.screens[i];
                }}
            }}
            if (!output) {{
                return FALLBACK;
            }}
            var area = workspace.clientArea(KWin.MaximizeArea, output, workspace.currentDesktop);
            var width = Math.min(WIDTH, area.width);
            return {{
                x: LEFT ? area.x : area.x + area.width - width,
                y: area.y,
                width: width,
                height: area.height
            }};
        }} catch (error) {{
            return FALLBACK;
        }}
    }}

    function apply(window) {{
        if (!window || window.pid !== PID || window.caption !== TITLE) {{
            return;
        }}
        window.skipTaskbar = true;
        window.skipPager = true;
        window.skipSwitcher = true;
        window.onAllDesktops = true;
        window.keepBelow = false;
        window.keepAbove = KEEP_ABOVE;
        if (!VISIBLE) {{
            window.minimized = true;
            return;
        }}
        window.minimized = false;
        window.frameGeometry = target();
    }}

    function applyAll() {{
        var list = workspace.windowList ? workspace.windowList() : workspace.clientList();
        for (var i = 0; i < list.length; i++) {{
            apply(list[i]);
        }}
    }}

    applyAll();
    (workspace.windowAdded || workspace.clientAdded).connect(apply);
    var screensChanged = workspace.screensChanged || workspace.numberScreensChanged;
    if (screensChanged) {{
        screensChanged.connect(applyAll);
    }}
    if (workspace.currentDesktopChanged) {{
        workspace.currentDesktopChanged.connect(applyAll);
    }}
}})();
"#,
        left_side = placement.side == Side::Left,
        keep_above = placement.keep_above,
        visible = placement.visible,
        x = rect.left,
        y = rect.top,
        w = rect.right - rect.left,
        h = rect.bottom - rect.top,
    )
}

fn scripting(method: &str, args: &[String]) -> Result<String, String> {
    let output = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--dest=org.kde.KWin",
            "/Scripting",
            &format!("org.kde.kwin.Scripting.{method}"),
        ])
        .args(args)
        .output()
        .map_err(|error| format!("KWin недоступен: не удалось запустить dbus-send ({error})"))?;
    if !output.status.success() {
        return Err(format!(
            "KWin отклонил {method}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn unload() {
    let _ = scripting("unloadScript", &[format!("string:{SCRIPT_NAME}")]);
}

/// Держит загруженным в KWin скрипт с текущим положением панели.
pub struct KwinDock {
    last: Option<Placement>,
    loaded: bool,
    path: PathBuf,
}

impl Default for KwinDock {
    fn default() -> Self {
        Self::new()
    }
}

impl KwinDock {
    pub fn new() -> Self {
        Self {
            last: None,
            loaded: false,
            path: super::runtime_dir().join("kwin-dock.js"),
        }
    }

    pub fn apply(&mut self, placement: &Placement) -> Result<(), String> {
        if self.last.as_ref() == Some(placement) {
            return Ok(());
        }
        // Запоминаем сразу, чтобы при ошибке не перезагружать скрипт на каждом кадре.
        self.last = Some(placement.clone());
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        fs::write(&self.path, script(placement, std::process::id()))
            .map_err(|error| format!("Не удалось записать скрипт KWin: {error}"))?;
        unload();
        self.loaded = false;
        let reply = scripting(
            "loadScript",
            &[
                format!("string:{}", self.path.to_string_lossy()),
                format!("string:{SCRIPT_NAME}"),
            ],
        )?;
        let id: i64 = reply
            .split_whitespace()
            .last()
            .and_then(|n| n.parse().ok())
            .unwrap_or(-1);
        if id < 0 {
            return Err("KWin не загрузил скрипт размещения панели".into());
        }
        self.loaded = true;
        scripting("start", &[]).map(|_| ())
    }
}

impl Drop for KwinDock {
    fn drop(&mut self) {
        if self.loaded {
            unload();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plasma_wayland_only() {
        assert!(is_kde_wayland("wayland-0", "KDE"));
        assert!(is_kde_wayland("wayland-0", "ubuntu:KDE"));
        assert!(!is_kde_wayland("", "KDE"));
        assert!(!is_kde_wayland("wayland-0", "GNOME"));
        assert!(!is_kde_wayland("wayland-0", "Hyprland"));
    }

    #[test]
    fn parses_kscreen_outputs_in_logical_pixels() {
        let json = r#"kscreen.doctor: warning
{"outputs":[
 {"id":1,"name":"DP-1","enabled":true,"connected":true,"pos":{"x":0,"y":0},
  "size":{"width":3840,"height":2160},"scale":1.5,"rotation":1,"priority":1,
  "currentModeId":"3","modes":[{"id":"3","size":{"width":3840,"height":2160}}]},
 {"id":2,"name":"HDMI-A-1","enabled":true,"connected":true,"pos":{"x":2560,"y":0},
  "size":{"width":1920,"height":1080},"scale":1,"rotation":8,"priority":2,
  "currentModeId":"7","modes":[{"id":"7","size":{"width":1920,"height":1080}}]},
 {"id":3,"name":"eDP-1","enabled":false,"connected":true,"pos":{"x":0,"y":0},
  "size":{"width":1920,"height":1080},"scale":1}
]}"#;
        let list = parse_kscreen(json);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "DP-1");
        assert!(list[0].primary);
        assert_eq!(
            list[0].rect,
            ScreenRect {
                left: 0,
                top: 0,
                right: 2560,
                bottom: 1440
            }
        );
        assert_eq!(list[0].scale, 1.0);
        assert_eq!(list[1].id, "HDMI-A-1");
        assert!(!list[1].primary);
        assert_eq!(
            list[1].rect,
            ScreenRect {
                left: 2560,
                top: 0,
                right: 3640,
                bottom: 1920
            }
        );
        assert!(parse_kscreen("kscreen-doctor: not found").is_empty());
    }

    #[test]
    fn script_targets_only_the_sidebar_window() {
        let placement = Placement {
            output: "DP-\"1".into(),
            fallback: ScreenRect {
                left: 2280,
                top: 0,
                right: 2560,
                bottom: 1440,
            },
            width: 280.0,
            side: Side::Right,
            keep_above: true,
            visible: true,
        };
        let text = script(&placement, 4242);
        assert!(text.contains("var PID = 4242;"));
        assert!(text.contains(r#"var TITLE = "MH Sidebar";"#));
        assert!(text.contains(r#"var OUTPUT = "DP-\"1";"#));
        assert!(text.contains("var LEFT = false;"));
        assert!(text.contains("var KEEP_ABOVE = true;"));
        assert!(text.contains("x: 2280, y: 0, width: 280, height: 1440"));
        // ES5: без let/const и стрелочных функций, иначе Plasma 5 не загрузит скрипт.
        assert!(!text.contains("const ") && !text.contains("let ") && !text.contains("=>"));
    }
}
