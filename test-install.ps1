# End-to-end test of the DiskScope installer / uninstaller.
# Message boxes are dismissed by clicking their actual button (BM_CLICK), which
# works regardless of control ids and without needing real foreground focus.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "uiprobe.ps1")

$setup = Join-Path $PSScriptRoot "dist\DiskScope-Setup.exe"
$dir   = Join-Path $env:LOCALAPPDATA "Programs\DiskScope"
$reg   = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DiskScope"
$lnkSm = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\DiskScope.lnk"
$lnkDk = Join-Path $env:USERPROFILE "Desktop\DiskScope.lnk"

# Keep clicking the requested button on every dialog this process shows until it
# exits (install shows one dialog, uninstall shows two).
function Answer-Dialogs {
    param([System.Diagnostics.Process]$Proc, [string[]]$Buttons = @("OK"), [int]$Timeout = 30)
    $clicked = 0
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ((Get-Date) -lt $deadline) {
        $Proc.Refresh()
        if ($Proc.HasExited) { return $clicked }
        $h = [UiProbe]::FindDialog([uint32]$Proc.Id)
        if ($h -ne [IntPtr]::Zero) {
            Start-Sleep -Milliseconds 250
            foreach ($b in $Buttons) {
                if ([UiProbe]::ClickButton($h, $b)) { $clicked++; Start-Sleep -Milliseconds 400; break }
            }
        }
        Start-Sleep -Milliseconds 200
    }
    return $clicked
}

$fail = 0
function Check([string]$name, [bool]$ok, [string]$detail = "") {
    if (-not $ok) { $script:fail++ }
    "{0,-20} {1} {2}" -f $name, $(if ($ok) { "OK  " } else { "FAIL" }), $detail
}

Write-Host "== cleanup" -ForegroundColor Cyan
Get-Process DiskScope, DiskScope-Setup -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
if (Test-Path $dir) { Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue }
Remove-Item $lnkSm, $lnkDk -Force -ErrorAction SilentlyContinue
Remove-Item $reg -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "== install" -ForegroundColor Cyan
$p = Start-Process -FilePath $setup -PassThru
$clicks = Answer-Dialogs $p @("OK")
Start-Sleep -Seconds 3

Check "dialogs answered" ($clicks -ge 1) "($clicks)"
Check "setup exited" $p.HasExited
Check "app exe" (Test-Path (Join-Path $dir "DiskScope.exe"))
Check "uninstaller" (Test-Path (Join-Path $dir "Uninstall.exe"))
Check "start menu lnk" (Test-Path $lnkSm)
Check "desktop lnk" (Test-Path $lnkDk)
$props = Get-ItemProperty $reg -ErrorAction SilentlyContinue
Check "registry" ($null -ne $props) $(if ($props) { "$($props.DisplayName) $($props.DisplayVersion)" })
Check "uninstall string" ($props.UninstallString -like "*Uninstall.exe* --uninstall") $props.UninstallString

$app = @(Get-Process DiskScope -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dir*" })
Check "app launched" ($app.Count -gt 0) $(if ($app.Count) { "pid=$($app[0].Id)" })

Write-Host "== uninstall while app is running" -ForegroundColor Cyan
$u = Start-Process -FilePath (Join-Path $dir "Uninstall.exe") -ArgumentList "--uninstall" -PassThru
$clicks = Answer-Dialogs $u @("Yes","OK")
Start-Sleep -Seconds 4

Check "dialogs answered" ($clicks -ge 1) "($clicks)"
Check "app exe gone" (-not (Test-Path (Join-Path $dir "DiskScope.exe")))
Check "app stopped" ($null -eq (Get-Process DiskScope -ErrorAction SilentlyContinue))
Check "start menu lnk gone" (-not (Test-Path $lnkSm))
Check "desktop lnk gone" (-not (Test-Path $lnkDk))
Check "registry gone" ($null -eq (Get-ItemProperty $reg -ErrorAction SilentlyContinue))
Check "install dir gone" (-not (Test-Path $dir))

Write-Host ""
if ($fail -eq 0) { Write-Host "ALL CHECKS PASSED" -ForegroundColor Green } else { Write-Host "$fail CHECK(S) FAILED" -ForegroundColor Red }
exit $fail

