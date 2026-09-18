Set-StrictMode -Version Latest
$root = Join-Path $env:TEMP 'mv-test'
$rootPath = $root.TrimEnd('\') + '\'
$targets = @('limine.conf', 'kernel\varix', 'ESP-MANIFEST.json', 'apps.json')
$existing = @($targets | Where-Object { Test-Path -LiteralPath (Join-Path $rootPath $_) })
Write-Host ("count=" + $existing.Count)
foreach ($e in $existing) { Write-Host ("elem type=" + $e.GetType().Name + " val=[" + $e + "]") }
$existing2 = @($targets | Where-Object { Test-Path -LiteralPath (Join-Path $rootPath $_) }) | ForEach-Object { $_ }
Write-Host "via-FE count=$($existing2.Count) elem0=$($existing2[0].GetType().Name)"
