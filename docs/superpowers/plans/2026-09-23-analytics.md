# Analytics v0.4

The approved roadmap asks for a large graph on click, min/average/max, history export,
and configurable warnings. Its success criterion is diagnosing a load spike and saving
data for comparison.

The expanded view opens from a metric row or block headline, shows the selected metric
for the configured history window, its device/source freshness, valid-sample statistics,
and CSV export. A second export action saves all retained metrics. CSV timestamps are
Unix milliseconds, values use a decimal dot, and absent/stale samples remain blank.
The current bounded in-memory history is the source of truth; exports do not silently
create long-term logging.

Alert rules select block, metric and optional stable device ID, threshold, required
duration and repeat cooldown. Rules skip stale/unavailable readings and reset after a
below-threshold reading. Delivery uses the existing Windows tray icon balloon; failure
to show one does not block sampling. New alerts are off by default, while existing
threshold coloring continues. A schema-4 migration backs up the prior JSON.

- [x] Add statistics and CSV export with tests for gaps, identifiers and timestamps.
- [x] Add alert settings and state machine with tests for duration, cooldown and recovery.
- [x] Connect expanded view, native CSV Save dialog and tray warnings.
- [x] Verify tests, lint, release, visual UI and package v0.4 separately.
