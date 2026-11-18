"""Alternate two identical-harness release binaries and preserve every sample."""
import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("baseline", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--samples", type=int, default=100)
parser.add_argument("--rounds", type=int, default=4)
args = parser.parse_args()
assert args.samples >= 20 and args.rounds >= 2
args.out.mkdir(parents=True, exist_ok=True)
binaries = {key: value.resolve() for key, value in [("baseline", args.baseline), ("candidate", args.candidate)]}
metadata = {"utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "platform": platform.platform(), "processor": platform.processor(), "samples": args.samples, "rounds": args.rounds, "executables": {key: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()} for key, path in binaries.items()}}
rows = []
for round_index in range(args.rounds):
    order = ["baseline", "candidate"] if round_index % 2 == 0 else ["candidate", "baseline"]
    for label in order:
        result = subprocess.run([str(binaries[label])], env=dict(os.environ, GPUI_PLAN_SAMPLES=str(args.samples)), text=True, encoding="utf-8", capture_output=True, check=True)
        (args.out / f"{round_index+1}-{label}.csv").write_text(result.stdout, encoding="utf-8")
        assert not result.stderr, result.stderr
        process_rows = list(csv.DictReader(io.StringIO(result.stdout)))
        assert len(process_rows) == args.samples * 10
        for row in process_rows:
            rows.append({"round": round_index+1, "variant": label, **row})
        print(f"completed round {round_index+1}: {label}", flush=True)
summary = []
for tree in ["flex", "grid"]:
    for operation in ["single_hover", "dense_hover", "sparse_environment", "metadata_only", "equal_styles"]:
        entry = {"tree": tree, "operation": operation}
        for label in binaries:
            processes=[]
            for round_index in range(args.rounds):
                values=sorted(int(row["nanoseconds"]) / 1000 for row in rows if row["tree"]==tree and row["operation"]==operation and row["variant"]==label and row["round"]==round_index+1)
                assert len(values)==args.samples
                percentile=lambda p: values[max(0, min(len(values)-1, math.ceil(p*len(values))-1))]
                processes.append({"round":round_index+1,"p50_us":percentile(.5),"p95_us":percentile(.95),"p99_us":percentile(.99),"max_us":values[-1]})
            entry[label]={"processes":processes, **{f"median_{key}":statistics.median(process[key] for process in processes) for key in ["p50_us","p95_us","p99_us","max_us"]}}
        entry["p50_reduction_percent"]=100*(1-entry["candidate"]["median_p50_us"]/entry["baseline"]["median_p50_us"])
        summary.append(entry)
(args.out / "metadata.json").write_text(json.dumps(metadata,indent=2),encoding="utf-8")
(args.out / "summary.json").write_text(json.dumps(summary,indent=2),encoding="utf-8")
print(json.dumps(summary,indent=2))
