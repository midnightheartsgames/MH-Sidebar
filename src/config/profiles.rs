use super::{BlockConfig, Settings};
use crate::model::Block;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density {
    #[default]
    Compact,
    Normal,
    Spacious,
}
impl Density {
    pub fn spacing(self) -> f32 {
        match self {
            Self::Compact => 2.,
            Self::Normal => 5.,
            Self::Spacious => 8.,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Compact => "Компактно",
            Self::Normal => "Обычно",
            Self::Spacious => "Свободно",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Preset {
    Minimal,
    Gaming,
    Work,
}
impl Preset {
    pub fn title(self) -> &'static str {
        match self {
            Self::Minimal => "Минимальный",
            Self::Gaming => "Игровой",
            Self::Work => "Рабочий",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelProfile {
    pub name: String,
    pub blocks: Vec<BlockConfig>,
    pub width: f32,
    pub font_size: f32,
    pub density: Density,
    pub opacity: f32,
    pub background: [u8; 3],
    pub accent: [u8; 3],
    pub show_header: bool,
    pub show_cores: bool,
    pub hide_unavailable: bool,
    pub graph_seconds: u64,
    pub show_seconds: bool,
    pub clock_24h: bool,
    pub show_date: bool,
}
impl Default for PanelProfile {
    fn default() -> Self {
        Self::capture("", &Settings::default())
    }
}
impl PanelProfile {
    fn capture(name: &str, s: &Settings) -> Self {
        Self {
            name: name.trim().to_owned(),
            blocks: s.blocks.clone(),
            width: s.width,
            font_size: s.font_size,
            density: s.density,
            opacity: s.opacity,
            background: s.background,
            accent: s.accent,
            show_header: s.show_header,
            show_cores: s.show_cores,
            hide_unavailable: s.hide_unavailable,
            graph_seconds: s.graph_seconds,
            show_seconds: s.show_seconds,
            clock_24h: s.clock_24h,
            show_date: s.show_date,
        }
    }
    fn apply(&self, s: &mut Settings) {
        s.blocks = self.blocks.clone();
        s.width = self.width;
        s.font_size = self.font_size;
        s.density = self.density;
        s.opacity = self.opacity;
        s.background = self.background;
        s.accent = self.accent;
        s.show_header = self.show_header;
        s.show_cores = self.show_cores;
        s.hide_unavailable = self.hide_unavailable;
        s.graph_seconds = self.graph_seconds;
        s.show_seconds = self.show_seconds;
        s.clock_24h = self.clock_24h;
        s.show_date = self.show_date;
    }
    pub(super) fn normalize(&mut self) {
        let mut s = Settings::default();
        self.apply(&mut s);
        s.normalize();
        *self = Self::capture(&self.name, &s);
    }
}
impl Settings {
    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1_048_576 {
            return Err("Файл настроек больше 1 МиБ".into());
        }
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("Некорректный JSON: {e}"))?;
        if !value.is_object() {
            return Err("Ожидается объект настроек".into());
        }
        if value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .is_some_and(|v| v > 4)
        {
            return Err("Настройки созданы более новой версией MH Sidebar".into());
        }
        let mut s: Self =
            serde_json::from_value(value).map_err(|e| format!("Некорректные настройки: {e}"))?;
        s.normalize();
        Ok(s)
    }
    pub fn move_block(&mut self, id: Block, target: usize) {
        if target >= self.blocks.len() {
            return;
        }
        if let Some(from) = self.blocks.iter().position(|b| b.id == id) {
            let block = self.blocks.remove(from);
            self.blocks.insert(target, block);
        }
    }
    pub fn save_profile(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("Название профиля: от 1 до 64 символов".into());
        }
        let mut profile = PanelProfile::capture(name, self);
        profile.normalize();
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name == name) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        Ok(())
    }
    pub fn apply_profile(&mut self, index: usize) -> Result<(), String> {
        let profile = self.profiles.get(index).ok_or("Профиль не найден")?.clone();
        profile.apply(self);
        self.normalize();
        Ok(())
    }
    pub fn apply_preset(&mut self, preset: Preset) {
        let mut s = Settings::default();
        match preset {
            Preset::Minimal => {
                s.width = 240.;
                s.font_size = 14.;
                s.show_date = false;
                for b in &mut s.blocks {
                    b.enabled = matches!(b.id, Block::Cpu | Block::Memory | Block::Clock);
                    b.graph = false;
                    b.show_name = false;
                }
            }
            Preset::Gaming => {
                s.width = 300.;
                s.density = Density::Normal;
                s.graph_seconds = 120;
                for b in &mut s.blocks {
                    b.enabled = matches!(b.id, Block::Cpu | Block::Gpu | Block::Memory);
                }
            }
            Preset::Work => {
                s.width = 300.;
                s.density = Density::Normal;
                for b in &mut s.blocks {
                    b.enabled = b.id != Block::Gpu;
                }
            }
        }
        PanelProfile::capture("", &s).apply(self);
    }
}
