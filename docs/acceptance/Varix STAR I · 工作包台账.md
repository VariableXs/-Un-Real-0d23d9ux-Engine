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
4. 百次对练收账：闭环链路已实证，`bash scripts/panic-drill.sh 100` 后台进行中，收账数回填本行。

**WP-106 最丑角落（m4 复盘用）**：复位代码三处重复（panicseq 自足副本 / bootnext 共用面 / usrshell 阶梯）——刻意自足（panic 路径零外部依赖）与 DRY 的张力，m2 重构时评估收敛；`format_cd` 返回 ([u8;40], usize) 的固定缓冲设计（no_std 无 format! 的诚实取舍）；通知链空集记账（run_notify_chain 空集 = Done 零开销，真实接线后有步骤才走超时路径——WP-203 承接时需验预算切片与真实步骤数的配比）；现场带落位依赖"memmap 逐 boot 相同"的确定性假设（同固件+同配置成立，热插拔内存/固件升级后落位漂移 → 重放静默失效——四道闸保证不误报，但"持久性"承诺在漂移场景降级为"尽力"）。
