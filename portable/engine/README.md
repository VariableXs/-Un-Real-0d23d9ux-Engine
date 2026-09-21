# AI-2 · W2 三级差分链（三体 S3.1）

Base→Apps→User 三层差分镜像链的创建/体检/回滚/演练脚本 + Base 母本制作。
布局契约（与 `src-tauri/src/shell/engine.rs` 的 `EngineConfig::discover` 一致）：

```
<WIN_ENGINE 卷>\Engine\Base.vhdx   母本层（官方镜像装入的可引导 Windows，只读封存）
                     Apps.vhdx    差分于 Base（软件安装落此层，可独立重置）
                     User.vhdx    差分于 Apps（每会话差分，重置即丢）
```

| 文件 | 用途 |
|---|---|
| `engine-chain.json` | 链配置（卷标签/Engine 目录/Base 容量与类型/VM 名） |
| `Base-Mother.ps1` | Base 母本制作（官方 ISO install.wim 装入 VHDX：GPT 三分区+Apply-Image+bcdboot；Plan/Build/Verify；Real=管理员 diskpart+dism，Mock=零管理员文件模拟）。一次成型后只读封存，幂等（-Force 才重建） |
| `Engine-Chain.ps1` | 三级链创建/体检/回滚/回滚演练 ×10（New/Health/Reset/Drill；Diskpart|HyperV|Mock 三后端逻辑同源） |

## W2 施工序（真后端，部署机上执行）

1. `Base-Mother.ps1 -Action Plan  -IsoPath <ISO>` —— 查 WIM 索引
2. `Base-Mother.ps1 -Action Build -IsoPath <ISO> -Index <n>` —— 母本成型并封存
3. `Engine-Chain.ps1 -Action New` —— 建 Apps/User 差分层
4. `Engine-Chain.ps1 -Action Drill -Rounds 10` —— 回滚演练 ×10（验收主证据）

## 用法（管理员 PowerShell）

```powershell
# 预演：解析配置、显示将创建的链（零写入）
.\Engine-Chain.ps1 -Action Plan

# 创建三级链（幂等：已存在的层跳过；Base 创建后置只读封存）
.\Engine-Chain.ps1 -Action New

# 体检：三层存在性/父引用/Base 只读状态
.\Engine-Chain.ps1 -Action Health

# 回滚：重置 user 层（Apps/Base 与数据盘不受影响）；重置 apps 层会连带重建 user 层
.\Engine-Chain.ps1 -Action Reset -Layer user

# 回滚演练 ×10：每轮 Base/Apps 哈希前后对比必须一致（S3.1 验收主证据）
.\Engine-Chain.ps1 -Action Drill -Rounds 10
```

## 后端

- `Diskpart`（默认）：Windows 自带，无需 Hyper-V 模块；`create vdisk ... parent=...`
  原生支持差分盘。创建/挂载需管理员。
- `HyperV`：有 Hyper-V 模块时可用（New-VHD -Differencing）。
- `Mock`：无需管理员，用普通文件模拟层与父引用，完整走同一套状态/哈希/幂等/
  回滚逻辑——用于开发机逻辑验证（本会话交付的 ×10 演练即 Mock 模式）；
  真实 VHDX ×10 演练在 S1.2 部署机上以 Diskpart/HyperV 后端执行。

```powershell
.\Engine-Chain.ps1 -Action Drill -Rounds 10 -Backend Mock -OutDir D:\tmp\engine-chain
```

## 验收对照（实施总步骤图 S3.1 / 分工图 AI-2 S3.1）

- 三级差分链创建脚本化（Base→Apps→User）：`New` ✓
- 容量配置化：`engine-chain.json`（Base sizeGB/type 可配）✓
- 幂等：重复 `New` 已存在层跳过，全链状态一致 ✓
- 回滚 ×10：`Drill -Rounds 10`，每轮 Base/Apps 哈希不变 ✓
- 快照联动：与 SNAPSHOT 快照体系的并入在 M4 整合验收核（链层文件即快照对象，
  不新造快照格式——总案 6.2 简洁性验收）
