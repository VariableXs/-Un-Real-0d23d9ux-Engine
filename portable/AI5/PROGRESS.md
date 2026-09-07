# AI-5 交付核 · 进度看板

> 更新时间：2026-09-07 · 分支 `arena/01a07a51-un-real-0d23d9ux-engine` · 改动已推送 GitHub
> 主计划：第11章（测试与验收）+ 第12章（交付与运维）+ 扩充 19/20/21/22/24/27/28/30

## 总进度

| 阶段 | 状态 | 说明 |
|---|---|---|
| 脚本实现 | ✅ 100% | 7 个脚本 + 公共库已落地 `portable/AI5/` |
| 数据实现 | ✅ 100% | Top200 矩阵 + 12 混沌场景已落地 `portable/AI5/Data/` |
| 文档 | ✅ 100% | `docs/AI5-测试交付.md`、`USER-MANUAL.md`、`FAQ.md`、`README.md` |
| 自检脚本 | ✅ 100% | `portable/tests/Run-PortableTests.ps1` |
| 结构校验 | ✅ 通过 | 9/9 个 `.ps1` 括号/字符串/here-string/续行完整；2/2 JSON 合法 |
| **PowerShell 语法与运行时验证** | ⬜ **未执行** | 沙箱是 Linux 且 PowerShell 下载域名被网络策略阻断；须在 Windows 上跑自检 |
| 真机验收（14 项） | ⬜ 0/14 | 无 Windows 宿主 / 无 1TB 目标盘 / 无 5 台测试机 |
| CI 接入 | ⏳ 待有权限者 | GitHub App 无 `workflows` 权限，YAML 片段已写在 `../tests/README.md` |

## 已实现（第11章 · 测试与验收）

- [x] 11.1 兼容矩阵：`Compat-Matrix.ps1`（List / Run / Report / Fill-ExeHint）+ `Data/compat-matrix.json` 200 条（办公/设计/开发/工具/游戏 各 40）
- [x] 11.2 混沌工程：`Chaos-Inject.ps1`（List / Plan / Run / Report）+ `Data/chaos-scenarios.json` 12 场景
- [x] 11.3 性能基线：`Bench-Perf.ps1`（Manifest / Run / Gate / Report）+ `docs/bench/2026-09-07-portable.md`
- [x] 验收门禁：`Accept-Gate.ps1`（Init / Report / Check）14 项，状态只来自实测或明确 `todo`

## 已实现（第12章 · 交付与运维）

- [x] 12.1 阶段1 造盘：`Deploy-To-USB.ps1 -Action Stage1`（调 AI-1 的 `Create-VHDX.ps1`，不复制逻辑）
- [x] 12.2 阶段2 隔离验证：`-Action Stage2`（调 `Test-VM.ps1`）
- [x] 12.3 阶段3 换皮：`-Action Stage3`（挂载 VHDX + 拷 Engine + 离线写 `Winlogon\Shell`，同时写 `ShellBackup` 可回退）
- [x] 12.4 阶段4 上盘：`-Action Stage4`（`robocopy /E /MT:8`，**不用 `/MIR`**，计时对照 10 分钟目标）
- [x] 12.5 运维：`Maintenance.ps1`（Status / Optimize / Backup / Restore / Schedule / Tune）
- [x] 联调：`AI-Integration.ps1`（AI1-5 交付物齐套预检 + 只读串跑）

## 已实现（扩充章）

- [x] 19 交付清单：`Deploy-To-USB -Action Verify` + `Accept-Gate` A13
- [x] 21 十种必测场景：`chaos-scenarios.json` S01-S10（6 个 manual 只出步骤卡，绝不代为执行）
- [x] 22 运维手册：`Maintenance.ps1` + `USER-MANUAL.md`
- [x] 24 术语 FAQ：`FAQ.md`
- [x] 26 性能调优清单：`Maintenance.ps1 -Action Tune`（只打印命令，不改宿主）
- [x] 27 用户手册：`USER-MANUAL.md`（主计划 27 的 7 步）
- [x] 28 验收单：`Accept-Gate.ps1` A01-A14（主计划 7 项全部纳入并扩展）
- [x] 30 未来路线：`docs/AI5-测试交付.md` §6

## 安全纪律（已内建，不是文档承诺）

| 机制 | 位置 |
|---|---|
| 危险场景只出步骤卡 | `chaos-scenarios.json` 中 `dangerous=true` 一律 `manual`；自检会把「dangerous 且 auto」判为失败 |
| 填盘默认 dry-run | `Chaos-Inject.ps1 -AllowFill` + `-FillMaxMB`（默认 512）+ `finally` 自动清理 |
| 只杀自己启动的进程 | S11 用一次性子进程 + PID 标记，不枚举用户进程 |
| 目标盘安全闸 | `AI5-Lib.ps1` `Test-Ai5UsbTarget`：拒绝宿主系统盘、拒绝固定磁盘、拒绝非法文件系统 |
| 不镜像删除 | 上盘用 `robocopy /E`，刻意不用 `/MIR` |
| 破坏性动作需确认 | `Confirm-Ai5Dangerous` + `AI5_NONINTERACTIVE=1` 时一律拒绝 |
| 还原不裸删 | `Maintenance -Action Restore` 先把旧文件改名保留 |

## 下一步（需真机 / 需权限）

- [ ] 在 Windows 上跑 `pwsh -File portable/tests/Run-PortableTests.ps1`，把语法与运行时问题清零
- [ ] 有 `workflows` 权限的人把 `../tests/README.md` 里的 YAML 加进 `.github/workflows/ci.yml`
- [ ] 1TB 固态U盘到位后：`Bench-Perf -Action Run -WriteRepo` 回填 `docs/bench/2026-09-07-portable.md`
- [ ] 5 台机 A/B 双模式启动，人工实测录入 `manual-results.json`，跑 `Accept-Gate -Action Check`
- [ ] 等 AI-1（第3+9章）与 AI-2（第4+5章）交付后重跑 `AI-Integration -Action Preflight`
