use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub raw: String,
    pub raw_bytes_len: usize,
    pub time: Option<f64>,
    pub host: Option<String>,
    pub source: Option<String>,
    pub sourcetype: Option<String>,
    pub index: Option<String>,
    pub fields: Option<Value>,
    pub endpoint: Endpoint,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Endpoint {
    Event,
    Raw,
    Ack,
}

impl Endpoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Raw => "raw",
            Self::Ack => "ack",
        }
    }
}

impl Event {
    #[allow(dead_code)]
    pub fn from_raw_line(raw: String, endpoint: Endpoint) -> Self {
        Self::from_raw_line_with_len(raw.len(), raw, endpoint)
    }

    pub fn from_raw_line_with_len(raw_bytes_len: usize, raw: String, endpoint: Endpoint) -> Self {
        Self {
            raw,
            raw_bytes_len,
            time: None,
            host: None,
            source: None,
            sourcetype: None,
            index: None,
            fields: None,
            endpoint,
        }
    }
}
