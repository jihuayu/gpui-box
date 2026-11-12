"""Recompute all summaries from the frozen, unfiltered export CSV files."""
from pathlib import Path
import csv
import json
import math
import statistics

root = Path(__file__).parent
runs = [list(csv.DictReader((root / f"run-{i}" / "export-benchmark.csv").open()))
        for i in range(1, 4)]
assert all(len(rows) == 480 for rows in runs)
summary = {"units": "microseconds", "p95_method": "nearest rank, ceil(0.95 * n)",
           "scope": "owner commit/publication/submission plus completed export; native drawing/presentation/sprite upload excluded",
           "filtered_samples": 0, "cases": []}
for width, height in [(128, 96), (800, 600), (1600, 900)]:
    case = {"size": [width, height], "modes": {}}
    for mode in ["gpu", "readback"]:
        parts = [[row for row in rows if int(row["width"]) == width
                  and int(row["height"]) == height and row["mode"] == mode] for rows in runs]
        assert all(len(part) == 80 for part in parts)
        metrics = {}
        for field in ["commit_submit_us", "export_complete_us", "total_us"]:
            values = [float(row[field]) for part in parts for row in part]
            metrics[field] = {
                "samples": len(values), "median": statistics.median(values),
                "p95": sorted(values)[math.ceil(0.95 * len(values)) - 1], "maximum": max(values),
                "execution_medians": [statistics.median(float(row[field]) for row in part) for part in parts],
                "round_medians": [[statistics.median(float(row[field]) for row in part
                                   if int(row["round"]) == r) for r in range(4)] for part in parts],
            }
        case["modes"][mode] = metrics
    case["total_median_change_percent"] = (case["modes"]["gpu"]["total_us"]["median"]
                                          / case["modes"]["readback"]["total_us"]["median"] - 1) * 100
    summary["cases"].append(case)
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
