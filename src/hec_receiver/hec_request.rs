use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
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
    protocol::Protocol,
    report::{facts, field, Outcome, Reason, ReportContext},
};

pub async fn post_event(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    process_hec_request(state, request, Endpoint::Event).await
}

pub async fn post_raw(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    process_hec_request(state, request, Endpoint::Raw).await
}

pub async fn post_ack(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    process_ack_request(state, request).await
}

struct PipelineOk {
    response: HecResponse,
    token_id: String,
}

struct PipelineErr {
    response: HecResponse,
    token_id: Option<String>,
    reason: Reason,
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
    HecResponse::new(status, text, code).into_response()
}

pub async fn stats(State(state): State<Arc<AppState>>) -> Response {
    Json(state.reporter.stats_snapshot()).into_response()
}

pub async fn not_found() -> Response {
    HecResponse::new(
        StatusCode::NOT_FOUND,
        "The requested URL was not found on this server.",
        404,
    )
    .into_response()
}

pub async fn method_not_allowed() -> Response {
    HecResponse::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "The requested URL was not found on this server.",
        404,
    )
    .into_response()
}

async fn process_ack_request(state: Arc<AppState>, request: Request<Body>) -> Response {
    let started = Instant::now();
    let ctx = ReportContext::request();
    let route_alias = request.uri().path().to_string();
    state.reporter.submit(
        &ctx,
        facts::REQUEST_RECEIVED,
        vec![
            field::endpoint_kind(Endpoint::Ack),
            field::route_alias(route_alias.clone()),
        ],
    );

    let (parts, _) = request.into_parts();
    let (outcome, reason, token_id) = if query_has_token(parts.uri.query()) {
        let error = HecError::QueryStringAuthorizationDisabled;
        (error.outcome(&state.protocol), error.reason(), None)
    } else {
        match state.tokens.authenticate_detailed(&parts.headers) {
            Ok(auth) if auth.ack_enabled => (
                HecError::AckDisabled.outcome(&state.protocol),
                HecError::AckDisabled.reason(),
                Some(auth.token_id),
            ),
            Ok(auth) => (
                HecError::AckDisabled.outcome(&state.protocol),
                HecError::AckDisabled.reason(),
                Some(auth.token_id),
            ),
            Err(failure) => {
                let outcome = failure.error.outcome(&state.protocol);
                let reason = failure.error.reason();
                report_auth_error(
                    &state,
                    &ctx,
                    failure.error,
                    failure.token_id.as_deref(),
                    &outcome,
                );
                (outcome, reason, failure.token_id)
            }
        }
    };
    let mut fields = vec![
        field::outcome(outcome_from_response(&outcome)),
        field::endpoint_kind(Endpoint::Ack),
        field::route_alias(route_alias),
        field::reason(reason),
        field::hec_code(outcome.code),
        field::http_status(outcome.status.as_u16()),
        field::failure_reason(outcome.text),
        field::elapsed_us(started.elapsed()),
    ];
    if let Some(token_id) = token_id {
        fields.push(field::token_id(token_id));
    }
    state.reporter.submit(&ctx, facts::REQUEST_FAILED, fields);

    outcome.into_response()
}

async fn process_hec_request(
    state: Arc<AppState>,
    request: Request<Body>,
    endpoint: Endpoint,
) -> Response {
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

    let result = process_hec_request_pipeline(&state, &ctx, request, endpoint).await;
    let response = match result {
        Ok(success) => {
            state.reporter.submit(
                &ctx,
                facts::REQUEST_SUCCEEDED,
                vec![
                    field::outcome(Outcome::Accepted),
                    field::endpoint_kind(endpoint),
                    field::route_alias(route_alias),
                    field::token_id(success.token_id),
                    field::hec_code(success.response.code),
                    field::http_status(success.response.status.as_u16()),
                    field::elapsed_us(started.elapsed()),
                ],
            );
            success.response.into_response()
        }
        Err(failure) => {
            let mut fields = vec![
                field::outcome(outcome_from_response(&failure.response)),
                field::endpoint_kind(endpoint),
                field::route_alias(route_alias),
                field::reason(failure.reason),
                field::hec_code(failure.response.code),
                field::http_status(failure.response.status.as_u16()),
                field::failure_reason(failure.response.text),
                field::elapsed_us(started.elapsed()),
            ];
            if let Some(token_id) = failure.token_id {
                fields.push(field::token_id(token_id));
            }
            state.reporter.submit(&ctx, facts::REQUEST_FAILED, fields);
            failure.response.into_response()
        }
    };

    response
}

async fn process_hec_request_pipeline(
    state: &Arc<AppState>,
    ctx: &ReportContext,
    request: Request<Body>,
    endpoint: Endpoint,
) -> Result<PipelineOk, PipelineErr> {
    let phase = state.health.current();
    if !phase.admits_work() {
        let error = match phase {
            Phase::Stopping => HecError::ServerShuttingDown,
            _ => HecError::ServerBusy,
        };
        return Err(pipeline_error(error, None, &state.protocol));
    }

    let (parts, body) = request.into_parts();
    if query_has_token(parts.uri.query()) {
        let error = HecError::QueryStringAuthorizationDisabled;
        let outcome = error.outcome(&state.protocol);
        report_auth_error(state, ctx, error, None, &outcome);
        return Err(PipelineErr {
            response: outcome,
            token_id: None,
            reason: error.reason(),
        });
    }

    let auth = state
        .tokens
        .authenticate_detailed(&parts.headers)
        .map_err(|failure| {
            let outcome = failure.error.outcome(&state.protocol);
            report_auth_error(
                state,
                ctx,
                failure.error,
                failure.token_id.as_deref(),
                &outcome,
            );
            PipelineErr {
                response: outcome,
                token_id: failure.token_id,
                reason: failure.error.reason(),
            }
        })?;
    let token_id = auth.token_id.clone();

    let encoding = parse_content_encoding(&parts.headers).map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        if matches!(error, HecError::UnsupportedEncoding) {
            state.reporter.submit(
                ctx,
                facts::BODY_UNSUPPORTED_ENCODING,
                vec![
                    field::outcome(Outcome::Rejected),
                    field::reason(error.reason()),
                    field::hec_code(outcome.code),
                    field::http_status(outcome.status.as_u16()),
                    field::failure_reason(outcome.text),
                ],
            );
        }
        PipelineErr {
            response: outcome,
            token_id: Some(token_id.clone()),
            reason: error.reason(),
        }
    })?;
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
            PipelineErr {
                response: outcome,
                token_id: Some(token_id.clone()),
                reason: error.reason(),
            }
        },
    )?;

    let http_body = read_limited_body(
        body,
        state.limits.max_http_body_bytes,
        state.limits.body_idle_timeout,
        state.limits.body_total_timeout,
    )
    .await
    .map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        report_body_error(state, ctx, error, &outcome);
        PipelineErr {
            response: outcome,
            token_id: Some(token_id.clone()),
            reason: error.reason(),
        }
    })?;
    state.reporter.submit(
        ctx,
        facts::HTTP_BODY_READ,
        vec![field::http_body_len(http_body.len())],
    );

    let http_body_len = http_body.len();
    let decoded = decode_limited(
        http_body,
        encoding,
        state.limits.max_decoded_body_bytes,
        state.limits.gzip_buffer_bytes,
    )
    .map_err(|error| {
        let outcome = error.outcome(&state.protocol);
        report_decode_error(state, ctx, error, encoding, http_body_len, &outcome);
        PipelineErr {
            response: outcome,
            token_id: Some(token_id.clone()),
            reason: error.reason(),
        }
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
            state.limits.max_index_len,
            auth.default_index.as_deref(),
            &auth.allowed_indexes,
            &state.protocol,
        )
        .map_err(|outcome| {
            let reason = reason_from_response(&outcome, &state.protocol);
            let fact = if outcome.code == state.protocol.incorrect_index {
                facts::EVENT_INDEX_INVALID
            } else {
                facts::PARSE_FAILED
            };
            state.reporter.submit(
                ctx,
                fact,
                vec![
                    field::outcome(outcome_from_response(&outcome)),
                    field::reason(reason),
                    field::hec_code(outcome.code),
                    field::http_status(outcome.status.as_u16()),
                    field::endpoint_kind(endpoint),
                    field::failure_reason(outcome.text),
                ],
            );
            PipelineErr {
                response: outcome,
                token_id: Some(token_id.clone()),
                reason,
            }
        })?,
        Endpoint::Raw => parse_raw_body(
            &decoded,
            state.limits.max_events_per_request,
            auth.default_index.as_deref(),
        )
        .map_err(|error| {
            if matches!(error, HecError::InvalidDataFormat | HecError::NoData) {
                let outcome = error.outcome(&state.protocol);
                state.reporter.submit(
                    ctx,
                    facts::PARSE_FAILED,
                    vec![
                        field::outcome(Outcome::Rejected),
                        field::endpoint_kind(endpoint),
                        field::reason(error.reason()),
                        field::hec_code(outcome.code),
                        field::http_status(outcome.status.as_u16()),
                        field::failure_reason(outcome.text),
                    ],
                );
            }
            PipelineErr {
                response: error.outcome(&state.protocol),
                token_id: Some(token_id.clone()),
                reason: error.reason(),
            }
        })?,
        Endpoint::Ack => {
            return Err(pipeline_error(
                HecError::AckDisabled,
                Some(token_id),
                &state.protocol,
            ));
        }
    };

    state.reporter.submit(
        ctx,
        facts::EVENTS_PARSED,
        vec![field::event_count(events.len())],
    );

    let sink_outcome = state.sink.submit_events(&events).await.map_err(|_| {
        state.reporter.submit(
            ctx,
            facts::SINK_FAILED,
            vec![
                field::outcome(Outcome::Failed),
                field::reason(Reason::SinkFailed),
                field::failure_reason("Sink failed"),
            ],
        );
        PipelineErr {
            response: HecError::ServerBusy.outcome(&state.protocol),
            token_id: Some(token_id.clone()),
            reason: Reason::SinkFailed,
        }
    })?;
    state.reporter.submit(
        ctx,
        facts::SINK_COMPLETED,
        vec![
            field::event_count(events.len()),
            field::drop_count(sink_outcome.dropped),
            field::written_count(sink_outcome.written),
        ],
    );

    Ok(PipelineOk {
        response: HecResponse::success(&state.protocol),
        token_id,
    })
}

fn report_auth_error(
    state: &AppState,
    ctx: &ReportContext,
    error: HecError,
    token_id: Option<&str>,
    outcome: &HecResponse,
) {
    let fact = match error {
        HecError::TokenRequired => facts::AUTH_TOKEN_REQUIRED,
        HecError::InvalidAuthorization => facts::AUTH_INVALID_AUTHORIZATION,
        HecError::QueryStringAuthorizationDisabled => facts::AUTH_INVALID_AUTHORIZATION,
        HecError::InvalidToken => facts::AUTH_TOKEN_INVALID,
        HecError::TokenDisabled => facts::AUTH_TOKEN_DISABLED,
        _ => return,
    };
    let mut fields = vec![
        field::outcome(Outcome::Rejected),
        field::token_present(!matches!(error, HecError::TokenRequired)),
        field::reason(error.reason()),
        field::hec_code(outcome.code),
        field::http_status(outcome.status.as_u16()),
        field::failure_reason(outcome.text),
    ];
    if let Some(token_id) = token_id {
        fields.push(field::token_id(token_id.to_string()));
    }
    state.reporter.submit(ctx, fact, fields);
}

fn query_has_token(query: Option<&str>) -> bool {
    query
        .unwrap_or_default()
        .split('&')
        .filter_map(|part| part.split_once('=').map(|(key, _)| key).or(Some(part)))
        .any(|key| key == "token")
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
            field::reason(error.reason()),
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
    http_body_len: usize,
    outcome: &HecResponse,
) {
    let fact = match (encoding, error) {
        (Encoding::Gzip, HecError::InvalidDataFormat | HecError::MalformedGzip) => {
            facts::GZIP_FAILED
        }
        (_, HecError::BodyTooLarge) => facts::BODY_TOO_LARGE,
        _ => return,
    };
    state.reporter.submit(
        ctx,
        fact,
        vec![
            field::outcome(Outcome::Rejected),
            field::http_body_len(http_body_len),
            field::reason(error.reason()),
            field::hec_code(outcome.code),
            field::http_status(outcome.status.as_u16()),
            field::failure_reason(outcome.text),
        ],
    );
}

fn pipeline_error(error: HecError, token_id: Option<String>, protocol: &Protocol) -> PipelineErr {
    PipelineErr {
        response: error.outcome(protocol),
        token_id,
        reason: error.reason(),
    }
}

fn reason_from_response(outcome: &HecResponse, protocol: &Protocol) -> Reason {
    if outcome.code == protocol.incorrect_index {
        Reason::IncorrectIndex
    } else if outcome.code == protocol.event_field_required {
        Reason::EventFieldRequired
    } else if outcome.code == protocol.event_field_blank {
        Reason::EventFieldBlank
    } else if outcome.code == protocol.handling_indexed_fields {
        Reason::IndexedFields
    } else if outcome.code == protocol.no_data {
        Reason::NoData
    } else if outcome.code == protocol.server_busy {
        Reason::ServerBusy
    } else {
        Reason::InvalidDataFormat
    }
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
    use crate::hec_receiver::{app::Limits, health::Phase, AppState, TokenRegistry};
    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, header::CONTENT_ENCODING, StatusCode},
    };
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    #[tokio::test]
    async fn health_returns_healthy_code_when_serving() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));

        let response = health(State(state)).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"text":"HEC is healthy","code":17}"#);
    }

    #[tokio::test]
    async fn health_returns_unhealthy_code_when_starting() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        state.health.set_phase(Phase::Starting);

        let response = health(State(state)).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.as_ref(), br#"{"text":"HEC is unhealthy","code":18}"#);
    }

    #[tokio::test]
    async fn health_returns_shutdown_code_when_stopping() {
        let state = Arc::new(AppState::drop_events(
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
            br#"{"text":"Server is shutting down","code":23}"#
        );
    }

    #[tokio::test]
    async fn hec_request_returns_shutdown_code_when_stopping() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        state.health.set_phase(Phase::Stopping);
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Raw).await;
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
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.as_ref(), br#"{"text":"Token is required","code":2}"#);
    }

    #[tokio::test]
    async fn blank_authorization_header_returns_token_required() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.as_ref(), br#"{"text":"Token is required","code":2}"#);
    }

    #[tokio::test]
    async fn empty_event_body_returns_no_data() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::empty())
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body.as_ref(), br#"{"text":"No data","code":5}"#);
    }

    #[tokio::test]
    async fn malformed_event_returns_hec_json_error() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"host":"h"}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
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
        let state = Arc::new(AppState::drop_events(
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

        let response = process_hec_request(state.clone(), request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.starts_with(b"<!doctype html>"));
        assert!(body
            .windows(b"The request your client sent was too large.".len())
            .any(|window| window == b"The request your client sent was too large."));
        assert_eq!(state.reporter.stats_snapshot().body_too_large, 1);
    }

    #[tokio::test]
    async fn unsupported_encoding_returns_splunk_style_html() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .header(axum::http::header::CONTENT_ENCODING, "br")
            .body(Body::from("abc"))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(state.reporter.stats_snapshot().unsupported_encoding, 1);
        assert!(body.starts_with(b"<!doctype html>"));
        assert!(body
            .windows(b"The requested URL does not support the media type sent.".len())
            .any(|window| window == b"The requested URL does not support the media type sent."));
    }

    #[tokio::test]
    async fn malformed_gzip_returns_invalid_data_format() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .header(CONTENT_ENCODING, "gzip")
            .body(Body::from("not gzip"))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            b"<!doctype html><html><head><meta http-equiv=\"content-type\" content=\"text/html; charset=UTF-8\"><title>400 Unparsable gzip header in request data</title></head><body><h1>Unparsable gzip header in request data</h1><p>HTTP Request was malformed.</p></body></html>\r\n"
        );
        assert_eq!(state.reporter.stats_snapshot().gzip_failures, 1);
    }

    #[tokio::test]
    async fn gzip_expansion_over_limit_returns_too_large() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits {
                max_decoded_body_bytes: 3,
                ..Limits::default()
            },
        ));
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"abcdef").unwrap();
        let compressed = encoder.finish().unwrap();
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .header(CONTENT_ENCODING, "gzip")
            .body(Body::from(compressed))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body.starts_with(b"<!doctype html>"));
        assert_eq!(state.reporter.stats_snapshot().body_too_large, 1);
    }

    #[tokio::test]
    async fn valid_raw_request_returns_success_code() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"text":"Success","code":0}"#);
    }

    #[tokio::test]
    async fn basic_auth_password_token_returns_success_code() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Basic dXNlcjp0ZXN0LXRva2Vu")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"text":"Success","code":0}"#);
    }

    #[tokio::test]
    async fn bearer_auth_returns_invalid_authorization() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Bearer test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Invalid authorization","code":3}"#
        );
    }

    #[tokio::test]
    async fn query_string_token_returns_code_16() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw?token=test-token")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Query string authorization is not enabled","code":16}"#
        );
    }

    #[tokio::test]
    async fn default_index_is_applied_to_event_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let state = Arc::new(AppState::capture_file_with_registry(
            TokenRegistry::single(
                "default".to_string(),
                "test-token".to_string(),
                true,
                Some("main".to_string()),
                vec!["main".to_string()],
                false,
            ),
            Limits::default(),
            &path,
            Default::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Event).await;
        assert_eq!(response.status(), StatusCode::OK);
        let written = tokio::fs::read_to_string(path).await.unwrap();
        let event: serde_json::Value =
            serde_json::from_str(written.lines().next().unwrap()).unwrap();

        assert_eq!(event["index"], "main");
    }

    #[tokio::test]
    async fn default_index_is_applied_to_raw_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let state = Arc::new(AppState::capture_file_with_registry(
            TokenRegistry::single(
                "default".to_string(),
                "test-token".to_string(),
                true,
                Some("main".to_string()),
                vec!["main".to_string()],
                false,
            ),
            Limits::default(),
            &path,
            Default::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("x\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        assert_eq!(response.status(), StatusCode::OK);
        let written = tokio::fs::read_to_string(path).await.unwrap();
        let event: serde_json::Value =
            serde_json::from_str(written.lines().next().unwrap()).unwrap();

        assert_eq!(event["index"], "main");
    }

    #[tokio::test]
    async fn ack_endpoint_returns_ack_disabled_code() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/ack")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"acks":[1]}"#))
            .unwrap();

        let response = process_ack_request(state, request).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body.as_ref(), br#"{"text":"ACK is disabled","code":14}"#);
    }

    #[tokio::test]
    async fn ack_endpoint_still_requires_authentication() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/ack")
            .body(Body::from(r#"{"acks":[1]}"#))
            .unwrap();

        let response = process_ack_request(state, request).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.as_ref(), br#"{"text":"Token is required","code":2}"#);
    }

    #[tokio::test]
    async fn malformed_authorization_returns_code_3() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Token test-token")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Invalid authorization","code":3}"#
        );
    }

    #[tokio::test]
    async fn invalid_token_returns_code_4() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk wrong-token")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::FORBIDDEN);
        assert_eq!(body.as_ref(), br#"{"text":"Invalid token","code":4}"#);
    }

    #[tokio::test]
    async fn disabled_token_returns_code_1() {
        let state = Arc::new(AppState::drop_events_with_registry(
            TokenRegistry::single(
                "disabled".to_string(),
                "test-token".to_string(),
                false,
                Some("main".to_string()),
                vec!["main".to_string()],
                false,
            ),
            Limits::default(),
            Default::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x"}"#))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::FORBIDDEN);
        assert_eq!(body.as_ref(), br#"{"text":"Token is disabled","code":1}"#);
        let stats = state.reporter.stats_snapshot();
        assert_eq!(stats.auth_failures, 1);
        assert_eq!(
            stats.reasons["hec.auth.token_disabled"]["token_disabled"],
            1
        );
        assert_eq!(stats.reasons["hec.request.failed"]["token_disabled"], 1);
    }

    #[tokio::test]
    async fn blank_raw_body_returns_code_5() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from("\n\r\n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body.as_ref(), br#"{"text":"No data","code":5}"#);
    }

    #[tokio::test]
    async fn whitespace_only_raw_body_returns_code_5() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/raw")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(" \t \n"))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Raw).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(body.as_ref(), br#"{"text":"No data","code":5}"#);
    }

    #[tokio::test]
    async fn malformed_json_returns_code_6() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"unterminated""#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Invalid data format","code":6,"invalid-event-number":0}"#
        );
    }

    #[tokio::test]
    async fn max_events_exceeded_returns_code_9() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits {
                max_events_per_request: 1,
                ..Limits::default()
            },
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"one"}{"event":"two"}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body.as_ref(), br#"{"text":"Server is busy","code":9}"#);
    }

    #[tokio::test]
    async fn invalid_index_returns_code_7() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x","index":"Bad.Index"}"#))
            .unwrap();

        let response = process_hec_request(state.clone(), request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Incorrect index","code":7,"invalid-event-number":1}"#
        );
        assert_eq!(state.reporter.stats_snapshot().parse_failures, 1);
    }

    #[tokio::test]
    async fn json_array_batch_returns_success_code() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"[{"event":"one"},{"event":"two"}]"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"text":"Success","code":0}"#);
    }

    #[tokio::test]
    async fn blank_event_returns_code_13() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":""}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Event field cannot be blank","code":13,"invalid-event-number":0}"#
        );
    }

    #[tokio::test]
    async fn nested_indexed_field_returns_code_15() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x","fields":{"nested":{"x":1}}}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Error in handling indexed fields","code":15,"invalid-event-number":0}"#
        );
    }

    #[tokio::test]
    async fn array_indexed_field_value_is_accepted() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x","fields":{"roles":["admin"]}}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::OK);
        assert_eq!(body.as_ref(), br#"{"text":"Success","code":0}"#);
    }

    #[tokio::test]
    async fn fields_array_returns_invalid_data_format_code_6() {
        let state = Arc::new(AppState::drop_events(
            vec!["test-token".to_string()],
            Limits::default(),
        ));
        let request = Request::builder()
            .uri("/services/collector/event")
            .header(AUTHORIZATION, "Splunk test-token")
            .body(Body::from(r#"{"event":"x","fields":["not","object"]}"#))
            .unwrap();

        let response = process_hec_request(state, request, Endpoint::Event).await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"Invalid data format","code":6,"invalid-event-number":0}"#
        );
    }

    #[tokio::test]
    async fn unknown_route_returns_splunk_style_json_404() {
        let response = not_found().await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::NOT_FOUND);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"The requested URL was not found on this server.","code":404}"#
        );
    }

    #[tokio::test]
    async fn wrong_method_returns_splunk_style_json_405() {
        let response = method_not_allowed().await;
        let (parts, body) = response.into_parts();
        let body = to_bytes(body, usize::MAX).await.unwrap();

        assert_eq!(parts.status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            body.as_ref(),
            br#"{"text":"The requested URL was not found on this server.","code":404}"#
        );
    }
}
