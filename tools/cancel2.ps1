Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class M4 {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = 0x0002u; l[1].U.mi.dwFlags = 0x0004u;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[M4]::Click(838, 552)
Start-Sleep -Milliseconds 800
Add-Type -AssemblyName System.Drawing; Add-Type -AssemblyName System.Windows.Forms
$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height)
$g=[System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0,0,0,0,$bmp.Size)
$bmp.Save("D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real4_after_cancel.png")
Write-Output "clicked 838,552"
