use serde_json::{Map, Value};
use std::{
    fmt,
    io::{self, Write},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use super::{
    event::Endpoint,
    stats::{Counter, Stats},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactId(pub u16);

#[derive(Debug)]
pub struct FactSpec {
    pub id: FactId,
    pub name: &'static str,
    pub phase: Phase,
    pub component: Component,
    pub step: Step,
    pub severity: Severity,
    pub outputs: OutputSet,
    pub counters: &'static [CounterBinding],
    pub fields: &'static [FieldId],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Phase {
    Ingress,
    Decode,
    Parse,
    Sink,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    Hec,
    Auth,
    Body,
    Parser,
    Sink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Step {
    RequestReceived,
    RequestCompleted,
    Authorize,
    ReadBody,
    DecodeBody,
    ParseEvent,
    ParseRaw,
    SubmitSink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Outcome {
    Accepted,
    Rejected,
    Failed,
    Skipped,
    Throttled,
    Recovered,
    Informational,
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Throttled => "throttled",
            Self::Recovered => "recovered",
            Self::Informational => "informational",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSet(u8);

impl OutputSet {
    #[allow(dead_code)]
    pub const NONE: Self = Self(0);
    pub const TRACING: Self = Self(1 << 0);
    pub const CONSOLE: Self = Self(1 << 1);
    pub const STATS: Self = Self(1 << 2);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldId(&'static str);

impl FieldId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn name(self) -> &'static str {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub id: FieldId,
    pub value: FieldValue,
}

#[derive(Debug, Clone)]
pub enum FieldValue {
    Bool(bool),
    U64(u64),
    Str(&'static str),
    String(String),
    Outcome(Outcome),
    Endpoint(Endpoint),
}

impl FieldValue {
    fn to_json(&self) -> Value {
        match self {
            Self::Bool(value) => Value::Bool(*value),
            Self::U64(value) => Value::Number((*value).into()),
            Self::Str(value) => Value::String((*value).to_string()),
            Self::String(value) => Value::String(value.clone()),
            Self::Outcome(value) => Value::String(value.to_string()),
            Self::Endpoint(value) => Value::String(value.as_str().to_string()),
        }
    }
}

pub mod field {
    use super::{Endpoint, Field, FieldId, FieldValue, Outcome};
    use std::time::Duration;

    pub const OUTCOME: FieldId = FieldId::new("outcome");
    pub const HEC_CODE: FieldId = FieldId::new("hec_code");
    pub const HTTP_STATUS: FieldId = FieldId::new("http_status");
    pub const AUTH_SCHEME: FieldId = FieldId::new("auth_scheme");
    pub const TOKEN_PRESENT: FieldId = FieldId::new("token_present");
    pub const TOKEN_ID: FieldId = FieldId::new("token_id");
    pub const AUTH_LEN: FieldId = FieldId::new("auth_len");
    pub const HTTP_BODY_LEN: FieldId = FieldId::new("http_body_len");
    pub const DECODED_LEN: FieldId = FieldId::new("decoded_len");
    pub const EVENT_COUNT: FieldId = FieldId::new("event_count");
    pub const DROP_COUNT: FieldId = FieldId::new("drop_count");
    pub const WRITTEN_COUNT: FieldId = FieldId::new("written_count");
    pub const ELAPSED_US: FieldId = FieldId::new("elapsed_us");
    pub const ENDPOINT_KIND: FieldId = FieldId::new("endpoint_kind");
    pub const ROUTE_ALIAS: FieldId = FieldId::new("route_alias");
    pub const FAILURE_REASON: FieldId = FieldId::new("failure_reason");
    pub const INPUT_CLASS: FieldId = FieldId::new("input_class");
    pub const INPUT_OFFSET: FieldId = FieldId::new("input_offset");

    pub fn outcome(value: Outcome) -> Field {
        Field {
            id: OUTCOME,
            value: FieldValue::Outcome(value),
        }
    }

    pub fn hec_code(value: u16) -> Field {
        u64_field(HEC_CODE, u64::from(value))
    }

    pub fn http_status(value: u16) -> Field {
        u64_field(HTTP_STATUS, u64::from(value))
    }

    #[allow(dead_code)]
    pub fn auth_scheme(value: &'static str) -> Field {
        str_field(AUTH_SCHEME, value)
    }

    pub fn token_present(value: bool) -> Field {
        Field {
            id: TOKEN_PRESENT,
            value: FieldValue::Bool(value),
        }
    }

    #[allow(dead_code)]
    pub fn token_id(value: String) -> Field {
        Field {
            id: TOKEN_ID,
            value: FieldValue::String(value),
        }
    }

    #[allow(dead_code)]
    pub fn auth_len(value: usize) -> Field {
        usize_field(AUTH_LEN, value)
    }

    pub fn http_body_len(value: usize) -> Field {
        usize_field(HTTP_BODY_LEN, value)
    }

    pub fn decoded_len(value: usize) -> Field {
        usize_field(DECODED_LEN, value)
    }

    pub fn event_count(value: usize) -> Field {
        usize_field(EVENT_COUNT, value)
    }

    pub fn drop_count(value: usize) -> Field {
        usize_field(DROP_COUNT, value)
    }

    pub fn written_count(value: usize) -> Field {
        usize_field(WRITTEN_COUNT, value)
    }

    pub fn elapsed_us(value: Duration) -> Field {
        u64_field(
            ELAPSED_US,
            value.as_micros().min(u128::from(u64::MAX)) as u64,
        )
    }

    pub fn endpoint_kind(value: Endpoint) -> Field {
        Field {
            id: ENDPOINT_KIND,
            value: FieldValue::Endpoint(value),
        }
    }

    pub fn route_alias(value: String) -> Field {
        Field {
            id: ROUTE_ALIAS,
            value: FieldValue::String(value),
        }
    }

    pub fn failure_reason(value: &'static str) -> Field {
        str_field(FAILURE_REASON, value)
    }

    #[allow(dead_code)]
    pub fn input_class(value: InputClass) -> Field {
        str_field(INPUT_CLASS, value.as_str())
    }

    #[allow(dead_code)]
    pub fn input_offset(value: usize) -> Field {
        usize_field(INPUT_OFFSET, value)
    }

    fn usize_field(id: FieldId, value: usize) -> Field {
        u64_field(id, value.min(u64::MAX as usize) as u64)
    }

    fn u64_field(id: FieldId, value: u64) -> Field {
        Field {
            id,
            value: FieldValue::U64(value),
        }
    }

    fn str_field(id: FieldId, value: &'static str) -> Field {
        Field {
            id,
            value: FieldValue::Str(value),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[allow(dead_code)]
    pub enum InputClass {
        Lf,
        Crlf,
        Nul,
        Control,
        NonAscii,
        InvalidUtf8,
        Oversize,
        Other,
    }

    impl InputClass {
        pub const fn as_str(self) -> &'static str {
            match self {
                Self::Lf => "lf",
                Self::Crlf => "crlf",
                Self::Nul => "nul",
                Self::Control => "control",
                Self::NonAscii => "non_ascii",
                Self::InvalidUtf8 => "invalid_utf8",
                Self::Oversize => "oversize",
                Self::Other => "other",
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CounterBinding {
    Increment(Counter),
    AddField { counter: Counter, field: FieldId },
    RecordLatency { field: FieldId },
}

#[derive(Debug)]
pub struct ReportContext {
    request_id: u64,
}

impl ReportContext {
    pub fn request() -> Self {
        static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            request_id: NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn request_id(&self) -> u64 {
        self.request_id
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReportOutputs {
    pub tracing: bool,
    pub console: bool,
    pub stats: bool,
}

impl Default for ReportOutputs {
    fn default() -> Self {
        Self {
            tracing: true,
            console: false,
            stats: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct Reporter {
    stats: Stats,
    outputs: ReportOutputs,
}

impl Reporter {
    pub fn new(outputs: ReportOutputs) -> Self {
        Self {
            stats: Stats::default(),
            outputs,
        }
    }

    pub fn submit(&self, ctx: &ReportContext, fact: FactId, fields: Vec<Field>) {
        let spec = fact_spec(fact);
        debug_assert!(
            fields.iter().all(|field| spec.fields.contains(&field.id)),
            "reporting fact {} received a field outside its declared field set",
            spec.name
        );
        if self.outputs.stats && spec.outputs.contains(OutputSet::STATS) {
            self.apply_counters(spec, &fields);
        }
        if self.outputs.tracing && spec.outputs.contains(OutputSet::TRACING) {
            self.emit_tracing(ctx, spec, &fields);
        }
        if self.outputs.console && spec.outputs.contains(OutputSet::CONSOLE) {
            self.emit_console(ctx, spec, &fields);
        }
    }

    #[allow(dead_code)]
    pub fn submit_lazy<F>(&self, ctx: &ReportContext, fact: FactId, fields: F)
    where
        F: FnOnce() -> Vec<Field>,
    {
        if self.enabled(fact) {
            self.submit(ctx, fact, fields());
        }
    }

    #[allow(dead_code)]
    pub fn enabled(&self, fact: FactId) -> bool {
        let spec = fact_spec(fact);
        (self.outputs.stats && spec.outputs.contains(OutputSet::STATS))
            || (self.outputs.tracing && spec.outputs.contains(OutputSet::TRACING))
            || (self.outputs.console && spec.outputs.contains(OutputSet::CONSOLE))
    }

    pub fn stats_snapshot(&self) -> super::stats::StatsSnapshot {
        self.stats.snapshot()
    }

    fn apply_counters(&self, spec: &FactSpec, fields: &[Field]) {
        for counter in spec.counters {
            match *counter {
                CounterBinding::Increment(counter) => self.stats.increment(counter),
                CounterBinding::AddField { counter, field } => {
                    if let Some(value) = get_u64(fields, field) {
                        self.stats.add(counter, value);
                    }
                }
                CounterBinding::RecordLatency { field } => {
                    if let Some(value) = get_u64(fields, field) {
                        self.stats
                            .record_latency(Duration::from_nanos(value.saturating_mul(1_000)));
                    }
                }
            }
        }
    }

    fn emit_tracing(&self, ctx: &ReportContext, spec: &FactSpec, fields: &[Field]) {
        let fields_json = fields_json(fields).to_string();
        macro_rules! emit_for_target {
            ($target:literal) => {
                match spec.severity {
                    Severity::Trace => tracing::trace!(
                        target: $target,
                        fact = spec.name,
                        phase = spec.phase.as_str(),
                        component = spec.component.as_str(),
                        step = spec.step.as_str(),
                        request_id = ctx.request_id(),
                        fields = %fields_json
                    ),
                    Severity::Debug => tracing::debug!(
                        target: $target,
                        fact = spec.name,
                        phase = spec.phase.as_str(),
                        component = spec.component.as_str(),
                        step = spec.step.as_str(),
                        request_id = ctx.request_id(),
                        fields = %fields_json
                    ),
                    Severity::Info => tracing::info!(
                        target: $target,
                        fact = spec.name,
                        phase = spec.phase.as_str(),
                        component = spec.component.as_str(),
                        step = spec.step.as_str(),
                        request_id = ctx.request_id(),
                        fields = %fields_json
                    ),
                    Severity::Warn => tracing::warn!(
                        target: $target,
                        fact = spec.name,
                        phase = spec.phase.as_str(),
                        component = spec.component.as_str(),
                        step = spec.step.as_str(),
                        request_id = ctx.request_id(),
                        fields = %fields_json
                    ),
                    Severity::Error => tracing::error!(
                        target: $target,
                        fact = spec.name,
                        phase = spec.phase.as_str(),
                        component = spec.component.as_str(),
                        step = spec.step.as_str(),
                        request_id = ctx.request_id(),
                        fields = %fields_json
                    ),
                }
            };
        }
        match spec.component {
            Component::Hec => emit_for_target!("hec.receiver"),
            Component::Auth => emit_for_target!("hec.auth"),
            Component::Body => emit_for_target!("hec.body"),
            Component::Parser => emit_for_target!("hec.parser"),
            Component::Sink => emit_for_target!("hec.sink"),
        }
    }

    fn emit_console(&self, ctx: &ReportContext, spec: &FactSpec, fields: &[Field]) {
        let _ = writeln!(
            io::stderr(),
            "{} {} phase={} component={} step={} request={} fields={}",
            spec.severity.as_str(),
            spec.name,
            spec.phase.as_str(),
            spec.component.as_str(),
            spec.step.as_str(),
            ctx.request_id(),
            fields_json(fields)
        );
    }
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ingress => "ingress",
            Self::Decode => "decode",
            Self::Parse => "parse",
            Self::Sink => "sink",
            Self::Runtime => "runtime",
        }
    }
}

impl Component {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hec => "hec",
            Self::Auth => "auth",
            Self::Body => "body",
            Self::Parser => "parser",
            Self::Sink => "sink",
        }
    }
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::RequestReceived => "request_received",
            Self::RequestCompleted => "request_completed",
            Self::Authorize => "authorize",
            Self::ReadBody => "read_body",
            Self::DecodeBody => "decode_body",
            Self::ParseEvent => "parse_event",
            Self::ParseRaw => "parse_raw",
            Self::SubmitSink => "submit_sink",
        }
    }
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

pub mod facts {
    use super::FactId;

    pub const REQUEST_RECEIVED: FactId = FactId(1);
    pub const REQUEST_SUCCEEDED: FactId = FactId(2);
    pub const REQUEST_FAILED: FactId = FactId(3);
    pub const AUTH_TOKEN_REQUIRED: FactId = FactId(10);
    pub const AUTH_INVALID_AUTHORIZATION: FactId = FactId(11);
    pub const AUTH_TOKEN_INVALID: FactId = FactId(12);
    pub const AUTH_TOKEN_DISABLED: FactId = FactId(13);
    pub const GZIP_REQUEST: FactId = FactId(20);
    pub const GZIP_FAILED: FactId = FactId(21);
    pub const BODY_TOO_LARGE: FactId = FactId(22);
    pub const BODY_TIMEOUT: FactId = FactId(23);
    pub const HTTP_BODY_READ: FactId = FactId(24);
    pub const BODY_DECODED: FactId = FactId(25);
    pub const BODY_READ_FAILED: FactId = FactId(26);
    pub const BODY_UNSUPPORTED_ENCODING: FactId = FactId(27);
    pub const PARSE_FAILED: FactId = FactId(30);
    pub const EVENTS_PARSED: FactId = FactId(31);
    pub const EVENT_INDEX_INVALID: FactId = FactId(32);
    pub const SINK_FAILED: FactId = FactId(40);
    pub const SINK_COMPLETED: FactId = FactId(41);
}

const LOG_STATS: OutputSet = OutputSet::TRACING.union(OutputSet::STATS);
const LOG_CONSOLE_STATS: OutputSet = OutputSet::TRACING
    .union(OutputSet::CONSOLE)
    .union(OutputSet::STATS);

const REQUEST_RECEIVED_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::RequestsTotal)];
const REQUEST_SUCCEEDED_COUNTERS: &[CounterBinding] = &[
    CounterBinding::Increment(Counter::RequestsOk),
    CounterBinding::RecordLatency {
        field: field::ELAPSED_US,
    },
];
const REQUEST_FAILED_COUNTERS: &[CounterBinding] = &[
    CounterBinding::Increment(Counter::RequestsFailed),
    CounterBinding::RecordLatency {
        field: field::ELAPSED_US,
    },
];
const AUTH_FAILURE_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::AuthFailures)];
const GZIP_REQUEST_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::GzipRequests)];
const GZIP_FAILED_COUNTERS: &[CounterBinding] = &[CounterBinding::Increment(Counter::GzipFailures)];
const BODY_TOO_LARGE_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::BodyTooLarge)];
const BODY_UNSUPPORTED_ENCODING_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::UnsupportedEncoding)];
const BODY_READ_FAILED_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::BodyReadErrors)];
const BODY_TIMEOUT_COUNTERS: &[CounterBinding] = &[CounterBinding::Increment(Counter::Timeouts)];
const HTTP_BODY_READ_COUNTERS: &[CounterBinding] = &[CounterBinding::AddField {
    counter: Counter::HttpBodyBytes,
    field: field::HTTP_BODY_LEN,
}];
const BODY_DECODED_COUNTERS: &[CounterBinding] = &[CounterBinding::AddField {
    counter: Counter::DecodedBytes,
    field: field::DECODED_LEN,
}];
const PARSE_FAILED_COUNTERS: &[CounterBinding] =
    &[CounterBinding::Increment(Counter::ParseFailures)];
const EVENTS_PARSED_COUNTERS: &[CounterBinding] = &[CounterBinding::AddField {
    counter: Counter::EventsObserved,
    field: field::EVENT_COUNT,
}];
const SINK_FAILED_COUNTERS: &[CounterBinding] = &[CounterBinding::Increment(Counter::SinkFailures)];
const SINK_COMPLETED_COUNTERS: &[CounterBinding] = &[
    CounterBinding::AddField {
        counter: Counter::EventsDropped,
        field: field::DROP_COUNT,
    },
    CounterBinding::AddField {
        counter: Counter::EventsWritten,
        field: field::WRITTEN_COUNT,
    },
];

const COMMON_REQUEST_FIELDS: &[FieldId] = &[
    field::OUTCOME,
    field::HEC_CODE,
    field::HTTP_STATUS,
    field::ENDPOINT_KIND,
    field::ROUTE_ALIAS,
    field::ELAPSED_US,
    field::FAILURE_REASON,
];

const AUTH_FIELDS: &[FieldId] = &[
    field::OUTCOME,
    field::AUTH_SCHEME,
    field::TOKEN_PRESENT,
    field::TOKEN_ID,
    field::AUTH_LEN,
    field::HEC_CODE,
    field::HTTP_STATUS,
];

const BODY_FIELDS: &[FieldId] = &[
    field::OUTCOME,
    field::HEC_CODE,
    field::HTTP_STATUS,
    field::HTTP_BODY_LEN,
    field::DECODED_LEN,
    field::INPUT_CLASS,
    field::INPUT_OFFSET,
    field::FAILURE_REASON,
];

const EVENT_FIELDS: &[FieldId] = &[field::EVENT_COUNT, field::OUTCOME];
const SINK_FIELDS: &[FieldId] = &[
    field::OUTCOME,
    field::EVENT_COUNT,
    field::DROP_COUNT,
    field::WRITTEN_COUNT,
];

static FACTS: &[FactSpec] = &[
    FactSpec {
        id: facts::REQUEST_RECEIVED,
        name: "hec.request.received",
        phase: Phase::Ingress,
        component: Component::Hec,
        step: Step::RequestReceived,
        severity: Severity::Debug,
        outputs: LOG_STATS,
        counters: REQUEST_RECEIVED_COUNTERS,
        fields: COMMON_REQUEST_FIELDS,
    },
    FactSpec {
        id: facts::REQUEST_SUCCEEDED,
        name: "hec.request.succeeded",
        phase: Phase::Ingress,
        component: Component::Hec,
        step: Step::RequestCompleted,
        severity: Severity::Info,
        outputs: LOG_STATS,
        counters: REQUEST_SUCCEEDED_COUNTERS,
        fields: COMMON_REQUEST_FIELDS,
    },
    FactSpec {
        id: facts::REQUEST_FAILED,
        name: "hec.request.failed",
        phase: Phase::Ingress,
        component: Component::Hec,
        step: Step::RequestCompleted,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: REQUEST_FAILED_COUNTERS,
        fields: COMMON_REQUEST_FIELDS,
    },
    FactSpec {
        id: facts::AUTH_TOKEN_REQUIRED,
        name: "hec.auth.token_required",
        phase: Phase::Ingress,
        component: Component::Auth,
        step: Step::Authorize,
        severity: Severity::Warn,
        outputs: LOG_CONSOLE_STATS,
        counters: AUTH_FAILURE_COUNTERS,
        fields: AUTH_FIELDS,
    },
    FactSpec {
        id: facts::AUTH_INVALID_AUTHORIZATION,
        name: "hec.auth.invalid_authorization",
        phase: Phase::Ingress,
        component: Component::Auth,
        step: Step::Authorize,
        severity: Severity::Warn,
        outputs: LOG_CONSOLE_STATS,
        counters: AUTH_FAILURE_COUNTERS,
        fields: AUTH_FIELDS,
    },
    FactSpec {
        id: facts::AUTH_TOKEN_INVALID,
        name: "hec.auth.token_invalid",
        phase: Phase::Ingress,
        component: Component::Auth,
        step: Step::Authorize,
        severity: Severity::Warn,
        outputs: LOG_CONSOLE_STATS,
        counters: AUTH_FAILURE_COUNTERS,
        fields: AUTH_FIELDS,
    },
    FactSpec {
        id: facts::AUTH_TOKEN_DISABLED,
        name: "hec.auth.token_disabled",
        phase: Phase::Ingress,
        component: Component::Auth,
        step: Step::Authorize,
        severity: Severity::Warn,
        outputs: LOG_CONSOLE_STATS,
        counters: AUTH_FAILURE_COUNTERS,
        fields: AUTH_FIELDS,
    },
    FactSpec {
        id: facts::GZIP_REQUEST,
        name: "hec.body.gzip_request",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::DecodeBody,
        severity: Severity::Debug,
        outputs: LOG_STATS,
        counters: GZIP_REQUEST_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::GZIP_FAILED,
        name: "hec.body.gzip_failed",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::DecodeBody,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: GZIP_FAILED_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::BODY_TOO_LARGE,
        name: "hec.body.too_large",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::ReadBody,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: BODY_TOO_LARGE_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::BODY_TIMEOUT,
        name: "hec.body.timeout",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::ReadBody,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: BODY_TIMEOUT_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::BODY_READ_FAILED,
        name: "hec.body.read_failed",
        phase: Phase::Ingress,
        component: Component::Body,
        step: Step::ReadBody,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: BODY_READ_FAILED_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::BODY_UNSUPPORTED_ENCODING,
        name: "hec.body.unsupported_encoding",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::DecodeBody,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: BODY_UNSUPPORTED_ENCODING_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::HTTP_BODY_READ,
        name: "hec.body.http_body_read",
        phase: Phase::Ingress,
        component: Component::Body,
        step: Step::ReadBody,
        severity: Severity::Debug,
        outputs: OutputSet::STATS,
        counters: HTTP_BODY_READ_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::BODY_DECODED,
        name: "hec.body.decoded",
        phase: Phase::Decode,
        component: Component::Body,
        step: Step::DecodeBody,
        severity: Severity::Debug,
        outputs: OutputSet::STATS,
        counters: BODY_DECODED_COUNTERS,
        fields: BODY_FIELDS,
    },
    FactSpec {
        id: facts::PARSE_FAILED,
        name: "hec.parser.failed",
        phase: Phase::Parse,
        component: Component::Parser,
        step: Step::ParseEvent,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: PARSE_FAILED_COUNTERS,
        fields: COMMON_REQUEST_FIELDS,
    },
    FactSpec {
        id: facts::EVENTS_PARSED,
        name: "hec.parser.events_parsed",
        phase: Phase::Parse,
        component: Component::Parser,
        step: Step::ParseEvent,
        severity: Severity::Debug,
        outputs: OutputSet::STATS,
        counters: EVENTS_PARSED_COUNTERS,
        fields: EVENT_FIELDS,
    },
    FactSpec {
        id: facts::EVENT_INDEX_INVALID,
        name: "hec.parser.index_invalid",
        phase: Phase::Parse,
        component: Component::Parser,
        step: Step::ParseEvent,
        severity: Severity::Warn,
        outputs: LOG_STATS,
        counters: PARSE_FAILED_COUNTERS,
        fields: COMMON_REQUEST_FIELDS,
    },
    FactSpec {
        id: facts::SINK_FAILED,
        name: "hec.sink.failed",
        phase: Phase::Sink,
        component: Component::Sink,
        step: Step::SubmitSink,
        severity: Severity::Error,
        outputs: LOG_STATS,
        counters: SINK_FAILED_COUNTERS,
        fields: SINK_FIELDS,
    },
    FactSpec {
        id: facts::SINK_COMPLETED,
        name: "hec.sink.completed",
        phase: Phase::Sink,
        component: Component::Sink,
        step: Step::SubmitSink,
        severity: Severity::Debug,
        outputs: OutputSet::STATS,
        counters: SINK_COMPLETED_COUNTERS,
        fields: SINK_FIELDS,
    },
];

fn fact_spec(id: FactId) -> &'static FactSpec {
    FACTS
        .iter()
        .find(|fact| fact.id == id)
        .unwrap_or_else(|| panic!("unknown reporting fact id {}", id.0))
}

fn get_u64(fields: &[Field], field: FieldId) -> Option<u64> {
    fields.iter().find_map(|candidate| {
        if candidate.id == field {
            if let FieldValue::U64(value) = candidate.value {
                return Some(value);
            }
        }
        None
    })
}

fn fields_json(fields: &[Field]) -> Value {
    let mut out = Map::new();
    for field in fields {
        out.insert(field.id.name().to_string(), field.value.to_json());
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::{facts, field, Outcome, ReportContext, ReportOutputs, Reporter};

    #[test]
    fn submit_updates_stats_from_fact_mapping() {
        let reporter = Reporter::default();
        let ctx = ReportContext::request();
        reporter.submit(
            &ctx,
            facts::AUTH_TOKEN_INVALID,
            vec![field::outcome(Outcome::Rejected), field::hec_code(4)],
        );

        let stats = reporter.stats_snapshot();
        assert_eq!(stats.auth_failures, 1);
    }

    #[test]
    fn submit_adds_length_fields_to_stats() {
        let reporter = Reporter::default();
        let ctx = ReportContext::request();
        reporter.submit(&ctx, facts::HTTP_BODY_READ, vec![field::http_body_len(11)]);
        reporter.submit(&ctx, facts::BODY_DECODED, vec![field::decoded_len(7)]);

        let stats = reporter.stats_snapshot();
        assert_eq!(stats.http_body_bytes, 11);
        assert_eq!(stats.decoded_bytes, 7);
    }

    #[test]
    fn submit_lazy_skips_field_construction_when_fact_disabled() {
        let reporter = Reporter::new(ReportOutputs {
            tracing: false,
            console: false,
            stats: false,
        });
        let ctx = ReportContext::request();
        let mut called = false;
        reporter.submit_lazy(&ctx, facts::AUTH_TOKEN_INVALID, || {
            called = true;
            vec![field::outcome(Outcome::Rejected)]
        });

        assert!(!called);
    }
}
