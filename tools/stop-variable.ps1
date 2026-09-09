# Stops Variable instances running from inside this repo (pre-build step).
#
# Real-machine fix: when target\release\variable.exe is held by a running
# instance, cargo fails to replace the exe at link time with
# "Access denied (os error 5)" and the whole tauri build aborts.
# Only instances whose executable path is inside this repo directory are
# stopped (target\release and dist-portable); Variable installed elsewhere
# (e.g. Program Files) is never touched.
#
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File tools\stop-variable.ps1
# Exit codes: 0 = nothing to stop (or all stopped, exe not locked);
#             1 = exe still locked (caller should abort the build).

param([string]$RepoRoot = (Split-Path -Parent $PSScriptRoot))

$ErrorActionPreference = "Stop"

$root = (Resolve-Path $RepoRoot).Path.TrimEnd('\')
$prefix = $root + '\'
$stopped = 0

Get-Process -Name variable -ErrorAction SilentlyContinue | ForEach-Object {
  $path = $null
  try { $path = $_.Path } catch { }
  # StartsWith string compare: no -like wildcard semantics, safe for
  # paths containing [ ] and other wildcard characters.
  if ($path -and $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    Write-Host "[ok] closed PID $($_.Id)  $path"
    Stop-Process -Id $_.Id -Force
    $stopped = $script:stopped + 1
  }
}

# Lock re-check: the release exe must be openable exclusively, otherwise the
# link stage will fail for sure. Handles (incl. WebView2 children) can take a
# few seconds to release after a force stop, so poll for up to ~5s.
$exe = Join-Path $root 'src-tauri\target\release\variable.exe'
if (Test-Path $exe) {
  $free = $false
  foreach ($i in 1..5) {
    $f = $null
    try {
      $f = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
      $f.Close()
      $free = $true
      break
    } catch {
      Start-Sleep -Milliseconds 1000
    }
  }
  if (-not $free) {
    Write-Host "[ERROR] $exe is still locked by another process. Close Variable and retry."
    exit 1
  }
}

exit 0
