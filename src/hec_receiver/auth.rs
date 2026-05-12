use axum::http::{header::AUTHORIZATION, HeaderMap};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::collections::HashMap;

use super::outcome::HecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub scheme: AuthScheme,
    pub default_index: Option<String>,
    pub allowed_indexes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    Splunk,
    Basic,
}

#[derive(Debug, Clone)]
pub struct TokenRegistry {
    tokens: HashMap<String, HecToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HecToken {
    secret: String,
    default_index: Option<String>,
    allowed_indexes: Vec<String>,
}

impl HecToken {
    pub fn new(
        secret: String,
        default_index: Option<String>,
        allowed_indexes: Vec<String>,
    ) -> Self {
        Self {
            secret,
            default_index,
            allowed_indexes,
        }
    }

    fn secret(&self) -> &str {
        &self.secret
    }

    fn default_index(&self) -> Option<&str> {
        self.default_index.as_deref()
    }

    fn allowed_indexes(&self) -> &[String] {
        &self.allowed_indexes
    }
}

impl TokenRegistry {
    pub fn new(tokens: Vec<String>) -> Self {
        let tokens = tokens
            .into_iter()
            .map(|token| HecToken::new(token, None, Vec::new()));
        Self::from_tokens(tokens)
    }

    pub fn single(
        token: String,
        default_index: Option<String>,
        allowed_indexes: Vec<String>,
    ) -> Self {
        Self::from_tokens([HecToken::new(token, default_index, allowed_indexes)])
    }

    pub fn from_tokens(tokens: impl IntoIterator<Item = HecToken>) -> Self {
        let tokens = tokens
            .into_iter()
            .filter(|token| !token.secret().is_empty())
            .map(|token| (token.secret().to_string(), token))
            .collect::<HashMap<_, _>>();
        Self { tokens }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<AuthContext, HecError> {
        let parsed = parse_authorization(headers)?;
        if let Some(token) = self.tokens.get(parsed.token.as_ref()) {
            Ok(AuthContext {
                scheme: parsed.scheme,
                default_index: token.default_index().map(ToOwned::to_owned),
                allowed_indexes: token.allowed_indexes().to_vec(),
            })
        } else {
            Err(HecError::InvalidToken)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedAuthorization<'a> {
    scheme: AuthScheme,
    token: std::borrow::Cow<'a, str>,
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

    let scheme = if scheme.eq_ignore_ascii_case("Splunk") {
        if token.is_empty() {
            return Err(HecError::InvalidAuthorization);
        }
        AuthScheme::Splunk
    } else if scheme.eq_ignore_ascii_case("Basic") {
        AuthScheme::Basic
    } else {
        return Err(HecError::InvalidAuthorization);
    };

    let token = match scheme {
        AuthScheme::Splunk => std::borrow::Cow::Borrowed(token),
        AuthScheme::Basic => parse_basic_token(token)?,
    };

    Ok(ParsedAuthorization { scheme, token })
}

fn parse_basic_token(encoded: &str) -> Result<std::borrow::Cow<'_, str>, HecError> {
    if encoded.is_empty() {
        return Err(HecError::InvalidAuthorization);
    }
    let decoded = BASE64
        .decode(encoded)
        .map_err(|_| HecError::InvalidAuthorization)?;
    let decoded = String::from_utf8(decoded).map_err(|_| HecError::InvalidAuthorization)?;
    let (_, password) = decoded
        .split_once(':')
        .ok_or(HecError::InvalidAuthorization)?;
    if password.is_empty() {
        return Err(HecError::InvalidAuthorization);
    }
    Ok(std::borrow::Cow::Owned(password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn accepts_splunk_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk abc"));
        let store = TokenRegistry::new(vec!["abc".to_string()]);
        assert_eq!(
            store.authenticate(&headers).unwrap().scheme,
            AuthScheme::Splunk
        );
    }

    #[test]
    fn accepts_basic_auth_password_as_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjphYmM="),
        );
        let store = TokenRegistry::new(vec!["abc".to_string()]);
        assert_eq!(
            store.authenticate(&headers).unwrap().scheme,
            AuthScheme::Basic
        );
    }

    #[test]
    fn returns_token_default_index() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk abc"));
        let store = TokenRegistry::single(
            "abc".to_string(),
            Some("main".to_string()),
            vec!["main".to_string()],
        );

        let context = store.authenticate(&headers).unwrap();

        assert_eq!(context.default_index.as_deref(), Some("main"));
        assert_eq!(context.allowed_indexes, vec!["main"]);
    }

    #[test]
    fn rejects_unknown_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Splunk wrong"));
        let store = TokenRegistry::new(vec!["abc".to_string()]);
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
    fn rejects_bearer_scheme_for_splunk_compatibility() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc"));
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
