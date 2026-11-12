"""Compare alternating immutable binary runs; retain every reported tail."""
from pathlib import Path
import json
import sys

root = Path(sys.argv[1])
metadata = json.loads((root / "metadata.json").read_text(encoding="utf-8-sig"))
assert metadata["sources_stable"] and metadata["no_sampled_conflicts"]
rows = []
for run in range(1, metadata["runs"] + 1):
    results = {}
    for mode in ("control", "optimized"):
        data = json.loads((root / f"round-{run}-{mode}" / "paired.json").read_text(encoding="utf-8"))
        assert data["local_snapshot"] and not data["quick"]
        results[mode] = {}
        for case in data["cases"]:
            assert case["pixel_equal"]
            local, full = case["modes"]
            assert not local["reextract"] and full["reextract"]
            assert local["completed"]["samples"] == 120
            results[mode][case["effects"], case["batch"]] = local
    assert results["control"].keys() == results["optimized"].keys()
    for (effects, batch), control in results["control"].items():
        optimized = results["optimized"][effects, batch]
        phases = {}
        for phase in ("commit", "publication", "submission", "completed"):
            phases[phase] = {
                "control": control[phase],
                "optimized": optimized[phase],
                "p50_reduction_percent": 100 * (1 - optimized[phase]["p50_us"] / control[phase]["p50_us"]),
                "p95_reduction_percent": 100 * (1 - optimized[phase]["p95_us"] / control[phase]["p95_us"]),
            }
        rows.append({"round": run, "effects": effects, "batch": batch,
                     "phases": phases, "work_control": control["work"],
                     "work_optimized": optimized["work"], "pixel_equal": True})
(root / "summary.json").write_text(json.dumps({"cases": rows}, indent=2) + "\n", encoding="utf-8")
print("round effects batch: local publication/submission/completed p50 reductions (%)")
for row in rows:
    print(row["round"], row["effects"], row["batch"],
          *[round(row["phases"][p]["p50_reduction_percent"], 1)
            for p in ("publication", "submission", "completed")])
