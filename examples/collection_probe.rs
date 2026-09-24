use mh_sidebar::{collection::Collector, model::Source};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
fn main() {
    let (sender, receiver) = mpsc::channel();
    let mut collector = Collector::start(500, move || {
        let _ = sender.send(());
    })
    .expect("start sources");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let _ = receiver.recv_timeout(Duration::from_millis(500));
        let snapshot = collector.snapshot(Instant::now());
        if snapshot.sources.len() == Source::ALL.len()
            && snapshot.sources.iter().all(|s| s.age.is_some() && !s.stale)
        {
            for source in &snapshot.sources {
                println!("{}: {:?}", source.source.title(), source.age);
            }
            println!(
                "{} devices, all six sources completed independently",
                snapshot.sections.len()
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "providers did not all complete: {:?}",
            snapshot.sources
        );
    }
}
