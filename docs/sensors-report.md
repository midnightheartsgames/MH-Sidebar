# Sensors implementation report

Implemented `Sampler::new()` / `sample()` in `src/sensors.rs`, isolated WDDM and PDH helpers in `src/sensors/`, and `examples/probe.rs`. No admin, drivers, ETW, PawnIO, or external workspace dependencies. Small reference helpers retain MH Monitoring MIT attribution.

## Validation

- Rate tests were written against a stub first: elapsed-time and reset tests failed with None instead of expected 0.5 and 1 MiB/s. Implementation then passed.
- `cargo test --lib sensors:: -- --nocapture`: 10 passed. Includes elapsed-time conversion, initial unknown, reset rebaseline, zero elapsed, real PDH queries, adapter enumeration, and forced WDDM sampling.
- `cargo run --example probe`: successful three-sample console probe. Ryzen 9 5900X, 24 logical CPUs; sample-zero load unknown, later load about 3.5%. OS-reported frequency 3701/3342 MHz. RAM about 22.7/63.9 GiB. RTX 5070 Ti returned 49 C, 1110–1215 MHz core, 405–810 MHz memory, 1.8/15.9 GiB VRAM, 33–35 W, zero fan percent. Nine disk volumes had unknown initial transfers then valid PDH rates. Ethernet upload around 0.3 MiB/s; all interface names retained.
- Forced NVML-disabled WDDM test returned approximately 28.7% GPU load, 50 C, 1162 MHz core, 405 MHz memory, 1.8/15.6 GiB dedicated VRAM and 0 RPM. WDDM power is explicitly unavailable because its power fraction is not watts.

## Behavior and limitations

- All numeric units correspond to actual stored values. Rates use actual elapsed duration; counter reset produces None and rebaselines. CPU needs a valid interval. GPU NVML values are instantaneous vendor readings, so they can be available immediately.
- CPU temperature and watts intentionally unavailable, with Russian explanations. Frequency is labeled OS-reported, not advertised as a guaranteed dynamic clock.
- Disk inventory/capacity uses sysinfo. Transfers use separate PDH read/write queries because sysinfo hides IOCTL failure behind zero counters. Missing/invalid PDH readings remain unavailable. Volume accessibility is checked before showing capacity.
- Hardware lists and failed NVML initialization retry every 30 seconds. Networks refresh each sample and disconnected interface rate state is removed. PDH wildcard instances refresh on collection. Failed counter initialization retries every 30 seconds.
- Network devices are separate clearly named sections (Ethernet, Ethernet 2, vEthernet etc.), with no aggregate total that would double count physical/virtual traffic. User device filtering is expected in UI. Loopback omitted.
- WDDM and NVML deduplicate exact adapter names one-to-one; drivers reporting different names may show two sections. NVIDIA and WDDM can report slightly different usable VRAM totals. AMD/Intel code path is implemented but actual AMD/Intel hardware was not available for validation.
- WDDM reports the busiest engine after summing process instances, clamped to 100%. GPU individual fields fail independently. NVML fan is percent; WDDM fan is RPM.
- GPU device identity currently includes vendor index or WDDM LUID; LUID changes after reboot, and NVML/WDDM failover changes device identity. Numeric reading keys remain stable.
