"""Summarize raw CPU cadence captures without conflating responsiveness/fps."""
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
rows = []
for path in sorted(root.glob("run-*/cadence.json")):
    for case in json.loads(path.read_text(encoding="utf-8-sig")):
        assert case["deadline_met"] + case["unsatisfied_ticks"] == case["source_ticks"]
        assert case["consumed"] + case["coalesced_samples"] == case["source_ticks"]
        assert case["probe_arrivals_handled"] == case["source_ticks"]
        assert len(case["completion_samples"]) == case["consumed"]
        rows.append({
            "run": path.parent.name,
            **{key: case[key] for key in ["effects", "hz", "quota", "source_ticks", "consumed", "deadline_met", "unsatisfied_ticks", "longest_unsatisfied_burst"]},
            **{key: case[key] for key in ["slice", "worker_init", "worker_retire", "synthetic_probe_oldest_latency", "source_to_cpu_completion"]},
        })
result = {"cases": len(rows), "rows": rows,
          "scope": "CPU pixels and synthetic event service only; prepared packets; no native input/upload/present", "goal_complete": False}
(root / "summary.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(f"{len(rows)} CPU cases accounted; no visible-frame qualification implied")
