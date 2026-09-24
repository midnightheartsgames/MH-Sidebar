use mh_sidebar::{
    history::{Histories, metric_key},
    model::{Block, Reading, Section, Snapshot},
};
use std::time::{Duration, Instant};
fn snapshot(at: Instant, id: &str, name: &str, value: f64) -> Snapshot {
    let mut row = Reading::number("load", "Load", Some(value), "%");
    row.sampled_at = Some(at);
    Snapshot {
        sections: vec![Section {
            disconnected: false,
            id: Block::Gpu,
            device_id: id.into(),
            device: name.into(),
            rows: vec![row],
        }],
        ..Default::default()
    }
}
#[test]
fn rename_and_reconnect_keep_history_with_a_gap_without_duplicate_samples() {
    let origin = Instant::now();
    let window = Duration::from_secs(10);
    let mut histories = Histories::default();
    let first = snapshot(origin, "gpu:a", "Old", 10.);
    histories.observe(&first, origin, origin, window);
    histories.observe(&first, origin + Duration::from_secs(1), origin, window);
    let key = metric_key(Block::Gpu, "gpu:a", "load");
    assert_eq!(histories.get(&key).unwrap().len(), 1);
    histories.observe(
        &Snapshot::default(),
        origin + Duration::from_secs(2),
        origin,
        window,
    );
    histories.observe(
        &snapshot(origin + Duration::from_secs(3), "gpu:a", "New", 20.),
        origin + Duration::from_secs(3),
        origin,
        window,
    );
    assert_eq!(
        histories.get(&key).unwrap().points(3., window),
        vec![(0., Some(10.)), (2., None), (3., Some(20.))]
    );
    histories.observe(
        &snapshot(origin + Duration::from_secs(4), "gpu:b", "New", 90.),
        origin + Duration::from_secs(4),
        origin,
        window,
    );
    assert_eq!(
        histories
            .get(&metric_key(Block::Gpu, "gpu:b", "load"))
            .unwrap()
            .len(),
        1
    );
    histories.observe(
        &Snapshot::default(),
        origin + Duration::from_secs(15),
        origin,
        window,
    );
    assert!(histories.is_empty());
}
#[test]
fn stale_source_adds_one_gap_and_resumes_on_fresh_sample() {
    let origin = Instant::now();
    let mut histories = Histories::default();
    let window = Duration::from_secs(30);
    let mut old = snapshot(origin, "gpu:a", "GPU", 10.);
    histories.observe(&old, origin, origin, window);
    old.sections[0].rows[0].value = None;
    old.sections[0].rows[0].stale = true;
    histories.observe(&old, origin + Duration::from_secs(6), origin, window);
    histories.observe(&old, origin + Duration::from_secs(7), origin, window);
    let key = metric_key(Block::Gpu, "gpu:a", "load");
    assert_eq!(
        histories.get(&key).unwrap().points(7., window),
        vec![(0., Some(10.)), (6., None)]
    );
    histories.observe(
        &snapshot(origin + Duration::from_secs(8), "gpu:a", "GPU", 30.),
        origin + Duration::from_secs(8),
        origin,
        window,
    );
    assert_eq!(histories.get(&key).unwrap().len(), 3);
}

#[test]
fn missing_device_retains_unavailable_card_until_history_expires() {
    let origin = Instant::now();
    let window = Duration::from_secs(10);
    let mut histories = Histories::default();
    histories.observe(
        &snapshot(origin, "gpu:a", "GPU", 10.),
        origin,
        origin,
        window,
    );
    histories.observe(
        &Snapshot::default(),
        origin + Duration::from_secs(2),
        origin,
        window,
    );
    let cards = histories.missing_sections(&Snapshot::default());
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].device_id, "gpu:a");
    assert!(cards[0].disconnected);
    assert_eq!(cards[0].rows[0].value, None);
    assert!(
        histories
            .missing_sections(&snapshot(origin, "gpu:a", "Renamed", 20.))
            .is_empty()
    );
    histories.observe(
        &Snapshot::default(),
        origin + Duration::from_secs(11),
        origin,
        window,
    );
    assert!(histories.missing_sections(&Snapshot::default()).is_empty());
}
