param(
    [string]$Output = 'target/rendering-perf',
    [ValidateRange(10, 10000)][int]$Samples = 101,
    [ValidateRange(1, 1000000)][int]$MinBatchUs = 1000,
    [ValidateRange(1, 10)][int]$Runs = 3,
    [switch]$LayoutOnly
)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    $arguments = @('build', '--release', '--locked', '--offline', '-p', 'gpui-box-rendering-perf')
    if (-not $LayoutOnly) { $arguments += @('--features', 'gpui-scene') }
    & mise exec -- cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Benchmark build failed' }

    # Build before collecting timings. This wrapper deliberately executes runs serially.
    $outputPath = [IO.Path]::GetFullPath((Join-Path $workspace $Output))
    New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
    $baseCommit = (& git rev-parse HEAD).Trim()
    $cpu = Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
    $gpu = Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion
    $os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
    $sourceFiles = @('Cargo.lock', 'Cargo.toml', 'tooling/rendering-perf/Cargo.toml', 'tooling/rendering-perf/run.ps1',
        'vendor/gpui/crates/gpui/src/scene.rs', 'vendor/gpui/crates/gpui/src/taffy.rs',
        'vendor/gpui/crates/gpui/src/window.rs', 'vendor/gpui/crates/gpui/src/bounds_tree.rs',
        'vendor/gpui/crates/gpui_windows/src/directx_renderer.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer.rs',
        'vendor/gpui/crates/gpui_apple/src/metal_renderer.rs') + @(Get-ChildItem -LiteralPath $PSScriptRoot/src -Filter '*.rs' | ForEach-Object { $_.FullName })
    $hashes = foreach ($sourceFile in $sourceFiles) {
        $hash = Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256
        [ordered]@{ path = [IO.Path]::GetRelativePath($workspace, $hash.Path).Replace('\', '/'); sha256 = $hash.Hash }
    }
    $metadata = [ordered]@{
        started_at = [DateTimeOffset]::Now.ToString('o')
        base_commit = $baseCommit
        upstream_commit = 'f8cfc7ed9e57503def9479138894f3139f5400cc'
        branch = (& git branch --show-current).Trim()
        source_hashes = @($hashes)
        rustc = (& mise exec -- rustc -Vv) -join "`n"
        cargo = (& mise exec -- cargo --version) -join "`n"
        os = $os
        cpu = $cpu
        gpu_inventory_only_not_exercised = @($gpu)
        power_scheme = (& powercfg /getactivescheme) -join "`n"
        release_profile = 'Cargo default release: opt-level=3, no custom LTO or target-cpu'
        features = @($(if (-not $LayoutOnly) { 'gpui-scene' }))
        samples = $Samples
        min_batch_us = $MinBatchUs
        sizes = @(300, 3000, 30000)
        runs = $Runs
        methodology = '8 warmup operations then doubling batch calibration; nearest-rank quantiles of batched ns/op; no window, GPU, shaping, input or presentation'
    }
    $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputPath 'metadata.json') -Encoding utf8
    for ($run = 1; $run -le $Runs; $run++) {
        & (Join-Path $workspace 'target/release/gpui-box-rendering-perf.exe') --output (Join-Path $outputPath "run-$run") --samples $Samples --min-batch-us $MinBatchUs
        if ($LASTEXITCODE -ne 0) { throw "Benchmark run $run failed" }
    }
    Write-Output "Recorded metadata and $Runs runs in $outputPath"
}
finally {
    Pop-Location
}
