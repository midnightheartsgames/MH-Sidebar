use crate::{
    history::{Histories, History},
    model::Block,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub min: f64,
    pub mean: f64,
    pub max: f64,
    pub count: usize,
    pub min_at: f64,
    pub max_at: f64,
}
impl Stats {
    pub fn from_history(history: &History, now: f64, window: Duration) -> Option<Self> {
        let mut values = history
            .points(now, window)
            .into_iter()
            .filter_map(|(t, v)| v.filter(|v| v.is_finite()).map(|v| (t, v)));
        let (first_at, first) = values.next()?;
        let mut result = Self {
            min: first,
            mean: first,
            max: first,
            count: 1,
            min_at: first_at,
            max_at: first_at,
        };
        let mut sum = first;
        for (at, value) in values {
            if value < result.min {
                result.min = value;
                result.min_at = at;
            }
            if value > result.max {
                result.max = value;
                result.max_at = at;
            }
            sum += value;
            result.count += 1;
        }
        result.mean = sum / result.count as f64;
        Some(result)
    }
}

fn field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

pub const CSV_HEADER: &str = "unix_ms,elapsed_s,block,device_id,device,metric,label,unit,value\n";

pub struct MetricDescriptor<'a> {
    pub block: &'a str,
    pub device_id: &'a str,
    pub device: &'a str,
    pub metric: &'a str,
    pub label: &'a str,
    pub unit: &'a str,
}

pub fn csv_series(
    history: &History,
    now: f64,
    window: Duration,
    origin: SystemTime,
    descriptor: MetricDescriptor<'_>,
) -> String {
    let mut output = String::from(CSV_HEADER);
    for (seconds, value) in history.points(now, window) {
        let wall = origin
            .checked_add(Duration::from_secs_f64(seconds.max(0.)))
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let value = value.map(|v| v.to_string()).unwrap_or_default();
        output.push_str(&format!(
            "{wall},{seconds:.3},{},{},{},{},{},{},{value}\n",
            field(descriptor.block),
            field(descriptor.device_id),
            field(descriptor.device),
            field(descriptor.metric),
            field(descriptor.label),
            field(descriptor.unit)
        ));
    }
    output
}

pub fn csv_all(histories: &Histories, now: f64, window: Duration, origin: SystemTime) -> String {
    let mut rows = Vec::new();
    for (meta, history) in histories.series() {
        let block = format!("{:?}", meta.block);
        for (seconds, value) in history.points(now, window) {
            let wall = origin
                .checked_add(Duration::from_secs_f64(seconds.max(0.)))
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0);
            rows.push((
                wall,
                seconds,
                block.clone(),
                meta.device_id.clone(),
                meta.device.clone(),
                meta.metric.clone(),
                meta.label.clone(),
                meta.unit.clone(),
                value,
            ));
        }
    }
    rows.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
            .then(a.5.cmp(&b.5))
    });
    let mut output = String::from(CSV_HEADER);
    for (wall, seconds, block, id, device, metric, label, unit, value) in rows {
        let value = value.map(|v| v.to_string()).unwrap_or_default();
        output.push_str(&format!(
            "{wall},{seconds:.3},{},{},{},{},{},{},{value}\n",
            field(&block),
            field(&id),
            field(&device),
            field(&metric),
            field(&label),
            field(&unit)
        ));
    }
    output
}

pub fn default_metric(block: Block) -> &'static str {
    match block {
        Block::Network => "download",
        Block::Clock => "",
        _ => "load",
    }
}
