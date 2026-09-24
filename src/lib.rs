pub mod alerts;
pub mod analytics;
pub mod collection;
pub mod config;
pub mod history;
pub mod model;
pub mod platform;
#[cfg(windows)]
pub mod sensors;
#[cfg(not(windows))]
#[path = "sensors/portable.rs"]
pub mod sensors;
