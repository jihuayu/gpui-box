param([Parameter(Mandatory=$true)][string]$Output, [switch]$SkipBuild, [int]$Runs=2, [switch]$Quick)
$ErrorActionPreference = 'Stop'
if ($Runs -lt 1) { throw 'Runs must be positive' }
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    if (Test-Path -LiteralPath $Output) { throw 'Choose a fresh evidence directory' }
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $destination = (Resolve-Path -LiteralPath $Output).Path
    & python tooling/rendering-perf/native-sources.py (Join-Path $destination 'source-before.json')
    if ($LASTEXITCODE -ne 0) { throw 'Source hashing failed' }
    if (-not $SkipBuild) {
        & cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf *> (Join-Path $destination 'build.log')
        if ($LASTEXITCODE -ne 0) { throw 'Dense benchmark build failed' }
    }
    $executable = (Resolve-Path -LiteralPath 'target/release/dense-runtime-perf.exe').Path
    & python tooling/rendering-perf/native-sources.py (Join-Path $destination 'source-manifest.json')
    if ((Get-FileHash (Join-Path $destination 'source-before.json')).Hash -ne (Get-FileHash (Join-Path $destination 'source-manifest.json')).Hash) { throw 'Sources changed during build' }
    $started = [DateTimeOffset]::Now.ToString('o')
    $hardware = & nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader
    $samples = [Collections.Generic.List[object]]::new()
    for ($run=1; $run -le $Runs; $run++) {
        $conflicts = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe' OR Name='dense-runtime-perf.exe' OR Name='retained-runtime-gpu.exe' OR Name='gpui-box-example-desktop.exe'")
        if ($conflicts.Count -ne 0) { throw 'Compiler or other benchmark activity before measurement' }
        $runPath = Join-Path $destination "run-$run"
        $arguments = @(('"' + $runPath + '"'))
        if ($Quick) { $arguments += '--quick' }
        $job = Start-Process -FilePath $executable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $destination "run-$run.log") -RedirectStandardError (Join-Path $destination "run-$run.err")
        while (-not $job.HasExited) {
            $active = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe' OR Name='dense-runtime-perf.exe' OR Name='retained-runtime-gpu.exe' OR Name='gpui-box-example-desktop.exe'" | Where-Object ProcessId -ne $job.Id | Select-Object ProcessId,Name)
            $samples.Add([ordered]@{at=[DateTimeOffset]::Now.ToString('o'); run=$run; conflicts=$active})
            Start-Sleep -Milliseconds 500
            $job.Refresh()
        }
        $job.WaitForExit()
        if ($job.ExitCode -ne 0) { throw "Dense benchmark run $run failed: see stderr" }
    }
    & python tooling/rendering-perf/native-sources.py (Join-Path $destination 'source-after.json')
    $stable = (Get-FileHash (Join-Path $destination 'source-manifest.json')).Hash -eq (Get-FileHash (Join-Path $destination 'source-after.json')).Hash
    $uncontaminated = @($samples | Where-Object { $_.conflicts.Count -ne 0 }).Count -eq 0
    [ordered]@{interval_ms=500; samples=@($samples.ToArray())} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $destination 'process-samples.json')
    [ordered]@{base_commit=(& git rev-parse HEAD); started=$started; finished=[DateTimeOffset]::Now.ToString('o'); binary_sha256=(Get-FileHash $executable).Hash.ToLowerInvariant(); hardware=$hardware; cpu=@(Get-CimInstance Win32_Processor | Select-Object -ExpandProperty Name); runs=$Runs; quick=[bool]$Quick; sources_stable=$stable; no_sampled_compiler_or_benchmark_conflicts=$uncontaminated; scope='independent OS visual worker to synchronous GPU completion; excludes native display and physical input'} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $destination 'metadata.json')
    if (-not $stable -or -not $uncontaminated) { throw 'Measurement evidence invalidated by source changes or sampled process conflicts' }
}
finally { Pop-Location }
