"""Validate every target tick and retain paired tails, including losing modes."""
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
summary = []
for run in sorted(root.glob("run-*")):
    if not run.is_dir():
        continue
    data = json.loads((run / "pipeline.json").read_text(encoding="utf-8"))
    for case in data["cases"]:
        rows = case["raw"]
        next_tick = skipped = missed = burst = longest = 0
        serial = 0
        for row in rows:
            assert row["tick"] >= next_tick
            assert row["superseded_ticks"] == row["tick"] - next_tick
            assert row["serial"] >= serial
            assert row["covering_serial"] >= row["serial"]
            assert row["source_to_owner_completion_observation_us"] >= row["source_to_completion_upper_bound_us"]
            late = row["source_to_completion_upper_bound_us"] > 1e6 / case["target_hz"]
            assert late == row["deadline_missed"]
            skipped += row["superseded_ticks"]
            burst += row["superseded_ticks"]
            longest = max(longest, burst)
            if late:
                missed += 1
                burst += 1
                longest = max(longest, burst)
            else:
                burst = 0
            next_tick = row["tick"] + 1
            serial = row["serial"]
        assert next_tick == case["expected_ticks"]
        assert len(rows) + skipped == case["expected_ticks"]
        assert skipped == case["superseded_ticks"]
        assert missed == case["deadline_misses_upper_bound"]
        assert longest == case["longest_consecutive_misses_upper_bound"]
        assert skipped + missed == case["unsatisfied_ticks_upper_bound"]
        assert case["maximum_gpu_in_flight"] <= case["max_in_flight"]
        assert case["leases_retired"] and case["pixel_equal"]
        summary.append({"run": run.name, **{k: v for k, v in case.items() if k != "raw"}})
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print("run nodes batch effects hz before: p95/p99/max ms; skips/misses/burst; waits")
for r in summary:
    d = r["source_to_completion_upper_bound"]
    print(r["run"], r["nodes"], r["batch"], r["effects"], r["target_hz"], r["admission_before_work"],
          *[round(d[k] / 1000, 3) for k in ("p95_us", "p99_us", "max_us")],
          r["superseded_ticks"], r["deadline_misses_upper_bound"],
          r["longest_consecutive_misses_upper_bound"], r["admission_waits"])
