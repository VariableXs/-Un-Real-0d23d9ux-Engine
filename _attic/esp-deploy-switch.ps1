# U 盘 ESP 部署（2026-09-21，S0.2 配置桥版）：内核 + boot-select.json 双副本。
# 只动 U 盘（Disk1）的 ESP 与 SHARED 分区，不碰内置硬盘任何文件。
# 戒律：改前备份；内容实证闸门；回读哈希；SHARED 按卷标定位+回读确认；
#       SHARED 已有配置绝不覆盖（单一事实源），ESP 副本永远向真相源对齐。
$ErrorActionPreference = 'Stop'
$NEWKERN = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\build\isoroot\kernel\varix'
$SEEDCFG = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\boot-select.json'
$BACKUP_DIR = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\esp-backup'

# limine.conf 与 repo 根 limine.conf 同源对齐（S0.2 起 kernel_cmdline 不再预置
# boot_timeout=0：倒计时/默认项/交接改由 boot-select.json 驱动——内核经 Limine
# internal module 读取引导卷根的 boot-select.json，副本缺失=verbose 警告+内置
# 默认，引导永不失败；cmdline 显式值优先级仍最高，需要时在此加 kernel_cmdline）。
# （2026-09-22：$CONF 内嵌副本已删除——第 4 步直接拷贝 repo 根 limine.conf。）

try {
  # 0) 摘历史残留 L:/Y:（Y: = 重启后系统可能给 U 盘 ESP 挂的盘符，失败忽略）
  $ErrorActionPreference = 'Continue'
  $pre = Join-Path $env:TEMP 'varix-deploy-pre.txt'
  [IO.File]::WriteAllText($pre, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`nremove letter=Y`r`n", [Text.Encoding]::ASCII)
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
  if (Test-Path 'L:\boot-select.json') {
    Copy-Item 'L:\boot-select.json' (Join-Path $BACKUP_DIR 'boot-select.json.bak') -Force
  }
  Write-Output 'backup ok (local + ESP limine.conf.bak)'

  # 4) 写新 limine.conf——**单一事实源 = repo 根 limine.conf**（2026-09-22 起
  #    不再内嵌 $CONF 副本，消除双源漂移；UTF-8 无 BOM 写回）。
  $REPOCONF = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\limine.conf'
  if (-not (Test-Path $REPOCONF)) { throw "repo limine.conf 缺失: $REPOCONF" }
  [IO.File]::WriteAllText('L:\limine.conf', (Get-Content $REPOCONF -Raw -Encoding UTF8), (New-Object System.Text.UTF8Encoding($false)))
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

  # 5.5) wallpaper module (S4 optional asset): absent = fallback bands, never blocks boot.
  #      落点=ESP 根级（与内核 module 请求 ../wallpaper.rgb565 一致；09-22 实机
  #      实证 /boot/ 子目录读不到 → module absent → 色带回退）。
  $WALLSRC = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\wallpaper-src\wallpaper-rgb565.bin'
  if (Test-Path $WALLSRC) {
    if (Test-Path 'L:\wallpaper.rgb565') {
      Copy-Item 'L:\wallpaper.rgb565' (Join-Path $BACKUP_DIR 'wallpaper.rgb565.bak') -Force
    }
    Copy-Item $WALLSRC 'L:\wallpaper.rgb565' -Force
    $w1 = (Get-FileHash $WALLSRC -Algorithm SHA256).Hash
    $w2 = (Get-FileHash 'L:\wallpaper.rgb565' -Algorithm SHA256).Hash
    if ($w1 -ne $w2) { throw 'wallpaper hash mismatch' }
    Write-Output ('wallpaper deployed hash=' + $w1.Substring(0,16))
  } else {
    Write-Output 'wallpaper-src absent - skip (desktop falls back to color bands)'
  }
  # 6) boot-select.json 双落点（S0.2 配置桥）：
  #    按 SHARED 卷标在同盘定位分区（不信卷号不信盘符），assign 后回读标签确认。
  $sharedLetter = $null
  try {
    $part = Get-Partition -DiskNumber 1 | Where-Object {
      $v = $_ | Get-Volume -ErrorAction SilentlyContinue
      ($null -ne $v) -and ($v.FileSystemLabel -eq 'SHARED')
    } | Select-Object -First 1
    if ($part) {
      foreach ($l in @('S','T','U','V','W','X','Y')) {
        if (-not (Get-PSDrive -Name $l -ErrorAction SilentlyContinue)) {
          $dp2 = Join-Path $env:TEMP 'varix-shared-assign.txt'
          [IO.File]::WriteAllText($dp2, "select disk 1`r`nselect partition $($part.PartitionNumber)`r`nassign letter=$l`r`n", [Text.Encoding]::ASCII)
          diskpart /s $dp2 | Out-Null
          $vol = Get-Volume -DriveLetter $l -ErrorAction SilentlyContinue
          if ($null -ne $vol -and $vol.FileSystemLabel -eq 'SHARED') { $sharedLetter = $l; break }
          # 回读标签不符 = 落点不对：立即摘掉，换下一个字母
          $dpx = Join-Path $env:TEMP 'varix-shared-remove.txt'
          [IO.File]::WriteAllText($dpx, "select disk 1`r`nselect partition $($part.PartitionNumber)`r`nremove letter=$l`r`n", [Text.Encoding]::ASCII)
          diskpart /s $dpx | Out-Null
        }
      }
    }
  } catch { Write-Output ('shared-locate-warn: ' + $_.Exception.Message) }

  if ($sharedLetter) {
    $sharedCfg = "${sharedLetter}:\boot-select.json"
    if (Test-Path $sharedCfg) {
      # 真相源已存在：只刷 ESP 副本，绝不覆盖 SHARED
      Copy-Item $sharedCfg 'L:\boot-select.json' -Force
      Write-Output 'esp-copy: refreshed from SHARED (truth source kept)'
    } else {
      # 首次装配：SHARED 缺配置 → repo 种子补种（写入=可追溯的新增，非覆盖）
      Copy-Item $SEEDCFG $sharedCfg -Force
      Copy-Item $SEEDCFG 'L:\boot-select.json' -Force
      Write-Output 'shared-seed: seeded from repo; esp-copy seeded'
    }
    $hA = (Get-FileHash 'L:\boot-select.json' -Algorithm SHA256).Hash
    $hB = (Get-FileHash $sharedCfg -Algorithm SHA256).Hash
    Write-Output ('boot-select esp hash=' + $hA.Substring(0,16))
    Write-Output ('boot-select shr hash=' + $hB.Substring(0,16))
    if ($hA -ne $hB) { throw 'boot-select.json 双副本哈希不一致' }
    Write-Output 'boot-select dual-copy hash verified'
  } else {
    # SHARED 定位失败（非阻断）：ESP 副本仍用种子补上——内核引导期读它；
    # SHARED 种子留给 Variable 首次写配置时补齐（同步通道会再刷 ESP）。
    Copy-Item $SEEDCFG 'L:\boot-select.json' -Force
    Write-Output 'esp-copy: seeded from repo (SHARED not found — Variable sync will mirror later)'
  }

  # 7) 摘字母（ESP + 可能挂上的 SHARED）
  $rmFile = Join-Path $env:TEMP 'varix-deploy-remove.txt'
  [IO.File]::WriteAllText($rmFile, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  diskpart /s $rmFile | Out-String | Write-Output
  if ($sharedLetter) {
    $pn = (Get-Partition -DiskNumber 1 | Where-Object {
      $v = $_ | Get-Volume -ErrorAction SilentlyContinue
      ($null -ne $v) -and ($v.FileSystemLabel -eq 'SHARED')
    } | Select-Object -First 1).PartitionNumber
    if ($pn) {
      $rmFile2 = Join-Path $env:TEMP 'varix-shared-remove2.txt'
      [IO.File]::WriteAllText($rmFile2, "select disk 1`r`nselect partition $pn`r`nremove letter=$sharedLetter`r`n", [Text.Encoding]::ASCII)
      diskpart /s $rmFile2 | Out-String | Write-Output
    }
  }
  Write-Output 'ESP-DEPLOY-DONE'
} catch {
  Write-Output ('DEPLOY-FAIL: ' + $_.Exception.Message)
  try {
    $rmFile3 = Join-Path $env:TEMP 'varix-deploy-remove3.txt'
    [IO.File]::WriteAllText($rmFile3, "select disk 1`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
    diskpart /s $rmFile3 | Out-Null
  } catch { }
}
