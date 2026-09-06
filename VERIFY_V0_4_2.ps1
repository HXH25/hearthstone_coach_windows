$ErrorActionPreference = "Stop"

function Invoke-CargoChecked {
    param([Parameter(Mandatory=$true)][string[]]$CargoArgs)
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($CargoArgs -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Write-Host "[1/5] cargo fmt"
Invoke-CargoChecked @("fmt")

Write-Host "[2/5] cargo fmt --check"
Invoke-CargoChecked @("fmt", "--", "--check")

Write-Host "[3/5] cargo check --bin hearthcoach_demo"
Invoke-CargoChecked @("check", "--bin", "hearthcoach_demo")

Write-Host "[4/5] cargo test"
Invoke-CargoChecked @("test")

Write-Host "[5/5] example config JSON"
Get-Content .\hearthcoach_demo.example.json -Raw | ConvertFrom-Json | Out-Null

Write-Host "V0.4.2 verification passed." -ForegroundColor Green
