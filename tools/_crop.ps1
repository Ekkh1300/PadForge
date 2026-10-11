# Crop a band out of a screenshot and blow it up.
#
# PrintWindow captures at window size, and the header is only 52px tall, so a
# collision inside it is invisible at 1:1. Scaling the crop is what makes the
# actual overlap readable.
param(
    [string]$In = "D:\ds op\PadForge\tools\_shot_ui1.png",
    [string]$Out = "D:\ds op\PadForge\tools\_shot_hdr.png",
    [int]$X = 0,
    [int]$W = 700,
    [int]$Y = 0,
    [int]$H = 100,
    [int]$Scale = 2
)
Add-Type -AssemblyName System.Drawing
$src = [System.Drawing.Image]::FromFile($In)
$sw = [int]$src.Width
if ($W -le 0 -or $W -gt $sw) { $W = $sw }
$rect = [System.Drawing.Rectangle]::new($X, $Y, $W, $H)
$part = [System.Drawing.Bitmap]::new($W * $Scale, $H * $Scale)
$g = [System.Drawing.Graphics]::FromImage($part)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
$dest = [System.Drawing.Rectangle]::new(0, 0, $part.Width, $part.Height)
$g.DrawImage($src, $dest, $rect, [System.Drawing.GraphicsUnit]::Pixel)
$g.Dispose()
$part.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$part.Dispose()
$src.Dispose()
Write-Output "cropped -> $Out"
