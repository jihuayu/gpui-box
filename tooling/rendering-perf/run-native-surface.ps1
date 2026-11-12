param([Parameter(Mandatory=$true)][string]$Output, [switch]$SkipBuild, [switch]$Delivery, [switch]$ReadbackControl)
$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Push-Location $workspace
$oldControl = $env:GPUI_RETAINED_READBACK
try {
    if (Test-Path -LiteralPath $Output) { throw 'Choose a fresh evidence directory' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $outputPath = (Resolve-Path -LiteralPath $Output).Path
    & python tooling/rendering-perf/native-sources.py (Join-Path $outputPath 'source-before.json')
    if ($LASTEXITCODE -ne 0) { throw 'Native source hash failed' }
    if (-not $SkipBuild) {
        & mise exec -- cargo build --locked --offline --release -p gpui-box-example-desktop --features native-check *> (Join-Path $outputPath 'build.log')
        if ($LASTEXITCODE -ne 0) { throw 'Native qualification build failed' }
    }
    & python tooling/rendering-perf/native-sources.py (Join-Path $outputPath 'source-manifest.json')
    if ($LASTEXITCODE -ne 0) { throw 'Native source hash failed' }
    if ((Get-FileHash (Join-Path $outputPath 'source-before.json')).Hash -ne (Get-FileHash (Join-Path $outputPath 'source-manifest.json')).Hash) { throw 'Sources changed during build' }
    $helpers = @(Get-CimInstance Win32_Process -Filter "Name='retained-runtime-gpu.exe' OR Name='quad-patch-gpu.exe' OR Name='gpui-box-example-desktop.exe'" | Select-Object ProcessId,Name)
    if ($helpers.Count -ne 0) { throw 'Other GPU qualification helpers are running' }
    $compilers = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe'" | Select-Object ProcessId,Name)
    if ($compilers.Count -ne 0) { throw 'Compiler activity: rerun after builds finish' }
    if ($ReadbackControl) { $env:GPUI_RETAINED_READBACK = '1' } else { $env:GPUI_RETAINED_READBACK = $null }
    $executable = (Resolve-Path 'target/release/gpui-box-example-desktop.exe').Path
    $argument = if ($Delivery) { '--retained-delivery-check' } else { '--retained-native-check' }
    $destination = if ($Delivery) { Join-Path $outputPath 'delivery.json' } else { $outputPath }
    $started = [DateTimeOffset]::Now.ToString('o')
    $job = Start-Process -FilePath $executable -ArgumentList @($argument, ('"' + $destination + '"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $outputPath 'stdout.log') -RedirectStandardError (Join-Path $outputPath 'stderr.log')
    $samples = [Collections.Generic.List[object]]::new()
    while (-not $job.HasExited) {
        $active = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe'" | Select-Object ProcessId,Name)
        $gpuHelpers = @(Get-CimInstance Win32_Process -Filter "Name='retained-runtime-gpu.exe' OR Name='quad-patch-gpu.exe' OR Name='gpui-box-example-desktop.exe'" | Where-Object ProcessId -ne $job.Id | Select-Object ProcessId,Name)
        $samples.Add([ordered]@{ at=[DateTimeOffset]::Now.ToString('o'); compilers=$active; other_gpu_helpers=$gpuHelpers })
        Start-Sleep -Milliseconds 500
        $job.Refresh()
    }
    $job.WaitForExit()
    [ordered]@{ interval_ms=500; samples=@($samples.ToArray()) } | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $outputPath 'process-samples.json')
    [ordered]@{ base_commit=(& git rev-parse HEAD); started=$started; finished=[DateTimeOffset]::Now.ToString('o'); binary_sha256=(Get-FileHash $executable -Algorithm SHA256).Hash.ToLowerInvariant(); delivery=[bool]$Delivery; readback_control=[bool]$ReadbackControl; exit_code=$job.ExitCode; native_launch='Start-Process -WindowStyle Hidden; explicit diagnostic draws; visibility reported separately' } | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $outputPath 'metadata.json')
    if ($job.ExitCode -ne 0) { Get-Content (Join-Path $outputPath 'stderr.log'); throw 'Native qualification failed' }
    $result = if ($Delivery) { Join-Path $outputPath 'delivery.json' } else { Join-Path $outputPath 'result.json' }
    if (-not (Get-Content $result -Raw | ConvertFrom-Json).passed) { Get-Content $result; throw 'Native validation rejected output' }
    if (@($samples | Where-Object { $_.compilers.Count -ne 0 -or $_.other_gpu_helpers.Count -ne 0 }).Count -ne 0) { throw 'Compiler activity invalidated timing observations' }
    & python tooling/rendering-perf/native-sources.py (Join-Path $outputPath 'source-after.json')
    if ((Get-FileHash (Join-Path $outputPath 'source-manifest.json')).Hash -ne (Get-FileHash (Join-Path $outputPath 'source-after.json')).Hash) { throw 'Sources changed during run' }
    Get-Content $result
}
finally { $env:GPUI_RETAINED_READBACK = $oldControl; Pop-Location }
