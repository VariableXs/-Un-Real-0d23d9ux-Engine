Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class HUNG {
  [DllImport("user32.dll")] public static extern bool IsHungAppWindow(IntPtr h);
}
"@
$p = Get-Process variable | Select-Object -First 1
$h = [IntPtr]$p.MainWindowHandle
Write-Output ("main window hung: " + [HUNG]::IsHungAppWindow($h))
Write-Output ("CPU seconds: " + $p.CPU + "  threads: " + $p.Threads.Count)
Add-Type -AssemblyName System.Drawing; Add-Type -AssemblyName System.Windows.Forms
$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp=New-Object System.Drawing.Bitmap($b.Width,$b.Height)
$g=[System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0,0,0,0,$bmp.Size)
$bmp.Save("D:\2\14\-Un-Real-0d23d9ux-Engine-main\tools\hung_screen.png")
Write-Output "screen saved"
