# AI-1 存储核 — 主计划第 3 章「存储架构」+ 第 9 章「性能与寿命」实现说明

> 版本 v1.0.0 | 日期 2026-09-07 | 执行 AI-1（存储核）| 分支 `arena/01a07a30-un-real-0d23d9ux-engine`
>
> 范围：`PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 3.1-3.3 + 9.1-9.5，含扩充 13.4 / 20.1 / 26 / 29.4
>
> 一句话结论：**1TB 1000MB/s 双接口盘上采用「VHDX 固定 150GB（NTFS 64KB 簇）+ Data 780GB（exFAT）」，系统盘只留符号链接、实体全在 Data，配合 CompactOS + TRIM + 关 SysMain，实测目标 SEQ≥900MB/s、4K≥20MB/s，VHDX 永不膨胀，寿命 80 年。**

---

## 0. 交付物与执行顺序

| 交付物 | 对应主计划 | 作用 |
|---|---|---|
| `portable/AI1/Create-VHDX.ps1` | 3.1 / 3.2 / 3.3 / 9.2 / 20.1 | 造盘定版：固定 150GB + GPT + 64KB 簇 + 4K 对齐校验 + Data 七目录 + 离线 CompactOS/注册表调优 + 可选三级差分链 |
| `portable/AI1/Tune-Guest.ps1` | 9.3 / 9.4 / 9.5 / 26.1-26.3 | 进系统内一次性调优：CompactOS、TRIM、关 SysMain/WSearch/DiagTrack、关休眠、WinSxS 精简、RAM 缓存契约 |
| `portable/AI1/Link-DataApps.ps1` | 3.2 / 13.4 | 读写分离：按映射表建/校验/搬迁/卸载符号链接，exFAT 自动回退 SymbolicLink |
| `portable/AI1/Bench-Storage.ps1` | 9.1 / 11.3 | 零依赖压测：SEQ 写（WriteThrough）/ SEQ 读 / 4K 随机 / 簇 / 对齐 / TRIM / VHDX 膨胀率与碎片 |
| `portable/AI1/Maintain-VHDX.ps1` | 3.3 / 9.3 / 26.4 | 每月维护：`Optimize-VHD -Mode Full` + 宿主卷 ReTrim + 碎片率 <5% 体检 + 可选 `Merge-VHD` |
| `portable/AI1/Bench.md` | 11.3 / 17 | 压测口径、判定线、寿命计算与实测区 |

执行顺序（造盘 → 调优 → 分离 → 压测 → 维护）：

```powershell
.\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -TuneHost   # 阶段1
# 用 ..\AI2\Test-VM.ps1 启动便携系统后，在系统内：
.\Tune-Guest.ps1 -Report                                                          # 阶段1.5
.\Link-DataApps.ps1 -DataRoot D:\Variable-USB\Data                                # 阶段1.6
.\Bench-Storage.ps1 -Path D:\ -Vhdx D:\Variable-USB\Variable-OS.vhdx              # 阶段1.7
.\Maintain-VHDX.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx                        # 每月
```

---

## 1. 主计划 3.1 VHDX 链设计

### 1.1 链结构与落地方式

```
Base.vhdx   (20GB, 动态, 只读)  Win11 22H2 + Sysprep + 万能驱动，文件属性置 ReadOnly，哈希校验防篡改
  ↓ parent（-Chain 生成）
Variable-OS-Apps.vhdx (差分)    大软件层：Blender / PS / VS，可整层分发给第二块盘
  ↓ parent
User.vhdx   (差分, 读写)        用户数据与设置，日常备份对象；关机可「合并」或「丢弃」
```

- 1TB 定版**默认不建链**：单盘 `Variable-OS.vhdx`（固定 150GB）就是系统盘，符合 11.1 验收「`Variable-OS.vhdx` 150GB 固定」。
- `-Chain` 为**层式分发**（主计划 8.1，AI-4 使用）预留：母盘只读 + 两级差分，还原 = 丢弃 User 层（秒级），扁平化 = `Merge-VHD -Path User.vhdx -DestinationPath Variable-OS-Apps.vhdx`。
- 差分盘的**虚拟大小继承母盘**，磁盘文件按实际写入增长；因此 `-AppsGB` 只是规划口径，不是分配量。
- 验收：`Get-VHD` 的 `FragmentationPercentage < 5%`，脚本收尾自动打印并对 >5% 给出黄色告警。

### 1.2 VHDX 选型对比表

| 方案 | 虚拟上限 | 磁盘实占 | 顺序性能 | 4K 随机 | 膨胀风险 | 还原能力 | 结论 |
|---|---|---|---|---|---|---|---|
| VHD 动态（旧格式） | 2TB | 小→增长 | 中 | 低 | 高（2MB 块 + 无 TRIM） | 差分 | ❌ 淘汰：无 TRIM、块粒度粗、2TB 上限 |
| VHDX 动态 | 64TB | 小→增长 | 高 | 中高 | 中（需定期 Optimize） | 差分/合并 | ✅ 机械盘 / 小容量盘首选 |
| **VHDX 固定** | 64TB | =虚拟大小 | **最高** | **最高** | **无** | 整文件 | ✅ **1TB 1000MB/s 定版（本方案）** |
| VHDX 固定 + NTFS 稀疏 | 64TB | 按已写块 | ≈固定 | ≈固定 | 低 | 整文件 | ⭕ 需满足「回收后 <20GB 实占」时启用（`-Sparse`） |
| VHDX 差分链 | 继承母盘 | 仅增量 | 略降（每层多一次父盘查找） | 略降 | 低 | 秒级丢弃 | ✅ 多用户 / 大软件层分发 |

选固定盘的三条理由：① 动态盘每次扩展都要改元数据（BAT 表）并可能触发文件碎片，1000MB/s 盘上这部分开销占比最高；② 固定盘无膨胀，「装 Blender 后 VHDX 大小不变」天然成立；③ 1TB 容量下 150GB 的预分配代价可接受（见第 9 节容量核算）。

---

## 2. 主计划 3.2 读写分离

### 2.1 Data 目录（`Create-VHDX.ps1` 自动创建）

```
<盘根>\Data\
├── Apps\       绿色软件/安装版实体（Blender-5.2 …）
├── MSIX\       MSIX 包与挂载点（主计划 8.2 / 13.5）
├── Plugins\    Variable Engine 热加载插件 DLL
├── User\       Documents / Desktop 等随盘数据
├── Exchange\   宿主↔虚拟系统受控摆渡通道（AI-2 层4）
├── Cache\      RamCache 契约与缩略图/临时缓存（9.5）
└── Dumps\      崩溃转储，不写母盘
```

### 2.2 mklink 清单（`linkmap.json` 模板内容，可增删）

| 系统盘路径（链接） | Data 目标（实体） | 类型 | 原因（主计划出处） |
|---|---|---|---|
| `C:\Program Files\Blender Foundation` | `{Data}\Apps\Blender-5.2` | SymbolicLink | 2.1GB 安装实体不入 VHDX（13.4） |
| `C:\ProgramData\Blender Foundation` | `{Data}\Apps\Blender-5.2\config` | SymbolicLink | 安装版必写 ProgramData（13.4） |
| `C:\Users\Public\Documents\Variable` | `{Data}\User\Documents` | SymbolicLink | 用户文档随盘走（3.2） |
| `C:\ProgramData\Variable\Dumps` | `{Data}\Dumps` | SymbolicLink | 转储不落母盘（2.2） |
| `C:\ProgramData\Variable\Cache` | `{Data}\Cache` | SymbolicLink | 缓存/缩略图（9.5） |

### 2.3 两个必须避开的坑（实现已处理）

1. **Data 是 exFAT 时不能用 Junction**：Junction 与硬链接都要求目标卷为 NTFS，exFAT 无重解析点支持。`Link-DataApps.ps1` 用 `Get-Volume` 探测 Data 卷文件系统，`-LinkType Auto` 时 NTFS→Junction、exFAT→SymbolicLink，并对显式 `-LinkType Junction` 自动回退并告警。链接对象本身建在系统盘（NTFS）一侧，目标指向 exFAT 路径是允许的。
2. **exFAT 无 ADS 与 ACL**：从 Data 直接运行的 exe 拿不到 `Zone.Identifier`（MOTW 丢失，SmartScreen 行为不同），也没有每用户 ACL。需要权限/标记的安装版软件建议放 NTFS 的 Data 卷；MOTW 兜底由 AI-4 安全章覆盖。

验收口径：装 Blender 到 `Data\Apps` 后 `Get-VHD` 的 `FileSize` 不变（固定盘恒成立，动态盘需 `Optimize-VHD` 后比对）。

---

## 3. 主计划 3.3 动态 vs 固定

| 盘类型 | 推荐 | 说明 |
|---|---|---|
| 1TB 1000MB/s 固态（本项目） | **Fixed 150GB** | 顺序/4K 均最优，无膨胀；`-Sparse` 可把实占降到已写块 |
| 机械移动硬盘 | Fixed | 避免动态盘扩展引发的文件碎片 |
| ≤256GB 固态 U 盘 | Dynamic | 容量紧张，牺牲少量性能换空间 |

- **每月**：`Maintain-VHDX.ps1`（`Optimize-VHD -Mode Full` + 宿主卷 `Optimize-Volume -ReTrim` + 碎片体检），对应主计划 26.4 的「Defrag 月一次」。
- **固定盘 + NTFS 稀疏**：`New-VHD -Fixed` 会立即预分配 150GB；`-Sparse` 追加 `fsutil sparse setflag` + `setrange`，使其在 NTFS 上按已写块实占。这是满足 11.1「回收后 <20GB 实占」的唯一路径——**普通固定盘实占恒等于 150GB，该验收项对非稀疏固定盘不成立**，此点已在 `Bench-Storage.ps1` 中显式提示。

---

## 4. 主计划 9.1 选盘验证

| 项 | 底线（9.1 通用固态 U 盘） | 本项目验收线（1TB 1000MB/s） | 判定工具 |
|---|---|---|---|
| 顺序读写 | ≥400MB/s | **≥900MB/s** | CrystalDiskMark / `Bench-Storage.ps1` |
| 4K 随机 | ≥20MB/s | **≥20MB/s（≈5120 IOPS）** | 同上 |
| TBW 寿命 | ≥600TB | ≥600TB | 厂商标称 |
| SLC 缓存 | — | ≥100GB（避免大文件掉速） | 持续写 200GB 观察掉速点 |
| 主控/颗粒 | RTS5766 + TLC | 同左 | 厂商规格 |

`Bench-Storage.ps1` 的口径必须一起读：顺序写用 `FileStream + WriteThrough` 直写（绕过写缓存，接近真实设备）；顺序读为带缓存读，**偏乐观**，权威值以 CrystalDiskMark 为准；4K 随机为容器文件内随机 4KB 偏移直写，输出 MB/s 与 IOPS。4K 换算：20MB/s ÷ 4KB = 5120 IOPS。

---

## 5. 主计划 9.2 文件系统

- **系统盘（VHDX 内）**：NTFS，簇 **64KB**（`Format-Volume -AllocationUnitSize 65536`），减少碎片、提升大文件顺序读。
- **Data 分区**：exFAT（跨 Win/macOS/Linux，单文件 1TB）；需要 ACL/ADS 时用 NTFS。
- **4K 对齐**（扩充 29.4）：`Get-Partition | Select Offset` 必须是 4096 的倍数，否则 4K 随机掉约 50%。`Create-VHDX.ps1` 在格式化前实测 `Offset % 4096` 并告警。
- **关 LastAccess**：`fsutil behavior set DisableLastAccess 1` + 注册表 `NtfsDisableLastAccessUpdate=1`（离线写入 SYSTEM hive，扩充 26.1），省掉每次读的元数据回写。
- **64KB 簇的代价**（必须记账）：平均每文件浪费 32KB。6 万文件≈1.83GB、10 万文件≈3.05GB、20 万文件≈6.10GB。这部分由 CompactOS 压缩抵消一部分，150GB 预算已含此余量。

---

## 6. 主计划 9.3 VHDX 优化

| 项 | 命令 | 落点 |
|---|---|---|
| CompactOS 压缩 | `compact /compactos:always` | `Tune-Guest.ps1`（离线侧 `dism /Image:X:\ /Compact:On` 在 `Create-VHDX.ps1`） |
| 启用 TRIM | `fsutil behavior set DisableDeleteNotify 0` | 宿主（`-TuneHost`）+ 系统内各一次 |
| 空闲块回收 | `Optimize-Volume -DriveLetter C -ReTrim` | `Tune-Guest.ps1` |
| 未用块回收 | `Optimize-VHD -Mode Full` | `Maintain-VHDX.ps1`（每月） |
| 自动碎片整理 | 关闭 VHDX 所在卷的计划任务 | `-TuneHost` 提示，手动每月 `defrag <卷> /O` |

CompactOS 的 30% 空间收益同时是**寿命收益**：写入量同比下降，直接反映在第 9 节的寿命公式里。

---

## 7. 主计划 9.4 系统裁剪

`Tune-Guest.ps1` 逐项落地并**回读实测值**（不是「执行了就算过」），末尾打印验收表：

| 项 | 目标值 | 手段 |
|---|---|---|
| `SysMain`（Superfetch） | Disabled | 离线注册表 `Start=4` + 系统内 `Set-Service` |
| `WSearch` | Manual | 同上（关对 VHDX 的索引） |
| `DiagTrack` | Disabled | 同上 |
| `wuauserv` | Manual + 策略 `NoAutoUpdate=1` | 自动更新改手动 |
| 休眠 | `hiberfil.sys` 不存在 | `powercfg /hibernate off`（省 8GB） |
| 待机/硬盘超时 | 0（不休眠） | `powercfg /change standby-timeout-ac 0`、`disk-timeout-ac 0`（26.3） |
| WinSxS | 精简 | `Dism /Online /Cleanup-Image /StartComponentCleanup /ResetBase` |

---

## 8. 主计划 9.5 缓存策略

- ReadyBoost 关闭，改用二级缓存（PrimoCache / ImDisk），目标命中率 80%。
- `Tune-Guest.ps1` 生成 **RAM 盘缓存契约** `Data\Cache\ramcache.json`：`{ version, sizeMB: 256, policy: "LRU", mountPoint: "R:\VariableCache", dir, warm: [] }`。真正建 RAM 盘需要第三方驱动（ImDisk/PrimoCache），存储核只负责**目录、契约与验收口径**，预热清单由 AI-2 的 6 件套填充、由 Core 消费——这是刻意划清的边界，避免两个 AI 改同一份逻辑。
- 收益口径（主计划 13.1）：热路径把 `blender.exe + 核心 DLL + 启动配置` 约 300MB 镜像放 RAM 盘，二次启动 `MapViewOfFile` 直映射，热启动目标 5.2-6s。

---

## 9. 容量核算与寿命计算

### 9.1 1TB 盘的真实容量（修正分工表的 800GB）

| 项 | 数值 |
|---|---|
| 标称 1TB | 1,000,000,000,000 B = **931.32 GiB** |
| 减分区表 + 文件系统开销（约 0.15%） | **≈929.9 GiB**（与 AI-5 验收「930GB 可用」一致） |
| `Variable-OS.vhdx` 固定 | 150 GiB |
| **Data 实得** | **≈779.9 ≈ 780 GiB** |

分工总览写的「150GB VHDX + 800GB Data」合计 950GiB，**超出可用容量 20.1GiB**。定版按 150 + 780 落地；若坚持 800GB Data，则 VHDX 需降到 130GB（Win11 + 软件层仍可容纳，但 CompactOS 前的余量偏紧）。

### 9.2 寿命 80 年推导

公式：`寿命(年) = TBW ÷ (日写入 × 365 × 0.7 × WAF)`，其中 `0.7` 为 CompactOS 的 30% 写入削减（9.3），`WAF=1.5` 为 TLC + SLC 缓存回写的典型写放大，`TBW=600TB=614,400GB`（按 1TB=1024GB）。

| 日均宿主可见写入 | 年闪存写入 | 预期寿命 |
|---|---|---|
| 5 GB/天 | 1,916 GB | 320.6 年 |
| 10 GB/天 | 3,833 GB | 160.3 年 |
| **20 GB/天（设计工况）** | **7,665 GB** | **80.2 年** ✅ |
| 30 GB/天 | 11,498 GB | 53.4 年 |
| 40 GB/天 | 15,330 GB | 40.1 年 |
| 60 GB/天 | 22,995 GB | 26.7 年 |

结论：在 20GB/天 的设计工况下寿命 80.2 年，达成「寿命 80 年」目标。把写入压到 20GB/天 靠三件事：读写分离（大软件与缓存全在 Data，不反复重写 VHDX 块）、CompactOS（少写 30%）、关 SysMain/WSearch/遥测/休眠（消掉后台持续写）。若实测日均写入超过 40GB，寿命跌破 40 年，需要换更高 TBW 的盘或把 Cache/Dumps 移到 RAM 盘。

---

## 10. 验收对照表

| 主计划条目 | 验收标准 | 落点 | 状态 |
|---|---|---|---|
| 3.1 VHDX 链 | Base 只读 / Apps 可选 / User 差分可合并；碎片 <5% | `Create-VHDX.ps1 -Chain` + `Maintain-VHDX.ps1` | ✅ 脚本落地，链上机待验 |
| 3.2 读写分离 | `C:\Program Files\Blender` → Data；ProgramData 同步；Data 七目录 | `Link-DataApps.ps1` | ✅ 脚本落地 |
| 3.3 动态/固定 | 1TB 用 Fixed 150GB；每月 Optimize | `Create-VHDX.ps1` / `Maintain-VHDX.ps1` | ✅ |
| 9.1 选盘验证 | SEQ≥900、4K≥20 | `Bench-Storage.ps1` + CrystalDiskMark | ⬜ 待 1TB 盘实测 |
| 9.2 文件系统 | 64KB 簇 + `NtfsDisableLastAccessUpdate=1` + 4K 对齐 | `Create-VHDX.ps1`（含实测回读） | ✅ |
| 9.3 VHDX 优化 | CompactOS always + TRIM + 关自动 Defrag | `Tune-Guest.ps1` / `-TuneHost` | ✅ |
| 9.4 系统裁剪 | 关 SysMain/WSearch、精简 WinSxS、关休眠 | `Tune-Guest.ps1` | ✅ |
| 9.5 缓存 | RAM 盘 256MB 缓存启动文件 | `ramcache.json` 契约 + Data\Cache\RamCache | ✅ 契约落地（RAM 盘驱动待接） |
| 11.1 性能验收 | SEQ≥900 / 4K≥20；150GB 固定；装 Blender 后 VHDX 不变 | `Bench-Storage.ps1` | ⬜ 待实测 |

---

## 11. 扩充 20.1 逐行讲解与定版修正清单

主计划扩充 20.1 给出的 `Create-VHDX.ps1` 全量脚本（也是 `portable/Create-VHDX.ps1` 的原型）能跑通「造盘 → 挂载 → 展开 → 引导」主线，但按 1TB 1000MB/s 的定版口径逐行复核后，发现 8 处必须修正的点，AI-1 版本已全部落地：

| # | 原脚本行为 | 问题 | 定版修正 |
|---|---|---|---|
| 1 | `dism /Apply-Image /ImageFile:$($wim.FullName) /Index:6 /ApplyDir:$drive\` 全裸传参 | ISO 或输出路径含空格即失败（如 `Win11 22H2.iso`） | 参数逐个加引号：`"/ImageFile:$($wim.FullName)"` |
| 2 | `dism`、`bcdboot` 后不检查返回码 | 展开失败仍继续写引导，得到一块「看似成功」的坏盘 | 每步校验 `$LASTEXITCODE`，非 0 立即 `throw` |
| 3 | `New-Partition -UseMaximumSize` 无条件新建 | 复用已有 VHDX（脚本自己支持复用）时直接报错中断 | 先 `Get-Partition` 复用，无分区才新建；无盘符时用 `Get-FreeLetter` 补 |
| 4 | `Get-Disk | Where {$_.Location -like "*文件名*"}` | 同名 VHDX 存在多份时可能挂错盘 | 保留匹配但取第一块并在报错信息里带上完整路径；挂载后 `Start-Sleep 2` 等待枚举 |
| 5 | 不校验分区偏移 | 非 4K 对齐时 4K 随机掉约 50%，且事后极难定位（扩充 29.4） | 格式化前实测 `Offset % 4096`，不对齐即黄色告警 |
| 6 | 扩充 20.1 版 `Format-Volume` 未指定簇大小（`portable/Create-VHDX.ps1` 已加 `-AllocationUnitSize 64KB`） | 前者默认 4KB 簇，大文件顺序读吃亏；两者都不回读校验实际簇大小 | `-AllocationUnitKB 64` 参数化，格式化后 `Get-Volume` 回读实测簇大小并打印 |
| 7 | `Get-ChildItem "$iso\sources" -Filter "*.wim" \| Select -First 1`；`Index:6` 硬编码 | `sources\` 下同时有 `boot.wim` 与 `install.wim`，按名称排序 **`boot.wim` 在前**，会展开引导镜像而不是安装镜像；换镜像版本时 Index 也不对 | 改为 `-Filter 'install.wim'`（找不到再回退 `install.esd`），`-ImageIndex` 参数化，注释给出 `dism /Get-WimInfo` 查法 |
| 8 | 无 try/finally | 中途失败 VHDX 一直处于挂载态，下次运行必失败 | `try/finally` 中 `Dismount-VHD`，ISO 也在异常路径 `Dismount-DiskImage` |

另外新增的能力：离线注册表写入（`reg load/add/unload` 直接改镜像内的 SYSTEM/SOFTWARE hive，不需要开机就能关 SysMain/WSearch/DiagTrack、开 LongPaths、关 LastAccess）、`-Sparse` 稀疏固定盘、`-Chain` 三级差分链、收尾 `Get-VHD` 自检（虚拟/实占/碎片）。

> 说明：`portable/Create-VHDX.ps1` 保持原样不动（主计划附录 A 与 `portable/README.md` 都指向它，作为快速上手入口）；1TB 定版一律用 `portable/AI1/Create-VHDX.ps1`。

---

## 12. 存储侧故障与回退

| 故障 | 现象 | 处置 |
|---|---|---|
| VHDX 内系统损坏 | 便携系统起不来 | 差分链：丢弃 `User.vhdx` 重建（秒级）；单盘：从 `Data\Cache` 的母盘备份还原，或重跑 `Create-VHDX.ps1 -Force` |
| 造盘中途失败 | VHDX 处于挂载态 | 脚本 `finally` 已自动 `Dismount-VHD`；仍失败时 `Dismount-VHD -Path ... -Force` 后重跑 |
| 固定盘写满 | 系统盘报空间不足 | 大软件必须走 `Link-DataApps.ps1` 迁到 Data；`WinSxS` 跑 `Tune-Guest.ps1` 精简；CompactOS 已开启 |
| Data(exFAT) 目录损坏 | 链接指向的目标打不开 | `chkdsk D: /f`；链接本身用 `Link-DataApps.ps1 -Verify` 体检，坏链标 `bad` |
| 拔盘导致写入中断 | 差分链父盘不一致 | 只用 `User.vhdx` 的最近备份；`Merge-VHD` 前先复制一份父盘（合并不可逆） |
| 4K 未对齐 | 4K 随机只有预期一半 | 重建分区（`Create-VHDX.ps1 -Force`）， Ventoy/原生 GPT 默认已对齐 |

---

## 13. 与其他 AI 的接口边界（避免重复实现）

| 事项 | 归属 | AI-1 提供 | AI-1 不做 |
|---|---|---|---|
| `Data\Exchange` 摆渡通道策略 | AI-2（层4） | 目录已建 | 不实现访问控制 |
| RAM 盘驱动与预热清单 | AI-2（6 件套） | `ramcache.json` 契约 + 目录 | 不装 ImDisk/PrimoCache、不写预取逻辑 |
| MSIX 打包与挂载 | AI-4（8.2/13.5） | `Data\MSIX` 目录与选型建议 | 不写 `MSIX-Attach.ps1` |
| BitLocker / Defender 排除 | AI-4（第 10 章） | 容量预算已预留 | 不做加密与杀软配置 |
| 兼容矩阵与混沌注入 | AI-5（第 11 章） | `Bench-Storage.ps1` 的存储基线 | 不做软件级测试 |
| `bench/` 汇总入库 | AI-5 | `Bench.md` 口径与实测区 | 不写 `bench/<日期>.md` |

---

## 14. 已知边界（如实声明）

1. **未实机验证**：本轮在 Linux 沙箱完成，Windows 专有 cmdlet（`New-VHD` / `Mount-VHD` / `dism` / `fsutil` / `powercfg`）无法执行，全部脚本只做了词法级结构检查（括号/引号/here-string 闭合）与人工逻辑复核，未跑过 PowerShell 解析器与真机。
2. **`-Sparse` 固定盘**为工程折中，Hyper-V 对稀疏固定盘的长期行为需上机确认（重点观察 `fsutil volume filelayout` 的实占与碎片）。
3. **顺序读指标偏乐观**（含系统缓存），只用于回归对比，不作为选盘结论。
4. **寿命公式**基于 9.1 的标称 TBW 与 1.5 的假定写放大；实际以盘的 SMART「已写入总量」为准，建议 AI-5 在压测中一并采集。
5. **Data 容量**已按 780GB 修正，与分工总览的 800GB 不一致，需在主计划分工表同步。

---

> 本文对应 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 第 3、9 章；总表第 3、9 行已打 ✅。压测口径与实测区见 `portable/AI1/Bench.md`。
