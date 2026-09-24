use crate::model::{Block, DriverStatus, Reading, Section, Snapshot, Source};
use nvml_wrapper::{
    Nvml,
    enum_wrappers::device::{Clock, TemperatureSensor},
};
use std::{collections::HashMap, time::Instant};
use sysinfo::{Components, Disks, Networks, System};

const GIB: f64 = 1_073_741_824.0;
const MIB: f64 = 1_048_576.0;

fn row(key: &str, label: &str, value: Option<f64>, unit: &str) -> Reading {
    Reading::number(key, label, value, unit)
}

fn ratio(part: u64, total: u64) -> Option<f64> {
    (total > 0).then_some(part as f64 * 100.0 / total as f64)
}

pub struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    nvml: Option<Nvml>,
    cpu_at: Option<Instant>,
    disk_at: Option<Instant>,
    network_rates: HashMap<String, (u64, u64, Instant)>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            nvml: None,
            cpu_at: None,
            disk_at: None,
            network_rates: HashMap::new(),
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let mut combined = Snapshot::default();
        for source in Source::ALL {
            if let Ok(snapshot) = self.sample_source(source) {
                combined.merge(snapshot);
            }
        }
        combined
    }

    pub fn sample_source(&mut self, source: Source) -> Result<Snapshot, String> {
        let mut snapshot = match source {
            Source::Cpu => self.cpu(),
            Source::CpuDriver => self.cpu_sensor(),
            Source::Memory => self.memory(),
            Source::Gpu => self.gpu(),
            Source::Disks => self.disks(),
            Source::Network => self.network(),
        };
        let now = Instant::now();
        for section in &mut snapshot.sections {
            for reading in &mut section.rows {
                reading.sampled_at = Some(now);
            }
        }
        Ok(snapshot)
    }

    fn cpu(&mut self) -> Snapshot {
        let now = Instant::now();
        let ready = self
            .cpu_at
            .is_some_and(|at| now.duration_since(at) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        if self.cpu_at.is_none() || ready {
            self.system.refresh_cpu_all();
            self.cpu_at = Some(now);
        }
        let mut rows = vec![row(
            "load",
            "Загрузка",
            ready.then(|| self.system.global_cpu_usage() as f64),
            "%",
        )];
        let clock = self
            .system
            .cpus()
            .first()
            .map(|cpu| cpu.frequency())
            .filter(|frequency| *frequency > 0);
        rows.push(row(
            "clock",
            "Частота ОС",
            clock.map(|frequency| frequency as f64),
            "MHz",
        ));
        for (index, cpu) in self.system.cpus().iter().enumerate() {
            rows.push(row(
                &format!("core_{index}"),
                &format!("Поток {}", index + 1),
                ready.then(|| cpu.cpu_usage() as f64),
                "%",
            ));
        }
        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Cpu,
                device_id: "cpu:system".into(),
                device: self
                    .system
                    .cpus()
                    .first()
                    .map(|cpu| cpu.brand())
                    .unwrap_or("CPU")
                    .into(),
                rows,
            }],
            ..Default::default()
        }
    }

    fn cpu_sensor(&mut self) -> Snapshot {
        self.components.refresh(true);
        let temperature = self
            .components
            .list()
            .iter()
            .find(|sensor| {
                let label = sensor.label().to_ascii_lowercase();
                label.contains("package") || label.contains("tctl") || label.contains("cpu")
            })
            .and_then(|sensor| sensor.temperature())
            .map(f64::from);
        let reason = if temperature.is_some() {
            "Температура CPU получена от системного датчика; мощность CPU недоступна".to_string()
        } else {
            "Система не предоставила датчик температуры и мощности CPU".to_string()
        };
        let mut temperature_row = row("temperature", "Температура", temperature, "°C");
        if temperature.is_none() {
            temperature_row.reason = Some(reason.clone());
        }
        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Cpu,
                device_id: "cpu:system".into(),
                device: "Процессор".into(),
                rows: vec![
                    temperature_row,
                    Reading::unavailable("power", "Мощность", &reason),
                ],
            }],
            cpu_driver: DriverStatus::Unsupported,
            cpu_diagnostic: reason,
            ..Default::default()
        }
    }

    fn memory(&mut self) -> Snapshot {
        self.system.refresh_memory();
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Memory,
                device_id: "memory:system".into(),
                device: "Оперативная память".into(),
                rows: vec![
                    row("load", "Загрузка", ratio(used, total), "%"),
                    row("used", "Использовано", Some(used as f64 / GIB), "GiB"),
                    row(
                        "free",
                        "Доступно",
                        Some(self.system.available_memory() as f64 / GIB),
                        "GiB",
                    ),
                    row("total", "Всего", Some(total as f64 / GIB), "GiB"),
                ],
            }],
            ..Default::default()
        }
    }

    fn gpu(&mut self) -> Snapshot {
        if self.nvml.is_none() {
            self.nvml = Nvml::init().ok();
        }
        let mut sections = Vec::new();
        if let Some(nvml) = &self.nvml
            && let Ok(count) = nvml.device_count()
        {
            for index in 0..count {
                let Ok(device) = nvml.device_by_index(index) else {
                    continue;
                };
                let memory = device.memory_info().ok();
                let id = device
                    .uuid()
                    .map(|uuid| format!("gpu:nvml:{}", uuid.to_lowercase()))
                    .unwrap_or_else(|_| format!("unstable:gpu:nvml:{index}"));
                sections.push(Section {
                    disconnected: false,
                    id: Block::Gpu,
                    device_id: id,
                    device: device
                        .name()
                        .unwrap_or_else(|_| format!("NVIDIA GPU {index}")),
                    rows: vec![
                        row(
                            "load",
                            "Загрузка",
                            device.utilization_rates().ok().map(|rate| rate.gpu as f64),
                            "%",
                        ),
                        row(
                            "temperature",
                            "Температура",
                            device
                                .temperature(TemperatureSensor::Gpu)
                                .ok()
                                .map(f64::from),
                            "°C",
                        ),
                        row(
                            "clock",
                            "Частота ядра",
                            device.clock_info(Clock::Graphics).ok().map(f64::from),
                            "MHz",
                        ),
                        row(
                            "memory_clock",
                            "Частота памяти",
                            device.clock_info(Clock::Memory).ok().map(f64::from),
                            "MHz",
                        ),
                        row(
                            "vram_used",
                            "Видеопамять занято",
                            memory.as_ref().map(|value| value.used as f64 / GIB),
                            "GiB",
                        ),
                        row(
                            "vram_total",
                            "Видеопамять всего",
                            memory.as_ref().map(|value| value.total as f64 / GIB),
                            "GiB",
                        ),
                        row(
                            "power",
                            "Мощность",
                            device.power_usage().ok().map(|value| value as f64 / 1000.0),
                            "W",
                        ),
                        row(
                            "fan",
                            "Вентилятор",
                            device.fan_speed(0).ok().map(f64::from),
                            "%",
                        ),
                    ],
                });
            }
        }
        if sections.is_empty() {
            sections.push(Section {
                disconnected: false,
                id: Block::Gpu,
                device_id: "unstable:gpu".into(),
                device: "Видеокарта".into(),
                rows: vec![Reading::unavailable(
                    "load",
                    "Загрузка",
                    "NVML не обнаружил NVIDIA GPU; системный GPU API пока не подключён",
                )],
            });
        }
        Snapshot {
            sections,
            ..Default::default()
        }
    }

    fn disks(&mut self) -> Snapshot {
        let now = Instant::now();
        let elapsed = self
            .disk_at
            .map(|at| now.duration_since(at).as_secs_f64())
            .filter(|value| *value > 0.0);
        self.disks.refresh(true);
        let sections = self
            .disks
            .list_mut()
            .iter_mut()
            .map(|disk| {
                disk.refresh();
                let total = disk.total_space();
                let used = total.saturating_sub(disk.available_space());
                let usage = disk.usage();
                let mount = disk.mount_point().to_string_lossy();
                Section {
                    disconnected: false,
                    id: Block::Disks,
                    device_id: format!("disk:{mount}"),
                    device: format!("{} {}", mount, disk.name().to_string_lossy()),
                    rows: vec![
                        row("load", "Заполнено", ratio(used, total), "%"),
                        row("used", "Занято", Some(used as f64 / GIB), "GiB"),
                        row("total", "Всего", Some(total as f64 / GIB), "GiB"),
                        row(
                            "read",
                            "Чтение",
                            elapsed.map(|seconds| usage.read_bytes as f64 / MIB / seconds),
                            "MiB/s",
                        ),
                        row(
                            "write",
                            "Запись",
                            elapsed.map(|seconds| usage.written_bytes as f64 / MIB / seconds),
                            "MiB/s",
                        ),
                    ],
                }
            })
            .collect();
        self.disk_at = Some(now);
        Snapshot {
            sections,
            ..Default::default()
        }
    }

    fn network(&mut self) -> Snapshot {
        let now = Instant::now();
        self.networks.refresh(true);
        let mut sections = Vec::new();
        for (name, device) in self.networks.list() {
            let previous = self.network_rates.insert(
                name.clone(),
                (device.total_received(), device.total_transmitted(), now),
            );
            let rates = previous.and_then(|(received, transmitted, at)| {
                let seconds = now.duration_since(at).as_secs_f64();
                (seconds > 0.0).then_some((
                    device.total_received().saturating_sub(received) as f64 / MIB / seconds,
                    device.total_transmitted().saturating_sub(transmitted) as f64 / MIB / seconds,
                ))
            });
            sections.push(Section {
                disconnected: false,
                id: Block::Network,
                device_id: format!("unstable:network:{name}"),
                device: name.clone(),
                rows: vec![
                    row("download", "Приём", rates.map(|value| value.0), "MiB/s"),
                    row("upload", "Передача", rates.map(|value| value.1), "MiB/s"),
                ],
            });
        }
        self.network_rates
            .retain(|name, _| self.networks.list().contains_key(name));
        Snapshot {
            sections,
            ..Default::default()
        }
    }
}
