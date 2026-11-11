param([string]$Output = 'target/retained-runtime-gpu')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin retained-runtime-gpu
    if ($LASTEXITCODE -ne 0) { throw 'Retained runtime build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.lock', 'crates/gpui-box/Cargo.toml', 'crates/gpui-box/src/retained.rs',
        'examples/hello/src/retained_demo.rs',
        'crates/gpui-box/src/retained/arena.rs', 'crates/gpui-box/src/retained/wgpu.rs', 'crates/gpui-box/src/retained/properties.rs',
        'crates/gpui-box/src/retained/wgpu/validation.rs',
        'crates/gpui-box/src/retained/reactive.rs',
        'crates/gpui-box/src/retained/reactive/computed.rs',
        'crates/gpui-box/src/retained/reactive/computed/tests.rs',
        'crates/gpui-box/src/retained/reactive/effects.rs',
        'crates/gpui-box/src/retained/reactive/effects/tests.rs',
        'crates/gpui-box/src/retained/logical.rs',
        'crates/gpui-box/src/retained/measure.rs',
        'crates/gpui-box/src/retained/paint_store.rs',
        'crates/gpui-box/src/retained/paint_store/hit_index.rs',
        'crates/gpui-box/src/retained/extraction.rs',
        'crates/gpui-box/src/retained/extraction_tests.rs',
        'crates/gpui-box/src/retained/responsive.rs',
        'crates/gpui-box/src/retained/primitive.rs',
        'crates/gpui-box/src/retained/frame_queue.rs',
        'crates/gpui-box/src/retained/image.rs',
        'crates/gpui-box/src/retained/paragraph.rs',
        'crates/gpui-box/src/retained/text.rs',
        'vendor/gpui/crates/gpui/src/text_system.rs',
        'vendor/gpui/crates/gpui/src/text_system/line.rs',
        'vendor/gpui/crates/gpui/src/text_system/line_layout.rs',
        'examples/hello/Cargo.toml',
        'examples/hello/src/retained_paragraph_tests.rs',
        'examples/hello/assets/NotoSansSC-Regular.otf',
        'tooling/rendering-perf/Cargo.toml',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer.rs',
        'vendor/gpui/crates/gpui_wgpu/src/cosmic_text_system.rs',
        'crates/gpui-box/src/retained/wgpu/composition.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/layers.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/layers.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/images.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/images.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/glyphs.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/glyphs.wgsl',
        'tooling/rendering-perf/src/retained_gpu.rs', 'tooling/rendering-perf/src/retained_bench.rs',
        'tooling/rendering-perf/src/retained_layers.rs', 'tooling/rendering-perf/src/retained_reactive.rs',
        'tooling/rendering-perf/src/retained_computed.rs',
        'tooling/rendering-perf/src/retained_logical.rs', 'tooling/rendering-perf/src/retained_measure.rs',
        'tooling/rendering-perf/src/retained_responsive.rs','tooling/rendering-perf/src/retained_primitive.rs',
        'tooling/rendering-perf/src/retained_queue.rs',
        'tooling/rendering-perf/src/retained_image.rs',
        'tooling/rendering-perf/src/retained_image_deltas.rs',
        'tooling/rendering-perf/src/retained_composition.rs',
        'tooling/rendering-perf/src/retained_text.rs',
        'tooling/rendering-perf/src/retained_glyph_bench.rs',
        'tooling/rendering-perf/src/retained_paint_store.rs',
        'tooling/rendering-perf/src/retained_extraction.rs',
        'tooling/rendering-perf/src/retained_validation.rs',
        'tooling/rendering-perf/src/retained_hit_index.rs',
        'tooling/rendering-perf/run-retained-gpu.ps1')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{
        started_at = [DateTimeOffset]::Now.ToString('o')
        base_commit = (& git rev-parse HEAD).Trim()
        rustc = (& mise exec -- rustc -Vv) -join "`n"
        source_hashes = @($hashes)
        scope = 'Retained UI frames, hardware pixel comparisons, upload payload counts, in-flight lifecycle'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    & ./target/release/retained-runtime-gpu.exe $Output
    if ($LASTEXITCODE -ne 0) { throw 'Retained runtime hardware validation failed' }
}
finally { Pop-Location }
