# AI-1 存储压测基线 — Bench

> 归属：AI-1 存储核（主计划 9.1 选盘验证 / 11.3 性能基线 / 3.1 碎片验收）
> 生成工具：`Bench-Storage.ps1`（零依赖，纯 PowerShell；权威值仍以 CrystalDiskMark 为准）
> 汇总入库：由 AI-5 复制到 `docs/bench/<日期>.md`

## 1. 判定线

| 指标 | 底线（主计划 9.1 通用固态 U 盘） | 本项目验收线（1TB 1000MB/s 双接口盘） |
|---|---|---|
| SEQ 顺序读写 | ≥400 MB/s | **≥900 MB/s** |
| 4K 随机 | ≥20 MB/s（≈5120 IOPS） | **≥20 MB/s** |
| NTFS 簇（9.2） | — | **64 KB** |
| 分区 4K 对齐（29.4） | — | **Offset % 4096 == 0** |
| TRIM（9.3） | — | **DisableDeleteNotify = 0** |
| VHDX 碎片（3.1） | — | **< 5%** |
| VHDX 实占（11.1） | — | 固定盘恒 = 150GB；`-Sparse` 或动态盘回收后 **< 20GB** |
| 膨胀率 | — | 装 Blender 到 Data 后 **VHDX 大小不变** |

## 2. 口径（读数前必看）

- **顺序写**：`FileStream` + `FileOptions.WriteThrough`，绕过写缓存，接近真实设备写速。
- **顺序读**：普通带缓存读，**偏乐观**，只用于回归对比，不作为选盘结论。
- **4K 随机写**：容器文件内随机 4KB 偏移直写（WriteThrough），输出 MB/s 与 IOPS。
- **膨胀率** = (VHDX 实占 − 只读挂载后的卷内有效数据) / VHDX 实占 × 100%。
  固定盘（非稀疏）实占恒等于虚拟大小，此指标只对动态盘 / `-Sparse` 固定盘有意义。
- **碎片率** 取 `Get-VHD` 的 `FragmentationPercentage`。

## 3. 复现命令

```powershell
# 选盘验证（对准 U 盘卷）
.\Bench-Storage.ps1 -Path E:\

# 定版全项 + 膨胀率 + 碎片
.\Bench-Storage.ps1 -Path D:\ -Vhdx D:\Variable-USB\Variable-OS.vhdx -OutFile .\Bench-<日期>.md

# 更大样本（掉速点观察，验证 SLC 缓存 >= 100GB）
.\Bench-Storage.ps1 -Path E:\ -SeqMB 20480 -RandOps 16384
```

## 4. 实测区（待 1TB 1000MB/s 盘到货后填入）

> 状态：**未实测**。本轮 AI-1 在 Linux 沙箱完成，Windows 专有 cmdlet（`Get-VHD`/`Get-Volume`/`fsutil`）无法执行，
> 下表为待填模板，由 1TB 盘到货后执行第 3 节命令生成（`Bench-Storage.ps1 -OutFile` 会直接产出同结构表）。

| 指标 | 实测 | 目标 | 判定 |
|---|---|---|---|
| SEQ 顺序写 | 待填 MB/s | ≥900 MB/s | ⬜ |
| SEQ 顺序读（含缓存） | 待填 MB/s | ≥900 MB/s | ⬜ |
| 4K 随机写 | 待填 MB/s / 待填 IOPS | ≥20 MB/s | ⬜ |
| NTFS 簇 | 待填 KB | 64 KB | ⬜ |
| 4K 对齐 | 待填 | OK | ⬜ |
| TRIM | 待填 | 0 | ⬜ |
| VHDX 碎片 | 待填 % | < 5% | ⬜ |
| VHDX 实占 / 有效数据 / 膨胀率 | 待填 GB / 待填 GB / 待填 % | <20GB 实占（`-Sparse`） | ⬜ |
| 装 Blender 后 VHDX 大小 | 待填 GB → 待填 GB | 不变 | ⬜ |

## 5. 寿命计算（与 `docs/AI1-存储.md` 第 9.2 节一致）

`寿命(年) = TBW ÷ (日写入 × 365 × 0.7 × WAF)`；`0.7` = CompactOS 削减 30% 写入，`WAF = 1.5`，`TBW = 600TB = 614,400 GB`。

| 日均写入 | 年闪存写入 | 预期寿命 |
|---|---|---|
| 5 GB/天 | 1,916 GB | 320.6 年 |
| 10 GB/天 | 3,833 GB | 160.3 年 |
| **20 GB/天（设计工况）** | **7,665 GB** | **80.2 年** ✅ |
| 30 GB/天 | 11,498 GB | 53.4 年 |
| 40 GB/天 | 15,330 GB | 40.1 年 |
| 60 GB/天 | 22,995 GB | 26.7 年 |

上机后请用盘的 SMART「已写入总量 / 通电天数」反推真实日均写入，替换表中的设计工况行。

## 6. 容量核算（1TB 真实可用）

| 项 | 数值 |
|---|---|
| 标称 1TB | 931.32 GiB |
| 减分区表 + 文件系统开销（约 0.15%） | ≈929.9 GiB（对应「930GB 可用」验收） |
| `Variable-OS.vhdx` 固定 | 150 GiB |
| **Data 实得** | **≈780 GiB**（分工表写 800GB，超 20.1GiB，已按 780 定版） |
