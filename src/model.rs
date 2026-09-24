use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Cpu,
    CpuDriver,
    Memory,
    Gpu,
    Disks,
    Network,
}
impl Source {
    pub const ALL: [Self; 6] = [
        Self::Cpu,
        Self::CpuDriver,
        Self::Memory,
        Self::Gpu,
        Self::Disks,
        Self::Network,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Cpu => "CPU / ОС",
            Self::CpuDriver => "CPU / PawnIO",
            Self::Memory => "Память",
            Self::Gpu => "GPU",
            Self::Disks => "Диски",
            Self::Network => "Сеть",
        }
    }
    pub fn block(self) -> Block {
        match self {
            Self::Cpu | Self::CpuDriver => Block::Cpu,
            Self::Memory => Block::Memory,
            Self::Gpu => Block::Gpu,
            Self::Disks => Block::Disks,
            Self::Network => Block::Network,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceStatus {
    pub source: Source,
    pub age: Option<Duration>,
    pub stale: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Block {
    Clock,
    Cpu,
    Gpu,
    Memory,
    Disks,
    Network,
}

impl Block {
    pub const ALL: [Self; 6] = [
        Self::Clock,
        Self::Cpu,
        Self::Gpu,
        Self::Memory,
        Self::Disks,
        Self::Network,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Clock => "Время",
            Self::Cpu => "Процессор",
            Self::Gpu => "Видеокарта",
            Self::Memory => "ОЗУ",
            Self::Disks => "Диски",
            Self::Network => "Сеть",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Self::Clock => "ВРЕМЯ",
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
            Self::Memory => "RAM",
            Self::Disks => "DISK",
            Self::Network => "NETWORK",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub sources: Vec<SourceStatus>,
    pub sections: Vec<Section>,
    pub cpu_driver: DriverStatus,
    pub cpu_diagnostic: String,
}
impl Snapshot {
    pub fn merge(&mut self, other: Snapshot) {
        if !other.cpu_diagnostic.is_empty() {
            self.cpu_driver = other.cpu_driver;
            self.cpu_diagnostic = other.cpu_diagnostic;
        }
        self.sources.extend(other.sources);
        for section in other.sections {
            if let Some(existing) = self
                .sections
                .iter_mut()
                .find(|s| s.id == section.id && s.device_id == section.device_id)
            {
                if section.device != "Процессор" {
                    existing.device = section.device;
                }
                existing.rows.extend(section.rows);
            } else {
                self.sections.push(section);
            }
        }
        self.sections.sort_by(|a, b| {
            let order = |b| Block::ALL.iter().position(|v| *v == b).unwrap_or(0);
            order(a.id)
                .cmp(&order(b.id))
                .then(a.device_id.cmp(&b.device_id))
        });
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriverStatus {
    #[default]
    Unknown,
    Ready,
    NotInstalled,
    NeedsAdmin,
    Unsupported,
    Failed,
}

impl DriverStatus {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Unknown => "Проверка драйвера PawnIO…",
            Self::Ready => "Датчик не ответил",
            Self::NotInstalled => "Нужен драйвер PawnIO — установите его в настройках процессора",
            Self::NeedsAdmin => "PawnIO установлен, но нужны права администратора",
            Self::Unsupported => "Процессор не поддерживается модулями PawnIO",
            Self::Failed => "Драйвер PawnIO отклонил запрос",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub disconnected: bool,
    pub id: Block,
    pub device_id: String,
    pub device: String,
    pub rows: Vec<Reading>,
}
#[derive(Debug, Clone)]
pub struct Reading {
    pub sampled_at: Option<Instant>,
    pub stale: bool,
    pub key: String,
    pub label: String,
    pub value: Option<f64>,
    pub text: String,
    pub unit: String,
    pub reason: Option<String>,
}

impl Reading {
    pub fn number(key: &str, label: &str, value: Option<f64>, unit: &str) -> Self {
        let value = value.filter(|v| v.is_finite());
        Self {
            sampled_at: None,
            stale: false,
            key: key.into(),
            label: label.into(),
            value,
            text: value
                .map(|v| format!("{v:.1} {unit}"))
                .unwrap_or_else(|| "—".into()),
            unit: unit.into(),
            reason: value.is_none().then(|| "Датчик недоступен".into()),
        }
    }
    pub fn unavailable(key: &str, label: &str, reason: &str) -> Self {
        Self {
            sampled_at: None,
            stale: false,
            key: key.into(),
            label: label.into(),
            value: None,
            text: "—".into(),
            unit: String::new(),
            reason: Some(reason.into()),
        }
    }
}
