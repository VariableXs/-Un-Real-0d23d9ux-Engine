# AI-4 拓展核 · 进度看板

> 更新时间：2026-09-07 · 分支 `arena/01a07b20-un-real-0d23d9ux-engine` · 范围仍限定 `portable/AI4/`
> 主计划：第8章（无限拓展）+ 第10章（安全合规）+ 扩充14/17/18

## 总进度

| 阶段 | 状态 | 说明 |
|---|---|---|
| 实现 | ✅ 100% | v1.0 全量脚本 + v1.1 加固（见下）均已在 `portable/AI4/` 落地 |
| 静态验证 | ✅ 100% | AI-2/AI-5 的 4 个检查器：词法 10/10、结构 10/10、无大小写冲突；xref 仅 1 处 AppxVolume cmdlet 白名单误报（已记录，待 AI-5 扩白名单） |
| 进度同步 | ⏳ 待 AI-5 | 按分工规范，两份计划总表的 ⬜→✅ 由 AI-5 合并时统一打勾，本核未越界改 docs/ |
| 真机验收 | ⏳ 待 AI5 联调 | BitLocker/MSIX 挂载等需在 Windows/真盘执行 |

## v1.1 加固（2026-09-07，结合 GitHub 上 AI-2/AI-5 已合并成果的复审）

### 修复（复现路径见 `AI4-拓展安全.md` §6.1）

- [x] F1 `Cloud-Sync.ps1` 给只读自动变量 `$args` 赋值 → 8.5 云同步一跑就崩 → 改 `$rArgs`
- [x] F2 `MSIX-Attach.ps1` 读 AppxVolume 不存在的 `PackageFullName` 属性（StrictMode 必炸）+ `Mount-AppxVolume -PackagePath/-VolumePath` 参数不存在 → 改真实 API（`Add-AppxVolume -Path` → `Add-AppxPackage -Volume` Stage → `Mount-AppxVolume -Volume`）
- [x] F3 `Data-Init.ps1` 的 `User.dat` 是空文件，`reg load` 必失败 → `reg save` 种出合法最小 Hive（regf 魔数），非法占位自动补种
- [x] F4 `Plugin-Host.ps1` `-VerifyOnly` 从未实现 → 接入主流程；`AI5_NONINTERACTIVE=1` 时网络插件默认拒绝（对齐 AI-5 CI 约定）
- [x] F5 `Security-Manager.ps1` 换 `-DataDrive E:` 后安全子路径仍指 D: → 未显式覆盖时按 DataDrive 重排
- [x] F6 `Merge-Apps.ps1` Status 读 `$_.Target`（PS5.1 无此属性，建过 Junction 后 StrictMode 必炸）+ New-Partition 盘符不刷新 → 均已守卫

### 补全（主计划点名、v1.0 缺失）

- [x] 8.5 加密：`Cloud-Sync -Action Setup -Crypt`（rclone crypt，密钥落 `Data\Sync\crypt.key`）
- [x] 14.3 版本：Sync 归档 `_archive/<日期>` + `-Action Prune` 保留 7 天
- [x] 14.3 备份：`-Action Backup` User.vhdx → `Data\Backup\User-YYYYMMDD.vhdx` 保留 3 份 + SHA256 日志
- [x] 10.2 强制扫描：`-Action Scan-Exchange` 真跑 `Start-MpScan`，检出威胁退出码 1 + BLOCKED，verdict 落 `exchange-scan-log.json`
- [x] 8.1/18.2 防篡改：`-Action Verify-Chain` 三层 VHDX+MSIX+插件 SHA256 基线，篡改/缺失 exit 1
- [x] 14.1 限额：`Plugin-Host -Sandbox` 插件进 rundll32 子进程 + JobObject（512MB + KILL_ON_JOB_CLOSE + 10s 熔断）
- [x] 8.4 随盘：`path.env` 支持 `${DATA_ROOT}`；Config-Runtime 挂/卸幂等 + Status 显示 Hive 魔数
- [x] 自检：新增 `SelfTest.ps1`（AST 语法 + Config 数据 + 临时目录可逆执行，含真篡改用例），供 CI/真机/联调直接跑

### 留给 AI-5 的两句话

1. `tools/portable/xref_check.py` 白名单建议补 `Get-AppxVolume Add-AppxVolume Remove-AppxVolume`（真实 Appx 模块 cmdlet，现误报 FAIL；tools/ 归 AI-5，本核未动）。
2. 联调时先跑 `portable/AI4/SelfTest.ps1`（不碰真实盘），再按 Chaos-Inject S08 校验 BitLocker。

## 已实现（第8章 · 无限拓展）

- [x] 8.1 层式 VHDX：`Merge-Apps.ps1`（Create / Merge / Add-PortableApp / Status）
- [x] 8.2 MSIX App Attach：`MSIX-Attach.ps1`（Package / Install / Mount / Dismount / Uninstall / Status）
- [x] 8.3 插件化：`Plugin-Host.ps1` + `Plugin-Manager.ps1`（SHA256 + Authenticode + LoadLibrary + 权限）
- [x] 8.4 配置随盘：`Config-Runtime.ps1` + `Data-Init.ps1` + `Config/path.env`
- [x] 8.5 云拓展：`Cloud-Sync.ps1`（Setup(-Crypt 加密) / Sync(7天归档) / Diff / Restore / Schedule / Prune / Backup / Status）

## 已实现（第10章 · 安全合规）

- [x] 10.1 加密：`Security-Manager.ps1 -Action BitLocker`（XTS-AES 256 + 拔盘即锁，`-UsedSpaceOnly` 提速）
- [x] 10.2 杀软：`Security-Manager.ps1 -Action Defender`（排除 Data\Apps）+ `-Action Scan-Exchange`（Exchange 强制真扫描，检出即 BLOCKED/exit 1）
- [x] 10.3 授权：`Security-Manager.ps1 -Action License`（slmgr /dlv + 零售/批量说明）
- [x] 10.4 合规：`Security-Manager.ps1 -Action Compliance/Cleanup`（BCD 备份/清理，不碰宿主 MBR/GPT）

## 已实现（扩充章）

- [x] 14 插件市场/云同步/外设/硬件：`Config/plugin-market.json`、`permissions.json`、`Cloud-Sync.ps1`
- [x] 17 性能压测：`Benchmark.ps1`（Run / Manifest，追加 CSV）
- [x] 18 纵深防御/供应链：`Security-Manager.ps1 -Action SelfCheck` + MSIX/插件签名校验

## 文档

- `portable/AI4/AI4-拓展安全.md` — 第8+10章实现说明（含 §6 v1.1 加固记录）
- `portable/AI4/README.md` — 快速上手
- `portable/AI4/Data/README.md` — `D:\Data` 目录模板说明

## 下一步（AI-5 交付核已接手，2026-09-07）

AI-5 已交付联调与验收工具，AI-4 的"真机验收"现在有了执行入口：

- [x] 交付物齐套性预检 → `portable/AI5/AI-Integration.ps1 -Action Preflight`（会逐项核验 AI-4 的 9 个脚本 + `Data/`、`Config/`、两份文档）
- [x] BitLocker 拔盘即锁的只读校验 → `portable/AI5/Chaos-Inject.ps1 -Action Run -Scenario S08`（查恢复密钥 48 位格式 + 保护状态 + autounlock 已关）
- [ ] 五机 A/B 启动联调 → 需物理机，步骤卡见 `Chaos-Inject.ps1 -Action Plan -Scenario S10`
- [ ] MSIX 挂载后开始菜单出现验收 → 需真环境，步骤见 `AI4/MSIX-Attach.ps1 -Action Mount` + `-Action Status`
- [ ] 统一合并到 `main` 后完成总表全 ✅

> AI-5 不改 AI-4 的任何文件；缺件时只在联调报告里标红，不代做。
