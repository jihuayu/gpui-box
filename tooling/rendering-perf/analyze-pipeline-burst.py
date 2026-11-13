"""Same-row pipeline age/owner analysis. Completion receipts are upper bounds."""
import json
import argparse
import math
from pathlib import Path


def dist(values):
    values = sorted(values)
    if not values:
        return {"count": 0}
    assert all(math.isfinite(v) and v >= 0 for v in values)
    return {"count": len(values), **{key: values[math.ceil(q * len(values)) - 1]
        for key, q in (("p50_us", .5), ("p95_us", .95), ("p99_us", .99), ("max_us", 1))}}


def losses(rows, field, budget):
    total = burst = longest = 0
    for row in rows:
        skipped = row["superseded_ticks"]
        total += skipped
        burst += skipped
        longest = max(longest, burst)
        if row[field] > budget:
            total += 1
            burst += 1
            longest = max(longest, burst)
        else:
            burst = 0
    return {"unsatisfied_ticks": total, "longest_unsatisfied_ticks": longest}


def analyze(roots, expected_cases, implementation_pair=False):
    cases = []
    rejected = []
    eligible = []
    covered_runs = set()
    binary_ids = set()
    for root in roots:
        manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8-sig"))
        for run in manifest["runs"]:
            reason = []
            if not manifest["source_hashes_stable"]: reason.append("sources changed")
            if not manifest["binary_hashes_stable"]: reason.append("binary changed")
            if run["exit_code"]: reason.append("nonzero exit")
            if run["conflicts"]: reason.append("known compiler/renderer interference")
            folder = root / f"run-{run['round']}-{run['mode']}"
            if reason:
                rejected.append({"run": str(folder), "reasons": reason, "conflict_observations": len(run["conflicts"])})
            else:
                assert "--pipeline-burst-only" in run["args"]
                key = (run["round"], run["mode"])
                assert key not in covered_runs, ("duplicate eligible control", key)
                covered_runs.add(key)
                eligible.append(folder)
                binary_ids.update(manifest["binary_sha256"].values())
    assert len(binary_ids) == (2 if implementation_pair else 1), "Unexpected number of pipeline control binaries"
    for run in eligible:
        data = json.loads((run / "pipeline.json").read_text(encoding="utf-8"))
        assert not data["quick"] and len(data["cases"]) == 4
        assert {(c["target_hz"], c["admission_before_work"]) for c in data["cases"]} == {
            (hz, before) for hz in (60, 120) for before in (False, True)}
        for case in data["cases"]:
            # The first capture binary shadowed its burst flag with the final
            # miss streak counter. Invocation and per-row epoch checks below
            # establish the workload independently of that metadata defect.
            assert case["pixel_equal"] and case["leases_retired"]
            assert case["service_completions_while_waiting"] == (implementation_pair or run.name.endswith("service-completions"))
            assert case["nodes"] == 10000 and case["effects"] and not case["batch"]
            assert case["max_in_flight"] == 2 and case["expected_ticks"] == case["target_hz"] * 10
            assert case["maximum_gpu_in_flight"] <= case["max_in_flight"]
            assert case["full_resyncs_after_initial"] == 0
            hz = case["target_hz"]
            budget = 1e6 / hz
            rows = []
            previous_tick = -1
            previous_epoch = 0
            for row in case["raw"]:
                assert row["tick"] > previous_tick
                assert row["superseded_ticks"] == row["tick"] - previous_tick - 1
                assert row["burst_epoch"] == row["tick"] // (hz // 2)
                assert row["burst_epochs_crossed"] == row["burst_epoch"] - previous_epoch
                previous_tick = row["tick"]
                previous_epoch = row["burst_epoch"]
                owner = sum(row[p] for p in ("commit_us", "publication_us", "submit_and_admission_us"))
                submit_age = owner + row["source_to_owner_us"]
                assert submit_age <= row["source_to_completion_upper_bound_us"] + .001
                assert row["source_to_completion_upper_bound_us"] <= row["source_to_owner_completion_observation_us"] + .001
                # Equal-state samples can reuse a receipt older than their
                # arrival. Count delay after max(receipt, owner submit), just
                # as the existing completed-output age does. Older captures'
                # raw handling field used receipt time even in that case.
                handling = max(0, row["source_to_owner_completion_observation_us"] - row["source_to_completion_upper_bound_us"])
                rows.append({**row, "raw_completion_to_owner_observation_us": row["completion_to_owner_observation_us"],
                    "completion_to_owner_observation_us": handling, "owner_us": owner, "source_to_submit_us": submit_age})
            assert len(rows) + case["superseded_ticks"] == case["expected_ticks"]
            assert previous_epoch == case["dense_epochs_crossed"] == 19
            worker = losses(rows, "source_to_completion_upper_bound_us", budget)
            assert worker["unsatisfied_ticks"] == case["unsatisfied_ticks_upper_bound"]
            assert worker["longest_unsatisfied_ticks"] == case["longest_consecutive_misses_upper_bound"]
            groups = {}
            fields = ("source_to_completion_upper_bound_us", "source_to_owner_completion_observation_us",
                      "completion_to_owner_observation_us", "owner_us", "source_to_submit_us",
                      "commit_us", "publication_us", "submit_and_admission_us", "admission_wait_us")
            for name, chosen in (("all", rows), ("dense", [r for r in rows if r["burst_epochs_crossed"]]),
                                 ("ordinary", [r for r in rows if not r["burst_epochs_crossed"]])):
                groups[name] = {field: dist(r[field] for r in chosen) for field in fields}
            cases.append({"run": str(run), "hz": hz, "before_work": case["admission_before_work"],
                "legacy_numeric_burst_metadata": type(case["burst"]) is int,
                "groups": groups, "completion_upper_bound_losses": worker,
                "owner_observation_losses": losses(rows, "source_to_owner_completion_observation_us", budget),
                **{key: case[key] for key in ("expected_ticks", "consumed_ticks", "superseded_ticks", "admission_waits",
                   "layer_budget_waits", "maximum_gpu_in_flight", "maximum_pending_receipts", "dense_epochs_crossed",
                   "full_resyncs_after_initial", "pixel_equal", "leases_retired")},
                "worst_owner_observation_rows": sorted(rows, key=lambda r: r["source_to_owner_completion_observation_us"], reverse=True)[:10]})
    assert len(cases) == expected_cases, (len(cases), expected_cases)
    if expected_cases == 16:
        assert covered_runs == {(round_index, mode) for round_index in (1, 2)
            for mode in (("original", "ordered") if implementation_pair else ("source-wait", "service-completions"))}
    return {"scope": "source through coalesced covering completion upper bound and owner observation; no GPU execution timestamp, native present/display or physical input", "binary_sha256": sorted(binary_ids), "rejected_runs": rejected, "expected_cases": expected_cases, "cases": cases}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("roots", type=Path, nargs="+")
    parser.add_argument("--expected-cases", type=int, default=16)
    parser.add_argument("--implementation-pair", action="store_true", help="Compare original/ordered binaries with completion servicing enabled")
    options = parser.parse_args()
    result = analyze(options.roots, options.expected_cases, options.implementation_pair)
    (options.roots[-1] / "pipeline-summary.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    for case in result["cases"]:
        fields = case["groups"]["all"]
        print(case["run"], case["hz"], "before", case["before_work"], "worker/owner/handling p99 ms",
              *[round(fields[field]["p99_us"] / 1000, 2) for field in (
                  "source_to_completion_upper_bound_us", "source_to_owner_completion_observation_us",
                  "completion_to_owner_observation_us")], "losses", case["completion_upper_bound_losses"]["unsatisfied_ticks"],
              case["owner_observation_losses"]["unsatisfied_ticks"])
