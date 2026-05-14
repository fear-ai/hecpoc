use axum::{
    body::{Body, Bytes},
    http::{header::CONTENT_ENCODING, header::CONTENT_LENGTH, HeaderMap},
};
use flate2::read::GzDecoder;
use http_body_util::BodyExt;
use std::io::Read;
use std::time::Duration;
use tokio::time;

use super::outcome::HecError;

const MIN_GZIP_BUFFER_BYTES: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Identity,
    Gzip,
}

pub fn parse_content_encoding(headers: &HeaderMap) -> Result<Encoding, HecError> {
    let Some(value) = headers.get(CONTENT_ENCODING) else {
        return Ok(Encoding::Identity);
    };
    let encoding = value.to_str().map_err(|_| HecError::UnsupportedEncoding)?;
    if encoding.eq_ignore_ascii_case("gzip") {
        Ok(Encoding::Gzip)
    } else if encoding.eq_ignore_ascii_case("identity") || encoding.trim().is_empty() {
        Ok(Encoding::Identity)
    } else {
        Err(HecError::UnsupportedEncoding)
    }
}

pub fn reject_advertised_oversize(headers: &HeaderMap, max: usize) -> Result<(), HecError> {
    let Some(value) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let length = value
        .to_str()
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or(HecError::InvalidDataFormat)?;
    if length > max {
        Err(HecError::BodyTooLarge)
    } else {
        Ok(())
    }
}

pub async fn read_limited_body(
    mut body: Body,
    max_bytes: usize,
    idle_timeout: Duration,
    total_timeout: Duration,
) -> Result<Bytes, HecError> {
    time::timeout(total_timeout, async move {
        let mut out = Vec::new();
        loop {
            let frame = match time::timeout(idle_timeout, body.frame()).await {
                Ok(Some(Ok(frame))) => frame,
                Ok(Some(Err(_))) => return Err(HecError::InvalidDataFormat),
                Ok(None) => break,
                Err(_) => return Err(HecError::Timeout),
            };
            if let Some(data) = frame.data_ref() {
                if out.len().saturating_add(data.len()) > max_bytes {
                    return Err(HecError::BodyTooLarge);
                }
                out.extend_from_slice(data);
            }
        }
        Ok(Bytes::from(out))
    })
    .await
    .map_err(|_| HecError::Timeout)?
}

pub fn decode_limited(
    body: Bytes,
    encoding: Encoding,
    max_decoded_bytes: usize,
    gzip_buffer_bytes: usize,
) -> Result<Bytes, HecError> {
    match encoding {
        Encoding::Identity => {
            if body.len() > max_decoded_bytes {
                Err(HecError::BodyTooLarge)
            } else {
                Ok(body)
            }
        }
        Encoding::Gzip => decode_gzip_limited(body, max_decoded_bytes, gzip_buffer_bytes),
    }
}

fn decode_gzip_limited(
    body: Bytes,
    max_decoded_bytes: usize,
    gzip_buffer_bytes: usize,
) -> Result<Bytes, HecError> {
    let mut decoder = GzDecoder::new(body.as_ref());
    let mut out = Vec::new();
    let mut buf = vec![0_u8; gzip_buffer_bytes.max(MIN_GZIP_BUFFER_BYTES)];
    loop {
        let read = decoder
            .read(&mut buf)
            .map_err(|_| HecError::MalformedGzip)?;
        if read == 0 {
            break;
        }
        if out.len().saturating_add(read) > max_decoded_bytes {
            return Err(HecError::BodyTooLarge);
        }
        out.extend_from_slice(&buf[..read]);
    }
    Ok(Bytes::from(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;

    #[test]
    fn parses_gzip_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("GZip"));
        assert_eq!(parse_content_encoding(&headers), Ok(Encoding::Gzip));
    }

    #[test]
    fn rejects_unsupported_encoding() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
        assert_eq!(
            parse_content_encoding(&headers),
            Err(HecError::UnsupportedEncoding)
        );
    }

    #[test]
    fn detects_gzip_decode_limit() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"abcdef").unwrap();
        let compressed = Bytes::from(encoder.finish().unwrap());
        assert_eq!(
            decode_limited(compressed, Encoding::Gzip, 3, 2),
            Err(HecError::BodyTooLarge)
        );
    }

    #[test]
    fn rejects_malformed_content_length() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("many"));
        assert_eq!(
            reject_advertised_oversize(&headers, 10),
            Err(HecError::InvalidDataFormat)
        );
    }

    #[test]
    fn rejects_advertised_oversize() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("11"));
        assert_eq!(
            reject_advertised_oversize(&headers, 10),
            Err(HecError::BodyTooLarge)
        );
    }

    #[tokio::test]
    async fn rejects_actual_body_over_limit_without_content_length() {
        let body = Body::from(Bytes::from_static(b"abcdef"));
        assert_eq!(
            read_limited_body(body, 3, Duration::from_secs(1), Duration::from_secs(1)).await,
            Err(HecError::BodyTooLarge)
        );
    }
}
