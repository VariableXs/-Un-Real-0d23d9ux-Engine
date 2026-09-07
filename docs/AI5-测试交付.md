# AI-5 测试交付 — 第11章 测试与验收 + 第12章 交付与运维 实现说明

> 归属：`portable/AI5/`（脚本与数据）+ 本文（`docs/`） · 对应主计划：第11章 + 第12章 + 扩充 19/20/21/22/24/27/28/30
> 作者：AI-5 交付核 · 日期：2026-09-07
> 范围约束：只新增/修改 `portable/AI5/`、`portable/tests/` 与本文档；不改 AI1/2/3/4 的实现文件。
> 一句话：把「Top200 兼容矩阵 / 混沌注入 / 性能基线 / 四阶段交付 / 日常运维」做成**可运行、可留证、可门禁**的脚本闭环，并且**没有实测证据的项一律标 `todo`，不自动打 ✅**。

---

## 0. 交付物总览

| 文件 | 主计划章节 | 作用 | 关键动作 |
|---|---|---|---|
| `Compat-Matrix.ps1` | 11.1 / 扩充16 | Top200 兼容矩阵：清单、实测、报告、回填 | `-Action List / Run / Report / Fill-ExeHint` |
| `Chaos-Inject.ps1` | 11.2 / 扩充15/21 | 混沌工程：12 场景（10 必测 + 2 自动注入） | `-Action List / Plan / Run / Report` |
| `Bench-Perf.ps1` | 11.3 / 扩充17 | 性能基线：顺序 / 4K / 启动 / 内存 + 门禁 | `-Action Manifest / Run / Gate / Report` |
| `Accept-Gate.ps1` | 1.3 / 扩充19/28 | 验收单汇总与门禁（14 项） | `-Action Init / Report / Check [-Strict]` |
| `Deploy-To-USB.ps1` | 12.1-12.4 / 扩充19 | 四阶段落地编排 + 成品盘核验 | `-Action Preflight / Stage1..4 / Verify / All` |
| `Maintenance.ps1` | 12.5 / 扩充22/26 | 运维：优化、备份、一键还原、计划任务、调优清单 | `-Action Status / Optimize / Backup / Restore / Schedule / Tune` |
| `AI-Integration.ps1` | 拆分计划「等 AI1-4 后联调」 | 5 核交付物齐套性预检 + 只读串跑 | `-Action Preflight / Run-All / Report` |
| `AI5-Lib.ps1` | — | 公共库：日志、JSON、证据、盘符安全闸 | 被上面脚本 dot-source |
| `Data/compat-matrix.json` | 11.1 | 200 条矩阵（5 类 × 40）+ 预算 + 判定枚举 | 由 `Compat-Matrix.ps1` 读写 |
| `Data/chaos-scenarios.json` | 11.2 / 21 | 12 个场景的注入方式/步骤/期望/证据 | 由 `Chaos-Inject.ps1` 读 |
| `portable/AI5/USER-MANUAL.md` | 扩充27 | 小白版用户手册（7 步） | 文档 |
| `portable/AI5/FAQ.md` | 扩充24 / 29 | 术语与 FAQ | 文档 |
| `portable/AI5/README.md` | — | 快速上手 | 文档 |
| `docs/bench/2026-09-07-portable.md` | 11.3 | 性能基线报告（待真机采集） | 文档 |
| `../tests/Run-PortableTests.ps1` | 11.3 门禁 | 真 PowerShell AST 语法 + 数据不变量 + 只读执行 | `pwsh -File portable/tests/Run-PortableTests.ps1` |

> 与 AI-4 的边界：AI-4 交付「拓展 + 安全」的能力脚本；AI-5 只做**测试、验收、交付、运维**，并把 AI-4 的产物纳入验收与联调检查，不重写 AI-4 的任何逻辑。

---

## 1. 第11章 测试与验收 实现

### 1.1 兼容矩阵（11.1 / 扩充16）

**数据**：`Data/compat-matrix.json` 共 **200 条 = 办公 40 / 设计 40 / 开发 40 / 工具 40 / 游戏 40**，应用名全局唯一。
判定枚举只有四种：`pass / warn / fail / todo`，含义写在 JSON 的 `legend` 里。

**关键纪律**：矩阵里的 `pass` 必须有来源。当前只有主计划 11.1 与 16.2 明确点名过的 **14 条**带既有结论
（WPS 12.1、微信 3.9、Blender 5.2、PS 2024、VS2022 17.8、TraeCN、Steam、7-Zip、Git、Node、Python、Figma、Notion、钉钉），
其余 **186 条一律 `todo`**，等真机跑 `-Action Run` 后回填。矩阵不因为「软件很有名」就打 ✅。

**执行**（真机、管理员 PowerShell）：

```powershell
.\Compat-Matrix.ps1 -Action List                        # 看矩阵与覆盖率（只读）
.\Compat-Matrix.ps1 -Action Fill-ExeHint                # 扫 Data\Apps 回填 exeHint
.\Compat-Matrix.ps1 -Action Run -Category office        # 只跑办公类
.\Compat-Matrix.ps1 -Action Run -Filter Blender -Limit 3
.\Compat-Matrix.ps1 -Action Report                      # 生成 compat-report.md
```

- **找得到才测**：`Run` 只在 `Data\Apps`（或矩阵里的 `exeHint`）下定位主程序；找不到记 `skip`，**不臆造结果**。
- **冷/热口径**：第一次拉起进程为冷启动，紧接着第二次为热启动（文件系统缓存已热）；计时到「主窗口句柄出现或进程退出」，60s 超时。测完主动关闭自己启动的进程。
- **预算判定**：热启动 > 6s、冷启动 > 18s、内存 > 4096MB 时该条降为 `warn`，备注写明超了哪一项。
- **A/B 双模式**：A 模式在虚拟系统内跑；B 模式（原生启动）同一脚本在 B 模式环境里再跑一遍，两次 `compat-results.json` 分别归档，`Report` 合并出 A/B 对照。反作弊类条目（英雄联盟 / 无畏契约 / CS2 / PUBG / 永劫无间 / 穿越火线 / WeGame）在矩阵里已标 `aMode=warn`，与扩充 21.6 一致。

### 1.2 混沌工程（11.2 / 扩充15/21）

**数据**：`Data/chaos-scenarios.json` 共 **12 个场景** = 扩充 21 的 **10 个必测场景（S01-S10）** + 主计划 11.2 明确要求的 **2 个可自动注入项（S11 看门狗、S12 `0x80000003`）**。
每个场景都有：`planRef`（主计划出处）、`level`（L1-L4 崩溃分级）、`automatable`（auto/manual）、`dangerous`、`inject`、`steps`、`expect`、`evidence`。

**安全纪律（这是本模块最重要的一条）**：

| 场景 | 类型 | 处理方式 |
|---|---|---|
| S01 拔盘 / S02 宿主蓝屏 / S03 虚拟机内删 C 盘 / S04 宿主中毒 / S07 驱动回退 / S10 五机切换 | `manual` | **脚本只出步骤卡（`-Action Plan`）与证据目录，绝不代为执行** |
| S05 空间不足 | `auto` | 默认 dry-run 只做算术演练；真写盘需 `-AllowFill`，且受 `-FillMaxMB`（默认 512MB）上限与 `finally` 自动清理保护 |
| S06 反作弊检测虚拟机 | `auto` | 只采集 `Win32_ComputerSystem/BIOS/PnPEntity` 特征，不启动任何游戏 |
| S08 BitLocker 恢复密钥 | `auto` | 只读校验恢复密钥格式与保护状态，**不改任何加密状态** |
| S09 4K 对齐 | `auto` | 只读校验 `StartingOffset % 4096` |
| S11 进程被杀（看门狗） | `auto` | **只杀本脚本自己启动的一次性子进程**，带 PID 与标记，绝不触碰用户已运行进程；测「发现死亡」耗时对照 3s 阈值 |
| S12 `0x80000003` | `auto` | 一次性子进程执行 `[Diagnostics.Debugger]::Break()`，校验退出码与「无阻塞式系统错误框」，对应 15.2 熔断策略 |

```powershell
.\Chaos-Inject.ps1 -Action List                      # 12 场景清单（只读）
.\Chaos-Inject.ps1 -Action Plan -Scenario S07        # 取某场景完整步骤卡（只读）
.\Chaos-Inject.ps1 -Action Run                       # 只跑 auto 场景
.\Chaos-Inject.ps1 -Action Run -Scenario S05 -AllowFill
.\Chaos-Inject.ps1 -Action Report                    # chaos-report.md
```

> 如实说明：S11/S12 是**故障注入的可复现模拟**（自启动子进程 + 真实 `Debugger.Break()`），
> 不是往 Blender/libcef 里注入崩溃。对真实大软件的崩溃验证仍需在 Stage2 的隔离虚拟机里人工演练（S01-S04）。

### 1.3 性能基线（11.3 / 扩充17）

```powershell
.\Bench-Perf.ps1 -Action Manifest                    # 指标 + 阈值（只读）
.\Bench-Perf.ps1 -Action Run -TestDrive E:\          # 实测被测卷
.\Bench-Perf.ps1 -Action Run -WriteRepo              # 同时写 docs/bench/<日期>-portable.md
.\Bench-Perf.ps1 -Action Gate                        # 门禁：未达预算退出码 1
```

| 指标 | 预算 | 口径（如实标注） |
|---|---|---|
| `seqWriteMBps` | ≥400 | .NET `FileStream` 4MB 块 + `Flush(true)` |
| `seqReadMBps` | ≥900 | 同文件顺序重读 |
| `rand4kReadMBps` | ≥20 | 4KB 随机读，取样 5 秒，同时记 IOPS |
| `coldStartSec` | ≤18 | 进程首次拉起墙钟 |
| `hotStartSec` | ≤6 | 后续轮次平均 |
| `memoryMB` | ≤600 | `variable*` 进程 WorkingSet 之和（与 `tools/bench.cjs` 同口径） |
| `systemBootSec` | ≤12 | **脚本测不到**，需 `portable/Test-VM.ps1` + 真机计时，未测即标 ⏭ |

> 边界：用户态 FileStream 读写**不等于** CrystalDiskMark 的裸盘值，只能横向对比同一台机器的不同盘/不同簇大小；
> 因此 `Bench-Perf` 的输出用「量级 + 口径」表述，不冒充裸盘指标。`-WriteRepo` 生成的报告里同样写了这句。

---

## 2. 第12章 交付与运维 实现

### 2.1 四阶段（12.1-12.4）

```powershell
.\Deploy-To-USB.ps1 -Action Preflight -Src D:\Variable-USB -Dst E:\   # 只读预检
.\Deploy-To-USB.ps1 -Action Stage1 -IsoPath C:\Win11_22H2.iso         # 造盘（调 portable\Create-VHDX.ps1）
.\Deploy-To-USB.ps1 -Action Stage2                                     # 隔离验证（调 portable\Test-VM.ps1）
.\Deploy-To-USB.ps1 -Action Stage3 -EngineDir ..\..\src-tauri\target\release
.\Deploy-To-USB.ps1 -Action Stage4 -Dst E:\                            # 上盘（robocopy，目标 10 分钟）
.\Deploy-To-USB.ps1 -Action Verify -Dst E:\                            # 成品盘核验（只读）
.\Deploy-To-USB.ps1 -Action All -IsoPath C:\Win11_22H2.iso -Dst E:\    # 串起来
```

- **Preflight** 检查 7 项：造盘/测 VM 脚本在位、Hyper-V 模块、管理员权限、源目录体积、目标盘合法性与空间、`Data` 九个目录齐套、盘总容量（1TB 盘实测应 ≥900GB，对应主计划 1.3「930GB 可用」）。
- **目标盘安全闸**：`Test-Ai5UsbTarget` 拒绝「宿主系统盘」，拒绝「固定磁盘」（除非显式 `-AllowFixedTarget`），拒绝非 exFAT/NTFS/FAT32。这一条是为了防手滑把成品盘部署到宿主 C 盘。
- **Stage3 换皮**：挂载 VHDX → 拷 Variable Engine 到 `<盘>:\Variable` → `reg load` 离线写 `Winlogon\Shell`，同时写 `ShellBackup=explorer.exe` 作为**一键回退**；默认 dry-run，需 `-Yes` 才真写。
- **Stage4 上盘**：`robocopy /E /MT:8`，**刻意不用 `/MIR`**（不镜像删除目标盘已有文件），排除 `Cache/Temp/*.log/*.tmp`，退出码 0-7 视为成功，并打印实际用时对照「10 分钟」目标。
- **Verify**：核验扩充 19 交付清单（`Variable-OS.vhdx` / `Data` / `Data\Apps` / `Data\Exchange` / `Data\Backup` / `PortableVM`）并记录 VHDX 实占。

### 2.2 运维（12.5 / 扩充22 / 扩充26）

```powershell
.\Maintenance.ps1 -Action Status                 # 只读：VHDX 链/碎片/备份/计划任务
.\Maintenance.ps1 -Action Optimize               # Optimize-VHD -Mode Full（月度）
.\Maintenance.ps1 -Action Backup                 # User.vhdx 日备，保留 3 份（14.3）
.\Maintenance.ps1 -Action Restore -DryRun        # 一键还原预演
.\Maintenance.ps1 -Action Restore -Yes           # 真还原（旧文件先改名保留）
.\Maintenance.ps1 -Action Schedule               # 注册计划任务（月度优化 + 每日备份）
.\Maintenance.ps1 -Action Tune                   # 打印 26.1-26.4 调优命令，不自动改宿主
```

- **一键还原**：`Restore` 先把当前 `User.vhdx` 改名为 `*.pre-restore-<时间戳>` 再覆盖，**不裸删**，符合扩充 22 的回退要求。
- **备份保留**：只删 `Data\Backup\User-*.vhdx`，按 `LastWriteTime` 保留最新 3 份。
- **调优不改宿主**：`Tune` 只打印注册表/服务/电源/碎片命令，明确要求「在便携虚拟系统内执行」，符合主计划 10.4 合规。

---

## 3. 扩充章交付

| 扩充章 | 交付位置 | 说明 |
|---|---|---|
| 19 交付清单与验收 | `Deploy-To-USB.ps1 -Action Verify` + `Accept-Gate.ps1` | 交付物核验 + 14 项验收单 |
| 20 脚本级实现 | `portable/Create-VHDX.ps1`（AI-1 域，AI-5 只调用不修改） | Stage1 直接复用，避免两份造盘逻辑 |
| 21 十种必测场景 | `Data/chaos-scenarios.json` S01-S10 + `Chaos-Inject.ps1` | manual 出步骤卡，auto 真执行并留证 |
| 22 运维手册 | `Maintenance.ps1` + `USER-MANUAL.md` | 日常/备份/升级三节 |
| 24 术语与 FAQ | `FAQ.md` | U盘容量、宿主安全、Mac、游戏、丢盘 |
| 26 性能调优清单 | `Maintenance.ps1 -Action Tune` | 注册表/服务/电源/碎片逐项命令 |
| 27 用户手册（小白版） | `USER-MANUAL.md` | 主计划 27 的 7 步，逐步给命令 |
| 28 验收单 | `Accept-Gate.ps1`（A01-A14） | 主计划 28 的 7 项全部纳入并扩到 14 项 |
| 30 未来路线 | 本文 §6 | v1.1-v2.0 |

---

## 4. 验收状态（如实，含未验证项）

`Accept-Gate.ps1` 的 14 项验收只有三种状态来源：**自动脚本实测** / **人工实测录入** / **明确 `todo` 待真机**。

| ID | 主计划 | 验收项 | 状态来源 | 本会话状态 |
|---|---|---|---|---|
| A01 | 1.3/28 | 5 台机 A/B 双模式各启动一次 | manual | ⬜ todo（需 5 台物理机） |
| A02 | 1.3/28 | 系统启动 ≤12s | manual | ⬜ todo（需真机秒表） |
| A03 | 1.3/28 | 大软件热启动 ≤6s | compat | ⬜ 待 `Compat-Matrix -Action Run` |
| A04 | 11.1 | Top200 A/B 跑通 | compat | ⬜ 矩阵已建（14/200 有既有结论） |
| A05 | 11.2/21 | 混沌 10 场景有结论 | chaos | ⬜ 6 项可自动，4+2 项需人工 |
| A06 | 11.3 | 顺序读 ≥900MB/s | bench | ⬜ 待真盘（1TB 1000MB/s 盘未到） |
| A07 | 11.3 | 4K 随机 ≥20MB/s | bench | ⬜ 待真盘 |
| A08 | 11.3 | 待机内存 ≤600MB | bench | ⬜ 待 variable.exe 在位 |
| A09 | 21.1-3 | 虚拟机内删 C 盘宿主无影响 | manual | ⬜ todo（危险，仅人工） |
| A10 | 21.1-1 | 拔盘宿主无痕迹 | manual | ⬜ todo |
| A11 | 10.1 | BitLocker + 拔盘即锁 | bitlocker | ⬜ 待 `Chaos-Inject -Scenario S08` |
| A12 | 12.5/22.2 | 一键还原可用 | restore | ⬜ 待 `Maintenance -Action Backup` |
| A13 | 12.4/19.1 | 成品盘交付物齐全 | deliver | ⬜ 待 `-UsbDrive E:\` |
| A14 | 21.1-9 | 全部分区 4K 对齐 | align | ⬜ 待 `Chaos-Inject -Scenario S09` |

> **不打假勾**：本会话在 Linux 沙箱内完成实现，没有 Windows 宿主、没有 1TB 固态U盘、没有 5 台测试机，
> 因此上表 14 项**全部保持 ⬜**。真机到位后按 §5 的清单跑一遍，`Accept-Gate -Action Check` 会把有证据的项自动转 ✅。

---

## 5. 本会话实际验证了什么（可复现）

| 检查 | 工具 | 结果 |
|---|---|---|
| 9 个新增 `.ps1` 结构完整性（括号/字符串/here-string/续行） | `python3 ps_struct_check.py portable/AI5 portable/tests` | 9/9 通过 |
| 两个数据文件是合法 JSON | `python3 -c json.load(...)` | 通过 |
| 矩阵不变量（200 条 / 5 类 × 40 / 名称唯一 / 点名条目在列） | 生成脚本内 `assert` | 通过 |
| **真 PowerShell AST 语法 + 只读动作执行** | `portable/tests/Run-PortableTests.ps1` | **本沙箱无法执行**（Linux，且 PowerShell 二进制下载域名被网络策略阻断） |

**未验证的部分（明确声明）**：本会话在 Linux 沙箱内**没有执行过任何 PowerShell**
（PowerShell 二进制的所有下载域名被网络策略阻断，仅 npm/PyPI 可达）。
因此 PowerShell 的语法正确性与运行时行为改由 CI 在真 PowerShell 上验证 —— 已接通，方式见下。

**验证接线（已落地，不需要 `workflows` 权限）**：本会话的 GitHub App 令牌无法修改
`.github/workflows/ci.yml`（推送被远端拒绝），但**现有 CI 的 `frontend` 作业本来就在
`windows-latest` 上跑 `npm test`**，于是把自检挂进 vitest：`portable/AI5/__tests__/portable.test.ts`

- 数据不变量用例：任何平台都跑，直接读仓库里的 `Data/*.json` 断言（读交付物本身，不是替身）；
- PowerShell 用例：`win32` 上真调 `pwsh`/`powershell` 执行 `Run-PortableTests.ps1`（超时 10 分钟），
  非 Windows 明确 `skip` 并注明原因。

所以 PR 上的 `frontend` 作业就会在真 PowerShell 上跑完整自检，无需新增 CI 作业。
本地手工补验同样可以：

```powershell
pwsh -NoProfile -File portable/tests/Run-PortableTests.ps1
```

`Run-PortableTests.ps1` 做三件事，全部作用于**本仓库真实文件**：
1. `[System.Management.Automation.Language.Parser]::ParseFile` 解析 `portable/` 下全部 `.ps1`（含 AI-4 的 9 个），有 ParseError 即失败；
2. 校验 `AI5/Data/*.json` 的不变量（200 条、5 类 × 40、名称唯一、枚举合法、`dangerous` 场景必须 `manual`、主计划点名条目在列）；
3. 在临时目录里真跑各脚本的只读动作与 `-DryRun`，检查退出码与输出，并跑一个负向用例（`Verify` 指向宿主系统盘必须判失败）。

---

## 6. 未来路线（扩充30）

- **v1.1**：ARM64 宿主 QEMU 加速分支；`Compat-Matrix` 增加 ARM64 判定列。
- **v1.2**：插件市场上线后，`Accept-Gate` 增加「插件签名四重校验」验收项（对接 AI-4 `Plugin-Manager -Action Verify`）。
- **v1.3**：云同步增量加密；`Maintenance -Action Backup` 之后自动 `Cloud-Sync -Action Sync`（AI-4 已有能力，AI-5 只编排）。
- **v2.0**：外接显卡直通 + 雷电 4；矩阵的游戏类条目按 B 模式直通重测。

---

## 7. 与其他核的接口约定

| 上游 | AI-5 消费方式 |
|---|---|
| AI-1 存储核 `Create-VHDX.ps1` | `Deploy-To-USB -Action Stage1` 直接调用，不复制逻辑 |
| AI-2 隔离核 `Test-VM.ps1` | `Stage2` 直接调用；隔离层的 7 层验证由 AI-2 负责，AI-5 只负责留证与验收汇总 |
| AI-3 兼容核 `src/system/compat/` | 矩阵 `note` 记录 AI-3 的兼容库结论（如 Wallpaper Engine） |
| AI-4 拓展核 `portable/AI4/*` | `AI-Integration -Action Preflight` 核验 9 个脚本 + `Data/`、`Config/`、文档齐套；`Accept-Gate` 的 A11 复用 AI-4 的 BitLocker 能力做只读校验 |

> AI-5 不改上游任何文件。上游缺失时 `AI-Integration` 会把该核标红并列出缺哪个文件，而不是替它补实现。
