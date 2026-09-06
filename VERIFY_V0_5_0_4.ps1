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
if (-not (Test-Path .\V0_5_0_4_PORTABLE_ENVIRONMENT.md)) { throw "missing V0_5_0_4_PORTABLE_ENVIRONMENT.md" }

Write-Host "`n== [7/8] portable environment source sanity ==" -ForegroundColor Cyan
if (-not (Test-Path '.\src\demo\environment.rs')) { throw "missing src/demo/environment.rs" }
$envSource = Get-Content '.\src\demo\environment.rs' -Raw
foreach ($needle in @('HEARTHCOACH_HEARTHSTONE_DIR', 'Hearthstone.exe', 'log.config', '[Power]', 'latest_power_log')) {
    if ($envSource -notlike "*$needle*") { throw "environment source missing expected behavior: $needle" }
}
if ($envSource -like '*PathBuf::from(r"D:\Hearthstone")*') { throw "machine-specific D:\Hearthstone default still present" }

Write-Host "`n== [8/8] launcher sanity ==" -ForegroundColor Cyan
if (-not (Test-Path '.\HearthCoach Launcher.cmd')) { throw "missing HearthCoach Launcher.cmd" }
if (-not (Test-Path '.\tools\bootstrap_launcher.ps1')) { throw "missing tools/bootstrap_launcher.ps1" }
$launcher = Get-Content '.\tools\bootstrap_launcher.ps1' -Raw
foreach ($needle in @('win.rustup.rs', 'vs_BuildTools.exe', 'cargo build --release --bin hearthcoach', 'HearthSim.HearthstoneDeckTracker', 'HEARTHCOACH_HEARTHDB_DLL', 'Find-HearthstoneDir', 'HEARTHCOACH_HEARTHSTONE_DIR', 'log.config', 'Verbose=True')) {
    if ($launcher -notlike "*$needle*") { throw "launcher missing expected bootstrap step: $needle" }
}

Write-Host "`nV0.5.0.4 verification passed." -ForegroundColor Green
Write-Host "Normal users can double-click: HearthCoach Launcher.cmd" -ForegroundColor Green
