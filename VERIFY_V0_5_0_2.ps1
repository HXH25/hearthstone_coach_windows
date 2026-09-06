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

Invoke-Native "[1/7] cargo fmt" { cargo fmt }
Invoke-Native "[2/7] cargo fmt --check" { cargo fmt -- --check }
Invoke-Native "[3/7] cargo check --bin hearthcoach" { cargo check --bin hearthcoach }
Invoke-Native "[4/7] cargo check --bin hearthcoach_demo" { cargo check --bin hearthcoach_demo }
Invoke-Native "[5/7] cargo test" { cargo test }

Write-Host "`n== [6/7] config/docs sanity ==" -ForegroundColor Cyan
$example = Get-Content .\hearthcoach_demo.example.json -Raw | ConvertFrom-Json
if (-not $example.deepseek.context_window) { throw "missing deepseek.context_window" }
if (-not $example.deepseek.pricing) { throw "missing deepseek.pricing" }
if (-not $example.deepseek.budget) { throw "missing deepseek.budget" }
if (-not $example.compliance.history_dir) { throw "missing compliance.history_dir" }
if (-not (Test-Path .\V0_5_0_COURSE_COMPLIANCE.md)) { throw "missing V0_5_0_COURSE_COMPLIANCE.md" }

Write-Host "`n== [7/7] launcher sanity ==" -ForegroundColor Cyan
if (-not (Test-Path '.\HearthCoach Launcher.cmd')) { throw "missing HearthCoach Launcher.cmd" }
if (-not (Test-Path '.\tools\bootstrap_launcher.ps1')) { throw "missing tools/bootstrap_launcher.ps1" }
$launcher = Get-Content '.\tools\bootstrap_launcher.ps1' -Raw
foreach ($needle in @('win.rustup.rs', 'vs_BuildTools.exe', 'cargo build --release --bin hearthcoach', 'target\release\hearthcoach.exe')) {
    if ($launcher -notlike "*$needle*") { throw "launcher missing expected bootstrap step: $needle" }
}

Write-Host "`nV0.5.0.2 verification passed." -ForegroundColor Green
Write-Host "Normal users can now double-click: HearthCoach Launcher.cmd" -ForegroundColor Green
