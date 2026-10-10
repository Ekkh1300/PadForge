# Click Test vibration, then capture a rapid burst of frames so the 350 ms
# hold cannot be missed by process-startup latency.
param([string]$OutPrefix = "_pulse", [int]$Frames = 10, [int]$GapMs = 45)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32p {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$p = Get-Process padforge -ErrorAction SilentlyContinue |
      Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "no padforge window"; exit 1 }
$h = $p.MainWindowHandle

$w = New-Object 'Win32p+RECT'
[Win32p]::GetWindowRect($h, [ref]$w) | Out-Null
$wd = $w.Right - $w.Left
$ht = $w.Bottom - $w.Top

[Win32p]::ShowWindow($h, 9) | Out-Null
[Win32p]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 700

# "Test vibration" centre, in window-rect coordinates.
[Win32p]::SetCursorPos($w.Left + 968, $w.Top + 276) | Out-Null
Start-Sleep -Milliseconds 300
[Win32p]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 90
[Win32p]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)

$bmp = New-Object System.Drawing.Bitmap($wd, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
for ($i = 1; $i -le $Frames; $i++) {
  [Win32p]::PrintWindow($h, $hdc, 2) | Out-Null
  $bmp.Save("$OutPrefix$i.png", [System.Drawing.Imaging.ImageFormat]::Png)
  Start-Sleep -Milliseconds $GapMs
}
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Dispose()
Write-Output "captured $Frames frames"
