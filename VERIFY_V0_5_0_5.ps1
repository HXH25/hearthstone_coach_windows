$ErrorActionPreference = "Stop"

function Invoke-Native {
    param(
        [Parameter(Mandatory=$true)][string]$Name,
        [Parameter(Mandatory=$true)][scriptblock]$Command
    )
    Write-Host "`n== $Name ==" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

Invoke-Native "[1/8] cargo fmt" { cargo fmt }
Invoke-Native "[2/8] cargo fmt --check" { cargo fmt -- --check }
Invoke-Native "[3/8] cargo check --bin hearthcoach" { cargo check --bin hearthcoach }
Invoke-Native "[4/8] cargo check --bin hearthcoach_demo" { cargo check --bin hearthcoach_demo }
Invoke-Native "[5/8] cargo test" { cargo test }

Write-Host "`n== [6/8] config/docs sanity ==" -ForegroundColor Cyan
$example = Get-Content .\hearthcoach_demo.example.json -Raw | ConvertFrom-Json
if (-not $example.deepseek.context_window) { throw "missing deepseek.context_window" }
if (-not $example.deepseek.pricing) { throw "missing deepseek.pricing" }
if (-not $example.deepseek.budget) { throw "missing deepseek.budget" }
if (-not $example.compliance.history_dir) { throw "missing compliance.history_dir" }
if (-not (Test-Path .\V0_5_0_COURSE_COMPLIANCE.md)) { throw "missing V0_5_0_COURSE_COMPLIANCE.md" }
if (-not (Test-Path .\V0_5_0_5_UNICODE_PATH_FIX.md)) { throw "missing V0_5_0_5_UNICODE_PATH_FIX.md" }

Write-Host "`n== [7/8] Unicode-safe Windows discovery source sanity ==" -ForegroundColor Cyan
$envSource = Get-Content '.\src\demo\environment.rs' -Raw
foreach ($needle in @('CreateToolhelp32Snapshot', 'Process32FirstW', 'QueryFullProcessImageNameW', 'PROCESS_QUERY_LIMITED_INFORMATION', 'OsString::from_wide', 'running Hearthstone process (Win32 Unicode)')) {
    if ($envSource -notlike "*$needle*") { throw "environment source missing Unicode-safe discovery behavior: $needle" }
}
if ($envSource -like '*Get-Process Hearthstone*') { throw "runtime still shells out to PowerShell for Hearthstone path" }
if ($envSource -like '*tasklist.exe*') { throw "runtime still parses tasklist output" }
if ($envSource -like '*String::from_utf8_lossy(&output.stdout)*') { throw "runtime still decodes process-path stdout as UTF-8" }

Write-Host "`n== [8/8] launcher sanity ==" -ForegroundColor Cyan
if (-not (Test-Path '.\HearthCoach Launcher.cmd')) { throw "missing HearthCoach Launcher.cmd" }
if (-not (Test-Path '.\tools\bootstrap_launcher.ps1')) { throw "missing tools/bootstrap_launcher.ps1" }
$launcher = Get-Content '.\tools\bootstrap_launcher.ps1' -Raw
foreach ($needle in @('cargo build --release --bin hearthcoach', 'HearthSim.HearthstoneDeckTracker', 'Ensure-HearthstoneEnvironment')) {
    if ($launcher -notlike "*$needle*") { throw "launcher missing expected bootstrap step: $needle" }
}

Write-Host "`nV0.5.0.5 verification passed." -ForegroundColor Green
Write-Host "Regression target: a running Hearthstone installed under a Unicode path such as D:\应用\Hearthstone." -ForegroundColor Green
