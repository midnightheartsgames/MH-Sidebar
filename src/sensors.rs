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

use crate::model::{Block, DriverStatus, Reading, Section, Snapshot};
use nvml_wrapper::{
    Nvml,
    enum_wrappers::device::{Clock, TemperatureSensor},
};
use std::{collections::HashMap, time::Instant};
use sysinfo::{Disks, Networks, System};
mod cpu_temp;
mod gpu;
mod pawnio;
mod pdh;
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
    networks: Networks,
    start: Instant,
    cpu_at: Option<Instant>,
    rates: HashMap<String, (Rate, Rate)>,
    nvml: Option<Nvml>,
    refresh_at: Option<Instant>,
    adapters: Vec<(gpu::AdapterInfo, Option<gpu::Adapter>)>,
    engines: Option<pdh::CounterQuery>,
    gpu_memory: Option<pdh::CounterQuery>,
    disk_read: Option<pdh::CounterQuery>,
    disk_write: Option<pdh::CounterQuery>,
    pdh_ready: bool,
    cpu_sensor: Option<cpu_temp::CpuSensor>,
    cpu_driver: DriverStatus,
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
            networks: Networks::new(),
            start: Instant::now(),
            cpu_at: None,
            rates: HashMap::new(),
            nvml: None,
            refresh_at: None,
            adapters: vec![],
            engines: None,
            gpu_memory: None,
            disk_read: None,
            disk_write: None,
            pdh_ready: false,
            cpu_sensor: None,
            cpu_driver: DriverStatus::Unknown,
        }
    }
    pub fn sample(&mut self) -> Snapshot {
        let now = Instant::now();
        let refresh = self
            .refresh_at
            .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30));
        if refresh {
            self.refresh_at = Some(now);
            self.disks.refresh(true);
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
            for (query, path) in [
                (&mut self.engines, r"\GPU Engine(*)\Utilization Percentage"),
                (
                    &mut self.gpu_memory,
                    r"\GPU Adapter Memory(*)\Dedicated Usage",
                ),
                (&mut self.disk_read, r"\LogicalDisk(*)\Disk Read Bytes/sec"),
                (
                    &mut self.disk_write,
                    r"\LogicalDisk(*)\Disk Write Bytes/sec",
                ),
            ] {
                if query.is_none() {
                    *query = pdh::CounterQuery::open(&[path]).ok();
                }
            }
            // Retried on the slow refresh so a freshly installed driver is picked up without restart.
            if self.cpu_sensor.is_none() && self.cpu_driver != DriverStatus::Unsupported {
                match cpu_temp::CpuSensor::open() {
                    Ok(sensor) => {
                        self.cpu_sensor = Some(sensor);
                        self.cpu_driver = DriverStatus::Ready;
                    }
                    Err(status) => self.cpu_driver = status,
                }
            }
        }
        self.system.refresh_memory();
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
        let (temperature, power) = self.cpu_sensor.as_mut().map_or((None, None), |s| {
            s.read(now.duration_since(self.start).as_millis() as u64)
        });
        for (key, label, value, unit) in [
            ("temperature", "Температура", temperature, "°C"),
            ("power", "Мощность", power, "W"),
        ] {
            let mut reading = row(key, label, value, unit);
            if reading.value.is_none() && self.cpu_driver != DriverStatus::Ready {
                reading.reason = Some(self.cpu_driver.reason().into());
            }
            cpu.push(reading);
        }
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
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let mut sections = vec![
            Section {
                id: Block::Cpu,
                device: name,
                rows: cpu,
            },
            Section {
                id: Block::Memory,
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
            },
        ];
        let collect = |q: &mut Option<pdh::CounterQuery>| {
            q.as_mut()
                .and_then(|q| q.collect().ok().map(|_| q.instances(0)))
                .unwrap_or_default()
        };
        let engines = collect(&mut self.engines);
        let memory = collect(&mut self.gpu_memory);
        let reads = collect(&mut self.disk_read);
        let writes = collect(&mut self.disk_write);
        let mut nv_names = vec![];
        let mut nv_failed = false;
        if let Some(nv) = &self.nvml {
            match nv.device_count() {
                Ok(count) => {
                    for i in 0..count {
                        let Ok(d) = nv.device_by_index(i) else {
                            continue;
                        };
                        let name = d.name().unwrap_or_else(|_| format!("NVIDIA GPU {i}"));
                        nv_names.push(name.clone());
                        let mem = d.memory_info().ok();
                        sections.push(Section {
                            id: Block::Gpu,
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
            if let Some(index) = nv_names
                .iter()
                .position(|n| n.eq_ignore_ascii_case(&info.name))
            {
                nv_names.remove(index);
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
                .pdh_ready
                .then(|| sums.values().copied().reduce(f64::max))
                .flatten()
                .map(|v| v.clamp(0.0, 100.0));
            let mem = memory
                .iter()
                .filter(|(n, _)| n.to_lowercase().starts_with(&tag))
                .map(|(_, v)| *v)
                .reduce(|a, b| a + b);
            sections.push(Section {
                id: Block::Gpu,
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
                id: Block::Gpu,
                device: "Видеокарта".into(),
                rows: vec![Reading::unavailable(
                    "load",
                    "Загрузка",
                    "NVML и WDDM не обнаружили доступную видеокарту",
                )],
            });
        }
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
                self.pdh_ready
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
                id: Block::Disks,
                device: format!("{} {}", mount, disk.name().to_string_lossy()),
                rows,
            });
        }
        self.networks.refresh(true);
        let time = self.start.elapsed();
        self.rates
            .retain(|name, _| self.networks.contains_key(name));
        for (name, data) in &self.networks {
            if name.to_lowercase().contains("loopback") {
                continue;
            }
            let rates = self.rates.entry(name.clone()).or_default();
            sections.push(Section {
                id: Block::Network,
                device: name.clone(),
                rows: vec![
                    row(
                        "download",
                        "Приём",
                        rates.0.update(data.total_received(), time),
                        "MiB/s",
                    ),
                    row(
                        "upload",
                        "Передача",
                        rates.1.update(data.total_transmitted(), time),
                        "MiB/s",
                    ),
                ],
            });
        }
        self.pdh_ready = true;
        sections.sort_by(|a, b| {
            let order = |b: Block| Block::ALL.iter().position(|x| *x == b).unwrap();
            order(a.id).cmp(&order(b.id)).then(a.device.cmp(&b.device))
        });
        Snapshot {
            sections,
            cpu_driver: self.cpu_driver,
        }
    }
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
}
