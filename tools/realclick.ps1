# -*- coding: utf-8 -*-
# Real mouse input via SendInput against the live Variable window.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class M {
  [StructLayout(LayoutKind.Sequential)]
  public struct INPUT { public uint type; public InputUnion U; }
  [StructLayout(LayoutKind.Explicit)]
  public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)]
  public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public const uint MOVE=0x0001, LEFTDOWN=0x0002, LEFTUP=0x0004, RIGHTDOWN=0x0008, RIGHTUP=0x0010, ABS=0x8000;
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(120);
    var list = new INPUT[2];
    list[0].type = 0; list[0].U.mi.dwFlags = right ? RIGHTDOWN : LEFTDOWN;
    list[1].type = 0; list[1].U.mi.dwFlags = right ? RIGHTUP : LEFTUP;
    SendInput(2, list, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

# bring Variable to front
$h = (Get-Process variable | Select-Object -First 1).MainWindowHandle
if ($h -eq 0) { Write-Output "WINDOW NOT FOUND"; exit 1 }
$h = [IntPtr]$h
[M]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500

$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Write-Output ("screen: {0}x{1}" -f $bounds.Width, $bounds.Height)

$cx = [int]($bounds.Width * 0.55)
$cy = [int]($bounds.Height * 0.60)
[M]::Click($cx, $cy, $true)   # real right-click on desktop
Start-Sleep -Milliseconds 800

$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$bmp.Save("D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real1_menu.png")
Write-Output "saved real1_menu.png; right-clicked at $cx,$cy"
