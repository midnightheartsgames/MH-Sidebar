use crate::model::{Block, TopApp, TopApps};
use std::{collections::HashMap, ffi::OsStr, time::Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, ThreadKind};

/// Допустимые размеры топа приложений; 0 выключает список.
pub const COUNTS: [usize; 5] = [0, 1, 3, 5, 7];
pub const MAX: usize = 7;
const GIB: f64 = 1_073_741_824.0;
const MIB: f64 = 1_048_576.0;

pub fn supports(block: Block) -> bool {
    matches!(block, Block::Cpu | Block::Gpu | Block::Memory)
}
pub fn normalize_count(count: usize) -> usize {
    COUNTS
        .into_iter()
        .rev()
        .find(|allowed| *allowed <= count)
        .unwrap_or(0)
}
pub fn app_name(name: &OsStr) -> String {
    let name = name.to_string_lossy();
    let trimmed = name.trim();
    match trimmed.len().checked_sub(4) {
        Some(cut)
            if trimmed.is_char_boundary(cut) && trimmed[cut..].eq_ignore_ascii_case(".exe") =>
        {
            trimmed[..cut].to_owned()
        }
        _ => trimmed.to_owned(),
    }
}
pub fn percent(value: f64) -> String {
    format!("{value:.1} %")
}
pub fn bytes(value: f64) -> String {
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else {
        format!("{:.0} MiB", value / MIB)
    }
}

/// Складывает процессы с одинаковым именем в одно приложение и оставляет `limit` самых нагруженных.
pub fn rank(
    block: Block,
    device_id: &str,
    entries: impl IntoIterator<Item = (String, f64)>,
    limit: usize,
    format: fn(f64) -> String,
) -> TopApps {
    let mut apps: HashMap<String, (f64, usize)> = HashMap::new();
    for (name, value) in entries {
        if name.is_empty() || !value.is_finite() || value <= 0.0 {
            continue;
        }
        let app = apps.entry(name).or_default();
        app.0 += value;
        app.1 += 1;
    }
    let mut apps: Vec<_> = apps
        .into_iter()
        .map(|(name, (value, processes))| TopApp {
            text: format(value),
            name,
            processes,
            value,
        })
        .collect();
    apps.sort_by(|a, b| {
        b.value
            .total_cmp(&a.value)
            .then_with(|| a.name.cmp(&b.name))
    });
    apps.truncate(limit);
    TopApps {
        block,
        device_id: device_id.into(),
        apps,
        reason: None,
    }
}
pub fn unavailable(block: Block, device_id: &str, reason: &str) -> TopApps {
    TopApps {
        block,
        device_id: device_id.into(),
        apps: Vec::new(),
        reason: Some(reason.into()),
    }
}

/// Нагрузка процессов на один адаптер по счётчику `\GPU Engine(*)\Utilization Percentage`.
/// Как в диспетчере задач: движки одного типа складываются, у процесса берётся самый загруженный тип.
pub fn gpu_engine_usage(instances: &[(String, f64)], luid_tag: &str) -> HashMap<u32, f64> {
    let mut engines: HashMap<(u32, String), f64> = HashMap::new();
    for (name, value) in instances {
        let name = name.to_ascii_lowercase();
        if !name.contains(luid_tag) {
            continue;
        }
        let pid = name
            .strip_prefix("pid_")
            .and_then(|rest| rest.split('_').next())
            .and_then(|pid| pid.parse::<u32>().ok());
        let engine = name
            .split_once("engtype_")
            .map(|(_, kind)| kind.to_owned())
            .unwrap_or_default();
        if let Some(pid) = pid {
            *engines.entry((pid, engine)).or_default() += value;
        }
    }
    let mut usage: HashMap<u32, f64> = HashMap::new();
    for ((pid, _), value) in engines {
        let entry = usage.entry(pid).or_default();
        *entry = entry.max(value.clamp(0.0, 100.0));
    }
    usage
}

/// Нагрузка процессов на NVIDIA GPU через NVML; возвращает метку времени для следующего запроса.
pub fn nvml_usage(
    device: &nvml_wrapper::Device,
    since: Option<u64>,
) -> Result<(HashMap<u32, f64>, Option<u64>), String> {
    let samples = match device.process_utilization_stats(since) {
        Ok(samples) => samples,
        Err(nvml_wrapper::error::NvmlError::NotFound) => Vec::new(),
        Err(error) => return Err(format!("NVML не отдаёт нагрузку процессов: {error}")),
    };
    let mut latest: HashMap<u32, (u64, f64)> = HashMap::new();
    for sample in &samples {
        let value = sample
            .sm_util
            .max(sample.enc_util)
            .max(sample.dec_util)
            .min(100) as f64;
        let entry = latest.entry(sample.pid).or_insert((0, 0.0));
        if sample.timestamp >= entry.0 {
            *entry = (sample.timestamp, value);
        }
    }
    let newest = samples.iter().map(|s| s.timestamp).max().or(since);
    Ok((
        latest.into_iter().map(|(pid, (_, v))| (pid, v)).collect(),
        newest,
    ))
}

/// Список процессов отдельного потока датчика.
pub struct Processes {
    system: System,
    cpu_at: Option<Instant>,
}
impl Default for Processes {
    fn default() -> Self {
        Self {
            system: System::new(),
            cpu_at: None,
        }
    }
}
impl Processes {
    fn refresh(&mut self, kind: ProcessRefreshKind) {
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind.without_tasks());
    }
    fn named<'a>(
        &'a self,
        value: impl Fn(&sysinfo::Process) -> f64 + 'a,
    ) -> impl Iterator<Item = (String, f64)> + 'a {
        self.system
            .processes()
            .iter()
            .filter(|(pid, process)| {
                pid.as_u32() != 0 && process.thread_kind() != Some(ThreadKind::Userland)
            })
            .map(move |(_, process)| (app_name(process.name()), value(process)))
    }
    pub fn cpu(&mut self, device_id: &str, limit: usize) -> TopApps {
        let now = Instant::now();
        let ready = self
            .cpu_at
            .is_some_and(|at| now.duration_since(at) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        if self.cpu_at.is_some() && !ready {
            return unavailable(Block::Cpu, device_id, "Ожидание второго замера процессов");
        }
        self.refresh(ProcessRefreshKind::nothing().with_cpu());
        self.cpu_at = Some(now);
        if !ready {
            return unavailable(Block::Cpu, device_id, "Ожидание второго замера процессов");
        }
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f64;
        let entries: Vec<_> = self
            .named(|process| process.cpu_usage() as f64 / threads)
            .collect();
        rank(Block::Cpu, device_id, entries, limit, percent)
    }
    pub fn memory(&mut self, device_id: &str, limit: usize) -> TopApps {
        self.refresh(ProcessRefreshKind::nothing().with_memory());
        let entries: Vec<_> = self.named(|process| process.memory() as f64).collect();
        rank(Block::Memory, device_id, entries, limit, bytes)
    }
    /// Имена процессов для сопоставления с PID из счётчиков GPU.
    pub fn names(&mut self) -> HashMap<u32, String> {
        self.refresh(ProcessRefreshKind::nothing());
        self.system
            .processes()
            .iter()
            .map(|(pid, process)| (pid.as_u32(), app_name(process.name())))
            .collect()
    }
}

pub fn gpu(
    device_id: &str,
    usage: &HashMap<u32, f64>,
    names: &HashMap<u32, String>,
    limit: usize,
) -> TopApps {
    let entries = usage
        .iter()
        .filter(|(pid, _)| **pid != 0)
        .map(|(pid, value)| {
            let name = names
                .get(pid)
                .cloned()
                .unwrap_or_else(|| format!("PID {pid}"));
            (name, *value)
        });
    let mut top = rank(Block::Gpu, device_id, entries, limit, percent);
    for app in &mut top.apps {
        app.value = app.value.min(100.0);
        app.text = percent(app.value);
    }
    top
}
