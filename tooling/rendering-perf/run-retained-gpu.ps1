param([string]$Output = 'target/retained-runtime-gpu', [switch]$ValidationOnly, [switch]$LayerBoundsOnly, [switch]$LayerBudgetOnly, [switch]$LayerReuseOnly, [switch]$ShadowsOnly, [switch]$SpatialOnly, [switch]$PublicationOnly, [switch]$RoutingOnly, [switch]$SpatialIndexOnly)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin retained-runtime-gpu
    if ($LASTEXITCODE -ne 0) { throw 'Retained runtime build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.toml', 'Cargo.lock', 'crates/gpui-box/Cargo.toml', 'crates/gpui-box/src/retained.rs',
        'vendor/taffy/Cargo.toml', 'vendor/taffy/UPSTREAM.json',
        'crates/gpui-box/src/retained/layout_publication.rs',
        'crates/gpui-box/src/retained/layout_publication/tests.rs',
        'examples/hello/src/retained_demo.rs',
        'examples/hello/src/retained_demo/delivery_check.rs',
        'examples/desktop/src/main.rs',
        'crates/gpui-box/src/retained/arena.rs', 'crates/gpui-box/src/retained/wgpu.rs', 'crates/gpui-box/src/retained/properties.rs',
        'crates/gpui-box/src/retained/properties/store.rs',
        'crates/gpui-box/src/retained/properties/store/tests.rs',
        'crates/gpui-box/src/retained/wgpu/validation.rs',
        'crates/gpui-box/src/retained/reactive.rs',
        'crates/gpui-box/src/retained/edit.rs',
        'crates/gpui-box/src/retained/edit/tests.rs',
        'crates/gpui-box/src/retained/reactive/edit.rs',
        'crates/gpui-box/src/retained/reactive/edit/tests.rs',
        'crates/gpui-box/src/retained/logical/edit.rs',
        'crates/gpui-box/src/retained/logical/edit/lifecycle_tests.rs',
        'tooling/rendering-perf/src/retained_edit.rs',
        'tooling/rendering-perf/src/retained_lifecycle.rs',
        'crates/gpui-box/src/retained/reactive/computed.rs',
        'crates/gpui-box/src/retained/reactive/computed/tests.rs',
        'crates/gpui-box/src/retained/reactive/effects.rs',
        'crates/gpui-box/src/retained/reactive/effects/tests.rs',
        'crates/gpui-box/src/retained/reactive/delivery.rs',
        'crates/gpui-box/src/retained/reactive/delivery/tests.rs',
        'crates/gpui-box/src/retained/logical.rs',
        'crates/gpui-box/src/retained/structure.rs',
        'crates/gpui-box/src/retained/structure/tests.rs',
        'tooling/rendering-perf/src/retained_structure.rs',
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
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/storage.rs',
        'vendor/gpui/crates/gpui/src/scene.rs',
        'tooling/rendering-perf/src/retained_shadows.rs',
        'crates/gpui-box/src/retained/spatial.rs',
        'crates/gpui-box/src/retained/wgpu/spatial.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/spatial.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/spatial.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/shaders_storage.wgsl',
        'tooling/rendering-perf/src/retained_spatial.rs',
        'crates/gpui-box/src/retained/paint_store/updates.rs',
        'crates/gpui-box/src/retained/paint_store/candidates.rs',
        'tooling/rendering-perf/src/retained_publication.rs',
        'crates/gpui-box/src/retained/wgpu/routing.rs',
        'crates/gpui-box/src/retained/wgpu/routing/tests.rs',
        'tooling/rendering-perf/src/retained_routing.rs',
        'tooling/rendering-perf/src/retained_spatial_index.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/spatial_index.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/spatial_index/tests.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/allocation_budget.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/completion.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/completion/tests.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer.rs',
        'vendor/gpui/crates/gpui_wgpu/src/shaders.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/cosmic_text_system.rs',
        'crates/gpui-box/src/retained/wgpu/composition.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/layers.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/layers.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/images.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/images.wgsl',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/glyphs.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/glyphs.wgsl',
        'tooling/rendering-perf/src/retained_gpu.rs', 'tooling/rendering-perf/src/retained_bench.rs',
        'tooling/rendering-perf/src/retained_layer_reuse.rs',
        'tooling/rendering-perf/src/retained_layer_budget.rs',
        'tooling/rendering-perf/src/retained_layer_bounds.rs',
        'tooling/rendering-perf/src/retained_layers.rs', 'tooling/rendering-perf/src/retained_reactive.rs',
        'tooling/rendering-perf/src/retained_computed.rs',
        'tooling/rendering-perf/src/retained_delivery.rs',
        'tooling/rendering-perf/src/retained_logical.rs', 'tooling/rendering-perf/src/retained_measure.rs',
        'tooling/rendering-perf/src/retained_responsive.rs','tooling/rendering-perf/src/retained_primitive.rs',
        'tooling/rendering-perf/src/retained_queue.rs',
        'tooling/rendering-perf/src/retained_completion.rs',
        'tooling/rendering-perf/src/retained_image.rs',
        'tooling/rendering-perf/src/retained_image_deltas.rs',
        'tooling/rendering-perf/src/retained_composition.rs',
        'tooling/rendering-perf/src/retained_text.rs',
        'tooling/rendering-perf/src/retained_glyph_bench.rs',
        'tooling/rendering-perf/src/retained_paint_store.rs',
        'tooling/rendering-perf/src/retained_extraction.rs',
        'tooling/rendering-perf/src/properties.rs',
        'tooling/rendering-perf/src/retained_properties.rs',
        'tooling/rendering-perf/src/retained_property_gpu.rs',
        'tooling/rendering-perf/src/layout_publication.rs',
        'tooling/rendering-perf/src/retained_layout.rs',
        'tooling/rendering-perf/src/retained_layout_gpu.rs',
        'tooling/rendering-perf/src/retained_validation.rs',
        'tooling/rendering-perf/src/retained_hit_index.rs',
        'tooling/rendering-perf/run-retained-gpu.ps1')
    $sources += @(Get-ChildItem -LiteralPath (Join-Path $workspace 'vendor/taffy/src') -Recurse -File |
        Sort-Object FullName | ForEach-Object { [IO.Path]::GetRelativePath($workspace, $_.FullName).Replace('\', '/') })
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{
        started_at = [DateTimeOffset]::Now.ToString('o')
        base_commit = (& git rev-parse HEAD).Trim()
        rustc = (& mise exec -- rustc -Vv) -join "`n"
        source_hashes = @($hashes)
        validation_only = [bool]$ValidationOnly
        layer_bounds_only = [bool]$LayerBoundsOnly
        layer_budget_only = [bool]$LayerBudgetOnly
        layer_reuse_only = [bool]$LayerReuseOnly
        shadows_only = [bool]$ShadowsOnly
        spatial_only = [bool]$SpatialOnly
        publication_only = [bool]$PublicationOnly
        routing_only = [bool]$RoutingOnly
        spatial_index_only = [bool]$SpatialIndexOnly
        scope = 'Retained UI frames, hardware pixel comparisons, upload payload counts, in-flight lifecycle'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    $runArgs = @('"' + (Resolve-Path -LiteralPath $Output).Path + '"')
    if ($ValidationOnly) { $runArgs += '--validation-only' }
    if ($LayerBoundsOnly) { $runArgs += '--layer-bounds-only' }
    if ($LayerBudgetOnly) { $runArgs += '--layer-budget-only' }
    if ($LayerReuseOnly) { $runArgs += '--layer-reuse-only' }
    if ($ShadowsOnly) { $runArgs += '--shadows-only' }
    if ($SpatialOnly) { $runArgs += '--spatial-only' }
    if ($PublicationOnly) { $runArgs += '--publication-only' }
    if ($RoutingOnly) { $runArgs += '--routing-only' }
    if ($SpatialIndexOnly) { $runArgs += '--spatial-index-only' }
    $stdout = Join-Path $Output 'stdout.log'
    $stderr = Join-Path $Output 'stderr.log'
    $job = Start-Process -FilePath (Join-Path $workspace 'target/release/retained-runtime-gpu.exe') -ArgumentList $runArgs -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $observations = [Collections.Generic.List[object]]::new()
    while (-not $job.HasExited) {
        $compilers = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe'" |
            Select-Object ProcessId, Name)
        $observations.Add([ordered]@{ at = [DateTimeOffset]::Now.ToString('o'); compilers = $compilers })
        Start-Sleep -Milliseconds 500
        $job.Refresh()
    }
    $job.WaitForExit()
    [ordered]@{ interval_ms = 500; samples = @($observations.ToArray()) } | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath (Join-Path $Output 'process-samples.json') -Encoding utf8
    Get-Content -LiteralPath $stdout
    Get-Content -LiteralPath $stderr
    if ($job.ExitCode -ne 0) { throw 'Retained runtime hardware validation failed' }
}
finally { Pop-Location }
