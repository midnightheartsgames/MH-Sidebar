use std::time::Duration;
#[derive(Default)]
struct Rate {
    previous: Option<(u64, Duration)>,
}
impl Rate {
    fn update(&mut self, bytes: u64, time: Duration) -> Option<f64> {
        let previous = self.previous.replace((bytes, time));
        let (old, then) = previous?;
        let elapsed = time.checked_sub(then)?.as_secs_f64();
        if elapsed <= 0.0 {
            return None;
        }
        Some(bytes.checked_sub(old)? as f64 / elapsed / 1048576.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_counter_is_unknown() {
        assert_eq!(Rate::default().update(999, Duration::ZERO), None);
    }
    #[test]
    fn rate_uses_elapsed_time_and_mib() {
        let mut r = Rate::default();
        r.update(100, Duration::ZERO);
        assert_eq!(
            r.update(2 * 1024 * 1024 + 100, Duration::from_secs(4)),
            Some(0.5)
        );
    }
    #[test]
    fn reset_rebaselines_without_spike() {
        let mut r = Rate::default();
        r.update(900, Duration::ZERO);
        assert_eq!(r.update(1, Duration::from_secs(1)), None);
        assert_eq!(r.update(1048577, Duration::from_secs(2)), Some(1.0));
    }
    #[test]
    fn zero_elapsed_is_unknown() {
        let mut r = Rate::default();
        r.update(0, Duration::ZERO);
        assert_eq!(r.update(10, Duration::ZERO), None);
    }
}

use crate::model::{Block, Reading, Section, Snapshot, Source};
use nvml_wrapper::{
    Nvml,
    enum_wrappers::device::{Clock, TemperatureSensor},
};
use std::{collections::HashMap, time::Instant};
use sysinfo::{Disks, System};
mod cpu_temp;
mod gpu;
mod identity;
mod pawnio;
mod pdh;
mod recovery;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn from_wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
}
unsafe fn from_wide_ptr(s: *const u16) -> String {
    if s.is_null() {
        return String::new();
    }
    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
        from_wide(std::slice::from_raw_parts(s, len))
    }
}
const GIB: f64 = 1073741824.0;
fn row(k: &str, l: &str, v: Option<f64>, u: &str) -> Reading {
    Reading::number(k, l, v, u)
}
fn ratio(a: u64, b: u64) -> Option<f64> {
    (b > 0).then(|| a as f64 / b as f64 * 100.0)
}

pub struct Sampler {
    system: System,
    disks: Disks,
    start: Instant,
    cpu_at: Option<Instant>,
    rates: HashMap<String, (Rate, Rate)>,
    nvml: Option<Nvml>,
    refresh_at: HashMap<Source, Instant>,
    adapters: Vec<(gpu::AdapterInfo, Option<gpu::Adapter>)>,
    engines: Option<pdh::CounterQuery>,
    gpu_memory: Option<pdh::CounterQuery>,
    disk_read: Option<pdh::CounterQuery>,
    disk_write: Option<pdh::CounterQuery>,
    gpu_ready: bool,
    disk_ready: bool,
    cpu_sensor: recovery::CpuRecovery<cpu_temp::CpuSensor>,
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
            disks: Disks::new(),
            start: Instant::now(),
            cpu_at: None,
            rates: HashMap::new(),
            nvml: None,
            refresh_at: HashMap::new(),
            adapters: vec![],
            engines: None,
            gpu_memory: None,
            disk_read: None,
            disk_write: None,
            gpu_ready: false,
            disk_ready: false,
            cpu_sensor: recovery::CpuRecovery::default(),
        }
    }
    fn refresh_due(&mut self, source: Source) -> bool {
        let now = Instant::now();
        if self
            .refresh_at
            .get(&source)
            .is_none_or(|at| now.duration_since(*at) >= Duration::from_secs(30))
        {
            self.refresh_at.insert(source, now);
            true
        } else {
            false
        }
    }
    pub fn sample(&mut self) -> Snapshot {
        let mut combined = Snapshot::default();
        for source in Source::ALL {
            match self.sample_source(source) {
                Ok(snapshot) => combined.merge(snapshot),
                Err(error) => combined.sources.push(crate::model::SourceStatus {
                    source,
                    age: None,
                    stale: true,
                    error: Some(error),
                }),
            }
        }
        combined
    }
    pub fn sample_source(&mut self, source: Source) -> Result<Snapshot, String> {
        let mut snapshot = match source {
            Source::Cpu => self.cpu(),
            Source::CpuDriver => self.cpu_driver(),
            Source::Memory => self.memory(),
            Source::Gpu => self.gpu(),
            Source::Disks => self.disks(),
            Source::Network => self.network()?,
        };
        let at = Instant::now();
        for section in &mut snapshot.sections {
            for row in &mut section.rows {
                row.sampled_at = Some(at);
            }
        }
        Ok(snapshot)
    }
    fn cpu(&mut self) -> Snapshot {
        let now = Instant::now();
        let cpu_ready = self
            .cpu_at
            .is_some_and(|t| now.duration_since(t) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        if self.cpu_at.is_none() || cpu_ready {
            self.system.refresh_cpu_all();
            self.cpu_at = Some(now);
        }
        let mut cpu = vec![row(
            "load",
            "Загрузка",
            cpu_ready.then(|| self.system.global_cpu_usage() as f64),
            "%",
        )];
        let clock = self
            .system
            .cpus()
            .first()
            .map(|c| c.frequency())
            .filter(|v| *v > 0);
        cpu.push(row("clock", "Частота ОС", clock.map(|v| v as f64), "MHz"));
        for (i, c) in self.system.cpus().iter().enumerate() {
            cpu.push(row(
                &format!("core_{i}"),
                &format!("Поток {}", i + 1),
                cpu_ready.then(|| c.cpu_usage() as f64),
                "%",
            ));
        }
        let name = self
            .system
            .cpus()
            .first()
            .map(|c| c.brand())
            .unwrap_or("CPU")
            .to_string();

        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Cpu,
                device_id: "cpu:system".into(),
                device: name,
                rows: cpu,
            }],
            ..Default::default()
        }
    }
    fn cpu_driver(&mut self) -> Snapshot {
        let mut cpu = Vec::new();
        let cpu_time = self.start.elapsed();
        let (temperature, power) = self.cpu_sensor.sample(
            cpu_time,
            cpu_temp::CpuSensor::open,
            cpu_temp::CpuSensor::read,
        );
        let cpu_diagnostic = self.cpu_sensor.diagnostic(cpu_time);
        for (key, label, value, unit) in [
            ("temperature", "Температура", temperature, "°C"),
            ("power", "Мощность", power, "W"),
        ] {
            let mut reading = row(key, label, value, unit);
            if reading.value.is_none() {
                reading.reason = Some(format!("{label}: показание недоступно. {cpu_diagnostic}"));
            }
            cpu.push(reading);
        }

        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Cpu,
                device_id: "cpu:system".into(),
                device: "Процессор".into(),
                rows: cpu,
            }],
            cpu_driver: self.cpu_sensor.status,
            cpu_diagnostic,
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
        if self.refresh_due(Source::Gpu) {
            if self.nvml.is_none() {
                self.nvml = Nvml::init().ok();
            }
            self.adapters = gpu::adapters()
                .into_iter()
                .filter(|a| !a.software)
                .map(|a| {
                    let handle = gpu::Adapter::open(a.luid);
                    (a, handle)
                })
                .collect();
            if self.engines.is_none() {
                self.engines =
                    pdh::CounterQuery::open(&[r"\GPU Engine(*)\Utilization Percentage"]).ok();
            }
            if self.gpu_memory.is_none() {
                self.gpu_memory =
                    pdh::CounterQuery::open(&[r"\GPU Adapter Memory(*)\Dedicated Usage"]).ok();
            }
        }
        let engines = collect(&mut self.engines);
        let memory = collect(&mut self.gpu_memory);
        let mut sections = Vec::new();
        let mut nv_ids = std::collections::HashSet::new();
        let mut nv_failed = false;
        if let Some(nv) = &self.nvml {
            match nv.device_count() {
                Ok(count) => {
                    for i in 0..count {
                        let Ok(d) = nv.device_by_index(i) else {
                            continue;
                        };
                        let name = d.name().unwrap_or_else(|_| format!("NVIDIA GPU {i}"));
                        let pci = d.pci_info().ok();
                        let matched = pci.as_ref().and_then(|pci| {
                            let function = pci
                                .bus_id
                                .rsplit('.')
                                .next()
                                .and_then(|f| u32::from_str_radix(f, 16).ok())?;
                            if pci.domain != 0 {
                                return None;
                            }
                            let mut matches = self.adapters.iter().filter(|(a, _)| {
                                a.pci_address == Some((pci.bus, pci.device, function))
                            });
                            let first = matches.next()?;
                            matches.next().is_none().then_some(&first.0)
                        });
                        let device_id = matched.map(|a| a.device_id.clone()).unwrap_or_else(|| {
                            d.uuid()
                                .map(|uuid| format!("gpu:nvml:{}", uuid.to_lowercase()))
                                .unwrap_or_else(|_| format!("unstable:gpu:nvml:{i}"))
                        });
                        nv_ids.insert(device_id.clone());
                        let mem = d.memory_info().ok();
                        sections.push(Section {
                            disconnected: false,
                            id: Block::Gpu,
                            device_id,
                            device: format!("{name} · NVIDIA {i}"),
                            rows: vec![
                                row(
                                    "load",
                                    "Загрузка",
                                    d.utilization_rates().ok().map(|u| u.gpu as f64),
                                    "%",
                                ),
                                row(
                                    "temperature",
                                    "Температура",
                                    d.temperature(TemperatureSensor::Gpu).ok().map(f64::from),
                                    "°C",
                                ),
                                row(
                                    "clock",
                                    "Частота ядра",
                                    d.clock_info(Clock::Graphics).ok().map(f64::from),
                                    "MHz",
                                ),
                                row(
                                    "memory_clock",
                                    "Частота памяти",
                                    d.clock_info(Clock::Memory).ok().map(f64::from),
                                    "MHz",
                                ),
                                row(
                                    "vram_used",
                                    "Видеопамять занято",
                                    mem.as_ref().map(|m| m.used as f64 / GIB),
                                    "GiB",
                                ),
                                row(
                                    "vram_total",
                                    "Видеопамять всего",
                                    mem.as_ref().map(|m| m.total as f64 / GIB),
                                    "GiB",
                                ),
                                row(
                                    "power",
                                    "Мощность",
                                    d.power_usage().ok().map(|v| v as f64 / 1000.0),
                                    "W",
                                ),
                                row("fan", "Вентилятор", d.fan_speed(0).ok().map(f64::from), "%"),
                            ],
                        });
                    }
                }
                Err(_) => nv_failed = true,
            }
        }
        if nv_failed {
            self.nvml = None;
        }
        for (info, adapter) in &self.adapters {
            if nv_ids.contains(&info.device_id) {
                continue;
            }
            let perf = adapter.as_ref().map(|a| a.perf()).unwrap_or_default();
            let tag = info.luid.pdh_tag();
            let mut sums = HashMap::<String, f64>::new();
            for (name, v) in &engines {
                let name = name.to_lowercase();
                if name.contains(&tag)
                    && let Some((_, engine)) = name.split_once("_eng_")
                {
                    *sums.entry(engine.to_string()).or_default() += v;
                }
            }
            let load = self
                .gpu_ready
                .then(|| sums.values().copied().reduce(f64::max))
                .flatten()
                .map(|v| v.clamp(0.0, 100.0));
            let mem = memory
                .iter()
                .filter(|(n, _)| n.to_lowercase().starts_with(&tag))
                .map(|(_, v)| *v)
                .reduce(|a, b| a + b);
            sections.push(Section {
                disconnected: false,
                id: Block::Gpu,
                device_id: info.device_id.clone(),
                device: format!("{} · {}", info.name, tag),
                rows: vec![
                    row("load", "Загрузка", load, "%"),
                    row("temperature", "Температура", perf.temperature_c, "°C"),
                    row("clock", "Частота ядра", perf.core_frequency_mhz, "MHz"),
                    row(
                        "memory_clock",
                        "Частота памяти",
                        perf.memory_frequency_mhz,
                        "MHz",
                    ),
                    row(
                        "vram_used",
                        "Видеопамять занято",
                        mem.map(|m| m / GIB),
                        "GiB",
                    ),
                    row(
                        "vram_total",
                        "Выделенная видеопамять",
                        info.dedicated_memory_bytes.map(|bytes| bytes as f64 / GIB),
                        "GiB",
                    ),
                    Reading::unavailable("power", "Мощность", "WDDM не сообщает мощность в ваттах"),
                    row("fan", "Вентилятор", perf.fan_rpm.map(f64::from), "RPM"),
                ],
            });
        }
        if !sections.iter().any(|s| s.id == Block::Gpu) {
            sections.push(Section {
                disconnected: false,
                id: Block::Gpu,
                device_id: "unstable:gpu".into(),
                device: "Видеокарта".into(),
                rows: vec![Reading::unavailable(
                    "load",
                    "Загрузка",
                    "NVML и WDDM не обнаружили доступную видеокарту",
                )],
            });
        }

        self.gpu_ready = true;
        Snapshot {
            sections,
            ..Default::default()
        }
    }
    fn disks(&mut self) -> Snapshot {
        if self.refresh_due(Source::Disks) {
            self.disks.refresh(true);
            if self.disk_read.is_none() {
                self.disk_read =
                    pdh::CounterQuery::open(&[r"\LogicalDisk(*)\Disk Read Bytes/sec"]).ok();
            }
            if self.disk_write.is_none() {
                self.disk_write =
                    pdh::CounterQuery::open(&[r"\LogicalDisk(*)\Disk Write Bytes/sec"]).ok();
            }
        }
        let reads = collect(&mut self.disk_read);
        let writes = collect(&mut self.disk_write);
        let mut sections = Vec::new();
        for disk in self.disks.list_mut() {
            disk.refresh();
            let valid = unsafe {
                windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                    wide(&disk.mount_point().to_string_lossy()).as_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0
            };
            let total = disk.total_space();
            let used = total.saturating_sub(disk.available_space());
            let mount = disk.mount_point().to_string_lossy().to_string();
            let pdh_name = mount.trim_end_matches('\\');
            let rate = |values: &[(String, f64)]| {
                self.disk_ready
                    .then(|| {
                        values
                            .iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(pdh_name))
                            .map(|(_, v)| v / 1048576.0)
                    })
                    .flatten()
            };
            let mut rows = vec![
                row(
                    "load",
                    "Заполнено",
                    valid.then(|| ratio(used, total)).flatten(),
                    "%",
                ),
                row("used", "Занято", valid.then_some(used as f64 / GIB), "GiB"),
                row("total", "Всего", valid.then_some(total as f64 / GIB), "GiB"),
                row("read", "Чтение", rate(&reads), "MiB/s"),
                row("write", "Запись", rate(&writes), "MiB/s"),
            ];
            for r in &mut rows[3..] {
                if r.value.is_none() {
                    r.reason=Some("Нет действительной пары замеров PDH или доступ к счётчику диска отсутствует".into());
                }
            }
            sections.push(Section {
                disconnected: false,
                id: Block::Disks,
                device_id: identity::volume_id(&mount),
                device: format!("{} {}", mount, disk.name().to_string_lossy()),
                rows,
            });
        }

        self.disk_ready = true;
        Snapshot {
            sections,
            ..Default::default()
        }
    }
    fn network(&mut self) -> Result<Snapshot, String> {
        let devices = identity::networks()?;
        let time = self.start.elapsed();
        self.rates
            .retain(|id, _| devices.iter().any(|d| &d.id == id));
        let sections = devices
            .into_iter()
            .map(|d| {
                let rates = self.rates.entry(d.id.clone()).or_default();
                Section {
                    disconnected: false,
                    id: Block::Network,
                    device_id: d.id,
                    device: d.name,
                    rows: vec![
                        row(
                            "download",
                            "Приём",
                            rates.0.update(d.received, time),
                            "MiB/s",
                        ),
                        row(
                            "upload",
                            "Передача",
                            rates.1.update(d.transmitted, time),
                            "MiB/s",
                        ),
                    ],
                }
            })
            .collect();
        Ok(Snapshot {
            sections,
            ..Default::default()
        })
    }
}
fn collect(query: &mut Option<pdh::CounterQuery>) -> Vec<(String, f64)> {
    query
        .as_mut()
        .and_then(|q| q.collect().ok().map(|_| q.instances(0)))
        .unwrap_or_default()
}

#[cfg(test)]
mod fallback_probe {
    use super::*;
    #[test]
    #[ignore = "requires live Windows GPU providers; run explicitly on hardware"]
    fn wddm_fallback_can_sample_this_machine_without_nvml() {
        let mut sampler = Sampler::new();
        sampler.sample();
        sampler.nvml = None;
        std::thread::sleep(Duration::from_millis(300));
        let snapshot = sampler.sample();
        let gpu = snapshot
            .sections
            .iter()
            .find(|s| s.id == Block::Gpu)
            .unwrap();
        println!("WDDM fallback: {gpu:?}");
        assert!(!gpu.rows.is_empty());
        for row in &gpu.rows {
            assert!(row.value.is_some_and(f64::is_finite) || row.reason.is_some());
        }
    }
    #[test]
    #[ignore = "requires live Windows storage, network and GPU providers"]
    fn stable_ids_are_unique_and_gpu_backends_agree_on_this_machine() {
        let mut sampler = Sampler::new();
        let first = sampler.sample();
        let mut ids = std::collections::HashSet::new();
        for section in &first.sections {
            println!(
                "{:?}: {} -> {}",
                section.id, section.device, section.device_id
            );
            assert!(
                ids.insert((section.id, section.device_id.clone())),
                "duplicate device ID"
            );
            if matches!(section.id, Block::Disks | Block::Network) {
                assert!(
                    !section.device_id.starts_with("unstable:"),
                    "stable identity unavailable on this machine"
                );
            }
        }
        let gpu_ids: Vec<_> = first
            .sections
            .iter()
            .filter(|s| s.id == Block::Gpu)
            .map(|s| s.device_id.clone())
            .collect();
        assert!(!gpu_ids.is_empty());
        sampler.nvml = None;
        let fallback = sampler.sample_source(Source::Gpu).unwrap();
        let fallback_ids: Vec<_> = fallback
            .sections
            .iter()
            .map(|s| s.device_id.clone())
            .collect();
        assert_eq!(
            gpu_ids, fallback_ids,
            "NVML and WDDM must resolve the same physical adapters"
        );
    }
}
