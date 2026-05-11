# Formats — Log Record Structures, Parser Technique, And Format-Specific Processing

Status: reference and requirements input.

Scope: log and record formats that Spank HECpoc must preserve, parse, classify, normalize, tokenize, index, validate, and eventually search. This document owns format origins, structural examples, parser choices, edge cases, exception handling, and source-specific technical nuance. It does not own HEC wire behavior, Tokio/Axum mechanics, configuration infrastructure, or project status.

Primary local source carried forward: `/Users/walter/Work/Spank/spank-py/Logs.md`, especially the format taxonomy, timestamp problem, syslog/auth.log validation, Apache access inventory, Vector coverage model, Sigma context, parser dispatch alternatives, and auth.log corpus expansion findings. This document restates only the material needed for the Rust HECpoc direction.

---

## 1. Format Scope And Parser Boundary

This document starts where an input has a candidate record or line and asks: what structure does that record encode, which parser should recognize it, which fields can be extracted, which aliases are justified, and which malformed cases must be preserved or rejected?

Owned subjects:

- format origins and version splits;
- concrete record examples;
- parser selection for known formats;
- field extraction and canonical aliases;
- parser-specific hostile input cases;
- parser correctness tests and parser microbenchmarks.

Excluded subjects:

- transport, runtime, HEC response, queue, store, retention, reporting, and configuration behavior.

Parser boundary:

```text
record bytes or text
  -> known format family
  -> parser variant
  -> extracted fields
  -> parser status and reason
  -> canonical fields plus source-specific originals
```

Format validation is parser validation: the parser accepts expected records, rejects or marks malformed records, preserves raw evidence, and emits stable field names and parse reasons. End-to-end ingest, store, queue, and OS behavior are validated elsewhere.

## 2. Format Traditions And Consequences

### 2.1 Syslog Tradition

Origin: BSD/syslog practice, later documented by RFC 3164 and then superseded structurally by RFC 5424.

Structural traits:

- usually one event per line;
- optional PRI field `<N>` where `facility = N >> 3` and `severity = N & 7`;
- timestamp may be RFC 3164 style with no year/timezone or RFC 5424/rsyslog ISO style;
- program/tag and PID are usually embedded in the message prefix;
- message body has no universal escaping or schema.

Examples:

```text
Mar 10 23:30:31 host sshd[1234]: Failed password for invalid user admin from 203.0.113.9 port 51234 ssh2
2026-03-10T23:30:31.123456+00:00 host sshd[1234]: Accepted publickey for walter from 198.51.100.7 port 61322 ssh2
<34>1 2026-03-10T23:30:31Z host app 1234 ID47 [example@32473 user="walter"] message
```

References:

- [RFC 5424 — The Syslog Protocol](https://www.rfc-editor.org/rfc/rfc5424.html) defines modern syslog header and structured data.
- `/Users/walter/Work/Spank/spank-py/Logs.md §4` documents prior syslog parser findings and the Ubuntu ISO timestamp fix.

Parser implications:

- Support both RFC 3164-like and ISO timestamp prefixes for Linux file logs.
- Do not assume local file format equals network syslog protocol.
- Parse PRI only when present; absence is normal in local files.
- Keep raw body even when tag/PID extraction fails.
- Treat full RFC 5424 structured data as a separate parser capability, not as a minor syslog regex extension.

### 2.2 Web Server Access Log Tradition

Origin: NCSA/CERN HTTP server lineage, Apache `mod_log_config`, later adopted by Nginx-compatible combined formats.

Structural traits:

- typically one request per line;
- fixed positions within a named format;
- bracketed timestamp;
- quoted request, referer, user-agent fields;
- `-` represents absent information;
- operators can redefine formats, so inference requires either configuration or classifier signals.

Apache common and combined examples:

```text
127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326
127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326 "http://example.com/start.html" "Mozilla/4.08"
```

References:

- [Apache `mod_log_config`](https://httpd.apache.org/docs/current/en/mod/mod_log_config.html) defines `LogFormat` and common/combined examples.
- [Apache log files documentation](https://httpd.apache.org/docs/current/logs.html) shows common and combined log examples.
- [Splunk source type documentation](https://help.splunk.com/en/splunk-enterprise/get-started/get-data-in/9.2/configure-source-types/why-source-types-matter) lists `access_combined` and `apache_error` as common predefined source types.

Parser implications:

- The parser must be parameterized by format variant: common, combined, vhost, custom.
- Do not parse by generic whitespace splitting; quoted request and user-agent fields contain spaces.
- Preserve raw request line, then derive `method`, `uri`, `protocol` when syntax allows.
- Treat `-` as absent/null at extraction time, not as the literal username or referer.
- Record parser variant and classifier confidence for later debugging.

### 2.3 Application And Structured Logging Tradition

Origin: application logging frameworks and cloud-native stdout collection.

Structural traits:

- line-oriented but format is application configuration, not global standard;
- may be text patterns, JSON Lines/NDJSON, logfmt, or multiline stack traces;
- timestamps may be ISO, Java comma-milliseconds, epoch seconds, epoch milliseconds, or absent;
- Kubernetes/container systems preserve stdout/stderr stream metadata separately from the application record.

Examples:

```json
{"timestamp":"2026-03-10T23:30:31Z","level":"INFO","message":"login accepted","user":"walter"}
```

```text
time=2026-03-10T23:30:31Z level=info msg="login accepted" user=walter
```

Parser implications:

- Use JSON parser for JSON Lines; do not regex JSON.
- Use logfmt parser for key/value logs with quoting rules.
- Add multiline grouping before parsing Java/Python tracebacks as one event.
- Store parser errors and raw bytes/line for replay rather than dropping unknown application records.

---

## 3. Splunk, Vector, And Sigma As External Shape Constraints

### 3.1 Splunk Sourcetypes

Splunk's `sourcetype` is a default field that controls event formatting at ingest and remains a search field later. Splunk documentation emphasizes that sourcetypes govern timestamp extraction and event breaking, and examples include `access_combined`, `apache_error`, and `cisco_syslog`.

Spank implication:

- `sourcetype` is not merely a label. It selects parser, timestamp policy, line/event breaker, field aliases, and benchmark expectations.
- HEC metadata `sourcetype` should be preserved even before source-specific parsing is implemented.
- Format inference should be a fallback after explicit source/sourcetype metadata, not a replacement for configuration.

Relevant Splunk mechanics:

- `props.conf` owns `LINE_BREAKER`, `SHOULD_LINEMERGE`, `TIME_PREFIX`, `TIME_FORMAT`, `TRUNCATE`, and sourcetype behavior.
- Splunk can assign sourcetype by input, source, rule-based recognition, automatic matching, delayed rules, and learning.
- HEC JSON event endpoint metadata does not pass through ordinary line breaking in the same way as file/TCP ingest; Spank should keep HEC endpoint behavior distinct from file-source parsing.

References:

- [Splunk: why source types matter](https://help.splunk.com/en/splunk-enterprise/get-started/get-data-in/9.2/configure-source-types/why-source-types-matter).
- [Splunk `props.conf`](https://docs.splunk.com/Documentation/Splunk/latest/admin/propsconf).

### 3.2 Vector Parser Coverage

Vector provides specialized VRL functions for common formats rather than relying on one generic parser.

Relevant Vector functions:

- `parse_apache_log(value, format, timestamp_format?)` for Apache access formats.
- `parse_common_log(value, timestamp_format?)` for Common Log Format.
- `parse_nginx_log(value, format, timestamp_format?)` for Nginx `combined`, `ingress_upstreaminfo`, `main`, and `error` formats.
- `parse_logfmt` for logfmt.
- `parse_syslog` for syslog-like records.

References:

- [Vector VRL functions](https://vector.dev/docs/reference/vrl/functions/), including `parse_apache_log`, `parse_common_log`, and `parse_nginx_log`.
- [Vector file source](https://vector.dev/docs/reference/configuration/sources/file/) for rotation, ordering/fairness, and fingerprinting behavior.

Spank implication:

- The format architecture should be tiered: fixed byte scanning, format-specific parsers, structured decoders, then fallback regex.
- Copy Vector's explicit parser-function model, not necessarily its general pipeline structure.
- Preserve parser function, parser variant, and parse failure reason as fields in validation output.

### 3.3 Sigma Logsource Model

Sigma rules identify required logs through `logsource` fields: `category`, `product`, and `service`. Sigma documentation states that the logsource determines which logs the SIEM must search and that mismatches can make detections ineffective.

Local Sigma corpus count from `/Users/walter/Work/Spank/sOSS/sigma`:

| Count Type | Top Findings |
|---|---|
| Rule files scanned | `4105` YAML files |
| Rules with parsed logsource | `3951` |
| Top products | `windows=3026`, `linux=243`, `azure=138`, `macos=75`, `aws=64`, `zeek=26`, `okta=23`, `kubernetes=15` |
| Top services | `security=191`, `system=81`, `cloudtrail=64`, `auditd=60`, `audit=49`, `activitylogs=43`, `signinlogs=29`, `okta=23` |
| Top categories | `process_creation=1684`, `file_event=244`, `registry_set=233`, `ps_script=186`, `webserver=83`, `network_connection=71`, `proxy=62`, `dns_query=29` |

Spank implication:

- Windows dominates Sigma, but HECpoc's near-term Linux/web focus still maps to meaningful Sigma categories: `linux`, `auditd`, `webserver`, `proxy`, `network_connection`, and `dns_query`.
- Field aliasing is required. Sigma logsource names do not by themselves define the concrete field names in Apache, Nginx, auditd, syslog, or Splunk CIM.
- The parser registry should expose capability metadata: which product/service/category views a parser can support, which fields are native, which are derived, and which require optional enrichment.

References:

- [Sigma logsources](https://sigmahq.io/docs/basics/log-sources.html).
- [Sigma taxonomy appendix](https://sigmahq.io/sigma-specification/specification/sigma-appendix-taxonomy.html).

---

## 4. Initial Format Priority

Priority is based on local logs, Splunk/Vector/Sigma relevance, parser cost, and security value.

| Priority | Format Family | Why It Matters | Initial Parser Tier |
|---:|---|---|---|
| 1 | HEC raw line payloads | current receiver path, lowest-friction validation | byte split + raw preservation |
| 2 | Linux syslog/auth.log | local production corpus, security review, SSH/sudo/PAM | syslog prefix parser + sshd/auth subparser |
| 3 | Apache access/error | tutorial/sample corpora, web attack detection, benchmarks | format-specific parser |
| 4 | Nginx access/error/ingress | modern reverse proxy and Kubernetes ingress | format-specific parser; ingress variant supported explicitly |
| 5 | Linux auditd | high security value and Sigma `auditd` coverage | correlation-aware key/value parser |
| 6 | JSON Lines / NDJSON | application and Vector/Wazuh logs | streaming JSON decoder |
| 7 | logfmt | Go services and Kubernetes/controller logs | logfmt parser |
| 8 | Kubernetes CRI/container logs | cloud-native operational baseline | CRI framing parser + inner parser dispatch |
| 9 | Cloud/identity logs | AWS/Azure/GCP/Okta detection relevance | JSON schema-aware parser by source |
| 10 | Zeek/DNS/proxy/firewall | network detections and Sigma categories | tab/JSON parser or product parser |

---

## 5. Format Specifications And Examples

### 5.1 Linux Syslog And Auth Logs

Canonical input forms:

```text
Mar 10 23:30:31 host sshd[1234]: Failed password for invalid user admin from 203.0.113.9 port 51234 ssh2
2026-03-10T23:30:31.123456+00:00 host sudo: pam_unix(sudo:session): session opened for user root(uid=0) by walter(uid=1000)
```

Core fields:

| Field | Source | Notes |
|---|---|---|
| `timestamp` / `_time` | syslog prefix | RFC 3164 requires year injection; ISO prefix includes zone |
| `host` | syslog prefix | may be local hostname or relay-provided |
| `process` | tag before `:` | often `sshd`, `sudo`, `kernel`, `systemd` |
| `pid` | optional `[pid]` | integer when present |
| `message` | remainder | source-specific subparser input |

Auth subparser fields:

| Field | Meaning |
|---|---|
| `action` | accepted, failed, invalid_user, disconnecting, reverse_mapping, no_identification |
| `user` | account name where present |
| `src_ip` | remote IP where present |
| `src_port` | remote port where present |
| `auth_method` | password, publickey, keyboard-interactive, etc. |
| `stage` | preauth or session stage when emitted |

Known version split from prior validation:

```text
OpenSSH 6.x:  Received disconnect from 187.12.249.74: 11: Bye Bye [preauth]
OpenSSH 7.x+: Received disconnect from 101.36.123.66 port 43856:11: Bye Bye [preauth]
```

Requirements:

- Accept both old and new OpenSSH message forms.
- Keep unmatched auth messages as structured syslog events with `process=sshd` and raw `message`.
- Store `parse_family=syslog`, `parse_variant=linux_auth`, `parse_status=matched|partial|unmatched`.
- Avoid one monolithic regex; use prefix parse followed by process/message dispatch.

### 5.2 Linux Auditd

Example multi-record audit event:

```text
type=SYSCALL msg=audit(1700000000.123:4501): arch=c000003e syscall=59 success=yes exit=0 pid=5678 auid=1000 uid=1000 comm="bash" exe="/bin/bash" key="exec_command"
type=PATH msg=audit(1700000000.123:4501): item=0 name="/bin/bash" inode=131073 dev=08:01 mode=0100755 objtype=NORMAL
type=EXECVE msg=audit(1700000000.123:4501): argc=3 a0="bash" a1="-c" a2="id"
```

Core requirements:

- Parse `type` and `msg=audit(timestamp:serial)` as first-class fields.
- Use `(timestamp, serial)` as correlation key for event assembly.
- Preserve every record line even if correlation assembly is deferred.
- Parse outer key/value fields and nested single-quoted `msg='op=... res=...'` sections separately.
- Treat `auid=4294967295` as a special unset/no-session value; preserve raw numeric value and optionally expose normalized `auid_normalized=-1` or `auid_unset=true`.
- Optional enrichment: syscall number to syscall name, architecture code to architecture name, SELinux context splitting.

Attack/edge cases:

- records arriving out of order;
- incomplete multi-record event at chunk boundary;
- quoted values with spaces;
- duplicated key names, especially `msg`;
- extremely long `EXECVE` argument lists;
- audit logs forwarded through syslog/audispd where an outer syslog header wraps the audit record.

### 5.3 Apache Access Logs

Common format structure:

```text
%h %l %u %t "%r" %>s %b
```

Combined format structure:

```text
%h %l %u %t "%r" %>s %b "%{Referer}i" "%{User-agent}i"
```

Example:

```text
198.51.100.10 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326 "http://example.com/start.html" "Mozilla/4.08"
```

Fields:

| Field | Apache Directive | Notes |
|---|---|---|
| `client_ip` / `clientip` | `%h` | host or IP; may be proxy address |
| `ident` | `%l` | usually `-` |
| `user` | `%u` | authenticated user or `-` |
| `timestamp` | `%t` | bracketed CLF time |
| `request` | `%r` | raw request line |
| `method`, `uri`, `protocol` | derived from `%r` | derivation can fail on malformed scanners |
| `status` | `%>s` | final response status |
| `bytes` | `%b` | `-` for zero bytes in Apache semantics |
| `referer` | `%{Referer}i` | combined only |
| `user_agent` | `%{User-agent}i` | combined only |

Requirements:

- Parser must know the format variant before extraction or classify with confidence.
- Preserve raw request line and parse-derived fields separately.
- Treat malicious request strings as data, not parser syntax.
- Add optional reverse-proxy enrichment for `X-Forwarded-For` only when source configuration declares it trustworthy.

Security patterns supported by this format:

- path traversal: `../`, `%2e%2e`, double-encoding;
- webshell probes: `.php`, upload paths, suspicious query parameters;
- Log4Shell strings in URI/user-agent;
- repeated 401/403/404/500 statuses by source;
- suspicious methods: `TRACE`, `CONNECT`, malformed method tokens;
- user-agent scanners and exploit kits.

### 5.4 Apache Error Logs

Apache error logs vary by version and module. Common structure:

```text
[Wed Oct 11 14:32:52.123456 2023] [core:error] [pid 1234:tid 5678] [client 198.51.100.10:54321] AH00123: message
```

Requirements:

- Parse bracketed timestamp and module/severity when present.
- Parse `pid`, `tid`, `client`, Apache `AH` code when present.
- Do not assume every line has a client address.
- Treat tab-indented continuation lines as possible multiline event continuations.
- Preserve module-specific payload for later specialization, especially PHP, proxy, SSL, and ModSecurity.

### 5.5 Nginx Access, Error, And Ingress Logs

Nginx access logs usually use `combined` unless `log_format` overrides it. Nginx Ingress Controller adds controller logs plus Nginx access/error logs and configurable access log formats.

Combined example:

```text
172.17.0.1 - alice [01/Apr/2021:12:02:31 +0000] "POST /not-found HTTP/1.1" 404 153 "http://localhost/somewhere" "Mozilla/5.0" "2.75"
```

Error example:

```text
2021/04/01 13:02:31 [error] 31#31: *1 open() "/usr/share/nginx/html/not-found" failed (2: No such file or directory), client: 172.17.0.1, server: localhost, request: "POST /not-found HTTP/1.1", host: "localhost:8081"
```

Ingress-specific implications:

- `ingress_upstreaminfo` may include upstream address, response time, upstream status, namespace, ingress, service, request ID, and proxy alternative upstream name depending on controller version/configuration.
- Kubernetes deployments often terminate TLS and rewrite client identity through `X-Forwarded-For`, `X-Real-IP`, and ingress annotations. Trust policy must be source-specific.
- Nginx error logs often carry the only reason for upstream failures that access logs summarize as 502/503/504.

References:

- [NGINX access log documentation](https://docs.nginx.com/waf/logging/access-logs/).
- [NGINX Ingress Controller logging](https://docs.nginx.com/nginx-ingress-controller/logging-and-monitoring/logging/).
- [Vector `parse_nginx_log`](https://vector.dev/docs/reference/vrl/functions/) documents `combined`, `ingress_upstreaminfo`, `main`, and `error` variants.

Requirements:

- Treat Nginx as first-class beside Apache, not as an Apache-compatible afterthought.
- Parser variants: `nginx_combined`, `nginx_error`, `nginx_ingress_upstreaminfo`, later custom `log_format`.
- Separate client IP from trusted original client IP. Do not promote forwarded headers without trust configuration.
- Preserve upstream timing/status fields for search and performance diagnostics.

### 5.6 JSON Lines / NDJSON

Examples:

```json
{"time":"2026-03-10T23:30:31Z","level":"warn","event":"auth_failed","user":"root","src_ip":"203.0.113.9"}
```

Requirements:

- One JSON object per line for file/network line input.
- HEC `/event` is not the same thing: HEC supports stacked JSON envelopes, not necessarily newline-delimited JSON.
- Keep original JSON type where practical; stringifying objects loses query semantics.
- Bound maximum object depth, string length, field count, and decoded bytes.
- Store parse error reason with byte offset when available.

### 5.7 logfmt

Example:

```text
time=2026-03-10T23:30:31Z level=info msg="login accepted" user=walter src_ip=198.51.100.7 ok=true
```

Requirements:

- Boolean standalone keys should be represented explicitly, not dropped.
- Quoted values may contain spaces and escaped quotes.
- Duplicate keys require policy: keep first, keep last, or preserve list.
- logfmt is a good fit for Go services and some Kubernetes/controller logs.

### 5.8 Kubernetes CRI / Container Logs

Common CRI line shape:

```text
2026-03-10T23:30:31.123456789Z stdout F application log payload
2026-03-10T23:30:31.123456789Z stderr P partial payload chunk
```

Requirements:

- Parse CRI prefix into container timestamp, stream, and partial/full marker.
- Reassemble partial (`P`) records when configured.
- Dispatch inner payload to JSON/logfmt/text parser after CRI framing.
- Preserve Kubernetes metadata supplied externally by shipper or file path: namespace, pod, container, node, labels.

### 5.9 Cloud And Identity JSON Logs

Examples include AWS CloudTrail, Azure Activity Logs, Azure Sign-in Logs, GCP Audit Logs, Okta System Log, GitHub audit, and M365 audit.

Requirements:

- Treat as structured JSON with source-specific schema/version.
- Preserve unknown fields; cloud providers add fields over time.
- Use source-specific time and identity fields for canonical aliases.
- Prefer schema-aware extraction over flatten-everything for hot fields.

### 5.10 Network, Proxy, Firewall, DNS, Zeek

Requirements:

- Prioritize DNS/proxy/firewall where Sigma categories and local logs justify it.
- Zeek may be TSV-like or JSON depending on configuration; parse variant must be explicit.
- Network logs frequently encode source/destination as pairs; normalization must preserve direction.
- Field aliases should support Splunk CIM-ish, Sigma, ECS, and OTel views without physically duplicating every field.

---

## 6. Parser Technique Selection

| Technique | Best For | Avoid For | Notes |
|---|---|---|---|
| `memchr` / byte scanning | LF/CRLF/NUL/sentinel detection, fixed delimiters | nested or quoted grammars | fastest stage; useful before UTF-8 assumptions |
| hand-written finite parser | syslog prefix, CLF/combined, CRI, audit header | arbitrary custom formats | highest control and predictable errors |
| `aho-corasick` | many keywords, Sigma prefilters, suspicious URI tokens | field extraction requiring structure | fast prefilter before regex or parser |
| Rust `regex` | untrusted extraction and filters | backreferences/lookaround compatibility | linear-time, no catastrophic backtracking by design |
| `regex-automata` | compiled regex sets, DFA/NFA control | simple one-off extraction | useful for parser dispatch or many-rule search |
| structured decoder | JSON, logfmt, CSV/TSV | malformed free text | preserve types; bound depth/field count |
| optional PCRE-like engine | compatibility mode for lookaround/backrefs | default ingest path | must be isolated with time/input limits |

Default policy:

1. Use byte-level framing first.
2. Use format-specific parser when format is known or confidently classified.
3. Use Rust `regex` for untrusted pattern extraction where grammar is still regular.
4. Use keyword prefilters before expensive extraction in Sigma/search paths.
5. Keep compatibility-only regex engines out of default ingest.

---

## 7. Exception Handling And Malicious Input

Parser output must distinguish these states:

| State | Meaning | Storage Policy |
|---|---|---|
| `matched` | parser recognized the format and extracted required fields | store raw + normalized fields |
| `partial` | outer framing matched, inner message unsupported | store raw + base fields + reason |
| `malformed` | claimed format but syntax invalid | store raw if allowed, report reason |
| `oversize` | line/event exceeds configured cap | configurable reject, truncate, or quarantine |
| `unsupported` | no parser for format | store raw with sourcetype/source evidence |
| `hostile` | suspicious resource abuse pattern | report, rate-limit/quarantine according to policy |

Attack and stress cases:

- extremely long line with no delimiter;
- many tiny lines to increase per-event overhead;
- invalid UTF-8, NUL bytes, CR-only, mixed CRLF/LF;
- deeply nested JSON or massive single field;
- gzip expansion bomb before parser;
- regex-like user patterns designed for ReDoS, mitigated by Rust `regex` in default path;
- audit event groups never completed;
- multiline stack trace with no new event boundary;
- web log quoted field never closed;
- forged `X-Forwarded-For` or invalid IP literal;
- duplicated fields and conflicting aliases.

Required parser diagnostics:

- `parser_family`;
- `parser_variant`;
- `parser_version`;
- `parse_status`;
- `parse_reason`;
- `raw_len`;
- `field_count`;
- optional `line_number` or chunk offset for file validation;
- source metadata: `source`, `sourcetype`, `host`, `input_kind`.

---

## 8. Field Views, Aliases, And Canonical Records

Canonical storage should be stable and technical. Views should translate for ecosystems.

| Concept | Canonical Candidate | Splunk/CIM View | ECS View | OTel View | Notes |
|---|---|---|---|---|---|
| raw event | `_raw` | `_raw` | `event.original` | `Body` or `log.record.original` | preserve raw bytes or reversible text |
| event time | `_time` | `_time` | `@timestamp` | `Timestamp` | precision decision must be explicit |
| ingest time | `_indextime` | `_indextime` | `event.ingested` | observed timestamp | useful for latency and replay |
| source host | `host` | `host` | `host.name` | `host.name` | HEC `host` may differ from parsed host |
| client IP | `client_ip` | `clientip` or `src` | `client.ip` / `source.ip` | `client.address` | source/direction matters |
| source IP | `src_ip` | `src` | `source.ip` | `network.peer.address` | use for network/security events |
| destination IP | `dst_ip` | `dest` | `destination.ip` | `network.local.address` | use for network/security events |
| user | `user` | `user` | `user.name` | `enduser.id` | preserve original field too if different |
| process | `process` | `process` | `process.name` | `process.executable.name` | split executable path separately |
| pid | `pid` | `pid` | `process.pid` | `process.pid` | integer |
| HTTP method | `http_method` | `http_method` | `http.request.method` | `http.request.method` | derived from request line |
| HTTP status | `http_status` | `status` | `http.response.status_code` | `http.response.status_code` | numeric |
| URI | `uri` | `uri` | `url.original` | `url.full` | preserve original encoding |

Alias policy:

- Canonical fields are used internally.
- Original source field names are preserved when possible for evidence and debugging.
- Splunk/CIM, Sigma, ECS, and OTel views are mappings, not separate storage schemas.
- A mapping must state whether a field is native, derived, inferred, or unavailable.

---

## 9. Parser Validation And Format Benchmarks

Validation here proves parser behavior, not network delivery, queue behavior, or durable storage. Each parser should have fixtures for valid, invalid, edge, and hostile records.

| Parser Family | Required Validation Cases |
|---|---|
| syslog/auth | RFC 3164 timestamp, ISO timestamp, missing PID, unmatched sshd message, OpenSSH 6.x and 7.x disconnect forms |
| auditd | `SYSCALL`, `PATH`, `EXECVE`, duplicate `msg`, quoted values, incomplete serial group |
| Apache access | common, combined, missing request, malformed request, quoted user-agent with spaces, `-` null fields |
| Apache error | with/without client, AH code, module/severity, continuation line |
| Nginx access/error/ingress | combined, error, ingress upstream timing/status, missing upstream fields, forged forwarded headers as data |
| JSON Lines | valid object, non-object value, nested object, huge field, malformed UTF-8 path where applicable |
| logfmt | quoted value, escaped quote, duplicate key, standalone boolean key |
| Kubernetes CRI | stdout/stderr, full/partial marker, nanosecond timestamp, inner JSON/logfmt/text dispatch |

Parser microbenchmarks should report:

- bytes/sec;
- records/sec;
- matched, partial, malformed, unsupported counts;
- allocation count if measured;
- parser variant and parser version;
- scalar versus optimized implementation agreement.

The parser benchmark input is a prepared buffer or file slice. It should not include HTTP, socket, queue, sink, or durable commit cost.

## 10. Format Outputs Required By Downstream Processing

Parser output must be precise enough for later storage and search work without forcing that work into the parser.

Required parser output fields:

- parser family and variant;
- parser version;
- parse status and reason;
- canonical fields extracted from the record;
- original source-specific field names where useful for evidence;
- raw record reference or raw text;
- byte length and field count;
- malformed/unsupported reason when applicable.

Format-specific parsers should not choose queue topology, block size, durable commit policy, retention policy, or index layout. They should expose enough structured output for those stages to make explicit choices later.

Open format gaps:

| Area | Gap |
|---|---|
| Nginx ingress | exact controller/version examples and field variants need expansion |
| Apache custom `LogFormat` | classifier and configured-format parser remain design work |
| auditd grouping | multi-record correlation policy and timeout remain open |
| Kubernetes metadata | file path versus shipper metadata precedence remains open |
| cloud JSON | source-specific schema/version examples are still thin |
| Zeek/proxy/firewall | concrete parser priorities need local corpus evidence |
| multiline | Java/Python/Apache continuation grouping remains parser-boundary work |

## 11. References

Local:

- `/Users/walter/Work/Spank/spank-py/Logs.md` — prior format landscape, syslog/auth.log validation, Apache access inventory, parser dispatch alternatives, Vector coverage model, and normalization comparison.
- `/Users/walter/Work/Spank/sOSS/sigma` — local Sigma rule corpus used for logsource counts.
- `/Users/walter/Work/Spank/sOSS/vector` — local Vector implementation and docs.

External:

- [Splunk: Why source types matter](https://help.splunk.com/en/splunk-enterprise/get-started/get-data-in/9.2/configure-source-types/why-source-types-matter).
- [Splunk `props.conf`](https://docs.splunk.com/Documentation/Splunk/latest/admin/propsconf).
- [Sigma logsources](https://sigmahq.io/docs/basics/log-sources.html).
- [Sigma taxonomy](https://sigmahq.io/sigma-specification/specification/sigma-appendix-taxonomy.html).
- [Vector VRL functions](https://vector.dev/docs/reference/vrl/functions/).
- [Vector file source](https://vector.dev/docs/reference/configuration/sources/file/).
- [Apache `mod_log_config`](https://httpd.apache.org/docs/current/en/mod/mod_log_config.html).
- [Apache log files](https://httpd.apache.org/docs/current/logs.html).
- [NGINX access logs](https://docs.nginx.com/waf/logging/access-logs/).
- [NGINX Ingress Controller logging](https://docs.nginx.com/nginx-ingress-controller/logging-and-monitoring/logging/).
- [RFC 5424 — The Syslog Protocol](https://www.rfc-editor.org/rfc/rfc5424.html).
