fn main() {
    let mut sampler = mh_sidebar::sensors::Sampler::new();
    for sample in 0..3 {
        println!("Sample {sample}");
        for section in sampler.sample().sections {
            println!("{:?}: {}", section.id, section.device);
            for row in section.rows {
                println!("  {} = {} {:?}", row.key, row.text, row.reason);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
