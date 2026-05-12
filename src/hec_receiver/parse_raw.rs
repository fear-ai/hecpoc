use bytes::Bytes;

use super::{
    event::{Endpoint, Event},
    outcome::HecError,
};

pub fn parse_raw_body(
    body: &Bytes,
    max_events: usize,
    default_index: Option<&str>,
) -> Result<Vec<Event>, HecError> {
    if body.is_empty() {
        return Err(HecError::NoData);
    }

    let mut events = Vec::new();
    for line in body.split(|byte| *byte == b'\n') {
        if events.len() >= max_events {
            return Err(HecError::ServerBusy);
        }
        let line = strip_trailing_cr(line);
        if is_blank_raw_line(line) {
            continue;
        }
        let raw_bytes_len = line.len();
        let raw = String::from_utf8_lossy(line).into_owned();
        let mut event = Event::from_raw_line_with_len(raw_bytes_len, raw, Endpoint::Raw);
        event.index = default_index.map(ToOwned::to_owned);
        events.push(event);
    }

    if events.is_empty() {
        Err(HecError::NoData)
    } else {
        Ok(events)
    }
}

fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn is_blank_raw_line(line: &[u8]) -> bool {
    line.is_empty() || line.iter().all(|byte| byte.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_crlf_lines() {
        let body = Bytes::from_static(b"one\r\ntwo\n");
        let events = parse_raw_body(&body, 10, None).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].raw, "one");
        assert_eq!(events[1].raw, "two");
    }

    #[test]
    fn lossy_decodes_non_utf8_without_panic() {
        let body = Bytes::from_static(b"\xff\xff\n");
        let events = parse_raw_body(&body, 10, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].raw_bytes_len, 2);
        assert!(events[0].raw.len() > events[0].raw_bytes_len);
    }

    #[test]
    fn preserves_nul_inside_raw_line() {
        let body = Bytes::from_static(b"a\0b\n");
        let events = parse_raw_body(&body, 10, None).unwrap();
        assert_eq!(events[0].raw.as_bytes(), b"a\0b");
    }

    #[test]
    fn rejects_only_blank_lines_as_no_data() {
        let body = Bytes::from_static(b"\n\r\n \t \n");
        assert_eq!(
            parse_raw_body(&body, 10, None).unwrap_err(),
            HecError::NoData
        );
    }

    #[test]
    fn applies_default_index_to_raw_events() {
        let body = Bytes::from_static(b"one\n");
        let events = parse_raw_body(&body, 10, Some("app_logs")).unwrap();
        assert_eq!(events[0].index.as_deref(), Some("app_logs"));
    }
}
