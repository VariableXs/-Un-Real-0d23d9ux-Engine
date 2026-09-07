# AI-4 拓展核 · 进度看板

> 更新时间：2026-09-07 · 分支 `arena/01a07a33-un-real-0d23d9ux-engine` · 全部改动已推送 GitHub
> 主计划：第8章（无限拓展）+ 第10章（安全合规）+ 扩充14/17/18

## 总进度

| 阶段 | 状态 | 说明 |
|---|---|---|
| 实现 | ✅ 100% | 所有脚本与配置已在 `portable/AI4/` 落地 |
| 进度同步 | ✅ 100% | `PORTABLE_AI_SPLIT_PLAN.md` / `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 已打勾 |
| GitHub 推送 | ✅ 100% | 已推送分支 + 已开 PR #5 |
| 真机验收 | ⏳ 待 AI5 联调 | BitLocker/MSIX 挂载等需在 Windows/真盘执行 |

## 已实现（第8章 · 无限拓展）

- [x] 8.1 层式 VHDX：`Merge-Apps.ps1`（Create / Merge / Add-PortableApp / Status）
- [x] 8.2 MSIX App Attach：`MSIX-Attach.ps1`（Package / Install / Mount / Dismount / Uninstall / Status）
- [x] 8.3 插件化：`Plugin-Host.ps1` + `Plugin-Manager.ps1`（SHA256 + Authenticode + LoadLibrary + 权限）
- [x] 8.4 配置随盘：`Config-Runtime.ps1` + `Data-Init.ps1` + `Config/path.env`
- [x] 8.5 云拓展：`Cloud-Sync.ps1`（Setup / Sync / Diff / Restore / Schedule / Status）

## 已实现（第10章 · 安全合规）

- [x] 10.1 加密：`Security-Manager.ps1 -Action BitLocker`（XTS-AES 256 + 拔盘即锁）
- [x] 10.2 杀软：`Security-Manager.ps1 -Action Defender`（排除 Data\Apps + Exchange 强制扫描）
- [x] 10.3 授权：`Security-Manager.ps1 -Action License`（slmgr /dlv + 零售/批量说明）
- [x] 10.4 合规：`Security-Manager.ps1 -Action Compliance/Cleanup`（BCD 备份/清理，不碰宿主 MBR/GPT）

## 已实现（扩充章）

- [x] 14 插件市场/云同步/外设/硬件：`Config/plugin-market.json`、`permissions.json`、`Cloud-Sync.ps1`
- [x] 17 性能压测：`Benchmark.ps1`（Run / Manifest，追加 CSV）
- [x] 18 纵深防御/供应链：`Security-Manager.ps1 -Action SelfCheck` + MSIX/插件签名校验

## 文档

- `portable/AI4/AI4-拓展安全.md` — 第8+10章实现说明（11829 字）
- `portable/AI4/README.md` — 快速上手
- `portable/AI4/Data/README.md` — `D:\Data` 目录模板说明

## 下一步（交给 AI-5 交付核）

- [ ] 五机 A/B 启动联调
- [ ] BitLocker 真盘解锁/拔盘锁验收
- [ ] MSIX 挂载后开始菜单出现验收
- [ ] 统一合并到 `main` 后完成总表全 ✅
