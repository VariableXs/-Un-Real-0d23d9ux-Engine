# U 盘 ESP 单文件内核更新（x2APIC 修复版）：Disk 1 → ESP → 覆盖 \kernel\varix
# 戒律：内容实证闸门不过不动盘；按标签确认落点；用后摘除字母。
$ErrorActionPreference = 'Stop'
$NEWKERN = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\build\isoroot\kernel\varix'
try {
  $esp = Get-Disk -Number 1 | Get-Partition | Where-Object { $_.GptType -eq '{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}' }
  if (-not $esp) { throw "Disk 1 上找不到 ESP 分区" }
  Write-Output ("ESP partition: number=" + $esp.PartitionNumber + " size=" + $esp.Size)

  $letter = 'L'
  $dp = "select disk 1`r`nselect partition " + $esp.PartitionNumber + "`r`nassign letter=L`r`n"
  $dpFile = Join-Path $env:TEMP 'varix-kernupd-assign.txt'
  [IO.File]::WriteAllText($dpFile, $dp, [Text.Encoding]::ASCII)
  diskpart /s $dpFile | Out-String | Write-Output

  # 闸门：落点必须有 Limine 引导件 + 旧内核（内容实证，防字母残留错卷）
  if (-not (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI')) { throw "L: 没有 BOOTX64.EFI——落点不对，不动盘" }
  if (-not (Test-Path 'L:\kernel\varix')) { throw "L: 没有 kernel\varix——落点不对，不动盘" }
  $lbl = (Get-Volume -DriveLetter L).FileSystemLabel
  Write-Output ("gate ok: label=" + $lbl)

  $old = (Get-Item 'L:\kernel\varix').Length
  $oldHash = (Get-FileHash 'L:\kernel\varix' -Algorithm SHA256).Hash
  Write-Output ("old: size=" + $old + " sha256=" + $oldHash.Substring(0,16) + "...")

  Copy-Item $NEWKERN 'L:\kernel\varix' -Force
  $new = (Get-Item 'L:\kernel\varix').Length
  $newHash = (Get-FileHash 'L:\kernel\varix' -Algorithm SHA256).Hash
  Write-Output ("new: size=" + $new + " sha256=" + $newHash.Substring(0,16) + "...")
  if ($newHash -ne (Get-FileHash $NEWKERN -Algorithm SHA256).Hash) { throw "回读哈希不一致——拷贝损坏" }

  # 戒律：diskpart /s 参数位置的 [IO.File]::WriteAllText()（返回 void）不能内联——
  # 内联时 PS 把 void 表达式当参数传给 diskpart 直接报「无法处理这些参数」，
  # remove 静默失败 → L: 字母残留。必须先赋变量再传。
  $rmFile = $dpFile + '.rm'
  $rmScript = "select disk 1`r`nselect partition " + $esp.PartitionNumber + "`r`nremove letter=L`r`n"
  [IO.File]::WriteAllText($rmFile, $rmScript, [Text.Encoding]::ASCII)
  diskpart /s $rmFile | Out-String | Write-Output
  Write-Output "KERN-UPDATE-DONE"
} catch {
  Write-Output ("UPDATE-FAIL: " + $_.Exception.Message)
  try { diskpart /s ([IO.File]::WriteAllText(($dpFile + '.rm'), "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)) | Out-Null } catch { }
}
