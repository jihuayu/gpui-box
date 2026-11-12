"""Summarize paired and sustained raw samples without dropping any outliers."""
from pathlib import Path
import json
import sys

root = Path(sys.argv[1])
summary = {"paired": [], "cadence": []}
for run in sorted(root.glob("run-*")):
    if not run.is_dir():
        continue
    paired = json.loads((run / "paired.json").read_text())
    for case in paired["cases"]:
        optimized, control = case["modes"]
        row = {"run": run.name, "effects": case["effects"], "batch": case["batch"]}
        for phase in ("commit", "publication", "submission", "completed"):
            row[phase] = {"optimized": optimized[phase], "control": control[phase],
                          "p50_reduction_percent": 100 * (1 - optimized[phase]["p50_us"] / control[phase]["p50_us"])}
        row["work"] = optimized["work"]
        row["pixel_equal"] = case["pixel_equal"]
        summary["paired"].append(row)
    cadence = json.loads((run / "cadence.json").read_text())
    for case in cadence["cases"]:
        rows = case["raw"]
        assert len(rows) + case["superseded_ticks"] == case["expected_ticks"]
        assert sum(row["deadline_missed"] for row in rows) == case["deadline_misses"]
        # Include superseded target ticks when checking consecutive stalls.
        burst = maximum = 0
        for row in rows:
            burst += row["superseded_ticks"]
            maximum = max(maximum, burst)
            if row["deadline_missed"]:
                burst += 1
                maximum = max(maximum, burst)
            else:
                burst = 0
        assert maximum == case["longest_consecutive_misses"]
        summary["cadence"].append({"run": run.name, **{k:v for k,v in case.items() if k != "raw"}})
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print("Paired: publication p50 / completed p50 reduction (%)")
for row in summary["paired"]:
    print(row["run"], "effects", row["effects"], "batch", row["batch"],
          round(row["publication"]["p50_reduction_percent"], 1),
          round(row["completed"]["p50_reduction_percent"], 1))
print("Cadence: p95/p99/max ms, skipped, misses, burst")
for row in summary["cadence"]:
    d = row["source_to_gpu_complete"]
    print(row["run"], row["nodes"], row["effects"], row["batch"], row["target_hz"],
          *[round(d[k]/1000, 3) for k in ("p95_us", "p99_us", "max_us")],
          row["superseded_ticks"], row["deadline_misses"], row["longest_consecutive_misses"])
