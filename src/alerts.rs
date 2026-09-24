use crate::{
    config::AlertRule,
    model::{Block, Snapshot},
};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub block: Block,
    pub device: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub threshold: f64,
}

#[derive(Default)]
struct AlertState {
    above_since: Option<Instant>,
    last_fired: Option<Instant>,
}

#[derive(Default)]
pub struct AlertEngine {
    states: HashMap<(usize, String), AlertState>,
}
impl AlertEngine {
    pub fn clear(&mut self) {
        self.states.clear();
    }
    pub fn observe(
        &mut self,
        snapshot: &Snapshot,
        rules: &[AlertRule],
        now: Instant,
    ) -> Vec<AlertEvent> {
        let mut active = HashSet::new();
        let mut events = Vec::new();
        for (index, rule) in rules.iter().enumerate().filter(|(_, r)| r.enabled) {
            for section in snapshot.sections.iter().filter(|s| {
                s.id == rule.block
                    && !s.disconnected
                    && (rule.device_id.is_empty() || s.device_id == rule.device_id)
            }) {
                let key = (index, section.device_id.clone());
                active.insert(key.clone());
                let state = self.states.entry(key).or_default();
                let reading = section
                    .rows
                    .iter()
                    .find(|r| r.key == rule.metric && !r.stale && r.sampled_at.is_some());
                let value = reading.and_then(|r| r.value).filter(|v| v.is_finite());
                if let Some(value) = value.filter(|v| *v >= rule.threshold) {
                    let since = *state.above_since.get_or_insert(now);
                    let ready = now.saturating_duration_since(since)
                        >= Duration::from_millis(rule.duration_ms);
                    let cooled = state.last_fired.is_none_or(|f| {
                        now.saturating_duration_since(f) >= Duration::from_millis(rule.cooldown_ms)
                    });
                    if ready && cooled {
                        state.last_fired = Some(now);
                        let reading = reading.unwrap();
                        events.push(AlertEvent {
                            block: rule.block,
                            device: section.device.clone(),
                            label: reading.label.clone(),
                            value,
                            unit: reading.unit.clone(),
                            threshold: rule.threshold,
                        });
                    }
                } else {
                    state.above_since = None;
                }
            }
        }
        for (key, state) in &mut self.states {
            if !active.contains(key) {
                state.above_since = None;
            }
        }
        self.states.retain(|(index, _), _| *index < rules.len());
        events
    }
}
