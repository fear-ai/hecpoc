use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use super::protocol::Protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HecError {
    TokenRequired,
    InvalidAuthorization,
    InvalidToken,
    NoData,
    InvalidDataFormat,
    ServerBusy,
    EventFieldRequired,
    EventFieldBlank,
    HandlingIndexedFields,
    UnsupportedEncoding,
    BodyTooLarge,
    Timeout,
}

impl HecError {
    pub fn outcome(self, protocol: &Protocol) -> HecOutcome {
        match self {
            Self::TokenRequired => HecOutcome::new(
                StatusCode::UNAUTHORIZED,
                "Token is required",
                protocol.token_required,
            ),
            Self::InvalidAuthorization => HecOutcome::new(
                StatusCode::UNAUTHORIZED,
                "Invalid authorization",
                protocol.invalid_authorization,
            ),
            Self::InvalidToken => HecOutcome::new(
                StatusCode::FORBIDDEN,
                "Invalid token",
                protocol.invalid_token,
            ),
            Self::NoData => HecOutcome::new(StatusCode::BAD_REQUEST, "No data", protocol.no_data),
            Self::InvalidDataFormat => HecOutcome::new(
                StatusCode::BAD_REQUEST,
                "Invalid data format",
                protocol.invalid_data_format,
            ),
            Self::ServerBusy => HecOutcome::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is busy",
                protocol.server_busy,
            ),
            Self::EventFieldRequired => HecOutcome::new(
                StatusCode::BAD_REQUEST,
                "Event field is required",
                protocol.event_field_required,
            ),
            Self::EventFieldBlank => HecOutcome::new(
                StatusCode::BAD_REQUEST,
                "Event field cannot be blank",
                protocol.event_field_blank,
            ),
            Self::HandlingIndexedFields => HecOutcome::new(
                StatusCode::BAD_REQUEST,
                "Error in handling indexed fields",
                protocol.handling_indexed_fields,
            ),
            Self::UnsupportedEncoding => HecOutcome::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid data format",
                protocol.invalid_data_format,
            ),
            Self::BodyTooLarge => HecOutcome::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request entity too large",
                protocol.invalid_data_format,
            ),
            Self::Timeout => HecOutcome::new(
                StatusCode::REQUEST_TIMEOUT,
                "Server is busy",
                protocol.server_busy,
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HecOutcome {
    #[serde(skip)]
    pub status: StatusCode,
    pub text: &'static str,
    pub code: u16,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ackId")]
    pub ack_id: Option<u64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "invalid-event-number"
    )]
    pub invalid_event_number: Option<usize>,
}

impl HecOutcome {
    pub fn new(status: StatusCode, text: &'static str, code: u16) -> Self {
        Self {
            status,
            text,
            code,
            ack_id: None,
            invalid_event_number: None,
        }
    }

    pub fn success(protocol: &Protocol) -> Self {
        Self::new(StatusCode::OK, "Success", protocol.success)
    }

    pub fn with_invalid_event_number(mut self, event_number: usize) -> Self {
        self.invalid_event_number = Some(event_number);
        self
    }
}

impl IntoResponse for HecOutcome {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(self)).into_response()
    }
}
