# VARIX last_boot 写回 · 归因方案矩阵（AI-1 设计件）

> **定位**：AI-1 任务清单「last_boot 写回闭环（连续交替 ×5 正确）」的拍板前置件。
> **日期**：2026-09-22。**状态**：等拍板（与《引导序重排方案矩阵》同批）。

---

## 0. 问题一句话

`boot-select.json` 的 `last_boot` 字段（`default_entry="last"` 的解析依据）**内核读得到、写不了**：
exFAT 只读挂载（写请求显式拒绝）+ ESP 副本是 Limine 只读模块通道——在 S4.2（xHCI MSC 直写）
落地前，内核侧没有任何落盘通道。

## 1. 归因分析（谁进过哪个系统，谁能作证）

| 事实 | 证人 | 证人能否落盘 |
|---|---|---|
| 进入过 Windows | Variable（Windows 侧启动即知） | ✅ 能写 SHARED；ESP 副本需提权同步 |
| 进入过 VARIX | 只有内核自己 | ❌ S4.2 前无任何写通道 |

- 「windows」侧：Variable 启动时写 `last_boot=windows` 事实正确（最近一次实际进入的就是
  Windows，无论经 handoff 还是 F12 直进）。
- 「variable」侧：handoff 时刻内核是唯一证人，但写不进任何盘。备选「内核写一个非引导类
  UEFI 变量 + Windows 侧提权读取」被否——读取固件环境变量要管理员权限，每次开机弹 UAC
  是不可接受的体验成本。

## 2. 方案清单

### 方案 1 · Variable 侧先写（**不做**，理由入档）

- **做什么**：Variable 启动写 SHARED 的 `last_boot=windows`。
- **为什么不做**：内核引导期读的是 **ESP 副本**（internal module 通道），SHARED 直写
  内核根本看不到；要让内核看到就得提权刷 ESP 副本——每次开机弹 UAC 不可接受，滞后
  同步则内核读到的是滞后值（假功能）。**不做没有实际价值的中间态功能。**

### 方案 2 · S4.2 根解（推荐）

- **做什么**：S4.2（xHCI MSC 直写 SHARED）落地后——
  1. 内核在「三卡选中 varix / ushell 进入 / handoff 发起」三点直接写 SHARED 的
     `boot-select.json` → `last_boot=variable`（写路径三道闸现成：vfsguard 白名单 +
     SnapshotArea 快照 + 断电注入 ×10）；
  2. Windows 侧 Variable 启动写 `last_boot=windows`（同一文件、同一事实源）；
  3. ESP 副本方案随 S4.2 验收光荣退役（S4.2 验收项原文），内核直读 SHARED。
- **收益**：last_boot 语义完整、无滞后、无 UAC。
- **验收**：「连续交替 ×5 正确」在方案 2 落地后执行（现在验不了 varix 侧写回）。

### 方案 3 · 早期标记类（均被否，登记防重提）

- UEFI 变量标记 + 提权读取 → UAC 噪声，否。
- ESP 时间戳/痕迹推断 → 无写入者，逻辑不成立，否。

## 3. 推荐与拍板问题

**推荐：方案 2（S4.2 根解）；S4.2 落地前 last_boot 保持现状（字段在位、值恒默认，
`default_entry="last"` 语义=回落内置默认），×5 交替验收顺延到 S4.2 之后。**

需拍板的一问：是否接受「×5 验收顺延至 S4.2 后」？（分工图依赖将同步改挂 S4.2+S1.5）
