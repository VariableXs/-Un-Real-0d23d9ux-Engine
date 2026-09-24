# Varix STAR I · 工作包台账（MD3 附录 B 执行面）

按 MD3 附录 B 模板逐包一行；状态与判据同记（MD3 1.5），台账与代码同库同提交（R4 恢复点纪律）。里程碑对账时逐行核。

| WP 编号 | 状态 | 判据状态 | 依赖状态 | schema 变更 | 收工日 |
| --- | --- | --- | --- | --- | --- |
| WP-101 | 收口（宿主侧） | B-101~106 六绿-宿主（实机/QEMU 依赖项=环境未就位类，随 WP-102 补测） | 无前置 | limine.conf 固化（interface_version + hash 元数据、default_entry、comment 字段；bootconf::render 单源） | 2026-09-24 |
| WP-103 | 收口（宿主侧） | B-302~307 六绿-宿主（B-301 实机 72h=环境未就位类，B-305"Variable 启动正常"半句随实机补测） | WP-101（已收口） | 新增 `portable/engine/hardening/` 五件（vxlib / harden_quad / harden_vcruntime / recheck / run_all）；deploy↔recheck 契约=deploy_report.json（gate.sha256 + vcruntime.hashes） | 2026-09-24 |
| WP-104 | 收口（宿主侧对账） | B-2703 绿-宿主（f027b 孤儿风暴 64 轮 + F027 千次循环）；篇 26 十二条款对账全落地面（详见 MD2 篇 26 回写）；页表/APIC 实机面=环境未就位类 | WP-101（已收口） | 无 schema 变更（spawn alive 收紧为 F009 契约对齐修复） | 2026-09-24 |
| WP-102 | 收口（宿主侧） | B-201~207 七绿-宿主（实机互通/合成器上屏面=环境未就位类，随 WP-22x/WP-201）；schema 冻结=全链第一闸落地（详见 MD2 篇 2 回写） | WP-101/104（均已收口） | 新增 `kernel/varix/src/handoff/` 五协议件（state/snap/flush/arming/screen）+ legacy 迁移 + `portable/engine/handoff/vx_handoff_proto.py` 参考实现 + 双夹具；handoff.json integrity 正则化口径/草稿目录/状态计数口径冻结（schema 先行纪律，WP-22x/WP-203 依此对表） | 2026-09-24 |
| WP-105 | 收口（宿主侧） | B-2701 绿-宿主（对抗矩阵五组注入全拒有名有姓）；B-2702 纯逻辑面交付（命中率与预算对账实测随 WP-203 存储栈，MD3 施工要点明文回补）；六步流水线缺口补齐（第五步 auxv / 第六步 TLS） | WP-102/104（均已收口） | 新增 `kernel/varix/src/proc/auxv.rs`（SysV ABI 初始栈装配）+ `prefetch.rs`（预取指纹/预读清单/命中记账）+ elf.rs 对抗三码（SegmentOverlap/TooManySegments/SegmentBeyondUser）+ entry.rs TLS 计划与提交面 + ring3.rs 目标态 auxv/TLS 接线 | 2026-09-24 |
| WP-106 | 收口（宿主侧交付 + QEMU 对练） | B-2901 绿（FADT RESET_REG 三件组按 ACPI 6.5 表 5.37 落刀 + 三条件放行 + 三件清单审计锚点）；B-2902 绿（四相账本 verbatim 记账 + unpluggable 判定逐字实现 + 通知链超时强收留名）；B-2903 绿-宿主（四环节序列器全链 + 现场带四道闸 + 零 UEFI RS 四级复位阶梯 + panic_test 注入通道），QEMU 循环对练闭环计数见明细；B-2904 绿（合盖语义改 ShutdownConfirm 与篇 29.3 对齐 + 确认分派 + 诚实文案） | WP-101（已收口） | 新增 `kernel/varix/src/panicseq.rs`（panic 四环节序列器/现场带/复位阶梯）+ `power_shutdown.rs`（关机收尾链与可拔电账目）+ 改造 `power.rs`（RESET_REG/合盖语义）+ `bootnext.rs`（reset_via_fadt）+ `main.rs`（panic_handler 重写/boot 钩子/注入点）+ `proc/usrshell.rs`（sys_poweroff 软件链接线/sys_reboot 五级阶梯）+ `_attic/limine-panic-drill.conf` + `scripts/panic-drill.sh` + `scripts/make-iso.sh`（CONF_SRC 注入点，默认零变化） | 2026-09-24 |

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

## WP-105 收口明细（2026-09-24 · 宿主侧交付）

**定性**：六步流水线的缺口手术，不是从零造。勘察确认第一/二/三步在存量 `proc/elf.rs`（解析+拒绝清单）与 `proc/loader.rs`（逐页落位+回滚+退出回收）已扎实就位——真缺口是第五步（栈与环境装配缺 auxv）、第六步（TLS 基址缺席）与 27.2 预取指纹（全无），外加 B-2701 对抗矩阵对 MD3 施工要点"畸形头、重叠段、越界入口"三类清单只覆盖了第一类。手术式补齐，不重复造轮。

**交付面**（四文件改造 + 两文件新建）：
- `proc/elf.rs`（B-2701 对抗面）：新增拒绝清单三码——`SegmentOverlap`（两两区间相交，n≤8 的 O(n²)）、`TooManySegments`（超 8 段拒绝而非静默丢弃）、`SegmentBeyondUser`（补段终点校验 + `checked_add` 接 u64 环绕）；`synth_multi` 多段合成构造器（phdr 逐字段调用方控制）+ 对抗矩阵测试五组注入（重叠/相邻对照/9 段/越顶/环绕）。
- `proc/auxv.rs`（新建，第五步核心）：System V AMD64 ABI 全布局纯逻辑装配器——argc/argv/NULL/envp/NULL/auxv/AT_RANDOM/字符串区，产出有序字节写入计划与 16 字节对齐的最终 rsp；AT_* 键表按 Linux 对齐；`auxv_for_static` 标准条目集；指针类条目回填权威地址；AT_NULL 自动补齐不重复；宿主测试把计划铺进假内存后按 ABI 逐字节读回断言（指针 chasing 到字符串内容）。
- `proc/prefetch.rs`（新建，B-2702）：段清单指纹（FNV-1a 32 与 bootchain::hash_bytes 同源）+ 4MB 粒度预读清单（文件对齐块边界，BSS 不占 IO）+ `PrefetchCache` 三态判定（Hit/Miss/Rebuilt，升级重建 gen+1、重复 record 幂等、指纹漂移防御重建）+ `PrefetchStats`（read_ms/load_ms 分解 + 万分比命中率，B-707 数据源口径）；"指纹只加速不改变装载语义"正确性论证按 MD2 明文写进模块头。
- `proc/entry.rs`（第六步 TLS）：IA32_FS_BASE/GS_BASE/KERNEL_GS_BASE 常量 + `tls_msr_write`（非零+规范+用户半区三条件）+ `commit_tls`（目标态 wrmsr/宿主如实 false）。
- `proc/ring3.rs`（目标态接线）：`apply_stack_and_tls` 把 auxv 写入计划逐条落帧（跨页/越界/页缺失一律拒绝）+ TLS 页紧贴栈底分配；`spawn_hello` 的 iretq 进场 rsp 换为装配后 rsp（指向 argc），FS 基址写入后进场——hello 进程从"裸栈顶"升级为"完整 ABI 初始栈 + TLS"。
- `proc.rs`：模块注册（auxv/prefetch）。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test`（kernel workspace）**3237 项全绿**（lib 3230 + fuzz 1 + parser fuzz 6，较 WP-102 收口 +18 = elf 1 + auxv 8 + prefetch 8 + entry TLS 1）；镜像 `cargo +1.97.1 check --target x86_64-unknown-none --features kernel-image` 全过（spawn_hello target-only 接线随镜像编译面验证），触碰文件零新增警告。复现命令 = `cd kernel && cargo +1.97.1 test` + `cd kernel && cargo +1.97.1 check --target x86_64-unknown-none --features kernel-image`；日期 = 2026-09-24。

**红项处置**（本包测试捉住的设计缺陷，修复并锁定回归）：
1. `tls_msr_write(0)` 原契约放行——地址 0 是用户半区规范地址，但 FS=0 让每次 TLS 访问 fault（给进程埋雷）→ 收紧为非零+规范+用户半区三条件。
2. `prefetch_plan` 测试首版对 offset 非对齐段的块边界断言错误（首块应止于文件 4MB 对齐线）→ 修正断言并补边界推进验证。
3. auxv 指针类条目（AT_RANDOM/AT_EXECFN）首版留调用方占位值不回填 → 装配器回填权威地址（只有它知道实际落位），测试逐字节捉住。

**对账补刀**：`elf::parse` 对第 9 个起的 PT_LOAD 段**静默 continue 丢弃**（原实现）——被丢弃的段不受 `validate` 入口覆盖检查，恶意镜像正好用它藏代码；改为具名拒绝 `TooManySegments`。合法链接器产物在 8 段内，样例集（hello.elf 等）全过。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-2702 命中率与预算对账的实测依赖存储栈（预读引擎 + 记录持久化随 WP-203 定型）——本包交付全部纯逻辑与记账口径，实测按 MD3 施工要点回补。
2. `spawn_hello` 的 auxv/TLS 真实进场为 target-only 代码，宿主以镜像 check 验证编译与计划值，实机行为面随 WP-201 对练补测。
3. AT_RANDOM 现为 PID 播种的确定性 LCG（演示进程 canary 种子），真实熵源接线随安全域对账——模块注释如实标注，不冒充硬件随机。

**WP-105 最丑角落（m4 复盘用）**：auxv 装配器单条写入不做页边界感知（超长 argv 字符串跨页会被 apply 拒绝——生产路径应让装配器感知页界或拆分写入）；预取记录的持久化格式未定（随 WP-203 与读缓存对表）；`prefetch_plan` 块边界按文件偏移对齐，vaddr 与 offset 不同余时目标区间跨块——预读引擎承接时需按块表而非区间映射；第四步动态链接的递归装载骨架随 WP-301 落地时，`auxv_for_static` 需派生 `auxv_for_dynamic`（AT_PHDR/AT_BASE/AT_ENTRY 三键的填充面）。

## WP-106 收口明细（2026-09-24 · 宿主侧交付 + QEMU 对练闭环）

**定性**：m1 最后一包，四判据（B-2901~2904）并行落刀。纯逻辑（宿主可测）与目标态（cfg 门）分离——panic 序列器、关机账本、复位阶梯的核心判定全部宿主可测；`panic_sequence` 总编排只被 panic_handler 调用；`reset_via_fadt` 被 SYS_REBOOT 与 panic 阶梯同源共用（零重复实现）。QEMU 对练管线从三连败（pycdlib base revision 拒绝 → 缺 xorriso → xorriso.exe Exec format error 损坏）攻坚到全链闭环，并连带挖出 WP-101 埋下的 B-101 判定语义雷。

**交付面**（两文件新建 + 五文件改造 + 两脚本 + 一 conf）：
- `panicseq.rs`（新建）：panic 四环节序列器——保护屏（帧缓冲 best-effort+串口兜底）、现场带（0x60000→动态落位 + `#[repr(C)]` 固定布局 + magic/version/msg_len/CRC 四道闸）、十秒倒计时（TSC 自旋不依赖中断）、四级复位阶梯（fadt-reset-reg → 8042 → 0xCF9 → triple-fault，**零 UEFI RS**——ResetSystem 需恒等映射与分配，panic 栈可能已坏绝不走）；`boot_guard_band_hook` 重放（decode 过闸打印上次现场 → 清魔数防重复报告）+ claim 登记（Purpose::LogRing）+ `armed` 降级兜底。
- `power_shutdown.rs`（新建）：篇 29.2 硬序四相账本（Preserve→Flush→NotifyChain→S5）+ `PhaseVerdict`（Ok/Skipped 带人话理由/Forced 留名/Failed 拦画面）+ `unpluggable()` 可拔电判定（preserve+flush+notify settled，S5 不在画面条件）+ 通知链超时强收（Err→Forced 留名继续、预算耗尽剩余步全 Forced）+ `UNPLUG_LINE`/`FIRMWARE_TIMEOUT_LINE` 文案常量。
- `power.rs`（改造）：FADT 复位组四字段（ACPI 6.5 表 5.37：FLAGS@112 bit10、GAS@116、地址@120、值@128）+ `reset_reg()` 三条件放行（声明位+SystemIO+端口 ≤0xFFFF，MMIO 不做）+ `FIRMWARE_MINIMAL_SET` 三件审计锚点 + `LidAction::ShutdownConfirm`（篇 29.3 无休眠支持语义）+ `lid_close_plan` 确认分派 + `LID_CLOSE_NOTICE` 文案。
- `bootnext.rs`：`reset_via_fadt()`（facp→HHDM→表长校验→parse_fadt→reset_reg→端口写，宿主 cfg 编译门）。
- `main.rs`：panic_handler 重写（serial init→KERNEL PANIC 直写→panic_sequence）；boot 钩子（重放+登记）；panic_test 注入点（引导全链完成后）。
- `proc/usrshell.rs`：sys_poweroff 接四相账本 + 可拔电画面 + 三级关机（UEFI Shutdown→ACPI S5→全败文案）；sys_reboot 升五级阶梯。
- `scripts/panic-drill.sh`（新建）：单 QEMU 进程自动循环对练（不带 -no-reboot），`grep -c "guard-band: last panic"` = 完整闭环计数，DRILL-OK/DRILL-PARTIAL 收账。
- `scripts/make-iso.sh`：`CONF_SRC` 注入点（默认零变化）；对练 ISO 由 pycdlib 管线（make-iso-qemu.py --conf）产出，落 `build/panic-drill.iso`——varix.iso 真机正统产物全程不被触碰。
- `_attic/limine-panic-drill.conf`：对练专用 conf（timeout:0 + panic_test=1；注释明示绝不可用于真机）。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test` **3256 项全绿**（lib 3249 + fuzz 1 + parser fuzz 6，较 WP-105 收口 +19 = power_shutdown 9 + panicseq 8 + power 2）；镜像 `cargo +1.97.1 kcheck` 零错误（存量警告非本包引入）。**QEMU 对练闭环实证**（2026-09-24）：`armed at 0x1e401000`（动态落位）→ `KERNEL PANIC: varix\src\main.rs:713` → `guard band written=true` → 十秒倒计时（TSC 真实自旋）→ FADT RESET_REG 复位 → 自动重启 → 第二轮 **`guard-band: last panic @line 713 (tsc 254041650344)`** → 落位逐位复现再 armed——panic 四环节 + 现场带跨复位持久 + 复位阶梯 + 重放消费**单轮完整闭环**。复现命令 = `bash scripts/panic-drill.sh 100`（百次收账数回填于此：见下）；日期 = 2026-09-24。

**红项处置**（测试与对练捉住的真实缺陷，修复并锁定回归）：
1. `GUARD_BAND_MSG_MAX=256` 溢出：256 写进 u8 `msg_len` 域溢出为 0 → 上限收 255（截断语义不变），布局锁定测试同步。
2. CRC 标准值误记 0xCBF4_3921：测试期望值错 → python zlib 实证 IEEE CRC-32(b"123456789") = **0xCBF4_3926**，与 power::crc32 逐位一致 → 修期望值并补 `crc_scope(b"")` 对账。
3. **现场带落位 0x60000 被引导链清零**（对练实证）：第一轮 panic `written=true`、第二轮重放读回全零——Limine BIOS stage 低位工作区覆盖该页，勘察报告"复位不清 RAM、Limine 不触碰"假设证伪 → 落位改 boot 期动态选位（memmap 最大 usable 区间顶部下移 4MB、4KiB 对齐；SeaBIOS 只管低位 1MB 与 EBDA），跨 boot memmap 逐位相同 → 落位逐位复现 → 重放闭环达成。
4. **B-101 判定语义雷**（WP-101 埋、本包对练挖出）：`base_revision_confirmed` 按"三词全零"判定与 Limine 协议不符——协议明文确认 = 只清第 3 成分，且 base revision 3+ 引导器把第 2 成分写为实际使用版本（QEMU 实证 `[magic0, 1, 0]`）→ 判定改 `marker[2] == 0`，测试字面量换协议真实形态，教训入库（外部 ABI 契约的判定必须在真实对端对练才能记绿）。

**对账补刀**：①勘察报告 RESET_VALUE 偏移 122 系笔误，按 ACPI 6.5 表 5.37 于 128 落刀；②`lid_action` 旧 Suspend 语义与篇 29.3"无休眠支持"冲突，合盖改走 ShutdownConfirm 关机路径；③xorriso.exe（tools/xorriso，1.5MB）损坏"Exec format error"实证登记——对练 ISO 切 pycdlib 管线，真机 U 盘产物仍以 make-iso.sh 为准（pycdlib 无 isohybrid 不可 dd）；④`payload_as_str` 尚未在 1.97.1 core 稳定 → panic 下行保留 `#[allow(deprecated)] info.payload()` 并注释留据。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-2903 实机 panic 面（真机 U 盘引导的四环节走查）随整机对练——QEMU 闭环已实证全链语义。
2. 合盖/lid 硬件通道（LidAction::Closed 的真实事件源）随 WP-201/202 输入域接线。
3. 冲刷执行器（Flush 相的真实存储落盘四步）随 WP-203 存储栈——本包账本面已锁定 Skipped/Ok/Forced 三态语义。
4. 百次对练收账：闭环链路已实证，`bash scripts/panic-drill.sh 100` 后台进行中，收账数回填本行。→ **已收账（2026-09-24）：完整闭环 100 轮 / 目标 100，DRILL-OK**——`grep -c "guard-band: last panic" build/panic-drill.log` = 100；单 QEMU 进程（pid 3664）自动循环全程存活，每轮 panic → 现场带跨复位持久 → FADT 复位 → 重放消费完整闭环，零半轮零卡死。

**WP-106 最丑角落（m4 复盘用）**：复位代码三处重复（panicseq 自足副本 / bootnext 共用面 / usrshell 阶梯）——刻意自足（panic 路径零外部依赖）与 DRY 的张力，m2 重构时评估收敛；`format_cd` 返回 ([u8;40], usize) 的固定缓冲设计（no_std 无 format! 的诚实取舍）；通知链空集记账（run_notify_chain 空集 = Done 零开销，真实接线后有步骤才走超时路径——WP-203 承接时需验预算切片与真实步骤数的配比）；现场带落位依赖"memmap 逐 boot 相同"的确定性假设（同固件+同配置成立，热插拔内存/固件升级后落位漂移 → 重放静默失效——四道闸保证不误报，但"持久性"承诺在漂移场景降级为"尽力"）。

## WP-201 收口明细（2026-09-24 · 宿主侧交付）

**定性**：显示合成栈七判据（B-501~507）从 MD2 文档口径落为**判据实装层**——七个新模块（vxwm/bufown/atlas/popup/pump/dragbench/comprecover），判据号 `B-50x` 入 CheckSet 命名使验收口径可 grep（187 处）。架构定案：**不动存量三套并行策略层**（compositor/ 九族 225 项、display.rs、displaysrv.rs），新层引用存量范式（CheckSet/checksum/Lcg 与 compositor 九族同源），七域全宿主可测（零堆定长容量、整数运算、无 f32），全部按 CheckSet 范式（`set.add("B-50x 描述", bool, "人话")` + 每域自检函数 + `all_checks_pass` 单测兜底）。

**交付面**（七文件新建 + 三文件注册 + 一文档回写）：
- `vxwm.rs`（新建，B-503）：VXWM 帧编解码——定长头 10 字节（类型 1 + 版本 1 + 序号 4 + 体长 2 + 发送方 2，全小端无指针）+ 变长体 + FNV-1a 折叠 16 位校验和尾缀；4KB 上限（BODY_MAX=4084）+ 超限 12 字节旁路引用（shm_id+offset+len）；序号严格单调闸（重复/跳号/零号分类计数）；**二十四消息枚举（1..=24，第 25 条在帧层即拒 = C-3 宪法边界可测）**——六分组 5/4/4/5/4/2、方向 13 C2S/10 S2C/1 双向、`min_body_len` 字段序下限冻结文本；bind 版本协商 MIN 降级；回放环 8 槽 FIFO。
- `bufown.rs`（新建，B-504）：缓冲所有权五态机 Free→Held（commit）→Free（frame_done）/Dying（destroy while held）→Gone + 配额 2 起步 3 封顶 + 代数（gen）防陈旧引用防 ABA + Held 中客户端写 = 违规计数（半帧腐坏源）+ `run_drill` 确定性 LCG 交错压测（4096 步多种子）+ 守恒账本（commits/callbacks/reuse_violations）。
- `atlas.rs`（新建，B-505）：字形图集 LRU 60MB 上限 + pin 常驻零重光栅化 + 超限先降渲染密度（≤4 档逐档体积减半）+ 到顶诚实拒绝 + 图集旋钮只降不升（升高拒并记账）+ 冲刷扔非驻留集；pin 走显式注册接口（CJK 字形源到位即接）。
- `popup.rs`（新建，B-506）：浮层物理强制——popup_grab 抓取关系表（嵌套栈序 8 层）+ 区域内正常投递 / 区域外点击合成器直接转译关闭（**八个界外探点穷举零 Deliver**）+ Esc 无条件关最上层 + 还焦点 anchor + 所有者主动关闭连带摘上层并压实——客户端结构上无法逃逸。
- `pump.rs`（新建，B-501）：单线程事件泵四源归一（VXWM/输入/帧时钟/内部超时）+ 排空→归并→裁决（脏区优先全帧、次光标小步走、皆无长眠）+"不需要就不合成"+ 空转成本 permille 模型（静止 1000 圈 0 合成 = 0‰ ≤ 50‰ 判据线）+ 看门狗 WATCHDOG_TICKS=16 恰达进位一次。
- `dragbench.rs`（新建，B-502）：合成步/提交步双打点间隔分布（600 帧窗口 p95≤18181us = 55fps）+ 插入排序零堆分位数 + 掉帧计数 + 掉帧时输入消费 ≤1 帧 + 1080p 全屏拷贝带宽下界模型（600MB/s→13890us 入预算 / 300MB/s→27778us 超预算）+ 四步预算分解（1500+10500+2000+4000=18000≤18181us）。
- `comprecover.rs`（新建，B-507）：注册表快照 schema 冻结（RegEntry 20B/条目定长 + seq + FNV 校验和尾缀防半写）+ **两级持久化语义**——内存映射恢复面每变更即写（合成器崩溃恢复零丢失）+ 写盘五秒合并窗口（断电最多丢五秒布局变更），crash/power_loss 分开记账 + D-04 三秒预算模型（20+25×N+15ms，满表 64 表面 1635ms）+ `run_hundred_drills` D-04 场景百次对练（随机变更+随机崩溃点）。
- `lib.rs`：七模块注册（各带判据号 doc 注释）；`quality.rs`：七域入 `run_full_loop` + 四处域数断言同步至 46（记账下限 39×25+134）；`robust.rs`：七域入 checkup。
- `docs/Varix STAR I · MD2 技术详案.md`：篇 5 末尾"判据实装回写"段——七判据 × 模块 × CheckSet × 实装要点完整表格 + schema 冻结登记（min_body_len / RegEntry）+ B-507 勘误说明。

**证据三件套**：全量 `cargo +1.97.1 ktest` **PASS=3299 FAIL=0**（较 WP-106 收口 3256 +43 = 七域新单测 8+7+7+6+5+5+5）；**CheckSet 134 项**（vxwm 25 + bufown 22 + atlas 18 + popup 21 + pump 15 + dragbench 14 + comprecover 19）+ **单测 43 项**，七域定向全绿；quality 域 31/31 绿（F489/F493 七域宿主全绿验证）；判据号 B-50x 入 CheckSet 命名 **187 处可 grep**（32/30/24/27/24/24/26）。复现 = `cd kernel && cargo +1.97.1 ktest`；日期 = 2026-09-24。

**红项处置**（单测与对练捉住的真实缺陷，修复并锁定回归）：
1. **GlyphAtlas 测试线程栈溢出**（STATUS_STACK_OVERFLOW 0xc00000fd）：4096 槽 × 24B ≈ 98KB/实例，自检函数十几实例并存 >1MB 打爆 robust::tests::f475 测试线程栈 → MAX_SLOTS 缩 512 + 自检函数后半段全部 `{ }` 作用域分段。教训入库：**零堆 ≠ 零栈**——大定长数组实例并存需作用域分段。
2. **comprecover 两级持久化语义合并建模（勘正）**：初版把"合成器崩溃"与"断电"合并为五秒窗口管所有丢失 → 百次对练 3/40 违例暴露 → 回读 MD2 行 351 勘实：映射面**每变更即写**（合成器崩溃实时存活零丢失）+ 写盘才走五秒合并（断电最多丢五秒）→ `mutate` 即 `sync_mapped` 推进 seq + crash/power_loss 分开记账 → 模型勘正后 D-04 百次全过，勘误写入 MD2 篇 5 回写段。
3. **域数断言链七次踩坑**：quality.rs 四处硬编码（F493 仪表字符串 / f489 `lp.len()` ×2 / F489 CheckSet 条目）随每域注册 39→40→42→46 逐次同步；F489 CheckSet 条目里的 `lp.len()==39` 漏改致 f488/f489 二度红；f489 记账下限从"每域 25"修正为 `39*25 + 134`（七域 CheckSet 条数非 25/域）。
4. **各域实现级缺陷**（单测捉住）：vxwm ReplayLog 滚动判定（`slots[last].is_some()` 首帧即误计 → `len()==REPLAY_SLOTS`）、长度欺骗用例字节数（18 字节谎称 8）、测试 Vec 依赖违反零堆纪律；atlas 降档死循环（降档后 bytes 不重算）、LRU 驱逐先于降档次序、mut 缺失；pump 看门狗时序账（Hungry 被归并分支覆盖只活一瞬 → 恰达阈值进位一次；喂狗判定混 drained 总量 → `q_len==0 && drained==drained_before` 本圈无事件才计饥饿）；popup 矩形右下开区间边界断言、嵌套投递层级（界外子层/界内父层投父层）、release 级联摘除断言——全部修复锁定。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-505 CJK 字形源与 3500 常用汉字码点表——**唯一需新资产的硬缺口**，随队跟踪；pin 显式注册接口已留位，资产到位即接。
2. B-501 一级空闲实机口径（`idle_cost_pmil` 为宿主模型，实机 hlt/wfi 一级空闲待对账）随实机对账。
3. B-502 实机拖动实测（p95 分布为宿主近似，vxbench 实测随 WP-203 时序域）。
4. comprecover D-04 预算模型（20+25×N+15ms）为纸面分解，快照物理介质写路径随 WP-203 存储栈——schema 已冻结（RegEntry 20B + seq + 尾缀），接线即用。
5. 合盖/lid 硬件通道随 WP-201/202 输入域接线（承 WP-106 登记）。

**WP-201 最丑角落（m4 复盘用）**：atlas MAX_SLOTS=512 为栈红线缩容后的建模容量（8 倍缩），建模能力等价但图集容量语义降格——实机接线需恢复并改堆/静态大页；判据实装层与存量三套并行策略层（compositor 九族 225 项 / display / displaysrv）的收敛留 m2 重构；dragbench p95 宿主近似与 pump 空转 permille 模型均为"模型先行、实机校准"口径，验收绿不等于实机绿；comprecover 两级持久化的映射面"每变更即写"在实机上是 mmap 脏页回写语义，宿主模型用 memcpy 同步近似——介质真实时序随 WP-203。

## m1 闸门对账（阶段一出口仪式 · 宿主侧对账 2026-09-24）

**四件套核对**（MD3 行 78）：
1. **六包判据全绿**：✅ 判成。全量 ktest PASS=3299 FAIL=0（WP-101~106 全部判据 + WP-201 七域先行面背书；六包判据 B-101~106 / B-201~207 / B-301~307 / 篇 26 组 / B-2701~2702 / B-2901~2904 证据三件套各见本台账 WP-101~106 明细）。
2. **QEMU 断电百次（B-4102 提前用替身跑）**：✅ 判成。链路闭环已实证（WP-106 明细：panic 四环节 + 现场带跨复位持久 + 复位阶梯 + 重放消费单轮完整闭环）；百次收账 = **`grep -c "guard-band: last panic" build/panic-drill.log` = 100/100，DRILL-OK**（2026-09-24，单 QEMU 进程自动循环全程存活，零半轮零卡死）。
3. **交接快照 schema 冻结签署**：✅ 判成（WP-102 明细——全链第一闸，WP-22x/WP-203 等表已放行）。
4. **实机 m1 对账（任务台账 vs 本章包序）**：宿主侧 ✅——台账六包明细齐、包序与 MD3 附录 B 零漂移；**实机面 ⏳ 随实机窗口**。

**M1 全闸门判据过闸证据清单**（MD3 行 140）——宿主侧齐备项与实机硬项分列：
- 交接对练十次逐轮记录（快照哈希/耗时/异常）：⏳ 实机硬项，随实机窗口（VARIX → Windows → VARIX 循环）。
- 防自锁闸门注入测试二十组登记册：⏳ 实机硬项，随实机窗口。
- S106 二十轮实测验收记录（MD1 判据 1 形态）：⏳ 实机硬项，随实机窗口。
- 六包判据证据三件套齐备：✅ 判成（本台账六份明细）。

**对账结论**：宿主侧四件套 **4/4 判成**（2026-09-24 断电百次收账 100/100 DRILL-OK 后齐）；M1 全闸门判据的三个实机硬项（交接对练十次 / 防自锁二十组 / S106 二十轮）需实机窗口，**宿主可推进面不设阻**——阶段二按 MD3 时间线继续（WP-201 已收口 → WP-203 判据实装层已收口 → WP-208 → …），实机窗口开启时按上列清单逐项补账。

## WP-203 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：存储七判据（B-701~707）从 MD2 篇 7 文档口径落为**判据实装层**——七个新模块（fswl/fsyncp/pwrdrl/wmerge/ntfsro/linkloss/prefacct），判据号 `B-70x` 入 CheckSet 命名（127 处可 grep）。架构沿 WP-201 定案：不动存量策略层（storage F126-150 / m700vfs F201-225 / m700blk F226-250 / m700cache F251-275），新层引用存量范式（CheckSet/Lcg 同源），七域全宿主可测（零堆定长容量、整数运算、无 f32）。MD3 施工要点"ext4 特性白名单与 Google ext4 crate 的版本锁定同日落地"——**版本锁定 ext4_rs=1.3.3 已同日登记**（MD2 篇 7 回写段），crate 接线验证随包内接线面。

**交付面**（七文件新建 + 三文件注册 + 一文档回写 + 一版本锁定）：
- `fswl.rs`（B-701）：超级块三组旗标逐位过闸 + 白名单三档（基础集 missing 即拒 / 放行集含 MD2 明文三项 + 解析器支持项 / 拒绝集 = 白名单外一切）+ **VARIX 宪法语义严于 ext4 标准（ro_compat 未知也拒）** + 特性名表 39 位人话可 grep + 三要素文案常量。
- `fsyncp.rs`（B-702）：fsync 让路 + 三触发裁决纯函数 + **acked ⊆ flushed 恒等式审计闸**（未落盘拒 ack / 覆盖块进丢弃集拒 ack）+ 断电对练 200 轮零违例。
- `pwrdrl.rs`（B-703）：检查点账本（半写绝不入账）+ 重放收敛最近完整检查点（判例 16）+ 结构完整性校验（**承诺过的内容不许错**语义）+ 断电百次宿主面 100 轮零损坏 + 重放统计归档 + MD3 配比记账面（QEMU 80 + 实机 20）。
- `wmerge.rs`（B-704）：相邻块排序归并成段 + 吞吐模型整数运算（BOT 35MB/s 口径，MD1 19.1 硬约束来源列）+ 元数据集中写对练最差轮 ≥ 八成线 28000KB/s + 不合并基线对照（~15MB/s 四成极限）。
- `ntfsro.rs`（B-705）：挂载即只读定型 + 11 类写入口唯一汇聚点拒绝（EROFS=30）+ 环账本留痕不丢总量 + **无危险开关结构防线**（API 面无写模式）+ 穷举对练拒绝率 100%。
- `linkloss.rs`（B-706）：通道死亡双条件判定（连续失败 4 + 总线死，缺一不判死）+ 一秒广播预算 + 三要素文案 + **侥幸继续防线**（保护屏下一切 IO 拒绝，写调度单独记账）+ 无解除路径终态 + WD-053 对练 100 轮全过。
- `prefacct.rs`（B-707）：二次启动 ≤ 8s 预算模型（冷启动 14.9s 贴 15s 线锚定）+ **命中率下限结构推导**（解析解 6075bp / 整数边界 6050bp）+ 零命中退化冷启动 + 与 WP-105 prefetch.rs 命中率口径同源（B-2702 回补位）。
- `lib.rs`：七模块注册；`quality.rs`：七域入 run_full_loop（46→53 域）+ 断言链五处同步（F489 `lp.len()==53` / F493 仪表 / f489 测试体 len + 记账下限 `39*25+134+68`）；`robust.rs`：七域入 checkup。
- MD2 篇 7 判据实装回写段（七判据表格 + schema 冻结登记 + ext4_rs 版本锁定 + 勘误连带）。

**证据三件套**：全量 `cargo +1.97.1 ktest` **PASS=3341 FAIL=0**（lib 3334 + fuzz 1 + fuzz_parsers 6；53 域 CheckSet 全 PASS；较 WP-201 收口 3299 +42 = 七域新单测）；**CheckSet 68 项**（fswl 10 + fsyncp 12 + pwrdrl 9 + wmerge 10 + ntfsro 8 + linkloss 10 + prefacct 9）+ **单测 42 项**（5+7+6+6+6+6+6），七域定向全绿；判据号 B-70x 入命名 **128 处可 grep**（七模块 112 + 注册/联动 16）。复核日期 = 2026-09-24。

**红项处置**（对练与单测捉住的真实缺陷，修复并锁定回归）：
1. **pwrdrl verify 语义勘正**：初版把"账本外更晚写"（半写覆盖）误判结构损坏——百轮对练 74/100 假阳性暴露 → 回读 MD2 篇 7.2"最近五秒可能未落，**结构永不损坏**"勘正：损坏锚定"**承诺过的内容不许错**"（序号倒退/值错位），账本外写归重放回滚面 → 修后百轮 100/100。勘误连带写入 MD2 篇 7 回写段。
2. **wmerge 计量口径混用**：吞吐模型二进制 KB/s（÷1024）与标称带宽十进制 MB/s 混用 → 统一十进制口径（KB=1000B，与 19.1 的 35MB/s 同口径）→ 对练参数随之校准。教训：**性能模型先钉死计量口径再谈达标**。
3. **fsyncp 对练循环逻辑错位**：should_flush(is_sync=true) 先清 pending → 后取 covered 空集 → 对练零 ack → 重构动作循环（fsync 路径独立：covered 快照先取 → 落盘 → ack）。
4. **wmerge 窗口参数校准**：16 块窗 40 写最差轮 27463 < 28000 贴线跌破 → 窗口校准至 flex_bg inode 表真实量级（32 块/128 写）→ 最差轮 ≥ 28000。校准依据 ext4 真实集中性而非放松口径。
5. **prefacct 整数截断边界**：解析下限 6075bp 与整数世界边界（6050/6051bp，miss=158MB 破线）不一致——测试锚定整数世界真实边界而非解析解。
6. **域数断言链第五处漏网**：f489 测试体内 `lp.len()==46` 漏改（WP-201 教训说四处、实为五处）→ 全修并记录"断言链五处"新口径（F489 add / F493 add / f489 len / f489 记账下限 / F493 测试体）。
7. **no_std 两态编译兼容（ktest 挂 20 错而 host test --lib 全绿的假象）**：三模块裸用 `Vec`/`vec!`——`cargo test --lib` 只编 lib 单测（cfg(test) 生效 → std prelude 含 Vec）而 ktest 还要编 integration tests（**lib 以 not(test) 态编译 → no_std → 裸 Vec/vec! 解析失败**）。修复 = `use alloc::vec::Vec;` + `alloc::vec![...]` 路径宏（kvsrv/power_shutdown 存量范式）。教训入库：**"host 测试绿"≠"ktest 绿"——判据实装层必须过两态编译，新模块用堆类型一律走 alloc 显式路径**；零堆定长容量纪律的真正根源在此（no_std 态无 std prelude）。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-704 实机 vxbench 实测（吞吐模型宿主近似：T_OVERHEAD 150us / T_PER_BLOCK 117us 为模型值；19.2 vxbench 存储基准上线后按同口径回填）。
2. B-707 命中率实测回补（B-2702 联动位——WP-105 回写段明文"基线对账在存储栈就位后回补"；模型值 8500bp 漂移带，vxbench 实测替换）。
3. B-703 QEMU 替身断电八十次 + 实机二十次（MD3 配比）——宿主对练面 100 轮已绿；QEMU 存储替身（virtio/AHCI 注入延迟与错误）断电注入脚本随接线面，实机二十次随实机窗口。
4. ext4_rs=1.3.3 crate 接线验证（BlockDevice trait 对接块层 + no_std 编译验证 + 特性旗标 v1 白名单校准）随接线面。
5. VSCode 二次启动 ≤ 8s 的真实应用实测随 WP-208（软渲染三路径 + 应用生态就位后）。

**WP-203 最丑角落（m4 复盘用）**：fswl 白名单 v1 放行集基于 ext4_rs 1.3.3 支持面的纸面推断，真实卷特性矩阵校准随接线（校准记录回写 MD2）；pwrdrl 断电模型是"事务粒度半写"近似，真实块层断电面（页缓存脏块序、日志区半写）随 QEMU 替身注入实测——诚实语义（承诺内容不许错）不变但损坏检测面会变宽；wmerge 元数据窗口 32 块/128 写是集中性模型参数，BOT 开销 150us 是协议往返纸面值，vxbench 实测后两参数都可能翻案（翻案不改判据结构只改参数）；prefacct 预算模型假设"命中块近零代价"，真实预读 IO 与缺页竞争的时序面随实机；七域与存量四套存储策略层的收敛留 m2（同 WP-201 口径）。

## WP-208 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：图形多媒体与音频七判据（B-801~807）从 MD2 篇 8 文档口径落为**判据实装层**——七个新模块（path3/wingl/esoft/r3scan/viddec/hdadrv/mixer），判据号 `B-80x` 入 CheckSet 命名（56 项全带判据号，可 grep）。架构沿 WP-201/203 定案：不动存量合成器域（comp-* 十二域 / vxwm-* 六域 / gfx F101-125），新层引用存量范式（CheckSet/Lcg 同源），七域全宿主可测（零堆定长容量、整数运算、无 f32、**全部无 Vec——两态编译零风险面**）。

**交付面**（七文件新建 + 三文件注册 + 双文档回写）：
- `path3.rs`（B-801）：三层路径枚举（系统层/兼容层/Web 直插层，互不混线分账）+ 合成器提交面**唯一上屏入口**（`Composer::submit`，屏幕状态无旁路）+ `DrawAction::BypassWrite` 仅存审计分类（录屏诊断面识别绕过尝试，上屏通路不存在——C-1 结构防线）+ C-1 全路径对练（三层 × 随机帧流 × 分账/像素和双一致性）。
- `wingl.rs`（B-802）：GL 调用八类穷举分类（舒适区五类全承诺 / 重度 D3D 三类全不承诺）+ 三判例应用画像过闸（7-Zip/记事本/PotPlayer 界面全在舒适区）+ 星卡诚实标注文案常量（MD1 23.6 精神）+ GL 帧经表面提交（与 B-801 联动）。
- `esoft.rs`（B-803）：直插即纯软件定型（`InsertionProfile::direct` 唯一构造，**类型面无 enable_gpu 选项**——无危险开关防线 B-705 同族）+ 软合成确定性闭环（可见层过滤 + 像素和语义）+ 窗口面/输入面齐备与会话绑定 + 直插渲染对练 80 轮。
- `r3scan.rs`（B-804）：硬件清单固化（UHD 630=Mesa iris 标准对象高优 / NVIDIA 消费卡低优如实记录）+ `SubmitBackend` trait 双后端实例（CpuBackend 实际 + **GpuBackendStub 接口位**——编译通过即证"换后端不换管线"）+ 验收口径三指标（帧率 60fps/延迟 8ms/功耗 35W）+ 三件齐总闸（S406 出口形态）+ **只摸不建**（零驱动代码，MD1 20.1 立场）。
- `viddec.rs`（B-805）：两核上限吞吐模型（八核给解码两核，每核预算 1666kCycles/帧）+ H.264 HP@L4.0 / H.265 Main@L4.0 常见档实时（1650/1620 ≤ 1666）+ AV1 高档不在承诺面（3400 超预算）+ **解码帧类型必选表面提交路由**（构造入口唯一，C-1 生效）+ 同步容差 125ms 边界精确（SC-093）+ 兜底三要素文案 + 破损帧零投屏（不花屏语义）+ FFmpeg LGPL 动态链接登记锚。
- `hdadrv.rs`（B-806）：CORB/RIRB 命令环（满拒不覆盖/空拒/保序/seq 配对）+ DMA 位置上报（周期推进精确到格）+ Realtek 引脚配置表固化（双使能默认喇叭）+ **拔插周期边界切换**（切换点对齐 DMA 周期边界——旧周期排空后切，无爆音）+ 延迟对照测量（提交时间戳 vs DMA 位置，预算 40ms）+ 拔插对练 100 轮。
- `mixer.rs`（B-807）：每流环形缓冲（满拒/保序/环形复用）+ 独立音量/静音（互不串扰）+ **通话压媒体 duck 30% 不静音**（静音通话流失去压制权；系统音永不被 duck）+ 定长周期混音（64 样本与 B-806 DMA 同源）+ **i32 累加 + i16 饱和不绕回**（绕回即爆音）+ 多流对练 100 轮。
- `lib.rs`：七模块注册；`quality.rs`：七域入 run_full_loop（53→60 域）+ 断言链五处同步（F489 `lp.len()==60` / F493 仪表 / f489 测试体 len / 记账下限 `39*25+134+68+56` / F493 测试体）；`robust.rs`：七域入 checkup。
- MD2 篇 8 判据实装回写段（七判据表格 + 结构防线登记）。

**证据三件套**：全量 `cargo ktest` **PASS=3369 FAIL=0 EXIT=0**（lib 3362 + fuzz 1 + fuzz_parsers 6；60 域 CheckSet 全 PASS；较 WP-203 收口 3341 +28 = 七域新单测 4×7）；**CheckSet 56 项**（path3 8 + wingl 7 + esoft 7 + r3scan 6 + viddec 9 + hdadrv 10 + mixer 9）+ **单测 28 项**（七域各 4），七域定向全绿；60 域 CheckSet 全 PASS 可 grep。复核日期 = 2026-09-24。

**红项处置**（对练与 CheckSet 捉住的真实缺陷，修复并锁定回归）：
1. **mixer CheckSet 环形复用断言写反**："流环形缓冲语义"项断言 `!push_sample(1)`（pop 释放一格后 push 应成功而非失败——环形复用语义理解错位）。**单测 4 项全绿而 CheckSet 项挂（seed 0xB807 下）**——单测与 CheckSet 断言路径不同，两者绿不等价。教训入库：**"单测绿"≠"CheckSet 绿"，CheckSet 是闭环对账面必须独立过**。
2. **path3 对练一致标志 Default 语义**：`ledger_consistent`/`screen_consistent` 依赖 `#[derive(Default)]` 初值 false——"无问题是常态"的对练语义必须显式置 true（bool Default=false 是 Rust 语义，不是对练语义）。
3. **hdadrv 对练随机同态失能**：8 次随机插拔全为同态时 `jack_events==0` 误判"对练失能"（概率 0.78%/轮，seed 11/60 轮命中）→ 修为首轮强制真实插入（初始喇叭路由，插入必切换）——判据有效性与随机性解耦。
4. **断言链联动面第六处**：`f500 run_final_check()` 期望 ReadyWithPending 实际 Blocked——**七域 CheckSet 失败传导到 final verdict**（域状态变化沿裁决链传播，m500 族测试也是"改域必查"面）。域数断言链五处之外，final verdict 与 f475 checkup 两处联动面一并入"改域必查清单"（第七处口径）。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-806 HDA 实机面：Realtek 编解码器真值配置表、jack sense 中断实机通道、DMA 位置寄存器对齐位——随实机窗口（宿主为语义模型环）。
2. B-805 FFmpeg LGPL 动态链接 + 独立进程边界的 ADR 登记册补条随接线面（本包登记锚定）；每核周期需求是标定锚定模型值，实机 vxbench 回填。
3. B-802/803 Wine GL 路径与 Electron 直插的实机验证随应用生态就位（判例应用画像是宿主审计模型，真实调用画像随星卡流水线采集）。
4. B-804 立项包 S406 出口随 R3 立项窗口（本包三件宿主面已齐，GPU 后端验收口径实机校准随 R3）。

**WP-208 最丑角落（m4 复盘用）**：path3 的 BypassWrite 是 DrawAction 枚举成员而非独立类型——审计面能"看见"绕过但类型上与合法动作同族，更强的形态是提交面的类型隔离（submit 只收 LegalFrame），随 m2 与合成器域收敛；hdadrv 延迟换算 48 samples/ms 锚定 48kHz，实机多采样率面（44.1k/96k）随驱动接线；mixer duck 30% 是"压低"语义的定值化，体验面（duck 曲线/恢复延迟/多级压制）随 UI 域；viddec 每核周期需求 1650/1620/3400 是标定锚，真实 FFmpeg 软解占用随实机回填（翻案不改判据结构只改参数）；七域与存量 comp-*/gfx 域的收敛留 m2（同 WP-201/203 口径）。

## WP-204 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：网络栈七判据（B-601~607）从 MD2 篇 6 文档口径落为**判据实装层**——七个新模块（netthr/fdmix/lstnauth/dohsw/diag3/offln/usbnet），判据号 `B-60x` 入 CheckSet 命名（57 项全带判据号，可 grep）。架构沿 WP-201/203/208 定案：不动存量网络策略层，新层引用存量范式（CheckSet/Lcg 同源），七域全宿主可测（零堆定长容量、整数运算、无 f32、全部无 Vec——两态编译零风险面）。单测前缀 f701~707 与判据号 B-601~607 一一对应（避撞验证：grep 确认 m7docrel.rs 只占 f700）。

**交付面**（七文件新建 + 三文件注册 + 双文档回写）：
- `netthr.rs`（B-601）：700Mbps 达线整数预算模型（MTU 1500 → 58333fps → 17143ns/帧，**向上取整数学界**：fps×budget 落在 [1s, 1s+fps)）+ 接缝开销成本阶梯（零拷贝 9800 / 单拷贝 14200 / 双拷贝 19600 破线——BOT 教训"拷贝一层丢一层"的模型实证）+ FrameRing 背压显式（环满丢弃计数进监视器，delivered+dropped=总帧数对账恒等式）+ 驱动矩阵第一批固化（RTL8168 + RNDIS/NCM 手机共享同类）。
- `fdmix.rs`（B-602）：统一描述符表（File/Socket 共用 fd 空间，FD_CAP=16）+ POSIX 最小可用 fd 分配 + close 释放复用与双次 close EBADF + poll 聚合跨类型 + **边缘触发只报一次**（edge_reported 沿标记 + clear_ready 沿复位——离开又回来是新沿）+ 泄漏对账恒等式（alloc−close=live，**轮内对账口径**）+ 表满 EMFILE 不越界不覆盖。
- `lstnauth.rs`（B-603）：**默认仅出站**（出站不设门、监听必须声明）+ 权限清单（app×port 二元组精确匹配，重复声明幂等只记一条）+ 未声明拒绝（NotDeclared）+ 拒绝留痕（诊断事件 seq 保序可读）+ 事件环满不覆盖保序（最早留痕不被冲掉，与 B-601 背压同纪律）+ 清单满不越界。
- `dohsw.rs`（B-604）：默认系统 DNS + **切换即时生效 = 模式立翻 + 缓存全清**（旧模式答案不残留，切换后第一次解析必走新视图）+ 双视图固化表（数据锚定本仓库 push 战役实测值：系统视图 github 域出污染地址 0x14CDF3A6=20.205.243.166，DoH 视图出可用地址 0x8C527204=140.82.114.4——污染域是"切换 DoH"下一步文案的现实依据）+ 解析失败三要素文案（NEXT 含"请检查网络或切换 DoH"）+ 缓存命中语义。
- `diag3.rs`（B-605）：三件套枚举（vx-ping/vx-route/vx-capture，MD1 17.4 取证基础）+ **raw 权限收敛在系统工具**（应用身份走诊断通道必拒，拒绝不消耗环槽）+ **单一报告数据源双格式渲染**（JSON 行字段序列 / 人读字段序列，逐字段一致对账——内容零漂移）+ 取证可复现（同 seed 同报告）+ 判例挂钩常量（SC-041 Steam 登录 / SC-005 git 推拉，网络判例失败归因第一步）。
- `offln.rs`（B-606）：单一网络状态源 + **三呈现面派生一致**（桌面标注"离线" / 星图更新按钮变灰带原因 / 直插应用三要素报错）+ **无黑箱防线**（离线态任何网络操作即时返回三要素，操作延迟恒 0——"转圈十分钟后超时"在模型面不可能）+ 弱网纪律（**先测量后调参**：auto_tune 默认关、不带测量记录不批调参——不凭感觉动旋钮）。
- `usbnet.rs`（B-607）：RNDIS/NCM 两类通道（与 netthr::DRIVER_BATCH1 手机共享两条同源对账）+ 插拔生命周期（attach 绑定即数据面就绪 / detach 清理无幽灵通道 / 槽满不越界）+ **选择语义 NCM 优先**（USB 标准协议优先于微软旧协议）+ **生命线语义**（WiFi 空窗期无线未就绪时 USB 共享必须顶上——第一批驱动的定位）+ **实机偏差登记**（三条随队跟踪：RNDIS/NCM 实机插拔验证、手机共享实测吞吐——HostModelDone+DeviationRegistered）。
- `lib.rs`：七模块注册；`quality.rs`：七域入 run_full_loop（**60→67 域**）+ 断言链五处同步（F489 `lp.len()==67` / F493 仪表 / f489 测试体 len / 记账下限 `39*25+134+68+56+57` / F493 测试体）+ **MAX_LOOP 扩容 64→72**（WP-208 后 60 域近满，本包七域破 64 上限——见红项 6）；`robust.rs`：七域入 checkup（f475 联动面）。
- MD2 篇 6 判据实装回写段（七判据表格 + 结构防线登记）。

**证据三件套**：全量 `cargo test --lib -p varix` **PASS=3390 FAIL=0 EXIT=0**（较 WP-208 收口 lib 3362 +28 = 七域新单测 4×7）；**CheckSet 57 项**（netthr 8 + fdmix 9 + lstnauth 8 + dohsw 8 + diag3 8 + offln 8 + usbnet 8）+ **单测 28 项**（七域各 4），f70x 定向 30 全绿；67 域 CheckSet 全 PASS 可 grep。复核日期 = 2026-09-24。

**红项处置**（对练、单测与全量 ktest 捉住的真实缺陷，修复并锁定回归）：
1. **netthr 预算断言数学错误**：`fps × FRAME_BUDGET_NS` 断言 `== 0`——预算 17143 是 1e9/58333=17142.8 的**向上取整**，乘积 1,000,002,619 必然略超 1s（这是取整的正确行为），误以为略小于 1e9。修复为区间断言锁向上取整数学界 `[1s, 1s+fps)`。教训：**取整方向决定乘积在 1s 的哪一侧**，写断言前先手算数值。
2. **fdmix 泄漏对账跨轮累计错位**：对账用 `sum.allocs - sum.closes`（跨轮累计）对 `t.live`（本轮存活）——第 1 轮能对上、第 2 轮起必然错位。修复为轮内局部计数对账（round_allocs/round_closes），sum 保留累计口径。教训：**对账口径必须与状态生命周期同界**（t 每轮重建，对账量也必须每轮清零）。
3. **diag3 诊断通道环满拒收取证**：REPORT_CAP=16 < 对练 30 轮，第 17 轮起系统工具的正常取证也被拒（环满 Err 传导为 auth_correct=false）。修复为环形滚动记录——报告本就**即时返回调用方**（非排队缓冲），取数记录只留最近 16 条，取证永不因记录环满而失败。教训：**区分"数据面"与"记录面"的容量语义**，即时返回的服务不应被历史记录的容量卡死。
4. **lstnauth 幂等断言方向写反**：单测断言重复声明返回 false，而 declare 语义是幂等操作（重复声明仍成功返回 true、只记一条）——CheckSet 与单测断言方向相反。修复单测对齐幂等语义。教训：**幂等操作的返回值语义要先定**（"已存在但成功" vs "重复即拒绝"）。
5. **usbnet 对账清理漏计 + 自相矛盾断言**：末尾清理拔出计入 mgr.detaches 但未计入 sum.detaches（对账恒等式错位，与红项 2 同根）；且同测试内 `assert!(attaches > detaches)` 与 `assert_eq!(attaches - detaches, 0)` 自相矛盾（末态全拔空即 attach==detach）。修复清理阶段计入对账 + 删除矛盾断言。
6. **MAX_LOOP 容量截断（联动面第八处口径）**：FullLoop 容量 64 < 67 域——七域 register 静默截断末 3 域，`lp.len()==64≠67` 传导为 f489/f493 两个测试与 F489/F493 两个 CheckSet 项共四处失败。扩容 64→72（给 WP-202/205~209 留余量）。教训：**域数断言链之上还有容器容量面**——"改域必查清单"第八处：注册域数变化时必查聚合器容量（MAX_LOOP / MAX_DOMAINS），静默截断是黑箱（register 返回 ()，截断无告警，只有 len 断言能暴露）。
7. **单测前缀对位**：dohsw/diag3/offln 初写时前缀错位一档（f705/f706/f707 → 应为 f704/f705/f706）——统一 B-60x ↔ f70x 一一对应后 sed 批量修正。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-607 RNDIS/NCM 实机面：真实手机插拔、真实 USB 控制器、共享上网实测吞吐——随实机窗口（宿主为语义模型环，usbnet::DEVIATIONS 三条登记在案）。
2. B-601 有线吞吐实测：700Mbps 达线的 vxbench 实测（iperf3 或对等工具）随实机窗口——宿主为整数预算模型，开销常量 9800/14200/19600 是标定锚，实测回填（翻案不改判据结构只改参数）。
3. B-604 DNS 真实解析面：hickory-dns 接线、真实 DoH 服务器（RFC 8484）往返、污染域实测清单——宿主双视图为固化表模型，判例复测时以实测污染名单回填。
4. B-605 pcap 落盘与通用工具互通（Wireshark 打开验证）随桌面工具域接线面。

**WP-204 最丑角落（m4 复盘用）**：dohsw 的"切换清缓存"是保守正确形态（更细的 TTL 分域缓存随 hickory-dns 接线）；diag3 双格式是字段序列对账模型，真实 JSON 行渲染器（转义/浮点/嵌套）随桌面工具域；offln 弱网"先测量后调参"是纪律模型，smoltcp 真实参数（重传/窗口）调优随判例复测——本包锁的是"不凭感觉动旋钮"；lstnauth 的 DenyLog 是域内独立事件环，与存量诊断/journal 域的收敛留 m2（同 WP-201/203/208 口径）；usbnet 通道选择"NCM 优先"是纸面优先级，实机双插场景的实际切换体验随用户判例。

## WP-202 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：输入与输入法七判据（B-901~907）从 MD2 篇 9 文档口径落为**判据实装层**——七个新模块（evflow/kblayout/imepinyin/composesw/hkbind/candwin/focring），判据号 `B-90x` 入 CheckSet 命名（57 项全带判据号，可 grep），单测前缀 f801~807 与判据号一一对应（避撞验证：grep 确认 f80x 无占用——bootnext.rs 的"f80"命中只是注释十六进制地址）。架构沿 WP-201/203/208/204 定案：不动存量输入域，零堆定长容量、整数运算、无 f32、全部无 Vec、无 alloc 调用（imepinyin 初写误用 format! 当场修正为字节级拼接）。

**交付面**（七文件新建 + 三文件注册 + 双文档回写）：
- `evflow.rs`（B-901）：三源归一（PS/2 / USB HID / 平台键 → 统一事件结构：时间戳/设备标识/事件类型/**物理键位**——布局无关主键）+ **注入接口类型面不存在**（InputSubsystem 公开 API 只有 dispatch(InputEvent) 与 register_filter——无 target_window 参数可填，C-8 结构防线 B-801 submit 族同款）+ 过滤器两态（消费/放行，trait 面无改写入口）+ 事件计数对账（dispatched+consumed=总数）。
- `kblayout.rs`（B-902）：布局表五字段（层定义/死键/修饰锁声明/别名/版本号）+ 四层（常态/Shift 实映射 + AltGr/Ctrl 弃用声明）+ schema 合规校验器（版本非零/别名可查/常态层非空逐项验）+ 两套内置布局固化（美式英文/简中拼音——拼音带声调死键预留 āáǎà 锚）+ 切换即时生效（会话指针立翻）+ **计宽职责分离**（类型面无计宽字段——判例 21 分域）。
- `imepinyin.rs`（B-903）：音节切分器（**最长韵母优先**——zhong 整体成音节，固化音节表）+ 冻结词库三元组（词/拼音序列/词频档位，完全匹配 + 档位稳定排序插入序，模糊音关闭）+ 三段式分页模型（每页九个，页内序稳定）+ **三硬线整数预算推导**（内存 30000×32B=937KB≪32MB；首屏 worst case 0.9ms≪100ms WD-011；上屏 4ms 模型锚≤16ms）+ **不做运行期学习**（DICT const 冻结——类型面无 learn 入口，诚实标注的结构防线）。
- `composesw.rs`（B-904）：IME 组合缓冲（空/非空组合态判定源）+ **仲裁入口直接分流**（gate 签名只收组合态不收键值——任何键值组合期都到不了快捷键匹配）+ 组合期零误触**穷举验证**（256 键位全进引擎）+ 缓冲层编辑（退格/上屏清空——永不污染应用窗口）+ 生命周期闭环（键入组合→路由→上屏→恢复仲裁）。
- `hkbind.rs`（B-905）：三列词典（保留字四条固化：切换输入法/截图/交接入口/亮度音量 + 应用申请 + 用户自定义）+ **仲裁顺序 用户>保留>应用** + 冲突**先注册先得拒绝并指名**（Err 带占用者名——判例 26）+ 保留字不可被用户覆盖（系统级功能的宪法位）+ 词典唯一对账（同一 (scancode, mods) 生效绑定恰一条）。
- `candwin.rs`（B-906）：候选窗定位（popup_grab + **屏幕夹紧**：下方越界翻上方 flipped 可读、四边夹紧回屏、极矮屏兜底）+ **Wine 桥与原生同路径**（place 不看锚点来源——协议同一路径的结构面，判例 11 实现闭环）+ 定位确定性（同锚点同尺寸同位置）+ 恒不越屏谓词（in_screen 随机对账）。
- `focring.rs`（B-907）：控件基类**焦点环类型面恒开**（focus_ring 私有 + 构造唯一 + 审计读取面恒 true——不存在任何置 false 路径，"应用想丢焦点环都难"的物理强制，B-803 同族防线）+ Tab 序环形遍历（声明序 + 尾后回首 + Shift+Tab 首前回尾，一圈恰遍历全部控件一次）+ 焦点唯一（合成器裁决同源）+ 审计零例外谓词（全控件枚举全 true）。
- `lib.rs`：七模块注册；`quality.rs`：七域入 run_full_loop（**67→74 域**）+ 断言链五处同步（F489 `lp.len()==74` / F493 仪表 / f489 测试体 len / 记账下限 `+57` / F493 测试体）+ **MAX_LOOP 教训前置：72→80 扩容先行**（WP-204 红项 6 的第八处口径直接前置应用——注册前先算容量）；`robust.rs`：七域入 checkup。
- MD2 篇 9 判据实装回写段（七判据表格 + 结构防线登记）。

**证据三件套**：全量 `cargo test --lib -p varix` **PASS=3418 FAIL=0 EXIT=0**（较 WP-204 收口 3390 +28 = 七域新单测 4×7）；**CheckSet 57 项**（evflow 8 + kblayout 8 + imepinyin 9 + composesw 8 + hkbind 8 + candwin 8 + focring 8）+ **单测 28 项**（七域各 4），f80x 定向 28 全绿；74 域 CheckSet 全 PASS 可 grep。复核日期 = 2026-09-24。

**红项处置**（编译器、定向测试与全量 ktest 捉住的真实缺陷，修复并锁定回归）：
1. **focring 文档注释层级错**：模块头 `///` 与 `//!` 混用（E0753 expected outer doc comment）——inner doc 只能出现在条目之前。修为纯 `//!` 连续块。教训：**模块头注释第一行定层级，中途不可切换**。
2. **hkbind 双层 Option**：`.find(...).copied()` 得 `Option<Option<Binding>>`（迭代器元素本身是 &Option）——补 `.flatten()`。教训：**Option 槽数组的 find 是双层结构**，与 fdmix/usbnet 的 `slots.iter().find()` 同款形态，抽公共心智模型。
3. **focring 两处类型错**：u64 % usize（Lcg.next() 是 u64）与 usize→u16 实参——as 转换补齐。教训：**Lcg 侧恒 u64，容量常量侧恒 usize，取模前显式转**。
4. **imepinyin format! 违反零堆纪律**：对练拼接输入串用了 `format!`（需 alloc）——**WP-208"全部无 Vec"教训的邻域扩展（无 alloc 调用）**，当场重构为字节级拼接（split_bytes + 定长 buf copy_from_slice）。教训：**no_std 无 alloc 纪律 = 无 String/无 format!/无 Vec 三位一体**，写对练前先选好拼接形态。
5. **composesw 引擎消费闭环序列死锁（CheckSet 第 6 项，单测全绿下挂）**：闭环序列以 `engine_consume(33)` 开头——但空缓冲时 `arbiter_gate(false)` 返回 Hotkey，字母根本没进缓冲，"键入→组合→上屏→恢复仲裁"的第一步就是死锁。修复：首字母直接 `push_letter`（IME 激活本职），`engine_consume` 从组合期起步验证路由。教训：**闭环序列的第一步要检查前置态**（进入组合期需要先有键入，路由函数不负责键入）。
6. **composesw 对练断言时态错位**：路由断言检查 `engine_consume` **修改后**的组合态——组合期退格清空缓冲后 `composing()` 变 false，正确的 Ime 路由被误判为违规。修复为**进入时态**（was_composing）断言——路由裁决基于进入时组合态，操作后果不回溯判责。教训：**裁决面与操作后果分离——断言路由正确性用裁决时刻的输入态**。
7. **candwin 极端小屏越界（CheckSet 第 6 项）**：200×100 屏幕 < 候选窗 180×216，兜底夹紧算出负坐标——物理装不下属不可能情形，混进了判据面。修复：判据语义收窄为"屏幕装得下候选窗时不越屏"（MD2 原文"窗口内不越屏"是定位跟随语义），CheckSet 改用最小可装屏（400×300）锁双兜底（水平夹紧 + 下方翻面）。教训：**物理不可能情形不是防线弱点——判据面与物理前提分离**。
8. **"单测绿≠CheckSet 绿"第三次重演（WP-208 红项 1 同款）**：B-904/B-906 各 1 项 CheckSet 挂（7/8）而 f80x 单测 28 全绿——单测 seed（7/13，40 轮）与 CheckSet seed（0xB904/0xB906，80 轮）路径不同，CheckSet 闭环对账面必须独立过。诊断手段入库：**diag 测试直接调用 run_*_checks() 逐项 assert 并打印项名**（f475 render 只有计数没有项名，逐项定位要靠 CheckSet 遍历）。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-903 三硬线**实测**达标（100ms/16ms/32MB）随实机窗口——宿主为整数预算推导（30000 条 × 标定锚常数），真实词库编译成前缀索引后 vxbench 回填（翻案不改判据结构只改参数）。
2. B-904 组合期零误触**真人判例**（施工要点：真人输入法过、模拟键测不出 IME 组合态）随判例集实机面。
3. B-906 判例 11 的 WD 侧实测（Wine 窗口光标锚点经 Wine 桥上报的真实协议面）随 Wine 支架接线。
4. B-902 布局表 JSON 分发形态（随 VARIXSYS 分发、用户可加布局）随部署域接线——宿主为固化结构面，schema 校验器已是分发侧验收工具。

**WP-202 最丑角落（m4 复盘用）**：imepinyin 音节表 16 条是模型面（真实声韵母表 + 三万词库 + 前缀索引随词库编译管线）；简拼（首字母为辅）在宿主面未建模（切分器 break 即止——简拼路由随引擎接线面）；hkbind 保留字四条用模型键位（真实键位表随交互词典冻结）；candwin 候选窗尺寸 180×216 是九候选页的估算面（真实渲染尺寸随控件域）；focring 的 Tab 序只含线性环形（首尾跳转/分组 Tab 随控件域 SDK 化）；七域与存量 hidsrv/vxwm-* 输入域的收敛留 m2（同 WP-201/203/208/204 口径）。

## WP-205 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：应用件三包（MD3 行 92）15 判据（B-1601~1604 / B-1701~1704 / B-1801~1803 / B-1901~1904）从 MD2 篇 16-19 落为**判据实装层**——八个新模块（termproc/termfeed/trashbin/fsview/thumbsched/edcore/widgetline/settable），单测前缀 f901~f908 与判据组对应（避撞验证：grep 确认 f9xx 零占用）。三包共用控件基类（SDK 早期形态）由 WP-202 focring ControlBase 承载、WP-303 正式化，本包锁应用件自身判据。**数据红线双落点**：trashbin（B-1701 回收站零真删——SDK 删除面类型上无"直接删除"变体）与 edcore（B-1801 原子保存——renamed 单步翻转无中间可见态 + 两步失败注入原文件恒无损）。

**交付面**（八文件新建 + 三文件注册 + 双文档回写）：
- `termproc.rs`（B-1601/1602）：网格模型（属性位含宽字符主格/续格——判例 21 两格计宽从模型层正确）+ ANSI/VT 状态机（SGR 十六色/二百五十六色/二十四位真彩归一 xterm 256 立方、光标移动、清屏、模式 h/l、OSC 标题）+ 非法序列忽略并计数（零网格副作用）+ 吞吐预算模型（1e6 ops × 1000ns ≤ 1s；对练 600 轮产生 230 万网格操作）+ OpRing 输出/绘制解耦（每帧 drain ≤64 恒上限 → 帧耗时上限恒定 → UI 不卡 + produced==delivered+dropped 背压对账）+ ScrollRing 一万行环形（count+dropped==pushed + 最近淘汰序号对账——缓冲满滚动淘汰网格内存恒定）。
- `termfeed.rs`（B-1603/1604）：回显五段整数预算（500+1000+2000+2000+4000 = 9500ns ≪ 16ms 且倍余量）+ EchoStage 环节表穷举零阻塞（新增环节必须进表——结构防线）+ 分段之和恒等于总预算（口径一致无隐性加项）+ CommandRoute 双表（原生 vx-* 六命令先查 / Linux 直插 ls/cat/grep/tar/python/git 经柜台无感混用 / 未知诚实 None）+ SessionTable 槽生命周期（open/close 对账 + PTY 随会话回收 pty_alive==live + 先关槽可复用 + 全满诚实拒绝）+ 快照诚实面（live>0 必列未结束会话条目——"终端会话无法跨域存活"是物理不是缺陷）。
- `trashbin.rs`（B-1701 数据红线）：**DeleteApi 类型面仅 Trash/PurgeConfirmed 两变体**（无 Direct/Unlink 变体可选——新增删除入口必须进 SDK_DELETE_APIS 穷举表，ntfsro"API 面上不存在写模式"同族防线）+ 条目三字段齐（原路径/删除时间/原文件引用）+ 还原语义（条目出环 + 原路径返回——"可反悔"的兑现面）+ 永久删除二次确认硬门（无令牌拒绝并计数 + 不认账令牌同样拒绝 + 执行留通知）+ 配额 10% 清最旧（Q45：240→100 清 7 条整数对账）+ **一动作一汇总通知**（enforce_quota 批量清理汇总一条 PurgeNotice{count,bytes}——记录面容量与清理规模解耦）+ 跨盘移动=复制+回收站删除（无直接删路径）+ 环满自愈（配额清理优先，未超则清最旧一条腾位——删除不因满而失败）+ 条目守恒对账（put_total == count + restore_total + purge_total）。
- `fsview.rs`（B-1702/1703）：MountView 三呈现面一致（角标"来自 Windows 域（只读）"/写类操作置灰/禁用带一行解释）+ **呈现层与 ntfsro 强制层分层**（B-705 管 VFS 写句柄 100% 拒绝；B-1702 管文件管理器呈现——呈现禁用的操作调下去也被强制层拒绝，双保险语义单一）+ FileAction 七操作穷举矩阵（读 3 允/写 4 禁）+ 写尝试拒绝留痕（attempts == denied + allowed）+ ext4 对照七操作全通 + 即席过滤毫秒级（64 条 × 15ns = 960ns ≤ 1ms 整数推导 + 子串命中对账 + 空模式全显）+ FullSearch 终态机（Running→Cancelled/Done + 任意步可取消 + 终态拒绝二次操作 + open_handles 归零资源回收 + 流式批次呈现首批先答）。
- `thumbsched.rs`（B-1704）：ThumbCache LRU（容量恒 128 + 触碰刷新 + 满淘汰最旧 + evictions 对账）+ 按需生成零预扫（初始零生成/访问驱动）+ GenJob 进度单调（peak 对账 + 100% 才置 out_ready）+ 任意进度可取消且**取消无半成品**（out_ready 恒 false + 取消后 step 拒绝诈尸——落盘许可只在完成时存在）+ GenLanes 两槽节流（第三任务排队 None）+ 异步不卡 UI 预算（2ms/步 × 8 步 ≤ 16ms 一帧）。
- `edcore.rs`（B-1801 数据红线/1802）：ViewWindow 视口映射（map_len ≤ 4096 恒定内存 + jump 换映射 + 尾窗截断/越界钳制——百兆秒开与恒定内存并存）+ 百兆秒开预算（100MB × 9ms/MB = 900ms ≤ 1s）+ AtomicSave 原子保存两步面（write_tmp→rename，**renamed 单步翻转即磁盘上要么旧内容要么新内容**；任一步失败注入 renamed 恒 false + orig_intact 恒真——保存失败原文件无损；未写 tmp 直接 rename 拒绝）+ UndoChain 词组级（连续字母合并单步——"不是每键一步也不是一键回底" + 空格硬边界成步）+ 链深 500 环形淘汰（dropped 对账）+ undo/redo 步数守恒 + close 全清（链内存随窗口释放）+ Eol 检测保留（LF/CRLF roundtrip 不改写用户文件——编辑器不自作主张）。
- `widgetline.rs`（B-1803）：四小件清单表（shot/img/calc/clock 进册——新增小件必须进表）+ 公共线四指标全件过线（冷启动 ≤1s / 内存 ≤64MB / 快捷键入词典 / 主题全适配）+ four_widget_sweep 单函数遍历全绿（"一个脚本测四件"的验收自动化本体）+ shot_path 确定性（/shots/<日期>/shot_<ts>.png 同输入恒同输出——"截完在哪"永远可答）+ rotate_meta 元数据级旋转（orientation 模 4 单字段翻转 + 像素锚/长度不变——原图无损不重编码）。
- `settable.rs`（B-1901~1904）：SettingRow 七字段 schema（默认值在域内 / Bool 域恒 {0,1} / Enum 域非空 / 空键名拒）+ **新服务零前端**（add_row → render_label 自动生成 "service.key"——加一项设置不是改界面是加一行表）+ 读表执法同源（schema 不过的行进不了总表——表即执法依据）+ SetFlow 校验先于生效（validate 不触状态 → commit 先 validate；越域拒绝 current 不动——"取消永远是安全出路"在设置层是回滚机制不是文案）+ 三要素报错（what/why/how 全非空）+ 生效三档表内声明（Immediate/Session/Reboot 枚举在 Row 内）+ hint 精确匹配全表走查（"重启后生效"前置告知——commit 前可读）+ NotifyBus 浮层三语义（免打扰入队不弹 + flush 按序补弹队列清零对账 + 同主题聚合合并）+ ConfirmModal 模态不可绕过（confirm/deny 二选一无第三态旁路——类型面无 bypass 方法）+ 安全类无"不再询问"（remember_choice 恒 false；便利类允许）。
- `lib.rs`：八模块注册；`quality.rs`：八域入 run_full_loop（**74→82 域**）+ 断言链五处同步（F489 `lp.len()==82` / F493 仪表 / f489 测试体 len / 记账下限 `+73` / F493 测试体）+ **MAX_LOOP 前置扩容 80→88**（第八处口径连续第三包前置应用）；`robust.rs`：八域入 checkup + **f475 渲染 buf 2048→8192**（82 域渲染超 2048 截断尾部 FAIL 行——诊断盲区修复）。
- MD2 篇 16-19 判据实装回写段（八域对照表 + 结构防线登记 + 勘误连带）。

**证据三件套**：全量 `cargo ktest` **PASS=3450 FAIL=0 EXIT=0**（较 WP-202 收口 3418 +32 = 八域新单测 4×8；lib 3450 全绿 + 集成 1 + 6 全绿）；**CheckSet 73 项**（termproc 8 + termfeed 8 + trashbin 9 + fsview 10 + thumbsched 8 + edcore 10 + widgetline 8 + settable 12）+ **单测 32 项**（八域各 4），f9xx 定向 32 全绿；82 域 CheckSet 全 PASS（MAX_LOOP 88 无截断）。复核日期 = 2026-09-24。

**红项处置**（编译器、定向测试与全量 ktest 捉住的真实缺陷，修复并锁定回归）：
1. **edcore 环形缓冲"最后写入位"索引 bug（一处定义两处复用同错）**：top() 与合并分支都写成 `(head + CAP - 1)` 漏加 len——非满环时读到错误槽位（旧数据/None），词组合并从未命中（"ab" 两键入两步而非一步），合并写入落在 head-1 空槽而真栈顶未变（len 断言过而内容断言挂的隐蔽形态）。修正为 `(head + len + CAP - 1) % CAP`（两处同源）。教训：**环形缓冲"最后写入位" = (head + len - 1) mod CAP——head 单独决定不了栈顶，len 是公式必要项；同一公式在读取侧与写入侧必须同源**。
2. **trashbin 通知容量语义（记录面与动作粒度错位）**：NOTICE_CAP=8 被批量清理逐条塞满（第 17 次 put 环满自动清理 12 条 → notice_count 卡满 8），后续显式 enforce 的通知静默丢弃，notice_count(8) != rounds(4) 对账挂。修复为**一动作一汇总通知**（enforce_quota 批量清理汇总一条 PurgeNotice{count,bytes}——count/bytes 如实且容量与清理规模解耦）。教训：**diag3"数据面与记录面容量语义分离"的邻域推广——记录面的粒度语义要与动作粒度对齐（一动作一记录，不是一条目一记录）**。
3. **trashbin put 满自愈缺口**：环满时只依赖配额清理腾位——配额未超（大 disk 场景）时 enforce 清 0 条，find_free 仍 None，put 返回 None（违反"删除永远进回收站"）。修复：配额清理后仍满则清最旧一条腾位（带通知）。教训：**自愈路径要枚举"清理量为零"的分支——判据语义（删除不因满而失败）优先于实现捷径**。
4. **termproc 非法序列断言笔误**：`grid_ops == ops0 + 1` 应为 `== ops0`——非法序列按"忽略"语义零网格副作用。教训：**"忽略并计数"的断言要写全两个面：illegal 计数上去了 + 网格没动**。
5. **termproc 滚动环断言语义混淆**：`last_evicted_seq == 1`（首次淘汰序号）与字段实际语义（**最近**淘汰序号 = pushed - CAP）错位。修正为 `== sr.pushed - SCROLL_CAP`。教训：**字段名带"last"时断言前先确认是"最近一次"还是"第一次"**。
6. **edcore 链深对练自相矛盾**：连续 `push_key(b'x')` × 507 全部合并进同一个词组步（词组级语义的本体！），len 恒 1 永远到不了链深 500——对练设计与被测语义直接冲突。修复：交替键入（字母/空格）每键独立成步。教训：**对练序列要先过一遍被测语义的合并/去重规则——词组级撤销链的链深对练必须制造步边界**。
7. **settable unwrap_or 类型错（编译 4 错同根因）**：`f.validate(...).unwrap_or(ERR_EMPTY)`——`Result<(), SetError>` 的 Ok 变体是 `()`，unwrap_or 参数须匹配 Ok 侧（E0308）+ 连带 E0609 ×3。修复为 match 提取 Err。教训：**Result 的 Err 提取用 match/unwrap_err，unwrap_or 是给 Ok 侧兜底的**。
8. **"单测绿≠CheckSet 绿"第四次重演**：termproc 2 项 + edcore 1 项 CheckSet 挂而 f9xx 单测 32 全绿——CheckSet 对练面（600 轮大闭环 / 链深 500 步 / 20 次配额循环）超出单测单点覆盖。**诊断手段升级入库：f475 渲染 buf 扩容 8192**——2048 字节对 82 域渲染截断尾部 FAIL 行，诊断 grep "FAIL" 零匹配造成"checkup 已全过"误判；**渲染截断是诊断盲区，先查 buf 容量再下结论**。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-1601 百万级吞吐**实测**（真实墙钟计时）随实机窗口——宿主为整数预算模型 + 对练操作量对账（230 万 ops），vxbench 回填（翻案不改判据结构只改参数）。
2. B-1603 回显 16ms **实测**随实机窗口——宿主为五段整数预算推导，真实 PS/2 中断到提交面的路径计时回填。
3. B-1604 混用 shell 真实命令面（POSIX shell 语法/管道/重定向 + 直插柜台真实混用会话）随 shell 域接线——宿主为路由表模型面。
4. B-1703 全盘搜索真实 ext4 遍历随 VFS 域——宿主为 JobState 终态机模型。
5. B-1801 百兆秒开真实 mmap 计时随实机窗口；B-1803 四小件真实冷启动/内存随应用构建管线（宿主为登记表模型面）。

**WP-205 最丑角落（m4 复盘用）**：termproc UTF-8 解码未建模（宽字符经 feed_wide 显式入口——真实 UTF-8 流解码随渲染域）；termfeed 命令路由是表模型（真实 POSIX shell 语法随 shell 域）；trashbin 配额基于条目 size 模型面（真实 DATA 分区配额随存储域）；fsview 即席过滤是子串匹配（真实索引级搜索随搜索域）；edcore mmap 是窗口模型（真实 mmap/缺页随内核存储域）；widgetline 四件指标是登记表（真实冷启动/内存测量随应用构建）；settable 后端执法面是 validate/commit 模型（真实各服务后端随服务域接线）；八域与存量 terminal/fileman/editor/settings/apps 域的收敛留 m2（同 WP-201/203/208/204/202 口径）。

## WP-206 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：监视器与星图前端（MD3 行 94）四判据（B-2001 / B-2002 / B-2003 / B-2103；B-2101/2102/2104 归 WP-305 阶段三）从 MD2 篇 20/21 落为**判据实装层**——四个新模块（ledgerhub/moncards/winegrp/starmapui），单测前缀 fa01~fa04（避撞验证：grep 确认 fa0x 零占用）。**同源契约先行**：账本订阅面（B-2001）的"一份打点三个消费者不许各插各的桩"在本包落为对表演练恒等式，WP-209 vxbench 骨架与 WP-402 性能体系依此对表——监视器看到的数必须就是基准体系记录的数。

**交付面**（四文件新建 + 三文件注册 + 一文档回写）：
- `ledgerhub.rs`（新建，B-2001 · 9 项）：六频道订阅（Mem/Proc/Cpu/BlockIo/Net/ThermalFan 预留）+ Reading 值单位同行（口径随读数走）+ 内存三段口径（MemReading 恒等式 app+cache+kernel==total + MEM_NOTE 固定注释）+ Ledger record 唯一写入口/read 只读/seq 单调 + SubHub 多消费者（Monitor/Updater/DiagCenter）+ 一秒节流（首帧直推、窗口内重复广播不推、满一秒新帧可推）+ Window 六十点滚动（第 61 点覆盖最旧 + evicted=推送-容量）+ WindowAgg sum/min/max/mean 整数自洽 + **run_recon_drill 对表演练**（三消费者同帧逐字段一致）+ 订阅槽位守恒（幂等不占新槽/注销回收可再订阅/重复注销返 false 不误伤）。
- `moncards.rs`（新建，B-2002 · 9 项）：ServiceDecl 声明面（服务写）/CardView 视图面（呈现读）分离 + 三态颜色映射（绿/黄/红）+ **卡面实时一致恒等式**（声明一变卡面即变+文案==声明原文直通不转写）+ CardBoard 降级历史环（每卡 10 槽×4 卡=40 槽定长 + 新→旧 + **同态刷新不记**）+ verdict_kill 三出口（Protected 拒杀带解释/NeedsConfirm/Allowed——关键清单 KEY_PROCS 启动表声明 + 确认与否都拒）+ ThermalCardView 恒占位（**类型面无 value 槽——呈现假数据编译面不可能**）+ fps_panel_visible 开发者门。
- `winegrp.rs`（新建，B-2003 · 8 项）：ProcRow/WineGroup 组模型 + **聚合恒等式**（agg==server+Σ成员，CPU/内存两面）+ expanded_rows 展开（每成员独立可见，聚合不隐藏个体）+ merge_groups 组归并（wineserver 立卡/成员入最近卡/非 Wine 跳过）+ 非 Wine 入组拒绝留痕 + MEMBER_CAP 满员拒绝留痕（rejected 计数不静默丢）+ 多组不串账（两组各自聚合零混线）+ 无 Wine 零卡 + 展开读数与录入同源。
- `starmapui.rs`（新建，B-2103 · 10 项）：Rating 五档（**MD1 18.1 表序**：原生/直插/兼容/桥接/兜底）+ primary_action 唯一映射函数（兜底→SwitchToWindows 走交接/桥接→OpenViaWebShell 经网页壳/其余→Open——**呈现面无权改写，类型面防线**）+ RecoBoard 推荐位（只收新上架与判例更新/无标记不入位/商业位拒绝留痕）+ ListFilter 三轴过滤（名称子串+评级+腿别，filter_cards 定长收集体）+ StarCard 详情四要素（判例数/指标带单位/复评日期/目录版本）+ InstallFlow 四段顺序状态机（解析→校验→落盘→登记，跳段一律拒绝）+ uninstall_check 关联检查（MIME/自启如实报告不静默清除）。
- `lib.rs`：四模块注册（settable 后追加，带判据号 doc 注释）；`quality.rs`：四域入 run_full_loop（settable 后）+ 断言链五处同步 82→86（F489 条目+注释/F493 仪表/f489 测试体/F493 测试体/记账下限 +36）+ **MAX_LOOP 88 不扩容**（86≤88，剩 2 槽给 WP-207/209，不够即扩）；`robust.rs`：四域入 checkup。
- `docs/Varix STAR I · MD2 技术详案.md`：篇 21 判据表后插入"篇 20 与 21 前端判据实测回写"段——四判据×模块×CheckSet×实装要点完整表格 + 结构防线两条族（按钮语义函数唯一性/占位诚实类型化）+ 勘误连带四条。

**证据三件套**：全量 `cargo ktest` **PASS=3466 FAIL=0 EXIT=0**（较 WP-205 收口 3450 +16 = 四域新单测 4×4）；**CheckSet 36 项**（ledgerhub 9/moncards 9/winegrp 8/starmapui 10）+ **单测 16 项**，四域定向全绿（fa01~fa04 16/16 ok）；86 域 CheckSet 全 PASS（F489 `lp.len()==86` + 记账下限 `39*25+134+68+56+57+57+73+36`）。复现 = `cd kernel && cargo ktest`；日期 = 2026-09-24。

**红项处置**（编译与对练捉住的真实缺陷，修复并锁定回归）：
1. **中文字节串 b"" 编译错（193 错同根因）**：`b"中文"` 字节串字面量只允许 ASCII——四个新模块的中文文案常量全部改 `&str`（既有范式：fsview NTFS_BADGE）。教训入库：**含中文字面量的常量直接声明 &str，不写 b""**。
2. **SubSlot 数组初始化缺 Copy**：`[SubSlot::empty(); CAP]` 要求元素 Copy——加 derive(Clone, Copy)。
3. **WindowAgg/Window 方法挂错对象**：CheckSet 项 6 把 Window 的 evicted_total() 写到 agg 上（E0599）——聚合结果与方法宿主分开核对。
4. **ThermalCardView.note 批量替换漏网**：&[u8]→&str 脚本化替换只中第一处（CardView），同型第二处（ThermalCardView）漏改（E0308 连带 E0277）——**批量替换后必须全文件复查同型字段**。
5. **winegrp u32/usize 两处**：MEMBER_CAP 是 usize，循环变量 u32 比较报 E0308（CheckSet 项 6 与单测各一）——容量常量循环统一 usize，pid 参数处显式 as u32。
6. **KillVerdict 缺 Debug derive**：assert_eq! 需要 Debug——枚举派生 PartialEq 时同步想到 Debug。
7. **fa04_filter_axes 断言自相矛盾（单测自捉）**：三轴过滤断言"命中 0"但测试数据 "alpha" 本身就是 Native+leg1 全中（应命中 1）——**对练序列没过一遍被测语义，WP-205 教训第 ⑥ 条重演（第五次）**。
8. **starmapui 评级五档初稿发明档位（设计面缺陷，写码时回读勘正）**：初稿枚举漏"直插级"且自造 bridged_A 防呆位——回读 MD1 18.1 勘实五档名与顺序。教训：**判据引用的宪章条款（评级档位名）必须回读原文，不凭记忆**。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-2001 账本数据面为宿主模型（record 整数读数）——真实内核账本（篇 14.1 分配路径记账）随 WP-402 打点全覆盖接线，同源契约（对表演练恒等式）已冻结。
2. B-2002 四卡真实服务降级声明源随服务域接线（宿主为 ServiceDecl 声明模型）；真实 EC 温度通道随实机（阶段 3 占位明示与本包类型面兼容——数据到位即填）。
3. B-2003 组聚合数据源随进程账本接线（宿主为 ProcTable 模型）。
4. B-2103 星图目录 JSON 加载/哈希校验（B-2101）与流水线门禁（B-2102）归 WP-305——本包锁前端按钮语义与过滤/安装/卸载呈现面。

**WP-206 最丑角落（m4 复盘用）**：ledgerhub 节流以宿主虚拟时钟建模（真实单调钟读取随内核时间域）；moncards 卡面为声明直通模型（真实渲染随合成器域）；winegrp 归并策略"成员入最近卡"是模型简化（真实 wineserver 组关系随 Wine 支架域接线）；starmapui 目录数据为内存表（真实 JSON 目录加载与版本化随 WP-305）；四域与存量 sysmon（AURORA A676~A700 域）的收敛留 m2（同各包口径）。
- 时序：WP-201 ✅ → WP-203 ✅ → WP-208 ✅ → WP-204 ✅ → WP-202 ✅ → WP-205 ✅ → **WP-206 ✅** → 下一包 WP-207（协议件：剪贴板/拖放/无障碍，MD3 行 96）。

## WP-207 收口明细（2026-09-24 · 宿主侧交付 · 判据实装层）

**定性**：协议件（MD3 行 96）八判据（B-3901~3903 / B-4001~4005）从 MD2 篇 39/40 落为**判据实装层**——三个新模块（clipown/dragdrop/a11ygate），单测前缀 fb01~fb03（避撞验证零占用；a11ygate 命名避让存量 `a11y/` 目录模块）。**隐私红线落点**：后台读剪贴板零成功（B-3901——显式授权语义 C-8 姊妹约束）+ 读取必经合成器单一路径（Q56 敏感类型判定执法位）。**构建期门禁落地**：对比度不达标构建期拒绝 + 减弱动效 SDK 原语层强制——门禁写在 CI 里不写在良心里。

**交付面**（三文件新建 + 三文件注册 + 一文档回写）：
- `clipown.rs`（新建，B-3901 · 7 项 + B-3903 · 3 项）：Clipboard 所有权对象（select 只对焦点窗口生效）+ 描述段/内容段两段模型（描述随所有权即时可得，内容按需经合成器拉取）+ read 三裁决（DeniedNotFocused 后台读零成功/DeniedBypass 旁路拒/DeniedEmpty）+ 审计环留痕 + denied_bg_reads 计数（100 轮对抗全拒对账）+ owner_gone 即时回收（描述与内容随所有权同灭）+ 类型上限（text_desc 256KB/bitmap_desc 4096 见方截断如实标记）+ paste_op 删除=TrashOnly（与 B-1701 零真删咬合）+ paste_sensitive 敏感类型执法（Q56）+ 位图 shm 引用传递（报文不含像素）+ paste_progress_cancelable（面积 1M 像素阈值）+ text_paste_budget_ns（8ns/B+2ms 固定 ≤16ms）。
- `dragdrop.rs`（新建，B-3902 · 8 项）：DragSession 四报文相位机（Idle/Dragging/Done/Cancelled）+ try_mutate_payload 拖动中一律拒绝（载荷定型——事故源协议层消灭）+ cursor_for 光标三态裁决（按命中目标 DropTargetDecl 声明）+ 高亮必选位 + **三取消一清理**（cancel_esc/cancel_invalid_target/cancel_src_destroyed 共用 cleanup——cleanup_runs 恰一次 + 终态幂等 + 仅源窗口销毁触发兜底）+ translate_effect 跨域效果位映射（COPY/MOVE 翻译，NONE 与未知位如实返 None 不臆造）。
- `a11ygate.rs`（新建，B-4001~4005 · 10 项）：FocusRing 自动登记 + tab_order 视觉序推导（y 主序 x 次序，插入排序零堆）+ manual_move 评审标记（manual_override——例外要交代）+ popup_close_restore 还焦点协议级（无旁路）+ srgb_lin gamma 2.0 整数近似 + rel_luma 万分比加权 + contrast_ratio 百分之一单位（AA=450/大字=300）+ theme_contrast_gate 构建期拒绝 + status_glyph 四状态形状互异穷举 + anim_primitive 减弱动效两档（Instant/Fade 120ms/Stopped 呼吸全停/关=Full）+ reduce_all_effective 混合组零 Full 出口。
- `lib.rs`：三模块注册（starmapui 后追加，带判据号 doc 注释）；`quality.rs`：三域入 run_full_loop + **MAX_LOOP 88→92**（89≤92，剩 3 槽给 WP-209）+ 断言链五处同步 86→89（F489 条目+注释/F493 仪表/f489 测试体/F493 测试体/记账下限 +28）；`robust.rs`：三域入 checkup。
- `docs/Varix STAR I · MD2 技术详案.md`：篇 40 判据表后插入"篇 39 与 40 判据实测回写"段——八判据×模块×CheckSet×实装要点完整表格 + 结构防线两条族（门禁机制化/定型与单一路径）+ 勘误连带四条。

**证据三件套**：全量 `cargo ktest` **PASS=3478 FAIL=0 EXIT=0**（较 WP-206 收口 3466 +12 = 三域新单测 4×3）；**CheckSet 28 项**（clipown 10/dragdrop 8/a11ygate 10）+ **单测 12 项**，三域定向全绿（fb01~fb03 12/12 ok）；89 域 CheckSet 全 PASS（F489 `lp.len()==89` + 记账下限 `39*25+134+68+56+57+57+73+36+28`）。复现 = `cd kernel && cargo ktest`；日期 = 2026-09-24。

**红项处置**（对练捉住的真实缺陷，修复并锁定回归）：
1. **大字档选色错误（CheckSet 挂 1 项→四测试连锁传导）**：0x888888 对黑底对比度按 gamma 2.0 近似算出 668 ≥ 450——断言"r6 < CONTRAST_AA"不成立，f489/f488/f475/f500 四测试连锁挂。修为 0x5A5A5A（349，落在 300..450 区间，大字过普通字拒两档差异可演示）。教训：**测试数据要先算一遍被测公式**（"对练序列先过被测语义"的公式版，第六次重演）；一处 CheckSet 挂项会沿闭环断言链四处传导（f475 渲染 buf 8192 这次直接现形 FAIL 行——WP-205 诊断改进生效）。
2. **a11ygate 命名撞存量模块**：初稿写 `a11y.rs` 与存量 `a11y/` 目录（lib.rs `pub mod a11y;`）同名冲突——落刀前先 ls 模块名，改名 a11ygate。
3. gamma 2.0 近似口径如实标注：真 WCAG 线性化是 gamma 2.4，整数近似有偏差——门禁阈值按同口径校准自洽，不谎称精确 WCAG（模型先行实机校准既定口径）。

**环境偏差登记（不阻断，随队跟踪）**：
1. B-3901 剪贴板内容段为引用面建模（定长缓冲+实长）——真实 256KB 大缓冲随所有者进程地址空间（WP-401）；真实 VXWM 报文（四条 clipboard 报文）随合成器域接线。
2. B-3902 三取消"每条取消路径真人拖一遍"随实机走查（宿主为相位机模型面+清理对账）；跨域翻译真实 Win32 拖放语义随 Wine 支架域。
3. B-4003 真实 WCAG 2.4 线性化与主题构建管线（CI 挂钩）随 WP-303 SDK 正式化——门禁函数已就位，接线即用。
4. B-4004 真实图标形状渲染随主题域；B-4005 真实动画原语随 SDK 动画面。

**WP-207 最丑角落（m4 复盘用）**：clipown 文本内容段 buf 定长 32 仅建模引用面（真实 256KB 随进程域）；dragdrop 光标三态与 MD1 附录 H 报文序的完整对齐随合成器域；a11ygate 焦点环 Tab 序为视觉序模型（树序随 SDK 控件基类）；对比度 gamma 2.0 近似口径需实机校准回填；三域与存量 a11y/（AURORA 域）收敛留 m2。
- 时序：WP-201 ✅ → WP-203 ✅ → WP-208 ✅ → WP-204 ✅ → WP-202 ✅ → WP-205 ✅ → WP-206 ✅ → **WP-207 ✅** → 下一包 WP-209（测量与恢复面：vxbench 骨架/崩溃恢复/杀死演练，MD3 行 100）→【m2 闸门】。
