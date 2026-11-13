"""Correlate burst/recovery tails from the same source-to-completion rows."""
import json
import math
from pathlib import Path
import sys


def dist(values):
    values = sorted(values)
    if not values:
        return {"count": 0}
    assert all(math.isfinite(v) and v >= 0 for v in values)
    return {"count": len(values), **{name: values[math.ceil(q * len(values)) - 1]
            for name, q in (("p50_us", .5), ("p95_us", .95), ("p99_us", .99), ("max_us", 1))}}


def analyze(root):
    results = []
    for run in sorted(root.glob("run-*")):
        for case in json.loads((run / "cadence.json").read_text(encoding="utf-8"))["cases"]:
            assert case["burst"] and case["pixel_equal"]
            rows = case["raw"]
            assert len(rows) + case["superseded_ticks"] == case["expected_ticks"]
            hz = case["target_hz"]
            half = hz // 2
            samples = []
            for row in rows:
                assert row["burst_epoch"] == row["tick"] // half
                owner = sum(row[p] for p in ("commit_us", "publication_queue_us", "submit_us"))
                rest = row["source_to_gpu_complete_us"] - row["source_to_owner_us"] - owner
                assert rest >= -.001
                samples.append({**row, "owner_us": owner, "after_submit_us": max(0, rest)})
            recovery = []
            for epoch in range(1, (case["expected_ticks"] - 1) // half + 1):
                tick = epoch * half
                # First on-time source tick following this burst, before the
                # next burst. Missing ticks count as unsatisfied throughout.
                observed = [r for r in samples if tick <= r["tick"] < tick + half]
                good = next((r for r in observed if not r["deadline_missed"]), None)
                recovery.append({"epoch": epoch, "scheduled_tick": tick,
                    "first_consumed_tick": observed[0]["tick"] if observed else None,
                    "first_ontime_tick": good["tick"] if good else None,
                    "unsatisfied_ticks_before_recovery": good["tick"] - tick if good else half,
                    "recovery_completion_age_us":
                        (good["tick"] - tick) * 1e6 / hz + good["source_to_gpu_complete_us"] if good else None})
            groups = {}
            for label, selected in (("all", samples),
                                    ("dense_change", [r for r in samples if r["burst_applied"]]),
                                    ("ordinary", [r for r in samples if not r["burst_applied"]])):
                groups[label] = {field: dist(r[field] for r in selected) for field in
                                ("source_to_gpu_complete_us", "owner_us", "commit_us",
                                 "publication_queue_us", "submit_us", "after_submit_us")}
            results.append({"run": run.name, "hz": hz, "groups": groups,
                **{k: case[k] for k in ("expected_ticks", "consumed_ticks", "superseded_ticks", "deadline_misses",
                                      "unsatisfied_ticks", "longest_consecutive_misses", "pixel_equal")},
                "dense_changes_consumed": sum(r["burst_applied"] for r in rows),
                "dense_epochs_crossed": sum(r["burst_epochs_crossed"] for r in rows),
                "recovery": recovery, "worst_completion_rows": sorted(samples,
                    key=lambda r: r["source_to_gpu_complete_us"], reverse=True)[:10]})
    assert len(results) in (8, 12)
    return {"scope": "same-sample monotonic phases; recovery is first on-time completion before next burst, not display or physical input; missing ticks stay unsatisfied", "cases": results}


if __name__ == "__main__":
    root = Path(sys.argv[1])
    result = analyze(root)
    (root / "summary.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    for case in result["cases"]:
        print(case["run"], case["hz"], "p99/max ms",
              *[round(case["groups"]["all"]["source_to_gpu_complete_us"][k] / 1000, 2)
                for k in ("p99_us", "max_us")], "loss", case["unsatisfied_ticks"],
              "burst", case["longest_consecutive_misses"])
