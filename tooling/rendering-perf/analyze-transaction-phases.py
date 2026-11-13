"""Attribute diagnostic transaction wall spans within strictly checked pipeline rows."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("pipeline_analysis", Path(__file__).with_name("analyze-pipeline-burst.py"))
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)
REACTIVE = ("settlement", "sink_collection", "tree_commit", "publication_effects")
PROPERTY = ("preflight", "application")


def analyze(root):
    base = pipeline.analyze([root], 16, True, "instrumented")
    cases = []
    for case in base["cases"]:
        raw = next(c for c in json.loads((Path(case["run"]) / "pipeline.json").read_text())["cases"]
                   if c["target_hz"] == case["hz"] and c["admission_before_work"] == case["before_work"])
        instrumented = Path(case["run"]).name.endswith("instrumented")
        measured = []
        for row in raw["raw"]:
            diagnostic = row.get("transaction_diagnostics")
            assert bool(diagnostic) == instrumented
            if not diagnostic:
                continue
            reactive = diagnostic["reactive_us"]
            prop = diagnostic["property_us"]
            assert len(reactive) == 4 and len(prop) == 2
            assert all(v >= 0 for v in reactive + prop)
            assert sum(prop) <= reactive[2] + .001
            assert sum(reactive) <= row["commit_us"] + .001
            assert diagnostic["effect_runs"] == 0
            assert diagnostic["sink_writes"] == diagnostic["changed_nodes"]
            # Source delivery contains one visual source and its derived values;
            # every retained dense epoch updates all10k leaves atomically.
            if row["burst_epochs_crossed"]:
                assert diagnostic["sink_writes"] >= 10000
            measured.append({"tick": row["tick"], "dense": bool(row["burst_epochs_crossed"]),
                "completion_us": row["source_to_completion_upper_bound_us"],
                **{name + "_us": v for name, v in zip(REACTIVE, reactive)},
                **{name + "_us": v for name, v in zip(PROPERTY, prop)},
                "property_residual_us": max(0, reactive[2] - sum(prop)),
                "delivery_residual_us": max(0, row["commit_us"] - sum(reactive)),
                "commit_us": row["commit_us"], "counters": diagnostic})
        if instrumented:
            assert len(measured) == len(raw["raw"])
            assert sum(r["dense"] for r in measured) == 19
        groups = {}
        for name, rows in (("all", measured), ("dense", [r for r in measured if r["dense"]]),
                           ("ordinary", [r for r in measured if not r["dense"]])):
            groups[name] = {key: pipeline.dist(r[key] for r in rows)
                            for key in measured[0] if key.endswith("_us")} if measured else {}
        cases.append({"run": case["run"], "hz": case["hz"], "before_work": case["before_work"],
            "groups": groups, "measured_rows": len(measured),
            "worst_commit_rows": sorted(measured, key=lambda r: r["commit_us"], reverse=True)[:10]})
    return {"scope": "diagnostic wall time including allocations/destruction; observer overhead can perturb scheduling; not pure CPU or GPU/display/input time", "cases": cases}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    options = parser.parse_args()
    result = analyze(options.root)
    (options.root / "transaction-phase-summary.json").write_text(json.dumps(result, indent=2))
    for case in result["cases"]:
        if not case["measured_rows"]:
            continue
        print(case["run"], case["hz"], "before", case["before_work"], "dense medians ms",
              {key: round(value["p50_us"] / 1000, 4) for key, value in case["groups"]["dense"].items()})
