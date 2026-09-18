$root = Join-Path $env:TEMP 'mv-test'
$rootPath = $root.TrimEnd('\') + '\'
$targets = @('limine.conf','kernel\varix')
Write-Output "step1"
$existing = @($targets | Where-Object { Test-Path -LiteralPath (Join-Path $rootPath $_) })
Write-Output "step2 count=$($existing.Count)"
$map = @{}
foreach ($rel in $existing) {
  $p = Join-Path $rootPath $rel
  $map[$rel] = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash
}
Write-Output "step3 map=$($map.Count)"
$mf = [pscustomobject]@{ version='1.0'; label='x'; taken=(Get-Date -Format 'o'); files=@($existing | ForEach-Object { [pscustomobject]@{ file=$_; sha256=$map[$_] } }) }
Write-Output "step4 files=$(@($mf.files).Count)"
