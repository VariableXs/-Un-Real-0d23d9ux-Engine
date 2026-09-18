Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Get-FileMap {
  param([string]$Base, [string[]]$Rel)
  Write-Host ("GF entry Base=" + $Base + " RelCount=" + @($Rel).Count) -ForegroundColor Yellow
  $map = @{}
  foreach ($rel in $Rel) {
    Write-Host ("  rel=[" + $rel + "] type=" + $rel.GetType().Name) -ForegroundColor Yellow
    $p = Join-Path $Base $rel
    $map[$rel] = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
  }
  return $map
}
$root = Join-Path $env:TEMP 'mv-test'
$rootPath = $root.TrimEnd('\') + '\'
$targets = @('limine.conf', 'kernel\varix', 'ESP-MANIFEST.json', 'apps.json')
$existing = @($targets | Where-Object { Test-Path -LiteralPath (Join-Path $rootPath $_) })
Write-Host ("existing.Count=" + $existing.Count) -ForegroundColor Yellow
$map = Get-FileMap $rootPath $existing
Write-Host ("OK map=" + $map.Count)
