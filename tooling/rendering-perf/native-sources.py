"""Hash the native qualification's owning sources, without normalizing bytes."""
from pathlib import Path
import hashlib
import json
import sys

workspace = Path(__file__).resolve().parents[2]
selected = set()
for directory in [
    "crates/gpui-box/src/retained", "examples/hello/src", "examples/desktop/src",
    "vendor/gpui/crates/gpui_wgpu/src", "vendor/gpui/crates/gpui_windows/src",
    "tooling/rendering-perf/src",
]:
    selected.update(path for path in (workspace / directory).rglob("*")
                    if path.suffix in {".rs", ".wgsl", ".hlsl"})
for name in [
    "Cargo.toml", "Cargo.lock", "crates/gpui-box/Cargo.toml", "crates/gpui-box/src/retained.rs",
    "examples/hello/Cargo.toml", "examples/desktop/Cargo.toml", "vendor/gpui/Cargo.toml",
    "vendor/gpui/crates/gpui/Cargo.toml", "vendor/gpui/crates/gpui_wgpu/Cargo.toml",
    "vendor/gpui/crates/gpui_windows/Cargo.toml", "vendor/gpui/crates/gpui_windows/build.rs",
    "vendor/gpui/crates/gpui/src/gpui.rs", "vendor/gpui/crates/gpui/src/platform.rs",
    "vendor/gpui/crates/gpui/src/window.rs", "vendor/gpui/crates/gpui/src/scene.rs",
    "vendor/gpui/crates/gpui/src/windows_gpu_surface.rs", "vendor/gpui/crates/gpui/src/assets.rs",
    "vendor/gpui/crates/gpui/src/color.rs", "vendor/gpui/crates/gpui/src/geometry.rs",
    "tooling/rendering-perf/native-sources.py", "tooling/rendering-perf/run-native-surface.ps1",
    "tooling/rendering-perf/Cargo.toml", "tooling/rendering-perf/run-dense.ps1",
    "vendor/taffy/Cargo.toml", "vendor/taffy/UPSTREAM.json",
]:
    selected.add(workspace / name)
manifest = {path.relative_to(workspace).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(selected)}
Path(sys.argv[1]).write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
