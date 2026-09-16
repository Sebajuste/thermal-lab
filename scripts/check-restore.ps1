# Etat de la machine, tel que Thermal Lab le voit : le schema actif, ses deux valeurs
# telles qu'elles sont dans le registre, et le journal de restauration.
#
# Lecture seule, aucune elevation requise. A lancer trois fois pour verifier la garantie
# d'arret sur la vraie machine :
#
#   1. avant de lancer l'application        -> l'etat d'origine, journal absent
#   2. application lancee, bridage active   -> 0 / 99, journal present avec l'etat 1
#   3. application arretee, quelle qu'en soit la maniere (menu, Fin de tache, arret de
#      session puis relance de l'application) -> l'etat 1 revenu, journal absent
#
#   powershell -ExecutionPolicy Bypass -File scripts\check-restore.ps1

$ErrorActionPreference = 'Stop'

$SubProcessor   = '54533251-82be-4824-96c1-47b60b740d00'
$PerfBoostMode  = 'be337238-0d82-4146-a960-4f3749d470c7'
$ProcThrottleMax = 'bc5038f7-23e0-4960-96da-33abaf5935ec'

# Meme lecture que power.rs : le registre, car powercfg /query n'affiche rien pour ces
# reglages quand un outil tiers a masque leur attribut. Cle absente = defaut de Windows.
function Get-Setting($guid, $setting, $default) {
    $key = "HKLM:\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\$guid\$SubProcessor\$setting"
    try { (Get-ItemProperty -Path $key -Name ACSettingIndex).ACSettingIndex }
    catch { $default }
}

$active = powercfg /getactivescheme
$guid = [regex]::Match($active, '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}').Value
$name = [regex]::Match($active, '\(([^)]*)\)$').Groups[1].Value

$boost = Get-Setting $guid $PerfBoostMode 2
$throttle = Get-Setting $guid $ProcThrottleMax 100
$capped = ($boost -eq 0) -or ($throttle -lt 100)

Write-Host ""
Write-Host "Schema actif      : $name" -NoNewline
Write-Host "  ($guid)" -ForegroundColor DarkGray
Write-Host "  PERFBOOSTMODE   : $boost    (2 = turbo autorise, 0 = turbo interdit)"
Write-Host "  PROCTHROTTLEMAX : $throttle  (100 = plafond nominal, 99 = turbo interdit)"
Write-Host "  Turbo           : " -NoNewline
if ($capped) { Write-Host "BRIDE" -ForegroundColor Yellow } else { Write-Host "libre" -ForegroundColor Green }

$journal = Join-Path $env:APPDATA 'dev.sebajuste.thermal-lab\restore.json'
Write-Host ""
if (Test-Path $journal) {
    $b = Get-Content $journal -Raw | ConvertFrom-Json
    Write-Host "Journal          : present" -ForegroundColor Yellow
    Write-Host "  $journal" -ForegroundColor DarkGray
    Write-Host "  Etat a rendre   : boost $($b.boostMode), plafond $($b.throttleMax) sur $($b.schemeGuid)"
    if ($capped) {
        Write-Host "  -> intervention en cours : l'arret rendra cet etat." -ForegroundColor DarkGray
    } else {
        Write-Host "  -> dette residuelle : une restauration a echoue, elle sera retentee." -ForegroundColor Red
    }
} else {
    Write-Host "Journal          : absent" -ForegroundColor Green
    if ($capped) {
        Write-Host "  -> le bridage en place ne vient pas de Thermal Lab." -ForegroundColor DarkGray
    } else {
        Write-Host "  -> rien en cours, rien du. La machine est dans son etat d'origine." -ForegroundColor DarkGray
    }
}
Write-Host ""
