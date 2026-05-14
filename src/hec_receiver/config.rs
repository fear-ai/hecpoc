use clap::Parser;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, env, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration,
};
use thiserror::Error;

use super::{app::Limits, index::is_valid_index_name, protocol::Protocol, report::ReportOutputs};

const DEFAULT_ADDR: &str = "127.0.0.1:18088";
const DEFAULT_TOKEN_ID: &str = "default";
const DEFAULT_TOKEN: &str = "dev-token";
const DEFAULT_INDEX: &str = "main";
const DEFAULT_MAX_BYTES: usize = 1_000_000;
const DEFAULT_MAX_DECODED_BYTES: usize = 4 * DEFAULT_MAX_BYTES;
const DEFAULT_MAX_EVENTS: usize = 100_000;
const DEFAULT_MAX_INDEX_LEN: usize = 128;
const DEFAULT_IDLE_TIMEOUT: &str = "5s";
const DEFAULT_TOTAL_TIMEOUT: &str = "30s";
const DEFAULT_GZIP_BUFFER_BYTES: usize = 8_192;
const MIN_GZIP_BUFFER_BYTES: usize = 512;
const MAX_GZIP_BUFFER_BYTES: usize = 1_048_576;

const ENV_CONFIG: &str = "HEC_CONFIG";
const ENV_ADDR: &str = "HEC_ADDR";
const ENV_TOKEN_ID: &str = "HEC_TOKEN_ID";
const ENV_TOKEN: &str = "HEC_TOKEN";
const ENV_SPANK_TOKEN: &str = "SPANK_HEC_TOKEN";
const ENV_TOKEN_ENABLED: &str = "HEC_TOKEN_ENABLED";
const ENV_TOKEN_ACK_ENABLED: &str = "HEC_TOKEN_ACK_ENABLED";
const ENV_DEFAULT_INDEX: &str = "HEC_DEFAULT_INDEX";
const ENV_ALLOWED_INDEXES: &str = "HEC_ALLOWED_INDEXES";
const ENV_CAPTURE: &str = "HEC_CAPTURE";
const ENV_MAX_BYTES: &str = "HEC_MAX_BYTES";
const ENV_MAX_DECODED_BYTES: &str = "HEC_MAX_DECODED_BYTES";
const ENV_MAX_EVENTS: &str = "HEC_MAX_EVENTS";
const ENV_MAX_INDEX_LEN: &str = "HEC_MAX_INDEX_LEN";
const ENV_IDLE_TIMEOUT: &str = "HEC_IDLE_TIMEOUT";
const ENV_TOTAL_TIMEOUT: &str = "HEC_TOTAL_TIMEOUT";
const ENV_GZIP_BUFFER_BYTES: &str = "HEC_GZIP_BUFFER_BYTES";
const ENV_OBSERVE_LEVEL: &str = "HEC_OBSERVE_LEVEL";
const ENV_OBSERVE_SOURCES: &str = "HEC_OBSERVE_SOURCES";
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
    pub tokens: Vec<RuntimeToken>,
    pub token_id: String,
    pub token: String,
    pub token_enabled: bool,
    pub token_ack_enabled: bool,
    pub default_index: Option<String>,
    pub allowed_indexes: Vec<String>,
    pub capture_path: Option<String>,
    pub limits: Limits,
    pub protocol: Protocol,
    pub observe: ObserveConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeToken {
    pub id: String,
    pub secret: String,
    pub enabled: bool,
    pub ack_enabled: bool,
    pub default_index: Option<String>,
    pub allowed_indexes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ObserveConfig {
    pub level: String,
    pub sources: BTreeMap<String, String>,
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

    pub fn filter_directives(&self) -> String {
        let mut parts = vec![self.level.clone()];
        parts.extend(
            self.sources
                .iter()
                .map(|(source, level)| format!("{source}={level}")),
        );
        parts.join(",")
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveSourceOverride {
    source: String,
    level: String,
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
    pub token_id: Option<String>,

    #[arg(long)]
    pub token: Option<String>,

    #[arg(long)]
    pub token_enabled: Option<bool>,

    #[arg(long)]
    pub token_ack_enabled: Option<bool>,

    #[arg(long)]
    pub default_index: Option<String>,

    #[arg(long, value_delimiter = ',')]
    pub allowed_indexes: Option<Vec<String>>,

    #[arg(long)]
    pub capture: Option<String>,

    #[arg(long)]
    pub max_bytes: Option<usize>,

    #[arg(long)]
    pub max_decoded_bytes: Option<usize>,

    #[arg(long)]
    pub max_events: Option<usize>,

    #[arg(long)]
    pub max_index_len: Option<usize>,

    #[arg(long)]
    pub idle_timeout: Option<String>,

    #[arg(long)]
    pub total_timeout: Option<String>,

    #[arg(long)]
    pub gzip_buffer_bytes: Option<usize>,

    #[arg(long)]
    pub protocol_success: Option<u16>,

    #[arg(long)]
    pub protocol_token_disabled: Option<u16>,

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
    pub protocol_incorrect_index: Option<u16>,

    #[arg(long)]
    pub protocol_server_busy: Option<u16>,

    #[arg(long)]
    pub protocol_event_field_required: Option<u16>,

    #[arg(long)]
    pub protocol_event_field_blank: Option<u16>,

    #[arg(long)]
    pub protocol_ack_disabled: Option<u16>,

    #[arg(long)]
    pub protocol_handling_indexed_fields: Option<u16>,

    #[arg(long)]
    pub protocol_query_string_authorization_disabled: Option<u16>,

    #[arg(long)]
    pub protocol_health_ok: Option<u16>,

    #[arg(long)]
    pub protocol_health_unhealthy: Option<u16>,

    #[arg(long)]
    pub protocol_server_shutting_down: Option<u16>,

    #[arg(long)]
    pub observe_level: Option<String>,

    #[arg(long = "observe-source", value_parser = parse_observe_source_cli)]
    pub observe_sources: Option<Vec<ObserveSourceOverride>>,

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
                token_id: self.token_id.clone(),
                token: self.token.clone(),
                token_enabled: self.token_enabled,
                token_ack_enabled: self.token_ack_enabled,
                default_index: self.default_index.clone(),
                allowed_indexes: self.allowed_indexes.clone(),
                capture: self.capture.clone(),
                tokens: None,
            })
            .filter(has_hec_values),
            limits: Some(LimitsDoc {
                max_bytes: self.max_bytes,
                max_decoded_bytes: self.max_decoded_bytes,
                max_events: self.max_events,
                max_index_len: self.max_index_len,
                idle_timeout: self.idle_timeout.clone(),
                total_timeout: self.total_timeout.clone(),
                gzip_buffer_bytes: self.gzip_buffer_bytes,
            })
            .filter(has_limit_values),
            protocol: Some(ProtocolDoc {
                success: self.protocol_success,
                token_disabled: self.protocol_token_disabled,
                token_required: self.protocol_token_required,
                invalid_authorization: self.protocol_invalid_authorization,
                invalid_token: self.protocol_invalid_token,
                no_data: self.protocol_no_data,
                invalid_data_format: self.protocol_invalid_data_format,
                incorrect_index: self.protocol_incorrect_index,
                server_busy: self.protocol_server_busy,
                event_field_required: self.protocol_event_field_required,
                event_field_blank: self.protocol_event_field_blank,
                ack_disabled: self.protocol_ack_disabled,
                handling_indexed_fields: self.protocol_handling_indexed_fields,
                query_string_authorization_disabled: self
                    .protocol_query_string_authorization_disabled,
                health_ok: self.protocol_health_ok,
                health_unhealthy: self.protocol_health_unhealthy,
                server_shutting_down: self.protocol_server_shutting_down,
            })
            .filter(has_protocol_values),
            observe: Some(ObserveDoc {
                level: self.observe_level.clone(),
                sources: self.observe_sources.as_ref().map(|sources| {
                    sources
                        .iter()
                        .map(|source| (source.source.clone(), source.level.clone()))
                        .collect()
                }),
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
                token_id: Some(DEFAULT_TOKEN_ID.to_string()),
                token: Some(DEFAULT_TOKEN.to_string()),
                token_enabled: Some(true),
                token_ack_enabled: Some(false),
                default_index: Some(DEFAULT_INDEX.to_string()),
                allowed_indexes: Some(vec![DEFAULT_INDEX.to_string()]),
                capture: None,
                tokens: None,
            }),
            limits: Some(LimitsDoc {
                max_bytes: Some(DEFAULT_MAX_BYTES),
                max_decoded_bytes: Some(DEFAULT_MAX_DECODED_BYTES),
                max_events: Some(DEFAULT_MAX_EVENTS),
                max_index_len: Some(DEFAULT_MAX_INDEX_LEN),
                idle_timeout: Some(DEFAULT_IDLE_TIMEOUT.to_string()),
                total_timeout: Some(DEFAULT_TOTAL_TIMEOUT.to_string()),
                gzip_buffer_bytes: Some(DEFAULT_GZIP_BUFFER_BYTES),
            }),
            protocol: Some(ProtocolDoc::defaults()),
            observe: Some(ObserveDoc {
                level: Some(DEFAULT_OBSERVE_LEVEL.to_string()),
                sources: Some(BTreeMap::from([
                    ("hec.receiver".to_string(), "info".to_string()),
                    ("hec.auth".to_string(), "warn".to_string()),
                    ("hec.body".to_string(), "warn".to_string()),
                    ("hec.parser".to_string(), "warn".to_string()),
                    ("hec.sink".to_string(), "warn".to_string()),
                ])),
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
                token_id: env_string(ENV_TOKEN_ID),
                token: env_string(ENV_TOKEN).or_else(|| env_string(ENV_SPANK_TOKEN)),
                token_enabled: env_parse(ENV_TOKEN_ENABLED, "hec.token_enabled")?,
                token_ack_enabled: env_parse(ENV_TOKEN_ACK_ENABLED, "hec.token_ack_enabled")?,
                default_index: env_string(ENV_DEFAULT_INDEX),
                allowed_indexes: env_list(ENV_ALLOWED_INDEXES),
                capture: env_string(ENV_CAPTURE),
                tokens: None,
            })
            .filter(has_hec_values),
            limits: Some(LimitsDoc {
                max_bytes: env_parse(ENV_MAX_BYTES, "limits.max_bytes")?,
                max_decoded_bytes: env_parse(ENV_MAX_DECODED_BYTES, "limits.max_decoded_bytes")?,
                max_events: env_parse(ENV_MAX_EVENTS, "limits.max_events")?,
                max_index_len: env_parse(ENV_MAX_INDEX_LEN, "limits.max_index_len")?,
                idle_timeout: env_string(ENV_IDLE_TIMEOUT),
                total_timeout: env_string(ENV_TOTAL_TIMEOUT),
                gzip_buffer_bytes: env_parse(ENV_GZIP_BUFFER_BYTES, "limits.gzip_buffer_bytes")?,
            })
            .filter(has_limit_values),
            protocol: Some(ProtocolDoc {
                success: env_parse("HEC_SUCCESS", "protocol.success")?,
                token_disabled: env_parse("HEC_TOKEN_DISABLED", "protocol.token_disabled")?,
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
                incorrect_index: env_parse("HEC_INCORRECT_INDEX", "protocol.incorrect_index")?,
                server_busy: env_parse("HEC_SERVER_BUSY", "protocol.server_busy")?,
                event_field_required: env_parse(
                    "HEC_EVENT_FIELD_REQUIRED",
                    "protocol.event_field_required",
                )?,
                event_field_blank: env_parse(
                    "HEC_EVENT_FIELD_BLANK",
                    "protocol.event_field_blank",
                )?,
                ack_disabled: env_parse("HEC_ACK_DISABLED", "protocol.ack_disabled")?,
                handling_indexed_fields: env_parse(
                    "HEC_HANDLING_INDEXED_FIELDS",
                    "protocol.handling_indexed_fields",
                )?,
                query_string_authorization_disabled: env_parse(
                    "HEC_QUERY_STRING_AUTHORIZATION_DISABLED",
                    "protocol.query_string_authorization_disabled",
                )?,
                health_ok: env_parse("HEC_HEALTH_OK", "protocol.health_ok")?,
                health_unhealthy: env_parse("HEC_HEALTH_UNHEALTHY", "protocol.health_unhealthy")?,
                server_shutting_down: env_parse(
                    "HEC_SERVER_SHUTTING_DOWN",
                    "protocol.server_shutting_down",
                )?,
            })
            .filter(has_protocol_values),
            observe: Some(ObserveDoc {
                level: env_string(ENV_OBSERVE_LEVEL),
                sources: env_sources(ENV_OBSERVE_SOURCES)?,
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
                token_id: Some(config.token_id.clone()),
                token: Some(if redact {
                    config.observe.redaction_text.clone()
                } else {
                    config.token.clone()
                }),
                token_enabled: Some(config.token_enabled),
                token_ack_enabled: Some(config.token_ack_enabled),
                default_index: config.default_index.clone(),
                allowed_indexes: Some(config.allowed_indexes.clone()),
                capture: config.capture_path.clone(),
                tokens: if config.tokens.len() > 1 {
                    Some(
                        config
                            .tokens
                            .iter()
                            .map(|token| HecTokenDoc {
                                id: Some(token.id.clone()),
                                secret: Some(if redact {
                                    config.observe.redaction_text.clone()
                                } else {
                                    token.secret.clone()
                                }),
                                enabled: Some(token.enabled),
                                ack_enabled: Some(token.ack_enabled),
                                default_index: token.default_index.clone(),
                                allowed_indexes: Some(token.allowed_indexes.clone()),
                            })
                            .collect(),
                    )
                } else {
                    None
                },
            }),
            limits: Some(LimitsDoc {
                max_bytes: Some(config.limits.max_http_body_bytes),
                max_decoded_bytes: Some(config.limits.max_decoded_body_bytes),
                max_events: Some(config.limits.max_events_per_request),
                max_index_len: Some(config.limits.max_index_len),
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

        let addr_text = required(hec.addr.clone(), "hec.addr")?;
        let addr = addr_text
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::Invalid {
                field: "hec.addr",
                message: error.to_string(),
            })?;
        if addr.port() == 0 {
            return invalid("hec.addr", "port must be greater than zero");
        }

        let token = required(hec.token.clone(), "hec.token")?;
        let token_id = required(hec.token_id.clone(), "hec.token_id")?;
        if token_id.trim().is_empty() {
            return invalid("hec.token_id", "token id cannot be empty");
        }
        let token_enabled = required(hec.token_enabled, "hec.token_enabled")?;
        let token_ack_enabled = required(hec.token_ack_enabled, "hec.token_ack_enabled")?;
        validate_token(&token)?;
        if matches!(hec.default_index.as_deref(), Some("")) {
            return invalid("hec.default_index", "default index cannot be empty");
        }
        if matches!(hec.capture.as_deref(), Some("")) {
            return invalid("hec.capture", "capture path cannot be empty");
        }

        let max_bytes = required(limits.max_bytes, "limits.max_bytes")?;
        let max_decoded_bytes = required(limits.max_decoded_bytes, "limits.max_decoded_bytes")?;
        let max_events = required(limits.max_events, "limits.max_events")?;
        let max_index_len = required(limits.max_index_len, "limits.max_index_len")?;
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
        if max_index_len == 0 {
            return invalid("limits.max_index_len", "must be greater than zero");
        }
        if let Some(default_index) = hec.default_index.as_deref() {
            if !is_valid_index_name(default_index, max_index_len) {
                return invalid(
                    "hec.default_index",
                    "must use lowercase ASCII letters, digits, underscore, or dash; cannot start with '_' or '-'; cannot contain 'kvstore'; cannot exceed limits.max_index_len",
                );
            }
        }
        let allowed_indexes = hec.allowed_indexes.clone().unwrap_or_default();
        for allowed_index in &allowed_indexes {
            if allowed_index.is_empty() {
                return invalid("hec.allowed_indexes", "index names cannot be empty");
            }
            if !is_valid_index_name(allowed_index, max_index_len) {
                return invalid(
                    "hec.allowed_indexes",
                    "each index must use lowercase ASCII letters, digits, underscore, or dash; cannot start with '_' or '-'; cannot contain 'kvstore'; cannot exceed limits.max_index_len",
                );
            }
        }
        if let Some(default_index) = hec.default_index.as_deref() {
            if !allowed_indexes.is_empty()
                && !allowed_indexes
                    .iter()
                    .any(|allowed| allowed == default_index)
            {
                return invalid(
                    "hec.default_index",
                    "default index must be listed in hec.allowed_indexes when an allow-list is configured",
                );
            }
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
        let tokens = build_runtime_tokens(&hec, max_index_len)?;

        Ok(RuntimeConfig {
            addr,
            tokens,
            token_id,
            token,
            token_enabled,
            token_ack_enabled,
            default_index: hec.default_index.clone(),
            allowed_indexes,
            capture_path: hec.capture.clone(),
            limits: Limits {
                max_content_length: max_bytes,
                max_http_body_bytes: max_bytes,
                max_decoded_body_bytes: max_decoded_bytes,
                max_events_per_request: max_events,
                max_index_len,
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
    token_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_ack_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_indexes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<Vec<HecTokenDoc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HecTokenDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_indexes: Option<Vec<String>>,
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
    max_index_len: Option<usize>,
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
    token_disabled: Option<u16>,
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
    incorrect_index: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_busy: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_field_required: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_field_blank: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_disabled: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handling_indexed_fields: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_string_authorization_disabled: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    health_ok: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    health_unhealthy: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_shutting_down: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveDoc {
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sources: Option<BTreeMap<String, String>>,
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
            sources: Some(observe.sources.clone()),
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
        let sources = self.sources.unwrap_or_default();
        validate_observe_level(&level, &sources)?;
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
            sources,
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
            token_disabled: Some(protocol.token_disabled),
            token_required: Some(protocol.token_required),
            invalid_authorization: Some(protocol.invalid_authorization),
            invalid_token: Some(protocol.invalid_token),
            no_data: Some(protocol.no_data),
            invalid_data_format: Some(protocol.invalid_data_format),
            incorrect_index: Some(protocol.incorrect_index),
            server_busy: Some(protocol.server_busy),
            event_field_required: Some(protocol.event_field_required),
            event_field_blank: Some(protocol.event_field_blank),
            ack_disabled: Some(protocol.ack_disabled),
            handling_indexed_fields: Some(protocol.handling_indexed_fields),
            query_string_authorization_disabled: Some(protocol.query_string_authorization_disabled),
            health_ok: Some(protocol.health_ok),
            health_unhealthy: Some(protocol.health_unhealthy),
            server_shutting_down: Some(protocol.server_shutting_down),
        }
    }

    fn to_protocol(self) -> Protocol {
        Protocol {
            success: self.success.expect("default protocol success"),
            token_disabled: self
                .token_disabled
                .expect("default protocol token_disabled"),
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
            incorrect_index: self
                .incorrect_index
                .expect("default protocol incorrect_index"),
            server_busy: self.server_busy.expect("default protocol server_busy"),
            event_field_required: self
                .event_field_required
                .expect("default protocol event_field_required"),
            event_field_blank: self
                .event_field_blank
                .expect("default protocol event_field_blank"),
            ack_disabled: self.ack_disabled.expect("default protocol ack_disabled"),
            handling_indexed_fields: self
                .handling_indexed_fields
                .expect("default protocol handling_indexed_fields"),
            query_string_authorization_disabled: self
                .query_string_authorization_disabled
                .expect("default protocol query_string_authorization_disabled"),
            health_ok: self.health_ok.expect("default protocol health_ok"),
            health_unhealthy: self
                .health_unhealthy
                .expect("default protocol health_unhealthy"),
            server_shutting_down: self
                .server_shutting_down
                .expect("default protocol server_shutting_down"),
        }
    }
}

fn has_hec_values(value: &HecDoc) -> bool {
    value.addr.is_some()
        || value.token_id.is_some()
        || value.token.is_some()
        || value.token_enabled.is_some()
        || value.token_ack_enabled.is_some()
        || value.default_index.is_some()
        || value.allowed_indexes.is_some()
        || value.capture.is_some()
        || value.tokens.is_some()
}

fn has_limit_values(value: &LimitsDoc) -> bool {
    value.max_bytes.is_some()
        || value.max_decoded_bytes.is_some()
        || value.max_events.is_some()
        || value.max_index_len.is_some()
        || value.idle_timeout.is_some()
        || value.total_timeout.is_some()
        || value.gzip_buffer_bytes.is_some()
}

fn has_protocol_values(value: &ProtocolDoc) -> bool {
    value.success.is_some()
        || value.token_disabled.is_some()
        || value.token_required.is_some()
        || value.invalid_authorization.is_some()
        || value.invalid_token.is_some()
        || value.no_data.is_some()
        || value.invalid_data_format.is_some()
        || value.incorrect_index.is_some()
        || value.server_busy.is_some()
        || value.event_field_required.is_some()
        || value.event_field_blank.is_some()
        || value.ack_disabled.is_some()
        || value.handling_indexed_fields.is_some()
        || value.query_string_authorization_disabled.is_some()
        || value.health_ok.is_some()
        || value.health_unhealthy.is_some()
        || value.server_shutting_down.is_some()
}

fn has_observe_values(value: &ObserveDoc) -> bool {
    value.level.is_some()
        || value.sources.is_some()
        || value.format.is_some()
        || value.redaction_mode.is_some()
        || value.redaction_text.is_some()
        || value.tracing.is_some()
        || value.console.is_some()
        || value.stats.is_some()
}

fn build_runtime_tokens(
    hec: &HecDoc,
    max_index_len: usize,
) -> Result<Vec<RuntimeToken>, ConfigError> {
    let tokens = match hec.tokens.as_ref() {
        Some(tokens) if !tokens.is_empty() => tokens
            .iter()
            .enumerate()
            .map(|(index, token)| {
                let field = "hec.tokens";
                let id = required(token.id.clone(), "hec.tokens.id")?;
                if id.trim().is_empty() {
                    return invalid("hec.tokens.id", "token id cannot be empty");
                }
                let secret = required(token.secret.clone(), "hec.tokens.secret")?;
                validate_token_field(&secret, "hec.tokens.secret")?;
                let enabled = token.enabled.unwrap_or(true);
                let ack_enabled = token.ack_enabled.unwrap_or(false);
                validate_optional_index(
                    token.default_index.as_deref(),
                    max_index_len,
                    "hec.tokens.default_index",
                )?;
                let allowed_indexes = token.allowed_indexes.clone().unwrap_or_default();
                validate_allowed_indexes(
                    &allowed_indexes,
                    max_index_len,
                    "hec.tokens.allowed_indexes",
                )?;
                validate_default_in_allowed(
                    token.default_index.as_deref(),
                    &allowed_indexes,
                    "hec.tokens.default_index",
                )?;
                Ok(RuntimeToken {
                    id,
                    secret,
                    enabled,
                    ack_enabled,
                    default_index: token.default_index.clone(),
                    allowed_indexes,
                })
                .map_err(|error: ConfigError| match error {
                    ConfigError::Invalid { field: _, message } => ConfigError::Invalid {
                        field,
                        message: format!("entry {index}: {message}"),
                    },
                    other => other,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![RuntimeToken {
            id: required(hec.token_id.clone(), "hec.token_id")?,
            secret: required(hec.token.clone(), "hec.token")?,
            enabled: required(hec.token_enabled, "hec.token_enabled")?,
            ack_enabled: required(hec.token_ack_enabled, "hec.token_ack_enabled")?,
            default_index: hec.default_index.clone(),
            allowed_indexes: hec.allowed_indexes.clone().unwrap_or_default(),
        }],
    };

    if tokens.is_empty() {
        return invalid("hec.tokens", "at least one token is required");
    }

    for (index, token) in tokens.iter().enumerate() {
        if token.id.trim().is_empty() {
            return invalid(
                "hec.tokens",
                format!("entry {index}: token id cannot be empty"),
            );
        }
        validate_token_field(&token.secret, "hec.tokens")?;
    }

    for left in 0..tokens.len() {
        for right in (left + 1)..tokens.len() {
            if tokens[left].id == tokens[right].id {
                return invalid("hec.tokens", "token ids must be unique");
            }
            if tokens[left].secret == tokens[right].secret {
                return invalid("hec.tokens", "token secrets must be unique");
            }
        }
    }

    Ok(tokens)
}

fn validate_optional_index(
    index: Option<&str>,
    max_index_len: usize,
    field: &'static str,
) -> Result<(), ConfigError> {
    if matches!(index, Some("")) {
        return invalid(field, "default index cannot be empty");
    }
    if let Some(index) = index {
        if !is_valid_index_name(index, max_index_len) {
            return invalid(
                field,
                "must use lowercase ASCII letters, digits, underscore, or dash; cannot start with '_' or '-'; cannot contain 'kvstore'; cannot exceed limits.max_index_len",
            );
        }
    }
    Ok(())
}

fn validate_allowed_indexes(
    allowed_indexes: &[String],
    max_index_len: usize,
    field: &'static str,
) -> Result<(), ConfigError> {
    for allowed_index in allowed_indexes {
        if allowed_index.is_empty() {
            return invalid(field, "index names cannot be empty");
        }
        if !is_valid_index_name(allowed_index, max_index_len) {
            return invalid(
                field,
                "each index must use lowercase ASCII letters, digits, underscore, or dash; cannot start with '_' or '-'; cannot contain 'kvstore'; cannot exceed limits.max_index_len",
            );
        }
    }
    Ok(())
}

fn validate_default_in_allowed(
    default_index: Option<&str>,
    allowed_indexes: &[String],
    field: &'static str,
) -> Result<(), ConfigError> {
    if let Some(default_index) = default_index {
        if !allowed_indexes.is_empty()
            && !allowed_indexes
                .iter()
                .any(|allowed| allowed == default_index)
        {
            return invalid(
                field,
                "default index must be listed in allowed indexes when an allow-list is configured",
            );
        }
    }
    Ok(())
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
    validate_token_field(token, "hec.token")
}

fn validate_token_field(token: &str, field: &'static str) -> Result<(), ConfigError> {
    if token.is_empty() {
        return invalid(field, "token cannot be empty");
    }
    if token.chars().any(|character| character.is_ascii_control()) {
        return invalid(field, "token cannot contain ASCII control characters");
    }
    Ok(())
}

fn validate_observe_level(
    level: &str,
    sources: &BTreeMap<String, String>,
) -> Result<(), ConfigError> {
    if level.trim().is_empty() {
        return invalid("observe.level", "cannot be empty");
    }
    for (source, source_level) in sources {
        if source.trim().is_empty() {
            return invalid("observe.sources", "source name cannot be empty");
        }
        if source_level.trim().is_empty() {
            return invalid("observe.sources", "source level cannot be empty");
        }
    }
    let mut directives = vec![level.to_string()];
    directives.extend(
        sources
            .iter()
            .map(|(source, source_level)| format!("{source}={source_level}")),
    );
    tracing_subscriber::filter::Targets::from_str(&directives.join(","))
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

fn env_list(name: &str) -> Option<Vec<String>> {
    env_string(name).map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
}

fn env_sources(name: &str) -> Result<Option<BTreeMap<String, String>>, ConfigError> {
    env_list(name)
        .map(|values| {
            values
                .iter()
                .map(|value| parse_source_override(value))
                .collect::<Result<BTreeMap<_, _>, _>>()
        })
        .transpose()
}

fn parse_observe_source_cli(value: &str) -> Result<ObserveSourceOverride, String> {
    parse_source_override(value)
        .map(|(source, level)| ObserveSourceOverride { source, level })
        .map_err(|error| error.to_string())
}

fn parse_source_override(value: &str) -> Result<(String, String), ConfigError> {
    let (source, level) = value.split_once('=').ok_or_else(|| ConfigError::Invalid {
        field: "observe.sources",
        message: "source overrides must use source=level".to_string(),
    })?;
    let source = source.trim();
    let level = level.trim();
    if source.is_empty() || level.is_empty() {
        return invalid("observe.sources", "source and level cannot be empty");
    }
    Ok((source.to_string(), level.to_string()))
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
        "HEC_TOKEN_ID",
        "HEC_TOKEN",
        "SPANK_HEC_TOKEN",
        "HEC_TOKEN_ENABLED",
        "HEC_TOKEN_ACK_ENABLED",
        "HEC_DEFAULT_INDEX",
        "HEC_ALLOWED_INDEXES",
        "HEC_CAPTURE",
        "HEC_MAX_BYTES",
        "HEC_MAX_DECODED_BYTES",
        "HEC_MAX_EVENTS",
        "HEC_MAX_INDEX_LEN",
        "HEC_IDLE_TIMEOUT",
        "HEC_TOTAL_TIMEOUT",
        "HEC_GZIP_BUFFER_BYTES",
        "HEC_OBSERVE_LEVEL",
        "HEC_OBSERVE_SOURCES",
        "HEC_OBSERVE_FORMAT",
        "HEC_OBSERVE_REDACTION_MODE",
        "HEC_OBSERVE_REDACTION_TEXT",
        "HEC_OBSERVE_TRACING",
        "HEC_OBSERVE_CONSOLE",
        "HEC_OBSERVE_STATS",
        "HEC_SUCCESS",
        "HEC_TOKEN_DISABLED",
        "HEC_TOKEN_REQUIRED",
        "HEC_INVALID_AUTHORIZATION",
        "HEC_INVALID_TOKEN",
        "HEC_NO_DATA",
        "HEC_INVALID_DATA_FORMAT",
        "HEC_INCORRECT_INDEX",
        "HEC_SERVER_BUSY",
        "HEC_EVENT_FIELD_REQUIRED",
        "HEC_EVENT_FIELD_BLANK",
        "HEC_ACK_DISABLED",
        "HEC_HANDLING_INDEXED_FIELDS",
        "HEC_QUERY_STRING_AUTHORIZATION_DISABLED",
        "HEC_HEALTH_OK",
        "HEC_HEALTH_UNHEALTHY",
        "HEC_SERVER_SHUTTING_DOWN",
    ];

    #[test]
    fn loads_toml_file_values() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token_id = "file-token-id"
token = "file-token"
token_enabled = true
token_ack_enabled = false
default_index = "app_logs"
allowed_indexes = ["app_logs", "audit_logs"]
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
level = "debug"
sources = { "hec.auth" = "debug", "hec.body" = "info" }
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
        assert_eq!(config.token_id, "file-token-id");
        assert_eq!(config.token, "file-token");
        assert!(config.token_enabled);
        assert!(!config.token_ack_enabled);
        assert_eq!(config.default_index.as_deref(), Some("app_logs"));
        assert_eq!(config.allowed_indexes, vec!["app_logs", "audit_logs"]);
        assert_eq!(
            config.capture_path.as_deref(),
            Some("/tmp/hec-events.jsonl")
        );
        assert_eq!(config.limits.max_content_length, 12345);
        assert_eq!(config.limits.max_http_body_bytes, 12345);
        assert_eq!(config.limits.max_decoded_body_bytes, 23456);
        assert_eq!(config.limits.max_events_per_request, 345);
        assert_eq!(config.limits.body_idle_timeout.as_millis(), 250);
        assert_eq!(config.limits.body_total_timeout.as_secs(), 5);
        assert_eq!(config.limits.gzip_buffer_bytes, 4096);
        assert_eq!(config.protocol.token_required, 202);
        assert_eq!(config.protocol.invalid_token, 204);
        assert_eq!(config.observe.level, "debug");
        assert_eq!(
            config.observe.sources.get("hec.auth").map(String::as_str),
            Some("debug")
        );
        assert_eq!(
            config.observe.filter_directives(),
            "debug,hec.auth=debug,hec.body=info,hec.parser=warn,hec.receiver=info,hec.sink=warn"
        );
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
        env::set_var("HEC_TOKEN_ID", "env-token-id");
        env::set_var("HEC_TOKEN_ENABLED", "false");
        env::set_var("HEC_TOKEN_ACK_ENABLED", "true");
        env::set_var("HEC_DEFAULT_INDEX", "env_index");
        env::set_var("HEC_ALLOWED_INDEXES", "env_index,other_index");
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
        assert_eq!(loaded.config.token_id, "env-token-id");
        assert_eq!(loaded.config.token, "env-token");
        assert!(!loaded.config.token_enabled);
        assert!(loaded.config.token_ack_enabled);
        assert_eq!(loaded.config.default_index.as_deref(), Some("env_index"));
        assert_eq!(
            loaded.config.allowed_indexes,
            vec!["env_index", "other_index"]
        );
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
    fn validation_rejects_decoded_limit_below_http_body_limit() {
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
    fn validation_rejects_zero_index_length() {
        let _guard = env_guard();
        let error = RuntimeConfig::load_with_cli(Cli {
            max_index_len: Some(0),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("limits.max_index_len"));
    }

    #[test]
    fn validation_rejects_invalid_default_index() {
        let _guard = env_guard();
        let error = RuntimeConfig::load_with_cli(Cli {
            default_index: Some("Bad.Index".to_string()),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("hec.default_index"));
    }

    #[test]
    fn defaults_to_main_index_and_allowed_index() {
        let _guard = env_guard();
        let loaded = RuntimeConfig::load_with_cli(Cli::default()).expect("load config");

        assert_eq!(loaded.config.default_index.as_deref(), Some("main"));
        assert_eq!(loaded.config.allowed_indexes, vec!["main"]);
        assert_eq!(loaded.config.token_id, "default");
        assert!(loaded.config.token_enabled);
        assert!(!loaded.config.token_ack_enabled);
        assert_eq!(loaded.config.tokens.len(), 1);
        assert_eq!(loaded.config.tokens[0].id, "default");
    }

    #[test]
    fn loads_multiple_token_records_from_toml() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "legacy-token"
default_index = "main"
allowed_indexes = ["main"]

[[hec.tokens]]
id = "main-token"
secret = "main-secret"
enabled = true
ack_enabled = false
default_index = "main"
allowed_indexes = ["main"]

[[hec.tokens]]
id = "audit-token"
secret = "audit-secret"
enabled = false
ack_enabled = true
default_index = "audit"
allowed_indexes = ["audit", "main"]
"#,
        );

        let loaded = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            ..Cli::default()
        })
        .expect("load config");

        assert_eq!(loaded.config.tokens.len(), 2);
        assert_eq!(loaded.config.tokens[0].id, "main-token");
        assert_eq!(loaded.config.tokens[0].secret, "main-secret");
        assert_eq!(
            loaded.config.tokens[0].default_index.as_deref(),
            Some("main")
        );
        assert_eq!(loaded.config.tokens[1].id, "audit-token");
        assert_eq!(loaded.config.tokens[1].secret, "audit-secret");
        assert!(!loaded.config.tokens[1].enabled);
        assert!(loaded.config.tokens[1].ack_enabled);
        assert_eq!(
            loaded.config.tokens[1].allowed_indexes,
            vec!["audit", "main"]
        );
    }

    #[test]
    fn rejects_duplicate_token_ids() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "legacy-token"

[[hec.tokens]]
id = "dup"
secret = "one"

[[hec.tokens]]
id = "dup"
secret = "two"
"#,
        );

        let error = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("token ids must be unique"));
    }

    #[test]
    fn rejects_duplicate_token_secrets() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "legacy-token"

[[hec.tokens]]
id = "one"
secret = "same"

[[hec.tokens]]
id = "two"
secret = "same"
"#,
        );

        let error = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("token secrets must be unique"));
    }

    #[test]
    fn rejects_token_default_index_outside_token_allowed_indexes() {
        let _guard = env_guard();
        let config_path = write_config(
            r#"
[hec]
addr = "127.0.0.1:18111"
token = "legacy-token"

[[hec.tokens]]
id = "bad-index"
secret = "secret"
default_index = "audit"
allowed_indexes = ["main"]
"#,
        );

        let error = RuntimeConfig::load_with_cli(Cli {
            config: Some(config_path),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("default index must be listed"));
    }

    #[test]
    fn env_observe_sources_compose_with_global_level() {
        let _guard = env_guard();
        env::set_var("HEC_OBSERVE_LEVEL", "info");
        env::set_var("HEC_OBSERVE_SOURCES", "hec.auth=debug,hec.body=trace");

        let loaded = RuntimeConfig::load_with_cli(Cli::default()).expect("load config");

        assert_eq!(
            loaded.config.observe.filter_directives(),
            "info,hec.auth=debug,hec.body=trace,hec.parser=warn,hec.receiver=info,hec.sink=warn"
        );
    }

    #[test]
    fn validation_rejects_default_index_outside_allowed_indexes() {
        let _guard = env_guard();
        let error = RuntimeConfig::load_with_cli(Cli {
            default_index: Some("other".to_string()),
            allowed_indexes: Some(vec!["main".to_string()]),
            ..Cli::default()
        })
        .expect_err("invalid config");

        assert!(error.to_string().contains("hec.default_index"));
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
