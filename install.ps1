# Install (or update) DiskScope from the latest GitHub release.
#
#   irm https://raw.githubusercontent.com/neonnathan/DiskScope/main/install.ps1 | iex
#
# Downloads DiskScope-Setup.exe to %TEMP% and runs it silently: the app lands in
# %LOCALAPPDATA%\Programs\DiskScope with Start Menu + Desktop shortcuts and an
# "Installed apps" entry. No admin rights needed. Running it again upgrades.

$ErrorActionPreference = "Stop"
$repo = "neonnathan/DiskScope"

Write-Host "DiskScope: looking up the latest release..." -ForegroundColor Cyan
$release = Invoke-RestMethod `
    -Uri "https://api.github.com/repos/$repo/releases/latest" `
    -Headers @{ "User-Agent" = "DiskScope-installer"; "Accept" = "application/vnd.github+json" }

$asset = $release.assets | Where-Object { $_.name -eq "DiskScope-Setup.exe" } | Select-Object -First 1
if (-not $asset) {
    throw "Release $($release.tag_name) has no DiskScope-Setup.exe asset."
}

$setup = Join-Path $env:TEMP "DiskScope-Setup.exe"
Write-Host "DiskScope: downloading $($release.tag_name)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $setup -UseBasicParsing

Write-Host "DiskScope: installing..." -ForegroundColor Cyan
$proc = Start-Process -FilePath $setup -ArgumentList "/S" -Wait -PassThru
if ($proc.ExitCode -ne 0) {
    $log = Join-Path $env:TEMP "diskscope-setup.log"
    $detail = if (Test-Path $log) { Get-Content $log -Raw } else { "no details" }
    throw "Installer failed (exit $($proc.ExitCode)): $detail"
}
Remove-Item $setup -Force -ErrorAction SilentlyContinue

$app = Join-Path $env:LOCALAPPDATA "Programs\DiskScope\DiskScope.exe"
Write-Host ""
Write-Host "DiskScope $($release.tag_name) installed." -ForegroundColor Green
Write-Host "  $app"
Write-Host "  Start Menu -> DiskScope, or run the path above."
Write-Host ""
Write-Host "Uninstall:  & `"$(Join-Path $env:LOCALAPPDATA 'Programs\DiskScope\Uninstall.exe')`" --uninstall" -ForegroundColor DarkGray
