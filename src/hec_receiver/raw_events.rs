//! Shadow implementation for raw HEC event scanning.
//!
//! This file is intentionally not wired into `hec_receiver::mod` yet. It is a
//! review target for the proposed RawEvents architecture: keep one owned request
//! byte buffer, store raw-event offsets, and materialize current `Event` values
//! only at compatibility boundaries.
//!
//! Wiring checklist, when approved:
//! - move `memchr` from dev-dependencies to dependencies in Cargo.toml;
//! - change `#[cfg(test)] mod raw_events;` to `mod raw_events;` in
//!   `hec_receiver/mod.rs`;
//! - run these tests against the current `parse_raw_body` behavior;
//! - only then consider replacing the current parser internals.

use bytes::Bytes;

use super::{
    event::{Endpoint, Event},
    outcome::HecError,
};

#[derive(Debug, Clone)]
pub struct RequestRaw {
    bytes: Bytes,
    default_index: Option<String>,
}

impl RequestRaw {
    pub fn new(bytes: Bytes, default_index: Option<String>) -> Self {
        Self {
            bytes,
            default_index,
        }
    }

    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    pub fn default_index(&self) -> Option<&str> {
        self.default_index.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEventRef {
    start: usize,
    end: usize,
    had_trailing_cr: bool,
}

impl RawEventRef {
    pub fn start(self) -> usize {
        self.start
    }

    pub fn end(self) -> usize {
        self.end
    }

    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn had_trailing_cr(self) -> bool {
        self.had_trailing_cr
    }
}

#[derive(Debug, Clone)]
pub struct RawEvents {
    request: RequestRaw,
    events: Vec<RawEventRef>,
}

impl RawEvents {
    pub fn request(&self) -> &RequestRaw {
        &self.request
    }

    pub fn event_refs(&self) -> &[RawEventRef] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn views(&self) -> impl Iterator<Item = RawEventView<'_>> {
        self.events.iter().map(|event| RawEventView {
            raw_events: self,
            event: *event,
        })
    }

    pub fn event_bytes(&self, event: RawEventRef) -> &[u8] {
        &self.request.bytes[event.start..event.end]
    }

    pub fn event_text_lossy(&self, event: RawEventRef) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.event_bytes(event))
    }

    pub fn to_owned_event(&self, event: RawEventRef) -> RawEventOwned {
        RawEventOwned {
            bytes: self.request.bytes.clone(),
            start: event.start,
            end: event.end,
            default_index: self.request.default_index.clone(),
        }
    }

    pub fn materialize_event(&self, event: RawEventRef) -> Event {
        let bytes = self.event_bytes(event);
        let mut out = Event::from_raw_line_with_len(
            bytes.len(),
            String::from_utf8_lossy(bytes).into_owned(),
            Endpoint::Raw,
        );
        out.index = self.request.default_index.clone();
        out
    }

    pub fn materialize_events(&self) -> Vec<Event> {
        self.events
            .iter()
            .copied()
            .map(|event| self.materialize_event(event))
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RawEventView<'a> {
    raw_events: &'a RawEvents,
    event: RawEventRef,
}

impl<'a> RawEventView<'a> {
    pub fn offset(self) -> RawEventRef {
        self.event
    }

    pub fn bytes(self) -> &'a [u8] {
        self.raw_events.event_bytes(self.event)
    }

    pub fn text_lossy(self) -> std::borrow::Cow<'a, str> {
        self.raw_events.event_text_lossy(self.event)
    }

    pub fn to_owned_event(self) -> RawEventOwned {
        self.raw_events.to_owned_event(self.event)
    }

    pub fn materialize_event(self) -> Event {
        self.raw_events.materialize_event(self.event)
    }
}

#[derive(Debug, Clone)]
pub struct RawEventOwned {
    bytes: Bytes,
    start: usize,
    end: usize,
    default_index: Option<String>,
}

impl RawEventOwned {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }

    pub fn text_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.bytes())
    }

    pub fn materialize_event(&self) -> Event {
        let bytes = self.bytes();
        let mut out = Event::from_raw_line_with_len(
            bytes.len(),
            String::from_utf8_lossy(bytes).into_owned(),
            Endpoint::Raw,
        );
        out.index = self.default_index.clone();
        out
    }
}

pub fn scan_raw_events(
    bytes: Bytes,
    max_events: usize,
    default_index: Option<String>,
) -> Result<RawEvents, HecError> {
    if bytes.is_empty() {
        return Err(HecError::NoData);
    }

    let mut events = Vec::new();
    let mut start = 0;
    let mut cursor = 0;

    loop {
        let relative_lf = find_lf(&bytes[cursor..]);
        let end = relative_lf
            .map(|offset| cursor + offset)
            .unwrap_or(bytes.len());

        let mut line_end = end;
        if line_end > start && bytes[line_end - 1] == b'\r' {
            line_end -= 1;
        }

        if has_non_whitespace(&bytes[start..line_end]) {
            if events.len() >= max_events {
                return Err(HecError::ServerBusy);
            }
            events.push(RawEventRef {
                start,
                end: line_end,
                had_trailing_cr: line_end < end,
            });
        }

        match relative_lf {
            Some(_) => {
                cursor = end + 1;
                start = cursor;
            }
            None => break,
        }
    }

    if events.is_empty() {
        return Err(HecError::NoData);
    }

    Ok(RawEvents {
        request: RequestRaw::new(bytes, default_index),
        events,
    })
}

pub fn parse_raw_body_compat(
    body: Bytes,
    max_events: usize,
    default_index: Option<String>,
) -> Result<Vec<Event>, HecError> {
    Ok(scan_raw_events(body, max_events, default_index)?.materialize_events())
}

fn find_lf(bytes: &[u8]) -> Option<usize> {
    memchr::memchr(b'\n', bytes)
}

fn has_non_whitespace(line: &[u8]) -> bool {
    line.iter().any(|byte| !byte.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hec_receiver::parse_raw::parse_raw_body;
    use std::borrow::Cow;

    fn scan(bytes: &'static [u8]) -> RawEvents {
        scan_raw_events(Bytes::from_static(bytes), 10, None).unwrap()
    }

    #[test]
    fn accepts_final_line_without_lf() {
        let events = scan(b"one");
        assert_eq!(events.len(), 1);
        assert_eq!(events.views().next().unwrap().bytes(), b"one");
    }

    #[test]
    fn ignores_trailing_empty_segment_after_lf() {
        let events = scan(b"one\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events.views().next().unwrap().bytes(), b"one");
    }

    #[test]
    fn strips_one_trailing_cr_from_crlf() {
        let events = scan(b"one\r\ntwo\n");
        let values = events
            .views()
            .map(|event| event.bytes().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(values, vec![b"one".to_vec(), b"two".to_vec()]);
        assert!(events.event_refs()[0].had_trailing_cr());
    }

    #[test]
    fn rejects_only_blank_lines_as_no_data() {
        assert_eq!(
            scan_raw_events(Bytes::from_static(b"\n\r\n \t \n"), 10, None).unwrap_err(),
            HecError::NoData
        );
    }

    #[test]
    fn preserves_nul_and_invalid_utf8_bytes() {
        let events = scan(b"a\0b\n\xff\xff\n");
        let values = events
            .views()
            .map(|event| event.bytes().to_vec())
            .collect::<Vec<_>>();
        assert_eq!(values, vec![b"a\0b".to_vec(), b"\xff\xff".to_vec()]);
    }

    #[test]
    fn materializes_current_event_shape() {
        let events = scan_raw_events(Bytes::from_static(b"one\n"), 10, Some("main".to_string()))
            .unwrap()
            .materialize_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].raw, "one");
        assert_eq!(events[0].raw_bytes_len, 3);
        assert_eq!(events[0].index.as_deref(), Some("main"));
    }

    #[test]
    fn enforces_max_events_only_before_push() {
        assert_eq!(
            scan_raw_events(Bytes::from_static(b"one\ntwo\n"), 1, None).unwrap_err(),
            HecError::ServerBusy
        );
    }

    #[test]
    fn compat_materialization_matches_current_parser_for_edge_inputs() {
        let cases = [
            b"one".as_slice(),
            b"one\n".as_slice(),
            b"one\r\n".as_slice(),
            b"a\rb\n".as_slice(),
            b"a\0b\n".as_slice(),
            b"\xff\xff\n".as_slice(),
            b"one\n\n two \n".as_slice(),
        ];

        for body in cases {
            let current = parse_raw_body(&Bytes::copy_from_slice(body), 10, Some("main")).unwrap();
            let compat =
                parse_raw_body_compat(Bytes::copy_from_slice(body), 10, Some("main".to_string()))
                    .unwrap();
            assert_eq!(compat.len(), current.len(), "body={body:?}");
            for (compat_event, current_event) in compat.iter().zip(current.iter()) {
                assert_eq!(compat_event.raw, current_event.raw, "body={body:?}");
                assert_eq!(
                    compat_event.raw_bytes_len, current_event.raw_bytes_len,
                    "body={body:?}"
                );
                assert_eq!(compat_event.index, current_event.index, "body={body:?}");
            }
        }
    }

    #[test]
    fn byte_view_supports_scan_only_consumers_without_materialization() {
        let events = scan(b"alpha\nbeta\n");
        let total_bytes: usize = events.views().map(|event| event.bytes().len()).sum();
        assert_eq!(events.len(), 2);
        assert_eq!(total_bytes, 9);
        assert_eq!(events.event_refs()[0].start(), 0);
        assert_eq!(events.event_refs()[0].end(), 5);
    }

    #[test]
    fn accessors_expose_review_api_shape() {
        let events =
            scan_raw_events(Bytes::from_static(b"one\n"), 10, Some("main".to_string())).unwrap();
        assert!(!events.is_empty());
        assert_eq!(events.request().bytes().as_ref(), b"one\n");
        assert_eq!(events.request().default_index(), Some("main"));

        let offset = events.views().next().unwrap().offset();
        assert_eq!(offset.len(), 3);
        assert!(!offset.is_empty());

        let empty_offset = RawEventRef {
            start: 4,
            end: 4,
            had_trailing_cr: false,
        };
        assert!(empty_offset.is_empty());
    }

    #[test]
    fn lossy_text_borrows_valid_utf8_and_owns_invalid_utf8() {
        let valid = scan(b"valid\n");
        let valid_text = valid.views().next().unwrap().text_lossy();
        assert!(matches!(valid_text, Cow::Borrowed("valid")));

        let invalid = scan(b"\xff\xff\n");
        let invalid_text = invalid.views().next().unwrap().text_lossy();
        assert!(matches!(invalid_text, Cow::Owned(_)));
    }

    #[test]
    fn owned_event_survives_after_raw_events_is_dropped() {
        let owned = {
            let events = scan_raw_events(
                Bytes::from_static(b"queued\n"),
                10,
                Some("queued_index".to_string()),
            )
            .unwrap();
            let owned = events.views().next().unwrap().to_owned_event();
            owned
        };

        assert_eq!(owned.bytes(), b"queued");
        assert_eq!(owned.text_lossy(), "queued");
        let event = owned.materialize_event();
        assert_eq!(event.raw, "queued");
        assert_eq!(event.index.as_deref(), Some("queued_index"));
    }

    #[test]
    fn materialize_one_view_for_current_sink_boundary() {
        let events = scan_raw_events(Bytes::from_static(b"one\ntwo\n"), 10, None).unwrap();
        let materialized = events.views().next().unwrap().materialize_event();
        assert_eq!(materialized.raw, "one");
        assert_eq!(materialized.raw_bytes_len, 3);
    }
}
