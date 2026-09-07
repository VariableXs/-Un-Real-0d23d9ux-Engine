# AI-5 交付核

对应 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 第 11 章（测试与验收）+ 第 12 章（交付与运维），
以及扩充章 19（交付清单）、21（十种必测场景）、22（运维手册）、24（术语 FAQ）、27（用户手册）、28（验收单）、30（未来路线）。

> 范围约束：只实现/修改 `portable/AI5/` 与 `portable/tests/`，不改 AI1/2/3/4 的实现文件。
> 纪律：**没有实测证据的验收项一律标 `todo`，不自动打 ✅。**

## 文件

| 脚本 | 主计划章节 | 用途 |
|---|---|---|
| `Compat-Matrix.ps1` | 11.1 / 扩充16 | Top200 兼容矩阵：清单 / 实测 / 报告 / 回填 exeHint |
| `Chaos-Inject.ps1` | 11.2 / 扩充15/21 | 混沌工程 12 场景（10 必测 + 2 自动注入） |
| `Bench-Perf.ps1` | 11.3 / 扩充17 | 顺序 / 4K / 启动 / 内存基线 + 门禁 |
| `Accept-Gate.ps1` | 1.3 / 扩充19/28 | 14 项验收单汇总与门禁 |
| `Deploy-To-USB.ps1` | 12.1-12.4 | 四阶段落地编排 + 成品盘核验 |
| `Maintenance.ps1` | 12.5 / 扩充22/26 | 优化 / 备份 / 一键还原 / 计划任务 / 调优清单 |
| `AI-Integration.ps1` | 拆分计划「联调」 | AI1-5 交付物齐套预检 + 只读串跑 |
| `AI5-Lib.ps1` | — | 公共库（日志、JSON、证据、盘符安全闸） |
| `Data/compat-matrix.json` | 11.1 | 200 条矩阵（5 类 × 40）+ 预算 + 判定枚举 |
| `Data/chaos-scenarios.json` | 11.2 / 21 | 12 场景注入方式/步骤/期望/证据 |
| `AI5-测试交付.md` | 11+12 | 实现说明（本文档同级） |
| `USER-MANUAL.md` | 扩充27 | 小白版用户手册 |
| `FAQ.md` | 扩充24/29 | 术语与常见问题 |
| `PROGRESS.md` | — | 进度看板（实时更新） |
| `../tests/Run-PortableTests.ps1` | 11.3 门禁 | 真 PowerShell AST 语法 + 数据不变量 + 只读执行 |

## 快速上手（管理员 PowerShell）

```powershell
# 0. 联调预检：5 个核的交付物是否齐全（只读）
.\AI-Integration.ps1 -Action Preflight

# 1. 测试：矩阵 / 混沌 / 基线
.\Compat-Matrix.ps1 -Action List
.\Compat-Matrix.ps1 -Action Run -Category office
.\Chaos-Inject.ps1 -Action List
.\Chaos-Inject.ps1 -Action Run
.\Bench-Perf.ps1 -Action Run -TestDrive D:\

# 2. 交付：四阶段
.\Deploy-To-USB.ps1 -Action Preflight -Src D:\Variable-USB -Dst E:\
.\Deploy-To-USB.ps1 -Action Stage1 -IsoPath C:\Win11_22H2.iso
.\Deploy-To-USB.ps1 -Action Stage2
.\Deploy-To-USB.ps1 -Action Stage3 -EngineDir ..\..\src-tauri\target\release -Yes
.\Deploy-To-USB.ps1 -Action Stage4 -Dst E:\
.\Deploy-To-USB.ps1 -Action Verify -Dst E:\

# 3. 验收
.\Accept-Gate.ps1 -Action Init      # 生成人工实测模板
.\Accept-Gate.ps1 -Action Report    # 汇总所有证据
.\Accept-Gate.ps1 -Action Check     # 门禁

# 4. 运维
.\Maintenance.ps1 -Action Status
.\Maintenance.ps1 -Action Backup
.\Maintenance.ps1 -Action Optimize
.\Maintenance.ps1 -Action Restore -DryRun
.\Maintenance.ps1 -Action Schedule
```

## 自检（必须在 Windows 上跑）

```powershell
pwsh -NoProfile -File portable/tests/Run-PortableTests.ps1
```

详见 `../tests/README.md` 与 `AI5-测试交付.md`。
