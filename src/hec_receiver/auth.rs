use axum::http::{header::AUTHORIZATION, HeaderMap};
use std::collections::HashSet;

use super::outcome::HecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub scheme: AuthScheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Splunk,
    Bearer,
}

#[derive(Debug, Clone)]
pub struct TokenStore {
    tokens: HashSet<String>,
}

impl TokenStore {
    pub fn new(tokens: Vec<String>) -> Self {
        let tokens = tokens
            .into_iter()
            .filter(|token| !token.is_empty())
            .collect::<HashSet<_>>();
        Self { tokens }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<AuthContext, HecError> {
        let parsed = parse_authorization(headers)?;
        if self.tokens.contains(parsed.token) {
            Ok(AuthContext {
                scheme: parsed.scheme,
            })
        } else {
            Err(HecError::InvalidToken)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedAuthorization<'a> {
    scheme: AuthScheme,
    token: &'a str,
}

fn parse_authorization(headers: &HeaderMap) -> Result<ParsedAuthorization<'_>, HecError> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Err(HecError::TokenRequired);
    };
    let header = value.to_str().map_err(|_| HecError::InvalidAuthorization)?;
    let header = header.trim();
    if header.is_empty() {
        return Err(HecError::TokenRequired);
    }

    let (scheme, token) = header
        .split_once(char::is_whitespace)
        .map(|(scheme, token)| (scheme, token.trim()))
        .unwrap_or((header, ""));
    if token.is_empty() {
        return Err(HecError::InvalidAuthorization);
    }

    let scheme = if scheme.eq_ignore_ascii_case("Splunk") {
        AuthScheme::Splunk
    } else if scheme.eq_ignore_ascii_case("Bearer") {
        AuthScheme::Bearer
    } else {
        return Err(HecError::InvalidAuthorization);
    };

    Ok(ParsedAuthorization { scheme, token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn accepts_splunk_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk abc"));
        let store = TokenStore::new(vec!["abc".to_string()]);
        assert_eq!(
            store.authenticate(&headers).unwrap().scheme,
            AuthScheme::Splunk
        );
    }

    #[test]
    fn rejects_unknown_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk wrong"));
        let store = TokenStore::new(vec!["abc".to_string()]);
        assert_eq!(store.authenticate(&headers), Err(HecError::InvalidToken));
    }

    #[test]
    fn distinguishes_malformed_from_absent() {
        let headers = HeaderMap::new();
        assert_eq!(parse_authorization(&headers), Err(HecError::TokenRequired));

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Token abc"));
        assert_eq!(
            parse_authorization(&headers),
            Err(HecError::InvalidAuthorization)
        );
    }

    #[test]
    fn rejects_non_text_header_value() {
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_bytes(b"Splunk \xff").unwrap();
        headers.insert(AUTHORIZATION, value);
        assert_eq!(
            parse_authorization(&headers),
            Err(HecError::InvalidAuthorization)
        );
    }

    #[test]
    fn rejects_missing_token_after_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk "));
        assert_eq!(
            parse_authorization(&headers),
            Err(HecError::InvalidAuthorization)
        );
    }
}
