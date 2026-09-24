use crate::model::{Block, Section, Snapshot};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

pub fn metric_key(block: Block, device_id: &str, metric: &str) -> String {
    format!("{block:?}/{}:{device_id}/{metric}", device_id.len())
}
struct DeviceHistory {
    history: History,
    last_sample: Instant,
    gap: bool,
    meta: SeriesMeta,
}
pub struct SeriesMeta {
    pub block: Block,
    pub device_id: String,
    pub device: String,
    pub metric: String,
    pub label: String,
    pub unit: String,
}
#[derive(Default)]
pub struct Histories {
    entries: HashMap<String, DeviceHistory>,
    devices: HashMap<(Block, String), (Section, Instant)>,
}
impl Histories {
    pub fn series(&self) -> impl Iterator<Item = (&SeriesMeta, &History)> {
        self.entries
            .values()
            .map(|entry| (&entry.meta, &entry.history))
    }
    pub fn get(&self, key: &str) -> Option<&History> {
        self.entries.get(key).map(|e| &e.history)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.devices.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn observe(
        &mut self,
        snapshot: &Snapshot,
        now: Instant,
        origin: Instant,
        window: Duration,
    ) {
        let mut active = HashSet::new();
        for section in &snapshot.sections {
            if let Some(at) = section
                .rows
                .iter()
                .filter(|r| !r.stale)
                .filter_map(|r| r.sampled_at)
                .max()
            {
                self.devices.insert(
                    (section.id, section.device_id.clone()),
                    (section.clone(), at),
                );
            }
            for row in &section.rows {
                let key = metric_key(section.id, &section.device_id, &row.key);
                active.insert(key.clone());
                if row.stale {
                    continue;
                }
                let Some(at) = row.sampled_at else {
                    continue;
                };
                if now.saturating_duration_since(at) > window {
                    continue;
                }
                let time = at.saturating_duration_since(origin).as_secs_f64();
                let meta = SeriesMeta {
                    block: section.id,
                    device_id: section.device_id.clone(),
                    device: section.device.clone(),
                    metric: row.key.clone(),
                    label: row.label.clone(),
                    unit: row.unit.clone(),
                };
                match self.entries.entry(key) {
                    std::collections::hash_map::Entry::Vacant(v) => {
                        let mut history = History::default();
                        history.push(time, row.value);
                        v.insert(DeviceHistory {
                            history,
                            last_sample: at,
                            gap: false,
                            meta,
                        });
                    }
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        let entry = o.get_mut();
                        entry.meta = meta;
                        if at > entry.last_sample {
                            entry.history.push(time, row.value);
                            entry.last_sample = at;
                            entry.gap = false;
                        }
                    }
                }
            }
        }
        let stale: HashSet<_> = snapshot
            .sections
            .iter()
            .flat_map(|s| {
                s.rows
                    .iter()
                    .filter(|r| r.stale)
                    .map(|r| metric_key(s.id, &s.device_id, &r.key))
            })
            .collect();
        for (key, entry) in &mut self.entries {
            if (!active.contains(key) || stale.contains(key)) && !entry.gap {
                entry
                    .history
                    .push(now.saturating_duration_since(origin).as_secs_f64(), None);
                entry.gap = true;
            }
        }
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.last_sample) <= window);
        self.devices
            .retain(|_, (_, at)| now.saturating_duration_since(*at) <= window);
    }
    pub fn missing_sections(&self, current: &Snapshot) -> Vec<Section> {
        self.devices
            .values()
            .filter(|(old, _)| {
                !current
                    .sections
                    .iter()
                    .any(|s| s.id == old.id && s.device_id == old.device_id)
            })
            .map(|(old, _)| {
                let mut section = old.clone();
                section.disconnected = true;
                for row in &mut section.rows {
                    row.value = None;
                    row.text = "—".into();
                    row.stale = true;
                    row.reason = Some(
                        "Устройство отключено. История сохранена до конца выбранного окна.".into(),
                    );
                }
                section
            })
            .collect()
    }
}

#[derive(Default)]
pub struct History {
    samples: VecDeque<(f64, Option<f64>)>,
}
impl History {
    pub fn push(&mut self, time: f64, value: Option<f64>) {
        if !time.is_finite() {
            return;
        }
        self.samples
            .push_back((time, value.filter(|v| v.is_finite())));
        while self.samples.front().is_some_and(|(t, _)| time - t > 600.)
            || self.samples.len() > 1202
        {
            self.samples.pop_front();
        }
    }
    pub fn points(&self, now: f64, window: Duration) -> Vec<(f64, Option<f64>)> {
        self.samples
            .iter()
            .filter(|(t, _)| *t >= now - window.as_secs_f64())
            .copied()
            .collect()
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

pub fn rate(previous: u64, current: u64, seconds: f64) -> Option<f64> {
    if !seconds.is_finite() || seconds <= 0. {
        return None;
    }
    current
        .checked_sub(previous)
        .map(|delta| delta as f64 / seconds)
}
