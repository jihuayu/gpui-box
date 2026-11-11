param([string]$Output = 'target/measure-runtime-perf')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-runtime --bin measure-runtime-perf
    if ($LASTEXITCODE -ne 0) { throw 'Measure runtime benchmark build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.lock', 'crates/gpui-box/src/retained.rs', 'crates/gpui-box/src/retained/measure.rs',
        'crates/gpui-box/src/retained/properties.rs', 'crates/gpui-box/src/retained/logical.rs',
        'crates/gpui-box/src/retained/reactive.rs', 'tooling/rendering-perf/src/measure.rs',
        'tooling/rendering-perf/Cargo.toml', 'tooling/rendering-perf/run-measure.ps1')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    $cpu = Get-CimInstance -ClassName Win32_Processor | Select-Object -First 1 Name, NumberOfCores, NumberOfLogicalProcessors
    $os = Get-CimInstance -ClassName Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
    [ordered]@{started_at = [DateTimeOffset]::Now.ToString('o'); base_commit = (& git rev-parse HEAD).Trim()
        cpu = $cpu; os = $os
        rustc = (& mise exec -- rustc -Vv) -join "`n"; source_hashes = @($hashes)
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    & ./target/release/measure-runtime-perf.exe $Output
    if ($LASTEXITCODE -ne 0) { throw 'Measure runtime benchmark failed' }
}
finally { Pop-Location }
