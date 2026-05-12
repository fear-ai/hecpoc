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
    IncorrectIndex,
    EventFieldRequired,
    EventFieldBlank,
    AckDisabled,
    HandlingIndexedFields,
    UnsupportedEncoding,
    BodyTooLarge,
    Timeout,
    ServerShuttingDown,
}

impl HecError {
    pub fn outcome(self, protocol: &Protocol) -> HecResponse {
        match self {
            Self::TokenRequired => HecResponse::new(
                StatusCode::UNAUTHORIZED,
                "Token is required",
                protocol.token_required,
            ),
            Self::InvalidAuthorization => HecResponse::new(
                StatusCode::UNAUTHORIZED,
                "Invalid authorization",
                protocol.invalid_authorization,
            ),
            Self::InvalidToken => HecResponse::new(
                StatusCode::FORBIDDEN,
                "Invalid token",
                protocol.invalid_token,
            ),
            Self::NoData => HecResponse::new(StatusCode::BAD_REQUEST, "No data", protocol.no_data),
            Self::InvalidDataFormat => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "Invalid data format",
                protocol.invalid_data_format,
            ),
            Self::ServerBusy => HecResponse::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is busy",
                protocol.server_busy,
            ),
            Self::IncorrectIndex => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "Incorrect index",
                protocol.incorrect_index,
            ),
            Self::EventFieldRequired => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "Event field is required",
                protocol.event_field_required,
            ),
            Self::EventFieldBlank => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "Event field cannot be blank",
                protocol.event_field_blank,
            ),
            Self::AckDisabled => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "ACK is disabled",
                protocol.ack_disabled,
            ),
            Self::HandlingIndexedFields => HecResponse::new(
                StatusCode::BAD_REQUEST,
                "Error in handling indexed fields",
                protocol.handling_indexed_fields,
            ),
            Self::UnsupportedEncoding => HecResponse::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid data format",
                protocol.invalid_data_format,
            ),
            Self::BodyTooLarge => HecResponse::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request entity too large",
                protocol.invalid_data_format,
            ),
            Self::Timeout => HecResponse::new(
                StatusCode::REQUEST_TIMEOUT,
                "Server is busy",
                protocol.server_busy,
            ),
            Self::ServerShuttingDown => HecResponse::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is shutting down",
                protocol.server_shutting_down,
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HecResponse {
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

impl HecResponse {
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

impl IntoResponse for HecResponse {
    fn into_response(self) -> Response {
        let status = self.status;
        (status, Json(self)).into_response()
    }
}
