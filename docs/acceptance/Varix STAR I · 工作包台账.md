# Varix STAR I · 工作包台账（MD3 附录 B 执行面）

按 MD3 附录 B 模板逐包一行；状态与判据同记（MD3 1.5），台账与代码同库同提交（R4 恢复点纪律）。里程碑对账时逐行核。

| WP 编号 | 状态 | 判据状态 | 依赖状态 | schema 变更 | 收工日 |
| --- | --- | --- | --- | --- | --- |
| WP-101 | 收口（宿主侧） | B-101~106 六绿-宿主（实机/QEMU 依赖项=环境未就位类，随 WP-102 补测） | 无前置 | limine.conf 固化（interface_version + hash 元数据、default_entry、comment 字段；bootconf::render 单源） | 2026-09-24 |
| WP-103 | 收口（宿主侧） | B-302~307 六绿-宿主（B-301 实机 72h=环境未就位类，B-305"Variable 启动正常"半句随实机补测） | WP-101（已收口） | 新增 `portable/engine/hardening/` 五件（vxlib / harden_quad / harden_vcruntime / recheck / run_all）；deploy↔recheck 契约=deploy_report.json（gate.sha256 + vcruntime.hashes） | 2026-09-24 |
| WP-104 | 收口（宿主侧对账） | B-2703 绿-宿主（f027b 孤儿风暴 64 轮 + F027 千次循环）；篇 26 十二条款对账全落地面（详见 MD2 篇 26 回写）；页表/APIC 实机面=环境未就位类 | WP-101（已收口） | 无 schema 变更（spawn alive 收紧为 F009 契约对齐修复） | 2026-09-24 |
| WP-102 | 收口（宿主侧） | B-201~207 七绿-宿主（实机互通/合成器上屏面=环境未就位类，随 WP-22x/WP-201）；schema 冻结=全链第一闸落地（详见 MD2 篇 2 回写） | WP-101/104（均已收口） | 新增 `kernel/varix/src/handoff/` 五协议件（state/snap/flush/arming/screen）+ legacy 迁移 + `portable/engine/handoff/vx_handoff_proto.py` 参考实现 + 双夹具；handoff.json integrity 正则化口径/草稿目录/状态计数口径冻结（schema 先行纪律，WP-22x/WP-203 依此对表） | 2026-09-24 |
| WP-105 | 未开工 | — | WP-101 | — | — |
| WP-106 | 未开工 | — | WP-101 | — | — |

## WP-101 收口明细（2026-09-24）

**交付面**：
- `kernel/varix/src/bootconf.rs`（新建）：limine.conf 唯一权威解析面——拥有型 `BootConf`（固定容量、零静态、并发安全）、`ConfIssue` 词表（8 类问题登记）、WD-003 `entry_render` 灰显判定、FNV-1a `conf_hash`（与 bootchain::hash_bytes 同源）、`render()` 安装器单源；`include_str!` 把仓库根 limine.conf 编译期嵌入测试，文件漂移即红。
- `kernel/varix/src/oneshot.rs`（新建）：OneShot 全链——40 字节帧编解码（魔法/schema/FNV 校验和）、`arm_conf_text`/`restore_conf_text` conf 改写（越界拒绝）、`on_boot_verify` 启动回读判定（双层消费兜底）、`choose_handoff_path` 三路径降级（Oneshot→BootNext→Menu→不交接）、`gate_check` 闸门三条件、20 组注入矩阵（B-105）。
- `kernel/varix/src/bootchain.rs`：BootStage 6→12 段（B-106）——`ELEVEN_CHECKPOINTS` 十一点位 + `checkpoints_complete` + `standard_timeline` 12 点（0→3100ms）。
- `kernel/varix/src/limine.rs`：B-101 BASE_REVISION 闸（volatile 读 + 三态判定）+ limine.conf 挂为第三可选模块（`../limine.conf`）。
- `kernel/varix/src/main.rs`：`refuse_boot` 拒绝启动出口（串口先行 + 帧缓冲人话屏 + hlt）+ 菜单前契约检查块（ContractMissing → 灰显注入）+ 灰显卡三路执行拦截（键盘 Enter / 倒计时归零默认项 / 鼠标点击）。
- `kernel/varix/src/bootselect.rs`：WD-003 灰显状态机（`WindowsGreyReason::{ConfigMissing, TargetMissing}` + 幂等 set/get）+ 灰显渲染（标题降暗/无高亮框/人话副标题）+ 并行测试串行闸（GREY_STATE_LOCK）。
- `limine.conf`（根目录，固化）：timeout 5 / serial yes / default_entry 0 / 双条目（varix + Windows 11 (USB)→WINESP GUID）/ 条目 comment / 头部元数据 `# interface_version: varix-bli-1` + `# hash: 8207912a`（口径：首个非注释非空行起至 EOF，FNV-1a 32，与内核同源；红线注释原样保留）。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test`（kernel workspace）3179 项全绿（lib 3172 + fuzz 1 + parser fuzz 6），耗时 3m36s；复现命令 = `cd kernel && cargo +1.97.1 test`；日期 = 2026-09-24。

**红项处置**：无红项。过程记录（修复并锁定回归）：F178 过期断言 6→12（BootStage 扩容后未同步）；`gate_check` 对 Incomplete（协议/path 残缺）误放行为 Pass 的真缺陷（B-105 组 03 逮住）；spin_lock 争用断言时序 flaky（起跑 Barrier + 首临界区拉长，从概率变必然）。

**环境偏差登记（不阻断，随队跟踪）**：
1. 冻结工具链 1.97.1 的 clippy 组件在本机损坏（rustup 组件账本缺条目，add/remove 均异常）——clippy 门禁以 1.98.1 视图代跑：2425 条警告全为存量（1.98 新 lint 全开），非本包引入；按"手术式改动"纪律不在本包清理，留专项。
2. 1.97.1 rustc 存量警告 31 条（xhci/ahci/winapi/usrshell/ring3/exfat/msc/security/winsurf/bootcfg 等存量文件）；WP-101 触碰文件（bootconf/oneshot/bootselect/main/limine/bootchain）零警告——本轮引入的 5 条（unused import + 4 处冗余 mut）已在收工前清零。

**WP-101 最丑角落（m4 复盘用）**：`main.rs` 的 `boot()` 已接近 700 行，契约检查块与灰显拦截散在引导序列里——WP-102 交接状态机接线时应把「菜单决策面」收拢为独立函数，boot() 只留时序骨架。

## WP-103 收口明细（2026-09-24）

**交付面**（落点 `portable/engine/hardening/`，收编源 `_attic/vx-stability4.py`、`_attic/vx-vcruntime.py`）：
- `vxlib.py`（核心库）：四纪律的工程化兑现——`LastWords` 遗言机制（逐行 flush + excepthook 全量栈 + CLEAN EXIT 标记 + 关闭后 FATAL 转 stderr 守卫）；`RegBackend` 抽象 + `RegCliBackend`（reg.exe 封装，90s 超时把 U 盘掉线的挂死变成可诊断失败）+ `FakeRegBackend`（内存后端：inject_value 篡改注入 / fail_on 故障注入 / ops 操作流水审计）；`HiveMount` 装载纪律（load→yield→finally unload，三次重试 + 查询确认，异常路径也卸载）；`Journal` JSONL 回滚账本（反向恢复 before/删除新建值）；`ensure_value` 幂等+写后必读单步原语；`value_match` 跨后端值匹配口径（REG_DWORD 的 0x50/80 归一，deploy/recheck/selftest 三方共用）；`sha256_file`。
- `harden_quad.py`（四板斧本体，B-302 审计对象）：板斧一 UASPStor.ImagePath→`\SystemRoot\System32\drivers\usbstor.sys`（REG_EXPAND_SZ）+ usbstor/USBXHCI/USBHUB3/UASPStor Start=0；板斧二 Services\USB\DisableSelectiveSuspend=1 + ENUM\USB 设备实例 SelectiveSuspendEnabled=0 尽力而为；板斧三 IoTimeoutValue 0xf→0x50；板斧四 BCD 文件 SHA-256 闸（record/check，离线可用）；附带项 PortableOperatingSystem=1 / CrashControl.AutoReboot=0 / DumpEnabled=1。SPEC 单一事实源（deploy 与 recheck import 同一份，期望值永不写两遍）；身份防呆前置（PortableOperatingSystem==1 主指纹，缺失回退 UASPStor.Start==0 指纹，对不上零写动作中止并输出人话）。
- `harden_vcruntime.py`（豁免件，唯一允许文件操作）：六件套清单锁死；段一哈希幂等复制（源/落盘双验）；段二版本提取（GetFileVersionInfo，试 040904B0/040904E4 两码页）；段三注册面（WinDLL 实际加载，探后 FreeLibrary 释放映射；离线返回 None 如实降级不记绿）；`audit_vcruntime` 复检三段审计（pass/fail/degraded 三态，degraded 不冒充绿）。
- `recheck.py`（只读复检，B-306）：四项复检（四板斧 SPEC 读回 / VC 三段 / BCD 闸比对（无基准=闸没上=fail，宁红勿漏）/ 交接分区可达性）；判定口径=明确 fail 拉红、未执行 None 不记绿、至少一项明确 pass 才算整体绿；退出码 0/2。
- `run_all.py`（编排入口）：deploy（身份确认→四板斧→附带→BCD 闸→VC→落 deploy_report.json+journal；生产 cli 后端必须 --yes）/ recheck / rollback / selftest 四子命令；selftest 八组宿主证据。

**证据三件套**：数据 = selftest 八证据全绿（① 身份防呆零写动作中止；② 写后必读终态逐键==SPEC；③ 幂等三连跑第 2/3 遍全 SKIP + 终态零漂移；④ 篡改 IoTimeoutValue+BCD → 复检全线红灯 exit 2；⑤ 遗言 FATAL@时间戳+步骤+全栈、无 CLEAN EXIT；⑤b 写失败 fail 行记红；⑥ 零文件操作静态审计零命中；⑦ VC 三段逻辑（真六件 copied 6/skip 6、版本 14.51.36247.0、加载探针双向）；⑧ 装载纪律异常路径卸载 + 回滚还原部署前快照），另加绿面 recheck 与 fake cmd 层冒烟；两连跑稳定。复现命令 = `python portable/engine/hardening/run_all.py selftest`；日期 = 2026-09-24。

**红项处置**（selftest 捉住的真实缺陷，均已修复并锁定回归）：
1. `ensure_value` 字符串直比在 CLI 后端下失效（REG_DWORD 读回 `0x50` ≠ 期望 `80`）→ 幂等 SKIP 失效 + 写后必读误报——补 `value_match` 归一口径。
2. selftest 静默 exit 1（无 traceback）：证据④的复检 LastWords 句柄未关 → Windows `TemporaryDirectory` 清理 PermissionError；异常又被已关闭句柄的 excepthook 吞掉——补句柄关闭 + LastWords 关闭后守卫（FATAL 转 stderr）。
3. `load_probe` 的 FreeLibrary 无句型声明 → 64 位句柄 OverflowError 静默吞掉 → DLL 映射残留锁文件——补 `c_void_p` 显式 argtypes。
4. 样本 DLL 两个实证教训：KnownDLL（kernel32 等）被进程钉死，FreeLibrary 无法解除映射；"一份样本冒充六个名字"的内容/名字错配副本掉进导入绑定地狱（WinError 127）——selftest 样本源改用 System32 真六件按本名复制（与产线 SRC 同源），六件不齐则诚实中止。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-301（0xED 实机 72h 长跑）与 B-305"Variable 启动正常"半句依赖 U 盘整机 + Y7000 实机——环境未就位类，随 WP-102 交接接线后补测。
2. 生产路径（cli 后端 + 离线 hive 挂载 + reg.exe）本机未实跑（零真实注册表接触是 selftest 的设计前提）；首次实机部署按 MD2 3.7 复检清单走人工监督。

**WP-103 最丑角落（m4 复盘用）**：recheck.py 的 run_recheck 判定布尔拼装（None 参与的"未执行不记绿"口径）可读性一般，随复检项增多应改为逐项 verdict 表驱动；SPEC 附带项的 `..\Control` 相对回退语法偏隐晦；version_of 码页只试 040904B0/040904E4 两种，非英文语言资源提取不到时如实 degraded（可扩全码页遍历）。

## WP-104 收口明细（2026-09-24 · 宿主侧对账收口）

**定性**：对账收口，非新建模块。勘察确认篇 26 全部条款的实现面在存量内核编号域已就位（mem/sched/quota/uspace 各模块自带 8~22 个测试），真缺口只有两处——手术式补齐，不重复造轮。

**对账审计面**（12 条款 → 存量文件映射，逐条证据见 MD2 篇 26 回写小节）：
- 26.1 内存：`pmm.rs` F051 FrameBitmap + F052 BuddyAllocator（自由链内嵌空闲帧、2GiB 位图 ≈0.03B/页）；`heap.rs` F053~F055 slab / 可失败 GlobalAlloc / 泄漏检测；`mm.rs` F066 DMA 单独通道。
- 26.2 地址空间：`addrspace.rs` + `paging.rs`（页表实机走查=环境未就位类）。
- 26.3 调度：`engine.rs` F076~F100（64 槽、每核队列、10 tick 时间片、RT 200 tick 上限、分层严格抢占）；`policy.rs` F087 延迟仪表 / F092 亲和性；`quota.rs` 三方配额（min_permil 保底 + 三档滞回水位 + 分级回收）。
- 26.4 自证：存量 F027 千次循环 + 本包新增 f027b 组合维度。

**两处手术**（`kernel/varix/src/proc/uspace.rs`）：
1. spawn 补 alive 检查：F009 契约写"父进程必须活着"，原实现只查槽位在不在——僵尸父下也能 spawn，孩子挂永远等不回来的父上（只能等收养兜底）。补 `!state.alive()` 拒绝，死父不生育。勘察确认全部存量调用点均从活父 spawn，无行为面破坏。
2. 新增 `f027b_orphan_zombie_storm_zero_leak`（B-2703 组合维度）：死父拒育前置回归（dad exit → spawn 报 NoParent）+ 64 轮两代家庭乱序死亡风暴（g1 先死挂父 → p1 死孤儿 g2 过继 init 断言 ppid==PID_INIT → wait 回 p1 → 全灭），轮轮断言 live/spaces/zombies 三清回基线 + exhausted==0。

**文档回写**：MD2 篇 26.4 后新增"篇 26 判据实测回写（WP-104 · 宿主侧对账收口）"小节——12 条款对账表 + B-2703 补刀说明。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test`（kernel workspace）3180 项全绿（lib 3173 + fuzz 1 + parser fuzz 6，+1 新压测）；uspace 定向 23/23 绿先行；存量警告 5 条核实全为存量文件（bootcfg/xhci/ahci/msc/exfat_rw/stage4_matrix），触碰文件零新增。复现命令 = `cd kernel && cargo +1.97.1 test`；日期 = 2026-09-24。

**红项处置**：spawn 文档-代码失配 1 处（F009 契约）→ 修复并锁定回归（f027b 前置断言：死父 spawn 报 NoParent）。无其他红项。

**环境偏差登记（不阻断，随队跟踪）**：
1. 26.2 页表实机走查 / 26.3 APIC 实机调度面依赖 U 盘整机 + Y7000——环境未就位类，随 WP-201 对练补测。
2. clippy 门禁仍以 +stable 代跑（1.97.1 组件损坏，同 WP-101 偏差 1）。

**WP-104 最丑角落（m4 复盘用）**：engine.rs 的 64 槽固定容量在未来进程数增长时需要扩容路径评估；policy.rs F087 延迟仪表的导出格式与 WP-402 基准体系的对接契约待 WP-402 时对账；quota.rs 三方配额目前只有内核态压测记账，用户态进程的真实扣减面要等 WP-205 进程 API 接线后才能实证。

## WP-102 收口明细（2026-09-24 · 宿主侧交付）

**定性**：交接协议全链第一闸——schema 冻结先行。五个协议件 + 助手侧参考实现 + 双夹具四端对锁（Rust 写/读 + Python 写/读）；存量 handoff.rs（需求 2 时代菜单 A 卡直通路径）git mv 迁移为 legacy.rs 原样保留，main.rs 调用点经 re-export 零变化。

**交付面**（`kernel/varix/src/handoff/` 六件 + 助手侧一件）：
- `mod.rs`：模块地图 + 对表纪律（WP-22x 助手侧与 WP-203 存储快照依 schema 对表）+ 状态计数口径文档。
- `state.rs`（B-201）：五状态机七迁移表驱动——`TRANSITIONS: [Transition; 7]` 单源（正向主干四边 + 取消边 + 失败边 + 中止确认边），`HandoffMachine::submit` 迁移与拒绝双入体验日志，拒绝不 mutate 状态；Q10 双请求 preserving 锁定 = "没有这条表边"的自然结果，不是特判。8 测试。
- `snap.rs`（B-202/206/207，最大模块）：handoff.json schema 冻结——手写紧凑 JSON（无 serde，内核零依赖）+ integrity SHA-256 封条（正则化口径：body = 不含 integrity 成员的完整文本，读方剥除补回后哈希全等才解析，Q6 先验哈希再解析）+ 版本协商三态（v1 严格缺必填拒收 / 更高版本"能读多少读多少"降级 + 注记 / 更低拒收）+ 256KB 截断为写方职责（字符边界截断 + truncated 置标，读方拒"超限不置标"）+ 重复键一律拒绝 + 解析护栏（深度 64 / 节点 4096 / 字符串 512KB / 全文 1MiB）+ 草稿目录常量冻结（/vx-snap /var-snap /diag /vx-drafts /var-drafts）+ surrogate pair。13 测试。
- `flush.rs`（B-203）：冲刷四步硬序（AppBuffers→Ext4Commit→FatFlush→BlockDrain）+ 15s 总预算两道闸（步自报 Timeout + 管线累计总额守门）+ 超时停 flushing 报错绝不带病重启 + WD-040 五步时序账 `FiveStepLedger`（保全→冲刷→闸门→写变量→重启 ≤25s）。5 测试。
- `arming.rs`（B-204）：武装序列编排——gate_check 先行→Blocked 零写中止→NeedUserConfirm 未确认不硬闯→arm_conf_text→**十次读回**（达标线 READBACK_ROUNDS=10）→不一致降级 BootNext 重试一次（DegradeEvent::OneshotArmFailed 可观测）→兜底也失败 aborted 三路径人话。6 测试。
- `screen.rs`（B-205）：四帧画面字符串表 FRAME_TABLE 单源（render ASCII 实绘 + zh 列存档篇 2.5 原文）；帧 3 无进度条——"装进度条就是撒谎"实证化为像素计数为零断言（帧 2 对照为正）；帧 4 呼吸三角波 breath_alpha + 目标域文案；错误分支三要素模板；FrameLog 四帧无缝衔接且与 WD-040 五步账逐毫秒对账（covers_ledger）。7 测试。
- `legacy.rs`（git mv 自 handoff.rs）：直通路径原样保留，色彩常量改 pub(crate)。7 测试迁移。
- `portable/engine/handoff/vx_handoff_proto.py`：助手侧参考实现（篇 22 协议面种子）——schema 常量镜像、seal/strip_seal/verify_seal、object_pairs_hook 重复键拒绝、字段表校验、gen-fixtures 生成双夹具、selftest 九断言（生成读回/跨实现互通/顶层键序零差异/篡改现行/十组损坏/版本协商/仓库夹具与生成器逐字节一致）。
- `kernel/varix/src/handoff/fixtures/handoff-varix.json` + `handoff-windows.json`：双夹具——varix 夹具与 Rust `serialize(&canonical())` 逐字节对锁；windows 夹含 files 类剪贴板 + `clipboard_skipped_reason`。

**对账补刀**：`kernel/varix/src/main.rs` 三处存量 bin 编译漂移修复（`char_width_scaled` 从未存在的 API→`GLYPH_W`；`wrap_ascii` 返回型矛盾→`&'static str`；`issue_list().is_empty()` 对 impl Iterator 不存在→`next().is_some()`）。`git show HEAD:main.rs` 核实三行 HEAD 即在、本包零改动——定性 WP-101 起 bin 编译从未进验证环（历次只跑 lib test），镜像 check 补上这块验证盲区；本包触碰文件零警告。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test`（kernel workspace）**3219 项全绿**（lib 3212 + fuzz 1 + parser fuzz 6，+39 handoff 新测试 + 7 legacy 迁移）；镜像 `cargo +1.97.1 check --target x86_64-unknown-none --features kernel-image` 全过（bin 编译面首次入验证环）；`python portable/engine/handoff/vx_handoff_proto.py selftest` **9/9 PASS CLEAN EXIT**。复现命令 = `cd kernel && cargo +1.97.1 test` + `cd kernel && cargo +1.97.1 check --target x86_64-unknown-none --features kernel-image` + `python portable/engine/handoff/vx_handoff_proto.py selftest`；日期 = 2026-09-24。

**红项处置**（selftest/测试捉住的真实缺陷，均已修复并锁定回归）：
1. Writer 嵌套丢逗号（7 测试红：`[{...}{...}]`）——单一 `first: bool` 在数组/对象嵌套穿层丢上下文 → 重写为 `first: Vec<bool>` 分层栈 + `after_key` 标志。
2. `clipboard_skipped_reason` 被当 v1 必填误拒（可选字段）→ 改直查不进 take()，走降级注记。
3. higher_version 测试没真删字段（restore_hint 置空串 ≠ 缺席）→ 真删 `,"restore_hint":""` 片段。
4. B-207 注入 #1 原地打空转（'R'→'R' 无操作）→ 'R'→'X'。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-202 实机互通（内核写 handoff.json ↔ 助手侧实读）依赖 U 盘整机——环境未就位类，随 WP-22x 助手侧接线补测；四端互证（Rust 写/读 + Python 写/读）已在夹具层完成，实机只差接线。
2. B-205 合成器上屏面依赖 WP-201 显示管线——四帧文案/结构/断言已锁，上屏走查随 WP-201。
3. 屏幕为 ASCII 字库，zh 列存档不上屏（篇 2.5 双语策略：帧面宽度的物理约束）。

**WP-102 最丑角落（m4 复盘用）**：snap.rs 手写 JSON 解析面无 fuzz 输入语料库（B-207 十二组注入是点覆盖），建议后续用 parser fuzz 通道喂快照语料；state.rs 的体验日志 LogEvent 尚无串口落盘面，实机接线时与体验日志域对账；flush.rs 的 FlushStep 由调用方提供 dyn 切片，实机四步的真实接线面在 WP-203。
