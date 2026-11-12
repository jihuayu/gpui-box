"""Derive same-sample owner budget and completion residual; never add percentiles.

Use only completed cadence captures. This does not measure GPU execution time:
the residual includes synchronous finish/driver/scheduling and prior queue work.
"""
import json
import math
from pathlib import Path
import sys


def distribution(values):
    values = sorted(values)
    assert values and all(math.isfinite(v) and v >= 0 for v in values)
    return {"samples": len(values), **{
        name: values[min(len(values) - 1, math.ceil(fraction * len(values)) - 1)]
        for name, fraction in (("p50_us", .50), ("p95_us", .95), ("p99_us", .99), ("max_us", 1.0))
    }}


def analyze(root):
    cases = []
    for run in sorted(root.glob("run-*")):
        if not run.is_dir():
            continue
        data = json.loads((run / "cadence.json").read_text(encoding="utf-8"))
        assert data["synchronous_completion_per_frame"]
        for case in data["cases"]:
            owner, submit_age, residual = [], [], []
            for row in case["raw"]:
                work = sum(row[p] for p in ("commit_us", "publication_queue_us", "submit_us"))
                age = row["source_to_owner_us"] + work
                rest = row["source_to_gpu_complete_us"] - age
                # All time spans come from the same monotonic-clock sample.
                assert rest >= -0.001, (run, row["tick"], rest)
                owner.append(work)
                submit_age.append(age)
                residual.append(max(0, rest))
            assert len(owner) + case["superseded_ticks"] == case["expected_ticks"]
            budget = case["budget_us"]
            owner_late = sum(v > budget for v in owner)
            pre_finish_late = sum(v > budget for v in submit_age)
            assert owner_late <= pre_finish_late <= case["deadline_misses"]
            cases.append({
                "run": run.name,
                **{k: case[k] for k in ("nodes", "batch", "effects", "target_hz", "budget_us", "expected_ticks", "consumed_ticks", "superseded_ticks", "deadline_misses")},
                "owner_work": distribution(owner),
                "source_to_submit": distribution(submit_age),
                "post_submit_finish_residual": distribution(residual),
                "owner_work_over_budget": owner_late,
                "already_late_before_finish": pre_finish_late,
                # Preserve correlated causes of the worst completion samples;
                # separate phase p99 values may belong to different frames.
                "worst_completion_samples": [
                    {
                        "tick": row["tick"],
                        "source_to_gpu_complete_us": row["source_to_gpu_complete_us"],
                        "source_to_owner_us": row["source_to_owner_us"],
                        "commit_us": row["commit_us"],
                        "publication_queue_us": row["publication_queue_us"],
                        "submit_us": row["submit_us"],
                        "owner_work_us": owner[i],
                        "post_submit_finish_residual_us": residual[i],
                        "superseded_ticks": row["superseded_ticks"],
                    }
                    for i, row in sorted(enumerate(case["raw"]),
                                         key=lambda pair: pair[1]["source_to_gpu_complete_us"],
                                         reverse=True)[:10]
                ],
            })
    assert cases
    return {
        "scope": "same-row sums of owner commit/publication/submission; residual is synchronous finish and scheduling, not GPU time; no native display or physical input; skipped ticks remain separate",
        "cases": cases,
    }


if __name__ == "__main__":
    root = Path(sys.argv[1])
    result = analyze(root)
    (root / "frame-budget.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print("run nodes independent effects hz: owner p50/p99/max ms; overbudget; late before finish")
    for case in result["cases"]:
        d = case["owner_work"]
        print(case["run"], case["nodes"], case["batch"], case["effects"], case["target_hz"],
              *[round(d[k] / 1000, 3) for k in ("p50_us", "p99_us", "max_us")],
              case["owner_work_over_budget"], case["already_late_before_finish"])
