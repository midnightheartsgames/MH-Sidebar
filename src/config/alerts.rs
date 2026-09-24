use crate::model::Block;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertRule {
    pub enabled: bool,
    pub block: Block,
    pub metric: String,
    pub device_id: String,
    pub threshold: f64,
    pub duration_ms: u64,
    pub cooldown_ms: u64,
}
impl Default for AlertRule {
    fn default() -> Self {
        Self {
            enabled: true,
            block: Block::Gpu,
            metric: "temperature".into(),
            device_id: String::new(),
            threshold: 80.,
            duration_ms: 5000,
            cooldown_ms: 60000,
        }
    }
}
impl AlertRule {
    pub fn normalize(&mut self) {
        if !self.threshold.is_finite() {
            self.threshold = 80.;
        }
        self.threshold = self.threshold.clamp(0., 1_000_000.);
        self.duration_ms = self.duration_ms.clamp(0, 120_000);
        self.cooldown_ms = self.cooldown_ms.clamp(1000, 3_600_000);
        self.metric.truncate(100);
        self.device_id.truncate(512);
    }
}
