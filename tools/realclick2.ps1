Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
# close the file dialog with ESC (it has focus as owned modal)
[System.Windows.Forms.SendKeys]::SendWait("{ESC}")
Start-Sleep -Milliseconds 500
$bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$cx = [int]($bounds.Width * 0.55)
$cy = [int]($bounds.Height * 0.60)
# real right-click (reuse M type from prior session is gone; redefine quickly)
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class M2 {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = right ? 0x0008u : 0x0002u;
    l[1].U.mi.dwFlags = right ? 0x0010u : 0x0004u;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[M2]::Click($cx, $cy, $true)
Start-Sleep -Milliseconds 800
$bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$bmp.Save("D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\real2_menu.png")
Write-Output "done"
