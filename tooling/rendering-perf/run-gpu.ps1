param(
    [string]$Output = 'target/quad-patch-gpu',
    [ValidateRange(10, 10000)][int]$Samples = 201,
    [ValidateRange(1, 10)][int]$Rounds = 3
)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features gpu-prototype --bin quad-patch-gpu
    if ($LASTEXITCODE -ne 0) { throw 'GPU prototype build failed' }
    $outputPath = [IO.Path]::GetFullPath((Join-Path $workspace $Output))
    New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
    $files = @('Cargo.lock', 'tooling/rendering-perf/Cargo.toml', 'tooling/rendering-perf/src/gpu.rs',
        'vendor/gpui/crates/gpui_wgpu/Cargo.toml', 'vendor/gpui/crates/gpui_wgpu/src/gpui_wgpu.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer.rs', 'vendor/gpui/crates/gpui_wgpu/src/wgpu_context.rs',
        'vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/quad_patch_prototype.rs',
        'vendor/gpui/crates/gpui_wgpu/src/shaders.wgsl', 'vendor/gpui/crates/gpui_wgpu/src/shaders_storage.wgsl')
    $hashes = foreach ($file in $files) {
        $hash = Get-FileHash -LiteralPath $file -Algorithm SHA256
        [ordered]@{ path = $file; sha256 = $hash.Hash }
    }
    [ordered]@{
        started_at = [DateTimeOffset]::Now.ToString('o')
        base_commit = (& git rev-parse HEAD).Trim()
        branch = (& git branch --show-current).Trim()
        source_hashes = @($hashes)
        rustc = (& mise exec -- rustc -Vv) -join "`n"
        cargo = (& mise exec -- cargo --version) -join "`n"
        os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
        cpu = Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
        power_scheme = (& powercfg /getactivescheme) -join "`n"
        samples = $Samples
        rounds = $Rounds
        warmup_per_path = 40
        compilation = 'release opt-level=3; no custom LTO or target-cpu'
    } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputPath 'metadata.json') -Encoding utf8
    & ./target/release/quad-patch-gpu.exe --output $outputPath --samples $Samples --rounds $Rounds
    if ($LASTEXITCODE -ne 0) { throw 'GPU measurement or correctness assertion failed' }
    python ./tooling/rendering-perf/summarize_gpu.py $outputPath
    if ($LASTEXITCODE -ne 0) { throw 'GPU evidence verification failed' }
}
finally { Pop-Location }
