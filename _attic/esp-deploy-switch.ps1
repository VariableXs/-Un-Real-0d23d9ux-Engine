# U 盘 ESP 部署（2026-09-20）：三入口引导菜单 + SET2 键盘修复内核。
# 只动 U 盘 Disk1-ESP，不碰内置硬盘任何引导文件。
# 戒律：改前备份；内容实证闸门；回读哈希；先赋变量再 diskpart；用后摘字母。
$ErrorActionPreference = 'Stop'
$NEWKERN = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\build\isoroot\kernel\varix'
$BACKUP_DIR = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\esp-backup'

$CONF = @'
# Varix kernel Limine config (v8+ limine.conf format)
timeout: 0
serial: yes

/VARIX Kernel
    protocol: limine
    kernel_path: boot():/kernel/varix
    kernel_cmdline: boot_timeout=0

/Windows 11 (built-in disk)
    protocol: chainload
    path: guid(425214ee-257c-4aec-b69c-40178a4e847b):/EFI/Microsoft/Boot/bootmgfw.efi
'@

try {
  # 0) 摘历史残留 L:（失败忽略）
  $ErrorActionPreference = 'Continue'
  $pre = Join-Path $env:TEMP 'varix-deploy-pre.txt'
  [IO.File]::WriteAllText($pre, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $pre | Out-Null
  $ErrorActionPreference = 'Stop'

  # 1) 挂载 ESP
  $dp = Join-Path $env:TEMP 'varix-deploy-assign.txt'
  [IO.File]::WriteAllText($dp, "select disk 1`r`nselect partition 1`r`nassign letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-String | Write-Output

  # 2) 内容实证闸门
  if (-not (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI')) { throw 'L: 无 BOOTX64.EFI——落点不对，不动盘' }
  if (-not (Test-Path 'L:\kernel\varix')) { throw 'L: 无 kernel\varix——落点不对，不动盘' }
  if (-not (Test-Path 'L:\limine.conf')) { throw 'L: 无 limine.conf——落点不对，不动盘' }
  Write-Output 'gate ok: VARIX ESP confirmed'

  # 3) 备份（本地 + ESP 各一份）
  New-Item -ItemType Directory -Force -Path $BACKUP_DIR | Out-Null
  Copy-Item 'L:\limine.conf' (Join-Path $BACKUP_DIR 'limine.conf.bak') -Force
  Copy-Item 'L:\limine.conf' 'L:\limine.conf.bak' -Force
  Copy-Item 'L:\kernel\varix' (Join-Path $BACKUP_DIR 'varix.kern.bak') -Force
  Write-Output 'backup ok (local + ESP limine.conf.bak)'

  # 4) 写新 limine.conf（UTF-8 无 BOM）
  [IO.File]::WriteAllText('L:\limine.conf', $CONF, (New-Object System.Text.UTF8Encoding($false)))
  Write-Output '===== new limine.conf ====='
  Get-Content 'L:\limine.conf' -Raw | Write-Output

  # 5) 写新内核 + 回读哈希
  Copy-Item $NEWKERN 'L:\kernel\varix' -Force
  $h1 = (Get-FileHash $NEWKERN -Algorithm SHA256).Hash
  $h2 = (Get-FileHash 'L:\kernel\varix' -Algorithm SHA256).Hash
  Write-Output ('kern src hash=' + $h1.Substring(0,16))
  Write-Output ('kern esp hash=' + $h2.Substring(0,16))
  if ($h1 -ne $h2) { throw '回读哈希不一致——拷贝损坏' }
  Write-Output 'kern hash verified'

  # 6) 摘字母
  $rmFile = Join-Path $env:TEMP 'varix-deploy-remove.txt'
  [IO.File]::WriteAllText($rmFile, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rmFile | Out-String | Write-Output
  Write-Output 'ESP-DEPLOY-DONE'
} catch {
  Write-Output ('DEPLOY-FAIL: ' + $_.Exception.Message)
  try {
    $rmFile2 = Join-Path $env:TEMP 'varix-deploy-remove2.txt'
    [IO.File]::WriteAllText($rmFile2, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
    diskpart /s $rmFile2 | Out-Null
  } catch { }
}
