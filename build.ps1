# Build both the portable app and the installer.
# NOTE: cargo writes progress to stderr; PowerShell 5.1 turns stderr into error
# records, which $ErrorActionPreference="Stop" would treat as fatal. So cargo is
# invoked through cmd, which merges stderr into stdout as plain text.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Invoke-Cargo {
    param([string]$Dir, [string]$What)
    Push-Location $Dir
    try {
        cmd /c "cargo build --release --color=never 2>&1"
        if ($LASTEXITCODE -ne 0) { throw "$What build failed (exit $LASTEXITCODE)" }
    } finally {
        Pop-Location
    }
}

# Running copies lock the exe on Windows.
Get-Process diskscope, diskscope-setup -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300

Write-Host "==> Building DiskScope (release)" -ForegroundColor Cyan
Invoke-Cargo $PSScriptRoot "DiskScope"

New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\diskscope.exe dist\DiskScope.exe -Force

Write-Host "==> Building installer" -ForegroundColor Cyan
Invoke-Cargo (Join-Path $PSScriptRoot "installer") "installer"
Copy-Item installer\target\release\diskscope-setup.exe dist\DiskScope-Setup.exe -Force

Write-Host ""
Write-Host "Done. Output in dist\:" -ForegroundColor Green
Get-ChildItem dist | Select-Object Name, @{ n = "Size (MB)"; e = { [math]::Round($_.Length / 1MB, 2) } } | Format-Table -AutoSize
