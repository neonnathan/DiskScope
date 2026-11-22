Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Drawing.Drawing2D

$out = Join-Path $PSScriptRoot 'icon.ico'
$sizes = @(16, 24, 32, 48, 64, 128, 256)

function New-RoundedPath([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

function New-IconBitmap([int]$s) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $pad = [float]($s * 0.03)
    $bg = New-RoundedPath $pad $pad ($s - 2*$pad) ($s - 2*$pad) ([float]($s * 0.22))
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        (New-Object System.Drawing.PointF(0, 0)),
        (New-Object System.Drawing.PointF($s, $s)),
        [System.Drawing.Color]::FromArgb(255, 20, 26, 38),
        [System.Drawing.Color]::FromArgb(255, 9, 12, 18))
    $g.FillPath($brush, $bg)
    $brush.Dispose()

    $cx = $s / 2.0
    $cy = $s / 2.0
    $outer = $s * 0.355
    $thick = $s * 0.165
    $inner = $outer - $thick
    $rect = New-Object System.Drawing.RectangleF(($cx - $outer), ($cy - $outer), (2*$outer), (2*$outer))

    $colors = @(
        [System.Drawing.Color]::FromArgb(255, 92, 148, 255),
        [System.Drawing.Color]::FromArgb(255, 63, 196, 178),
        [System.Drawing.Color]::FromArgb(255, 140, 124, 255),
        [System.Drawing.Color]::FromArgb(255, 232, 168, 92),
        [System.Drawing.Color]::FromArgb(255, 224, 108, 139)
    )
    $angles = @(0, 128, 206, 286, 336)

    for ($i = 0; $i -lt $colors.Count; $i++) {
        $start = $angles[$i]
        $sweep = if ($i -lt $colors.Count - 1) { $angles[$i + 1] - $start } else { 360 - $start }
        $sweep = $sweep - 3.0
        $pen = New-Object System.Drawing.Pen($colors[$i], [float]$thick)
        $pen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $pen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        $g.DrawArc($pen, $rect, [float]$start, [float]$sweep)
        $pen.Dispose()
    }

    # center dot
    $dot = $s * 0.045
    $white = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(235, 240, 246, 255))
    $g.FillEllipse($white, [float]($cx - $dot), [float]($cy - $dot), [float](2*$dot), [float](2*$dot))
    $white.Dispose()
    $bg.Dispose()
    $g.Dispose()
    return $bmp
}

$pngs = @()
foreach ($s in $sizes) {
    $bmp = New-IconBitmap $s
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngs += ,$ms.ToArray()
    $bmp.Dispose()
    $ms.Dispose()
}

$fs = [System.IO.File]::Create($out)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([UInt16]0)
$bw.Write([UInt16]1)
$bw.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]
    $b = $pngs[$i]
    $dim = if ($s -ge 256) { 0 } else { $s }
    $bw.Write([Byte]$dim)
    $bw.Write([Byte]$dim)
    $bw.Write([Byte]0)
    $bw.Write([Byte]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]32)
    $bw.Write([UInt32]$b.Length)
    $bw.Write([UInt32]$offset)
    $offset += $b.Length
}
foreach ($b in $pngs) { $bw.Write($b) }
$bw.Flush()
$bw.Dispose()
$fs.Dispose()

Write-Output "wrote $out ($((Get-Item $out).Length) bytes)"
