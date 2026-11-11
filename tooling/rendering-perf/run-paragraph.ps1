param([string]$Output = 'target/paragraph-runtime-perf')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --release --locked --offline -p gpui-box-rendering-perf --features retained-text --bin paragraph-runtime-perf
    if ($LASTEXITCODE -ne 0) { throw 'Paragraph benchmark build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $sources = @('Cargo.lock','crates/gpui-box/Cargo.toml','crates/gpui-box/src/retained.rs',
        'crates/gpui-box/src/retained/measure.rs','crates/gpui-box/src/retained/paragraph.rs',
        'vendor/gpui/crates/gpui/src/text_system.rs',
        'vendor/gpui/crates/gpui/src/text_system/line.rs',
        'vendor/gpui/crates/gpui/src/text_system/line_layout.rs',
        'vendor/gpui/crates/gpui_wgpu/src/cosmic_text_system.rs',
        'examples/hello/assets/NotoSansSC-Regular.otf',
        'examples/hello/src/retained_paragraph_tests.rs',
        'tooling/rendering-perf/Cargo.toml','tooling/rendering-perf/src/paragraph.rs')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{started_at = [DateTimeOffset]::Now.ToString('o'); base_commit = (& git rev-parse HEAD).Trim(); rustc = (& mise exec -- rustc -Vv) -join "`n"; source_hashes = @($hashes)} |
        ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Output 'metadata.json') -Encoding utf8
    & ./target/release/paragraph-runtime-perf.exe $Output
    if ($LASTEXITCODE -ne 0) { throw 'Paragraph benchmark failed' }
}
finally { Pop-Location }
