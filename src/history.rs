use std::{collections::VecDeque, time::Duration};

#[derive(Default)]
pub struct History {
    samples: VecDeque<(f64, Option<f64>)>,
}
impl History {
    pub fn push(&mut self, time: f64, value: Option<f64>) {
        if !time.is_finite() {
            return;
        }
        self.samples
            .push_back((time, value.filter(|v| v.is_finite())));
        while self.samples.front().is_some_and(|(t, _)| time - t > 600.)
            || self.samples.len() > 1202
        {
            self.samples.pop_front();
        }
    }
    pub fn points(&self, now: f64, window: Duration) -> Vec<(f64, Option<f64>)> {
        self.samples
            .iter()
            .filter(|(t, _)| *t >= now - window.as_secs_f64())
            .copied()
            .collect()
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

pub fn rate(previous: u64, current: u64, seconds: f64) -> Option<f64> {
    if !seconds.is_finite() || seconds <= 0. {
        return None;
    }
    current
        .checked_sub(previous)
        .map(|delta| delta as f64 / seconds)
}
