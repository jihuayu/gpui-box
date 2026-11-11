param([string]$Output = 'target/reactive-runtime-perf')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-runtime --bin reactive-runtime-perf
    if ($LASTEXITCODE -ne 0) { throw 'Reactive runtime benchmark build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.lock', 'crates/gpui-box/src/retained.rs', 'crates/gpui-box/src/retained/reactive.rs',
        'crates/gpui-box/src/retained/properties.rs', 'examples/hello/src/retained_demo.rs',
        'tooling/rendering-perf/src/reactive.rs', 'tooling/rendering-perf/Cargo.toml')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{started_at = [DateTimeOffset]::Now.ToString('o'); base_commit = (& git rev-parse HEAD).Trim()
        rustc = (& mise exec -- rustc -Vv) -join "`n"; source_hashes = @($hashes)
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    & ./target/release/reactive-runtime-perf.exe $Output
    if ($LASTEXITCODE -ne 0) { throw 'Reactive runtime benchmark failed' }
}
finally { Pop-Location }
