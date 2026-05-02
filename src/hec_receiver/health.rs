use serde::Serialize;
use std::sync::RwLock;

#[derive(Debug)]
pub struct HealthState {
    phase: RwLock<Phase>,
}

impl HealthState {
    pub fn serving() -> Self {
        Self {
            phase: RwLock::new(Phase::Serving),
        }
    }

    pub fn current(&self) -> Phase {
        self.phase
            .read()
            .map(|phase| *phase)
            .unwrap_or(Phase::Stopping)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Phase {
    Starting,
    Serving,
    Degraded,
    Stopping,
}

impl Phase {
    pub fn admits_work(self) -> bool {
        matches!(self, Self::Serving | Self::Degraded)
    }
}
