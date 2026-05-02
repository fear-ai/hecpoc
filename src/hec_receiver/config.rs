use serde::Deserialize;
use std::{env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use super::{app::Limits, protocol::Protocol};

const DEFAULT_ADDR: &str = "127.0.0.1:18088";
const DEFAULT_TOKEN: &str = "dev-token";
const DEFAULT_MAX_BYTES: usize = 1_048_576;
const DEFAULT_MAX_DECODED_BYTES: usize = 4 * DEFAULT_MAX_BYTES;
const DEFAULT_MAX_EVENTS: usize = 100_000;
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_GZIP_BUFFER_BYTES: usize = 8_192;

const ENV_CONFIG: &str = "HEC_CONFIG";
const ENV_ADDR: &str = "HEC_ADDR";
const ENV_TOKEN: &str = "HEC_TOKEN";
const ENV_SPANK_TOKEN: &str = "SPANK_HEC_TOKEN";
const ENV_CAPTURE: &str = "HEC_CAPTURE";
const ENV_MAX_BYTES: &str = "HEC_MAX_BYTES";
const ENV_MAX_DECODED_BYTES: &str = "HEC_MAX_DECODED_BYTES";
const ENV_MAX_EVENTS: &str = "HEC_MAX_EVENTS";
const ENV_IDLE_TIMEOUT: &str = "HEC_IDLE_TIMEOUT";
const ENV_TOTAL_TIMEOUT: &str = "HEC_TOTAL_TIMEOUT";
const ENV_GZIP_BUFFER_BYTES: &str = "HEC_GZIP_BUFFER_BYTES";

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub addr: SocketAddr,
    pub token: String,
    pub capture_path: Option<String>,
    pub limits: Limits,
    pub protocol: Protocol,
}

impl RuntimeConfig {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = match parse_optional_string(ENV_CONFIG) {
            Some(path) => Self::from_file(path)?,
            None => Self::default(),
        };

        config.apply_env()?;
        Ok(config)
    }

    pub fn from_file(path: impl Into<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        let contents = fs::read_to_string(&path)?;
        let file_config: FileConfig = toml::from_str(&contents)?;
        let mut config = Self::default();
        file_config.apply_to(&mut config);
        Ok(config)
    }

    fn apply_env(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(addr) = parse_optional_string(ENV_ADDR) {
            self.addr = addr.parse::<SocketAddr>()?;
        }
        if let Some(token) =
            parse_optional_string(ENV_TOKEN).or_else(|| parse_optional_string(ENV_SPANK_TOKEN))
        {
            self.token = token;
        }
        if let Some(capture_path) = parse_optional_string(ENV_CAPTURE) {
            self.capture_path = Some(capture_path);
        }
        self.limits.apply_env()?;
        self.protocol.apply_env()?;
        Ok(())
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            addr: DEFAULT_ADDR
                .parse::<SocketAddr>()
                .expect("default HEC address is valid"),
            token: DEFAULT_TOKEN.to_string(),
            capture_path: None,
            limits: Limits::default_values(),
            protocol: Protocol::default(),
        }
    }
}

impl Limits {
    pub fn default_values() -> Self {
        Self {
            max_content_length: DEFAULT_MAX_BYTES,
            max_wire_body_bytes: DEFAULT_MAX_BYTES,
            max_decoded_body_bytes: DEFAULT_MAX_DECODED_BYTES,
            max_events_per_request: DEFAULT_MAX_EVENTS,
            body_idle_timeout: DEFAULT_IDLE_TIMEOUT,
            body_total_timeout: DEFAULT_TOTAL_TIMEOUT,
            gzip_buffer_bytes: DEFAULT_GZIP_BUFFER_BYTES,
        }
    }

    fn apply_env(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(value) = parse_optional_usize(ENV_MAX_BYTES)? {
            self.max_content_length = value;
            self.max_wire_body_bytes = value;
        }
        if let Some(value) = parse_optional_usize(ENV_MAX_DECODED_BYTES)? {
            self.max_decoded_body_bytes = value;
        }
        if let Some(value) = parse_optional_usize(ENV_MAX_EVENTS)? {
            self.max_events_per_request = value;
        }
        if let Some(value) = parse_optional_duration(ENV_IDLE_TIMEOUT)? {
            self.body_idle_timeout = value;
        }
        if let Some(value) = parse_optional_duration(ENV_TOTAL_TIMEOUT)? {
            self.body_total_timeout = value;
        }
        if let Some(value) = parse_optional_usize(ENV_GZIP_BUFFER_BYTES)? {
            self.gzip_buffer_bytes = value;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    hec: Option<FileHec>,
    limits: Option<FileLimits>,
    protocol: Option<FileProtocol>,
}

impl FileConfig {
    fn apply_to(self, config: &mut RuntimeConfig) {
        if let Some(hec) = self.hec {
            hec.apply_to(config);
        }
        if let Some(limits) = self.limits {
            limits.apply_to(&mut config.limits);
        }
        if let Some(protocol) = self.protocol {
            protocol.apply_to(&mut config.protocol);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileHec {
    addr: Option<SocketAddr>,
    token: Option<String>,
    capture: Option<String>,
}

impl FileHec {
    fn apply_to(self, config: &mut RuntimeConfig) {
        if let Some(addr) = self.addr {
            config.addr = addr;
        }
        if let Some(token) = self.token {
            config.token = token;
        }
        if let Some(capture) = self.capture {
            config.capture_path = Some(capture);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileLimits {
    max_bytes: Option<usize>,
    max_decoded_bytes: Option<usize>,
    max_events: Option<usize>,
    idle_timeout: Option<DurationText>,
    total_timeout: Option<DurationText>,
    gzip_buffer_bytes: Option<usize>,
}

impl FileLimits {
    fn apply_to(self, limits: &mut Limits) {
        if let Some(value) = self.max_bytes {
            limits.max_content_length = value;
            limits.max_wire_body_bytes = value;
        }
        if let Some(value) = self.max_decoded_bytes {
            limits.max_decoded_body_bytes = value;
        }
        if let Some(value) = self.max_events {
            limits.max_events_per_request = value;
        }
        if let Some(value) = self.idle_timeout {
            limits.body_idle_timeout = value.into_duration();
        }
        if let Some(value) = self.total_timeout {
            limits.body_total_timeout = value.into_duration();
        }
        if let Some(value) = self.gzip_buffer_bytes {
            limits.gzip_buffer_bytes = value;
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileProtocol {
    success: Option<u16>,
    token_required: Option<u16>,
    invalid_authorization: Option<u16>,
    invalid_token: Option<u16>,
    no_data: Option<u16>,
    invalid_data_format: Option<u16>,
    server_busy: Option<u16>,
    event_field_required: Option<u16>,
    event_field_blank: Option<u16>,
    handling_indexed_fields: Option<u16>,
    health: Option<u16>,
}

impl FileProtocol {
    fn apply_to(self, protocol: &mut Protocol) {
        if let Some(value) = self.success {
            protocol.success = value;
        }
        if let Some(value) = self.token_required {
            protocol.token_required = value;
        }
        if let Some(value) = self.invalid_authorization {
            protocol.invalid_authorization = value;
        }
        if let Some(value) = self.invalid_token {
            protocol.invalid_token = value;
        }
        if let Some(value) = self.no_data {
            protocol.no_data = value;
        }
        if let Some(value) = self.invalid_data_format {
            protocol.invalid_data_format = value;
        }
        if let Some(value) = self.server_busy {
            protocol.server_busy = value;
        }
        if let Some(value) = self.event_field_required {
            protocol.event_field_required = value;
        }
        if let Some(value) = self.event_field_blank {
            protocol.event_field_blank = value;
        }
        if let Some(value) = self.handling_indexed_fields {
            protocol.handling_indexed_fields = value;
        }
        if let Some(value) = self.health {
            protocol.health = value;
        }
    }
}

#[derive(Debug)]
struct DurationText(Duration);

impl DurationText {
    fn into_duration(self) -> Duration {
        self.0
    }
}

impl<'de> Deserialize<'de> for DurationText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        humantime::parse_duration(&value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn parse_optional_usize(
    name: &str,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(Some(value.parse()?)),
        _ => Ok(None),
    }
}

pub(crate) fn parse_optional_u64(name: &str) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(Some(value.parse()?)),
        _ => Ok(None),
    }
}

fn parse_optional_duration(name: &str) -> Result<Option<Duration>, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(Some(humantime::parse_duration(&value)?)),
        _ => Ok(None),
    }
}

fn parse_optional_string(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeConfig;

    #[test]
    fn loads_toml_file_values() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let config_path = dir.path().join("hec.toml");
        std::fs::write(
            &config_path,
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "file-token"
capture = "/tmp/hec-events.jsonl"

[limits]
max_bytes = 12345
max_decoded_bytes = 23456
max_events = 345
idle_timeout = "250ms"
total_timeout = "5s"
gzip_buffer_bytes = 4096

[protocol]
token_required = 202
invalid_token = 204
"#,
        )
        .expect("write config");

        let config = RuntimeConfig::from_file(config_path).expect("load config");

        assert_eq!(config.addr.to_string(), "127.0.0.1:18111");
        assert_eq!(config.token, "file-token");
        assert_eq!(
            config.capture_path.as_deref(),
            Some("/tmp/hec-events.jsonl")
        );
        assert_eq!(config.limits.max_content_length, 12345);
        assert_eq!(config.limits.max_wire_body_bytes, 12345);
        assert_eq!(config.limits.max_decoded_body_bytes, 23456);
        assert_eq!(config.limits.max_events_per_request, 345);
        assert_eq!(config.limits.body_idle_timeout.as_millis(), 250);
        assert_eq!(config.limits.body_total_timeout.as_secs(), 5);
        assert_eq!(config.limits.gzip_buffer_bytes, 4096);
        assert_eq!(config.protocol.token_required, 202);
        assert_eq!(config.protocol.invalid_token, 204);
    }
}
