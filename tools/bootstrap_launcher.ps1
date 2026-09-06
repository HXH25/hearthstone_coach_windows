[CmdletBinding()]
param(
    [switch]$ForceRebuild,
    [switch]$SkipBuildToolsInstall,
    [switch]$SkipHdtInstall,
    [switch]$SkipHearthstoneRepair,
    [switch]$KeepConsole
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$BootstrapDir = Join-Path $env:TEMP "hearthcoach-bootstrap"
$LogDir = Join-Path $ProjectRoot "logs"
$CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$RustupInit = Join-Path $BootstrapDir "rustup-init.exe"
$VsBuildToolsInstaller = Join-Path $BootstrapDir "vs_BuildTools.exe"
$HdtInstaller = Join-Path $BootstrapDir "HDT-Installer.exe"
$RustupUrl = "https://win.rustup.rs/x86_64"
$VsBuildToolsUrl = "https://aka.ms/vs/17/release/vs_BuildTools.exe"
$HdtWingetId = "HearthSim.HearthstoneDeckTracker"
$HdtReleaseApi = "https://api.github.com/repos/HearthSim/Hearthstone-Deck-Tracker/releases/latest"
$ConfigPath = Join-Path $ProjectRoot "hearthcoach_demo.json"

function Write-Step {
    param([string]$Message)
    Write-Host "`n== $Message ==" -ForegroundColor Cyan
}

function Write-Ok {
    param([string]$Message)
    Write-Host "[OK] $Message" -ForegroundColor Green
}

function Refresh-Path {
    $machine = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $user = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @($CargoBin, $machine, $user) | Where-Object { $_ -and $_.Trim() }
    $env:Path = ($parts -join ";")
}

function Ensure-Tls12 {
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        # PowerShell 7 / modern .NET may not need this legacy setting.
    }
}

function Download-OfficialFile {
    param(
        [Parameter(Mandatory=$true)][string]$Uri,
        [Parameter(Mandatory=$true)][string]$OutFile,
        [Parameter(Mandatory=$true)][string]$Label,
        [hashtable]$Headers = @{}
    )
    Write-Host "Downloading $Label..."
    New-Item -ItemType Directory -Force -Path (Split-Path $OutFile) | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $OutFile -Headers $Headers
    if (-not (Test-Path $OutFile) -or (Get-Item $OutFile).Length -lt 1024) {
        throw "$Label download failed or returned an invalid file."
    }
}

function Ensure-Rust {
    Refresh-Path
    $cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
    $rustc = Get-Command rustc.exe -ErrorAction SilentlyContinue
    if ($cargo -and $rustc) {
        Write-Ok "Rust already installed: $(& cargo --version)"
        return
    }

    Write-Step "Rust toolchain not found - installing rustup + stable Rust"
    Ensure-Tls12
    Download-OfficialFile -Uri $RustupUrl -OutFile $RustupInit -Label "official rustup-init"

    & $RustupInit -y --profile minimal --default-host x86_64-pc-windows-msvc --default-toolchain stable
    if ($LASTEXITCODE -ne 0) {
        throw "rustup-init failed with exit code $LASTEXITCODE"
    }

    Refresh-Path
    if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) {
        throw "Rust installation finished but cargo.exe is still unavailable. Expected: $CargoBin"
    }

    & rustup component add rustfmt
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "rustfmt component installation failed. HearthCoach can still run, but VERIFY may fail."
    }

    Write-Ok "Installed $(& cargo --version)"
}

function Get-RustHost {
    $hostLine = & rustc -vV | Where-Object { $_ -like "host:*" } | Select-Object -First 1
    if (-not $hostLine) { return "" }
    return ($hostLine -replace '^host:\s*', '').Trim()
}

function Find-VsWhere {
    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"),
        (Join-Path $env:ProgramFiles "Microsoft Visual Studio\Installer\vswhere.exe")
    ) | Where-Object { $_ -and (Test-Path $_) }
    return $candidates | Select-Object -First 1
}

function Find-VcVars64 {
    $vswhere = Find-VsWhere
    if (-not $vswhere) { return $null }

    $installationPath = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1)
    if (-not $installationPath) { return $null }

    $vcvars = Join-Path $installationPath "VC\Auxiliary\Build\vcvars64.bat"
    if (Test-Path $vcvars) { return $vcvars }
    return $null
}

function Install-VsBuildTools {
    if ($SkipBuildToolsInstall) {
        throw "MSVC C++ Build Tools are missing and automatic installation was disabled."
    }

    Write-Step "MSVC linker not found - installing Visual Studio 2022 Build Tools (C++)"
    Write-Host "Windows may show a UAC prompt. This is required by Microsoft to install compiler/linker components." -ForegroundColor Yellow
    Write-Host "This download is much larger than Rust and can take several minutes." -ForegroundColor Yellow

    Ensure-Tls12
    Download-OfficialFile -Uri $VsBuildToolsUrl -OutFile $VsBuildToolsInstaller -Label "official Visual Studio Build Tools bootstrapper"

    $args = @(
        "--quiet",
        "--wait",
        "--norestart",
        "--nocache",
        "--add", "Microsoft.VisualStudio.Workload.VCTools",
        "--includeRecommended"
    )
    $process = Start-Process -FilePath $VsBuildToolsInstaller -ArgumentList $args -Verb RunAs -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) {
        throw "Visual Studio Build Tools installer failed with exit code $($process.ExitCode)"
    }
    if ($process.ExitCode -eq 3010) {
        Write-Warning "Build Tools requested a reboot. Try launching HearthCoach now; reboot Windows if the compiler is still unavailable."
    }
}

function Ensure-NativeBuildEnvironment {
    $hostTriple = Get-RustHost
    Write-Host "Rust host: $hostTriple"
    if ($hostTriple -notlike "*-pc-windows-msvc") {
        Write-Ok "Non-MSVC Rust host detected; Visual Studio Build Tools are not required by this launcher."
        return $null
    }

    $vcvars = Find-VcVars64
    if (-not $vcvars) {
        Install-VsBuildTools
        $vcvars = Find-VcVars64
    }
    if (-not $vcvars) {
        throw "MSVC Rust is installed, but vcvars64.bat could not be found after Build Tools setup."
    }
    Write-Ok "MSVC C++ build environment: $vcvars"
    return $vcvars
}

function Find-HearthDbDll {
    # Explicit override wins.
    if ($env:HEARTHCOACH_HEARTHDB_DLL -and (Test-Path -LiteralPath $env:HEARTHCOACH_HEARTHDB_DLL -PathType Leaf)) {
        return (Resolve-Path -LiteralPath $env:HEARTHCOACH_HEARTHDB_DLL).Path
    }

    $direct = @()
    if ($env:LOCALAPPDATA) {
        $direct += (Join-Path $env:LOCALAPPDATA "HearthstoneDeckTracker\HearthDb.dll")
    }
    if ($env:APPDATA) {
        $direct += (Join-Path $env:APPDATA "HearthstoneDeckTracker\HearthDb.dll")
    }
    foreach ($candidate in $direct) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    $roots = @()
    if ($env:LOCALAPPDATA) { $roots += (Join-Path $env:LOCALAPPDATA "HearthstoneDeckTracker") }
    if ($env:APPDATA) { $roots += (Join-Path $env:APPDATA "HearthstoneDeckTracker") }
    if ($env:ProgramFiles) { $roots += (Join-Path $env:ProgramFiles "HearthstoneDeckTracker") }
    if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} "HearthstoneDeckTracker") }
    if ($env:ProgramData) { $roots += (Join-Path $env:ProgramData "chocolatey\lib\hearthstone-deck-tracker") }

    $matches = @()
    foreach ($root in $roots | Select-Object -Unique) {
        if (-not $root -or -not (Test-Path -LiteralPath $root)) { continue }
        try {
            $matches += Get-ChildItem -LiteralPath $root -Filter "HearthDb.dll" -File -Recurse -ErrorAction SilentlyContinue
        } catch {
            # Some protected folders can fail enumeration. Continue with the rest.
        }
    }

    $best = $matches | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    if ($best) { return $best.FullName }
    return $null
}

function Install-HdtWithWinget {
    $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $winget) { return $false }

    Write-Host "Installing Hearthstone Deck Tracker with winget package '$HdtWingetId'..." -ForegroundColor Yellow
    & $winget.Source install --id $HdtWingetId --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity
    $code = $LASTEXITCODE
    if ($code -eq 0) {
        return $true
    }

    Write-Warning "winget HDT install returned exit code $code; trying the official GitHub release installer instead."
    return $false
}

function Install-HdtFromOfficialGithub {
    Write-Host "Downloading Hearthstone Deck Tracker from the official HearthSim GitHub release..." -ForegroundColor Yellow
    Ensure-Tls12
    $headers = @{ "User-Agent" = "HearthCoach-Launcher/0.5.0.4" }
    $release = Invoke-RestMethod -Uri $HdtReleaseApi -Headers $headers
    if (-not $release -or -not $release.assets) {
        throw "Could not read the latest Hearthstone Deck Tracker release metadata from GitHub."
    }

    $asset = $release.assets | Where-Object {
        $_.name -match '(?i)^HDT[-_ ].*Installer.*\.exe$' -or $_.name -match '(?i)^HDT-Installer\.exe$'
    } | Select-Object -First 1

    if (-not $asset) {
        $asset = $release.assets | Where-Object { $_.name -match '(?i)installer.*\.exe$' } | Select-Object -First 1
    }
    if (-not $asset -or -not $asset.browser_download_url) {
        throw "The latest official HDT GitHub release did not expose a Windows installer asset."
    }

    Download-OfficialFile -Uri $asset.browser_download_url -OutFile $HdtInstaller -Label "official Hearthstone Deck Tracker installer" -Headers $headers
    $process = Start-Process -FilePath $HdtInstaller -ArgumentList "--silent" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Hearthstone Deck Tracker installer failed with exit code $($process.ExitCode)"
    }
}

function Ensure-HdtDependency {
    # Advanced users may provide an exported card database instead of HDT.
    if ($env:HEARTHCOACH_CARD_DB -and (Test-Path -LiteralPath $env:HEARTHCOACH_CARD_DB -PathType Leaf)) {
        Write-Ok "Using HEARTHCOACH_CARD_DB; HDT HearthDb.dll bootstrap is not required."
        return
    }

    $dll = Find-HearthDbDll
    if ($dll) {
        $env:HEARTHCOACH_HEARTHDB_DLL = $dll
        Write-Ok "HDT HearthDb.dll: $dll"
        return
    }

    if ($SkipHdtInstall) {
        throw "HDT HearthDb.dll is required but was not found, and automatic HDT installation was disabled. Install Hearthstone Deck Tracker or set HEARTHCOACH_HEARTHDB_DLL."
    }

    Write-Step "HDT card database dependency not found - installing Hearthstone Deck Tracker"
    Write-Host "HearthCoach uses HDT's HearthDb.dll/HearthMirror.dll as its local authoritative Battlegrounds card metadata source." -ForegroundColor Yellow
    Write-Host "The launcher will install the official HearthSim Hearthstone Deck Tracker package for the current Windows user." -ForegroundColor Yellow

    $installed = Install-HdtWithWinget
    if (-not $installed) {
        Install-HdtFromOfficialGithub
    }

    # Squirrel/winget can finish before filesystem timestamps fully settle.
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 750
        $dll = Find-HearthDbDll
        if ($dll) { break }
    } while ((Get-Date) -lt $deadline)

    if (-not $dll) {
        throw "HDT installation completed, but HearthDb.dll is still not discoverable. Expected under %LOCALAPPDATA%\HearthstoneDeckTracker\app-*\. Open HDT once, then rerun this launcher; or set HEARTHCOACH_HEARTHDB_DLL manually."
    }

    $env:HEARTHCOACH_HEARTHDB_DLL = $dll
    Write-Ok "Installed/detected HDT HearthDb.dll: $dll"
}


function Test-HearthstoneDir {
    param([string]$Path)
    if (-not $Path) { return $false }
    try {
        return Test-Path -LiteralPath (Join-Path $Path "Hearthstone.exe") -PathType Leaf
    } catch {
        return $false
    }
}

function Find-HearthstoneDir {
    if ($env:HEARTHCOACH_HEARTHSTONE_DIR -and (Test-HearthstoneDir $env:HEARTHCOACH_HEARTHSTONE_DIR)) {
        return (Resolve-Path -LiteralPath $env:HEARTHCOACH_HEARTHSTONE_DIR).Path
    }

    try {
        $running = Get-Process -Name "Hearthstone" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($running -and $running.Path) {
            $dir = Split-Path -Parent $running.Path
            if (Test-HearthstoneDir $dir) { return $dir }
        }
    } catch {
        # Protected process metadata may fail on unusual Windows setups.
    }

    if (Test-Path $ConfigPath) {
        try {
            $cfg = Get-Content $ConfigPath -Raw | ConvertFrom-Json
            $saved = [string]$cfg.hearthstone_dir
            if (Test-HearthstoneDir $saved) {
                return (Resolve-Path -LiteralPath $saved).Path
            }
        } catch {
            Write-Warning "Could not inspect saved hearthstone_demo.json while detecting Hearthstone: $($_.Exception.Message)"
        }
    }

    $candidates = New-Object System.Collections.Generic.List[string]
    foreach ($drive in Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue) {
        $root = $drive.Root
        foreach ($relative in @(
            "Hearthstone",
            "Games\Hearthstone",
            "Blizzard\Hearthstone",
            "Battle.net\Hearthstone",
            "Program Files\Hearthstone",
            "Program Files (x86)\Hearthstone",
            "Program Files\Blizzard Entertainment\Hearthstone",
            "Program Files (x86)\Blizzard Entertainment\Hearthstone"
        )) {
            $candidates.Add((Join-Path $root $relative))
        }
    }
    foreach ($candidate in $candidates) {
        if (Test-HearthstoneDir $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    return $null
}

function Update-HearthCoachHearthstoneConfig {
    param([string]$HearthstoneDir)
    if (-not $HearthstoneDir) { return }
    $env:HEARTHCOACH_HEARTHSTONE_DIR = $HearthstoneDir
    if (-not (Test-Path $ConfigPath)) { return }
    try {
        $cfg = Get-Content $ConfigPath -Raw | ConvertFrom-Json
        if ([string]$cfg.hearthstone_dir -ne $HearthstoneDir) {
            $cfg.hearthstone_dir = $HearthstoneDir
            $json = $cfg | ConvertTo-Json -Depth 20
            [System.IO.File]::WriteAllText($ConfigPath, $json, (New-Object System.Text.UTF8Encoding($false)))
            Write-Ok "Updated saved Hearthstone path: $HearthstoneDir"
        }
    } catch {
        Write-Warning "Could not update hearthcoach_demo.json automatically: $($_.Exception.Message)"
    }
}

function Test-PowerLoggingReady {
    param([string]$Text)
    if (-not $Text) { return $false }
    $match = [regex]::Match($Text, '(?ms)^\[Power\]\s*\r?\n(?<body>.*?)(?=^\[[^\]]+\]\s*$|\z)')
    if (-not $match.Success) { return $false }
    $body = $match.Groups['body'].Value
    $level = [regex]::Match($body, '(?im)^\s*LogLevel\s*=\s*(\d+)\s*$')
    $file = [regex]::Match($body, '(?im)^\s*FilePrinting\s*=\s*(True|1)\s*$')
    $verbose = [regex]::Match($body, '(?im)^\s*Verbose\s*=\s*(True|1)\s*$')
    return $level.Success -and ([int]$level.Groups[1].Value -ge 1) -and $file.Success -and $verbose.Success
}

function Ensure-HearthstonePowerLogging {
    if ($SkipHearthstoneRepair) {
        Write-Warning "Hearthstone logging repair was skipped by command-line option."
        return
    }
    if (-not $env:LOCALAPPDATA) {
        Write-Warning "LOCALAPPDATA is unavailable; cannot automatically configure Hearthstone Power.log."
        return
    }

    $dir = Join-Path $env:LOCALAPPDATA "Blizzard\Hearthstone"
    $path = Join-Path $dir "log.config"
    $content = if (Test-Path $path) { Get-Content -LiteralPath $path -Raw } else { "" }
    if (Test-PowerLoggingReady $content) {
        Write-Ok "Hearthstone Power logging is already enabled: $path"
        return
    }

    $power = "[Power]`r`nLogLevel=1`r`nFilePrinting=True`r`nConsolePrinting=False`r`nScreenPrinting=False`r`nVerbose=True`r`n"
    $pattern = '(?ms)^\[Power\]\s*\r?\n.*?(?=^\[[^\]]+\]\s*$|\z)'
    if ([regex]::IsMatch($content, $pattern)) {
        $newContent = [regex]::Replace($content, $pattern, $power)
    } else {
        $trimmed = $content.TrimEnd("`r", "`n")
        if ($trimmed) { $newContent = $trimmed + "`r`n`r`n" + $power }
        else { $newContent = $power }
    }

    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    if (Test-Path $path) {
        try { (Get-Item -LiteralPath $path).IsReadOnly = $false } catch {}
    }
    [System.IO.File]::WriteAllText($path, $newContent, (New-Object System.Text.UTF8Encoding($false)))
    Write-Ok "Enabled Hearthstone Power logging: $path"
    $running = Get-Process -Name "Hearthstone" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($running) {
        Write-Warning "Hearthstone is currently running. Restart Hearthstone once so the new log.config takes effect."
    }
}

function Ensure-HearthstoneEnvironment {
    Write-Step "Detecting Hearthstone installation and Power.log configuration"
    $dir = Find-HearthstoneDir
    if ($dir) {
        Update-HearthCoachHearthstoneConfig -HearthstoneDir $dir
        Write-Ok "Hearthstone: $dir"
    } else {
        Write-Warning "Hearthstone.exe was not found in common locations. HearthCoach will keep auto-detecting after startup; launching Hearthstone and clicking Environment -> Auto detect/repair will also resolve it."
    }
    Ensure-HearthstonePowerLogging
}

function Invoke-CargoBuild {
    param([string]$VcVars64)

    Write-Step "Building HearthCoach (release)"
    Push-Location $ProjectRoot
    try {
        if ($VcVars64) {
            $cmd = 'call "' + $VcVars64 + '" >nul && cargo build --release --bin hearthcoach'
            & $env:ComSpec /d /c $cmd
        } else {
            & cargo build --release --bin hearthcoach
        }
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

function Needs-Build {
    $exe = Join-Path $ProjectRoot "target\release\hearthcoach.exe"
    if ($ForceRebuild -or -not (Test-Path $exe)) { return $true }

    $exeTime = (Get-Item $exe).LastWriteTimeUtc
    $inputs = @(
        (Join-Path $ProjectRoot "Cargo.toml"),
        (Join-Path $ProjectRoot "Cargo.lock")
    )
    $sourceRoot = Join-Path $ProjectRoot "src"
    if (Test-Path $sourceRoot) {
        $inputs += Get-ChildItem $sourceRoot -Recurse -File -Include *.rs | Select-Object -ExpandProperty FullName
    }
    foreach ($input in $inputs) {
        if ($input -and (Test-Path $input) -and (Get-Item $input).LastWriteTimeUtc -gt $exeTime) {
            return $true
        }
    }
    return $false
}

function Start-HearthCoach {
    $exe = Join-Path $ProjectRoot "target\release\hearthcoach.exe"
    if (-not (Test-Path $exe)) {
        throw "Built executable not found: $exe"
    }

    $existing = Get-Process -Name "hearthcoach" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($existing) {
        Write-Ok "HearthCoach is already running (PID $($existing.Id))."
        return
    }

    New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
    $stdout = Join-Path $LogDir "hearthcoach.stdout.log"
    $stderr = Join-Path $LogDir "hearthcoach.stderr.log"

    Write-Step "Starting HearthCoach"
    $startArgs = @{
        FilePath = $exe
        WorkingDirectory = $ProjectRoot
        PassThru = $true
        RedirectStandardOutput = $stdout
        RedirectStandardError = $stderr
    }
    if (-not $KeepConsole) {
        $startArgs.WindowStyle = "Hidden"
    }
    $process = Start-Process @startArgs
    Start-Sleep -Milliseconds 1200

    if ($process.HasExited) {
        $details = ""
        if (Test-Path $stderr) {
            $details = (Get-Content $stderr -Tail 40 -ErrorAction SilentlyContinue) -join "`n"
        }
        throw "HearthCoach exited during startup (code $($process.ExitCode)).`n$details"
    }

    Write-Ok "HearthCoach started (PID $($process.Id))."
    Write-Host "Logs: $LogDir" -ForegroundColor DarkGray
}

try {
    if ($env:OS -ne "Windows_NT") {
        throw "The one-click launcher currently supports Windows only."
    }

    Set-Location $ProjectRoot
    Write-Host "HearthCoach One-click Launcher V0.5.0.7" -ForegroundColor White
    Write-Host "Project: $ProjectRoot" -ForegroundColor DarkGray

    Refresh-Path
    $rustMissing = -not (Get-Command cargo.exe -ErrorAction SilentlyContinue) -or -not (Get-Command rustc.exe -ErrorAction SilentlyContinue)
    if ($rustMissing) {
        Write-Step "Preparing Windows C++ prerequisites for the default Rust toolchain"
        if (-not (Find-VcVars64)) {
            Install-VsBuildTools
        } else {
            Write-Ok "Visual Studio C++ Build Tools already installed."
        }
    }

    Ensure-Rust
    $vcvars = Ensure-NativeBuildEnvironment

    if (Needs-Build) {
        Invoke-CargoBuild -VcVars64 $vcvars
    } else {
        Write-Ok "Release executable is up to date; skipping compilation."
    }

    # V0.5.0.3: the card metadata dependency is a first-class launcher prerequisite,
    # not a surprise runtime failure after compilation.
    Ensure-HdtDependency
    Ensure-HearthstoneEnvironment

    Start-HearthCoach
    Write-Host "`nYou can close this launcher window." -ForegroundColor Green
    exit 0
} catch {
    Write-Host "`n[ERROR] $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Project: $ProjectRoot" -ForegroundColor DarkGray
    Write-Host "Tip: run 'HearthCoach Launcher.cmd' again after fixing the reported prerequisite." -ForegroundColor Yellow
    exit 1
}
