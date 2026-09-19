$ErrorActionPreference='Continue'
select-string -Path 'x' -ErrorAction SilentlyContinue | Out-Null
$dp = Join-Path $env:TEMP 'rm-L.txt'
[IO.File]::WriteAllText($dp, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
diskpart /s $dp | Out-String | Write-Output
Write-Output ('L-Test-Path: ' + (Test-Path 'L:\'))
Write-Output 'RM-L-DONE'
