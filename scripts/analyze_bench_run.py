#!/usr/bin/env python3
import json
import pathlib
import re
import sys
from typing import Any


def load_json(path: pathlib.Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    return json.loads(path.read_text())


def delta(before: dict[str, Any], after: dict[str, Any]) -> dict[str, int | float]:
    out: dict[str, int | float] = {}
    for key, value in after.items():
        if isinstance(value, (int, float)) and isinstance(before.get(key), (int, float)):
            out[key] = value - before[key]
    return out


def ab_metrics(path: pathlib.Path) -> dict[str, float | int | str]:
    text = path.read_text(errors="replace") if path.exists() else ""
    patterns = {
        "complete_requests": r"Complete requests:\s+(\d+)",
        "failed_requests": r"Failed requests:\s+(\d+)",
        "requests_per_sec": r"Requests per second:\s+([0-9.]+)",
        "seconds": r"Time taken for tests:\s+([0-9.]+)",
        "body_sent_bytes": r"Total body sent:\s+(\d+)",
    }
    out: dict[str, float | int | str] = {"file": path.name}
    for key, pat in patterns.items():
        m = re.search(pat, text)
        if not m:
            continue
        raw = m.group(1)
        out[key] = float(raw) if "." in raw else int(raw)
    return out


def summarize_stage(name: str, before: dict[str, Any], after: dict[str, Any], seconds: float | None) -> dict[str, Any]:
    d = delta(before, after)
    summary: dict[str, Any] = {"stage": name, "delta": d}
    if seconds and seconds > 0:
        summary["receiver_requests_per_sec"] = d.get("requests_ok", 0) / seconds
        summary["receiver_wire_mib_per_sec"] = d.get("wire_bytes", 0) / (1024 * 1024) / seconds
        summary["receiver_events_per_sec"] = d.get("events_observed", 0) / seconds
    return summary


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: analyze_bench_run.py RUN_DIR", file=sys.stderr)
        return 2
    run = pathlib.Path(sys.argv[1])
    before = load_json(run / "stats-before.json")
    c1 = load_json(run / "stats-after-c1.json")
    cN = load_json(run / "stats-after-cN.json") or load_json(run / "stats-after-c16.json")
    ab_c1_files = sorted(run.glob("ab-*-c1.txt"))
    ab_cn_files = sorted(path for path in run.glob("ab-*.txt") if "-c1." not in path.name)
    ab_files = ab_c1_files[:1] + ab_cn_files[:1] + [
        path for path in sorted(run.glob("ab-*.txt")) if path not in set(ab_c1_files[:1] + ab_cn_files[:1])
    ]
    ab = [ab_metrics(path) for path in ab_files]

    stages = []
    if ab:
        stages.append(summarize_stage("c1", before, c1, float(ab[0].get("seconds", 0) or 0)))
    if len(ab) > 1:
        stages.append(summarize_stage("cN", c1, cN, float(ab[1].get("seconds", 0) or 0)))

    failed_facts = 0
    body_read_failed = 0
    stderr = run / "server.stderr"
    if stderr.exists():
        for line in stderr.read_text(errors="replace").splitlines():
            if "hec.request.failed" in line:
                failed_facts += 1
            if "hec.body.read_failed" in line:
                body_read_failed += 1

    result = {
        "run_dir": str(run),
        "ab": ab,
        "stages": stages,
        "stats_final": cN or c1 or before,
        "logged_request_failed_facts": failed_facts,
        "logged_body_read_failed_facts": body_read_failed,
    }
    print(json.dumps(result, indent=2, sort_keys=True))
    (run / "summary.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
