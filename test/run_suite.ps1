param(
    [string]$LogDirName = ".log"
)

$ErrorActionPreference = "Continue"

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$logDir = Join-Path $projectRoot $LogDirName
$null = New-Item -ItemType Directory -Path $logDir -Force

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$summaryPath = Join-Path $logDir "$timestamp-summary.log"

$commands = @(
    @{ Name = "cargo_check";      Exe = "cargo"; Args = @("check") },
    @{ Name = "cargo_check_wasm"; Exe = "cargo"; Args = @("check", "--target", "wasm32-unknown-unknown") },
    @{ Name = "cargo_test";       Exe = "cargo"; Args = @("test") },
    @{ Name = "cargo_run_help";   Exe = "cargo"; Args = @("run", "--", "--help") },
    @{ Name = "cargo_bench";      Exe = "cargo"; Args = @("bench", "--bench", "core_bench") }
)

$summary = New-Object System.Collections.Generic.List[string]
$summary.Add("Run timestamp: $(Get-Date -Format o)")
$summary.Add("Project root: $projectRoot")
$summary.Add("")

Push-Location $projectRoot
try {
    foreach ($cmd in $commands) {
        $name = $cmd.Name
        $logPath = Join-Path $logDir "$timestamp-$name.log"
        $commandString = "$($cmd.Exe) $($cmd.Args -join ' ')"
        $tmpStdout = Join-Path $logDir "$timestamp-$name.stdout.tmp"
        $tmpStderr = Join-Path $logDir "$timestamp-$name.stderr.tmp"

        "[INFO] Running: $commandString" | Tee-Object -FilePath $logPath
        $proc = Start-Process `
            -FilePath $cmd.Exe `
            -ArgumentList $cmd.Args `
            -Wait `
            -NoNewWindow `
            -PassThru `
            -RedirectStandardOutput $tmpStdout `
            -RedirectStandardError $tmpStderr

        if (Test-Path $tmpStdout) {
            Get-Content $tmpStdout | Tee-Object -FilePath $logPath -Append
            Remove-Item $tmpStdout -Force -ErrorAction SilentlyContinue
        }
        if (Test-Path $tmpStderr) {
            Get-Content $tmpStderr | Tee-Object -FilePath $logPath -Append
            Remove-Item $tmpStderr -Force -ErrorAction SilentlyContinue
        }

        $exitCode = $proc.ExitCode

        if ($exitCode -eq 0) {
            $summary.Add("[PASS] $name")
        } else {
            $summary.Add("[FAIL] $name (exit $exitCode)")
        }
        $summary.Add("  log: $logPath")
        $summary.Add("")
    }
}
finally {
    Pop-Location
}

$summary | Set-Content -Path $summaryPath
$summary | ForEach-Object { Write-Host $_ }

if ($summary -match "^\[FAIL\]") {
    exit 1
}
