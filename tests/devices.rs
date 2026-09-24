use mh_sidebar::{
    config::Settings,
    model::{Block, Section, Snapshot},
};

fn device(id: &str, name: &str) -> Section {
    Section {
        disconnected: false,
        id: Block::Network,
        device_id: id.into(),
        device: name.into(),
        rows: vec![],
    }
}

#[test]
fn old_names_migrate_to_ids_and_survive_rename() {
    let mut settings: Settings = serde_json::from_str(
        r#"{"schema_version":1,"blocks":[{"id":"Network","devices":["Ethernet"]}]}"#,
    )
    .unwrap();
    settings.normalize();
    assert_eq!(settings.schema_version, 4);
    let snapshot = Snapshot {
        sections: vec![device("net:abc", "Ethernet")],
        ..Default::default()
    };
    assert!(settings.resolve_devices(&snapshot));
    let block = settings.block(Block::Network).unwrap();
    assert_eq!(block.device_ids, ["net:abc"]);
    assert!(block.devices.is_empty());
    assert!(block.selects(&device("net:abc", "Office")));
    assert!(!block.selects(&device("net:other", "Ethernet")));
}

#[test]
fn ambiguous_and_absent_legacy_names_do_not_select_everything() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"blocks":[{"id":"Network","devices":["VPN","Missing"]}]}"#)
            .unwrap();
    settings.normalize();
    let mut snapshot = Snapshot {
        sections: vec![device("net:a", "VPN"), device("net:b", "VPN")],
        ..Default::default()
    };
    assert!(!settings.resolve_devices(&snapshot));
    let block = settings.block(Block::Network).unwrap();
    assert!(!block.all_devices());
    assert!(!block.selects(&snapshot.sections[0]));
    snapshot.sections.remove(1);
    assert!(settings.resolve_devices(&snapshot));
    let block = settings.block(Block::Network).unwrap();
    assert_eq!(block.device_ids, ["net:a"]);
    assert_eq!(block.devices, ["Missing"]);
    assert!(!block.all_devices());
    let restored: Settings =
        serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!(restored, settings);
}

#[test]
fn default_selection_includes_new_devices_but_explicit_selection_does_not() {
    let mut settings = Settings::default();
    settings.normalize();
    assert!(
        settings
            .block(Block::Network)
            .unwrap()
            .selects(&device("net:new", "New"))
    );
    let block = settings
        .blocks
        .iter_mut()
        .find(|b| b.id == Block::Network)
        .unwrap();
    block.device_ids.push("net:old".into());
    assert!(!block.selects(&device("net:new", "New")));
}

#[test]
fn loading_schema_one_preserves_original_before_saving_schema_two() {
    let dir = std::env::temp_dir().join(format!(
        "mh-migration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let original =
        br#"{"schema_version":1,"width":320,"blocks":[{"id":"Network","devices":["Ethernet"]}]}"#;
    std::fs::write(&path, original).unwrap();
    let loaded = Settings::load(&path);
    assert!(loaded.writable);
    assert_eq!(loaded.settings.schema_version, 4);
    assert_eq!(
        std::fs::read(path.with_extension("v1.json")).unwrap(),
        original
    );
    loaded.settings.save(&path).unwrap();
    assert_eq!(Settings::load(&path).settings, loaded.settings);
    assert_eq!(
        std::fs::read(path.with_extension("v1.json")).unwrap(),
        original
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn boot_local_gpu_index_does_not_resolve_two_identical_models() {
    let mut settings: Settings =
        serde_json::from_str(r#"{"blocks":[{"id":"Gpu","devices":["RTX · NVIDIA 0"]}]}"#).unwrap();
    settings.normalize();
    let mut a = device("gpu:a", "RTX · NVIDIA 0");
    a.id = Block::Gpu;
    let mut b = device("gpu:b", "RTX · NVIDIA 1");
    b.id = Block::Gpu;
    let mut snapshot = Snapshot {
        sections: vec![a, b],
        ..Default::default()
    };
    assert!(!settings.resolve_devices(&snapshot));
    assert!(settings.block(Block::Gpu).unwrap().device_ids.is_empty());
    snapshot.sections.remove(0);
    assert!(settings.resolve_devices(&snapshot));
    assert_eq!(settings.block(Block::Gpu).unwrap().device_ids, ["gpu:b"]);
}
