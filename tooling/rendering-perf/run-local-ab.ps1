param(
    [Parameter(Mandatory=$true)][string]$Output,
    [Parameter(Mandatory=$true)][string]$ControlBinary,
    [Parameter(Mandatory=$true)][string]$ControlCommit,
    [int]$Runs=2
)
$ErrorActionPreference = 'Stop'
if ($Runs -lt 2) { throw 'At least two alternating rounds are required' }
$workspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
Push-Location -LiteralPath $workspace
try {
    if (Test-Path -LiteralPath $Output) { throw 'Choose a fresh evidence directory' }
    $control = (Resolve-Path -LiteralPath $ControlBinary).Path
    $optimized = (Resolve-Path -LiteralPath 'target/release/dense-runtime-perf.exe').Path
    New-Item -ItemType Directory -Path $Output -Force | Out-Null
    $destination = (Resolve-Path -LiteralPath $Output).Path
    & python tooling/rendering-perf/native-sources.py (Join-Path $destination 'source-before.json')
    if ($LASTEXITCODE -ne 0) { throw 'Source hashing failed' }
    $samples = [Collections.Generic.List[object]]::new()
    $started = [DateTimeOffset]::Now.ToString('o')
    $binaries = [ordered]@{control=$control; optimized=$optimized}
    $identities = foreach ($name in $binaries.Keys) {
        [ordered]@{mode=$name; binary_sha256=(Get-FileHash $binaries[$name]).Hash.ToLowerInvariant()}
    }
    for ($run=1; $run -le $Runs; $run++) {
        $order = if ($run % 2 -eq 1) { @('control','optimized') } else { @('optimized','control') }
        foreach ($mode in $order) {
            $conflicts = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe' OR Name='dense-runtime-perf.exe' OR Name='retained-runtime-gpu.exe' OR Name='gpui-box-example-desktop.exe'")
            if ($conflicts.Count -ne 0) { throw 'Compiler or other benchmark activity before measurement' }
            $label = "round-$run-$mode"
            $runPath = Join-Path $destination $label
            $job = Start-Process -FilePath $binaries[$mode] -ArgumentList @(('"' + $runPath + '"'), '--local', '--paired-only') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $destination "$label.log") -RedirectStandardError (Join-Path $destination "$label.err")
            while (-not $job.HasExited) {
                $active = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='clippy-driver.exe' OR Name='dense-runtime-perf.exe' OR Name='retained-runtime-gpu.exe' OR Name='gpui-box-example-desktop.exe'" | Where-Object ProcessId -ne $job.Id | Select-Object ProcessId,Name)
                $samples.Add([ordered]@{at=[DateTimeOffset]::Now.ToString('o'); case=$label; conflicts=$active})
                Start-Sleep -Milliseconds 500
                $job.Refresh()
            }
            $job.WaitForExit()
            if ($job.ExitCode -ne 0) { throw "$label failed; inspect stderr" }
        }
    }
    & python tooling/rendering-perf/native-sources.py (Join-Path $destination 'source-after.json')
    if ($LASTEXITCODE -ne 0) { throw 'Source hashing failed' }
    $stable = (Get-FileHash (Join-Path $destination 'source-before.json')).Hash -eq (Get-FileHash (Join-Path $destination 'source-after.json')).Hash
    $uncontaminated = @($samples | Where-Object { $_.conflicts.Count -ne 0 }).Count -eq 0
    [ordered]@{interval_ms=500; samples=@($samples.ToArray())} | ConvertTo-Json -Depth 6 | Set-Content -Encoding utf8 (Join-Path $destination 'process-samples.json')
    [ordered]@{base_commit=(& git rev-parse HEAD); control_commit=$ControlCommit; binaries=@($identities); started=$started; finished=[DateTimeOffset]::Now.ToString('o'); runs=$Runs; sources_stable=$stable; no_sampled_conflicts=$uncontaminated; gpu=(& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader); cpu=@(Get-CimInstance Win32_Processor | Select-Object -ExpandProperty Name); scope='alternating previous and current binaries; unchanged local benchmark and layer policy; 120 paired samples per mode/case; no native display'} | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $destination 'metadata.json')
    if (-not $stable -or -not $uncontaminated) { throw 'A/B evidence invalidated by source or process conflict' }
}
finally { Pop-Location }
