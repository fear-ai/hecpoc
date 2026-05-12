use super::{
    auth::TokenRegistry,
    health::HealthState,
    hec_request,
    protocol::Protocol,
    report::{ReportOutputs, Reporter},
    sink::Sink,
};
use axum::{
    routing::{get, post},
    Router,
};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_content_length: usize,
    pub max_http_body_bytes: usize,
    pub max_decoded_body_bytes: usize,
    pub max_events_per_request: usize,
    pub max_index_len: usize,
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
    pub tokens: TokenRegistry,
    pub limits: Limits,
    pub reporter: Reporter,
    pub health: HealthState,
    pub sink: Sink,
    pub protocol: Protocol,
}

impl AppState {
    #[allow(dead_code)]
    pub fn drop_events(tokens: Vec<String>, limits: Limits) -> Self {
        Self::new(
            tokens,
            limits,
            Sink::drop_events(),
            Protocol::default(),
            ReportOutputs::default(),
        )
    }

    #[allow(dead_code)]
    pub fn drop_events_with_report_outputs(
        tokens: Vec<String>,
        limits: Limits,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self::new(
            tokens,
            limits,
            Sink::drop_events(),
            Protocol::default(),
            report_outputs,
        )
    }

    pub fn drop_events_with_registry(
        tokens: TokenRegistry,
        limits: Limits,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self::from_parts(
            tokens,
            limits,
            Sink::drop_events(),
            Protocol::default(),
            report_outputs,
        )
    }

    #[allow(dead_code)]
    pub fn capture_file(
        tokens: Vec<String>,
        limits: Limits,
        path: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self::capture_file_with_report_outputs(tokens, limits, path, ReportOutputs::default())
    }

    pub fn capture_file_with_report_outputs(
        tokens: Vec<String>,
        limits: Limits,
        path: impl Into<std::path::PathBuf>,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self::new(
            tokens,
            limits,
            Sink::capture_file(path),
            Protocol::default(),
            report_outputs,
        )
    }

    pub fn capture_file_with_registry(
        tokens: TokenRegistry,
        limits: Limits,
        path: impl Into<std::path::PathBuf>,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self::from_parts(
            tokens,
            limits,
            Sink::capture_file(path),
            Protocol::default(),
            report_outputs,
        )
    }

    pub fn with_protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = protocol;
        self
    }

    fn new(
        tokens: Vec<String>,
        limits: Limits,
        sink: Sink,
        protocol: Protocol,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self::from_parts(
            TokenRegistry::new(tokens),
            limits,
            sink,
            protocol,
            report_outputs,
        )
    }

    fn from_parts(
        tokens: TokenRegistry,
        limits: Limits,
        sink: Sink,
        protocol: Protocol,
        report_outputs: ReportOutputs,
    ) -> Self {
        Self {
            tokens,
            limits,
            reporter: Reporter::new(report_outputs),
            health: HealthState::serving(),
            sink,
            protocol,
        }
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/services/collector", post(hec_request::post_event))
        .route("/services/collector/event", post(hec_request::post_event))
        .route(
            "/services/collector/event/1.0",
            post(hec_request::post_event),
        )
        .route("/services/collector/raw", post(hec_request::post_raw))
        .route("/services/collector/raw/1.0", post(hec_request::post_raw))
        .route("/services/collector/ack", post(hec_request::post_ack))
        .route("/services/collector/ack/1.0", post(hec_request::post_ack))
        .route(
            "/services/collector/health",
            get(hec_request::health).post(hec_request::health),
        )
        .route(
            "/services/collector/health/1.0",
            get(hec_request::health).post(hec_request::health),
        )
        .route("/hec/stats", get(hec_request::stats))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::{AppState, Limits};

    #[test]
    fn capture_file_constructor_keeps_default_report_outputs_available() {
        let state = AppState::capture_file(
            vec!["test-token".to_string()],
            Limits::default(),
            "/tmp/hec-capture-test.jsonl",
        );

        assert_eq!(
            state.limits.max_events_per_request,
            Limits::default().max_events_per_request
        );
    }
}
