# Variable OS 用户手册（小白版） — 主计划扩充 27

> 归属：`portable/AI5/` · 面向完全没用过 PowerShell 的人。每一步都给可复制的命令。
> 前置：一台 Windows 11 电脑（管理员权限）、一个 Win11 ISO、（可选）1TB NVMe 固态U盘。

---

## 第 0 步：打开管理员 PowerShell

按 `Win`，输入 `powershell`，右键「以管理员身份运行」。
如果提示脚本被禁止执行，先跑一次（只对当前用户放开，不改系统策略）：

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

---

## 第 1 步：买对U盘（主计划 9.1 / 27.1）

- 买 **1TB NVMe 固态U盘**，别买 30 元的普通U盘。
- 认准：持续读写 ≥400MB/s、4K 随机 ≥20MB/s、TBW ≥600TB、双接口（USB-A + USB-C）。
- 参考型号：闪迪 CZ880 / 爱国者 A82 / 三星 T7 Shield。

---

## 第 2 步：在自己电脑上造盘（主计划 12.1 / 27.2）

```powershell
cd <仓库目录>\portable\AI5
.\Deploy-To-USB.ps1 -Action Stage1 -IsoPath C:\Win11_22H2.iso -Src D:\Variable-USB -SizeGB 150
```

- 固态盘默认动态 VHDX；机械盘加 `-Fixed`。
- 想自己逐步看，也可以直接跑 `..\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -SizeGB 150`。
- 耗时约 10-20 分钟。完成后 `D:\Variable-USB\Variable-OS.vhdx` 就是你的系统盘。

---

## 第 3 步：先在自己电脑上试跑（主计划 12.2 / 27.3）

```powershell
.\Deploy-To-USB.ps1 -Action Stage2
# 等价于 ..\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx
```

弹出的窗口就是你的便携系统。里面删文件、装软件、故意崩溃，**都只影响差分盘**，关机选「丢弃」就还原。

试完顺手把测试跑一遍：

```powershell
.\Chaos-Inject.ps1 -Action List          # 看 12 个混沌场景
.\Bench-Perf.ps1 -Action Run -TestDrive D:\
.\Compat-Matrix.ps1 -Action List         # 看 200 条兼容矩阵
```

---

## 第 4 步：换成你的桌面（主计划 12.3 / 27.4）

先在自己电脑上编译 Variable Engine（仓库根目录）：

```powershell
npm ci
npm run tauri build
```

再把产物写进 VHDX 并设为 Shell：

```powershell
.\Deploy-To-USB.ps1 -Action Stage3 -EngineDir ..\..\src-tauri\target\release -Yes
```

- 脚本会同时写一个 `ShellBackup=explorer.exe`，想退回原生桌面就用它。
- 不加 `-Yes` 时只打印将要做的事（dry-run），可以先看一遍。

---

## 第 5 步：装大软件到 Data（主计划 13.4 / 27.5）

大软件**不要装进 C 盘**，实体放 `Data\Apps`，C 盘只留链接（这样 VHDX 不会膨胀）：

```powershell
# 用 AI-4 的脚本（推荐，自动建链接）
..\AI4\Merge-Apps.ps1 -Action Add-PortableApp -AppName Blender -AppSource D:\dl\Blender

# 或手工
New-Item -ItemType Directory -Force -Path D:\Data\Apps\Blender-5.2
# 把 Blender 解压到上面目录，然后
New-Item -ItemType SymbolicLink -Path "C:\Program Files\Blender Foundation" -Target "D:\Data\Apps\Blender-5.2"
```

装完回填矩阵并实测启动时间：

```powershell
.\Compat-Matrix.ps1 -Action Fill-ExeHint
.\Compat-Matrix.ps1 -Action Run -Filter Blender
```

---

## 第 6 步：U盘到手，10 分钟上盘（主计划 12.4 / 27.6）

```powershell
# 1) 先用 Ventoy2Disk.exe 把U盘刷成 Ventoy 盘（一次性）
# 2) 预检（只读，会告诉你目标盘合不合法）
.\Deploy-To-USB.ps1 -Action Preflight -Src D:\Variable-USB -Dst E:\
# 3) 上盘
.\Deploy-To-USB.ps1 -Action Stage4 -Src D:\Variable-USB -Dst E:\
# 4) 核验成品盘
.\Deploy-To-USB.ps1 -Action Verify -Dst E:\
```

- 脚本**不会**把宿主 C 盘当目标，也**不会**镜像删除U盘上已有文件（不用 `/MIR`）。
- 上盘超过 10 分钟通常是线材/接口问题，换 USB3.2 Gen2 口再试。

---

## 第 7 步：日常怎么用（主计划扩充 22 / 27.7）

| 场景 | 怎么做 |
|---|---|
| 开机 | 插盘 → A 模式双击 `PortableVM\启动.exe`；B 模式重启按 F12 选U盘 |
| 装软件 | 安装包丢进 `Data\Exchange`，在便携系统里打开，安装路径选 `D:\Data\Apps\` |
| 关机 | 系统内正常关机，或关窗口时选「保存 / 丢弃」 |
| 备份 | `.\Maintenance.ps1 -Action Backup`（每日，保留 3 份） |
| 优化 | `.\Maintenance.ps1 -Action Optimize`（每月一次 `Optimize-VHD`） |
| 出事回滚 | `.\Maintenance.ps1 -Action Restore`（一键还原 User.vhdx） |
| 自动排程 | `.\Maintenance.ps1 -Action Schedule`（注册计划任务） |
| 换电脑 | 直接插另一台电脑启动，PnP 目标 <60s |

---

## 出问题了看这里

1. **进不去桌面** → 用 `ShellBackup` 值改回 `explorer.exe`，或安全模式 `Dism /Remove-Driver` 回退驱动（混沌场景 S07 有完整步骤）。
2. **软件打不开** → 跑 `.\Compat-Matrix.ps1 -Action Run -Filter <软件名>` 看实测结果；反作弊类（英雄联盟/无畏契约/CS2/PUBG）请切 B 模式。
3. **U盘变慢** → `.\Bench-Perf.ps1 -Action Run -TestDrive E:\` 看顺序/4K 是否掉档；分区未对齐会让 4K 掉约 50%（场景 S09 会查）。
4. **U盘丢了** → BitLocker 已加密 + 云同步，新盘 `rclone copy` + VHDX 合并，约 10 分钟恢复（AI-4 `Cloud-Sync.ps1`）。
5. **验收没过** → `.\Accept-Gate.ps1 -Action Report` 看哪一项 fail，报告里写了证据文件路径。

更多问答见同目录 `FAQ.md`。
