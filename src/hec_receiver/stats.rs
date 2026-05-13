use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counter {
    RequestsTotal,
    RequestsOk,
    RequestsFailed,
    AuthFailures,
    BodyTooLarge,
    UnsupportedEncoding,
    BodyReadErrors,
    Timeouts,
    GzipRequests,
    GzipFailures,
    ParseFailures,
    HttpBodyBytes,
    DecodedBytes,
    EventsObserved,
    EventsDropped,
    EventsWritten,
    SinkFailures,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub requests_total: AtomicU64,
    pub requests_ok: AtomicU64,
    pub requests_failed: AtomicU64,
    pub auth_failures: AtomicU64,
    pub body_too_large: AtomicU64,
    pub unsupported_encoding: AtomicU64,
    pub body_read_errors: AtomicU64,
    pub timeouts: AtomicU64,
    pub gzip_requests: AtomicU64,
    pub gzip_failures: AtomicU64,
    pub parse_failures: AtomicU64,
    pub http_body_bytes: AtomicU64,
    pub decoded_bytes: AtomicU64,
    pub events_observed: AtomicU64,
    pub events_dropped: AtomicU64,
    pub events_written: AtomicU64,
    pub sink_failures: AtomicU64,
    pub latency_nanos_total: AtomicU64,
    pub latency_nanos_max: AtomicU64,
}

impl Stats {
    pub fn increment(&self, counter: Counter) {
        self.add(counter, 1);
    }

    pub fn add(&self, counter: Counter, value: u64) {
        self.counter(counter).fetch_add(value, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            requests_total: load(&self.requests_total),
            requests_ok: load(&self.requests_ok),
            requests_failed: load(&self.requests_failed),
            auth_failures: load(&self.auth_failures),
            body_too_large: load(&self.body_too_large),
            unsupported_encoding: load(&self.unsupported_encoding),
            body_read_errors: load(&self.body_read_errors),
            timeouts: load(&self.timeouts),
            gzip_requests: load(&self.gzip_requests),
            gzip_failures: load(&self.gzip_failures),
            parse_failures: load(&self.parse_failures),
            http_body_bytes: load(&self.http_body_bytes),
            decoded_bytes: load(&self.decoded_bytes),
            events_observed: load(&self.events_observed),
            events_dropped: load(&self.events_dropped),
            events_written: load(&self.events_written),
            sink_failures: load(&self.sink_failures),
            latency_nanos_total: load(&self.latency_nanos_total),
            latency_nanos_max: load(&self.latency_nanos_max),
        }
    }

    pub fn record_latency(&self, elapsed: Duration) {
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.latency_nanos_total.fetch_add(nanos, Ordering::Relaxed);
        let mut current = self.latency_nanos_max.load(Ordering::Relaxed);
        while nanos > current {
            match self.latency_nanos_max.compare_exchange_weak(
                current,
                nanos,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
    }

    fn counter(&self, counter: Counter) -> &AtomicU64 {
        match counter {
            Counter::RequestsTotal => &self.requests_total,
            Counter::RequestsOk => &self.requests_ok,
            Counter::RequestsFailed => &self.requests_failed,
            Counter::AuthFailures => &self.auth_failures,
            Counter::BodyTooLarge => &self.body_too_large,
            Counter::UnsupportedEncoding => &self.unsupported_encoding,
            Counter::BodyReadErrors => &self.body_read_errors,
            Counter::Timeouts => &self.timeouts,
            Counter::GzipRequests => &self.gzip_requests,
            Counter::GzipFailures => &self.gzip_failures,
            Counter::ParseFailures => &self.parse_failures,
            Counter::HttpBodyBytes => &self.http_body_bytes,
            Counter::DecodedBytes => &self.decoded_bytes,
            Counter::EventsObserved => &self.events_observed,
            Counter::EventsDropped => &self.events_dropped,
            Counter::EventsWritten => &self.events_written,
            Counter::SinkFailures => &self.sink_failures,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatsSnapshot {
    pub requests_total: u64,
    pub requests_ok: u64,
    pub requests_failed: u64,
    pub auth_failures: u64,
    pub body_too_large: u64,
    pub unsupported_encoding: u64,
    pub body_read_errors: u64,
    pub timeouts: u64,
    pub gzip_requests: u64,
    pub gzip_failures: u64,
    pub parse_failures: u64,
    pub http_body_bytes: u64,
    pub decoded_bytes: u64,
    pub events_observed: u64,
    pub events_dropped: u64,
    pub events_written: u64,
    pub sink_failures: u64,
    pub latency_nanos_total: u64,
    pub latency_nanos_max: u64,
}

fn load(value: &AtomicU64) -> u64 {
    value.load(Ordering::Relaxed)
}
