# Varix STAR I · 工作包台账（MD3 附录 B 执行面）

按 MD3 附录 B 模板逐包一行；状态与判据同记（MD3 1.5），台账与代码同库同提交（R4 恢复点纪律）。里程碑对账时逐行核。

| WP 编号 | 状态 | 判据状态 | 依赖状态 | schema 变更 | 收工日 |
| --- | --- | --- | --- | --- | --- |
| WP-101 | 收口（宿主侧） | B-101~106 六绿-宿主（实机/QEMU 依赖项=环境未就位类，随 WP-102 补测） | 无前置 | limine.conf 固化（interface_version + hash 元数据、default_entry、comment 字段；bootconf::render 单源） | 2026-09-24 |
| WP-103 | 收口（宿主侧） | B-302~307 六绿-宿主（B-301 实机 72h=环境未就位类，B-305"Variable 启动正常"半句随实机补测） | WP-101（已收口） | 新增 `portable/engine/hardening/` 五件（vxlib / harden_quad / harden_vcruntime / recheck / run_all）；deploy↔recheck 契约=deploy_report.json（gate.sha256 + vcruntime.hashes） | 2026-09-24 |
| WP-104 | 未开工 | — | WP-101（与 WP-103 交错） | — | — |
| WP-102 | 未开工 | — | WP-101/104 | — | — |
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
