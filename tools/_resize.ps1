param(
    [int]$W = 1360,
    [int]$H = 860,
    [int]$WaitMs = 800,
    [string]$Proc = "padforge"
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32size {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
}
"@
Start-Sleep -Milliseconds $WaitMs
$p = Get-Process $Proc -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { Write-Output "no window for '$Proc'"; exit 1 }
# SWP_NOMOVE|SWP_NOZORDER = 0x0005
[Win32size]::SetWindowPos($p.MainWindowHandle, [IntPtr]::Zero, 80, 60, $W, $H, 0x0005) | Out-Null
Start-Sleep -Milliseconds 900
Write-Output "resized $($p.MainWindowHandle) to ${W}x${H}"
