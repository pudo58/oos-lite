Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$svgPath = Join-Path $PSScriptRoot "app-icon.svg"

# 1. Prepare HTML harness for Edge rendering
$renderHtml = Join-Path $PSScriptRoot "temp_render.html"
$svgContent = Get-Content -Path $svgPath -Raw
$html = @"
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  html, body { width: 512px; height: 512px; background: transparent; overflow: hidden; }
  svg { width: 512px; height: 512px; display: block; }
</style>
</head>
<body>
$svgContent
</body>
</html>
"@
[System.IO.File]::WriteAllText($renderHtml, $html, [System.Text.Encoding]::UTF8)

# 2. Render 512x512 master PNG using Edge headless
$edgePath = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
$png512 = Join-Path $PSScriptRoot "app-icon-512.png"

$args = @(
    "--headless",
    "--disable-gpu",
    "--hide-scrollbars",
    "--default-background-color=00000000",
    "--window-size=512,512",
    "--screenshot=$png512",
    "file:///$renderHtml"
)
Start-Process -FilePath $edgePath -ArgumentList $args -Wait -NoNewWindow
Remove-Item -Path $renderHtml -Force -ErrorAction SilentlyContinue

if (-not (Test-Path $png512)) {
    Write-Error "Failed to generate master 512x512 PNG"
    exit 1
}

Write-Host "Generated master PNG: $png512"

# 3. Load master bitmap and generate sizes
$masterBmp = [System.Drawing.Bitmap]::FromFile($png512)

function Resize-Bitmap($src, $targetW, $targetH) {
    $dest = New-Object System.Drawing.Bitmap($targetW, $targetH, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($dest)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.DrawImage($src, (New-Object System.Drawing.Rectangle(0, 0, $targetW, $targetH)))
    $g.Dispose()
    return $dest
}

# Sizes to include in multi-resolution ICO
$sizes = @(256, 128, 64, 48, 32, 16)
$imagesData = @()

foreach ($sz in $sizes) {
    $resized = Resize-Bitmap $masterBmp $sz $sz
    $ms = New-Object System.IO.MemoryStream
    $resized.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bytes = $ms.ToArray()
    $ms.Dispose()
    $resized.Dispose()
    $imagesData += ,@($sz, $bytes)
}

# Also save 256 PNG standalone for docs/support
$png256Path = Join-Path $PSScriptRoot "app-icon-256.png"
$bmp256 = Resize-Bitmap $masterBmp 256 256
$bmp256.Save($png256Path, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp256.Dispose()

$masterBmp.Dispose()

# 4. Pack into standard ICO format
$icoMs = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($icoMs)

# ICO Header
$bw.Write([UInt16]0)                  # Reserved
$bw.Write([UInt16]1)                  # Type 1 = ICO
$bw.Write([UInt16]$imagesData.Count)  # Image count

# Calculate offsets
# Header (6 bytes) + Directory Entries (16 bytes each)
$offset = 6 + (16 * $imagesData.Count)

foreach ($item in $imagesData) {
    $sz = $item[0]
    $data = $item[1]

    $wByte = if ($sz -ge 256) { [byte]0 } else { [byte]$sz }
    $hByte = if ($sz -ge 256) { [byte]0 } else { [byte]$sz }

    $bw.Write([byte]$wByte)           # Width
    $bw.Write([byte]$hByte)           # Height
    $bw.Write([byte]0)                # Color palette count
    $bw.Write([byte]0)                # Reserved
    $bw.Write([UInt16]1)              # Color planes
    $bw.Write([UInt16]32)             # Bits per pixel
    $bw.Write([UInt32]$data.Length)   # Image size in bytes
    $bw.Write([UInt32]$offset)        # File offset

    $offset += $data.Length
}

# Write image streams
foreach ($item in $imagesData) {
    $bw.Write($item[1])
}

$bw.Flush()
$icoBytes = $icoMs.ToArray()
$bw.Dispose()
$icoMs.Dispose()

# 5. Distribute ICO to destinations
$destinations = @(
    (Join-Path $PSScriptRoot "app.ico"),
    (Join-Path $root "cli\app.ico"),
    (Join-Path $root "docs\support\app.ico")
)

foreach ($dst in $destinations) {
    $dir = Split-Path -Parent $dst
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    [System.IO.File]::WriteAllBytes($dst, $icoBytes)
    Write-Host "Updated icon at: $dst ($($icoBytes.Length) bytes)"
}

Write-Host "Multi-resolution icon generation complete!"
