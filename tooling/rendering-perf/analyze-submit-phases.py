"""Attribute diagnostic owner spans using correlated raw pipeline rows."""
import argparse
import importlib.util
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("pipeline_analysis", Path(__file__).with_name("analyze-pipeline-burst.py"))
pipeline = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pipeline)
LOCAL = ("provenance", "preparation", "batch", "backend", "ack")
PREPARATION = ("material", "spatial", "ink")
MATERIAL_SAMPLE = ("validation", "classification", "spatial_lookup", "primitive", "content")
BACKEND = ("primitive_spatial_validation", "content_layer_validation", "admission",
           "target_setup", "storage_upload", "dependency_invalidation", "content_prepare",
           "layer_encoding", "root_encoding", "queue_submit", "retirement_setup", "return")


def analyze(root, expected_cases):
    # Includes full workload, epochs, pixel/lease, no-op and deadline accounting.
    base = pipeline.analyze([root], expected_cases, True, "instrumented")
    results = []
    for case in base["cases"]:
        raw_case = next(c for c in json.loads((Path(case["run"]) / "pipeline.json").read_text())["cases"]
                        if c["target_hz"] == case["hz"] and c["admission_before_work"] == case["before_work"])
        measured = []
        instrumented = "instrumented" in Path(case["run"]).name
        for row in raw_case["raw"]:
            diagnostic = row.get("diagnostics")
            assert bool(diagnostic) == (instrumented and row["gpu_submitted"])
            if not diagnostic or diagnostic["local_us"] is None:
                continue
            local = diagnostic["local_us"]
            backend = diagnostic["backend_us"]
            assert len(local) == len(LOCAL) and len(backend) == len(BACKEND)
            assert all(p >= 0 for p in local + backend)
            assert sum(backend) <= local[3] + .001
            # Before-work admission is outside the outer submit span; after-work
            # admission is inside it. Both contain the complete local call.
            assert sum(local) <= row["submit_and_admission_us"] + .001
            outer = row["submit_and_admission_us"] - (0 if case["before_work"] else row["admission_wait_us"])
            assert sum(local) <= outer + .001
            preparation = diagnostic.get("preparation_us")
            cleanup = diagnostic.get("cleanup_us")
            detailed = {}
            sample = diagnostic.get("material_sample_us")
            if sample is not None:
                assert len(sample) == len(MATERIAL_SAMPLE) and all(p >= 0 for p in sample)
                count = diagnostic["material_sample_count"]
                assert type(count) is int and 0 <= count <= diagnostic["material_item_count"]
                assert sum(sample) <= preparation[0] + .001
                assert count > 0 or sum(sample) == 0
                detailed.update({f"sample_{name}_us": value for name, value in zip(MATERIAL_SAMPLE, sample)})
            if preparation is not None:
                assert len(preparation) == len(PREPARATION) and all(p >= 0 for p in preparation)
                assert abs(sum(preparation) - local[1]) <= .001
                detailed.update({f"preparation_{name}_us": value for name, value in zip(PREPARATION, preparation)})
            if cleanup is not None:
                assert cleanup >= 0 and sum(local) + cleanup <= outer + .001
                detailed.update({"explicit_cleanup_us": cleanup,
                    "unattributed_after_cleanup_us": max(0, outer - sum(local) - cleanup)})
            values = {**{f"local_{name}_us": value for name, value in zip(LOCAL, local)},
                      **{f"backend_{name}_us": value for name, value in zip(BACKEND, backend)},
                      **detailed,
                      "local_total_us": sum(local), "backend_total_us": sum(backend),
                      "outside_backend_us": sum(local) - local[3],
                      "outer_submit_excluding_admission_us": outer,
                      "outer_residual_us": max(0, outer - sum(local))}
            measured.append({"tick": row["tick"], "dense": bool(row["burst_epochs_crossed"]),
                "completion_us": row["source_to_completion_upper_bound_us"],
                "counters": {key: value for key, value in diagnostic.items() if not key.endswith("_us")}, **values})
        if instrumented:
            assert measured, "Missing diagnostic feature or local route"
            assert len(measured) == sum(r["gpu_submitted"] for r in raw_case["raw"])
            assert sum(r["dense"] for r in measured) == 19
        groups = {}
        for name, rows in (("all", measured), ("dense", [r for r in measured if r["dense"]]),
                           ("ordinary", [r for r in measured if not r["dense"]])):
            groups[name] = {key: pipeline.dist(r[key] for r in rows)
                for key in measured[0] if key.endswith("_us")} if measured else {}
        results.append({"run": case["run"], "hz": case["hz"], "before_work": case["before_work"],
            "groups": groups, "measured_rows": len(measured),
            "worst_rows": sorted(measured, key=lambda r: r["local_total_us"], reverse=True)[:10],
            "worst_completion_rows": sorted(measured, key=lambda r: r["completion_us"], reverse=True)[:10]})
    return {"scope": "diagnostic owner wall-clock attribution, not pure CPU/GPU execution or display; compile-time instrumentation can perturb timings", "cases": results}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--expected-cases", type=int, default=16)
    options = parser.parse_args()
    result = analyze(options.root, options.expected_cases)
    (options.root / "submit-phase-summary.json").write_text(json.dumps(result, indent=2))
    for case in result["cases"]:
        if case["groups"]["dense"]:
            values = case["groups"]["dense"]
            print(Path(case["run"]).name, case["hz"], case["before_work"],
                  sorted(((key, round(value["p50_us"] / 1000, 3)) for key, value in values.items()),
                         key=lambda p: p[1], reverse=True)[:12])
