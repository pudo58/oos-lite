Add-Type -AssemblyName System.Drawing

$size = 256
$bmp = New-Object System.Drawing.Bitmap($size, $size)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

# 1. Background rounded rectangle
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$r = 52
$rect = New-Object System.Drawing.Rectangle(12, 12, 232, 232)
$path.AddArc($rect.X, $rect.Y, $r, $r, 180, 90)
$path.AddArc($rect.Right - $r, $rect.Y, $r, $r, 270, 90)
$path.AddArc($rect.Right - $r, $rect.Bottom - $r, $r, $r, 0, 90)
$path.AddArc($rect.X, $rect.Bottom - $r, $r, $r, 90, 90)
$path.CloseFigure()

# Dark Indigo to Deep Slate gradient
$brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Point(0, 0)),
    (New-Object System.Drawing.Point($size, $size)),
    [System.Drawing.Color]::FromArgb(255, 30, 27, 75),   # Deep Indigo #1e1b4b
    [System.Drawing.Color]::FromArgb(255, 15, 23, 42)    # Slate 900 #0f172a
)
$g.FillPath($brush, $path)

# Subtle outer border glow
$penGlow = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(160, 99, 102, 241), 5) # Indigo 500
$g.DrawPath($penGlow, $path)

# 2. Outer Vault Tech Ring
$ringPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 56, 189, 248), 12) # Sky 400
$g.DrawEllipse($ringPen, 64, 56, 128, 128)

# 3. Vault Dial Teeth (4 ticks around ring)
$dialPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(200, 129, 140, 248), 8)
$g.DrawLine($dialPen, 128, 42, 128, 56)
$g.DrawLine($dialPen, 128, 184, 128, 198)
$g.DrawLine($dialPen, 50, 120, 64, 120)
$g.DrawLine($dialPen, 192, 120, 206, 120)

# 4. Center Padlock / Vault Core
$lockBodyBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Point(92, 110)),
    (New-Object System.Drawing.Point(164, 168)),
    [System.Drawing.Color]::FromArgb(255, 99, 102, 241),  # Indigo 500
    [System.Drawing.Color]::FromArgb(255, 79, 70, 229)   # Indigo 600
)
$lockBodyPath = New-Object System.Drawing.Drawing2D.GraphicsPath
$br = 16
$lRect = New-Object System.Drawing.Rectangle(94, 114, 68, 54)
$lockBodyPath.AddArc($lRect.X, $lRect.Y, $br, $br, 180, 90)
$lockBodyPath.AddArc($lRect.Right - $br, $lRect.Y, $br, $br, 270, 90)
$lockBodyPath.AddArc($lRect.Right - $br, $lRect.Bottom - $br, $br, $br, 0, 90)
$lockBodyPath.AddArc($lRect.X, $lRect.Bottom - $br, $br, $br, 90, 90)
$lockBodyPath.CloseFigure()
$g.FillPath($lockBodyBrush, $lockBodyPath)

# 5. Padlock Shackle (Arch)
$shacklePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 224, 231, 255), 10)
$shacklePath = New-Object System.Drawing.Drawing2D.GraphicsPath
$shacklePath.AddArc(106, 82, 44, 46, 180, 180)
$shacklePath.AddLine(150, 105, 150, 118)
$g.DrawPath($shacklePen, $shacklePath)

# 6. Keyhole in Lock Body
$khBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 15, 23, 42))
$g.FillEllipse($khBrush, 122, 128, 12, 12)
$khPoly = @(
    (New-Object System.Drawing.Point(124, 136)),
    (New-Object System.Drawing.Point(132, 136)),
    (New-Object System.Drawing.Point(135, 152)),
    (New-Object System.Drawing.Point(121, 152))
)
$g.FillPolygon($khBrush, $khPoly)

# Clean graphics
$g.Dispose()

# Save ICO
$hIcon = $bmp.GetHicon()
$icon = [System.Drawing.Icon]::FromHandle($hIcon)

$destDir1 = "installer"
$destDir2 = "cli"
New-Item -ItemType Directory -Force -Path $destDir1 | Out-Null
New-Item -ItemType Directory -Force -Path $destDir2 | Out-Null

$outPath1 = Join-Path $destDir1 "app.ico"
$outPath2 = Join-Path $destDir2 "app.ico"

$fs1 = [System.IO.File]::Open($outPath1, [System.IO.FileMode]::Create)
$icon.Save($fs1)
$fs1.Close()

$fs2 = [System.IO.File]::Open($outPath2, [System.IO.FileMode]::Create)
$icon.Save($fs2)
$fs2.Close()

$bmp.Dispose()
Write-Host "Icons generated successfully at $outPath1 and $outPath2"
