use super::{
    auth::TokenStore, handler, health::HealthState, protocol::Protocol, sink::Sink, stats::Stats,
};
use axum::{
    routing::{get, post},
    Router,
};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_content_length: usize,
    pub max_wire_body_bytes: usize,
    pub max_decoded_body_bytes: usize,
    pub max_events_per_request: usize,
    pub body_idle_timeout: Duration,
    pub body_total_timeout: Duration,
    pub gzip_buffer_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self::default_values()
    }
}

#[derive(Debug)]
pub struct AppState {
    pub tokens: TokenStore,
    pub limits: Limits,
    pub stats: Stats,
    pub health: HealthState,
    pub sink: Sink,
    pub protocol: Protocol,
}

impl AppState {
    pub fn drop_only(tokens: Vec<String>, limits: Limits) -> Self {
        Self::new(tokens, limits, Sink::drop_only(), Protocol::default())
    }

    pub fn capture_file(
        tokens: Vec<String>,
        limits: Limits,
        path: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self::new(
            tokens,
            limits,
            Sink::capture_file(path),
            Protocol::default(),
        )
    }

    pub fn with_protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = protocol;
        self
    }

    fn new(tokens: Vec<String>, limits: Limits, sink: Sink, protocol: Protocol) -> Self {
        Self {
            tokens: TokenStore::new(tokens),
            limits,
            stats: Stats::default(),
            health: HealthState::serving(),
            sink,
            protocol,
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/services/collector", post(handler::post_event))
        .route("/services/collector/event", post(handler::post_event))
        .route("/services/collector/event/1.0", post(handler::post_event))
        .route("/services/collector/raw", post(handler::post_raw))
        .route("/services/collector/raw/1.0", post(handler::post_raw))
        .route(
            "/services/collector/health",
            get(handler::health).post(handler::health),
        )
        .route(
            "/services/collector/health/1.0",
            get(handler::health).post(handler::health),
        )
        .route("/hec/stats", get(handler::stats))
        .with_state(state)
}
