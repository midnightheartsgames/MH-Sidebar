# Reliability v0.1.x Implementation Plan

**Goal:** Implement the reliability stage approved in the conversation: independent CPU/GPU temperature limits, CPU sensor recovery, visible diagnostics, failure tests and current documentation.

**Architecture:** Keep the existing sampler and UI. Add a small CPU recovery controller with injected open/read operations for deterministic failure tests. Preserve the existing GPU JSON fields; optional CPU limits inherit them when loading older configurations.

**Tech Stack:** Rust, serde, egui, existing PawnIO interface; no new dependencies.

## Tasks

- [x] Add regression tests for legacy temperature settings, independent CPU/GPU limits and invalid ranges; implement config normalization and UI/rendering integration.
- [x] Add deterministic recovery tests for repeated missing reads, partial readings, failed reopen, bounded backoff, recovery and unsupported hardware; integrate the controller into Sampler.
- [x] Display recovery diagnostics in CPU settings and missing-reading tooltips, without inventing values or classifying optional power readings as complete failures.
- [x] Run the full offline test suite, formatting and Clippy; build and smoke-test the application where the desktop permits.
- [x] Update README and validation documentation with actual results and remaining hardware limitations.

## Review focus

Old JSON with custom GPU thresholds must retain its effective values. CPU/GPU changes must be independent after normalization. A successful reopen without successful reads must not reset backoff. An unsupported CPU must not be retried automatically. CPU load and other sources must remain available during CPU temperature recovery.

## Scope

No stable device-ID migration, provider threading redesign, system notifications or privileged helper in this stage. Recovery covers calls that return; a blocked kernel call cannot be interrupted by this controller.

## Verification outcome

38 standard tests and 4 explicit hardware tests passed; Clippy, formatting and debug/release builds passed. GUI smoke was attempted but blocked by the missing test window while another instance was running; documented in docs/VALIDATION.md. Release packaged separately under dist/v0.1.1. No commit or user-process restart performed.
