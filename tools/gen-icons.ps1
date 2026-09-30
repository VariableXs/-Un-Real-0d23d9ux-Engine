# Generates Variable app icons: PNG sizes + multi-size icon.ico (PNG-compressed entries).
# Run: powershell -ExecutionPolicy Bypass -File tools\gen-icons.ps1
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root "src-tauri\icons"
New-Item -ItemType Directory -Force -Path $out | Out-Null

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    # Deep space rounded square background
    $r = [Math]::Max(2.0, $size * 0.18)
    $rect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($rect.X, $rect.Y, $d, $d, 180, 90)
    $path.AddArc($rect.Right - $d, $rect.Y, $d, $d, 270, 90)
    $path.AddArc($rect.Right - $d, $rect.Bottom - $d, $d, $d, 0, 90)
    $path.AddArc($rect.X, $rect.Bottom - $d, $d, $d, 90, 90)
    $path.CloseFigure()

    $bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        $rect,
        [System.Drawing.Color]::FromArgb(255, 10, 16, 34),
        [System.Drawing.Color]::FromArgb(255, 4, 6, 14),
        90.0)
    $g.FillPath($bgBrush, $path)

    # Stars
    $rand = New-Object System.Random(42)
    $starBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(200, 190, 210, 245))
    for ($i = 0; $i -lt 26; $i++) {
        $x = $rand.NextDouble() * $size
        $y = $rand.NextDouble() * $size
        $s = [Math]::Max(0.7, $size * ($rand.NextDouble() * 0.02 + 0.008))
        $g.FillEllipse($starBrush, [single]$x, [single]$y, [single]$s, [single]$s)
    }

    # V glyph
    $fontSize = $size * 0.62
    $font = New-Object System.Drawing.Font("Segoe UI", [single]$fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $fmt = New-Object System.Drawing.StringFormat
    $fmt.Alignment = [System.Drawing.StringAlignment]::Center
    $fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
    $textRect = New-Object System.Drawing.RectangleF(0, ($size * -0.03), $size, $size)
    $textBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 224, 234, 250))
    $g.DrawString("V", $font, $textBrush, $textRect, $fmt)

    $g.Dispose()
    return , $bmp
}

$sizes = @(16, 24, 32, 48, 64, 128, 256)
$bitmaps = @{}
foreach ($s in $sizes) { $bitmaps[$s] = New-IconBitmap $s }

# PNG outputs required by Tauri / Windows
$bitmaps[32].Save((Join-Path $out "32x32.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$bitmaps[128].Save((Join-Path $out "128x128.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$bitmaps[256].Save((Join-Path $out "128x128@2x.png"), [System.Drawing.Imaging.ImageFormat]::Png)
$bitmaps[256].Save((Join-Path $out "icon.png"), [System.Drawing.Imaging.ImageFormat]::Png)

# Build .ico containing PNG entries (valid on Windows Vista+) + BMP entry for 32px legacy.
$icoPath = Join-Path $out "icon.ico"
$entries = New-Object System.Collections.Generic.List[byte[]]
$count = $sizes.Count
$headerSize = 6
$dirEntrySize = 16
$dataOffset = $headerSize + $dirEntrySize * $count

$msEntries = New-Object System.Collections.Generic.List[object]
foreach ($s in $sizes) {
    $bmp = $bitmaps[$s]
    $pngStream = New-Object System.IO.MemoryStream
    if ($s -eq 32) {
        # Legacy BMP entry: BGRA bottom-up with AND mask.
        $w = 32; $h = 32
        $bmpData = $bmp.LockBits((New-Object System.Drawing.Rectangle(0, 0, $w, $h)), [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stride = $bmpData.Stride
        $pix = New-Object byte[] ($stride * $h)
        [System.Runtime.InteropServices.Marshal]::Copy($bmpData.Scan0, $pix, 0, $pix.Length)
        $bmp.UnlockBits($bmpData)
        $xorSize = $w * 4 * $h
        $andStride = [Math]::Ceiling($w / 8.0); $andStride = [int]([Math]::Ceiling($andStride / 4.0) * 4)
        $andSize = $andStride * $h
        $body = New-Object byte[] (40 + $xorSize + $andSize)
        # BITMAPINFOHEADER (biHeight = 2*h for ICO)
        [BitConverter]::GetBytes([uint32]40).CopyTo($body, 0)
        [BitConverter]::GetBytes([uint32]$w).CopyTo($body, 4)
        [BitConverter]::GetBytes([uint32]($h * 2)).CopyTo($body, 8)
        [BitConverter]::GetBytes([uint16]1).CopyTo($body, 12)
        [BitConverter]::GetBytes([uint16]32).CopyTo($body, 14)
        $row = New-Object byte[] ($w * 4)
        for ($yy = 0; $yy -lt $h; $yy++) {
            $srcRow = ($h - 1 - $yy) * $stride
            for ($xx = 0; $xx -lt $w; $xx++) {
                $b = $pix[$srcRow + $xx * 4]; $gg = $pix[$srcRow + $xx * 4 + 1]; $rr = $pix[$srcRow + $xx * 4 + 2]; $aa = $pix[$srcRow + $xx * 4 + 3]
                $dst = 40 + $yy * ($w * 4) + $xx * 4
                $body[$dst] = $b; $body[$dst + 1] = $gg; $body[$dst + 2] = $rr; $body[$dst + 3] = $aa
            }
        }
        $pngBytes = $body
        $isBmp = $true
    } else {
        $bmp.Save($pngStream, [System.Drawing.Imaging.ImageFormat]::Png)
        $pngBytes = $pngStream.ToArray()
        $isBmp = $false
    }
    $pngStream.Dispose()
    $msEntries.Add(@{ Size = $s; Data = $pngBytes; IsBmp = $isBmp })
}

$fs = [System.IO.File]::Create($icoPath)
$bw = New-Object System.IO.BinaryWriter($fs)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$count)
$offset = $dataOffset
foreach ($e in $msEntries) {
    $dim = if ($e.Size -ge 256) { 0 } else { $e.Size }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim)
    $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1)
    $bpp = if ($e.IsBmp) { 32 } else { 32 }
    $bw.Write([uint16]$bpp)
    $bw.Write([uint32]$e.Data.Length)
    $bw.Write([uint32]$offset)
    $offset += $e.Data.Length
}
foreach ($e in $msEntries) { $bw.Write($e.Data) }
$bw.Flush(); $bw.Dispose(); $fs.Dispose()

Write-Host "Icons written to $out"
