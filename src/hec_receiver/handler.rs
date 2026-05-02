use axum::{
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use super::{
    app::AppState,
    body::{
        decode_limited, parse_content_encoding, read_limited_body, reject_advertised_oversize,
        Encoding,
    },
    event::Endpoint,
    outcome::{HecError, HecOutcome},
    parse_event::parse_event_body,
    parse_raw::parse_raw_body,
};

pub async fn post_event(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    handle_hec(state, request, Endpoint::Event).await
}

pub async fn post_raw(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    handle_hec(state, request, Endpoint::Raw).await
}

pub async fn health(State(state): State<Arc<AppState>>) -> Response {
    let phase = state.health.current();
    let status = if phase.admits_work() {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({
            "text": if phase.admits_work() { "HEC is healthy" } else { "HEC is unhealthy" },
            "code": state.protocol.health
        })),
    )
        .into_response()
}

pub async fn stats(State(state): State<Arc<AppState>>) -> Response {
    Json(state.stats.snapshot()).into_response()
}

async fn handle_hec(state: Arc<AppState>, request: Request<Body>, endpoint: Endpoint) -> Response {
    let started = Instant::now();
    state.stats.requests_total.fetch_add(1, Ordering::Relaxed);

    let result = handle_hec_inner(&state, request, endpoint).await;
    let response = match result {
        Ok(outcome) => {
            state.stats.requests_ok.fetch_add(1, Ordering::Relaxed);
            outcome.into_response()
        }
        Err(outcome) => {
            state.stats.requests_failed.fetch_add(1, Ordering::Relaxed);
            record_error_outcome(&state, &outcome);
            outcome.into_response()
        }
    };

    state.stats.record_latency(started.elapsed());
    response
}

async fn handle_hec_inner(
    state: &Arc<AppState>,
    request: Request<Body>,
    endpoint: Endpoint,
) -> Result<HecOutcome, HecOutcome> {
    if !state.health.current().admits_work() {
        return Err(HecError::ServerBusy.outcome(&state.protocol));
    }

    let (parts, body) = request.into_parts();
    state
        .tokens
        .authenticate(&parts.headers)
        .map_err(|error| error.outcome(&state.protocol))?;

    let encoding =
        parse_content_encoding(&parts.headers).map_err(|error| error.outcome(&state.protocol))?;
    if encoding == Encoding::Gzip {
        state.stats.gzip_requests.fetch_add(1, Ordering::Relaxed);
    }

    reject_advertised_oversize(&parts.headers, state.limits.max_content_length)
        .map_err(|error| error.outcome(&state.protocol))?;

    let wire = read_limited_body(
        body,
        state.limits.max_wire_body_bytes,
        state.limits.body_idle_timeout,
        state.limits.body_total_timeout,
    )
    .await
    .map_err(|error| error.outcome(&state.protocol))?;
    state
        .stats
        .wire_bytes
        .fetch_add(wire.len() as u64, Ordering::Relaxed);

    let decoded = decode_limited(
        wire,
        encoding,
        state.limits.max_decoded_body_bytes,
        state.limits.gzip_buffer_bytes,
    )
    .map_err(|error| {
        if encoding == Encoding::Gzip {
            state.stats.gzip_failures.fetch_add(1, Ordering::Relaxed);
        }
        error.outcome(&state.protocol)
    })?;
    state
        .stats
        .decoded_bytes
        .fetch_add(decoded.len() as u64, Ordering::Relaxed);

    let events = match endpoint {
        Endpoint::Event => parse_event_body(
            &decoded,
            state.limits.max_events_per_request,
            &state.protocol,
        )
        .map_err(|outcome| {
            state.stats.parse_failures.fetch_add(1, Ordering::Relaxed);
            outcome
        })?,
        Endpoint::Raw => {
            parse_raw_body(&decoded, state.limits.max_events_per_request).map_err(|error| {
                if matches!(error, HecError::InvalidDataFormat | HecError::NoData) {
                    state.stats.parse_failures.fetch_add(1, Ordering::Relaxed);
                }
                error.outcome(&state.protocol)
            })?
        }
    };

    let event_count = events.len() as u64;
    state
        .stats
        .events_observed
        .fetch_add(event_count, Ordering::Relaxed);

    let sink_report = state.sink.submit_batch(&events).await.map_err(|_| {
        state.stats.sink_failures.fetch_add(1, Ordering::Relaxed);
        HecError::ServerBusy.outcome(&state.protocol)
    })?;
    state
        .stats
        .events_drop_sink
        .fetch_add(sink_report.dropped as u64, Ordering::Relaxed);
    state
        .stats
        .events_written
        .fetch_add(sink_report.written as u64, Ordering::Relaxed);

    Ok(HecOutcome::success(&state.protocol))
}

fn record_error_outcome(state: &AppState, outcome: &HecOutcome) {
    if matches!(
        outcome.code,
        code if code == state.protocol.token_required
            || code == state.protocol.invalid_authorization
            || code == state.protocol.invalid_token
    ) {
        inc(&state.stats.auth_failures);
    } else if outcome.status == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
        inc(&state.stats.body_too_large);
    } else if outcome.code == state.protocol.invalid_data_format {
        inc(&state.stats.parse_failures);
    } else if outcome.status == axum::http::StatusCode::REQUEST_TIMEOUT {
        inc(&state.stats.timeouts);
    }
}

fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hec_receiver::{app::Limits, AppState};
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, StatusCode},
    };

    #[tokio::test]
    async fn missing_auth_returns_hec_json_error() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = handle_hec(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.as_ref(), br#"{"text":"Token is required","code":2}"#);
    }

    #[tokio::test]
    async fn malformed_event_returns_hec_json_error() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"host":"h"}"#))
            .unwrap();

        let response = handle_hec(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Event field is required","code":12,"invalid-event-number":0}"#
        );
    }
}
