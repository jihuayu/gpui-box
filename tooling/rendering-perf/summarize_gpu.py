"""Verify GPU raw samples and derive nearest-rank per-round summaries."""
import csv
import json
import math
from pathlib import Path
import statistics
import sys

directory = Path(sys.argv[1])
adapter = json.loads((directory / "adapter.json").read_text())
with (directory / "samples.csv").open(newline="") as stream:
    samples = list(csv.DictReader(stream))
expected_rows = 3 * 3 * adapter["rounds"] * adapter["samples"]
assert len(samples) == expected_rows, (len(samples), expected_rows)
validation = json.loads((directory / "validation.json").read_text())
assert len(validation) == 18
assert all(row["pixel_equal"] and row["changed_pixels"] == 9 for row in validation)
assert adapter["device_type"] != "Cpu"
metrics = ("upload_ns", "submit_ns", "cpu_ns", "completion_ns", "gpu_draw_ns")
summary = []
for nodes in (300, 3000, 30000):
    for round_index in range(adapter["rounds"]):
        for path in ("full", "retained_full", "patch"):
            group = [
                row for row in samples
                if (int(row["nodes"]), int(row["round"]), row["path"])
                == (nodes, round_index, path)
            ]
            assert len(group) == adapter["samples"]
            assert sorted(int(row["sample"]) for row in group) == list(range(adapter["samples"]))
            expected_bytes = 160 if path == "patch" else nodes * 160
            assert all(int(row["instance_bytes"]) == expected_bytes for row in group)
            assert all(int(row["instance_writes"]) == 1 for row in group)
            result = dict(nodes=nodes, round=round_index, path=path, instance_bytes=expected_bytes)
            for metric in metrics:
                values = sorted(float(row[metric]) for row in group if row[metric])
                for percentile in (50, 95, 99):
                    key = f"{metric}_p{percentile}"
                    result[key] = values[math.ceil(len(values)*percentile/100)-1] if values else ""
            summary.append(result)
with (directory / "summary.csv").open("w", newline="") as stream:
    writer = csv.DictWriter(stream, fieldnames=list(summary[0]))
    writer.writeheader()
    writer.writerows(summary)
print(f"Verified {len(samples)} hardware samples and 18 pixel comparisons")
print("Median of round-level p50, microseconds")
for nodes in (300, 3000, 30000):
    for path in ("full", "retained_full", "patch"):
        group = [row for row in summary if row["nodes"] == nodes and row["path"] == path]
        values = [statistics.median(float(row[f"{metric}_p50"] or float("nan")) for row in group)/1000 for metric in metrics]
        print(nodes, path, " ".join(f"{metric}={value:.2f}" for metric, value in zip(metrics, values)))
