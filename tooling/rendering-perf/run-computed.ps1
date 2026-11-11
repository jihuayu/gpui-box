param([string]$Output = 'target/computed-runtime-perf')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-runtime --bin computed-runtime-perf
    if ($LASTEXITCODE -ne 0) { throw 'Computed runtime benchmark build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.lock', 'crates/gpui-box/src/retained.rs', 'crates/gpui-box/src/retained/reactive.rs',
        'crates/gpui-box/src/retained/reactive/computed.rs', 'crates/gpui-box/src/retained/reactive/computed/tests.rs',
        'crates/gpui-box/src/retained/reactive/effects.rs', 'crates/gpui-box/src/retained/reactive/effects/tests.rs',
        'crates/gpui-box/src/retained/logical.rs', 'examples/hello/src/retained_demo.rs',
        'tooling/rendering-perf/src/computed.rs', 'tooling/rendering-perf/Cargo.toml', 'tooling/rendering-perf/run-computed.ps1')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{started_at = [DateTimeOffset]::Now.ToString('o'); base_commit = (& git rev-parse HEAD).Trim()
        rustc = (& mise exec -- rustc -Vv) -join "`n"; source_hashes = @($hashes)
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    & ./target/release/computed-runtime-perf.exe $Output
    if ($LASTEXITCODE -ne 0) { throw 'Computed runtime benchmark failed' }
}
finally { Pop-Location }
