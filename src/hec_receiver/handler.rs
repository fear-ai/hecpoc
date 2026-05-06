use axum::{
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{sync::Arc, time::Instant};

use super::{
    app::AppState,
    body::{
        decode_limited, parse_content_encoding, read_limited_body, reject_advertised_oversize,
        Encoding,
    },
    event::Endpoint,
    health::Phase,
    outcome::{HecError, HecResponse},
    parse_event::parse_event_body,
    parse_raw::parse_raw_body,
    report::{facts, field, Outcome, ReportContext},
};

pub async fn post_event(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    handle_hec(state, request, Endpoint::Event).await
}

pub async fn post_raw(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    handle_hec(state, request, Endpoint::Raw).await
}

pub async fn health(State(state): State<Arc<AppState>>) -> Response {
    let phase = state.health.current();
    let (status, text, code) = match phase {
        Phase::Serving | Phase::Degraded => (
            axum::http::StatusCode::OK,
            "HEC is healthy",
            state.protocol.health_ok,
        ),
        Phase::Stopping => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is shutting down",
            state.protocol.server_shutting_down,
        ),
        Phase::Starting => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "HEC is unhealthy",
            state.protocol.health_unhealthy,
        ),
    };
    (
        status,
        Json(json!({
            "text": text,
            "code": code
        })),
    )
        .into_response()
}

pub async fn stats(State(state): State<Arc<AppState>>) -> Response {
    Json(state.reporter.stats_snapshot()).into_response()
}

async fn handle_hec(state: Arc<AppState>, request: Request<Body>, endpoint: Endpoint) -> Response {
    let started = Instant::now();
    let ctx = ReportContext::request();
    let route_alias = request.uri().path().to_string();
    state.reporter.submit(
        &ctx,
        facts::REQUEST_RECEIVED,
        vec![
            field::endpoint_kind(endpoint),
            field::route_alias(route_alias.clone()),
        ],
    );

    let result = handle_hec_inner(&state, &ctx, request, endpoint).await;
    let response = match result {
        Ok(outcome) => {
            state.reporter.submit(
                &ctx,
                facts::REQUEST_SUCCEEDED,
                vec![
                    field::outcome(Outcome::Accepted),
                    field::endpoint_kind(endpoint),
                    field::route_alias(route_alias),
                    field::hec_code(outcome.code),
                    field::http_status(outcome.status.as_u16()),
                    field::elapsed_us(started.elapsed()),
                ],
            );
            outcome.into_response()
        }
        Err(outcome) => {
            state.reporter.submit(
                &ctx,
                facts::REQUEST_FAILED,
                vec![
                    field::outcome(outcome_from_response(&outcome)),
                    field::endpoint_kind(endpoint),
                    field::route_alias(route_alias),
                    field::hec_code(outcome.code),
                    field::http_status(outcome.status.as_u16()),
                    field::failure_reason(outcome.text),
                    field::elapsed_us(started.elapsed()),
                ],
            );
            outcome.into_response()
        }
    };

    response
}

async fn handle_hec_inner(
    state: &Arc<AppState>,
    ctx: &ReportContext,
    request: Request<Body>,
    endpoint: Endpoint,
) -> Result<HecResponse, HecResponse> {
    let phase = state.health.current();
    if !phase.admits_work() {
        let error = match phase {
            Phase::Stopping => HecError::ServerShuttingDown,
            _ => HecError::ServerBusy,
        };
        return Err(error.outcome(&state.protocol));
    }

    let (parts, body) = request.into_parts();
    state.tokens.authenticate(&parts.headers).map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        report_auth_error(state, ctx, error, &outcome);
        outcome
    })?;

    let encoding =
        parse_content_encoding(&parts.headers).map_err(|error| error.outcome(&state.protocol))?;
    if encoding == Encoding::Gzip {
        state.reporter.submit(
            ctx,
            facts::GZIP_REQUEST,
            vec![field::outcome(Outcome::Accepted)],
        );
    }

    reject_advertised_oversize(&parts.headers, state.limits.max_content_length).map_err(
        |error| {
            let outcome = error.outcome(&state.protocol);
            report_body_error(state, ctx, error, &outcome);
            outcome
        },
    )?;

    let wire = read_limited_body(
        body,
        state.limits.max_wire_body_bytes,
        state.limits.body_idle_timeout,
        state.limits.body_total_timeout,
    )
    .await
    .map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        report_body_error(state, ctx, error, &outcome);
        outcome
    })?;
    state.reporter.submit(
        ctx,
        facts::WIRE_BODY_READ,
        vec![field::wire_len(wire.len())],
    );

    let wire_len = wire.len();
    let decoded = decode_limited(
        wire,
        encoding,
        state.limits.max_decoded_body_bytes,
        state.limits.gzip_buffer_bytes,
    )
    .map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        report_decode_error(state, ctx, error, encoding, wire_len, &outcome);
        outcome
    })?;
    state.reporter.submit(
        ctx,
        facts::BODY_DECODED,
        vec![field::decoded_len(decoded.len())],
    );

    let events = match endpoint {
        Endpoint::Event => parse_event_body(
            &decoded,
            state.limits.max_events_per_request,
            &state.protocol,
        )
        .map_err(|outcome| {
            state.reporter.submit(
                ctx,
                facts::PARSE_FAILED,
                vec![
                    field::outcome(outcome_from_response(&outcome)),
                    field::hec_code(outcome.code),
                    field::http_status(outcome.status.as_u16()),
                    field::endpoint_kind(endpoint),
                ],
            );
            outcome
        })?,
        Endpoint::Raw => {
            parse_raw_body(&decoded, state.limits.max_events_per_request).map_err(|error| {
                if matches!(error, HecError::InvalidDataFormat | HecError::NoData) {
                    state.reporter.submit(
                        ctx,
                        facts::PARSE_FAILED,
                        vec![
                            field::outcome(Outcome::Rejected),
                            field::endpoint_kind(endpoint),
                        ],
                    );
                }
                error.outcome(&state.protocol)
            })?
        }
    };

    state.reporter.submit(
        ctx,
        facts::EVENTS_PARSED,
        vec![field::event_count(events.len())],
    );

    let sink_report = state.sink.submit_batch(&events).await.map_err(|_| {
        state.reporter.submit(
            ctx,
            facts::SINK_FAILED,
            vec![field::outcome(Outcome::Failed)],
        );
        HecError::ServerBusy.outcome(&state.protocol)
    })?;
    state.reporter.submit(
        ctx,
        facts::SINK_COMPLETED,
        vec![
            field::event_count(events.len()),
            field::drop_count(sink_report.dropped),
            field::written_count(sink_report.written),
        ],
    );

    Ok(HecResponse::success(&state.protocol))
}

fn report_auth_error(
    state: &AppState,
    ctx: &ReportContext,
    error: HecError,
    outcome: &HecResponse,
) {
    let fact = match error {
        HecError::TokenRequired => facts::AUTH_TOKEN_REQUIRED,
        HecError::InvalidAuthorization => facts::AUTH_INVALID_AUTHORIZATION,
        HecError::InvalidToken => facts::AUTH_TOKEN_INVALID,
        _ => return,
    };
    state.reporter.submit(
        ctx,
        fact,
        vec![
            field::outcome(Outcome::Rejected),
            field::token_present(!matches!(error, HecError::TokenRequired)),
            field::hec_code(outcome.code),
            field::http_status(outcome.status.as_u16()),
        ],
    );
}

fn report_body_error(
    state: &AppState,
    ctx: &ReportContext,
    error: HecError,
    outcome: &HecResponse,
) {
    let fact = match error {
        HecError::BodyTooLarge => facts::BODY_TOO_LARGE,
        HecError::Timeout => facts::BODY_TIMEOUT,
        HecError::InvalidDataFormat => facts::BODY_READ_FAILED,
        _ => return,
    };
    state.reporter.submit(
        ctx,
        fact,
        vec![
            field::outcome(Outcome::Rejected),
            field::hec_code(outcome.code),
            field::http_status(outcome.status.as_u16()),
            field::failure_reason(outcome.text),
        ],
    );
}

fn report_decode_error(
    state: &AppState,
    ctx: &ReportContext,
    error: HecError,
    encoding: Encoding,
    wire_len: usize,
    outcome: &HecResponse,
) {
    let fact = match (encoding, error) {
        (Encoding::Gzip, HecError::InvalidDataFormat) => facts::GZIP_FAILED,
        (_, HecError::BodyTooLarge) => facts::BODY_TOO_LARGE,
        _ => return,
    };
    state.reporter.submit(
        ctx,
        fact,
        vec![
            field::outcome(Outcome::Rejected),
            field::wire_len(wire_len),
            field::hec_code(outcome.code),
            field::http_status(outcome.status.as_u16()),
        ],
    );
}

fn outcome_from_response(outcome: &HecResponse) -> Outcome {
    if outcome.status.is_success() {
        Outcome::Accepted
    } else if outcome.status == axum::http::StatusCode::SERVICE_UNAVAILABLE {
        Outcome::Throttled
    } else {
        Outcome::Rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hec_receiver::{app::Limits, health::Phase, AppState};
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, StatusCode},
    };

    #[tokio::test]
    async fn health_returns_healthy_code_when_serving() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));

        let response = health(State(state)).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"code":17,"text":"HEC is healthy"}"#);
    }

    #[tokio::test]
    async fn health_returns_unhealthy_code_when_starting() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        state.health.set_phase(Phase::Starting);

        let response = health(State(state)).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.as_ref(), br#"{"code":18,"text":"HEC is unhealthy"}"#);
    }

    #[tokio::test]
    async fn health_returns_shutdown_code_when_stopping() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        state.health.set_phase(Phase::Stopping);

        let response = health(State(state)).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body.as_ref(),
            br#"{"code":23,"text":"Server is shutting down"}"#
        );
    }

    #[tokio::test]
    async fn hec_request_returns_shutdown_code_when_stopping() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        state.health.set_phase(Phase::Stopping);
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = handle_hec(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Server is shutting down","code":23}"#
        );
    }

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

    #[tokio::test]
    async fn advertised_oversize_increments_body_too_large_counter() {
        let state = Arc::new(AppState::drop_only(
            vec!["test-token".to_string()],
            Limits {
                max_content_length: 2,
                ..Limits::default()
            },
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .header(axum::http::header::CONTENT_LENGTH, "3")
            .body(Body::from("abc"))
            .unwrap();

        let response = handle_hec(state.clone(), request, Endpoint::Raw).await;

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(state.reporter.stats_snapshot().body_too_large, 1);
    }
}
