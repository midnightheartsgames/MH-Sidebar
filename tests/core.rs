use mh_sidebar::{
    config::{BlockConfig, Settings, Side},
    history::{History, rate},
    model::{Block, Reading},
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
fn first_run_offers_elevated_startup_by_default() {
    assert!(Settings::default().autostart);
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

#[test]
fn legacy_temperature_limits_migrate_without_changing_effective_values() {
    let mut s: Settings =
        serde_json::from_str(r#"{"warning_temperature":73,"critical_temperature":88}"#).unwrap();
    s.normalize();
    assert_eq!(s.temperature_limits(Block::Cpu), Some((73., 88.)));
    assert_eq!(s.temperature_limits(Block::Gpu), Some((73., 88.)));
    assert_eq!(s.temperature_limits(Block::Memory), None);
    s.warning_temperature = 65.;
    assert_eq!(s.temperature_limits(Block::Cpu), Some((73., 88.)));
    assert_eq!(s.temperature_limits(Block::Gpu), Some((65., 88.)));
}

#[test]
fn cpu_temperature_limits_are_normalized_and_roundtrip_independently() {
    let mut s: Settings = serde_json::from_str(
        r#"{"warning_temperature":70,"critical_temperature":90,"cpu_temperature":{"warning":200,"critical":-1}}"#,
    ).unwrap();
    s.normalize();
    assert_eq!(s.temperature_limits(Block::Cpu), Some((110., 111.)));
    assert_eq!(s.temperature_limits(Block::Gpu), Some((70., 90.)));
    let dir = temporary_dir();
    let path = dir.join("settings.json");
    s.save(&path).unwrap();
    assert_eq!(Settings::load(&path).settings, s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn metric_order_moves_load_and_keeps_threads_together() {
    let mut block = BlockConfig::new(Block::Cpu);
    let keys = ["load", "clock", "core_*", "temperature", "power"]
        .map(str::to_owned);
    block.move_row("core_*", 0, &keys);
    block.move_row("load", 3, &keys);
    let rows: Vec<_> = ["load", "clock", "core_0", "core_1", "temperature", "power"]
        .into_iter()
        .map(|key| Reading {
            sampled_at: None,
            stale: false,
            key: key.to_owned(),
            label: key.to_owned(),
            value: Some(1.),
            text: "1".to_owned(),
            unit: String::new(),
            reason: None,
        })
        .collect();
    let ordered: Vec<_> = block
        .ordered_rows(&rows)
        .iter()
        .map(|row| row.key.as_str())
        .collect();
    assert_eq!(
        ordered,
        ["core_0", "core_1", "clock", "temperature", "load", "power"]
    );
}

#[test]
fn metric_order_survives_save_load_and_missing_devices() {
    let dir = temporary_dir();
    let path = dir.join("settings.json");
    let mut settings = Settings::default();
    let block = settings
        .blocks
        .iter_mut()
        .find(|block| block.id == Block::Gpu)
        .unwrap();
    block.row_order = vec!["power".into(), "temperature".into(), "load".into()];
    block.move_row("load", 0, &["load".into(), "temperature".into()]);
    assert!(block.row_order.contains(&"power".to_owned()));
    let expected = block.row_order.clone();
    settings.save(&path).unwrap();
    let loaded = Settings::load(&path).settings;
    assert_eq!(loaded.block(Block::Gpu).unwrap().row_order, expected);
    std::fs::remove_dir_all(dir).unwrap();
}
