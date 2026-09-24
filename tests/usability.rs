use mh_sidebar::{
    config::{Density, Preset, Settings},
    model::Block,
};

#[test]
fn profile_roundtrip_preserves_machine_controls() {
    let mut source = Settings::default();
    source.apply_preset(Preset::Gaming);
    source.save_profile("Game").unwrap();
    let bytes = serde_json::to_vec(&source).unwrap();
    let mut target = Settings::import(&bytes).unwrap();
    target.monitor_id = "monitor-other".into();
    target.autostart = true;
    target.visible = false;
    target.hotkeys.visibility = "Ctrl+Alt+F8".into();
    target.apply_preset(Preset::Minimal);
    target.apply_profile(0).unwrap();
    assert_eq!(target.blocks, source.blocks);
    assert_eq!(target.density, source.density);
    assert_eq!(target.monitor_id, "monitor-other");
    assert!(target.autostart);
    assert!(!target.visible);
    assert_eq!(target.hotkeys.visibility, "Ctrl+Alt+F8");
}

#[test]
fn import_validates_and_normalizes_without_touching_files() {
    assert!(Settings::import(br#"{"schema_version":999}"#).is_err());
    assert!(Settings::import(b"broken").is_err());
    assert!(Settings::import(&vec![b' '; 1_048_577]).is_err());
    let settings = Settings::import(br#"{"schema_version":2,"width":9999}"#).unwrap();
    assert_eq!(settings.schema_version, 4);
    assert_eq!(settings.width, 480.);
    assert_eq!(settings.density, Density::Compact);
    assert!(settings.profiles.is_empty());
}

#[test]
fn moving_a_block_preserves_other_order_and_options() {
    let mut settings = Settings::default();
    settings.blocks[0].device_ids.push("selected".into());
    let mut expected = settings.blocks.clone();
    let moved = expected.remove(0);
    expected.insert(3, moved);
    settings.move_block(settings.blocks[0].id, 3);
    assert_eq!(settings.blocks, expected);
    settings.move_block(Block::Cpu, usize::MAX);
    assert_eq!(settings.blocks, expected);
}

#[test]
fn named_profiles_update_and_reject_blank_names() {
    let mut settings = Settings::default();
    assert!(settings.save_profile("  ").is_err());
    settings.save_profile(" Work ").unwrap();
    settings.width = 400.;
    settings.save_profile("Work").unwrap();
    assert_eq!(settings.profiles.len(), 1);
    settings.width = 280.;
    settings.apply_profile(0).unwrap();
    assert_eq!(settings.width, 400.);
    assert!(settings.apply_profile(8).is_err());
}

#[test]
fn schema_two_migration_keeps_an_exact_backup() {
    let dir = std::env::temp_dir().join(format!("mh-v3-migration-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let original = br#"{"schema_version":2,"width":330}"#;
    std::fs::write(&path, original).unwrap();
    let loaded = Settings::load(&path);
    assert!(loaded.writable);
    assert_eq!(loaded.settings.schema_version, 4);
    assert_eq!(
        std::fs::read(path.with_extension("v2.json")).unwrap(),
        original
    );
    loaded.settings.save(&path).unwrap();
    assert_eq!(Settings::load(&path).settings, loaded.settings);
    assert_eq!(
        std::fs::read(path.with_extension("v2.json")).unwrap(),
        original
    );
    std::fs::remove_dir_all(dir).unwrap();
}
