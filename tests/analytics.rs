use mh_sidebar::{
    analytics::{MetricDescriptor, Stats, csv_all, csv_series},
    history::{Histories, History},
    model::{Block, Reading, Section, Snapshot},
};
use std::time::{Duration, SystemTime};

#[test]
fn statistics_ignore_gaps_and_limit_to_visible_window() {
    let mut h = History::default();
    for (t, v) in [
        (0., Some(100.)),
        (10., Some(20.)),
        (20., None),
        (30., Some(80.)),
        (40., Some(50.)),
    ] {
        h.push(t, v);
    }
    let stats = Stats::from_history(&h, 40., Duration::from_secs(30)).unwrap();
    assert_eq!(
        (stats.min, stats.mean, stats.max, stats.count),
        (20., 50., 80., 3)
    );
    assert_eq!(stats.max_at, 30.);
    assert!(Stats::from_history(&h, 100., Duration::from_secs(30)).is_none());
}

#[test]
fn csv_keeps_gaps_and_escapes_names() {
    let mut h = History::default();
    h.push(1., Some(3.5));
    h.push(2., None);
    let csv = csv_series(
        &h,
        2.,
        Duration::from_secs(10),
        SystemTime::UNIX_EPOCH,
        MetricDescriptor {
            block: "Gpu",
            device_id: "gpu:1",
            device: "Card, \"A\"",
            metric: "power",
            label: "Power",
            unit: "W",
        },
    );
    assert!(csv.contains("1000,1.000,Gpu,gpu:1,\"Card, \"\"A\"\"\",power,Power,W,3.5"));
    assert!(csv.contains("2000,2.000,Gpu,gpu:1,\"Card, \"\"A\"\"\",power,Power,W,\n"));
}

#[test]
fn full_export_keeps_distinct_devices_and_one_sample_per_acquisition() {
    let origin = std::time::Instant::now();
    let mut histories = Histories::default();
    let make = |id: &str, value| {
        let mut row = Reading::number("load", "Загрузка", Some(value), "%");
        row.sampled_at = Some(origin + Duration::from_secs(1));
        Section {
            disconnected: false,
            id: Block::Gpu,
            device_id: id.into(),
            device: "RTX".into(),
            rows: vec![row],
        }
    };
    let snapshot = Snapshot {
        sections: vec![make("gpu:a", 20.), make("gpu:b", 80.)],
        ..Default::default()
    };
    histories.observe(
        &snapshot,
        origin + Duration::from_secs(1),
        origin,
        Duration::from_secs(60),
    );
    histories.observe(
        &snapshot,
        origin + Duration::from_secs(2),
        origin,
        Duration::from_secs(60),
    );
    let csv = csv_all(
        &histories,
        2.,
        Duration::from_secs(60),
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(csv.lines().count(), 3);
    assert!(csv.contains("gpu:a,RTX,load,Загрузка,%,20"));
    assert!(csv.contains("gpu:b,RTX,load,Загрузка,%,80"));
}

#[test]
fn full_export_keeps_metric_after_it_disappears_from_live_section() {
    let origin = std::time::Instant::now();
    let section = |at: u64, temperature: bool| {
        let mut load = Reading::number("load", "Загрузка", Some(20.), "%");
        load.sampled_at = Some(origin + Duration::from_secs(at));
        let mut rows = vec![load];
        if temperature {
            let mut temp = Reading::number("temperature", "Температура", Some(77.), "°C");
            temp.sampled_at = Some(origin + Duration::from_secs(at));
            rows.push(temp);
        }
        Snapshot {
            sections: vec![Section {
                disconnected: false,
                id: Block::Gpu,
                device_id: "gpu:a".into(),
                device: "RTX".into(),
                rows,
            }],
            ..Default::default()
        }
    };
    let mut histories = Histories::default();
    histories.observe(
        &section(1, true),
        origin + Duration::from_secs(1),
        origin,
        Duration::from_secs(60),
    );
    histories.observe(
        &section(2, false),
        origin + Duration::from_secs(2),
        origin,
        Duration::from_secs(60),
    );
    let csv = csv_all(
        &histories,
        2.,
        Duration::from_secs(60),
        SystemTime::UNIX_EPOCH,
    );
    assert!(csv.contains("gpu:a,RTX,temperature,Температура,°C,77"));
    assert!(csv.contains("gpu:a,RTX,temperature,Температура,°C,\n"));
}
