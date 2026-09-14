Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class M3 {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = 0x0002u; l[1].U.mi.dwFlags = 0x0004u;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
# find the dialog window of variable process
$pidv = (Get-Process variable | Select-Object -First 1).Id
$enums = @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public class EnumW {
  public delegate bool CB(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public static List<string> List(uint pid) {
    var outp = new List<string>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
        var c = new StringBuilder(256); GetClassNameW(h, c, 256);
        outp.Add(h.ToString() + " | " + c.ToString() + " | " + t.ToString());
      }
      return true;
    }, IntPtr.Zero);
    return outp;
  }
}
'@
Add-Type -TypeDefinition $enums
$wins = [EnumW]::List([uint32]$pidv)
$wins | ForEach-Object { Write-Output $_ }
# click the qu-xiao (Cancel) button at screen coords
[M3]::Click(559, 368)
Start-Sleep -Milliseconds 800
Write-Output "--- after cancel click ---"
[EnumW]::List([uint32]$pidv) | ForEach-Object { Write-Output $_ }
