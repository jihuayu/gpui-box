"""Build a common public-API control with isolated source and target trees."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--baseline", default="d38d5007c09dc90d641a23c055b6c8c0ce428c86")
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
output = Path(tempfile.mkdtemp(prefix="producer-invalidation-", dir=root / "target"))
compiler = subprocess.check_output(["rustup", "which", "--toolchain", "1.98.1", "rustc"], text=True).strip()
cargo = subprocess.check_output(["rustup", "which", "--toolchain", "1.98.1", "cargo"], text=True).strip()
env = dict(os.environ, RUSTC=compiler, TEMP=str(output), TMP=str(output))
manifest = '''[package]
name="producer-invalidation-bench"
version="0.0.0"
edition="2024"
[workspace]
[features]
default=["retained"]
retained=[]
retained-text=[]
retained-wgpu=[]
retained-diagnostics=[]
desktop=[]
android=[]
[dependencies]
taffy={path="TAFFY_PATH",default-features=false,features=["std","taffy_tree","flexbox","grid","block_layout","float_layout","content_size","affine"]}
[lib]
name="gpui_box"
path="crates/gpui-box/src/lib.rs"
[[bin]]
name="producer-invalidation-bench"
path="benchmark.rs"
'''.replace("TAFFY_PATH", (root / "vendor/taffy").as_posix())
results = {}
for variant in ["baseline", "candidate"]:
    directory = output / variant
    directory.mkdir()
    if variant == "baseline":
        archive = subprocess.check_output(["git", "archive", args.baseline, "crates/gpui-box/src"], cwd=root)
        with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
            stream.extractall(directory, filter="data")
    else:
        shutil.copytree(root / "crates/gpui-box/src", directory / "crates/gpui-box/src")
    (directory / "Cargo.toml").write_text(manifest, encoding="utf-8")
    shutil.copyfile(Path(__file__).with_name("benchmark.rs"), directory / "benchmark.rs")
    target = directory / "build"
    subprocess.run([cargo,"build","--manifest-path",str(directory/"Cargo.toml"),"--target-dir",str(target),"--release","--offline"],cwd=directory,env=env,check=True)
    exe = target / "release" / ("producer-invalidation-bench.exe" if os.name == "nt" else "producer-invalidation-bench")
    results[variant] = {"path":str(exe),"sha256":hashlib.sha256(exe.read_bytes()).hexdigest()}
assert results["baseline"]["sha256"] != results["candidate"]["sha256"]
(output/"build.json").write_text(json.dumps(results,indent=2),encoding="utf-8")
print(json.dumps(results,indent=2))
