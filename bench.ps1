# Launch DiskScope against a root and sample CPU / memory until the scan stops
# changing, then screenshot. Used to check real-world scan cost.
param(
    [string]$Root = "C:\",
    [int]$Seconds = 180,
    [int]$Interval = 1,
    [string]$Out = "$PSScriptRoot\shot_full.png"
)
$ErrorActionPreference = "Stop"
$exe = Join-Path $PSScriptRoot "dist\DiskScope.exe"
Get-Process DiskScope -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

$p = Start-Process -FilePath $exe -ArgumentList "`"$Root`"" -PassThru
$sw = [Diagnostics.Stopwatch]::StartNew()
$samples = @()
$lastCpu = 0.0
$idle = 0
while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
    Start-Sleep -Seconds $Interval
    $p.Refresh()
    if ($p.HasExited) { "process exited early"; break }
    $cpu = $p.TotalProcessorTime.TotalSeconds
    $ws = [math]::Round($p.WorkingSet64 / 1MB, 1)
    $delta = [math]::Round($cpu - $lastCpu, 2)
    $lastCpu = $cpu
    $samples += [pscustomobject]@{ t = [math]::Round($sw.Elapsed.TotalSeconds); cpu_s = [math]::Round($cpu, 1); cpu_5s = $delta; ws_mb = $ws }
    if ($delta -lt 0.2) { $idle++ } else { $idle = 0 }
    if ($idle -ge 3) { "scan looks finished at ~$([math]::Round($sw.Elapsed.TotalSeconds))s"; break }
}
$samples | Format-Table -AutoSize
"peak working set: {0} MB" -f (($samples | Measure-Object ws_mb -Maximum).Maximum)

# Screenshot the window we already have open.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Cap {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$p.Refresh()
$h = $p.MainWindowHandle
[Cap]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 800
$r = New-Object Cap+RECT
[Cap]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $ht = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $ht)))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"screenshot: $Out ${w}x${ht}"

