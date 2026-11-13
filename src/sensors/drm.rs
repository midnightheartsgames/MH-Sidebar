//! Видеокарты Linux через sysfs и DRM fdinfo: AMD (amdgpu), Intel (i915, xe) и прочие PCI-адаптеры.
//!
//! Все данные читаются без прав root: `/sys/class/drm/card*/device` даёт загрузку AMD,
//! видеопамять, частоты и датчики hwmon; у Intel загрузка считается по времени простоя (RC6 /
//! gtidle). Нагрузка приложений берётся из `/proc/<pid>/fdinfo` — стандартных счётчиков
//! `drm-engine-*` и `drm-cycles-*`, которые ядро отдаёт для процессов текущего пользователя.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

pub const VENDOR_AMD: u16 = 0x1002;
pub const VENDOR_INTEL: u16 = 0x8086;
pub const VENDOR_NVIDIA: u16 = 0x10de;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// PCI-адрес вида `0000:03:00.0`, он же `drm-pdev` в fdinfo.
    pub address: String,
    pub vendor: u16,
    pub device: u16,
    /// Ревизия чипа: у AMD по ней различаются модели на одном кристалле (RX 6700 / 6700 XT / 6750 XT).
    pub revision: Option<u8>,
    pub driver: String,
    /// Каталог `/sys/class/drm/cardN`.
    pub card: PathBuf,
    /// Каталог PCI-устройства (`cardN/device`).
    pub path: PathBuf,
}

impl Card {
    pub fn id(&self) -> String {
        format!("gpu:pci:{}", self.address)
    }
    pub fn is_intel(&self) -> bool {
        self.vendor == VENDOR_INTEL || matches!(self.driver.as_str(), "i915" | "xe")
    }
}

/// Мгновенные показания одной видеокарты; `None` — датчик есть, но значения пока нет.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    pub load: Option<Option<f64>>,
    pub temperature: Option<Option<f64>>,
    pub clock: Option<Option<f64>>,
    pub memory_clock: Option<Option<f64>>,
    pub vram_used: Option<Option<u64>>,
    pub vram_total: Option<Option<u64>>,
    pub power: Option<Option<f64>>,
    pub fan: Option<Option<f64>>,
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}
fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.parse().ok()
}
fn read_hex(path: impl AsRef<Path>) -> Option<u16> {
    let text = read(path)?;
    u16::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}
fn first_existing(base: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(|name| base.join(name))
        .find(|path| path.exists())
}

/// Видеоадаптеры PCI из `root` (обычно `/sys/class/drm`), по одному на PCI-адрес.
pub fn cards(root: &Path) -> Vec<Card> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<_> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    names.sort_by_key(|name| name[4..].parse::<u32>().unwrap_or(u32::MAX));
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for name in names {
        let card = root.join(&name);
        let link = card.join("device");
        let Some(class) = read(link.join("class")) else {
            continue;
        };
        // 0x03xxxx — дисплейные контроллеры (VGA, 3D, прочие).
        if !class.starts_with("0x03") {
            continue;
        }
        let (Some(vendor), Some(device)) =
            (read_hex(link.join("vendor")), read_hex(link.join("device")))
        else {
            continue;
        };
        let path = fs::canonicalize(&link).unwrap_or(link);
        let address = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.clone());
        if !seen.insert(address.clone()) {
            continue;
        }
        let driver = fs::read_link(path.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let revision = read(path.join("revision"))
            .and_then(|text| u8::from_str_radix(text.trim_start_matches("0x"), 16).ok());
        found.push(Card {
            address,
            vendor,
            device,
            revision,
            driver,
            card,
            path,
        });
    }
    found
}

/// Приводит PCI-адрес NVML (`00000000:01:00.0`) к виду sysfs (`0000:01:00.0`).
pub fn normalize_bus_id(bus_id: &str) -> String {
    let bus_id = bus_id.trim().to_ascii_lowercase();
    match bus_id.split_once(':') {
        Some((domain, rest)) if domain.len() > 4 => {
            format!("{}:{rest}", &domain[domain.len() - 4..])
        }
        _ => bus_id,
    }
}

fn vendor_label(vendor: u16) -> &'static str {
    match vendor {
        VENDOR_AMD => "AMD",
        VENDOR_INTEL => "Intel",
        VENDOR_NVIDIA => "NVIDIA",
        _ => "GPU",
    }
}

/// Название устройства из базы `pci.ids`: берётся маркетинговое имя в квадратных скобках, если оно есть.
pub fn lookup_name(pci_ids: &str, vendor: u16, device: u16) -> Option<String> {
    let vendor_tag = format!("{vendor:04x}");
    let device_tag = format!("{device:04x}");
    let mut in_vendor = false;
    for line in pci_ids.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                return None;
            }
            in_vendor = line.len() > 4 && line[..4].eq_ignore_ascii_case(&vendor_tag);
            continue;
        }
        if !in_vendor || line.starts_with("\t\t") {
            continue;
        }
        let line = &line[1..];
        if line.len() > 4 && line[..4].eq_ignore_ascii_case(&device_tag) {
            let name = line[4..].trim();
            let name = match (name.find('['), name.rfind(']')) {
                (Some(open), Some(close)) if close > open + 1 => &name[open + 1..close],
                _ => name,
            };
            return Some(name.trim().to_owned());
        }
    }
    None
}

/// Маркетинговое имя AMD из `amdgpu.ids` (libdrm) по точной паре «устройство, ревизия».
pub fn lookup_amdgpu(amdgpu_ids: &str, device: u16, revision: u8) -> Option<String> {
    amdgpu_ids.lines().find_map(|line| {
        let mut fields = line.splitn(3, ',');
        let id = u16::from_str_radix(fields.next()?.trim(), 16).ok()?;
        let rev = u8::from_str_radix(fields.next()?.trim(), 16).ok()?;
        let name = fields.next()?.trim();
        (id == device && rev == revision && !name.is_empty()).then(|| name.to_owned())
    })
}

pub fn display_name(card: &Card, pci_ids: Option<&str>, amdgpu_ids: Option<&str>) -> String {
    let vendor = vendor_label(card.vendor);
    if card.vendor == VENDOR_AMD
        && let (Some(ids), Some(revision)) = (amdgpu_ids, card.revision)
        && let Some(name) = lookup_amdgpu(ids, card.device, revision)
    {
        return name;
    }
    match pci_ids.and_then(|ids| lookup_name(ids, card.vendor, card.device)) {
        Some(name)
            if name
                .to_ascii_lowercase()
                .starts_with(&vendor.to_ascii_lowercase()) =>
        {
            name
        }
        Some(name) => format!("{vendor} {name}"),
        None => format!("{vendor} GPU {:04x}:{:04x}", card.vendor, card.device),
    }
}

fn amdgpu_ids() -> Option<String> {
    [
        "/usr/share/libdrm/amdgpu.ids",
        "/usr/local/share/libdrm/amdgpu.ids",
    ]
    .into_iter()
    .find_map(|path| fs::read_to_string(path).ok())
}

fn pci_ids() -> Option<String> {
    [
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/pci.ids",
    ]
    .into_iter()
    .find_map(|path| fs::read_to_string(path).ok())
}

fn hwmon(device: &Path) -> Option<PathBuf> {
    let mut list: Vec<_> = fs::read_dir(device.join("hwmon"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    list.sort();
    list.into_iter().next()
}

/// Температура: у amdgpu предпочтительна `edge`, у остальных — первый датчик.
fn hwmon_temperature(hwmon: &Path) -> Option<PathBuf> {
    for index in 1..=8 {
        if read(hwmon.join(format!("temp{index}_label"))).is_some_and(|label| label == "edge") {
            return Some(hwmon.join(format!("temp{index}_input")));
        }
    }
    first_existing(hwmon, &["temp1_input"])
}

/// Счётчик простоя графического ядра Intel в миллисекундах.
fn intel_idle_counter(card: &Card) -> Option<PathBuf> {
    first_existing(
        &card.card,
        &[
            "gt/gt0/rc6_residency_ms",
            "power/rc6_residency_ms",
            "device/tile0/gt0/gtidle/idle_residency_ms",
        ],
    )
}

fn intel_clock(card: &Card) -> Option<PathBuf> {
    first_existing(
        &card.card,
        &[
            "gt_act_freq_mhz",
            "gt/gt0/rps_act_freq_mhz",
            "device/tile0/gt0/freq0/act_freq",
        ],
    )
}

/// Активная частота из таблиц DPM amdgpu (`1: 2100Mhz *`).
pub fn parse_dpm(text: &str) -> Option<f64> {
    let line = text.lines().find(|line| line.trim_end().ends_with('*'))?;
    let value = line.split(':').nth(1)?.trim();
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    let number: f64 = digits.parse().ok()?;
    Some(if value.to_ascii_lowercase().contains("ghz") {
        number * 1000.0
    } else {
        number
    })
}

#[derive(Default)]
struct Counter {
    last: Option<(u64, Instant)>,
}
impl Counter {
    /// Скорость изменения счётчика в единицах за секунду.
    fn rate(&mut self, value: Option<u64>, now: Instant) -> Option<f64> {
        let value = value?;
        let previous = self.last.replace((value, now));
        let (old, at) = previous?;
        let seconds = now.duration_since(at).as_secs_f64();
        (seconds > 0.0 && value >= old).then(|| (value - old) as f64 / seconds)
    }
}

#[derive(Default)]
struct CardState {
    idle: Counter,
    energy: Counter,
}

/// Счётчики одного клиента DRM из fdinfo.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Client {
    pub driver: String,
    pub pdev: String,
    pub id: u64,
    /// Время занятости движка в нс (`drm-engine-*`) либо такты (`drm-cycles-*`).
    pub busy: HashMap<String, u64>,
    /// Общее число тактов для `drm-cycles-*` (xe).
    pub total: HashMap<String, u64>,
    /// Число движков класса (`drm-engine-capacity-*`, i915).
    pub capacity: HashMap<String, u64>,
}

pub fn parse_fdinfo(text: &str) -> Option<Client> {
    let mut client = Client::default();
    let mut has_id = false;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let number = || {
            value
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
        };
        if key == "drm-driver" {
            client.driver = value.into();
        } else if key == "drm-pdev" {
            client.pdev = value.to_ascii_lowercase();
        } else if key == "drm-client-id" {
            client.id = number()?;
            has_id = true;
        } else if let Some(engine) = key.strip_prefix("drm-engine-capacity-") {
            client.capacity.insert(engine.into(), number().unwrap_or(1));
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            client.busy.insert(engine.into(), number().unwrap_or(0));
        } else if let Some(engine) = key.strip_prefix("drm-total-cycles-") {
            client.total.insert(engine.into(), number().unwrap_or(0));
        } else if let Some(engine) = key.strip_prefix("drm-cycles-") {
            client.busy.insert(engine.into(), number().unwrap_or(0));
        }
    }
    (has_id && !client.driver.is_empty()).then_some(client)
}

/// Загрузка движков клиента между двумя замерами, % по каждому движку.
pub fn engine_usage(before: &Client, after: &Client, elapsed_ns: f64) -> HashMap<String, f64> {
    let mut usage = HashMap::new();
    for (engine, busy) in &after.busy {
        let Some(old) = before.busy.get(engine) else {
            continue;
        };
        let delta = busy.saturating_sub(*old) as f64;
        let percent = match (after.total.get(engine), before.total.get(engine)) {
            (Some(total), Some(old_total)) => {
                let span = total.saturating_sub(*old_total) as f64;
                if span <= 0.0 {
                    continue;
                }
                delta * 100.0 / span
            }
            _ => {
                if elapsed_ns <= 0.0 {
                    continue;
                }
                let capacity = after.capacity.get(engine).copied().unwrap_or(1).max(1) as f64;
                delta * 100.0 / elapsed_ns / capacity
            }
        };
        usage.insert(engine.clone(), percent.clamp(0.0, 100.0));
    }
    usage
}

/// Складывает движки одного типа, как диспетчер задач, и берёт самый загруженный тип.
fn busiest(engines: &HashMap<String, f64>) -> f64 {
    engines
        .values()
        .copied()
        .fold(0.0, f64::max)
        .clamp(0.0, 100.0)
}

/// Клиенты DRM всех доступных процессов: (pdev, client id) → (pid, счётчики).
fn scan_clients(proc_root: &Path) -> Clients {
    let mut clients = HashMap::new();
    let Ok(processes) = fs::read_dir(proc_root) else {
        return clients;
    };
    for process in processes.flatten() {
        let Some(pid) = process
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(fds) = fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let is_drm =
                fs::read_link(fd.path()).is_ok_and(|target| target.starts_with("/dev/dri"));
            if !is_drm {
                continue;
            }
            let info = process.path().join("fdinfo").join(fd.file_name());
            let Some(client) = fs::read_to_string(info).ok().and_then(|t| parse_fdinfo(&t)) else {
                continue;
            };
            if client.busy.is_empty() {
                continue;
            }
            clients
                .entry((client.pdev.clone(), client.id))
                .or_insert((pid, client));
        }
    }
    clients
}

/// Клиенты DRM: (pdev, client id) → (pid, счётчики).
pub type Clients = HashMap<(String, u64), (u32, Client)>;

/// Нагрузка процессов и итоговая нагрузка адаптера по данным fdinfo.
#[derive(Debug, Default, Clone)]
pub struct ClientUsage {
    pub processes: HashMap<String, HashMap<u32, f64>>,
    pub devices: HashMap<String, f64>,
}

pub fn client_usage(before: &Clients, after: &Clients, elapsed_ns: f64) -> ClientUsage {
    let mut per_process: HashMap<(String, u32), HashMap<String, f64>> = HashMap::new();
    let mut per_device: HashMap<String, HashMap<String, f64>> = HashMap::new();
    for (key, (pid, client)) in after {
        let Some((_, old)) = before.get(key) else {
            continue;
        };
        for (engine, value) in engine_usage(old, client, elapsed_ns) {
            *per_process
                .entry((client.pdev.clone(), *pid))
                .or_default()
                .entry(engine.clone())
                .or_default() += value;
            *per_device
                .entry(client.pdev.clone())
                .or_default()
                .entry(engine)
                .or_default() += value;
        }
    }
    let mut usage = ClientUsage::default();
    for ((pdev, pid), engines) in per_process {
        usage
            .processes
            .entry(pdev)
            .or_default()
            .insert(pid, busiest(&engines));
    }
    for (pdev, engines) in per_device {
        usage.devices.insert(pdev, busiest(&engines));
    }
    usage
}

pub struct Gpus {
    root: PathBuf,
    proc_root: PathBuf,
    names: HashMap<String, String>,
    states: HashMap<String, CardState>,
    clients: Option<(Clients, Instant)>,
}

impl Default for Gpus {
    fn default() -> Self {
        Self::new("/sys/class/drm", "/proc")
    }
}

impl Gpus {
    pub fn new(root: impl Into<PathBuf>, proc_root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            proc_root: proc_root.into(),
            names: HashMap::new(),
            states: HashMap::new(),
            clients: None,
        }
    }

    pub fn cards(&self) -> Vec<Card> {
        cards(&self.root)
    }

    pub fn name(&mut self, card: &Card) -> String {
        if let Some(name) = self.names.get(&card.address) {
            return name.clone();
        }
        let amdgpu = (card.vendor == VENDOR_AMD).then(amdgpu_ids).flatten();
        let name = display_name(card, pci_ids().as_deref(), amdgpu.as_deref());
        self.names.insert(card.address.clone(), name.clone());
        name
    }

    /// Обходит fdinfo процессов; без предыдущего замера возвращает пустую нагрузку.
    pub fn clients(&mut self) -> ClientUsage {
        let now = Instant::now();
        let current = scan_clients(&self.proc_root);
        let usage = match &self.clients {
            Some((previous, at)) => client_usage(
                previous,
                &current,
                now.duration_since(*at).as_nanos() as f64,
            ),
            None => ClientUsage::default(),
        };
        self.clients = Some((current, now));
        usage
    }

    pub fn has_client_history(&self) -> bool {
        self.clients.is_some()
    }

    pub fn read(&mut self, card: &Card) -> Reading {
        let now = Instant::now();
        let state = self.states.entry(card.address.clone()).or_default();
        let device = &card.path;
        let hwmon = hwmon(device);
        let mut reading = Reading::default();

        if let Some(path) = first_existing(device, &["gpu_busy_percent"]) {
            reading.load = Some(read_u64(path).map(|v| (v as f64).min(100.0)));
        } else if card.is_intel()
            && let Some(path) = intel_idle_counter(card)
        {
            // Счётчик простоя в мс растёт на 1000 за секунду полного простоя.
            reading.load = Some(
                state
                    .idle
                    .rate(read_u64(path), now)
                    .map(|idle_ms_per_s| (100.0 - idle_ms_per_s / 10.0).clamp(0.0, 100.0)),
            );
        }

        if let Some(path) = first_existing(device, &["mem_info_vram_used"]) {
            reading.vram_used = Some(read_u64(path));
        }
        if let Some(path) = first_existing(device, &["mem_info_vram_total"]) {
            reading.vram_total = Some(read_u64(path).filter(|v| *v > 0));
        }

        if let Some(hwmon) = &hwmon {
            if let Some(path) = hwmon_temperature(hwmon) {
                reading.temperature = Some(read_u64(path).map(|v| v as f64 / 1000.0));
            }
            if let Some(path) = first_existing(hwmon, &["power1_average", "power1_input"]) {
                reading.power = Some(read_u64(path).map(|v| v as f64 / 1_000_000.0));
            } else if let Some(path) = first_existing(hwmon, &["energy1_input"]) {
                // Энергия в мкДж: производная даёт мощность.
                reading.power = Some(
                    state
                        .energy
                        .rate(read_u64(path), now)
                        .map(|uj_per_s| uj_per_s / 1_000_000.0),
                );
            }
            if let Some(path) = first_existing(hwmon, &["freq1_input"]) {
                reading.clock = Some(read_u64(path).map(|hz| hz as f64 / 1_000_000.0));
            }
            if let Some(path) = first_existing(hwmon, &["freq2_input"]) {
                reading.memory_clock = Some(read_u64(path).map(|hz| hz as f64 / 1_000_000.0));
            }
            if let Some(path) = first_existing(hwmon, &["pwm1"]) {
                let max = read_u64(hwmon.join("pwm1_max")).unwrap_or(255).max(1) as f64;
                reading.fan = Some(read_u64(path).map(|v| (v as f64 * 100.0 / max).min(100.0)));
            }
        }

        if reading.clock.is_none() {
            if let Some(path) = first_existing(device, &["pp_dpm_sclk"]) {
                reading.clock = Some(read(path).as_deref().and_then(parse_dpm));
            } else if card.is_intel()
                && let Some(path) = intel_clock(card)
            {
                reading.clock = Some(read_u64(path).map(|v| v as f64));
            }
        }
        if reading.memory_clock.is_none()
            && let Some(path) = first_existing(device, &["pp_dpm_mclk"])
        {
            reading.memory_clock = Some(read(path).as_deref().and_then(parse_dpm));
        }
        reading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mh-sidebar-drm-{name}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn fake_card(root: &Path, card: &str, address: &str, vendor: &str, class: &str) -> PathBuf {
        let device = root.join("devices").join(address);
        write(&device.join("class"), class);
        write(&device.join("vendor"), vendor);
        write(&device.join("device"), "0x73bf\n");
        write(&device.join("revision"), "0xc1\n");
        fs::create_dir_all(root.join("drm").join(card)).unwrap();
        std::os::unix::fs::symlink(&device, root.join("drm").join(card).join("device")).unwrap();
        device
    }

    #[test]
    fn finds_display_cards_once_and_skips_connectors() {
        let root = temp_root("cards");
        fake_card(&root, "card1", "0000:03:00.0", "0x1002\n", "0x030000\n");
        fake_card(&root, "card0", "0000:00:02.0", "0x8086\n", "0x038000\n");
        fake_card(&root, "card2", "0000:05:00.0", "0x1b21\n", "0x010802\n");
        fs::create_dir_all(root.join("drm/card1-DP-1")).unwrap();
        let found = cards(&root.join("drm"));
        let addresses: Vec<_> = found.iter().map(|c| c.address.as_str()).collect();
        assert_eq!(addresses, ["0000:00:02.0", "0000:03:00.0"]);
        assert_eq!(found[1].vendor, VENDOR_AMD);
        assert_eq!(found[1].revision, Some(0xc1));
        assert_eq!(found[1].id(), "gpu:pci:0000:03:00.0");
        assert!(found[0].is_intel());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_amdgpu_sensors() {
        let root = temp_root("amd");
        let device = fake_card(&root, "card0", "0000:03:00.0", "0x1002", "0x030000");
        write(&device.join("gpu_busy_percent"), "42\n");
        write(&device.join("mem_info_vram_used"), "1073741824\n");
        write(&device.join("mem_info_vram_total"), "17163091968\n");
        write(&device.join("pp_dpm_mclk"), "0: 96Mhz\n1: 1000Mhz *\n");
        let hwmon = device.join("hwmon/hwmon3");
        write(&hwmon.join("temp1_label"), "edge\n");
        write(&hwmon.join("temp1_input"), "51000\n");
        write(&hwmon.join("temp2_label"), "junction\n");
        write(&hwmon.join("temp2_input"), "60000\n");
        write(&hwmon.join("power1_average"), "35000000\n");
        write(&hwmon.join("freq1_input"), "2100000000\n");
        write(&hwmon.join("pwm1"), "51\n");
        let mut gpus = Gpus::new(root.join("drm"), root.join("proc"));
        let card = gpus.cards().remove(0);
        let reading = gpus.read(&card);
        assert_eq!(reading.load, Some(Some(42.0)));
        assert_eq!(reading.temperature, Some(Some(51.0)));
        assert_eq!(reading.power, Some(Some(35.0)));
        assert_eq!(reading.clock, Some(Some(2100.0)));
        assert_eq!(reading.memory_clock, Some(Some(1000.0)));
        assert_eq!(reading.vram_used, Some(Some(1 << 30)));
        assert_eq!(reading.fan, Some(Some(20.0)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn intel_load_comes_from_idle_residency() {
        let root = temp_root("intel");
        fake_card(&root, "card0", "0000:00:02.0", "0x8086", "0x030000");
        let card_dir = root.join("drm/card0");
        write(&card_dir.join("gt_act_freq_mhz"), "1300\n");
        write(&card_dir.join("gt/gt0/rc6_residency_ms"), "1000\n");
        let mut gpus = Gpus::new(root.join("drm"), root.join("proc"));
        let card = gpus.cards().remove(0);
        let first = gpus.read(&card);
        assert_eq!(first.load, Some(None));
        assert_eq!(first.clock, Some(Some(1300.0)));
        assert_eq!(first.vram_total, None);
        std::thread::sleep(std::time::Duration::from_millis(200));
        // За ~200 мс ядро простаивало ~50 мс → загрузка около 75 %.
        write(&card_dir.join("gt/gt0/rc6_residency_ms"), "1050\n");
        let load = gpus.read(&card).load.flatten().unwrap();
        assert!((50.0..=80.0).contains(&load), "{load}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_fdinfo_for_amdgpu_i915_and_xe() {
        let amd = parse_fdinfo(
            "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\ndrm-pdev:\t0000:03:00.0\ndrm-client-id:\t17\ndrm-engine-gfx:\t1500 ns\ndrm-engine-compute:\t0 ns\ndrm-memory-vram:\t1024 KiB\n",
        )
        .unwrap();
        assert_eq!(amd.driver, "amdgpu");
        assert_eq!(amd.pdev, "0000:03:00.0");
        assert_eq!(amd.id, 17);
        assert_eq!(amd.busy["gfx"], 1500);
        let i915 = parse_fdinfo(
            "drm-driver:\ti915\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t3\ndrm-engine-render:\t900 ns\ndrm-engine-capacity-video:\t2\ndrm-engine-video:\t10 ns\n",
        )
        .unwrap();
        assert_eq!(i915.capacity["video"], 2);
        assert_eq!(i915.busy["video"], 10);
        let xe = parse_fdinfo(
            "drm-driver:\txe\ndrm-pdev:\t0000:03:00.0\ndrm-client-id:\t9\ndrm-cycles-rcs:\t500\ndrm-total-cycles-rcs:\t1000\n",
        )
        .unwrap();
        assert_eq!(xe.busy["rcs"], 500);
        assert_eq!(xe.total["rcs"], 1000);
        assert!(parse_fdinfo("pos:\t0\nflags:\t0\n").is_none());
    }

    #[test]
    fn client_usage_uses_busy_time_cycles_and_capacity() {
        let client =
            |pdev: &str, id, busy: &[(&str, u64)], total: &[(&str, u64)], cap: &[(&str, u64)]| {
                Client {
                    driver: "test".into(),
                    pdev: pdev.into(),
                    id,
                    busy: busy.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
                    total: total.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
                    capacity: cap.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
                }
            };
        let amd = "0000:03:00.0".to_string();
        let intel = "0000:00:02.0".to_string();
        let mut before = HashMap::new();
        let mut after = HashMap::new();
        // AMD: 30 % gfx у процесса 10 и 20 % у процесса 11.
        before.insert(
            (amd.clone(), 1),
            (10, client(&amd, 1, &[("gfx", 0)], &[], &[])),
        );
        after.insert(
            (amd.clone(), 1),
            (10, client(&amd, 1, &[("gfx", 300_000_000)], &[], &[])),
        );
        before.insert(
            (amd.clone(), 2),
            (11, client(&amd, 2, &[("gfx", 0), ("compute", 0)], &[], &[])),
        );
        after.insert(
            (amd.clone(), 2),
            (
                11,
                client(
                    &amd,
                    2,
                    &[("gfx", 200_000_000), ("compute", 100_000_000)],
                    &[],
                    &[],
                ),
            ),
        );
        // Intel i915: два видеодвижка заняты на 100 % → половина ёмкости класса.
        before.insert(
            (intel.clone(), 5),
            (12, client(&intel, 5, &[("video", 0)], &[], &[("video", 2)])),
        );
        after.insert(
            (intel.clone(), 5),
            (
                12,
                client(&intel, 5, &[("video", 1_000_000_000)], &[], &[("video", 2)]),
            ),
        );
        // xe: такты, а не наносекунды.
        before.insert(
            (intel.clone(), 6),
            (
                13,
                client(&intel, 6, &[("rcs", 100)], &[("rcs", 1000)], &[]),
            ),
        );
        after.insert(
            (intel.clone(), 6),
            (
                13,
                client(&intel, 6, &[("rcs", 350)], &[("rcs", 2000)], &[]),
            ),
        );
        // Новый клиент без предыдущего замера пропускается.
        after.insert(
            (amd.clone(), 7),
            (14, client(&amd, 7, &[("gfx", 999)], &[], &[])),
        );
        let usage = client_usage(&before, &after, 1_000_000_000.0);
        let round = |v: f64| (v * 10.0).round() / 10.0;
        assert_eq!(round(usage.processes[&amd][&10]), 30.0);
        assert_eq!(round(usage.processes[&amd][&11]), 20.0);
        assert!(!usage.processes[&amd].contains_key(&14));
        assert_eq!(round(usage.processes[&intel][&12]), 50.0);
        assert_eq!(round(usage.processes[&intel][&13]), 25.0);
        assert_eq!(round(usage.devices[&amd]), 50.0);
        assert_eq!(round(usage.devices[&intel]), 50.0);
    }

    #[test]
    fn names_come_from_pci_ids() {
        let ids = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]\n\t\t1002 0e3a  Radeon RX 6900 XT\n8086  Intel Corporation\n\t46a6  Alder Lake-P GT2 [Iris Xe Graphics]\n\t56a0  DG2 [Arc A770]\n";
        assert_eq!(
            lookup_name(ids, 0x1002, 0x73bf).as_deref(),
            Some("Radeon RX 6800/6800 XT / 6900 XT")
        );
        assert_eq!(lookup_name(ids, 0x8086, 0x0001), None);
        let card = |vendor, device| Card {
            address: String::new(),
            vendor,
            device,
            revision: None,
            driver: String::new(),
            card: PathBuf::new(),
            path: PathBuf::new(),
        };
        assert_eq!(
            display_name(&card(0x1002, 0x73bf), Some(ids), None),
            "AMD Radeon RX 6800/6800 XT / 6900 XT"
        );
        assert_eq!(
            display_name(&card(0x8086, 0x46a6), Some(ids), None),
            "Intel Iris Xe Graphics"
        );
        assert_eq!(
            display_name(&card(0x8086, 0x9999), None, None),
            "Intel GPU 8086:9999"
        );
        let amdgpu = "# device_id,\trevision_id,\tproduct_name\n1.0.0\n73DF,\tC0,\tAMD Radeon RX 6750 XT\n73DF,\tC1,\tAMD Radeon RX 6700 XT\n73DF,\tDF,\tAMD Radeon RX 6700\n";
        assert_eq!(
            lookup_amdgpu(amdgpu, 0x73df, 0xc1).as_deref(),
            Some("AMD Radeon RX 6700 XT")
        );
        assert_eq!(lookup_amdgpu(amdgpu, 0x73df, 0xaa), None);
        let navi22 = |revision| Card {
            device: 0x73df,
            revision,
            ..card(0x1002, 0x73df)
        };
        let ids_navi22 = "1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t73df  Navi 22 [Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT]\n";
        assert_eq!(
            display_name(&navi22(Some(0xdf)), Some(ids_navi22), Some(amdgpu)),
            "AMD Radeon RX 6700"
        );
        // Неизвестная ревизия или нет libdrm — общее имя кристалла из pci.ids.
        assert_eq!(
            display_name(&navi22(Some(0xaa)), Some(ids_navi22), Some(amdgpu)),
            "AMD Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT"
        );
        assert_eq!(
            display_name(&navi22(Some(0xc1)), Some(ids_navi22), None),
            "AMD Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT"
        );
        assert_eq!(normalize_bus_id("00000000:01:00.0"), "0000:01:00.0");
        assert_eq!(parse_dpm("0: 500Mhz\n1: 2100Mhz *\n"), Some(2100.0));
    }
}
