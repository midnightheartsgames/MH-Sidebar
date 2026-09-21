use serde::{Deserialize, Serialize};

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
    pub sections: Vec<Section>,
    /// State of the PawnIO driver that provides CPU temperature and power.
    pub cpu_driver: DriverStatus,
}

/// Why CPU temperature and power are (un)available; the settings UI offers a fix for each case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DriverStatus {
    /// Not probed yet.
    #[default]
    Unknown,
    Ready,
    /// The PawnIO driver is not installed.
    NotInstalled,
    /// The driver is installed but can only be opened by an elevated process.
    NeedsAdmin,
    /// No PawnIO module for this CPU.
    Unsupported,
    /// The driver rejected the module or the request.
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
    pub id: Block,
    pub device: String,
    pub rows: Vec<Reading>,
}

/// Numeric values carry explicit units; unsupported readings are None with a reason.
#[derive(Debug, Clone)]
pub struct Reading {
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
            key: key.into(),
            label: label.into(),
            value: None,
            text: "—".into(),
            unit: String::new(),
            reason: Some(reason.into()),
        }
    }
}
