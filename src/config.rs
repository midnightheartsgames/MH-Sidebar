use crate::model::Block;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Side {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockConfig {
    pub id: Block,
    pub enabled: bool,
    pub show_name: bool,
    pub graph: bool,
    pub graph_metric: String,
    pub hidden_rows: Vec<String>,
    /// Empty selects all detected devices; otherwise exact device names.
    pub devices: Vec<String>,
}
impl Default for BlockConfig {
    fn default() -> Self {
        Self::new(Block::Cpu)
    }
}
impl BlockConfig {
    pub fn new(id: Block) -> Self {
        Self {
            id,
            enabled: true,
            show_name: true,
            graph: true,
            graph_metric: if id == Block::Network {
                "download".into()
            } else {
                "load".into()
            },
            hidden_rows: Vec::new(),
            devices: Vec::new(),
        }
    }
    pub fn shows(&self, key: &str) -> bool {
        !self.hidden_rows.iter().any(|k| k == key)
    }
    pub fn set_row(&mut self, key: &str, visible: bool) {
        self.hidden_rows.retain(|k| k != key);
        if !visible {
            self.hidden_rows.push(key.into());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub visibility: String,
    pub lock: String,
    pub settings: String,
}
impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            visibility: "Ctrl+Alt+F11".into(),
            lock: "Ctrl+Alt+F10".into(),
            settings: "Ctrl+Alt+F12".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub monitor_id: String,
    pub side: Side,
    pub width: f32,
    pub reserve_space: bool,
    pub visible: bool,
    pub always_on_top: bool,
    pub locked: bool,
    pub autostart: bool,
    pub interval_ms: u64,
    pub opacity: f32,
    pub font_size: f32,
    pub background: [u8; 3],
    pub accent: [u8; 3],
    pub show_header: bool,
    pub show_seconds: bool,
    pub clock_24h: bool,
    pub show_date: bool,
    pub hide_unavailable: bool,
    pub show_cores: bool,
    pub graph_seconds: u64,
    pub blocks: Vec<BlockConfig>,
    pub hotkeys: Hotkeys,
    pub alerts: bool,
    pub warning_temperature: f64,
    pub critical_temperature: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            monitor_id: String::new(),
            side: Side::Right,
            width: 280.,
            reserve_space: false,
            visible: true,
            always_on_top: false,
            locked: false,
            autostart: false,
            interval_ms: 1000,
            opacity: 0.94,
            font_size: 16.,
            background: [11, 11, 13],
            accent: [63, 208, 216],
            show_header: true,
            show_seconds: false,
            clock_24h: true,
            show_date: true,
            hide_unavailable: true,
            show_cores: false,
            graph_seconds: 60,
            blocks: Block::ALL.into_iter().map(BlockConfig::new).collect(),
            hotkeys: Hotkeys::default(),
            alerts: true,
            warning_temperature: 80.,
            critical_temperature: 90.,
        }
    }
}

pub struct Loaded {
    pub settings: Settings,
    pub notice: Option<String>,
    pub writable: bool,
}

pub fn default_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("MH Sidebar")
        .join("settings.json")
}

impl Settings {
    pub fn normalize(&mut self) {
        self.width = finite(self.width, 280.).clamp(220., 480.);
        self.font_size = finite(self.font_size, 16.).clamp(12., 26.);
        self.opacity = finite(self.opacity, 0.94).clamp(0.1, 1.);
        self.interval_ms = self.interval_ms.clamp(500, 5000);
        self.graph_seconds = self.graph_seconds.clamp(10, 300);
        if !self.warning_temperature.is_finite() {
            self.warning_temperature = 80.;
        }
        if !self.critical_temperature.is_finite() {
            self.critical_temperature = 90.;
        }
        self.warning_temperature = self.warning_temperature.clamp(30., 110.);
        self.critical_temperature = self
            .critical_temperature
            .clamp(self.warning_temperature + 1., 120.);
        let mut seen = Vec::new();
        self.blocks.retain(|b| {
            if seen.contains(&b.id) {
                false
            } else {
                seen.push(b.id);
                true
            }
        });
        for id in Block::ALL {
            if !seen.contains(&id) {
                self.blocks.push(BlockConfig::new(id));
            }
        }
    }
    pub fn block(&self, id: Block) -> Option<&BlockConfig> {
        self.blocks.iter().find(|b| b.id == id)
    }
    pub fn load(path: &Path) -> Loaded {
        let defaults = |notice, writable| Loaded {
            settings: Self::default(),
            notice,
            writable,
        };
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return defaults(None, true),
            Err(e) => return defaults(Some(format!("Не удалось прочитать настройки: {e}")), false),
        };
        let parsed = serde_json::from_slice::<serde_json::Value>(&bytes);
        if parsed
            .as_ref()
            .ok()
            .and_then(|v| v.get("schema_version"))
            .and_then(|v| v.as_u64())
            .is_some_and(|v| v > 1)
        {
            return defaults(Some("Файл создан более новой версией MH Sidebar. Сохранение отключено, исходный файл сохранён.".into()),false);
        }
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut settings) => {
                settings.normalize();
                defaults_loaded(settings)
            }
            Err(e) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let backup = path.with_file_name(format!("settings.corrupted-{stamp}.json"));
                match std::fs::rename(path, &backup) {
                    Ok(()) => defaults(
                        Some(format!(
                            "Повреждённые настройки сохранены в {}. Используются стандартные параметры. ({e})",
                            backup.display()
                        )),
                        true,
                    ),
                    Err(error) => defaults(
                        Some(format!(
                            "Настройки повреждены; резервную копию создать не удалось: {error}. Сохранение отключено."
                        )),
                        false,
                    ),
                }
            }
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut safe = self.clone();
        safe.normalize();
        let bytes = serde_json::to_vec_pretty(&safe).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temp = path.with_extension("tmp");
        let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let wide = |p: &Path| {
                p.as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>()
            };
            let from = wide(&temp);
            let to = wide(path);
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn finite(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}
fn defaults_loaded(settings: Settings) -> Loaded {
    Loaded {
        settings,
        notice: None,
        writable: true,
    }
}
