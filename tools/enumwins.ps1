# UTF-8 BOM required for PS5.1
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public class EW2 {
  public delegate bool CB(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h);
}
"@
$rows = New-Object System.Collections.ArrayList
$cb = [EW2+CB]{
  param($h, $l)
  if ([EW2]::IsWindowVisible($h)) {
    $pid2 = 0
    [EW2]::GetWindowThreadProcessId($h, [ref]$pid2) | Out-Null
    $t = New-Object System.Text.StringBuilder 256
    [EW2]::GetWindowTextW($h, $t, 256) | Out-Null
    $c = New-Object System.Text.StringBuilder 256
    [EW2]::GetClassNameW($h, $c, 256) | Out-Null
    $hung = [EW2]::IsHungAppWindow($h)
    $owner = [EW2]::GetWindow($h, 4)  # GW_OWNER
    [void]$rows.Add(([pscustomobject]@{ HWND=$h; PID=$pid2; Class=$c.ToString(); Title=$t.ToString(); Hung=$hung; Owner=$owner }))
  }
  return $true
}
[EW2]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$rows | Where-Object { $_.PID -eq (Get-Process variable).Id -or $_.Class -like "*32770*" -or $_.Title -match "打开|取消|Variable" } | Format-Table -AutoSize | Out-String -Width 200
Write-Output "--- all visible top-level ---"
$rows | Format-Table -AutoSize | Out-String -Width 200
