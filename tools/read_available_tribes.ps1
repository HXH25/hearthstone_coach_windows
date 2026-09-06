param(
    [Parameter(Mandatory = $true)]
    [string]$HearthMirrorDll
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $HearthMirrorDll -PathType Leaf)) {
    throw "HearthMirror.dll not found: $HearthMirrorDll"
}

$resolved = (Resolve-Path -LiteralPath $HearthMirrorDll).Path
$assemblyDir = Split-Path -Parent $resolved

# HearthMirror currently returns race enum values as integers in some builds.
# Resolve them through the exact HearthDb.dll shipped next to HDT so Rust sees
# stable semantic names (MECHANICAL, DRAGON, ...), not "17", "24", ...
$raceType = $null
$hearthDbPath = Join-Path $assemblyDir 'HearthDb.dll'
if (Test-Path -LiteralPath $hearthDbPath -PathType Leaf) {
    try {
        $hearthDbAssembly = [System.Reflection.Assembly]::LoadFrom($hearthDbPath)
        $raceType = $hearthDbAssembly.GetType('HearthDb.Enums.Race', $false)
    } catch { }
}

# Use the user's installed HDT/HearthMirror files in place. HearthCoach does not
# redistribute any of these assemblies.
$handler = [System.ResolveEventHandler]{
    param($sender, $eventArgs)
    try {
        $simpleName = (New-Object System.Reflection.AssemblyName($eventArgs.Name)).Name
        $candidate = Join-Path $assemblyDir ($simpleName + '.dll')
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return [System.Reflection.Assembly]::LoadFrom($candidate)
        }
    } catch { }
    return $null
}
[AppDomain]::CurrentDomain.add_AssemblyResolve($handler)

try {
    $assembly = [System.Reflection.Assembly]::LoadFrom($resolved)
    $reflectionType = $assembly.GetType('HearthMirror.Reflection', $true)
    $flags = [System.Reflection.BindingFlags]'Public,NonPublic,Static'

    # Reflection.Client has existed as either a property or field across
    # HearthMirror builds. Support both shapes.
    $client = $null
    $clientProperty = $reflectionType.GetProperty('Client', $flags)
    if ($null -ne $clientProperty) {
        $client = $clientProperty.GetValue($null)
    }
    if ($null -eq $client) {
        $clientField = $reflectionType.GetField('Client', $flags)
        if ($null -ne $clientField) {
            $client = $clientField.GetValue($null)
        }
    }
    if ($null -eq $client) {
        throw 'HearthMirror.Reflection.Client member not found or returned null'
    }

    $method = $client.GetType().GetMethods() | Where-Object {
        $_.Name -eq 'GetAvailableBattlegroundsRaces' -and $_.GetParameters().Count -eq 0
    } | Select-Object -First 1
    if ($null -eq $method) {
        throw 'GetAvailableBattlegroundsRaces() not found on HearthMirror client'
    }

    $races = $method.Invoke($client, @())
    if ($null -eq $races) {
        'null'
        exit 0
    }

    $names = @($races | ForEach-Object {
        $name = $_.ToString()
        $numeric = 0
        if ($null -ne $raceType -and [int]::TryParse($name, [ref]$numeric)) {
            $enumName = [System.Enum]::GetName($raceType, $numeric)
            if ($enumName) { $name = $enumName }
        }
        $name
    } | Where-Object { $_ -and $_ -ne 'INVALID' })
    if ($names.Count -eq 0) {
        'null'
    } else {
        # -InputObject preserves array shape even when exactly one value exists.
        ConvertTo-Json -InputObject $names -Compress
    }
} finally {
    [AppDomain]::CurrentDomain.remove_AssemblyResolve($handler)
}
