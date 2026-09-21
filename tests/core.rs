use mh_sidebar::{
    config::{BlockConfig, Settings, Side},
    history::{History, rate},
    model::Block,
};
use std::time::Duration;

fn temporary_dir() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mh-sidebar-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn repairs_invalid_ranges_and_duplicate_blocks_without_losing_user_order() {
    let mut s = Settings {
        width: 9000.,
        interval_ms: 0,
        opacity: f32::NAN,
        blocks: vec![
            BlockConfig::new(Block::Gpu),
            BlockConfig::new(Block::Gpu),
            BlockConfig::new(Block::Cpu),
        ],
        ..Default::default()
    };
    s.normalize();
    assert_eq!(s.width, 480.);
    assert_eq!(s.interval_ms, 500);
    assert_eq!(s.opacity, 0.94);
    assert_eq!(s.blocks.len(), 6);
    assert_eq!(s.blocks[0].id, Block::Gpu);
    assert_eq!(s.blocks[1].id, Block::Cpu);
}

#[test]
fn config_replaces_existing_file_and_roundtrips() {
    let dir = temporary_dir();
    let path = dir.join("settings.json");
    let mut s = Settings::default();
    s.save(&path).unwrap();
    s.side = Side::Left;
    s.width = 320.;
    s.save(&path).unwrap();
    let loaded = Settings::load(&path);
    assert_eq!(loaded.settings.side, Side::Left);
    assert_eq!(loaded.settings.width, 320.);
    assert!(loaded.writable);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn damaged_config_is_preserved_before_defaults_are_used() {
    let dir = temporary_dir();
    let path = dir.join("settings.json");
    std::fs::write(&path, b"{broken").unwrap();
    let loaded = Settings::load(&path);
    assert!(loaded.notice.is_some());
    assert!(loaded.writable);
    let copies: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|x| x.unwrap().path())
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read(&copies[0]).unwrap(), b"{broken");
    assert!(!path.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn future_schema_is_not_replaced() {
    let dir = temporary_dir();
    let path = dir.join("settings.json");
    let original = b"{\"schema_version\":900}";
    std::fs::write(&path, original).unwrap();
    let loaded = Settings::load(&path);
    assert!(!loaded.writable);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn history_expires_by_time_and_preserves_missing_sample_gaps() {
    let mut h = History::default();
    h.push(0., Some(10.));
    h.push(1., None);
    h.push(2., Some(30.));
    assert_eq!(h.points(2., Duration::from_secs(5)).len(), 3);
    assert_eq!(
        h.points(2., Duration::from_secs(1)),
        vec![(1., None), (2., Some(30.))]
    );
    for n in 3..10000 {
        h.push(n as f64, Some(1.));
    }
    assert!(h.len() <= 1202);
}

#[test]
fn counter_reset_and_zero_elapsed_do_not_invent_traffic() {
    assert_eq!(rate(100, 300, 2.), Some(100.));
    assert_eq!(rate(300, 20, 1.), None);
    assert_eq!(rate(100, 300, 0.), None);
    assert_eq!(rate(100, 300, f64::NAN), None);
}
