Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class M6 {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(200);
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = right ? 0x0008u : 0x0002u;
    l[1].U.mi.dwFlags = right ? 0x0010u : 0x0004u;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
# real LEFT click on Variable Code icon (approx virtualized 57,400 from screenshot)
[M6]::Click(57, 400, $false)
Start-Sleep -Milliseconds 1500
Add-Type -AssemblyName System.Drawing; Add-Type -AssemblyName System.Windows.Forms
$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height)
$g=[System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0,0,0,0,$bmp.Size)
$bmp.Save("D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\after_icon_click.png")
Write-Output "clicked icon"
