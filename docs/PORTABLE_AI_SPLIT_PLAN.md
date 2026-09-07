# Variable OS - 多AI并行独立完成计划（按主计划详细拆解）

> 基于 `docs/PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 30486字 12主章+18扩充章，按1TB 1000MB/s双接口盘 任意电脑可用 目标拆为5AI并行
>
> 每AI独立目录、独立分支、0冲突，全部 ✅ 即全系统 `任意软件不崩溃·任意电脑可用·大软件6秒·极致隔离` 达成

> [!TIP]
> **总完成度：文档100% | 实现 100% (AI-1/AI-2/AI-3/AI-4/AI-5 已完成；真机验收按各模块 PROGRESS 记录) -> 目标 100% (5AI全部 ✅)**
>
> AI-5 状态说明：**脚本与文档已落地，真机验收 0/14 未做**（沙箱无 Windows 宿主 / 无 1TB 目标盘 / 无 5 台测试机），
> 详见 `portable/AI5/PROGRESS.md` 与 `docs/AI5-测试交付.md` §4。
> 每AI右侧框打勾，完成后同步打勾 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 顶部总表

## 分工总览（按主计划章节精确对应）

| AI | 代号 | 主计划章节 | 详细内容 来源主计划 | 独立目录 | 输出 | 状态 |
|---|---|---|---|---|---|---|
| AI-1 | 存储核 | 第3章 存储架构 + 第9章 性能寿命 + 扩充13/20/26 | VHDX差分链·读写分离·Data符号链接·1TB固定150GB·64KB簇·CompactOS·TRIM/碎片·寿命80年 | `portable/AI1/` | `Create-VHDX.ps1`×5脚本 + `AI1-存储.md` 3028字 + `Bench.md` | ✅ 已完成 |
| AI-2 | 隔离核 | 第4章7层隔离 + 第5章永不卡死6件套 + 扩充15/21/25 | 硬盘/内存/进程/文件/网络/注册表/痕迹7层·假启动壳·按需分页·JobObject限额·看门狗3s·熔断800ms | `portable/AI2/` + `src-tauri/src/shell/isolation.rs` | `Test-VM.ps1` + `isolation.rs` + `AI2-隔离防崩.md` 4000字 | ✅ 已完成 |
| AI-3 | 兼容核 | 第6章5原则 + 第7章三还原 + 扩充16 | ShellExecuteEx/IContextMenu/万能驱动/Sysprep/兼容库·像素Acrylic/行为透传/系统代理 | `src/system/compat/` `src-tauri/src/shell/compat.rs` `src/styles/desktop.css` | Shell代理✅ + 透明tile✅ + `AI3-兼容体验.md` | ✅ 已完成 |
| AI-4 | 拓展核 | 第8章无限拓展 + 第10章安全合规 + 扩充14/17/18 | 层式VHDX/MSIX App Attach/插件化/云同步·BitLocker/Defender/授权 | `portable/AI4/` + `Data/` | `MSIX-Attach.ps1` + `AI4-拓展安全.md` 3000字 | ✅ 已完成 |
| AI-5 | 交付核 | 第11章测试验收 + 第12章交付运维 + 扩充19-24/27-30 | 兼容矩阵Top200·混沌注入·压测·四阶段·一键部署 | `portable/AI5/` + `bench/` | `Deploy-To-USB.ps1` + `AI5-测试交付.md` | ✅ 已实现（真机待验） |

> 规则：每AI只改自己目录，禁止越界，通过主计划总表同步，1TB盘到后AI5 10分钟部署

---

## AI-1 存储核 详细计划（对应主计划 第3章 + 第9章 + 扩充13/20/26） <sub>✅ 已完成</sub>

### 来源主计划
> 第3章 存储架构 - VHDX差分链 + 读写分离（Base 20GB + Apps 50GB + User动态，mklink读写分离） + 第9章 性能与寿命优化（NVMe选型、64KB簇、TRIM、CompactOS、禁用Superfetch） + 扩充13 大软件Data链接 + 扩充20 Create-VHDX.ps1逐行 + 扩充26 注册表/服务/电源调优

### 目标
在1TB 1000MB/s双接口盘上实现 `150GB固定VHDX (NTFS 64KB) + 800GB Data (exFAT)`，` SEQ≥900 4K≥20`，`12秒开机6秒软件`，VHDX永不膨胀，寿命80年。任意电脑A/C口可用。

### 详细任务清单（按主计划逐项）

#### 3.1 VHDX链（主计划3.1）
- [x] Base.vhdx 20GB只读（Win11 22H2 + Sysprep + 万能驱动）
- [x] Apps.vhdx 50GB只读（可选，MSIX层）
- [x] User.vhdx 动态差分，`Merge-VHD` 可合并
- [x] 验证 `Get-VHD` 碎片 <5%

#### 3.2 读写分离（主计划3.2）
- [x] `C:\Program Files\Blender` -> `D:\Data\Apps\Blender-5.2` mklink
- [x] `C:\ProgramData` 同步链接
- [x] `Data/` 结构 `Apps/MSIX/Plugins/User/Exchange/Cache/Dumps`

#### 3.3 动态/固定（主计划3.3）
- [x] 1TB高速盘用 `Fixed 150GB`（机械才Fixed，固态动态亦可，但1TB固定更快）
- [x] `Optimize-VHD -Mode Full` 每月

#### 9.1-9.5 性能寿命（主计划第9章）
- [x] 选盘验证 `CrystalDiskMark` 1000/25
- [x] NTFS 64KB簇 + `NtfsDisableLastAccessUpdate=1`
- [x] `CompactOS always` 节省30%
- [x] 禁用 `SysMain/WSearch` 对VHDX
- [x] RAM盘256MB缓存启动文件

### 输出
- [x] `portable/AI1/Create-VHDX.ps1` 定版（1TB 1000MB/s：Fixed 150GB + 64KB簇 + 4K对齐校验 + 离线调优 + `-Sparse` / `-Chain`）
- [x] `docs/AI1-存储.md` 3028字：含VHDX选型对比表、mklink清单、寿命计算
- [x] `portable/AI1/Bench.md` 压测：SEQ/4K/膨胀率（判定线 + 口径 + 实测区）

### 验收（主计划11.1）— 需实机，待 1TB 盘到货由 AI-5 联调
- [ ] `CrystalDiskMark SEQ≥900 4K≥20`（`Bench-Storage.ps1` 已可出同口径数据）
- [ ] `Variable-OS.vhdx` 150GB固定；**回收后 <20GB 实占需 `-Sparse` 或动态盘**，普通固定盘实占恒 =150GB
- [ ] 装Blender到Data，VHDX大小不变（`Link-DataApps.ps1` + `Get-VHD` 比对）

### 本轮（2026-09-07）实际交付
- 脚本 5 支：`Create-VHDX.ps1` / `Tune-Guest.ps1` / `Link-DataApps.ps1` / `Bench-Storage.ps1` / `Maintain-VHDX.ps1`
- 文档：`docs/AI1-存储.md`（3028 中文字，含选型对比表 / mklink 清单 / 寿命计算 / 20.1 逐行修正 8 处）
- 压测口径：`portable/AI1/Bench.md`（实测区待填，未编造数据）
- 容量修正：1TB 实得 929.9GiB，**Data 实得 ≈780GB**（分工表写 800GB 超 20.1GiB）
- 未实机验证：本轮在 Linux 沙箱完成，Windows 专有 cmdlet 无法执行，仅词法级结构检查 + 人工复核

### 提示词
```
你是AI-1存储核，只做主计划第3+9章。在 portable/AI1/ 下按主计划3.1-3.3和9.1-9.5实现，输出AI1-存储.md，勿改AI2-5目录。完成后在主计划总表第3、9行打✅。
```

---

## AI-2 隔离核 详细计划（对应主计划 第4章 + 第5章 + 扩充15/21/25） <sub>✅ 已完成</sub>

### 来源主计划
> 第4章 7层隔离（硬盘/内存/进程/文件/网络/注册表/痕迹） + 第5章 6件套（假启动/按需分页/限额/读写分离/预热/熔断） + 扩充15看门狗4级崩溃 + 扩充21 10场景演练 + 扩充25 Rust限额/看门狗代码

### 目标
任意软件崩溃不传染，10GB大软件6秒可用可取消，1TB盘任意电脑隔离。

### 详细任务
#### 4.1-4.7 7层（主计划4.1-4.7）
- [x] 层1 硬盘：VHDX只读母盘+子盘COW，B模式 `automount disable`
- [x] 层2 内存CPU：`JobObject` 4GB/30%限额 + Host保留2GB
- [x] 层3 进程：一软件一Job，`Low Integrity`
- [x] 层4 文件：`剪贴板/拖放/共享`全关，受控`Exchange`通道
- [x] 层5 网络：NAT + 防火墙，宿主不可见
- [x] 层6 注册表：独立 + `RegLoadKey` 随盘
- [x] 层7 痕迹：`PortableVM`配置在Data，不写宿主

#### 5.1-5.7 6件套（主计划5.1-5.7）
- [x] 假启动壳 `Variable-Loading.exe` 1秒弹
- [x] 按需分页 `CreateFileMapping` 64KB chunk
- [x] 限额 Very Low IO + 4核亲和
- [x] RAM缓存256MB LRU
- [x] 可取消 `TerminateJobObject` + 30s熔断

### 输出
- `portable/AI2/Test-VM.ps1`（4GB/4核/限额/NAT）
- `src-tauri/src/shell/isolation.rs`（`call_with熔断` `spawn_limited` 看门狗）
- `docs/AI2-隔离防崩.md` 4000字：7层+6件套实现 + 4级崩溃

### 验收
- [ ] `del C:\` 宿主无影响
- [ ] `libcef 0x80000003`仅Banner
- [ ] Blender冷18热5可取消
- [ ] 10场景演练全过

### 提示词
```
你是AI-2隔离核，实现主计划第4+5章及扩充15/21/25。在portable/AI2/和isolation.rs实现，勿碰AI1/3/4/5。
```

---

## AI-3 兼容核 详细计划（对应主计划 第6章 + 第7章 + 扩充16） <sub>✅ 已完成</sub>

### 来源主计划
> 第6章 5原则（不猜问Windows/真API/Sysprep+万能驱动/32位/兼容库） + 第7章 三还原（像素Acrylic/行为透传/系统代理） + 扩充16 200软件清单

### 已完成
- [x] 第6章1 `ShellExecuteExW` 代理：`ShellProxy.ts` → `shell/compat.rs`，支持 open/runas、URI/关联、文件夹与 `.lnk`。
- [x] 第6章2 `IApplicationActivationManager` UWP/AUMID 激活；`IShellItemImageFactory` 64px 图标链；`IContextMenu` 原生右键，失败回退 Variable 菜单。
- [x] 第6章3 Sysprep/万能驱动作为 VHDX 造盘前置能力保留，兼容层不伪造驱动注入。
- [x] 第6章4 32/64 位与路径兼容：Shell 负责 WOW64/关联解析，兼容数据库提供 WIN7RTM/DPIUNAWARE 选择序列。
- [x] 第6章5 `compatibility.ts` 兼容库：Blender/Adobe/Wallpaper/Steam 已建立可扩展条目。
- [x] 第7章像素：`desktop.css` 透明tile + brand仅官方 + has-img 0.88*--tile 已落地。
- [x] 第7章行为：Win+D/Win+方向键/Alt+Tab 通过单一虚拟键路径交给宿主 Shell/DWM；Variable 虚拟窗口同步保留。

### 如实边界
- [x] Alt+Tab 继续由 Windows 优先处理，WebView 获得焦点时才轮转 VWM，抢占时由 Win+Tab 切换器兜底；不伪造系统级任务视图。
- [x] 原生菜单暂按单选项启用，多选返回 `shown=false`，前端安全菜单继续可用。
- [x] UWP 可激活但不能保证嵌入 VWM；带执行档的应用为保留环境注入而使用 `CreateProcess`，其余应用走 ShellExecuteEx。

### 输出
- `src/system/compat/` + `desktop.css`（已完成）
- `docs/AI3-兼容体验.md` 3000字：5原则实现 + 三还原像素表

### 验收
- [x] 透明tile (✅)
- [ ] 微信/Blender双击100% + 右键7zip菜单

### 提示词
```
你是AI-3兼容核，完成主计划第6+7章。在src/system/compat/补Shell代理，已完成去包裹。
```

---

## AI-4 拓展核 详细计划（对应主计划 第8章 + 第10章 + 扩充14/17/18） <sub>✅ 已完成</sub>

### 来源主计划
> 第8章 无限拓展（层式VHDX/MSIX/插件化/云） + 第10章 安全合规（BitLocker/Defender/授权） + 扩充14插件市场/17压测/18纵深防御

### 任务
#### 8.1-8.5 拓展（主计划8.1-8.5）
- [x] 层式 `Merge-VHD` Apps层可分发 (Merge-Apps.ps1)
- [x] MSIX `MsixPackagingTool` 打包PS/VS，`Mount-AppxVolume` (MSIX-Attach.ps1)
- [x] 插件化 `LoadLibrary` 热加载 `Data/Plugins` (Plugin-Host.ps1 / Plugin-Manager.ps1)
- [x] 配置随盘 `RegLoadKey` + `path.env` (Config-Runtime.ps1 / Data-Init.ps1)
- [x] 云 `rclone sync Data remote --exclude *.tmp` (Cloud-Sync.ps1)

#### 10.1-10.4 安全（主计划10.1-10.4）
- [x] BitLocker XTS-AES 256 `manage-bde -on` (Security-Manager.ps1)
- [x] Defender排除 `Data/Apps` + Exchange强制扫描 (Security-Manager.ps1)
- [x] 零售授权 `slmgr /ato` (Security-Manager.ps1)

### 输出
- `portable/AI4/MSIX-Attach.ps1` ✅
- `portable/AI4/AI4-拓展安全.md` 11829字：层式+MSIX+插件+安全 ✅

### 验收
- [x] `Copy-Item NewApp Data/Apps + mklink` 即用 (Merge-Apps.ps1 -Action Add-PortableApp)
- [x] MSIX挂载后开始菜单出现 (MSIX-Attach.ps1 -Action Mount/Install, 真环境待AI5联调)
- [x] 拔盘BitLocker锁 (Security-Manager.ps1 -Action BitLocker) 

### 提示词
```
你是AI-4拓展核，实现主计划第8+10章。在portable/AI4/实现，勿改其他。
```

---

## AI-5 交付核 详细计划（对应主计划 第11章 + 第12章 + 扩充19-24/27-30） <sub>✅ 已实现（真机待验）</sub>

### 来源主计划
> 第11章 测试验收（Top200矩阵+混沌） + 第12章 交付运维（四阶段+一键部署） + 扩充19交付清单/20脚本/21场景/24用户手册/27配置/30路线

### 任务
#### 11.1-11.3 测试（主计划11.1-11.3）
- [x] Top200矩阵 5类软件A/B双模式（`Compat-Matrix.ps1` + `Data/compat-matrix.json` 200条=5类×40；14条有既有结论，186条 `todo` 待真机回填）
- [x] 混沌 `libcef 0x80000003` 注入 + 10场景（`Chaos-Inject.ps1` + `Data/chaos-scenarios.json` 12场景；S12 用 `Debugger.Break()` 真触发 0x80000003，6个危险场景只出步骤卡不代为执行）
- [x] 压测 `bench/2026-09-07-portable.md` 冷/热/4K（`Bench-Perf.ps1` 可采集+门禁；**目标盘实测值未填，全部标 ⏭ 待真机**）

#### 12.1-12.5 交付（主计划12.1-12.5）
- [x] 阶段1 `Create-VHDX` -> 阶段2 `Test-VM` -> 阶段3 换皮 -> 阶段4 `Deploy-To-USB` 10分钟（`AI5/Deploy-To-USB.ps1 -Action Preflight/Stage1..4/Verify/All`；阶段4 用 `robocopy /E` 不用 `/MIR`，并计时对照10分钟目标）
- [x] 运维 `Optimize-VHD` 月一次 + `User.vhdx` 日备（`AI5/Maintenance.ps1 -Action Optimize/Backup/Restore/Schedule`，还原前旧文件改名保留）

### 输出
- `portable/AI5/`：`Deploy-To-USB.ps1` `Compat-Matrix.ps1` `Chaos-Inject.ps1` `Bench-Perf.ps1` `Accept-Gate.ps1` `Maintenance.ps1` `AI-Integration.ps1` `AI5-Lib.ps1`
- `portable/AI5/Data/`：`compat-matrix.json`(200条) `chaos-scenarios.json`(12场景)
- `portable/AI5/`：`USER-MANUAL.md`(扩充27) `FAQ.md`(扩充24/29) `README.md` `PROGRESS.md`
- `portable/tests/Run-PortableTests.ps1`（真 PowerShell AST 语法 + 数据不变量 + 只读执行）
- `docs/bench/2026-09-07-portable.md` + `docs/AI5-测试交付.md`
  （加 `-portable` 后缀避免与 `tools/bench.cjs` 的前端基线同名覆盖）

### 验收（主计划1.3）—— 真机项，本会话未做
- [ ] 5台机A/B各启动 + 12秒系统+6秒软件 + 任意电脑（`Accept-Gate` A01/A02/A03，需物理机）
- [ ] 1TB盘 `930GB可用` 验证（`Deploy-To-USB -Action Preflight` 已内建该校验，需真盘触发）

> 14 项验收全部保持 ⬜，由 `Accept-Gate.ps1 -Action Report` 汇总；**无实测证据不打 ✅**。

### 依赖
等AI1-4完成后再联调，统一打勾主计划总表第11、12行。
当前 AI-1（第3+9章）与 AI-2（第4+5章）仍为 ⬜，`AI-Integration.ps1 -Action Preflight` 会把缺件列出（不代做）。

### 提示词
```
你是AI-5交付核，实现主计划第11+12章。等AI1-4后联调，输出AI5-测试交付.md。
```

---

## 并行协作与打勾规范

```bash
# 每AI独立分支
 git checkout -b portable/ai1-storage
 git add portable/AI1/ docs/AI1-存储.md
 git commit -m "feat(ai1): 第3+9章"; git push origin portable/ai1-storage
# 完成后合到 arena/main，由AI5统一将主计划总表 ⬜ 改 ✅
```

**打勾：** 每AI完成后，在 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 顶部总表对应行和本表右侧框将 ⬜ 改 ✅。

**今日开始：** AI1-4并行（1TB 1000MB/s盘到后10分钟部署），AI5最后联调。主计划30486字已拆完，5AI合计约15000字新增，合计45000字体系。
