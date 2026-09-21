# AI-2 · 部署与引擎线 S1.1 + S3 批验收记录（2026-09-21）

> 工位：AI-2（Windows 部署与引擎线）。本批 = S1.1 实测 + S1.2 就绪 + S3.1/S3.2/S3.7 代码级交付 + S3.11 验证关闭。
> 纪律：全程零实机写入（U 盘仅只读测速）；多会话并行纪律遵守（只 add 显式路径）。

---

## 1. 交付清单（对照分工图任务清单）

| 任务 | 状态 | 交付物 | 证据 |
|---|---|---|---|
| S1.1 盘体性能实测 | ✅ 实测完成，**顺序门 FAIL**（如实） | 测速报告 + 探针 | docs/acceptance/deploy/ai2-盘体实测-S1.1-2026-09-21.md；`_attic/ai2-s111-perf.py` + result.json |
| S1.2 Hasleo 部署 | ⏸ 就绪待用户（S1.1 门未过须拍板） | 部署手册 + 只读预检 | docs/deploy/W1-U盘Windows部署手册-2026-09-21.md；`_attic/ai2-w1-preflight.py`（实跑 PREFLIGHT=PASS） |
| S3.1 差分三级链脚本 | ✅ 代码级交付（Mock ×10 PASS；真后端待 S1.2） | portable/engine/ 三件套 | `Engine-Chain.ps1`（PSParser 0 错）+ engine-chain.json + README；Drill ×10 全轮 PASS |
| S3.2 引擎编排底座 | ✅ 代码级交付（17/17 单测；真机链路待 S1.2） | src-tauri/src/shell/engine.rs | 五态状态机/心跳 47631/幂等拉起/看门狗重启/拔盘联动/步进式短持锁 |
| S3.3-S3.6 画面流/输入/VWM 接管 | ⏸ 依赖 S1.2（无镜像无 VM 可验证） | — | 分工图既定依赖，非缺口 |
| S3.7 占位卡拉起协议 | ✅ 闭环（10/10 sessions 测试） | engineSessions 重写 + 通道分流接线 | 27/27 引擎前端测试（model 17 + sessions 10） |
| S3.8 ramcache / S3.10 异常演练 | ⏸ 依赖 S1.2（异常三场景的前端语义已建模并测试） | — | engine-model.test.ts 场景一/二/三 |
| S3.9 延迟拆解三档 | ✅（模型层已在位：五段拆解/P95/三次方差/三档参数化） | engineModel.ts 既有 | EngineTab 延迟面板消费 |
| S3.11 设置页引擎组 UI | ✅ 验证关闭（双域已交付，链路完整） | — | 挂载点/默认值/负向 coerce/持久化四层核对 |

## 2. S3.2 引擎编排底座 · 设计要点（src-tauri/src/shell/engine.rs）

- **五态状态机显式建模**：Closed→Launching→Ready→Hibernating→Hibernated，Failed 从任意态收束；转移表与双域 PS 编排器同源 + 显式 Stop 出口。
- **步进式拉起**：`launch_step` 每调用推进一步（发阶段事件/挂盘/建机/上电/探针），每步独立短持锁——`engine_status` 在整个 150s 冷启动期间永远可响应（长持锁阻塞状态查询的初版设计已自我否决重写）。
- **事件契约**：`engine://state` 事件 `{seq, kind, stage?, reason?}`，seq 单调 + 128 条有界重放缓冲（对齐 boot.rs 模式）；五阶段键与前端 `ENGINE_BOOT_STAGES` 逐键一致（vhdx-mount/vm-create/vm-power/agent-heartbeat/ready-handshake），serde 契约有测试看护。
- **心跳协议**：与 vm_agent.rs 同源（47631 PING→READY）；feature 开启时有常量相等性测试。
- **幂等**：状态机即锁（Launching/Ready 重复 wake 吸收）+ 进程级 BUSY 原子标志（拉起线程全程持有，跨线程不双开）。
- **心跳超时重启策略**：连续 3 次失联（≈15s）→ 自动重启一次（closed 事件→五阶段重放）；再失联 → Failed + crashed 三要素文案。
- **拔盘联动（异常场景一/三）**：REMOVED 原子标志 + 拉起步进内中止检查 → 安静收束 Closed + usb-removed（只发一次），绝不把拔盘伪装成崩溃；usb.rs 监视器已挂钩。
- **后端可插拔**：`EngineBackend` trait——MockBackend（单测/非 Windows）/ HyperVBackend（#[cfg(windows)]，PowerShell cmdlet：Mount-VHD/New-VM Gen2/Start-VM/Save-VM/Restore-VM）。
- **零 unwrap**：生产路径全部显式错误；失败文案统一三要素（发生了什么/为什么/下一步）。
- **新命令 7 条**（lib.rs 注册）：engine_status / engine_replay / engine_preflight / engine_wake / engine_sleep / engine_resume / engine_stop。

## 3. S3.7 拉起协议闭环 · 改动面

- `engineModel.ts`：needWake 扩展为 `lifecycle !== "ready"`（crashed 后重新点击 → 后端复位重拉；hibernated 走恢复；hibernating 后端如实拒绝）——纯模型演进，既有 17 项测试全绿。
- `engineSessions.ts`：①未就绪点击 → 占位窗立即出现（总案 6.7"未就绪占位卡"）+ 登记请求 + 唤醒（幂等）；②就绪事件 → 聚焦既有占位窗（窗体自然过渡到流面）**不双开**，无窗才新开；③防双击双窗（同软件等待中重复点击只登记一次）；④唤醒失败如实透传后端三要素文案；⑤vwm 改动态 import + wake/windowOps/notify 三条可注入缝（vitest 零 Tauri/VWM 依赖）。
- `thirdApps.ts`：`launchThirdApp` 按 channel 分流——channel==="engine" 且 settings.engineEnabled → 走拉起协议；否则既有嵌入路径诚实回落（不静默丢语义，非 engine 通道行为零改动）。
- `lib/ipc.ts`：引擎命令组 7 条 + 三个 DTO 类型（与 Rust serde 契约逐字段一致）。
- `EngineStreamPane`（既有）：starting 叙事卡/ready 流面/crashed 诚实卡三态齐备，取消按钮 → cancelEngineApp ✓。

## 4. S3.1 三级差分链 · 交付与演练证据

- `portable/engine/Engine-Chain.ps1`：Plan/New/Health/Reset/Drill 五动作；容量配置化（engine-chain.json：Base sizeGB/type 可配）；幂等（已存在层跳过）；Base 只读封存；回滚 apps 层连带重建 user 层（Layer-Chain 既有语义沿用）。
- 三后端：Diskpart（默认，无 Hyper-V 依赖）/ HyperV（New-VHD）/ **Mock**（普通文件模拟层与父引用，与真后端共用同一套状态/哈希/幂等/回滚代码路径）。
- **Mock 演练 ×10 PASS**（本会话实测，落点 `_attic/ai2-s31-mock/`，WIN_ENGINE 真卷零写入）：每轮 Base 不变=True / Apps 不变=True / User 重建=True，10/10。
- 真实 VHDX ×10 演练：待 S1.2 镜像就绪后在部署机以 Diskpart/HyperV 后端执行（`-Action Drill -Rounds 10`，管理员）。
- 过程缺陷自纠：初版 `-OutDir` 未接线会在 Mock 模式误写真卷——已修复并加"模拟文件绝不写上真卷"守卫。

## 5. S1.1 实测结论（详见测速报告）

- 顺序：六轮 386.8-395.0 MB/s（均值 391.9，稳定 ~2% 摆幅）→ **门（≥400）FAIL**；
- 4K 随机：20.4-21.5 MB/s（5211-5408 IOPS）→ PASS；
- 判读：USB 3.2 Gen1 链路典型饱和值；4K 为 SSD 级（SSD 盒标准 4K≥15 亦过）；
- 处置：按 S1.1 纪律"不达标 = 如实告知并停下问询"→ **S1.2 暂停待用户拍板**（选项：接受 Gen1 体验 / 换 Gen2 口盒复测 / 推迟）。

## 6. 自检六问

1. **原始需求逐条对上**：分工图 AI-2 清单 S1.1（实测+报告+如实判定）/S1.2（手册+预检就绪）/S3.1（脚本+配置化+幂等+×10）/S3.2（五态/心跳/幂等拉起）/S3.7（取消/失败撤卡+唤醒）/S3.11（验证关闭）——逐条有交付或明确的依赖挂起说明；未完成项（S1.2 执行、S3.3-S3.6/S3.8/S3.10 实测、S3.12）全部因"依赖 S1.2 实机镜像"挂起，非本会话可解。
2. **关键改动回读验证**：所有 Rust/TS/PS1 编辑点经 grep 回读（本次会话再次实证 Edit 批量静默丢失坑——两处编辑回执成功但未落盘，均被回读抓出补齐）；脚本先 PSParser 0 错再执行；预检脚本先自暴露 3 缺陷后修复。
3. **门禁**：tsc 0 错 ✓；vitest 全绿（结果见提交后附注）；cargo test -p variable --lib 全绿（含新引擎 17 项）。涉及线单跑。
4. **边界/负向路径**：Rust 17 测覆盖非法转移/失败收束/心跳超时/重启预算耗尽/拔盘三场景/abort/重放缓冲上界/serde 契约；TS 27 测覆盖取消/失败/防双击/乱序幂等；PS 演练 ×10 每轮哈希门禁。
5. **用户可见界面**：占位卡三态 + 取消 + 诚实通知；引擎设置页此前已过体验验收；本批未新增界面元素（仅接线）。
6. **归档**：本记录 + 测速报告 + 部署手册 + 分工图更新 + 每日日志；过程件（探针/演练脚本/演练产物）全部 `_attic/`。

## 7. 遗留与下一步

1. **等用户拍板**：S1.1 顺序门 FAIL 的处置选项（§5）；
2. S1.2 执行（手册 §3 步骤卡 + 预检脚本）；执行后：S3.1 真后端 ×10、S3.2 端到端（含 Hyper-V 功能启用）、S3.3-S3.10 依次解锁；
3. S3.3-S3.6（画面流/输入注入/VWM 接管）施工需先读总案 6.3-6.6 与 RDP/Spice 流协议选型评估；
4. usb-removed 前端接线核验：usb.rs 已调 engine::usb_removed，`usb://removed` 前端 → resetEngineSession 的既有链路在 M4 联验时走查。

---

## 8. 第二批（同日追加）：S3 批无 S1.2 依赖部分收尾

> 用户指令「继续」后，AI-2 在不依赖 S1.2 实机镜像的前提下，把 S3 批剩余可代码级交付部分全部落地。

| 任务 | 状态 | 交付物 | 证据 |
|---|---|---|---|
| S3.3/S3.4 流通道 v1 | ✅ 代码级（传输抽象冻结 + v1 真实传输） | src-tauri/src/shell/engine_stream.rs | 8/8 单测；tsc 0 错 |
| S3.5 输入注入通道 | ✅ v1 语义定版（注记） | engine_stream.rs 模块头 | mstsc 传输下键鼠/IME 经 RDP 会话原生直达，不做二次注入；30s 无丢键=实机验收 |
| S3.8 ramcache | ✅ 代码级全交付 | src-tauri/src/shell/ramcache.rs | 10/10 单测；命令 ramcache_stats/clear 已注册 |
| S3.10 拔盘复位补线 | ✅ 前端接线补齐 | DesktopShell.tsx | usb://removed → resetEngineSession（import+调用回读确认） |
| S1.1 拍板项 | ⏸ 仍待用户 | — | 顺序门 FAIL（391.9 vs 400）三选项见 §5 |

### 8.1 S3.3/S3.4 流通道 v1 · 设计决策（诚实记录）

- **传输选型**：v1 真实传输 = Windows 内置 mstsc（RDP 协议）——成熟、零新协议代码、其窗口经**既有 embed 管线收编进 VWM**（边框/贴靠/几何持久化零新代码，对齐总案"与 embed.rs 既有语义复用不重写"）。Spice/自研流为可替换项——`build_rdp_file` 与会话注册表冻结的接口不因换传输而变。
- **双模式定版**：全屏桌面会话（S3.3 先行）与 RemoteApp 单应用窗口（S3.4）共用一套会话生命周期（幂等：同 appKey 重复 open 返回既有会话不双开）。
- **画质三档映射**：office（16bpp/高压缩/视觉降级）/ balanced（32bpp/压缩）/ gaming（32bpp/零压缩）——`quality_parse_strict` 测试锁定未知档位拒绝不静默回落。
- **数据隔离红线（RDP 文件级硬门禁，三档 × 逐键测试看护）**：宿主盘重定向强制为空（`drivestoredirect:s:` 空）、打印机/智能卡/串口/POS/PnP 设备全关、剪贴板开（能力对照表声明项）。**SHARED 是唯一互通面，引擎会话内宿主盘不可见。**
- **断流重连**：RDP 层 `autoreconnection enabled:i:1` + 进程死亡走 embed 占位卡语义 + 再次 open 幂等重连。
- **S3.5 语义**：mstsc 传输下键鼠/IME 经 RDP 会话原生直达引擎，Variable 侧不做二次注入（总案"输入事件与阶段 3 输入总线同源"的 Windows 侧等价物=会话原生输入）；30s 无丢键与 IME 全流程为 S1.2 后实机验收。
- 前端落点：EngineStreamPane 就绪态新增「连接画面流（全屏 · RDP）/断开画面流」双按钮（画质取设置总线 engineQuality）；ipc 增 engineStreamOpen/Close/Status 三命令 + StreamSession DTO（serde 契约逐字段一致）。

### 8.2 S3.8 ramcache · 交付要点

- **只缓不落盘（构造性保证）**：模块自身对文件系统零写入（I/O 仅调用方 loader 读盘），数据只存进程内存 `Vec<u8>`——"关机后 U 盘字节级零残留"没有可残留的落盘点；整盘 hash 对比校验为 S1.2 后实机验收项。
- **关机即清双保险**：进程退出=内存归还（物理）+ 引擎 stop/拔盘联动显式 `global_clear()`（engine.rs 布线层两处已接线）。
- **一致性失效**：以 (mtime, size) 为凭据——盘上文件被另一系统改写 → 自动失效重读（`consistency_invalidate_*` 两测试）。
- **预算联动**：默认 256MiB，`set_budget` 随性能档位调整并立即逐出至合规；单条超预算 1/4 整条旁路不挤兑。
- **命中率公示**：`ramcache_stats` 命令（hits/misses/evictions/bytes/hit_rate，零查询恒 0 不虚构）。

### 8.3 门禁（第二批后，单跑）

- tsc 0 错；vitest **2851 passed / 4 skipped**（与第一批持平——本批前端仅接线无新纯逻辑）；cargo test -p variable --lib **348 passed**（+18：ramcache 10 + engine_stream 8）。

---

## 9. S1.1 拍板落地（2026-09-21 21:29）

- **用户拍板：①接受 Gen1 级体验继续**——S1.1 顺序门 FAIL（391.9 vs 400）转为已知且被接受的部署约束，S1.1 关闭、报告终版归档。
- **S1.2 解锁**：执行待用户在场（Hasleo GUI + 首次引导设置 + 重启循环）。
- **就绪度只读核查**（`_attic/ai2-hasleo-probe.py`）：Hasleo WinToUSB **未安装**；常见位置（Downloads/D:/iso 等）**未发现 Win11 ISO** → 用户备料两项（官网下载即可，手册 §0.1 已列）。下载安装第三方软件与数 GB ISO 由用户自主执行，AI 不代劳（外部动作先问再做）。
- S1.2 执行会话的步骤卡 = 手册 §3；执行后按 SOP 下机后 4 项核验收口，另开 acceptance 文档记录。
- 至此 AI-2 辖区内**全部可自主推进的工作已交付完毕**：代码级 8 项（S3.1-S3.5/S3.7/S3.8/S3.11）+ 实测 1 项（S1.1）+ 就绪 2 项（S1.2 手册/预检），余下全部等 S1.2 执行（用户在场）。
