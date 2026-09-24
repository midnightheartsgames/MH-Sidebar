use mh_sidebar::{
    alerts::AlertEngine,
    config::AlertRule,
    model::{Block, Reading, Section, Snapshot},
};
use std::time::{Duration, Instant};

fn snapshot(value: Option<f64>, stale: bool, at: Instant) -> Snapshot {
    let mut row = Reading::number("temperature", "Температура", value, "°C");
    row.sampled_at = Some(at);
    row.stale = stale;
    Snapshot {
        sections: vec![Section {
            disconnected: false,
            id: Block::Gpu,
            device_id: "gpu:1".into(),
            device: "Card".into(),
            rows: vec![row],
        }],
        ..Default::default()
    }
}

#[test]
fn alert_requires_duration_then_cooldown_and_rearms_after_recovery() {
    let start = Instant::now();
    let rule = AlertRule {
        block: Block::Gpu,
        metric: "temperature".into(),
        threshold: 80.,
        duration_ms: 3000,
        cooldown_ms: 10000,
        ..Default::default()
    };
    let mut engine = AlertEngine::default();
    let high = snapshot(Some(85.), false, start);
    assert!(
        engine
            .observe(&high, std::slice::from_ref(&rule), start)
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(2)
            )
            .is_empty()
    );
    assert_eq!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(3)
            )
            .len(),
        1
    );
    assert!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(12)
            )
            .is_empty()
    );
    assert_eq!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(13)
            )
            .len(),
        1
    );
    assert!(
        engine
            .observe(
                &snapshot(Some(30.), false, start),
                std::slice::from_ref(&rule),
                start + Duration::from_secs(14)
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(15)
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &high,
                std::slice::from_ref(&rule),
                start + Duration::from_secs(18)
            )
            .is_empty()
    );
    assert_eq!(
        engine
            .observe(&high, &[rule], start + Duration::from_secs(23))
            .len(),
        1
    );
}

#[test]
fn stale_or_missing_measurement_cannot_trigger_warning() {
    let start = Instant::now();
    let rule = AlertRule {
        block: Block::Gpu,
        metric: "temperature".into(),
        threshold: 80.,
        duration_ms: 2000,
        ..Default::default()
    };
    let mut engine = AlertEngine::default();
    assert!(
        engine
            .observe(
                &snapshot(Some(90.), false, start),
                std::slice::from_ref(&rule),
                start
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &snapshot(None, true, start),
                std::slice::from_ref(&rule),
                start + Duration::from_secs(3)
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &snapshot(Some(90.), false, start),
                std::slice::from_ref(&rule),
                start + Duration::from_secs(4)
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &Snapshot::default(),
                std::slice::from_ref(&rule),
                start + Duration::from_secs(8)
            )
            .is_empty()
    );
    assert!(
        engine
            .observe(
                &snapshot(Some(90.), false, start),
                &[rule],
                start + Duration::from_secs(9)
            )
            .is_empty()
    );
}

#[test]
fn rules_roundtrip_and_are_disabled_by_default() {
    let mut settings = mh_sidebar::config::Settings::default();
    assert!(!settings.notifications_enabled);
    settings.alert_rules.push(AlertRule {
        threshold: f64::NAN,
        duration_ms: 200_000,
        cooldown_ms: 0,
        ..Default::default()
    });
    settings.normalize();
    assert_eq!(settings.schema_version, 4);
    assert_eq!(settings.alert_rules[0].threshold, 80.);
    assert_eq!(settings.alert_rules[0].duration_ms, 120_000);
    assert_eq!(settings.alert_rules[0].cooldown_ms, 1_000);
    let decoded =
        mh_sidebar::config::Settings::import(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(decoded.alert_rules, settings.alert_rules);
}

#[test]
fn loading_v03_saves_backup_before_schema_four() {
    let dir = std::env::temp_dir().join(format!("mh-alert-migration-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let original = br#"{"schema_version":3,"font_size":18,"profiles":[]}"#;
    std::fs::write(&path, original).unwrap();
    let loaded = mh_sidebar::config::Settings::load(&path);
    assert!(loaded.writable);
    assert_eq!(loaded.settings.schema_version, 4);
    assert_eq!(
        std::fs::read(path.with_extension("v3.json")).unwrap(),
        original
    );
    loaded.settings.save(&path).unwrap();
    assert_eq!(
        std::fs::read(path.with_extension("v3.json")).unwrap(),
        original
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn repeated_v03_migration_preserves_each_distinct_original() {
    let dir = std::env::temp_dir().join(format!("mh-alert-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let first = br#"{"schema_version":3,"width":280}"#;
    let second = br#"{"schema_version":3,"width":390}"#;
    std::fs::write(&path, first).unwrap();
    assert!(mh_sidebar::config::Settings::load(&path).writable);
    std::fs::write(&path, second).unwrap();
    assert!(mh_sidebar::config::Settings::load(&path).writable);
    let backups = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.v3")
        })
        .map(|p| std::fs::read(p).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 2);
    assert!(backups.contains(&first.to_vec()));
    assert!(backups.contains(&second.to_vec()));
    std::fs::remove_dir_all(dir).unwrap();
}
