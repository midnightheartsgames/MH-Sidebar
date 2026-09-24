use crate::model::{Block, Reading, Section, Snapshot};
use serde::{Deserialize, Serialize};
mod alerts;
mod profiles;
pub use alerts::AlertRule;
pub use profiles::{Density, PanelProfile, Preset};
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
    pub row_order: Vec<String>,
    pub devices: Vec<String>,
    pub device_ids: Vec<String>,
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
            row_order: Vec::new(),
            devices: Vec::new(),
            device_ids: Vec::new(),
        }
    }
    pub fn shows(&self, key: &str) -> bool {
        !self.hidden_rows.iter().any(|k| k == key)
    }
    pub fn all_devices(&self) -> bool {
        self.devices.is_empty() && self.device_ids.is_empty()
    }
    pub fn selects(&self, section: &Section) -> bool {
        self.id == section.id
            && (self.all_devices() || self.device_ids.contains(&section.device_id))
    }
    pub fn set_row(&mut self, key: &str, visible: bool) {
        self.hidden_rows.retain(|k| k != key);
        if !visible {
            self.hidden_rows.push(key.into());
        }
    }
    pub fn row_group(key: &str) -> &str {
        if key.starts_with("core_") {
            "core_*"
        } else {
            key
        }
    }
    pub fn row_position(&self, key: &str) -> usize {
        self.row_order
            .iter()
            .position(|saved| saved == Self::row_group(key))
            .unwrap_or(usize::MAX)
    }
    pub fn ordered_rows<'a>(&self, rows: &'a [Reading]) -> Vec<&'a Reading> {
        let mut ordered: Vec<_> = rows.iter().collect();
        ordered.sort_by_key(|row| self.row_position(&row.key));
        ordered
    }
    pub fn move_row(&mut self, key: &str, target: usize, available: &[String]) {
        if target >= available.len() {
            return;
        }
        let mut ordered = available.to_vec();
        ordered.sort_by_key(|key| self.row_position(key));
        if let Some(from) = ordered.iter().position(|current| current == key) {
            let moved = ordered.remove(from);
            ordered.insert(target, moved);
            ordered.extend(
                self.row_order
                    .iter()
                    .filter(|saved| !available.contains(saved))
                    .cloned(),
            );
            self.row_order = ordered;
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
pub struct TemperatureLimits {
    pub warning: f64,
    pub critical: f64,
}
impl Default for TemperatureLimits {
    fn default() -> Self {
        Self {
            warning: 80.,
            critical: 90.,
        }
    }
}
impl TemperatureLimits {
    fn normalize(&mut self) {
        if !self.warning.is_finite() {
            self.warning = 80.;
        }
        if !self.critical.is_finite() {
            self.critical = 90.;
        }
        self.warning = self.warning.clamp(30., 110.);
        self.critical = self.critical.clamp(self.warning + 1., 120.);
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
    pub density: Density,
    pub profiles: Vec<PanelProfile>,
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
    pub notifications_enabled: bool,
    pub alert_rules: Vec<AlertRule>,
    pub warning_temperature: f64,
    pub critical_temperature: f64,
    pub cpu_temperature: Option<TemperatureLimits>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 4,
            monitor_id: String::new(),
            side: Side::Right,
            width: 280.,
            reserve_space: false,
            visible: true,
            always_on_top: false,
            locked: false,
            autostart: true,
            interval_ms: 1000,
            opacity: 0.94,
            font_size: 16.,
            density: Density::default(),
            profiles: Vec::new(),
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
            notifications_enabled: false,
            alert_rules: Vec::new(),
            warning_temperature: 80.,
            critical_temperature: 90.,
            cpu_temperature: None,
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
        self.schema_version = 4;
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
        self.cpu_temperature
            .get_or_insert(TemperatureLimits {
                warning: self.warning_temperature,
                critical: self.critical_temperature,
            })
            .normalize();
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
        for block in &mut self.blocks {
            let mut seen_rows = Vec::new();
            block.row_order.retain(|key| {
                if key.is_empty() || seen_rows.contains(key) {
                    false
                } else {
                    seen_rows.push(key.clone());
                    true
                }
            });
        }
        for profile in &mut self.profiles {
            profile.normalize();
        }
        for rule in &mut self.alert_rules {
            rule.normalize();
        }
    }
    pub fn block(&self, id: Block) -> Option<&BlockConfig> {
        self.blocks.iter().find(|b| b.id == id)
    }
    pub fn resolve_devices(&mut self, snapshot: &Snapshot) -> bool {
        let mut changed = false;
        for block in &mut self.blocks {
            block.devices.retain(|name| {
                let candidates: Vec<_> = snapshot
                    .sections
                    .iter()
                    .filter(|s| s.id == block.id)
                    .collect();
                let mut matches: Vec<_> = candidates.iter().filter(|s| &s.device == name).collect();
                if block.id == Block::Gpu {
                    let base = name.split(" · ").next().unwrap_or(name);
                    matches = candidates
                        .iter()
                        .filter(|s| s.device.split(" · ").next() == Some(base))
                        .collect();
                }
                if matches.len() == 1 && !matches[0].device_id.starts_with("unstable:") {
                    let id = &matches[0].device_id;
                    if !block.device_ids.contains(id) {
                        block.device_ids.push(id.clone());
                    }
                    changed = true;
                    false
                } else {
                    true
                }
            });
        }
        changed
    }
    pub fn temperature_limits(&self, block: Block) -> Option<(f64, f64)> {
        match block {
            Block::Cpu => Some(self.cpu_temperature.as_ref().map_or(
                (self.warning_temperature, self.critical_temperature),
                |limits| (limits.warning, limits.critical),
            )),
            Block::Gpu => Some((self.warning_temperature, self.critical_temperature)),
            _ => None,
        }
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
            .is_some_and(|v| v > 4)
        {
            return defaults(Some("Файл создан более новой версией MH Sidebar. Сохранение отключено, исходный файл сохранён.".into()),false);
        }
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(mut settings) => {
                let old_schema = parsed
                    .as_ref()
                    .ok()
                    .and_then(|v| v.get("schema_version"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                settings.normalize();
                if old_schema < 4 {
                    let result = backup_before_migration(path, &bytes, old_schema);
                    if let Err(error) = result {
                        return Loaded {
                            settings,
                            writable: false,
                            notice: Some(format!(
                                "Не удалось сохранить настройки схемы {old_schema} перед миграцией: {error}. Сохранение отключено."
                            )),
                        };
                    }
                }
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

fn backup_before_migration(path: &Path, bytes: &[u8], schema: u64) -> std::io::Result<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..100 {
        let suffix = if attempt == 0 {
            format!("v{schema}.json")
        } else {
            format!("v{schema}-{stamp}-{attempt}.json")
        };
        let backup = path.with_extension(suffix);
        match std::fs::read(&backup) {
            Ok(existing) if existing == bytes => return Ok(()),
            Ok(_) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup);
        let mut file = match file {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let result = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(&backup);
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Не удалось выбрать имя резервной копии",
    ))
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
