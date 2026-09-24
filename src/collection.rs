use crate::{
    model::{DriverStatus, Snapshot, Source, SourceStatus},
    sensors::Sampler,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type SampleResult = Result<Snapshot, String>;
type SampleFn = Box<dyn FnMut(bool) -> SampleResult>;
struct SourceWorker {
    source: Source,
    latest: Arc<Mutex<Option<(Instant, SampleResult)>>>,
    interval: Arc<AtomicU64>,
    restart: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl SourceWorker {
    fn start(
        source: Source,
        interval: u64,
        wake: Arc<dyn Fn() + Send + Sync>,
        factory: impl FnOnce() -> SampleFn + Send + 'static,
    ) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(None));
        let delay = Arc::new(AtomicU64::new(interval));
        let restart = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (output, worker_delay, worker_restart, worker_stop) =
            (latest.clone(), delay.clone(), restart.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name(format!("mh-{}", source.title()))
            .spawn(move || {
                let mut sample = factory();
                while !worker_stop.load(Ordering::Acquire) {
                    let restart = worker_restart.swap(false, Ordering::AcqRel);
                    let caught =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sample(restart)));
                    let panicked = caught.is_err();
                    let result = caught.unwrap_or_else(|_| {
                        Err(
                            "Поток датчика завершил замер с паникой; требуется перезапуск датчиков"
                                .into(),
                        )
                    });
                    if let Ok(mut slot) = output.lock() {
                        *slot = Some((Instant::now(), result));
                    }
                    wake();
                    if panicked {
                        while !worker_stop.load(Ordering::Acquire)
                            && !worker_restart.load(Ordering::Acquire)
                        {
                            std::thread::park_timeout(Duration::from_secs(1));
                        }
                    } else {
                        std::thread::park_timeout(Duration::from_millis(
                            worker_delay.load(Ordering::Acquire),
                        ));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            source,
            latest,
            interval: delay,
            restart,
            stop,
            thread: Some(thread),
        })
    }
    fn wake(&self) {
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
    }
}
impl Drop for SourceWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.wake();
        if let Some(thread) = self.thread.take()
            && thread.is_finished()
        {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct SourceEntry {
    at: Option<Instant>,
    snapshot: Snapshot,
    error: Option<String>,
}
#[derive(Default)]
struct SourceCache {
    entries: HashMap<Source, SourceEntry>,
}
impl SourceCache {
    fn update(&mut self, source: Source, at: Instant, result: SampleResult) {
        let entry = self.entries.entry(source).or_default();
        match result {
            Ok(mut snapshot) => {
                for section in &mut snapshot.sections {
                    for row in &mut section.rows {
                        row.sampled_at = Some(at);
                    }
                }
                entry.at = Some(at);
                entry.snapshot = snapshot;
                entry.error = None;
            }
            Err(error) => entry.error = Some(error),
        }
    }
    fn snapshot(&self, now: Instant, timeout: Duration) -> Snapshot {
        let mut result = Snapshot::default();
        for source in Source::ALL {
            let entry = self.entries.get(&source);
            let age = entry
                .and_then(|e| e.at)
                .map(|at| now.saturating_duration_since(at));
            let error = entry.and_then(|e| e.error.clone());
            let stale = error.is_some() || age.is_none_or(|age| age > timeout);
            if let Some(entry) = entry {
                let mut snapshot = entry.snapshot.clone();
                if stale {
                    let reason = error.clone().unwrap_or_else(|| {
                        format!("{}: данные устарели, ожидание источника", source.title())
                    });
                    for section in &mut snapshot.sections {
                        for row in &mut section.rows {
                            row.value = None;
                            row.text = "—".into();
                            row.reason = Some(reason.clone());
                            row.stale = true;
                        }
                    }
                    if source == Source::CpuDriver {
                        snapshot.cpu_driver = DriverStatus::Unknown;
                        snapshot.cpu_diagnostic = reason;
                    }
                }
                result.merge(snapshot);
            }
            result.sources.push(SourceStatus {
                source,
                age,
                stale,
                error,
            });
        }
        result
    }
}

pub struct Collector {
    workers: Vec<SourceWorker>,
    cache: SourceCache,
    interval: u64,
}
impl Collector {
    pub fn start(interval: u64, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let mut workers = Vec::new();
        for source in Source::ALL {
            workers.push(SourceWorker::start(
                source,
                interval,
                wake.clone(),
                move || {
                    let mut sampler = Sampler::new();
                    Box::new(move |restart| {
                        if restart {
                            sampler = Sampler::new();
                        }
                        sampler.sample_source(source)
                    })
                },
            )?);
        }
        Ok(Self {
            workers,
            cache: SourceCache::default(),
            interval,
        })
    }
    pub fn set_interval(&mut self, interval: u64) {
        self.interval = interval;
        for worker in &self.workers {
            worker.interval.store(interval, Ordering::Release);
            worker.wake();
        }
    }
    pub fn restart(&mut self) {
        for worker in &self.workers {
            worker.restart.store(true, Ordering::Release);
            worker.wake();
        }
    }
    pub fn snapshot(&mut self, now: Instant) -> Snapshot {
        for worker in &self.workers {
            if let Some((at, result)) = worker.latest.lock().ok().and_then(|mut slot| slot.take()) {
                self.cache.update(worker.source, at, result);
            }
        }
        self.cache
            .snapshot(now, Duration::from_millis((self.interval * 3).max(5000)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Reading, Section};
    fn sample(id: &str, key: &str, value: f64) -> Snapshot {
        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Cpu,
                device_id: id.into(),
                device: "CPU".into(),
                rows: vec![Reading::number(key, key, Some(value), "%")],
            }],
            ..Default::default()
        }
    }
    #[test]
    fn stale_driver_does_not_invalidate_fresh_cpu_load() {
        let start = Instant::now();
        let mut cache = SourceCache::default();
        cache.update(
            Source::CpuDriver,
            start,
            Ok(sample("cpu:system", "temperature", 55.)),
        );
        cache.update(
            Source::Cpu,
            start + Duration::from_secs(8),
            Ok(sample("cpu:system", "load", 35.)),
        );
        let result = cache.snapshot(start + Duration::from_secs(9), Duration::from_secs(5));
        assert_eq!(result.sections.len(), 1);
        let rows = &result.sections[0].rows;
        assert_eq!(
            rows.iter().find(|r| r.key == "load").unwrap().value,
            Some(35.)
        );
        assert!(rows.iter().find(|r| r.key == "temperature").unwrap().stale);
        assert_eq!(
            rows.iter().find(|r| r.key == "temperature").unwrap().value,
            None
        );
    }
    #[test]
    fn failed_source_keeps_identity_and_recovers_without_stale_values() {
        let now = Instant::now();
        let mut cache = SourceCache::default();
        cache.update(Source::Cpu, now, Ok(sample("cpu:system", "load", 10.)));
        cache.update(Source::Cpu, now, Err("temporary failure".into()));
        let failed = cache.snapshot(now, Duration::from_secs(5));
        assert_eq!(failed.sections[0].device_id, "cpu:system");
        assert_eq!(failed.sections[0].rows[0].value, None);
        assert!(
            failed.sections[0].rows[0]
                .reason
                .as_ref()
                .unwrap()
                .contains("temporary failure")
        );
        cache.update(Source::Cpu, now, Ok(sample("cpu:system", "load", 20.)));
        assert_eq!(
            cache.snapshot(now, Duration::from_secs(5)).sections[0].rows[0].value,
            Some(20.)
        );
        cache.update(Source::Cpu, now, Ok(Snapshot::default()));
        assert!(
            cache
                .snapshot(now, Duration::from_secs(5))
                .sections
                .is_empty()
        );
    }
    #[test]
    fn blocked_worker_does_not_block_other_sources_or_drop() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (healthy_tx, healthy_rx) = std::sync::mpsc::channel();
        let blocked = SourceWorker::start(Source::Disks, 1000, Arc::new(|| {}), move || {
            Box::new(move |_| {
                entered_tx.send(()).unwrap();
                let _ = release_rx.recv();
                Ok(Snapshot::default())
            })
        })
        .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let healthy = SourceWorker::start(
            Source::Memory,
            1000,
            Arc::new(move || {
                let _ = healthy_tx.send(());
            }),
            || Box::new(|_| Ok(sample("memory:system", "load", 25.))),
        )
        .unwrap();
        healthy_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(healthy.latest.lock().unwrap().is_some());
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let dropper = std::thread::spawn(move || {
            drop(blocked);
            drop(healthy);
            done_tx.send(()).unwrap();
        });
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        let _ = release_tx.send(());
        dropper.join().unwrap();
        assert!(result.is_ok(), "shutdown waited for blocked native call");
    }
}
