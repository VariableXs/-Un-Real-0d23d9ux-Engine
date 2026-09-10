Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public class DC {
  public delegate bool CB(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] i, int s);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public MOUSEINPUT mi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
  public static string Dump(IntPtr dlg) {
    var sb = new StringBuilder();
    EnumChildWindows(dlg, (h, l) => {
      var t = new StringBuilder(128); GetWindowTextW(h, t, 128);
      var c = new StringBuilder(128); GetClassNameW(h, c, 128);
      RECT r; GetWindowRect(h, out r);
      sb.Append(h).Append(" [").Append(c).Append("] '").Append(t).Append("' (").Append((r.L+r.R)/2).Append(",").Append((r.T+r.B)/2).Append(")\n");
      return true;
    }, IntPtr.Zero);
    return sb.ToString();
  }
  public static void Click(int x, int y) {
    SetCursorPos(x, y); System.Threading.Thread.Sleep(200);
    var l = new INPUT[2];
    l[0].U.mi.dwFlags = 0x0002u; l[1].U.mi.dwFlags = 0x0004u;
    SendInput(2, l, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
$dlg = [IntPtr]5835850
Write-Output ([DC]::Dump($dlg))
