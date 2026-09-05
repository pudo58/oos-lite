use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WatcherConfig {
    pub watch_dir: PathBuf,
    pub debounce_duration: Duration,
    pub cooldown_window: Duration,
    pub throttle_ms: u64,
    pub reconcile_interval: Duration,
}

impl WatcherConfig {
    pub fn new<P: Into<PathBuf>>(watch_dir: P) -> Self {
        Self {
            watch_dir: watch_dir.into(),
            debounce_duration: Duration::from_secs(3),
            cooldown_window: Duration::from_secs(60),
            throttle_ms: 10,
            reconcile_interval: Duration::from_secs(3600), // 1 hour
        }
    }

    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce_duration = debounce;
        self
    }

    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown_window = cooldown;
        self
    }

    pub fn with_throttle_ms(mut self, throttle_ms: u64) -> Self {
        self.throttle_ms = throttle_ms;
        self
    }

    pub fn with_reconcile_interval(mut self, interval: Duration) -> Self {
        self.reconcile_interval = interval;
        self
    }
}
