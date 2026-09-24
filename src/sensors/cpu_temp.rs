use super::pawnio::{PawnIo, PawnIoError, PciAccessLock};
use crate::model::DriverStatus;

const AMD_FAMILY17_MODULE: &[u8] = include_bytes!("../../assets/pawnio/AMDFamily17.bin");
const INTEL_MSR_MODULE: &[u8] = include_bytes!("../../assets/pawnio/IntelMSR.bin");
const PCI_LOCK_TIMEOUT_MS: u32 = 50;
const ZEN_REPORTED_TEMP_CTRL: u64 = 0x0005_9800;
const MSR_AMD_RAPL_POWER_UNIT: u64 = 0xC001_0299;
const MSR_AMD_PKG_ENERGY_STATUS: u64 = 0xC001_029B;
const MSR_IA32_TEMPERATURE_TARGET: u64 = 0x1A2;
const MSR_IA32_PACKAGE_THERM_STATUS: u64 = 0x1B1;
const MSR_IA32_THERM_STATUS: u64 = 0x19C;
const MSR_RAPL_POWER_UNIT: u64 = 0x606;
const MSR_PKG_ENERGY_STATUS: u64 = 0x611;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vendor {
    Amd,
    Intel,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CpuIdentity {
    vendor: Vendor,
    family: u32,
    brand: String,
    thermal_leaf: u32,
}

impl CpuIdentity {
    fn is_amd_zen(&self) -> bool {
        self.vendor == Vendor::Amd && (0x17..=0x1A).contains(&self.family)
    }
}
fn decode_family(eax: u32) -> u32 {
    let base = (eax >> 8) & 0xF;
    if base == 0xF {
        base + ((eax >> 20) & 0xFF)
    } else {
        base
    }
}

fn decode_vendor(ebx: u32, edx: u32, ecx: u32) -> Vendor {
    let mut bytes = [0u8; 12];
    bytes[..4].copy_from_slice(&ebx.to_le_bytes());
    bytes[4..8].copy_from_slice(&edx.to_le_bytes());
    bytes[8..].copy_from_slice(&ecx.to_le_bytes());
    match &bytes {
        b"AuthenticAMD" => Vendor::Amd,
        b"GenuineIntel" => Vendor::Intel,
        _ => Vendor::Other,
    }
}

#[cfg(target_arch = "x86_64")]
fn identify() -> CpuIdentity {
    use std::arch::x86_64::__cpuid;
    let leaf0 = __cpuid(0);
    let mut brand = Vec::with_capacity(48);
    if __cpuid(0x8000_0000).eax >= 0x8000_0004 {
        for leaf in 0x8000_0002..=0x8000_0004 {
            let regs = __cpuid(leaf);
            for register in [regs.eax, regs.ebx, regs.ecx, regs.edx] {
                brand.extend_from_slice(&register.to_le_bytes());
            }
        }
    }
    CpuIdentity {
        vendor: decode_vendor(leaf0.ebx, leaf0.edx, leaf0.ecx),
        family: decode_family(__cpuid(1).eax),
        brand: String::from_utf8_lossy(&brand)
            .trim_matches(['\0', ' '])
            .to_string(),
        thermal_leaf: if leaf0.eax >= 6 { __cpuid(6).eax } else { 0 },
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn identify() -> CpuIdentity {
    CpuIdentity {
        vendor: Vendor::Other,
        family: 0,
        brand: String::new(),
        thermal_leaf: 0,
    }
}
fn decode_tctl(register: u32) -> f64 {
    let mut celsius = f64::from(register >> 21) * 0.125;
    if register & (1 << 19) != 0 || register & (0b11 << 16) == 0b11 << 16 {
        celsius -= 49.0;
    }
    celsius
}
fn tctl_offset(family: u32, brand: &str) -> f64 {
    const OFFSETS: &[(&str, f64)] = &[
        ("AMD Ryzen 5 1600X", 20.0),
        ("AMD Ryzen 7 1700X", 20.0),
        ("AMD Ryzen 7 1800X", 20.0),
        ("AMD Ryzen 7 2700X", 10.0),
        ("AMD Ryzen Threadripper 19", 27.0),
        ("AMD Ryzen Threadripper 29", 27.0),
    ];
    if family != 0x17 {
        return 0.0;
    }
    OFFSETS
        .iter()
        .find(|(model, _)| brand.contains(model))
        .map_or(0.0, |(_, offset)| *offset)
}
fn tj_max(temperature_target: u64) -> f64 {
    match (temperature_target >> 16) & 0xFF {
        0 => 100.0,
        value => value as f64,
    }
}
fn intel_temperature(status: u64, tj_max: f64) -> f64 {
    tj_max - ((status >> 16) & 0x7F) as f64
}
fn energy_unit_joules(power_unit_register: u64) -> f64 {
    1.0 / f64::from(2u32).powi(((power_unit_register >> 8) & 0x1F) as i32)
}
#[derive(Debug, Clone)]
struct EnergyMeter {
    unit_joules: f64,
    previous: Option<(u32, u64)>,
}

impl EnergyMeter {
    fn new(unit_joules: f64) -> Self {
        Self {
            unit_joules,
            previous: None,
        }
    }
    fn update(&mut self, counter: u32, now_ms: u64) -> Option<f64> {
        let Some((previous_counter, previous_ms)) = self.previous else {
            self.previous = Some((counter, now_ms));
            return None;
        };
        let elapsed_ms = now_ms.saturating_sub(previous_ms);
        if elapsed_ms < 50 {
            return None;
        }
        self.previous = Some((counter, now_ms));
        let delta = counter.wrapping_sub(previous_counter);
        let watts = f64::from(delta) * self.unit_joules / (elapsed_ms as f64 / 1_000.0);
        (watts.is_finite() && (0.0..=1_000.0).contains(&watts)).then_some(watts)
    }
}

enum Kind {
    Amd {
        lock: Option<PciAccessLock>,
        tctl_offset: f64,
    },
    Intel {
        tj_max: f64,
        package_sensor: bool,
    },
}
pub struct CpuSensor {
    pawnio: PawnIo,
    kind: Kind,
    meter: Option<EnergyMeter>,
}

fn status(error: PawnIoError) -> DriverStatus {
    match error {
        PawnIoError::NotInstalled => DriverStatus::NotInstalled,
        PawnIoError::AccessDenied => DriverStatus::NeedsAdmin,
        _ => DriverStatus::Failed,
    }
}

impl CpuSensor {
    pub fn open() -> Result<Self, DriverStatus> {
        let cpu = identify();
        let (module, power_unit) = if cpu.is_amd_zen() {
            (AMD_FAMILY17_MODULE, MSR_AMD_RAPL_POWER_UNIT)
        } else if cpu.vendor == Vendor::Intel && cpu.thermal_leaf & 1 != 0 {
            (INTEL_MSR_MODULE, MSR_RAPL_POWER_UNIT)
        } else {
            return Err(DriverStatus::Unsupported);
        };
        let pawnio = PawnIo::open_with_module(module).map_err(status)?;
        let kind = if cpu.vendor == Vendor::Amd {
            Kind::Amd {
                lock: PciAccessLock::open().ok(),
                tctl_offset: tctl_offset(cpu.family, &cpu.brand),
            }
        } else {
            Kind::Intel {
                tj_max: tj_max(
                    pawnio
                        .call("ioctl_read_msr", MSR_IA32_TEMPERATURE_TARGET)
                        .unwrap_or(0),
                ),
                package_sensor: cpu.thermal_leaf & (1 << 6) != 0,
            }
        };
        let meter = pawnio
            .call("ioctl_read_msr", power_unit)
            .ok()
            .map(|register| EnergyMeter::new(energy_unit_joules(register)));
        Ok(Self {
            pawnio,
            kind,
            meter,
        })
    }
    pub fn read(&mut self, now_ms: u64) -> (Option<f64>, Option<f64>) {
        let msr = |register| self.pawnio.call("ioctl_read_msr", register).ok();
        let (temperature, energy) = match &self.kind {
            Kind::Amd { lock, tctl_offset } => {
                let tctl = lock
                    .as_ref()
                    .and_then(|lock| {
                        lock.with(PCI_LOCK_TIMEOUT_MS, || {
                            self.pawnio.call("ioctl_read_smn", ZEN_REPORTED_TEMP_CTRL)
                        })
                    })
                    .and_then(Result::ok);
                (
                    tctl.map(|r| decode_tctl(r as u32) - tctl_offset),
                    msr(MSR_AMD_PKG_ENERGY_STATUS),
                )
            }
            Kind::Intel {
                tj_max,
                package_sensor,
            } => {
                let temperature = if *package_sensor {
                    msr(MSR_IA32_PACKAGE_THERM_STATUS).map(|s| intel_temperature(s, *tj_max))
                } else {
                    msr(MSR_IA32_THERM_STATUS)
                        .filter(|s| s & (1 << 31) != 0)
                        .map(|s| intel_temperature(s, *tj_max))
                };
                (temperature, msr(MSR_PKG_ENERGY_STATUS))
            }
        };
        let power = energy.and_then(|counter| {
            self.meter
                .as_mut()
                .and_then(|meter| meter.update(counter as u32, now_ms))
        });
        (temperature, power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_modules_are_the_audited_ones() {
        assert_eq!(AMD_FAMILY17_MODULE.len(), 10_652);
        assert_eq!(INTEL_MSR_MODULE.len(), 5_324);
    }

    #[test]
    fn signatures_and_vendors_decode() {
        assert_eq!(decode_family(0x00A2_0F10), 0x19);
        assert_eq!(decode_family(0x0080_0F11), 0x17);
        assert_eq!(decode_family(0x0009_06ED), 0x6);
        let word = |t: &[u8]| u32::from_le_bytes(t.try_into().unwrap());
        let text = b"AuthenticAMD";
        assert_eq!(
            decode_vendor(word(&text[0..4]), word(&text[4..8]), word(&text[8..12])),
            Vendor::Amd
        );
    }

    fn tctl_register(celsius: f64, flags: u32) -> u32 {
        (((celsius * 8.0) as u32) << 21) | flags
    }

    #[test]
    fn tctl_decodes_both_range_shifts() {
        assert_eq!(decode_tctl(tctl_register(62.625, 0)), 62.625);
        assert_eq!(decode_tctl(tctl_register(94.0, 1 << 19)), 45.0);
        assert_eq!(decode_tctl(tctl_register(94.0, 0b11 << 16)), 45.0);
        assert_eq!(decode_tctl(tctl_register(94.0, 1 << 16)), 94.0);
    }

    #[test]
    fn only_early_ryzens_have_a_tctl_offset() {
        assert_eq!(
            tctl_offset(0x17, "AMD Ryzen 7 1700X Eight-Core Processor"),
            20.0
        );
        assert_eq!(
            tctl_offset(0x19, "AMD Ryzen 9 5900X 12-Core Processor"),
            0.0
        );
    }

    #[test]
    fn intel_temperature_is_tj_max_minus_the_readout() {
        assert_eq!(tj_max(0x0069_0A00), 105.0);
        assert_eq!(tj_max(0), 100.0);
        assert_eq!(intel_temperature((38 << 16) | 0x8800_0000, 100.0), 62.0);
    }

    #[test]
    fn energy_meter_handles_wraparound_and_noise() {
        let unit = 1.0 / 65_536.0;
        assert_eq!(energy_unit_joules(0x000A_1003), unit);
        let mut meter = EnergyMeter::new(unit);
        assert_eq!(meter.update(u32::MAX - 65_536 * 20 + 1, 0), None);
        let watts = meter.update(65_536 * 30, 500).unwrap();
        assert!((watts - 100.0).abs() < 1e-6, "watts = {watts}");
        assert_eq!(meter.update(65_536 * 31, 510), None, "10 мс — шум");
        let mut broken = EnergyMeter::new(1.0);
        broken.update(0, 0);
        assert_eq!(
            broken.update(5_000, 1_000),
            None,
            "5 кВт — испорченное чтение"
        );
    }
    #[test]
    fn this_machine_either_reads_or_explains() {
        match CpuSensor::open() {
            Ok(mut sensor) => {
                let (celsius, _) = sensor.read(0);
                let celsius = celsius.expect("температура читается");
                assert!((5.0..=110.0).contains(&celsius), "T = {celsius}");
            }
            Err(status) => assert_ne!(status, DriverStatus::Ready),
        }
    }
}
