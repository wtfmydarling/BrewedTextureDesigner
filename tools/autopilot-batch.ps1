# Runs many autopilots of Brewed Texture Designer: batches of runs side by side, each run its
# own seed (and so its own folder: brewed-autopilot-<seed> in the temp folder). Every run's log
# and output are copied into one results folder, with a summary of the findings.
#
#   pwsh tools/autopilot-batch.ps1                          # 2 batches of 10 runs, 10 minutes each
#   pwsh tools/autopilot-batch.ps1 -Batches 5 -Runs 10 -Minutes 15 -FirstSeed 500
#
# Runs side by side share the GPU and the processor: a run may then report slow frames or a
# slow engine ("FINDING ... took") that a run alone would not. Other findings are real.

param(
    [int]$Batches = 2,
    [int]$Runs = 10,
    [double]$Minutes = 10,
    [int]$FirstSeed = 100,
    [string]$Results = (Join-Path $HOME "brewed-autopilot-results")
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$out = Join-Path $Results (Get-Date -Format "yyyy-MM-dd_HH-mm-ss")
New-Item -ItemType Directory -Force $out | Out-Null

# A build of its own, with the autopilot (the release build users get has none).
Write-Host "Building with the dev tools..."
& cargo build --release -p brewed-texture-designer --features dev-tools --target-dir (Join-Path $repo "target/autopilot") --manifest-path (Join-Path $repo "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "The build failed" }
$exe = Join-Path $repo "target/autopilot/release/brewed-texture-designer"
if ($IsWindows -or $env:OS -eq "Windows_NT") { $exe += ".exe" }

$seed = $FirstSeed
for ($batch = 1; $batch -le $Batches; $batch++) {
    Write-Host "Batch $batch of ${Batches}: seeds $seed..$($seed + $Runs - 1), $Minutes minutes"
    $started = @()
    for ($i = 0; $i -lt $Runs; $i++, $seed++) {
        $started += [pscustomobject]@{
            Seed    = $seed
            Process = Start-Process -FilePath $exe -ArgumentList "--autopilot", $Minutes, "--seed", $seed -PassThru `
                -RedirectStandardOutput (Join-Path $out "seed-$seed.out.txt") -RedirectStandardError (Join-Path $out "seed-$seed.err.txt")
        }
    }
    foreach ($run in $started) {
        $run.Process.WaitForExit()
        $log = Join-Path ([System.IO.Path]::GetTempPath()) "brewed-autopilot-$($run.Seed)/autopilot.log"
        if (Test-Path $log) { Copy-Item $log (Join-Path $out "seed-$($run.Seed).autopilot.log") }
        "seed $($run.Seed): exit $($run.Process.ExitCode)" | Add-Content (Join-Path $out "exits.txt")
    }
}

# Every finding of every run, and how many each run had.
$summary = Join-Path $out "summary.txt"
foreach ($log in Get-ChildItem $out -Filter "seed-*.autopilot.log" | Sort-Object Name) {
    $findings = @(Select-String -Path $log.FullName -Pattern "FINDING")
    "$($log.Name): $($findings.Count) finding(s)" | Add-Content $summary
    foreach ($f in $findings) { "    $($f.Line)" | Add-Content $summary }
}
Write-Host "Done: $out"
Get-Content $summary -ErrorAction SilentlyContinue | Select-Object -First 40
