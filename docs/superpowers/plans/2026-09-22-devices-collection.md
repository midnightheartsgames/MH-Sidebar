# Devices and collection v0.2

User-approved second roadmap stage, executed in the current workspace on top of v0.1.1.

## Design

- Separate `Section.device_id` from the display name. Use volume GUIDs, network InterfaceGuid, GPU Windows device paths with PCI matching between NVML/WDDM. Document explicitly marked fallback identities if Windows cannot supply an ID.
- Schema 2 stores selected IDs and retains unresolved legacy names. Resolve only unique matches. Future schemas remain read-only; v0.1 rejects schema 2 rather than overwriting it.
- Six source workers: CPU OS, CPU PawnIO, memory, GPU, disks, network. Each owns its sampler and a bounded latest-result slot. UI merges source snapshots, marks only stale sources unavailable and does not join blocked native calls during shutdown. Restart is queued on each existing worker, so repeated commands cannot create unlimited blocked threads.
- Timestamp each reading at collection; history consumes each sample once, keeps missing-device graphs for the selected history window and inserts gaps on disappearance or staleness.
- Settings show source freshness and unavailable saved selections. No optional external service or new dependency crate.

## Tasks

- [x] Test and implement identity/selection migration, including ambiguous and absent devices.
- [x] Split sampler operations and implement Windows identity lookup.
- [x] Test and implement independent workers, source freshness and bounded shutdown.
- [x] Test and implement retained history, wire settings and sidebar.
- [x] Run formatting, full tests, Clippy, hardware probes and release build; document limits and package v0.2.0 separately.

## Acceptance

Renaming a device preserves its selection/history; two equal display names remain distinct; a stalled source cannot stop healthy sources; disconnected history survives until its window expires and reconnects with a gap. Tests must not require taking down a user's driver or stopping an existing application instance.

## Outcome — 2026-09-23

Implemented and reviewed. 50 standard and 5 explicit hardware tests passed, six-source runtime probe passed, formatting/Clippy/debug/release builds passed. Release v0.2.0 packaged separately and launched in the desktop session (exit 0, empty stderr). Full coordinate-based GUI smoke remains unverified at the current DPI; framebuffer visual inspection and partial interaction results are documented in docs/VALIDATION.md. No commit or interruption of user processes performed.
