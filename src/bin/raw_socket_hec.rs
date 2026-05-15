use clap::Parser;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{create_dir_all, File},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    path::PathBuf,
    thread::sleep,
    time::{Duration, Instant},
};

const AFTER_LONG_HELP: &str = r#"Case reference:
  all  Run every implemented raw-socket case.
  1    valid_raw                 Complete raw HEC request; should reach handler and succeed.
  2    partial_header_timeout    Request line and partial headers only; usually stack-owned/no handler.
  3    header_length             Content-Length header promises body bytes that never arrive.
  4    header_length_shutdown    Same as 3, then brief read, write shutdown, and another read.
  5    header_length_short_body  Content-Length larger than actual body, then half-close.
  6    chunked_truncated         Truncated chunked transfer body.
  7    slow_body                 Segmented body with configurable inter-segment delay.
  8    malformed_header_bytes    Invalid header bytes; usually stack-owned/no handler.

Artifacts:
  OUT/YYYYMMDDThhmmssZ/show-config.log  run configuration and selected cases
  OUT/YYYYMMDDThhmmssZ/requests/NN_name.bin   exact bytes written to the TCP socket
  OUT/YYYYMMDDThhmmssZ/responses/NN_name.bin  exact bytes read from the TCP socket
  OUT/YYYYMMDDThhmmssZ/stats/NN_name_before.json and stats/NN_name_after.json when --stats-url is set
  OUT/YYYYMMDDThhmmssZ/summary.tsv            compact human review table
  OUT/YYYYMMDDThhmmssZ/results.json           machine-readable result records

Interpretation:
  A status value means an HTTP response was read from the target.
  timed_out=true means this verifier's read timeout expired.
  error records client-side socket errors such as reset while writing.
  verdict=pass/fail is used only for simple expectations; info/review means evidence.
  Cases that never reach the HEC handler are still useful stack evidence.
"#;

#[derive(Debug, Parser)]
#[command(
    name = "raw_socket_hec",
    about = "Send exact-byte raw TCP/HTTP HEC probes",
    long_about = "Send exact-byte raw TCP/HTTP HEC probes that curl, AB, and normal HTTP clients cannot reliably express.",
    after_long_help = AFTER_LONG_HELP
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18088", help = "Target HEC host:port")]
    addr: SocketAddr,
    #[arg(long, default_value = "dev-token", help = "Splunk HEC token value")]
    token: String,
    #[arg(
        long,
        default_value = "results/raw-socket",
        help = "Output root; each run creates a UTC timestamp subdirectory"
    )]
    out: PathBuf,
    #[arg(
        long,
        default_value = "all",
        help = "Case selector: all, a case number, or a case name"
    )]
    case: String,
    #[arg(
        long,
        default_value_t = 8_000,
        help = "Socket read/write timeout in milliseconds"
    )]
    read_timeout_ms: u64,
    #[arg(
        long,
        default_value_t = 6_000,
        help = "Delay in milliseconds before completing the slow_body case"
    )]
    slow_body_delay_ms: u64,
    #[arg(long, help = "Print numbered case reference and exit")]
    list_cases: bool,
    #[arg(
        long,
        help = "Optional plain-HTTP stats URL to snapshot before and after each case, for example http://127.0.0.1:18088/hec/stats"
    )]
    stats_url: Option<String>,
    #[arg(
        long,
        default_value_t = 1_000,
        help = "Stats snapshot read/write timeout in milliseconds"
    )]
    stats_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct CaseSpec {
    id: u8,
    name: &'static str,
    purpose: &'static str,
    handler_expectation: &'static str,
}

#[derive(Debug, Clone)]
struct RawHttpCase {
    id: u8,
    name: &'static str,
    purpose: &'static str,
    handler_expectation: &'static str,
    segments: Vec<Segment>,
    shutdown_write: bool,
    pre_shutdown_read_timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
struct Segment {
    bytes: Vec<u8>,
    sleep_after: Duration,
}

#[derive(Debug, Serialize)]
struct RawHttpResult {
    id: u8,
    name: String,
    purpose: &'static str,
    handler_expectation: &'static str,
    verdict: Verdict,
    diagnosis: String,
    pre_shutdown_timed_out: bool,
    elapsed_ms: u128,
    timed_out: bool,
    status: Option<u16>,
    response_bytes: usize,
    response_file: String,
    error: Option<String>,
    stats_before_file: Option<String>,
    stats_after_file: Option<String>,
    stats_delta: Option<BTreeMap<String, i64>>,
    stats_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Pass,
    Fail,
    Info,
    Review,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if cli.list_cases {
        print!("{}", case_reference());
        return Ok(());
    }

    let run_dir = cli.out.join(run_timestamp());
    create_dir_all(run_dir.join("requests"))?;
    create_dir_all(run_dir.join("responses"))?;
    if cli.stats_url.is_some() {
        create_dir_all(run_dir.join("stats"))?;
    }

    let cases = select_cases(&cli.case, &cli.token, cli.slow_body_delay_ms)?;
    write_run_config(&run_dir, &cli, &cases)?;
    let mut results = Vec::new();
    let stats_timeout = Duration::from_millis(cli.stats_timeout_ms);
    for case in cases {
        let request_file = run_dir
            .join("requests")
            .join(format!("{}.bin", case.file_stem()));
        write_request_file(&request_file, &case)?;
        let stats_before = match &cli.stats_url {
            Some(url) => snapshot_stats(&run_dir, &case, url, stats_timeout, "before"),
            None => None,
        };
        let result = run_case(cli.addr, &case, Duration::from_millis(cli.read_timeout_ms));
        let stats_after = match &cli.stats_url {
            Some(url) => snapshot_stats(&run_dir, &case, url, stats_timeout, "after"),
            None => None,
        };
        results.push(write_result(&run_dir, &case, result, stats_before, stats_after)?);
    }
    write_summary(&run_dir, &results)?;
    serde_json::to_writer_pretty(File::create(run_dir.join("results.json"))?, &results)?;
    println!("{}", run_dir.display());
    Ok(())
}

fn run_timestamp() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now())
        .to_string()
        .replace(['-', ':'], "")
}

fn write_run_config(
    out: &std::path::Path,
    cli: &Cli,
    cases: &[RawHttpCase],
) -> std::io::Result<()> {
    let mut file = File::create(out.join("show-config.log"))?;
    writeln!(file, "addr={}", cli.addr)?;
    writeln!(file, "case={}", cli.case)?;
    writeln!(file, "read_timeout_ms={}", cli.read_timeout_ms)?;
    writeln!(file, "slow_body_delay_ms={}", cli.slow_body_delay_ms)?;
    writeln!(file, "stats_url={}", cli.stats_url.as_deref().unwrap_or("-"))?;
    writeln!(file, "stats_timeout_ms={}", cli.stats_timeout_ms)?;
    writeln!(file, "token_present={}", !cli.token.is_empty())?;
    writeln!(file)?;
    writeln!(file, "selected_cases:")?;
    for case in cases {
        writeln!(
            file,
            "  {} {}: {} ({})",
            case.id, case.name, case.purpose, case.handler_expectation
        )?;
    }
    Ok(())
}

fn select_cases(
    name: &str,
    token: &str,
    slow_body_delay_ms: u64,
) -> Result<Vec<RawHttpCase>, String> {
    let cases = cases(token, slow_body_delay_ms);
    if name == "all" {
        return Ok(cases);
    }
    let selected = cases
        .into_iter()
        .filter(|case| case.name == name || case.id.to_string() == name)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        Err(format!(
            "unknown raw socket case: {name}\n\n{}",
            case_reference()
        ))
    } else {
        Ok(selected)
    }
}

const CASE_SPECS: &[CaseSpec] = &[
    CaseSpec {
        id: 1,
        name: "valid_raw",
        purpose: "baseline complete raw HEC request",
        handler_expectation: "handler-visible success",
    },
    CaseSpec {
        id: 2,
        name: "partial_header_timeout",
        purpose: "request line and unterminated header block",
        handler_expectation: "usually stack-owned before handler",
    },
    CaseSpec {
        id: 3,
        name: "header_length",
        purpose: "Content-Length promises body bytes that never arrive",
        handler_expectation: "handler-visible body timeout",
    },
    CaseSpec {
        id: 4,
        name: "header_length_shutdown",
        purpose: "Content-Length promises body bytes, brief read, then write shutdown",
        handler_expectation: "handler-visible timeout or EOF classification",
    },
    CaseSpec {
        id: 5,
        name: "header_length_short_body",
        purpose: "fewer body bytes than Content-Length, then write half-close",
        handler_expectation: "handler-visible body read error",
    },
    CaseSpec {
        id: 6,
        name: "chunked_truncated",
        purpose: "incomplete chunked transfer framing",
        handler_expectation: "handler-visible body read error",
    },
    CaseSpec {
        id: 7,
        name: "slow_body",
        purpose: "segmented body with configurable delay before completion",
        handler_expectation: "handler-visible success or timeout depending on configured delay",
    },
    CaseSpec {
        id: 8,
        name: "malformed_header_bytes",
        purpose: "non-UTF/non-token header bytes",
        handler_expectation: "usually stack-owned before handler",
    },
];

fn case_reference() -> String {
    let mut text = String::from("Raw-socket HEC cases:\n");
    for spec in CASE_SPECS {
        text.push_str(&format!(
            "  {:>2}. {:<30} {:<52} {}\n",
            spec.id, spec.name, spec.purpose, spec.handler_expectation
        ));
    }
    text
}

impl RawHttpCase {
    fn from_spec(spec: CaseSpec, segments: Vec<Segment>, shutdown_write: bool) -> Self {
        Self {
            id: spec.id,
            name: spec.name,
            purpose: spec.purpose,
            handler_expectation: spec.handler_expectation,
            segments,
            shutdown_write,
            pre_shutdown_read_timeout: None,
        }
    }

    fn with_pre_shutdown_read_timeout(mut self, timeout: Duration) -> Self {
        self.pre_shutdown_read_timeout = Some(timeout);
        self
    }

    fn file_stem(&self) -> String {
        format!("{:02}_{}", self.id, self.name)
    }
}

fn spec(name: &str) -> CaseSpec {
    CASE_SPECS
        .iter()
        .copied()
        .find(|case| case.name == name)
        .expect("raw socket case spec must exist")
}

fn cases(token: &str, slow_body_delay_ms: u64) -> Vec<RawHttpCase> {
    vec![
        valid_raw(token),
        partial_header_timeout(),
        header_length(token),
        header_length_shutdown(token),
        header_length_short_body(token),
        chunked_truncated(token),
        slow_body(token, slow_body_delay_ms),
        malformed_header_bytes(),
    ]
}

fn valid_raw(token: &str) -> RawHttpCase {
    let mut bytes = headers(token, Some(4), None);
    bytes.extend_from_slice(b"a\nb\n");
    RawHttpCase::from_spec(spec("valid_raw"), vec![segment(bytes, 0)], false)
}

fn partial_header_timeout() -> RawHttpCase {
    RawHttpCase::from_spec(
        spec("partial_header_timeout"),
        vec![segment(
            b"POST /services/collector/raw HTTP/1.1\r\nHost: localhost\r\n".to_vec(),
            1_000,
        )],
        false,
    )
}

fn header_length(token: &str) -> RawHttpCase {
    RawHttpCase::from_spec(
        spec("header_length"),
        vec![segment(headers(token, Some(10), None), 1_000)],
        false,
    )
}

fn header_length_shutdown(token: &str) -> RawHttpCase {
    RawHttpCase::from_spec(
        spec("header_length_shutdown"),
        vec![segment(headers(token, Some(10), None), 0)],
        true,
    )
    .with_pre_shutdown_read_timeout(Duration::from_millis(500))
}

fn header_length_short_body(token: &str) -> RawHttpCase {
    let mut bytes = headers(token, Some(10), None);
    bytes.extend_from_slice(b"abc");
    RawHttpCase::from_spec(
        spec("header_length_short_body"),
        vec![segment(bytes, 0)],
        true,
    )
}

fn chunked_truncated(token: &str) -> RawHttpCase {
    let mut bytes = headers(token, None, Some("chunked"));
    bytes.extend_from_slice(b"5\r\nabc");
    RawHttpCase::from_spec(spec("chunked_truncated"), vec![segment(bytes, 0)], true)
}

fn slow_body(token: &str, slow_body_delay_ms: u64) -> RawHttpCase {
    RawHttpCase::from_spec(
        spec("slow_body"),
        vec![
            segment(headers(token, Some(4), None), 0),
            segment(b"a".to_vec(), slow_body_delay_ms),
            segment(b"\nb\n".to_vec(), 0),
        ],
        false,
    )
}

fn malformed_header_bytes() -> RawHttpCase {
    RawHttpCase::from_spec(
        spec("malformed_header_bytes"),
        vec![segment(
            b"POST /services/collector/raw HTTP/1.1\r\nHost: local\xffhost\r\n\r\n".to_vec(),
            0,
        )],
        true,
    )
}

fn headers(token: &str, content_length: Option<usize>, transfer_encoding: Option<&str>) -> Vec<u8> {
    let mut headers = format!(
        "POST /services/collector/raw HTTP/1.1\r\nHost: localhost\r\nAuthorization: Splunk {token}\r\nContent-Type: text/plain\r\nConnection: close\r\n"
    );
    if let Some(content_length) = content_length {
        headers.push_str(&format!("Content-Length: {content_length}\r\n"));
    }
    if let Some(transfer_encoding) = transfer_encoding {
        headers.push_str(&format!("Transfer-Encoding: {transfer_encoding}\r\n"));
    }
    headers.push_str("\r\n");
    headers.into_bytes()
}

fn segment(bytes: Vec<u8>, sleep_after_ms: u64) -> Segment {
    Segment {
        bytes,
        sleep_after: Duration::from_millis(sleep_after_ms),
    }
}

fn run_case(
    addr: SocketAddr,
    case: &RawHttpCase,
    read_timeout: Duration,
) -> std::io::Result<RunOutcome> {
    let started = Instant::now();
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(read_timeout))?;
    for segment in &case.segments {
        stream.write_all(&segment.bytes)?;
        if !segment.sleep_after.is_zero() {
            sleep(segment.sleep_after);
        }
    }
    let mut response = Vec::new();
    let mut pre_shutdown_timed_out = false;
    if let Some(pre_shutdown_read_timeout) = case.pre_shutdown_read_timeout {
        stream.set_read_timeout(Some(pre_shutdown_read_timeout))?;
        pre_shutdown_timed_out = read_available(&mut stream, &mut response)?;
        stream.set_read_timeout(Some(read_timeout))?;
    }
    if case.shutdown_write {
        let _ = stream.shutdown(Shutdown::Write);
    }

    let timed_out = read_available(&mut stream, &mut response)?;
    Ok(RunOutcome {
        response,
        elapsed: started.elapsed(),
        timed_out,
        pre_shutdown_timed_out,
    })
}

#[derive(Debug)]
struct RunOutcome {
    response: Vec<u8>,
    elapsed: Duration,
    timed_out: bool,
    pre_shutdown_timed_out: bool,
}

fn read_available(stream: &mut TcpStream, response: &mut Vec<u8>) -> std::io::Result<bool> {
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(n) => response.extend_from_slice(&buffer[..n]),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(true);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn write_request_file(path: &PathBuf, case: &RawHttpCase) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    for segment in &case.segments {
        file.write_all(&segment.bytes)?;
    }
    Ok(())
}

fn write_result(
    out: &std::path::Path,
    case: &RawHttpCase,
    result: std::io::Result<RunOutcome>,
    stats_before: Option<StatsSnapshot>,
    stats_after: Option<StatsSnapshot>,
) -> std::io::Result<RawHttpResult> {
    let response_file = out
        .join("responses")
        .join(format!("{}.bin", case.file_stem()));
    let stats_delta = stats_delta(stats_before.as_ref(), stats_after.as_ref());
    let stats_error = stats_error(stats_before.as_ref(), stats_after.as_ref());
    let stats_before_file = stats_before.as_ref().and_then(|stats| stats.file.clone());
    let stats_after_file = stats_after.as_ref().and_then(|stats| stats.file.clone());
    match result {
        Ok(outcome) => {
            let response = outcome.response;
            File::create(&response_file)?.write_all(&response)?;
            let status = parse_status(&response);
            let (verdict, diagnosis) = diagnose(
                case.id,
                status,
                outcome.timed_out,
                None,
                stats_delta.as_ref(),
            );
            Ok(RawHttpResult {
                id: case.id,
                name: case.name.to_string(),
                purpose: case.purpose,
                handler_expectation: case.handler_expectation,
                verdict,
                diagnosis,
                pre_shutdown_timed_out: outcome.pre_shutdown_timed_out,
                elapsed_ms: outcome.elapsed.as_millis(),
                timed_out: outcome.timed_out,
                status,
                response_bytes: response.len(),
                response_file: response_file.display().to_string(),
                error: None,
                stats_before_file,
                stats_after_file,
                stats_delta,
                stats_error,
            })
        }
        Err(error) => {
            let error_text = error.to_string();
            let (verdict, diagnosis) = diagnose(
                case.id,
                None,
                false,
                Some(error_text.as_str()),
                stats_delta.as_ref(),
            );
            Ok(RawHttpResult {
                id: case.id,
                name: case.name.to_string(),
                purpose: case.purpose,
                handler_expectation: case.handler_expectation,
                verdict,
                diagnosis,
                pre_shutdown_timed_out: false,
                elapsed_ms: 0,
                timed_out: false,
                status: None,
                response_bytes: 0,
                response_file: response_file.display().to_string(),
                error: Some(error_text),
                stats_before_file,
                stats_after_file,
                stats_delta,
                stats_error,
            })
        }
    }
}

fn parse_status(response: &[u8]) -> Option<u16> {
    let line = response.split(|byte| *byte == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut parts = line.split_whitespace();
    let _http_version = parts.next()?;
    parts.next()?.parse().ok()
}

fn write_summary(out: &std::path::Path, results: &[RawHttpResult]) -> std::io::Result<()> {
    let mut file = File::create(out.join("summary.tsv"))?;
    writeln!(
        file,
        "id\tcase\tverdict\tstatus\ttimed_out\tpre_shutdown_timed_out\trequests_delta\tfailed_delta\tbody_errors_delta\ttimeouts_delta\tresponse_bytes\tresponse_file\terror\tdiagnosis"
    )?;
    for result in results {
        writeln!(
            file,
            "{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            result.id,
            result.name,
            result.verdict,
            result
                .status
                .map(|status| status.to_string())
                .unwrap_or_else(|| "-".to_string()),
            result.timed_out,
            result.pre_shutdown_timed_out,
            delta_field(result, "requests_total"),
            delta_field(result, "requests_failed"),
            delta_field(result, "body_read_errors"),
            delta_field(result, "timeouts"),
            result.response_bytes,
            result.response_file,
            result.error.as_deref().unwrap_or("-"),
            result.diagnosis
        )?;
    }
    Ok(())
}

fn delta_field(result: &RawHttpResult, name: &str) -> String {
    result
        .stats_delta
        .as_ref()
        .and_then(|delta| delta.get(name))
        .map(i64::to_string)
        .unwrap_or_else(|| "-".to_string())
}

fn diagnose(
    id: u8,
    status: Option<u16>,
    timed_out: bool,
    error: Option<&str>,
    stats_delta: Option<&BTreeMap<String, i64>>,
) -> (Verdict, String) {
    let handler_seen = stats_delta
        .and_then(|delta| delta.get("requests_total"))
        .copied()
        .unwrap_or(0)
        > 0;
    let failed = |message: &str| (Verdict::Fail, message.to_string());
    let passed = |message: &str| (Verdict::Pass, message.to_string());
    let info = |message: &str| (Verdict::Info, message.to_string());
    let review = |message: &str| (Verdict::Review, message.to_string());

    match id {
        1 if status == Some(200) => passed("complete raw request returned HTTP 200"),
        1 => failed("complete raw request did not return HTTP 200"),
        2 if !handler_seen && (status.is_none() || timed_out || error.is_some()) => {
            info("partial header did not become a HEC request; inspect socket/stack behavior")
        }
        2 => review("partial header produced handler-visible or unexpected HTTP behavior"),
        3 if status == Some(408) => review("server returned HTTP 408 for missing body bytes"),
        3 if status.is_none() && (timed_out || error.is_some()) => {
            review("no HTTP response for missing body bytes; common stack behavior is wait, close, or log socket error")
        }
        3 => review("missing body bytes produced target-specific behavior"),
        4 if status == Some(400) || status == Some(408) => {
            review("server returned an explicit response after body absence plus write shutdown")
        }
        4 if status.is_none() || error.is_some() => {
            review("no HTTP response after body absence plus write shutdown; common stack behavior is close/log socket error")
        }
        4 => review("body absence plus write shutdown produced target-specific behavior"),
        5 if status == Some(400) || status == Some(408) => {
            review("server returned an explicit response for short Content-Length body")
        }
        5 if status.is_none() || error.is_some() => {
            review("no HTTP response for short Content-Length body; common stack behavior is close/log socket error")
        }
        5 => review("short Content-Length body produced target-specific behavior"),
        6 if status == Some(400) || status == Some(408) => {
            review("server returned an explicit response for truncated chunked body")
        }
        6 if status.is_none() || error.is_some() => {
            review("no HTTP response for truncated chunked body; common stack behavior is close/log socket error")
        }
        6 => review("truncated chunked body produced target-specific behavior"),
        7 if status == Some(200) => {
            review("slow body returned HTTP 200; delay stayed within server timeout")
        }
        7 if status == Some(408) => {
            passed("slow body exceeded server timeout and returned HTTP 408")
        }
        7 if handler_seen => {
            review("server recorded handler activity, but client saw no clean slow-body response")
        }
        7 => failed("slow body did not return HTTP 200, HTTP 408, or handler-visible activity"),
        8 if !handler_seen && status.is_none() => {
            info("malformed header bytes were rejected before HEC handler visibility")
        }
        8 => review("malformed header bytes produced handler-visible or unexpected HTTP behavior"),
        _ => review("no simple expectation defined for this case"),
    }
}

#[derive(Debug)]
struct StatsSnapshot {
    file: Option<String>,
    value: Option<Value>,
    error: Option<String>,
}

fn snapshot_stats(
    out: &std::path::Path,
    case: &RawHttpCase,
    url: &str,
    timeout: Duration,
    label: &str,
) -> Option<StatsSnapshot> {
    let file = out
        .join("stats")
        .join(format!("{}_{}.json", case.file_stem(), label));
    match get_json(url, timeout) {
        Ok(value) => {
            if let Ok(mut output) = File::create(&file) {
                let _ = serde_json::to_writer_pretty(&mut output, &value);
                let _ = output.write_all(b"\n");
            }
            Some(StatsSnapshot {
                file: Some(file.display().to_string()),
                value: Some(value),
                error: None,
            })
        }
        Err(error) => Some(StatsSnapshot {
            file: None,
            value: None,
            error: Some(format!("{label}: {error}")),
        }),
    }
}

fn stats_error(before: Option<&StatsSnapshot>, after: Option<&StatsSnapshot>) -> Option<String> {
    let mut errors = Vec::new();
    if let Some(error) = before.and_then(|stats| stats.error.as_ref()) {
        errors.push(error.as_str());
    }
    if let Some(error) = after.and_then(|stats| stats.error.as_ref()) {
        errors.push(error.as_str());
    }
    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

fn stats_delta(
    before: Option<&StatsSnapshot>,
    after: Option<&StatsSnapshot>,
) -> Option<BTreeMap<String, i64>> {
    let before = before?.value.as_ref()?.as_object()?;
    let after = after?.value.as_ref()?.as_object()?;
    let mut delta = BTreeMap::new();
    for (key, after_value) in after {
        if let (Some(before_number), Some(after_number)) = (
            before.get(key).and_then(Value::as_i64),
            after_value.as_i64(),
        ) {
            delta.insert(key.clone(), after_number - before_number);
        }
    }
    Some(delta)
}

fn get_json(url: &str, timeout: Duration) -> Result<Value, String> {
    let (host_port, path) = parse_http_url(url)?;
    let mut stream = TcpStream::connect(host_port.as_str()).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    let status =
        parse_status(&response).ok_or_else(|| "stats response has no status".to_string())?;
    if status != 200 {
        return Err(format!("stats endpoint returned HTTP {status}"));
    }
    let body_start = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .ok_or_else(|| "stats response has no header terminator".to_string())?;
    serde_json::from_slice(&response[body_start..]).map_err(|error| error.to_string())
}

fn parse_http_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| "stats URL must start with http://".to_string())?;
    let (host_port, path) = match rest.split_once('/') {
        Some((host_port, path)) => (host_port, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if host_port.is_empty() {
        return Err("stats URL must include host:port".to_string());
    }
    Ok((host_port.to_string(), path))
}
