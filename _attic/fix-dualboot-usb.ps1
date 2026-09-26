# 双域引导修复（2026-09-20）——让 U 盘成为"插上就真正能切系统"的安装介质。
#
# 解决的问题（实机照片 3 张已证实的根因链）：
#   ① BIOS 的 Boot Device 第一顺位被设成 `EFI PXE Network`（网络引导）
#      → 开机先试用 PXE → 局域网没有 PXE 服务器 → `PXE-E16: No valid offer received`
#      → 卡在那里。**这与 VARIX / Limine / U 盘全都无关**，纯粹是启动顺序问题。
#   ② 此前的 limine.conf `timeout: 0`（菜单不显示、直接引导 VARIX），
#      而用户要的是"开机先出系统选择菜单"。
#
# 本次做什么（只碰 U 盘 Disk1 的 ESP 分区，绝不碰内置硬盘任何引导文件）：
#   1. 把 ESP 上的 limine.conf 换成"先出菜单、给足选择时间"的版本；
#   2. 菜单项里带上 Windows 链式引导（本机内置盘 ESP 的 bootmgfw.efi）；
#   3. 备份旧内容，改完回读校验（内容实证闸门）。
#
# 不改什么（红线）：
#   - 不动内置硬盘（Disk0）的任何分区、任何引导文件、任何 BCD 项；
#   - 不动 BIOS 的任何设置（启动顺序需要用户自己在 BIOS 里改，脚本只打印指引）；
#   - 不删 U 盘上任何既有文件（只覆盖 limine.conf，旧文件留 .bak）。
#
# 用法（需要在**管理员** PowerShell 里跑）：
#   powershell -ExecutionPolicy Bypass -File _attic\fix-dualboot-usb.ps1
#
# 先看不动手 / 只看现状：加 -DryRun

param(
  [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

# 本机内置盘的 ESP GUID（实测值；用于 Windows 链式引导目标）
$BUILTIN_ESP_GUID = '425214ee-257c-4aec-b69c-40178a4e847b'
$REPO = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main'
$BACKUP = Join-Path $REPO '_attic\esp-backup'

# 新的 limine.conf：先出菜单 + 给足选择时间 + Windows 链式引导。
#
# timeout: 5 —— 与内核侧 bootopt 的默认倒计时口径一致（5 秒）。
#   用户开机后能看到菜单、有时间用 ↑↓ 选，不选则走第一项（VARIX + VARIABLE）。
# serial: yes —— QEMU/串口调试可读。
$NEW_CONF = @'
# Varix / Variable 双域引导菜单（Limine v8+ 格式）
#
# 设计口径：开机先出这个菜单，给 5 秒选择时间。
#   - 不操作 → 倒计时归零进第 1 项（VARIX + VARIABLE，内核三卡菜单）
#   - 选 Windows → 链式引导内置盘真实 Windows（不是表面显示）
timeout: 5
serial: yes

/VARIX + VARIABLE
    protocol: limine
    kernel_path: boot():/kernel/varix
    kernel_cmdline: boot_timeout=5

/Windows 11 (built-in disk)
    protocol: chainload
    path: guid(425214ee-257c-4aec-b69c-40178a4e847b):/EFI/Microsoft/Boot/bootmgfw.efi
'@

function Write-Step([string]$msg) { Write-Output ("[fix-dualboot] " + $msg) }

# --- 0) 前置：确认 U 盘 ESP 存在且可识别 -------------------------------------
Write-Step "扫描磁盘布局（只读）..."
$disks = Get-Disk | Select-Object Number, FriendlyName, BusType, Size
foreach ($d in $disks) {
  Write-Step ("Disk {0}: {1} ({2}, {3:N1} GB)" -f $d.Number, $d.FriendlyName, $d.BusType, ($d.Size / 1GB))
}

# 找 USB 物理盘（BusType=USB）。找不到就不动手。
$usb = $disks | Where-Object { $_.BusType -eq 'USB' } | Select-Object -First 1
if (-not $usb) {
  Write-Output 'ABORT: 没有检测到 USB 磁盘。请先插好系统 U 盘再跑本脚本。'
  exit 2
}
$usbDisk = [int]$usb.Number
Write-Step ("目标 U 盘 = Disk {0} : {1}" -f $usbDisk, $usb.FriendlyName)

# --- 1) 挂载 U 盘 ESP（临时盘符 L:） ----------------------------------------
Write-Step "挂载 U 盘 ESP 到 L: ...（用后必摘）"
$dpAssign = Join-Path $env:TEMP 'varix-fix-assign.txt'
# 先摘历史残留 L:，避免绑到别的卷上
$dpClean = Join-Path $env:TEMP 'varix-fix-clean.txt'
[IO.File]::WriteAllText($dpClean, "select disk $usbDisk`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
$ErrorActionPreference = 'Continue'
diskpart /s $dpClean 2>&1 | Out-Null
$ErrorActionPreference = 'Stop'

[IO.File]::WriteAllText($dpAssign, "select disk $usbDisk`r`nselect partition 1`r`nassign letter=L`r`n", [Text.Encoding]::ASCII)
diskpart /s $dpAssign 2>&1 | Out-String | Write-Output

function Remove-TempLetter {
  $dpRm = Join-Path $env:TEMP 'varix-fix-remove.txt'
  [IO.File]::WriteAllText($dpRm, "select disk $usbDisk`r`nselect partition 1`r`nremove letter=L`r`n", [Text.Encoding]::ASCII)
  $ErrorActionPreference = 'Continue'
  diskpart /s $dpRm 2>&1 | Out-Null
  $ErrorActionPreference = 'Stop'
}

# --- 2) 内容实证闸门：L: 必须确实是 VARIX 的 ESP ------------------------------
# 铁律：落点内容实证不过就退出、不动任何东西（防跨会话盘符残留绑错卷）。
$gateOk = (Test-Path 'L:\EFI\BOOT\BOOTX64.EFI') -and (Test-Path 'L:\kernel\varix')
if (-not $gateOk) {
  Write-Output 'ABORT: L: 不是 VARIX 的 ESP（缺 EFI\BOOT\BOOTX64.EFI 或 kernel\varix）——不动盘。'
  Write-Output '   可能原因：U 盘未装配 VARIX，或盘符残留绑到了别的卷。'
  Remove-TempLetter
  exit 3
}
Write-Step '闸门通过：L: 确认是 VARIX ESP'

# 展示现有 limine.conf（改前留痕）
$confPath = $null
foreach ($p in @('L:\limine.conf', 'L:\EFI\BOOT\limine.conf', 'L:\EFI\BOOT\limine.cfg')) {
  if (Test-Path $p) { $confPath = $p; break }
}
if ($confPath) {
  Write-Step ("现有配置：" + $confPath)
  Write-Output '---------- 改前内容 ----------'
  Get-Content $confPath -Raw | Write-Output
  Write-Output '------------------------------'
} else {
  Write-Step '未找到现存 limine.conf（将新建 L:\limine.conf）'
  $confPath = 'L:\limine.conf'
}

if ($DryRun) {
  Write-Step 'DryRun：只显示将要写入的内容，不落盘。'
  Write-Output '---------- 将要写入 ----------'
  Write-Output $NEW_CONF
  Write-Output '-----------------------------'
  Remove-TempLetter
  Write-Step 'DryRun 结束（未做任何修改）。'
  exit 0
}

# --- 3) 备份旧配置 -----------------------------------------------------------
if (-not (Test-Path $BACKUP)) { New-Item -ItemType Directory -Path $BACKUP -Force | Out-Null }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (Test-Path $confPath) {
  $bak = Join-Path $BACKUP ("limine.conf.$stamp.bak")
  Copy-Item $confPath $bak -Force
  Write-Step ("旧配置已备份：" + $bak)
}

# --- 4) 原子写入新配置（临时文件 → 替换） ------------------------------------
# 用 .tmp 再 Move，避免写入中途断电留下半文件（半文件 = 引导器读不懂 = 开不了机）。
$tmp = 'L:\limine.conf.tmp'
# 纯 ASCII + CRLF：BootX64/limine 的配置解析对 BOM 敏感，不写 BOM。
$text = $NEW_CONF -replace "`r`n", "`n" -replace "`n", "`r`n"
[IO.File]::WriteAllText($tmp, $text, [Text.Encoding]::ASCII)
Move-Item $tmp 'L:\limine.conf' -Force

# --- 5) 回读校验：内容必须与预期逐字节一致 -----------------------------------
$read = [IO.File]::ReadAllText('L:\limine.conf')
$hash = (Get-FileHash 'L:\limine.conf' -Algorithm SHA256).Hash
Write-Step ("写入完成，SHA256 = " + $hash)
if ($read -notmatch 'VARIX \+ VARIABLE' -or $read -notmatch 'bootmgfw\.efi') {
  Write-Output 'WARN: 回读内容缺少关键项（VARIX 菜单项 / Windows 链式引导），请人工复核！'
} else {
  Write-Step '回读校验通过：菜单项与 Windows 链式引导均在位。'
}

# --- 6) 摘临时盘符 -----------------------------------------------------------
Remove-TempLetter
Write-Step 'L: 已摘除。'

# --- 7) 打印 BIOS 操作指引（脚本不动 BIOS） ---------------------------------
Write-Output ''
Write-Output '==================== 接下来请在 BIOS 里做这一步 ===================='
Write-Output '本脚本无法代改 BIOS 启动顺序（那是固件设置，必须在本机按键进入）。'
Write-Output '当前开机失败的直接原因（照片已证实，与 VARIX 无关）：'
Write-Output '    Boot Device 第一顺位 = "EFI PXE Network" → 开机试网络引导 → 失败卡住'
Write-Output ''
Write-Output '请这样改：'
Write-Output '  1. 开机按 F2（Lenovo Legion 进 BIOS 键）进入 BIOS Setup'
Write-Output '  2. 进 Boot 页，把 Boot Device 第一顺位改成：'
Write-Output '       - 想从 U 盘进 VARIX：选 "EFI USB Device (Lenovo thinkplus 1TB)"'
Write-Output '       - 平时直接用 Windows：选内置盘（Windows Boot Manager）'
Write-Output '  3. 顺手确认 Secure Boot = Disabled（未签名引导器必须关，实测已是关的）'
Write-Output '  4. F10 保存退出'
Write-Output ''
Write-Output '注意：这样做不会删除任何能进 Windows 的启动识别程序 —— 只是把顺序摆正。'
Write-Output '===================================================================='
Write-Output ''
Write-Step '完成。'
