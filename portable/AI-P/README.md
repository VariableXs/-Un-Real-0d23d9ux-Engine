# AI-P · 便携与部署线（任务 6 / 任务 9 交付）

## 文件清单

| 文件 | 用途 |
|---|---|
| `partition-plan.json` | 五分区容量配置（version=1，容量/对齐全部参数化，改配置不改脚本） |
| `Create-Partitions.ps1` | 任务 6：U 盘五分区 GPT 脚本（幂等重跑 + 4K 对齐校验 + 清盘二次确认 + `-PlanOnly` 纯计算模式） |
| `Init-Shared.ps1` | 任务 9：SHARED 契约初始化（幂等 + 损坏留证重建 + `-ValidateOnly` 校验模式） |
| `Test-AIP.ps1` | 本模块自测（PlanOnly 布局断言 + 契约幂等/损坏恢复/schema 负向用例） |

## 五分区定版布局（1TB 盘）

1. `ESP` 1GB FAT32（GPT 类型 C12A7328…，VARIX 引导器 + Windows 引导文件双链）
2. `VARIX_SYS` 64GB NTFS（内核 ELF + Variable 运行时）
3. `WIN_ENGINE` 300GB NTFS（Windows 引擎 VHDX 差分链 Base→Apps→User）
4. `SHARED` 600GB exFAT（唯一互通面）
5. `SNAPSHOT` 余量 NTFS（快照/回收/恢复）

容量改 `partition-plan.json`；`SNAPSHOT.sizeGB=0` 表示吃余量（最低 `minSnapshotGB`）。总容量低于 `minDiskGB` 拒绝分区。

## SHARED 目录契约

```
SHARED\
├── apps.json            软件登记总表
├── boot-select.json     引导选择页配置（两系统可读写）
├── whitelist\whitelist.json   共享分区白名单规则（默认拒绝）
└── handoff\             意图接力队列（跨系统启动接力）
```

### apps.json schema（定版 version=1）

```json
{
  "version": 1,
  "updated": "ISO8601 时间戳，可空",
  "apps": [
    {
      "id": "唯一短标识",
      "name": "显示名",
      "icon": "图标路径或空",
      "channel": "wine | engine | native-only",
      "tier": "ok | partial | blocked",
      "updated": "ISO8601，可空"
    }
  ]
}
```

- 未知字段必须忽略（前向兼容）；`version` 升版走 `Move-AppsJsonVersion` 迁移占位。
- 三色分级映射：`tier: ok=✅可跑 / partial=🔶逐步补 / blocked=❌不可跑`。

### 各契约文件三方表（写者 / 读者）

| 文件 | 写者 | 读者 |
|---|---|---|
| `apps.json` | Variable 设置页（AI-V）、AI-P 部署脚本 | VARIX 桌面、Windows 引擎侧启动器 |
| `boot-select.json` | 内核（last_boot 写回）、Windows 侧开机小脚本、部署脚本 | VARIX 内核引导、Windows 侧 |
| `whitelist.json` | Variable 白名单管理 UI | 内核 VFS 裁决层（AI-S） |
| `handoff\` | Variable（点击软件写接力） | Windows 引擎开机消费、VARIX 回读 |

## 用法

```powershell
# 纯计算预览（不碰磁盘，无管理员要求）
.\Create-Partitions.ps1 -PlanOnly -PlanDiskGB 1024

# 真实分区（管理员；物理盘或 VHD）
.\Create-Partitions.ps1 -DiskNumber 3
.\Create-Partitions.ps1 -VhdPath D:\varix-usb.vhdx -Yes

# 契约初始化 / 校验
.\Init-Shared.ps1 -SharedRoot S:\
.\Init-Shared.ps1 -SharedRoot S:\ -ValidateOnly

# 自测
powershell -NoProfile -File .\Test-AIP.ps1
```

## 安全边界

- 启动/系统盘直接拒绝操作；清盘必须大写 `YES` 确认（`-Yes` 仅供自动化）。
- 幂等重跑：布局一致即跳过；不一致必须 `-Recreate` 且仍过二次确认。
- 契约 JSON 损坏：改名 `*.corrupt-<时间戳>` 留证后重建默认（不静默吞掉）。
