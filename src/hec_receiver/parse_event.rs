use bytes::Bytes;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::{
    event::{Endpoint, Event},
    index::is_valid_index_name,
    outcome::{HecError, HecResponse},
    protocol::Protocol,
};

#[derive(Debug, Deserialize)]
struct HecEnvelope {
    event: Option<Value>,
    time: Option<Value>,
    host: Option<String>,
    source: Option<String>,
    sourcetype: Option<String>,
    index: Option<String>,
    fields: Option<Value>,
}

pub fn parse_event_body(
    body: &Bytes,
    max_events: usize,
    max_index_len: usize,
    default_index: Option<&str>,
    allowed_indexes: &[String],
    protocol: &Protocol,
) -> Result<Vec<Event>, HecResponse> {
    if body.is_empty() {
        return Err(HecError::NoData.outcome(protocol));
    }

    let stream = serde_json::Deserializer::from_slice(body).into_iter::<Value>();
    let mut events = Vec::new();
    for value in stream {
        let value =
            value.map_err(|_| event_error(HecError::InvalidDataFormat, events.len(), protocol))?;
        match value {
            Value::Array(values) => {
                for value in values {
                    push_envelope(
                        value,
                        &mut events,
                        max_events,
                        max_index_len,
                        default_index,
                        allowed_indexes,
                        protocol,
                    )?;
                }
            }
            value => push_envelope(
                value,
                &mut events,
                max_events,
                max_index_len,
                default_index,
                allowed_indexes,
                protocol,
            )?,
        }
    }

    if events.is_empty() {
        Err(HecError::NoData.outcome(protocol))
    } else {
        Ok(events)
    }
}

fn push_envelope(
    value: Value,
    events: &mut Vec<Event>,
    max_events: usize,
    max_index_len: usize,
    default_index: Option<&str>,
    allowed_indexes: &[String],
    protocol: &Protocol,
) -> Result<(), HecResponse> {
    let index = events.len();
    if index >= max_events {
        return Err(HecError::ServerBusy.outcome(protocol));
    }
    let envelope: HecEnvelope =
        from_value(value).map_err(|_| event_error(HecError::InvalidDataFormat, index, protocol))?;
    let event_value = envelope
        .event
        .ok_or_else(|| event_error(HecError::EventFieldRequired, index, protocol))?;
    if event_value.is_null() {
        return Err(event_error(HecError::EventFieldRequired, index, protocol));
    }

    let raw = match event_value {
        Value::String(text) => {
            if text.is_empty() {
                return Err(event_error(HecError::EventFieldBlank, index, protocol));
            }
            text
        }
        other => other.to_string(),
    };

    let event_index = envelope
        .index
        .or_else(|| default_index.map(ToOwned::to_owned));
    validate_index(event_index.as_deref(), max_index_len, allowed_indexes)
        .map_err(|_| index_error(index, protocol))?;

    let fields =
        validate_fields(envelope.fields).map_err(|error| event_error(error, index, protocol))?;

    let raw_bytes_len = raw.len();
    events.push(Event {
        raw,
        raw_bytes_len,
        time: parse_time(envelope.time),
        host: envelope.host,
        source: envelope.source,
        sourcetype: envelope.sourcetype,
        index: event_index,
        fields,
        endpoint: Endpoint::Event,
    });
    Ok(())
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(value)
}

fn validate_index(
    index: Option<&str>,
    max_index_len: usize,
    allowed_indexes: &[String],
) -> Result<(), ()> {
    let Some(index) = index else {
        return Ok(());
    };
    if !is_valid_index_name(index, max_index_len) {
        return Err(());
    }
    if !allowed_indexes.is_empty() && !allowed_indexes.iter().any(|allowed| allowed == index) {
        return Err(());
    }
    Ok(())
}

fn validate_fields(fields: Option<Value>) -> Result<Option<Value>, HecError> {
    let Some(fields) = fields else {
        return Ok(None);
    };
    let Value::Object(map) = &fields else {
        return Err(HecError::InvalidDataFormat);
    };
    if map.values().any(|value| matches!(value, Value::Object(_))) {
        return Err(HecError::HandlingIndexedFields);
    }
    Ok(Some(fields))
}

fn event_error(error: HecError, index: usize, protocol: &Protocol) -> HecResponse {
    error.outcome(protocol).with_invalid_event_number(index)
}

fn index_error(index: usize, protocol: &Protocol) -> HecResponse {
    error_with_splunk_index_number(HecError::IncorrectIndex, index, protocol)
}

fn error_with_splunk_index_number(
    error: HecError,
    index: usize,
    protocol: &Protocol,
) -> HecResponse {
    error.outcome(protocol).with_invalid_event_number(index + 1)
}

fn parse_time(value: Option<Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_concatenated_json_events() {
        let body = Bytes::from_static(br#"{"event":"one"}{"event":{"k":"two"}}"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].raw, "one");
        assert_eq!(events[1].raw, r#"{"k":"two"}"#);
    }

    #[test]
    fn parses_flat_scalar_fields() {
        let body = Bytes::from_static(
            br#"{"event":"x","fields":{"text":"v","n":7,"flag":true,"none":null}}"#,
        );
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].fields.is_some());
    }

    #[test]
    fn rejects_blank_event() {
        let body = Bytes::from_static(br#"{"event":""}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 13);
    }

    #[test]
    fn rejects_nested_fields() {
        let body = Bytes::from_static(br#"{"event":"x","fields":{"nested":{"x":1}}}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 15);
    }

    #[test]
    fn accepts_array_field_values() {
        let body = Bytes::from_static(br#"{"event":"x","fields":{"roles":["admin"]}}"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events[0].fields.as_ref().unwrap()["roles"][0], "admin");
    }

    #[test]
    fn rejects_fields_that_are_not_an_object_as_invalid_data() {
        let body = Bytes::from_static(br#"{"event":"x","fields":["not","object"]}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 6);
    }

    #[test]
    fn rejects_missing_event() {
        let body = Bytes::from_static(br#"{"host":"h"}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 12);
    }

    #[test]
    fn rejects_null_event() {
        let body = Bytes::from_static(br#"{"event":null}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 12);
    }

    #[test]
    fn rejects_trailing_garbage() {
        let body = Bytes::from_static(br#"{"event":"ok"}xyz"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 6);
        assert_eq!(outcome.invalid_event_number, Some(1));
    }

    #[test]
    fn rejects_unclosed_json_object() {
        let body = Bytes::from_static(br#"{"event":"x""#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 6);
        assert_eq!(outcome.invalid_event_number, Some(0));
    }

    #[test]
    fn rejects_unclosed_json_string() {
        let body = Bytes::from_static(br#"{"event":"x}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 6);
        assert_eq!(outcome.invalid_event_number, Some(0));
    }

    #[test]
    fn accepts_json_array_batch() {
        let body = Bytes::from_static(br#"[{"event":"one"},{"event":"two"}]"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].raw, "one");
        assert_eq!(events[1].raw, "two");
    }

    #[test]
    fn rejects_event_count_over_limit() {
        let body = Bytes::from_static(br#"{"event":"one"}{"event":"two"}"#);
        let outcome = parse_event_body(
            &body,
            1,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 9);
    }

    #[test]
    fn accepts_splunk_style_index_name() {
        let body = Bytes::from_static(br#"{"event":"x","index":"app_logs-1"}"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events[0].index.as_deref(), Some("app_logs-1"));
    }

    #[test]
    fn applies_default_index_when_event_omits_index() {
        let body = Bytes::from_static(br#"{"event":"x"}"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            Some("app_logs"),
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events[0].index.as_deref(), Some("app_logs"));
    }

    #[test]
    fn event_index_overrides_default_index() {
        let body = Bytes::from_static(br#"{"event":"x","index":"event_logs"}"#);
        let events = parse_event_body(
            &body,
            10,
            128,
            Some("default_logs"),
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events[0].index.as_deref(), Some("event_logs"));
    }

    #[test]
    fn rejects_index_over_configured_length() {
        let body = Bytes::from_static(br#"{"event":"x","index":"abcd"}"#);
        let outcome = parse_event_body(
            &body,
            10,
            3,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 7);
    }

    #[test]
    fn rejects_index_with_invalid_splunk_syntax() {
        let body = Bytes::from_static(br#"{"event":"x","index":"Bad.Index"}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 7);
    }

    #[test]
    fn rejects_index_with_reserved_kvstore_word() {
        let body = Bytes::from_static(br#"{"event":"x","index":"my_kvstore_logs"}"#);
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &[],
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 7);
        assert_eq!(outcome.invalid_event_number, Some(1));
    }

    #[test]
    fn accepts_index_in_allowed_registry() {
        let body = Bytes::from_static(br#"{"event":"x","index":"main"}"#);
        let allowed = vec!["main".to_string()];
        let events = parse_event_body(
            &body,
            10,
            128,
            None,
            &allowed,
            &super::super::protocol::Protocol::default(),
        )
        .unwrap();
        assert_eq!(events[0].index.as_deref(), Some("main"));
    }

    #[test]
    fn rejects_index_missing_from_allowed_registry() {
        let body = Bytes::from_static(br#"{"event":"x","index":"other"}"#);
        let allowed = vec!["main".to_string()];
        let outcome = parse_event_body(
            &body,
            10,
            128,
            None,
            &allowed,
            &super::super::protocol::Protocol::default(),
        )
        .unwrap_err();
        assert_eq!(outcome.code, 7);
        assert_eq!(outcome.invalid_event_number, Some(1));
    }
}
