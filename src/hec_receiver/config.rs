use clap::Parser;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};
use thiserror::Error;

use super::{app::Limits, protocol::Protocol, report::ReportOutputs};

const DEFAULT_ADDR: &str = "127.0.0.1:18088";
const DEFAULT_TOKEN: &str = "dev-token";
const DEFAULT_MAX_BYTES: usize = 1_048_576;
const DEFAULT_MAX_DECODED_BYTES: usize = 4 * DEFAULT_MAX_BYTES;
const DEFAULT_MAX_EVENTS: usize = 100_000;
const DEFAULT_IDLE_TIMEOUT: &str = "5s";
const DEFAULT_TOTAL_TIMEOUT: &str = "30s";
const DEFAULT_GZIP_BUFFER_BYTES: usize = 8_192;
const MIN_GZIP_BUFFER_BYTES: usize = 512;
const MAX_GZIP_BUFFER_BYTES: usize = 1_048_576;

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
const ENV_OBSERVE_LEVEL: &str = "HEC_OBSERVE_LEVEL";
const ENV_OBSERVE_FORMAT: &str = "HEC_OBSERVE_FORMAT";
const ENV_OBSERVE_REDACTION_MODE: &str = "HEC_OBSERVE_REDACTION_MODE";
const ENV_OBSERVE_REDACTION_TEXT: &str = "HEC_OBSERVE_REDACTION_TEXT";
const ENV_OBSERVE_TRACING: &str = "HEC_OBSERVE_TRACING";
const ENV_OBSERVE_CONSOLE: &str = "HEC_OBSERVE_CONSOLE";
const ENV_OBSERVE_STATS: &str = "HEC_OBSERVE_STATS";
const DEFAULT_OBSERVE_LEVEL: &str = "info";
const DEFAULT_OBSERVE_FORMAT: &str = "compact";
const DEFAULT_REDACTION_MODE: &str = "redact";
const DEFAULT_REDACTION_TEXT: &str = "<redacted>";

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub addr: SocketAddr,
    pub token: String,
    pub capture_path: Option<String>,
    pub limits: Limits,
    pub protocol: Protocol,
    pub observe: ObserveConfig,
}

#[derive(Debug, Clone)]
pub struct ObserveConfig {
    pub level: String,
    pub format: ObserveFormat,
    pub redaction_mode: RedactionMode,
    pub redaction_text: String,
    pub tracing: bool,
    pub console: bool,
    pub stats: bool,
}

impl ObserveConfig {
    pub(crate) fn report_outputs(&self) -> ReportOutputs {
        ReportOutputs {
            tracing: self.tracing,
            console: self.console,
            stats: self.stats,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveFormat {
    Compact,
    Json,
}

impl ObserveFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionMode {
    Redact,
    Passthrough,
}

impl RedactionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Redact => "redact",
            Self::Passthrough => "passthrough",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    Run,
    ShowConfig,
    CheckConfig,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: RuntimeConfig,
    pub action: ConfigAction,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("configuration source error: {0}")]
    Source(#[from] figment::Error),
    #[error("invalid config field {field}: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
    #[error("failed to render effective configuration: {0}")]
    Render(#[from] toml::ser::Error),
}

#[derive(Debug, Parser, Default)]
#[command(name = "hec-receiver")]
#[command(about = "Run a focused Splunk HEC-compatible receiver")]
#[command(version)]
pub struct Cli {
    #[arg(short, long, env = ENV_CONFIG)]
    pub config: Option<PathBuf>,

    #[arg(long, conflicts_with = "check_config")]
    pub show_config: bool,

    #[arg(long)]
    pub check_config: bool,

    #[arg(long)]
    pub addr: Option<String>,

    #[arg(long)]
    pub token: Option<String>,

    #[arg(long)]
    pub capture: Option<String>,

    #[arg(long)]
    pub max_bytes: Option<usize>,

    #[arg(long)]
    pub max_decoded_bytes: Option<usize>,

    #[arg(long)]
    pub max_events: Option<usize>,

    #[arg(long)]
    pub idle_timeout: Option<String>,

    #[arg(long)]
    pub total_timeout: Option<String>,

    #[arg(long)]
    pub gzip_buffer_bytes: Option<usize>,

    #[arg(long)]
    pub protocol_success: Option<u16>,

    #[arg(long)]
    pub protocol_token_required: Option<u16>,

    #[arg(long)]
    pub protocol_invalid_authorization: Option<u16>,

    #[arg(long)]
    pub protocol_invalid_token: Option<u16>,

    #[arg(long)]
    pub protocol_no_data: Option<u16>,

    #[arg(long)]
    pub protocol_invalid_data_format: Option<u16>,

    #[arg(long)]
    pub protocol_server_busy: Option<u16>,

    #[arg(long)]
    pub protocol_event_field_required: Option<u16>,

    #[arg(long)]
    pub protocol_event_field_blank: Option<u16>,

    #[arg(long)]
    pub protocol_handling_indexed_fields: Option<u16>,

    #[arg(long)]
    pub protocol_health: Option<u16>,

    #[arg(long)]
    pub observe_level: Option<String>,

    #[arg(long)]
    pub observe_format: Option<String>,

    #[arg(long)]
    pub observe_redaction_mode: Option<String>,

    #[arg(long)]
    pub observe_redaction_text: Option<String>,

    #[arg(long)]
    pub observe_tracing: Option<bool>,

    #[arg(long)]
    pub observe_console: Option<bool>,

    #[arg(long)]
    pub observe_stats: Option<bool>,
}

impl RuntimeConfig {
    pub fn load() -> Result<LoadedConfig, ConfigError> {
        Self::load_with_cli(Cli::parse())
    }

    pub fn load_with_cli(cli: Cli) -> Result<LoadedConfig, ConfigError> {
        let action = match (cli.show_config, cli.check_config) {
            (true, _) => ConfigAction::ShowConfig,
            (false, true) => ConfigAction::CheckConfig,
            (false, false) => ConfigAction::Run,
        };
        let config_path = env_string(ENV_CONFIG)
            .map(PathBuf::from)
            .or(cli.config.clone());
        let mut figment = Figment::from(Serialized::defaults(ConfigDoc::compiled_defaults()));
        if let Some(path) = config_path {
            figment = figment.merge(Toml::file(path));
        }
        figment = figment.merge(Serialized::defaults(cli.to_doc()));
        figment = figment.merge(Serialized::defaults(ConfigDoc::from_env()?));

        let doc: ConfigDoc = figment.extract()?;
        let config = doc.to_runtime()?;
        Ok(LoadedConfig { config, action })
    }

    pub fn redacted_toml(&self) -> Result<String, ConfigError> {
        Ok(toml::to_string_pretty(&ConfigDoc::from_runtime(
            self,
            self.observe.redaction_mode == RedactionMode::Redact,
        ))?)
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        ConfigDoc::compiled_defaults()
            .to_runtime()
            .expect("compiled defaults are valid")
    }
}

impl Limits {
    pub fn default_values() -> Self {
        RuntimeConfig::default().limits
    }
}

impl Cli {
    fn to_doc(&self) -> ConfigDoc {
        ConfigDoc {
            hec: Some(HecDoc {
                addr: self.addr.clone(),
                token: self.token.clone(),
                capture: self.capture.clone(),
            })
            .filter(has_hec_values),
            limits: Some(LimitsDoc {
                max_bytes: self.max_bytes,
                max_decoded_bytes: self.max_decoded_bytes,
                max_events: self.max_events,
                idle_timeout: self.idle_timeout.clone(),
                total_timeout: self.total_timeout.clone(),
                gzip_buffer_bytes: self.gzip_buffer_bytes,
            })
            .filter(has_limit_values),
            protocol: Some(ProtocolDoc {
                success: self.protocol_success,
                token_required: self.protocol_token_required,
                invalid_authorization: self.protocol_invalid_authorization,
                invalid_token: self.protocol_invalid_token,
                no_data: self.protocol_no_data,
                invalid_data_format: self.protocol_invalid_data_format,
                server_busy: self.protocol_server_busy,
                event_field_required: self.protocol_event_field_required,
                event_field_blank: self.protocol_event_field_blank,
                handling_indexed_fields: self.protocol_handling_indexed_fields,
                health: self.protocol_health,
            })
            .filter(has_protocol_values),
            observe: Some(ObserveDoc {
                level: self.observe_level.clone(),
                format: self.observe_format.clone(),
                redaction_mode: self.observe_redaction_mode.clone(),
                redaction_text: self.observe_redaction_text.clone(),
                tracing: self.observe_tracing,
                console: self.observe_console,
                stats: self.observe_stats,
            })
            .filter(has_observe_values),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    hec: Option<HecDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limits: Option<LimitsDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<ProtocolDoc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observe: Option<ObserveDoc>,
}

impl ConfigDoc {
    fn compiled_defaults() -> Self {
        Self {
            hec: Some(HecDoc {
                addr: Some(DEFAULT_ADDR.to_string()),
                token: Some(DEFAULT_TOKEN.to_string()),
                capture: None,
            }),
            limits: Some(LimitsDoc {
                max_bytes: Some(DEFAULT_MAX_BYTES),
                max_decoded_bytes: Some(DEFAULT_MAX_DECODED_BYTES),
                max_events: Some(DEFAULT_MAX_EVENTS),
                idle_timeout: Some(DEFAULT_IDLE_TIMEOUT.to_string()),
                total_timeout: Some(DEFAULT_TOTAL_TIMEOUT.to_string()),
                gzip_buffer_bytes: Some(DEFAULT_GZIP_BUFFER_BYTES),
            }),
            protocol: Some(ProtocolDoc::defaults()),
            observe: Some(ObserveDoc {
                level: Some(DEFAULT_OBSERVE_LEVEL.to_string()),
                format: Some(DEFAULT_OBSERVE_FORMAT.to_string()),
                redaction_mode: Some(DEFAULT_REDACTION_MODE.to_string()),
                redaction_text: Some(DEFAULT_REDACTION_TEXT.to_string()),
                tracing: Some(true),
                console: Some(false),
                stats: Some(true),
            }),
        }
    }

    fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            hec: Some(HecDoc {
                addr: env_string(ENV_ADDR),
                token: env_string(ENV_TOKEN).or_else(|| env_string(ENV_SPANK_TOKEN)),
                capture: env_string(ENV_CAPTURE),
            })
            .filter(has_hec_values),
            limits: Some(LimitsDoc {
                max_bytes: env_parse(ENV_MAX_BYTES, "limits.max_bytes")?,
                max_decoded_bytes: env_parse(ENV_MAX_DECODED_BYTES, "limits.max_decoded_bytes")?,
                max_events: env_parse(ENV_MAX_EVENTS, "limits.max_events")?,
                idle_timeout: env_string(ENV_IDLE_TIMEOUT),
                total_timeout: env_string(ENV_TOTAL_TIMEOUT),
                gzip_buffer_bytes: env_parse(ENV_GZIP_BUFFER_BYTES, "limits.gzip_buffer_bytes")?,
            })
            .filter(has_limit_values),
            protocol: Some(ProtocolDoc {
                success: env_parse("HEC_SUCCESS", "protocol.success")?,
                token_required: env_parse("HEC_TOKEN_REQUIRED", "protocol.token_required")?,
                invalid_authorization: env_parse(
                    "HEC_INVALID_AUTHORIZATION",
                    "protocol.invalid_authorization",
                )?,
                invalid_token: env_parse("HEC_INVALID_TOKEN", "protocol.invalid_token")?,
                no_data: env_parse("HEC_NO_DATA", "protocol.no_data")?,
                invalid_data_format: env_parse(
                    "HEC_INVALID_DATA_FORMAT",
                    "protocol.invalid_data_format",
                )?,
                server_busy: env_parse("HEC_SERVER_BUSY", "protocol.server_busy")?,
                event_field_required: env_parse(
                    "HEC_EVENT_FIELD_REQUIRED",
                    "protocol.event_field_required",
                )?,
                event_field_blank: env_parse(
                    "HEC_EVENT_FIELD_BLANK",
                    "protocol.event_field_blank",
                )?,
                handling_indexed_fields: env_parse(
                    "HEC_HANDLING_INDEXED_FIELDS",
                    "protocol.handling_indexed_fields",
                )?,
                health: env_parse("HEC_HEALTH", "protocol.health")?,
            })
            .filter(has_protocol_values),
            observe: Some(ObserveDoc {
                level: env_string(ENV_OBSERVE_LEVEL),
                format: env_string(ENV_OBSERVE_FORMAT),
                redaction_mode: env_string(ENV_OBSERVE_REDACTION_MODE),
                redaction_text: env_string(ENV_OBSERVE_REDACTION_TEXT),
                tracing: env_parse(ENV_OBSERVE_TRACING, "observe.tracing")?,
                console: env_parse(ENV_OBSERVE_CONSOLE, "observe.console")?,
                stats: env_parse(ENV_OBSERVE_STATS, "observe.stats")?,
            })
            .filter(has_observe_values),
        })
    }

    fn from_runtime(config: &RuntimeConfig, redact: bool) -> Self {
        Self {
            hec: Some(HecDoc {
                addr: Some(config.addr.to_string()),
                token: Some(if redact {
                    config.observe.redaction_text.clone()
                } else {
                    config.token.clone()
                }),
                capture: config.capture_path.clone(),
            }),
            limits: Some(LimitsDoc {
                max_bytes: Some(config.limits.max_wire_body_bytes),
                max_decoded_bytes: Some(config.limits.max_decoded_body_bytes),
                max_events: Some(config.limits.max_events_per_request),
                idle_timeout: Some(
                    humantime::format_duration(config.limits.body_idle_timeout).to_string(),
                ),
                total_timeout: Some(
                    humantime::format_duration(config.limits.body_total_timeout).to_string(),
                ),
                gzip_buffer_bytes: Some(config.limits.gzip_buffer_bytes),
            }),
            protocol: Some(ProtocolDoc::from_protocol(&config.protocol)),
            observe: Some(ObserveDoc::from_observe(&config.observe)),
        }
    }

    fn to_runtime(self) -> Result<RuntimeConfig, ConfigError> {
        let hec = self.hec.unwrap_or_default();
        let limits = self.limits.unwrap_or_default();
        let protocol = self.protocol.unwrap_or_default();
        let observe = self.observe.unwrap_or_default();

        let addr_text = required(hec.addr, "hec.addr")?;
        let addr = addr_text
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid {
                field: "hec.addr",
                message: error.to_string(),
            })?;
        if addr.port() == 0 {
            return invalid("hec.addr", "port must be greater than zero");
        }

        let token = required(hec.token, "hec.token")?;
        validate_token(&token)?;
        if matches!(hec.capture.as_deref(), Some("")) {
            return invalid("hec.capture", "capture path cannot be empty");
        }

        let max_bytes = required(limits.max_bytes, "limits.max_bytes")?;
        let max_decoded_bytes = required(limits.max_decoded_bytes, "limits.max_decoded_bytes")?;
        let max_events = required(limits.max_events, "limits.max_events")?;
        let idle_timeout = parse_duration(
            required(limits.idle_timeout, "limits.idle_timeout")?,
            "limits.idle_timeout",
        )?;
        let total_timeout = parse_duration(
            required(limits.total_timeout, "limits.total_timeout")?,
            "limits.total_timeout",
        )?;
        let gzip_buffer_bytes = required(limits.gzip_buffer_bytes, "limits.gzip_buffer_bytes")?;

        if max_bytes == 0 {
            return invalid("limits.max_bytes", "must be greater than zero");
        }
        if max_decoded_bytes < max_bytes {
            return invalid(
                "limits.max_decoded_bytes",
                "must be greater than or equal to limits.max_bytes",
            );
        }
        if max_events == 0 {
            return invalid("limits.max_events", "must be greater than zero");
        }
        if idle_timeout.is_zero() {
            return invalid("limits.idle_timeout", "must be greater than zero");
        }
        if total_timeout < idle_timeout {
            return invalid(
                "limits.total_timeout",
                "must be greater than or equal to limits.idle_timeout",
            );
        }
        if !(MIN_GZIP_BUFFER_BYTES..=MAX_GZIP_BUFFER_BYTES).contains(&gzip_buffer_bytes) {
            return invalid(
                "limits.gzip_buffer_bytes",
                "must be between 512 and 1048576 bytes",
            );
        }
        let observe = observe.to_observe()?;

        Ok(RuntimeConfig {
            addr,
            token,
            capture_path: hec.capture,
            limits: Limits {
                max_content_length: max_bytes,
                max_wire_body_bytes: max_bytes,
                max_decoded_body_bytes: max_decoded_bytes,
                max_events_per_request: max_events,
                body_idle_timeout: idle_timeout,
                body_total_timeout: total_timeout,
                gzip_buffer_bytes,
            },
            protocol: protocol.to_protocol(),
            observe,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HecDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_decoded_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_events: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    idle_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gzip_buffer_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    success: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_required: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invalid_authorization: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invalid_token: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    no_data: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invalid_data_format: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_busy: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_field_required: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_field_blank: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handling_indexed_fields: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    health: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    redaction_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    redaction_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    console: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<bool>,
}

impl ObserveDoc {
    fn from_observe(observe: &ObserveConfig) -> Self {
        Self {
            level: Some(observe.level.clone()),
            format: Some(observe.format.as_str().to_string()),
            redaction_mode: Some(observe.redaction_mode.as_str().to_string()),
            redaction_text: Some(observe.redaction_text.clone()),
            tracing: Some(observe.tracing),
            console: Some(observe.console),
            stats: Some(observe.stats),
        }
    }

    fn to_observe(self) -> Result<ObserveConfig, ConfigError> {
        let level = required(self.level, "observe.level")?;
        validate_observe_level(&level)?;
        let format = match required(self.format, "observe.format")?.as_str() {
            "compact" => ObserveFormat::Compact,
            "json" => ObserveFormat::Json,
            _ => return invalid("observe.format", "must be one of: compact, json"),
        };
        let redaction_mode = match required(self.redaction_mode, "observe.redaction_mode")?.as_str()
        {
            "redact" => RedactionMode::Redact,
            "passthrough" => RedactionMode::Passthrough,
            _ => {
                return invalid(
                    "observe.redaction_mode",
                    "must be one of: redact, passthrough",
                );
            }
        };
        let redaction_text = required(self.redaction_text, "observe.redaction_text")?;
        if redaction_text.is_empty() {
            return invalid("observe.redaction_text", "cannot be empty");
        }

        Ok(ObserveConfig {
            level,
            format,
            redaction_mode,
            redaction_text,
            tracing: required(self.tracing, "observe.tracing")?,
            console: required(self.console, "observe.console")?,
            stats: required(self.stats, "observe.stats")?,
        })
    }
}

impl ProtocolDoc {
    fn defaults() -> Self {
        Self::from_protocol(&Protocol::default())
    }

    fn from_protocol(protocol: &Protocol) -> Self {
        Self {
            success: Some(protocol.success),
            token_required: Some(protocol.token_required),
            invalid_authorization: Some(protocol.invalid_authorization),
            invalid_token: Some(protocol.invalid_token),
            no_data: Some(protocol.no_data),
            invalid_data_format: Some(protocol.invalid_data_format),
            server_busy: Some(protocol.server_busy),
            event_field_required: Some(protocol.event_field_required),
            event_field_blank: Some(protocol.event_field_blank),
            handling_indexed_fields: Some(protocol.handling_indexed_fields),
            health: Some(protocol.health),
        }
    }

    fn to_protocol(self) -> Protocol {
        Protocol {
            success: self.success.expect("default protocol success"),
            token_required: self
                .token_required
                .expect("default protocol token_required"),
            invalid_authorization: self
                .invalid_authorization
                .expect("default protocol invalid_authorization"),
            invalid_token: self.invalid_token.expect("default protocol invalid_token"),
            no_data: self.no_data.expect("default protocol no_data"),
            invalid_data_format: self
                .invalid_data_format
                .expect("default protocol invalid_data_format"),
            server_busy: self.server_busy.expect("default protocol server_busy"),
            event_field_required: self
                .event_field_required
                .expect("default protocol event_field_required"),
            event_field_blank: self
                .event_field_blank
                .expect("default protocol event_field_blank"),
            handling_indexed_fields: self
                .handling_indexed_fields
                .expect("default protocol handling_indexed_fields"),
            health: self.health.expect("default protocol health"),
        }
    }
}

fn has_hec_values(value: &HecDoc) -> bool {
    value.addr.is_some() || value.token.is_some() || value.capture.is_some()
}

fn has_limit_values(value: &LimitsDoc) -> bool {
    value.max_bytes.is_some()
        || value.max_decoded_bytes.is_some()
        || value.max_events.is_some()
        || value.idle_timeout.is_some()
        || value.total_timeout.is_some()
        || value.gzip_buffer_bytes.is_some()
}

fn has_protocol_values(value: &ProtocolDoc) -> bool {
    value.success.is_some()
        || value.token_required.is_some()
        || value.invalid_authorization.is_some()
        || value.invalid_token.is_some()
        || value.no_data.is_some()
        || value.invalid_data_format.is_some()
        || value.server_busy.is_some()
        || value.event_field_required.is_some()
        || value.event_field_blank.is_some()
        || value.handling_indexed_fields.is_some()
        || value.health.is_some()
}

fn has_observe_values(value: &ObserveDoc) -> bool {
    value.level.is_some()
        || value.format.is_some()
        || value.redaction_mode.is_some()
        || value.redaction_text.is_some()
        || value.tracing.is_some()
        || value.console.is_some()
        || value.stats.is_some()
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError::Invalid {
        field,
        message: "missing required value after applying defaults".to_string(),
    })
}

fn invalid<T>(field: &'static str, message: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError::Invalid {
        field,
        message: message.into(),
    })
}

fn parse_duration(value: String, field: &'static str) -> Result<Duration, ConfigError> {
    humantime::parse_duration(&value).map_err(|error| ConfigError::Invalid {
        field,
        message: error.to_string(),
    })
}

fn validate_token(token: &str) -> Result<(), ConfigError> {
    if token.is_empty() {
        return invalid("hec.token", "token cannot be empty");
    }
    if token.chars().any(|character| character.is_ascii_control()) {
        return invalid("hec.token", "token cannot contain ASCII control characters");
    }
    Ok(())
}

fn validate_observe_level(level: &str) -> Result<(), ConfigError> {
    if level.trim().is_empty() {
        return invalid("observe.level", "cannot be empty");
    }
    tracing_subscriber::filter::Targets::from_str(level)
        .map(|_| ())
        .map_err(|error| ConfigError::Invalid {
            field: "observe.level",
            message: error.to_string(),
        })
}

fn env_string(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

fn env_parse<T>(name: &str, field: &'static str) -> Result<Option<T>, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env_string(name) {
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|error| ConfigError::Invalid {
                field,
                message: format!("environment variable {name} has invalid value: {error}"),
            }),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, ConfigAction, RuntimeConfig};
    use std::{env, sync::Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    const ENV_NAMES: &[&str] = &[
        "HEC_CONFIG",
        "HEC_ADDR",
        "HEC_TOKEN",
        "SPANK_HEC_TOKEN",
        "HEC_CAPTURE",
        "HEC_MAX_BYTES",
        "HEC_MAX_DECODED_BYTES",
        "HEC_MAX_EVENTS",
        "HEC_IDLE_TIMEOUT",
        "HEC_TOTAL_TIMEOUT",
        "HEC_GZIP_BUFFER_BYTES",
        "HEC_OBSERVE_LEVEL",
        "HEC_OBSERVE_FORMAT",
        "HEC_OBSERVE_REDACTION_MODE",
        "HEC_OBSERVE_REDACTION_TEXT",
        "HEC_OBSERVE_TRACING",
        "HEC_OBSERVE_CONSOLE",
        "HEC_OBSERVE_STATS",
        "HEC_SUCCESS",
        "HEC_TOKEN_REQUIRED",
        "HEC_INVALID_AUTHORIZATION",
        "HEC_INVALID_TOKEN",
        "HEC_NO_DATA",
        "HEC_INVALID_DATA_FORMAT",
        "HEC_SERVER_BUSY",
        "HEC_EVENT_FIELD_REQUIRED",
        "HEC_EVENT_FIELD_BLANK",
        "HEC_HANDLING_INDEXED_FIELDS",
        "HEC_HEALTH",
    ];

    #[test]
    fn loads_toml_file_values() {
        let _guard = env_guard();
        let config_path = write_config(
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

[observe]
level = "hec_receiver=debug"
format = "json"
redaction_mode = "redact"
redaction_text = "[hidden]"
tracing = true
console = true
stats = true
"#,
        );

        let loaded = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            ..Cli::default()
        })
        .expect("load config");
        let config = loaded.config;

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
        assert_eq!(config.observe.level, "hec_receiver=debug");
        assert_eq!(config.observe.format, super::ObserveFormat::Json);
        assert_eq!(config.observe.redaction_text, "[hidden]");
        assert!(config.observe.tracing);
        assert!(config.observe.console);
        assert!(config.observe.stats);
    }

    #[test]
    fn cli_overrides_file_and_env_overrides_cli() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "file-token"

[limits]
max_bytes = 1000
max_decoded_bytes = 2000
max_events = 10
"#,
        );
        env::set_var("HEC_TOKEN", "env-token");
        env::set_var("HEC_MAX_EVENTS", "30");
        env::set_var("HEC_OBSERVE_CONSOLE", "true");

        let loaded = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            addr: Some("127.0.0.1:18112".to_string()),
            token: Some("cli-token".to_string()),
            max_events: Some(20),
            ..Cli::default()
        })
        .expect("load config");

        assert_eq!(loaded.config.addr.to_string(), "127.0.0.1:18112");
        assert_eq!(loaded.config.token, "env-token");
        assert_eq!(loaded.config.limits.max_content_length, 1000);
        assert_eq!(loaded.config.limits.max_events_per_request, 30);
        assert!(loaded.config.observe.console);
    }

    #[test]
    fn env_config_path_overrides_cli_config_path() {
        let _guard = env_guard();
        let cli_path = write_config(
            r#"
[hec]
token = "cli-file-token"
"#,
        );
        let env_path = write_config(
            r#"
[hec]
token = "env-file-token"
"#,
        );
        env::set_var("HEC_CONFIG", env_path);

        let loaded = RuntimeConfig::load_with_cli(Cli {
            config: Some(cli_path),
            ..Cli::default()
        })
        .expect("load config");

        assert_eq!(loaded.config.token, "env-file-token");
    }

    #[test]
    fn show_config_action_renders_redacted_token() {
        let _guard = env_guard();
        let loaded = RuntimeConfig::load_with_cli(Cli {
            show_config: true,
            token: Some("secret-token".to_string()),
            ..Cli::default()
        })
        .expect("load config");

        assert_eq!(loaded.action, ConfigAction::ShowConfig);
        let rendered = loaded.config.redacted_toml().expect("render config");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("secret-token"));
    }

    #[test]
    fn show_config_uses_configured_redaction_text() {
        let _guard = env_guard();
        let loaded = RuntimeConfig::load_with_cli(Cli {
            show_config: true,
            token: Some("secret-token".to_string()),
            observe_redaction_text: Some("[secret]".to_string()),
            ..Cli::default()
        })
        .expect("load config");

        let rendered = loaded.config.redacted_toml().expect("render config");
        assert!(rendered.contains("[secret]"));
        assert!(!rendered.contains("secret-token"));
    }

    #[test]
    fn show_config_can_explicitly_pass_through_secret_values() {
        let _guard = env_guard();
        let loaded = RuntimeConfig::load_with_cli(Cli {
            show_config: true,
            token: Some("secret-token".to_string()),
            observe_redaction_mode: Some("passthrough".to_string()),
            ..Cli::default()
        })
        .expect("load config");

        let rendered = loaded.config.redacted_toml().expect("render config");
        assert!(rendered.contains("secret-token"));
    }

    #[test]
    fn check_config_action_validates_without_run() {
        let _guard = env_guard();
        let loaded = RuntimeConfig::load_with_cli(Cli {
            check_config: true,
            ..Cli::default()
        })
        .expect("load config");

        assert_eq!(loaded.action, ConfigAction::CheckConfig);
    }

    #[test]
    fn validation_rejects_decoded_limit_below_wire_limit() {
        let _guard = env_guard();
        let error = RuntimeConfig::load_with_cli(Cli {
            max_bytes: Some(2000),
            max_decoded_bytes: Some(1000),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("limits.max_decoded_bytes"));
    }

    #[test]
    fn validation_rejects_empty_token() {
        let _guard = env_guard();
        let error = RuntimeConfig::load_with_cli(Cli {
            token: Some(String::new()),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("hec.token"));
    }

    #[test]
    fn validation_rejects_invalid_numeric_env_value() {
        let _guard = env_guard();
        env::set_var("HEC_MAX_BYTES", "not-a-number");

        let error = RuntimeConfig::load_with_cli(Cli::default()).expect_err("invalid config");

        assert!(error.to_string().contains("limits.max_bytes"));
        assert!(error.to_string().contains("HEC_MAX_BYTES"));
    }

    fn write_config(contents: &str) -> std::path::PathBuf {
        let file = tempfile::NamedTempFile::new().expect("temporary config file");
        let (_file, path) = file.keep().expect("keep temporary config file");
        std::fs::write(&path, contents).expect("write config");
        path
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = ENV_LOCK.lock().expect("env lock");
        for name in ENV_NAMES {
            env::remove_var(name);
        }
        guard
    }
}
