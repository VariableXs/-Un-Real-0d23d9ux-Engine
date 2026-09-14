param([int]$x, [int]$y, [switch]$right)
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class MR {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(180);
    var down = right ? 0x0008u : 0x0002u; var up = right ? 0x0010u : 0x0004u;
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = down; l[1].U.mi.dwFlags = up;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[MR]::Click($x, $y, [bool]$right)
Write-Output "clicked $x,$y right=$right"
