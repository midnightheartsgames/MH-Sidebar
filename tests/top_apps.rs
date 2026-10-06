use mh_sidebar::{
    config::Settings,
    model::{Block, Snapshot},
    processes::{self, Processes},
};
use std::{collections::HashMap, ffi::OsStr};

#[test]
fn processes_with_one_name_are_one_app_and_the_list_is_cut_to_the_limit() {
    let top = processes::rank(
        Block::Cpu,
        "cpu:system",
        [
            ("chrome".to_owned(), 4.),
            ("game".to_owned(), 30.),
            ("chrome".to_owned(), 7.),
            ("idle".to_owned(), 0.),
            ("editor".to_owned(), 2.),
        ],
        2,
        processes::percent,
    );
    let names: Vec<_> = top.apps.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["game", "chrome"]);
    assert_eq!(top.apps[1].processes, 2);
    assert_eq!(top.apps[1].text, "11.0 %");
    assert_eq!(top.reason, None);
}

#[test]
fn memory_is_written_in_mib_below_one_gib() {
    assert_eq!(processes::bytes(512. * 1_048_576.), "512 MiB");
    assert_eq!(processes::bytes(1.5 * 1_073_741_824.), "1.50 GiB");
}

#[test]
fn windows_executable_suffix_is_dropped_from_names() {
    assert_eq!(processes::app_name(OsStr::new("chrome.EXE")), "chrome");
    assert_eq!(processes::app_name(OsStr::new("firefox")), "firefox");
    assert_eq!(processes::app_name(OsStr::new(".exe")), "");
}

#[test]
fn gpu_load_per_process_takes_the_busiest_engine_type_of_one_adapter() {
    let tag = "luid_0x00000000_0x0000d1f2";
    let instances = vec![
        (format!("pid_100_{tag}_phys_0_eng_0_engtype_3D"), 20.),
        (format!("pid_100_{tag}_phys_0_eng_1_engtype_3D"), 15.),
        (
            format!("pid_100_{tag}_phys_0_eng_4_engtype_VideoDecode"),
            30.,
        ),
        (format!("pid_200_{tag}_phys_0_eng_0_engtype_3D"), 5.),
        (
            "pid_300_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D".into(),
            90.,
        ),
    ];
    let usage = processes::gpu_engine_usage(&instances, tag);
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[&100], 35.);
    assert_eq!(usage[&200], 5.);
    let names = HashMap::from([(100, "game".to_owned())]);
    let top = processes::gpu("gpu:test", &usage, &names, 7);
    let names: Vec<_> = top.apps.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["game", "PID 200"]);
}

#[test]
fn only_cpu_gpu_and_memory_get_a_top_and_counts_snap_to_offered_sizes() {
    let mut settings: Settings = serde_json::from_str(
        r#"{"schema_version":4,"blocks":[{"id":"Cpu","top_apps":4},{"id":"Disks","top_apps":5},{"id":"Memory","top_apps":99}]}"#,
    )
    .unwrap();
    settings.normalize();
    assert_eq!(settings.block(Block::Cpu).unwrap().top_apps, 3);
    assert_eq!(settings.block(Block::Memory).unwrap().top_apps, 7);
    assert_eq!(settings.block(Block::Disks).unwrap().top_apps, 0);
    assert_eq!(settings.block(Block::Gpu).unwrap().top_apps, 3);
    let mut memory = settings.block(Block::Memory).unwrap().clone();
    memory.enabled = false;
    assert_eq!(memory.top_apps(), 0);
}

#[test]
fn a_newer_top_replaces_the_previous_one_for_the_same_device() {
    let mut snapshot = Snapshot::default();
    for value in [10., 20.] {
        snapshot.merge(Snapshot {
            top_apps: vec![processes::rank(
                Block::Memory,
                "memory:system",
                [("app".to_owned(), value)],
                3,
                processes::bytes,
            )],
            ..Default::default()
        });
    }
    assert_eq!(snapshot.top_apps.len(), 1);
    let top = snapshot.top_apps(Block::Memory, "memory:system").unwrap();
    assert_eq!(top.apps[0].value, 20.);
}

#[test]
fn this_machine_lists_memory_hungry_apps() {
    let mut list = Processes::default();
    let top = list.memory("memory:system", 5);
    assert!(!top.apps.is_empty() && top.apps.len() <= 5, "{top:?}");
    assert!(top.apps.windows(2).all(|w| w[0].value >= w[1].value));
    // sysinfo не считает загрузку процесса, у которого к первому замеру ещё нет тиков CPU.
    let burn = || {
        let busy = std::time::Instant::now();
        while busy.elapsed() < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL * 2 {
            std::hint::black_box(busy.elapsed());
        }
    };
    burn();
    let first = list.cpu("cpu:system", 3);
    assert!(first.reason.is_some());
    burn();
    let second = list.cpu("cpu:system", 3);
    assert_eq!(second.reason, None);
    assert!(!second.apps.is_empty(), "{second:?}");
    assert!(second.apps.iter().all(|a| a.value <= 100.5));
}
