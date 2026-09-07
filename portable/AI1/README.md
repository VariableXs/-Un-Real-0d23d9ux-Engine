# AI-1 存储核 — 主计划第 3 章 + 第 9 章

对应 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 3.1-3.3（存储架构）与 9.1-9.5（性能与寿命），
含扩充 13.4 / 20.1 / 26 / 29.4。详细说明见 **`docs/AI1-存储.md`**，压测口径见 **`Bench.md`**。

## 脚本清单

| 脚本 | 对应章节 | 何时跑 |
|---|---|---|
| `Create-VHDX.ps1` | 3.1 / 3.2 / 3.3 / 9.2 / 20.1 | 阶段1 造盘（管理员，宿主） |
| `Tune-Guest.ps1` | 9.3 / 9.4 / 9.5 / 26 | 阶段1.5 进便携系统内一次 |
| `Link-DataApps.ps1` | 3.2 / 13.4 | 阶段1.6 系统内建读写分离链接 |
| `Bench-Storage.ps1` | 9.1 / 11.3 | 阶段1.7 选盘验证与基线 |
| `Maintain-VHDX.ps1` | 3.3 / 9.3 / 26.4 | 每月维护 |

## 三步上手

```powershell
# 1) 造盘：1TB 1000MB/s 定版 = 固定 150GB + 64KB 簇 + Data 七目录 + 离线调优
.\Create-VHDX.ps1 -IsoPath C:\Win11_22H2.iso -OutDir D:\Variable-USB -TuneHost

# 2) 启动便携系统（AI-2 负责），进系统后：
.\Tune-Guest.ps1 -Report
.\Link-DataApps.ps1 -DataRoot D:\Variable-USB\Data

# 3) 压测与验收
.\Bench-Storage.ps1 -Path D:\ -Vhdx D:\Variable-USB\Variable-OS.vhdx
```

## 与 `portable/Create-VHDX.ps1` 的关系

根目录那份是快速上手原型（主计划附录 A / `portable/README.md` 指向它），**保持不动**；
1TB 1000MB/s 定版一律用本目录的 `Create-VHDX.ps1`，逐行差异与修正清单见
`docs/AI1-存储.md` 第 11 节（8 处修正：引号传参、返回码校验、分区复用、
`boot.wim` 误选、4K 对齐、簇大小回读、Index 参数化、try/finally 卸载）。

## 验收（主计划 11.1）

- [ ] CrystalDiskMark / `Bench-Storage.ps1`：SEQ ≥ 900 MB/s，4K ≥ 20 MB/s
- [ ] `Variable-OS.vhdx` 150GB 固定；`-Sparse` 时回收后实占 < 20GB
- [ ] 装 Blender 到 `Data\Apps` 后 VHDX 大小不变
- [x] 脚本落地与文档（本轮完成）；实机数据待 1TB 盘到货由 AI-5 联调补入 `Bench.md` 第 4 节
