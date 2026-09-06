param(
    [Parameter(Mandatory = $true)]
    [string]$HearthDbDll,

    [Parameter(Mandatory = $false)]
    [string]$CardDefsBase,

    [Parameter(Mandatory = $true)]
    [string]$Output
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $HearthDbDll -PathType Leaf)) {
    throw "HearthDb.dll not found: $HearthDbDll"
}

$assembly = [System.Reflection.Assembly]::LoadFrom((Resolve-Path -LiteralPath $HearthDbDll))
$cardsType = $assembly.GetType('HearthDb.Cards', $true)
$localeType = $assembly.GetType('HearthDb.Enums.Locale', $true)
$gameTagType = $assembly.GetType('HearthDb.Enums.GameTag', $true)
$activateTooltipTag = $null
try { $activateTooltipTag = [System.Enum]::Parse($gameTagType, 'BACON_ACTIVATE_TOOLTIP') } catch { }

# HDT may have a newer downloaded CardDefs.base.xml than the snapshot bundled
# in HearthDb.dll. When present, load that exact HDT cache before querying All.
if ($CardDefsBase -and (Test-Path -LiteralPath $CardDefsBase -PathType Leaf)) {
    $loadMethod = $cardsType.GetMethods() | Where-Object {
        $_.Name -eq 'LoadBaseData' -and
        $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.FullName -eq 'System.IO.Stream'
    } | Select-Object -First 1
    if ($null -eq $loadMethod) {
        throw 'HearthDb.Cards.LoadBaseData(Stream) not found'
    }
    $stream = [System.IO.File]::OpenRead((Resolve-Path -LiteralPath $CardDefsBase))
    try {
        $null = $loadMethod.Invoke($null, @($stream))
    } finally {
        $stream.Dispose()
    }
}
$zhCN = [System.Enum]::Parse($localeType, 'zhCN')
$enUS = [System.Enum]::Parse($localeType, 'enUS')

# Accessing Cards.All triggers HearthDb's normal static initialization. HDT uses
# this same dictionary for CardId -> static card metadata lookups.
$all = $cardsType.GetProperty('All').GetValue($null)
# HearthDb builds this dictionary from Card.IsBaconPoolMinion after the exact
# CardDefs snapshot above is loaded. Premium triples intentionally are not in it.
$baconPool = $cardsType.GetProperty('BaconPoolMinions').GetValue($null)
$tripleMap = $cardsType.GetProperty('TripleToNormalCardIds').GetValue($null)

$rows = New-Object System.Collections.Generic.List[object]
foreach ($entry in $all.GetEnumerator()) {
    $card = $entry.Value

    $normalCardId = $null
    if ($tripleMap -ne $null -and $tripleMap.ContainsKey($card.Id)) {
        $normalCardId = [string]$tripleMap[$card.Id]
    }

    $nameZh = $null
    $nameEn = $null
    $textZh = $null
    $textEn = $null
    $mechanics = @()
    $activateKeyword = $false
    try { $nameZh = [string]$card.GetLocName($zhCN) } catch { }
    try { $nameEn = [string]$card.GetLocName($enUS) } catch { }
    try { $textZh = [string]$card.GetLocText($zhCN) } catch { }
    try { $textEn = [string]$card.GetLocText($enUS) } catch { }
    try { $mechanics = @($card.Mechanics | ForEach-Object { [string]$_ }) } catch { }
    if ($null -ne $activateTooltipTag) {
        try { $activateKeyword = ([int]$card.Entity.GetTag($activateTooltipTag)) -gt 0 } catch { }
    }

    $tier = [int]$card.TechLevel
    if ($tier -le 0) { $tierValue = $null } else { $tierValue = $tier }

    $rows.Add([pscustomobject]@{
        card_id = [string]$card.Id
        dbf_id = [int]$card.DbfId
        name_zh_cn = $nameZh
        name_en_us = $nameEn
        text_zh_cn = $textZh
        text_en_us = $textEn
        mechanics = $mechanics
        activate_keyword = [bool]$activateKeyword
        in_bacon_pool = [bool]($baconPool -ne $null -and $baconPool.ContainsKey($card.Id))
        card_type = [string]$card.Type
        race = [string]$card.Race
        secondary_race = [string]$card.SecondaryRace
        tavern_tier = $tierValue
        cost = [int]$card.Cost
        attack = [int]$card.Attack
        health = [int]$card.Health
        premium = [bool]$card.Premium
        normal_card_id = $normalCardId
    })
}

$parent = Split-Path -Parent $Output
if ($parent -and -not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}

# Windows PowerShell 5.1 writes an UTF-8 BOM. Rust's loader explicitly accepts it.
$rows | ConvertTo-Json -Depth 4 -Compress | Set-Content -LiteralPath $Output -Encoding UTF8
Write-Output "Exported $($rows.Count) HearthDb cards to $Output"
