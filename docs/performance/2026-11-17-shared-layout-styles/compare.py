"""Alternate exact baseline/shared release test executables; retain all output."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess
import time
import os

parser = argparse.ArgumentParser()
parser.add_argument("baseline", type=Path)
parser.add_argument("shared", type=Path)
parser.add_argument("--samples", type=int, default=200)
parser.add_argument("--rounds", type=int, default=4)
parser.add_argument("--out", type=Path, default=Path(__file__).parent)
args = parser.parse_args()
assert args.samples >= 20 and args.rounds >= 2
args.out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, GPUI_STYLE_SAMPLES=str(args.samples))
rows = []
executables = {"baseline": args.baseline.resolve(), "shared": args.shared.resolve()}
metadata = {
    "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    "platform": platform.platform(),
    "processor": platform.processor(),
    "samples": args.samples,
    "rounds": args.rounds,
    "units": "microseconds",
    "executables": {
        label: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        for label, path in executables.items()
    },
}
for round_index in range(args.rounds):
    order = ["baseline", "shared"] if round_index % 2 == 0 else ["shared", "baseline"]
    for label in order:
        result = subprocess.run(
            [str(executables[label]), "style_snapshot_benchmark", "--ignored", "--nocapture", "--test-threads=1"],
            env=env, text=True, encoding="utf-8", capture_output=True, check=True,
        )
        (args.out / f"{round_index + 1}-{label}.txt").write_text(
            (result.stdout + result.stderr).rstrip() + "\n", encoding="utf-8"
        )
        for line in result.stdout.splitlines():
            if "STYLE_BENCH," not in line:
                continue
            fields = line[line.index("STYLE_BENCH,"):].split(",")
            _, name, count, p50, p95, maximum = fields
            # The first baseline executable used an inaccurate label for the
            # same isolated root-measure call; it never measured live layout.
            name = name.replace("_relayout", "_dry_root")
            rows.append([round_index + 1, label, name, int(count), float(p50), float(p95), float(maximum)])
        print(f"completed round {round_index + 1}: {label}", flush=True)
with (args.out / "samples.csv").open("w", newline="", encoding="utf-8") as stream:
    writer = csv.writer(stream)
    writer.writerow(["round", "variant", "operation", "samples", "p50_us", "p95_us", "max_us"])
    writer.writerows(rows)
summary = []
for name in dict.fromkeys(row[2] for row in rows):
    entry = {"operation": name}
    for label in executables:
        selected = [row for row in rows if row[1] == label and row[2] == name]
        assert len(selected) == args.rounds
        entry[label] = {
            "median_p50_us": statistics.median(row[4] for row in selected),
            "median_p95_us": statistics.median(row[5] for row in selected),
            "p50_min_us": min(row[4] for row in selected),
            "p50_max_us": max(row[4] for row in selected),
        }
    entry["median_p50_reduction_percent"] = 100 * (
        1 - entry["shared"]["median_p50_us"] / entry["baseline"]["median_p50_us"]
    )
    summary.append(entry)
(args.out / "metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
(args.out / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
print(json.dumps(summary, indent=2))
