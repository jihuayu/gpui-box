"""Same-sample completed-work distributions; all source losses retained."""
import json
import math
import sys
from pathlib import Path

root = Path(sys.argv[1])

def distribution(values):
    values = sorted(values)
    return {label: values[math.ceil((len(values)-1)*quantile)]
            for label, quantile in [("p50_us", .5), ("p95_us", .95), ("p99_us", .99), ("max_us", 1)]}

cadence = []
paired = []
for path in sorted(root.glob("run-*/translation-cadence.json")):
    for case in json.loads(path.read_text(encoding="utf-8-sig")):
        samples = case["samples"]
        assert len(samples) + case["coalesced"] == case["source_ticks"]
        assert sum(sample["deadline_met"] for sample in samples) == case["deadline_met"]
        cadence.append({"run": path.parent.name,
                        **{key: value for key, value in case.items() if key != "samples"},
                        **{key: distribution([sample[key] for sample in samples])
                           for key in ["completion_us", "owner_submit_us", "source_to_owner_us"]}})
for path in sorted(root.glob("run-*/translation-benchmark.json")):
    samples = json.loads(path.read_text(encoding="utf-8-sig"))
    for round_number in [0, 1]:
        for enabled in [False, True]:
            group = [sample for sample in samples if sample["round"] == round_number and sample["enabled"] == enabled]
            paired.append({"run": path.parent.name, "round": round_number, "enabled": enabled,
                           "rasterized_layers": sum(sample["rasterized_layers"] for sample in group),
                           **{key: distribution([sample[key] for sample in group]) for key in ["owner_submit_us", "completed_us"]}})
result = {"cadence": cadence, "paired": paired, "goal_complete": False,
          "scope": "owned resolved frame + synchronous GPU completion; independent source arrivals; excludes native present and physical input"}
(root / "summary.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")
print(f"{len(cadence)} cadence and {len(paired)} paired groups accounted")
