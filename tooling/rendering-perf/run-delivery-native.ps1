param([string]$Output = 'target/delivery-native-check')
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    & mise exec -- cargo build --locked --offline -p gpui-box-example-desktop --features retained-wgpu
    if ($LASTEXITCODE -ne 0) { throw 'Native delivery diagnostic build failed' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $outputPath = (Resolve-Path -LiteralPath $Output).Path
    $resultPath = Join-Path $outputPath ('result-' + [Guid]::NewGuid().ToString('N') + '.json')
    $executable = Join-Path $workspace 'target/debug/gpui-box-example-desktop.exe'
    $sources = @('Cargo.lock', 'crates/gpui-box/src/retained.rs', 'crates/gpui-box/src/retained/reactive.rs',
        'crates/gpui-box/src/retained/reactive/computed.rs', 'crates/gpui-box/src/retained/reactive/effects.rs',
        'crates/gpui-box/src/retained/reactive/delivery.rs', 'crates/gpui-box/src/retained/logical.rs',
        'examples/hello/src/retained_demo.rs', 'examples/hello/src/retained_demo/delivery_check.rs',
        'examples/hello/src/lib.rs', 'examples/desktop/src/main.rs', 'tooling/rendering-perf/run-delivery-native.ps1')
    $hashes = foreach ($source in $sources) {
        [ordered]@{path = $source; sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash}
    }
    [ordered]@{started_at = [DateTimeOffset]::Now.ToString('o'); base_commit = (& git rev-parse HEAD).Trim()
        rustc = (& mise exec -- rustc -Vv) -join "`n"; source_hashes = @($hashes)
        executable_sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
        launch_window_style = 'Hidden'; scope = 'Real OS worker and foreground executor, native window creation, labelled manual draw when hidden, GPU pixel comparison; no wake-latency or energy claim'
    } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $outputPath 'native-metadata.json') -Encoding utf8
    $process = Start-Process -FilePath $executable -ArgumentList @('--retained-delivery-check', ('"' + $resultPath + '"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $outputPath 'native-stdout.log') -RedirectStandardError (Join-Path $outputPath 'native-stderr.log') -PassThru
    $process.Id | Set-Content -LiteralPath (Join-Path $outputPath 'native-pid.txt')
    if (-not $process.WaitForExit(20000)) { throw "Native delivery diagnostic still running, PID=$($process.Id); inspect that process before retrying" }
    if (-not (Test-Path -LiteralPath $resultPath)) { throw 'Native delivery diagnostic exited without a fresh result; inspect native-stderr.log' }
    $result = Get-Content -LiteralPath $resultPath -Raw -Encoding utf8 | ConvertFrom-Json
    Copy-Item -LiteralPath $resultPath -Destination (Join-Path $outputPath 'native-result.json') -Force
    if (-not $result.passed) { throw "Native delivery diagnostic failed: $($result.error)" }
    Get-Content -LiteralPath $resultPath -Encoding utf8
}
finally { Pop-Location }
