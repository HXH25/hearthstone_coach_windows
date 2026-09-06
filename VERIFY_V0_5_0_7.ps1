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

Invoke-Native "[1/9] cargo fmt" { cargo fmt }
Invoke-Native "[2/9] cargo fmt --check" { cargo fmt -- --check }
Invoke-Native "[3/9] cargo check --bin hearthcoach" { cargo check --bin hearthcoach }
Invoke-Native "[4/9] cargo check --bin hearthcoach_demo" { cargo check --bin hearthcoach_demo }
Invoke-Native "[5/9] cargo test" { cargo test }

Write-Host "`n== [6/9] config/docs sanity ==" -ForegroundColor Cyan
$example = Get-Content .\hearthcoach_demo.example.json -Raw | ConvertFrom-Json
if (-not $example.deepseek.context_window) { throw "missing deepseek.context_window" }
if (-not $example.deepseek.pricing) { throw "missing deepseek.pricing" }
if (-not $example.deepseek.budget) { throw "missing deepseek.budget" }
if (-not $example.compliance.history_dir) { throw "missing compliance.history_dir" }
if (-not (Test-Path .\V0_5_0_COURSE_COMPLIANCE.md)) { throw "missing V0_5_0_COURSE_COMPLIANCE.md" }
if (-not (Test-Path .\V0_5_0_7_POWER_LOG_REFRESH.md)) { throw "missing V0_5_0_7_POWER_LOG_REFRESH.md" }

Write-Host "`n== [7/9] latest Power.log switching sanity ==" -ForegroundColor Cyan
$monitor = Get-Content '.\src\harness\monitor.rs' -Raw
foreach ($needle in @('latest != followed_path', 'newer Power.log detected', 'force_refresh_generation', 'manual Power.log refresh requested', 'MatchInterrupted')) {
    if ($monitor -notlike "*$needle*") { throw "monitor missing expected refresh behavior: $needle" }
}
if ($monitor -like '*latest != followed_path && !runtime.is_match_active()*') {
    throw 'stale V0.5.0.6 active-match guard still blocks newer Power.log switching'
}
$runtime = Get-Content '.\src\harness\runtime.rs' -Raw
if ($runtime -notlike '*MatchInterrupted { reason: String }*') { throw 'HarnessEvent::MatchInterrupted missing' }

Write-Host "`n== [8/9] Control Center log refresh sanity ==" -ForegroundColor Cyan
$server = Get-Content '.\src\demo\server.rs' -Raw
foreach ($needle in @('request_log_refresh', 'log_refresh_generation_handle', 'watched_power_log', 'finalize_interrupted_session')) {
    if ($server -notlike "*$needle*") { throw "server missing log refresh lifecycle: $needle" }
}
$control = Get-Content '.\src\demo\control_center.rs' -Raw
foreach ($needle in @('刷新日志', 'ID_ENV_LOG_REFRESH', '当前监听日志', 'request_log_refresh')) {
    if ($control -notlike "*$needle*") { throw "Control Center missing log refresh UI: $needle" }
}

Write-Host "`n== [9/9] previous portability/launcher regressions ==" -ForegroundColor Cyan
$envSource = Get-Content '.\src\demo\environment.rs' -Raw
foreach ($needle in @('CreateToolhelp32Snapshot', 'Process32FirstW', 'QueryFullProcessImageNameW', 'OsString::from_wide')) {
    if ($envSource -notlike "*$needle*") { throw "environment source missing Unicode-safe discovery behavior: $needle" }
}
$overlay = Get-Content '.\src\demo\overlay.rs' -Raw
foreach ($needle in @('CardPortraits', 'CardTiles', 'strip_png_iccp_chunk')) {
    if ($overlay -notlike "*$needle*") { throw "overlay missing robust card art behavior: $needle" }
}
if (-not (Test-Path '.\HearthCoach Launcher.cmd')) { throw "missing HearthCoach Launcher.cmd" }
if (-not (Test-Path '.\tools\bootstrap_launcher.ps1')) { throw "missing tools/bootstrap_launcher.ps1" }

Write-Host "`nV0.5.0.7 verification passed." -ForegroundColor Green
Write-Host "Regression target: restart Hearthstone mid-match, then verify Environment shows the newest Power.log and the watcher switches without restarting HearthCoach." -ForegroundColor Green
