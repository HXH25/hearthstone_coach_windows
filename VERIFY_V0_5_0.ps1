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

Invoke-Native "[1/6] cargo fmt" { cargo fmt }
Invoke-Native "[2/6] cargo fmt --check" { cargo fmt -- --check }
Invoke-Native "[3/6] cargo check --bin hearthcoach" { cargo check --bin hearthcoach }
Invoke-Native "[4/6] cargo check --bin hearthcoach_demo" { cargo check --bin hearthcoach_demo }
Invoke-Native "[5/6] cargo test" { cargo test }

Write-Host "`n== [6/6] config/docs sanity ==" -ForegroundColor Cyan
$example = Get-Content .\hearthcoach_demo.example.json -Raw | ConvertFrom-Json
if (-not $example.deepseek.context_window) { throw "missing deepseek.context_window" }
if (-not $example.deepseek.pricing) { throw "missing deepseek.pricing" }
if (-not $example.deepseek.budget) { throw "missing deepseek.budget" }
if (-not $example.compliance.history_dir) { throw "missing compliance.history_dir" }
if (-not (Test-Path .\V0_5_0_COURSE_COMPLIANCE.md)) { throw "missing V0_5_0_COURSE_COMPLIANCE.md" }

Write-Host "`nV0.5.0 verification passed." -ForegroundColor Green
Write-Host "Run: cargo run --bin hearthcoach" -ForegroundColor Green
