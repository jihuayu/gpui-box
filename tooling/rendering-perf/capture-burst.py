"""Sequential Windows burst captures; sample process names without command lines.

No compiler is allowed during captures. Process observations cannot prove a
quiet GPU/OS or qualify visible presentation. Each mode uses the same executable.
"""
import ctypes as c
import argparse
from ctypes import wintypes as w
import hashlib
import json
from pathlib import Path
import subprocess
import time


class Process(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
                ("heap", c.c_size_t), ("module", w.DWORD), ("threads", w.DWORD),
                ("parent", w.DWORD), ("priority", w.LONG), ("flags", w.DWORD),
                ("exe", w.WCHAR * 260)]


k = c.WinDLL("kernel32", use_last_error=True)
k.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
k.CreateToolhelp32Snapshot.restype = w.HANDLE
k.Process32FirstW.argtypes = [w.HANDLE, c.POINTER(Process)]
k.Process32NextW.argtypes = [w.HANDLE, c.POINTER(Process)]
k.CloseHandle.argtypes = [w.HANDLE]


def processes():
    handle = k.CreateToolhelp32Snapshot(2, 0)
    if handle == c.c_void_p(-1).value:
        raise c.WinError(c.get_last_error())
    entry = Process()
    entry.size = c.sizeof(entry)
    result = []
    try:
        ok = k.Process32FirstW(handle, c.byref(entry))
        while ok:
            name = entry.exe.lower()
            if name in {"cargo.exe", "rustc.exe", "cl.exe", "link.exe", "ninja.exe",
                        "dense-runtime-perf.exe", "retained-runtime-gpu.exe",
                        "software-runtime-perf.exe", "quad-patch-gpu.exe"}:
                result.append({"pid": entry.pid, "name": name})
            ok = k.Process32NextW(handle, c.byref(entry))
    finally:
        k.CloseHandle(handle)
    return result


def hashes(paths):
    return {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    pair_options = parser.add_mutually_exclusive_group()
    for flag in ("material-pair", "stream-pair", "pipeline-pair", "recovery-pair", "publication-pair"):
        pair_options.add_argument(f"--{flag}", action="store_true")
    pair_options.add_argument("--baseline", type=Path, help="Exact baseline binary for a general implementation pair")
    parser.add_argument("--candidate-name", default="candidate", help="Filesystem-safe label for a general implementation pair")
    parser.add_argument("--round", type=int, choices=(1, 2), help="Capture only this planned round in a fresh directory")
    parser.add_argument("--pipelined", action="store_true", help="Use pipelined burst workload for an implementation pair")
    options = parser.parse_args()
    output = options.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    binary = Path("target/release/dense-runtime-perf.exe").resolve()
    sources = sorted(Path("crates/gpui-box/src").rglob("*.rs"))
    sources += sorted(Path("vendor/gpui/crates/gpui_wgpu/src").rglob("*.rs"))
    sources += sorted(Path("tooling/rendering-perf/src").rglob("*.rs"))
    sources += [Path(__file__), Path("Cargo.lock"), Path("crates/gpui-box/Cargo.toml"),
                Path("vendor/gpui/crates/gpui_wgpu/Cargo.toml"), Path("tooling/rendering-perf/Cargo.toml")]
    material_pair = options.material_pair
    stream_pair = options.stream_pair
    pipeline = options.pipeline_pair
    publication_pair = options.publication_pair
    pair = options.recovery_pair or material_pair or stream_pair or publication_pair or options.baseline is not None
    assert options.candidate_name.replace("-", "").isalnum() and options.candidate_name != "original"
    assert not options.pipelined or pair, "--pipelined requires an implementation pair"
    baseline = Path("target/publication-order-baseline/dense-runtime-perf.exe" if publication_pair else "target/material-stream-baseline/dense-runtime-perf.exe" if stream_pair else "target/material-baseline/dense-runtime-perf.exe" if material_pair else "target/burst-baseline/dense-runtime-perf.exe").resolve()
    if options.baseline is not None:
        baseline = options.baseline.resolve()
    binaries = [binary, baseline] if pair else [binary]
    binary_hashes = hashes(binaries)
    before = hashes(sources)
    (output / "sources.json").write_text(json.dumps(before, indent=2), encoding="utf-8")
    assert not processes(), "Build or renderer process active before capture"
    modes = {"source-wait": ["--source-only-wait"], "service-completions": []} if pipeline else {"original": [], options.candidate_name if options.baseline is not None else "ordered" if publication_pair else "streamed" if stream_pair else "material-pages" if material_pair else "full-cache": []} if pair else {
        "default": [],
        "tight-lazy": ["--local-tight-layers"],
        "tight-lazy-raster": ["--local-tight-layers", "--layer-translation-reuse"],
    }
    runs = []
    planned_rounds = [options.round - 1] if options.round else list(range(2))
    for round_index in planned_rounds:
        order = list(modes) if round_index == 0 else list(reversed(modes))
        for mode in order:
            folder = output / f"run-{round_index + 1}-{mode}"
            folder.mkdir()
            chosen_binary = baseline if pair and mode == "original" else binary
            args = [str(chosen_binary), str(folder), "--local", "--integer-motion",
                    *(["--pipeline-only", "--pipeline-burst-only"] if pipeline or options.pipelined else ["--burst-cadence-only"]), *modes[mode]]
            samples = []
            start = time.perf_counter()
            with (folder / "stdout.txt").open("w", encoding="utf-8") as log:
                child = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
                while child.poll() is None:
                    at = time.perf_counter()
                    seen = processes()
                    samples.append({"elapsed_s": at - start, "processes": seen,
                                    "sample_us": (time.perf_counter() - at) * 1e6})
                    time.sleep(.25)
                code = child.wait()
            (folder / "process-samples.json").write_text(json.dumps(samples), encoding="utf-8")
            conflicts = [p for s in samples for p in s["processes"] if p["pid"] != child.pid]
            runs.append({"round": round_index + 1, "mode": mode, "args": args,
                         "exit_code": code, "conflicts": conflicts})
            print(round_index + 1, mode, "exit", code, "conflicts", len(conflicts), flush=True)
            if code or conflicts:
                break
        if runs[-1]["exit_code"] or runs[-1]["conflicts"]:
            break
    manifest = {"base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "binary_sha256": binary_hashes, "binary_hashes_stable": binary_hashes == hashes(binaries),
                "source_hashes_stable": before == hashes(sources),
                "process_sample_interval_s": .25, "planned_rounds": [r + 1 for r in planned_rounds], "runs": runs,
                "scope": "source/owner/GPU completion upper bound; process sampler observes known compiler/renderer conflicts only; no native present/input, power/thermal/driver isolation or modern desktop qualification"}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    assert manifest["source_hashes_stable"]
    assert manifest["binary_hashes_stable"]
    assert len(runs) == len(modes) * len(planned_rounds) and all(r["exit_code"] == 0 and not r["conflicts"] for r in runs)


if __name__ == "__main__":
    main()
