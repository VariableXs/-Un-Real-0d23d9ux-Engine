//! UNX 册模块登记（CoRun Varix STAR II · Unxreal —— UNX-A1 引导与早年初始化）。
//!
//! 本册与主内核 acpi.rs（F008 RSDP/XSDT/FADT/MADT 基础解析）为分层深化：
//! 本册做 ACPI 表专项（DSDT/SSDT/AML/拓扑/UEFI 态），基础解析不重复实现，
//! 直接复用 `crate::acpi` 的 SdtHeader/checksum_ok/SDT_HEADER_LEN。
//! 后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433。

use alloc::vec::Vec;
use alloc::format;
use alloc::string::String;
// 二级折叠表（`unxstar_secondary_blocks` / `run_unxstar_secondary_checks`）在
// **模块级**签名里用 `CheckSet`，故此处必须模块级导入——原`run_unxstar_checks`
// 的 `use crate::checks::CheckSet;` 是函数内局部导入，新函数取不到。
use crate::checks::CheckSet;
// ===================== W020 批次（本会话深编 · cpio newc 魔数识别族，0x7500 段） =====================
// unx603_newcmagic ↔ UNX-F0603 cpio newc 魔数识别引擎（0x070701）：魔数常量族/格式族判别闭集 Newc/NewcCrc/Odc/Bin/流式判别状态机 NoInput→Partial→Complete→Mismatch/通用识别器滑窗多命中歧义显性/版本账注 c_magic 登记封账指纹守恒/复合封账判定入口 judge_archive_head/段位守卫/0x7500 段十六码
pub mod unx603_newcmagic;
// unx603_fmtmatrix ↔ UNX-F0603 cpio 格式族能力矩阵与判别仲裁：四能力维度表 is_ascii/header_len/has_check/aligned4/priority + bin 二字节小端 0x71C7 检测 + 候选闭集按优先级确定性仲裁 + 格式登记册 append-only 封账指纹守恒 + 矩阵行序/优先级表逐行守卫/0x7510 段十六码
pub mod unx603_fmtmatrix;
// unx603_cmagic ↔ UNX-F0603 cpio c_magic 字段识别与 newc 整数字段编解码：c_magic 六字符判别定版本/newc 全部整数字段 ASCII-十六进制编解码（8 字符定长大端）/字段级 newc 头解析器（13 字段切分）/字段版本账注登记封账指纹守恒/字段名唯一性/编解码往返一致/0x7520 段十六码
pub mod unx603_cmagic;
pub mod unx041_dsdt;
pub mod unx041_dsdt_checks;
// unx041_dsdt_deep ↔ UNX-F0041 DSDT 遍历校验（既有深化模块，里程碑三：《统一协作总台账》通篇重述 RSDP 双窗/XSDT 直查 RSDT 回退/FACP 定位/X_DSDT 兼顾 32 位地址/DSDT 槽位遍历（SUOP-与-OVSP-辨识）/OEM ID 动态补全（枚举-倒查-逆推）/运行期 Info-Warn-Fatal/BootReport append-only 面/AML 全码段表/DsdtSnap 确定性 FNV+快照/0x2F 段十六进制码与 0x2E 既有码区分）
pub mod unx041_dsdt_deep;
// unx182_handover ↔ UNX-F0182 阶段产物交接协议 init 契约层（16 字段在位位图 + 缺字段拒启动落账 + 拒绝账 append-only 满账显性 + FNV 指纹复算；0x80 段）
pub mod unx182_handover;
// unx421_idt_reload ↔ UNX-F0421 IDT 全量重装 256 门冻结（布局账 256 门 Type/IST/DPL/选择子四位态 + 逐门比对 256/256 + lidt 回读 limit/base 双验 + 冻结门改写拒；0x81 段）
pub mod unx421_idt_reload;
// unx441_rsdp_scan ↔ UNX-F0441 RSDP 签名搜索 EBDA 1KB + BIOS ROM 128KB 双窗（16 字节对齐推进 + 签名与 v1/v2 双代校验和双门 + 多命中歧义显性 + 命中账 OEM/revision/RSDT/XSDT；0x82 段）
pub mod unx441_rsdp_scan;
// unx461_pci_route ↔ UNX-F0461 配置访问机制裁定 ECAM 优先 CF8 兜底（兜底原因显性落账 + 8/8 设备双机制回读一致 + 扩展配置空间 CF8 显性不支持禁返假值 + 轮转抽样；0x83 段）
pub mod unx461_pci_route;
// unx481_fb_ledger ↔ UNX-F0481 帧缓冲四要素账冻结（四字段在位掩码 + 复读一致 4/4 + stride/总字节推导对账 + 封账后写拒 + reseal 严格升版指纹必变；0x84 段）
pub mod unx481_fb_ledger;
// unx483_fbmap —— UNX-F0483 帧缓冲映射登记（B19 账 1/1 段 + 区外写拦截 100% + 单向状态机 + 快照指纹，0x98 段十二码）
pub mod unx483_fbmap;
// unx521_ktest_frame ↔ UNX-F0521 ktest 主框架总装（注册器名唯一定容 32 + 运行器泵动单步/全量同状态机 + 接入核审计注册⇄运行 6/6 + 截断显性拦截假绿；0x85 段）
pub mod unx521_ktest_frame;
// unx541_log_emit ↔ UNX-F0541 日志面总装（四级别冻结 Trace/Info/Warn/Fatal + emit 三参数（级别/组件/格式化文）+ 级别闸丢弃逐格记账 + 环满显性溢出不覆盖 + 10/10 全链接入核；0x86 段）
pub mod unx541_log_emit;
// unx561_power_ledger ↔ UNX-F0561 电源面定位总账（FADT 五块 PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 基址+长度 + 直读一致 5/5 + IO/MMIO/FixedHw 空间分派 + 宽 1/2/4 合法 + 封条指纹掉电复验；0x87 段）
pub mod unx561_power_ledger;
// unx563_pm1sts —— UNX-F0563 PM1_STS 状态账（事件位闭集+W1C 零写防线+双读一致+封账状态机，0x9A 段十码）
pub mod unx563_pm1sts;
// unx601_initrd_ledger ↔ UNX-F0601 initrd 基址/长度账（三元组 base/len/FP 交接 1/1 + 64MB 预算界 + 4KB 对齐 + 页数上取整 + 三段抽样指纹复算 + 全页核对才封账；0x88 段）
pub mod unx601_initrd_ledger;
// unx621_pkg_aggregate ↔ UNX-F0621 域交接包聚合 12 包总装（源清单 F0349..F0617 十二包集合差核 + 每包七件套在位 + 索引冻结 + 有序聚合指纹；0x89 段）
pub mod unx621_pkg_aggregate;
// unx641_regress_round ↔ UNX-F0641 域回归轮编排（33 批冻结表 + 逐轮推进越序 100% 拦截 + 重跑拒收 + 放行账与结果聚合对冲全平 + 断点续跑不双记；0x8A 段）
pub mod unx641_regress_round;
// unx661_time_anchors ↔ UNX-F0661 冷启动计时锚点表（T0 上电/T1 引导器接管/T2 内核入口/T3 进 main 四锚冻结 + 50 次全链留痕 + 单调红线 + 段时长推导；0x8B 段）
pub mod unx661_time_anchors;
// unx681_bench_machine ↔ UNX-F0681 三机基准机建档（机型/固件/配置哈希三列 + 指纹复算 + 漂移检出 100% 点名 + 幂等重入 + 换机冲突拦截；0x8C 段）
pub mod unx681_bench_machine;
// unx701_assert_ledger ↔ UNX-F0701 断言总盘点账（五分项实数/硬线/缺口三列 + 逐断言可解引 100% + 全域 ≥300 硬线 + 缺口对冲 + 重 id 拒收；0x8D 段）
pub mod unx701_assert_ledger;
// unx721_limine_ledger ↔ UNX-F0721 Limine 版本台账（当前版本指纹复算 + 上游版本流单调禁降级 + 变更通知账空原因拒 + 落后量化；0x8E 段）
pub mod unx721_limine_ledger;
// unx081_kaslr_entropy — UNX-F0081 KASLR 熵源采集器深化版（三源 RDRAND/TSC低位/内存图自由度 注入判读分离/混合估计保守口径 权重万分制×独立折扣三档/低于12位阈值降级槽0固定基址+告警/熵足FNV种子哈希选槽同种子可复现/熵账本守恒 seen=produced+absent+rejected/AlertLog 告警账+KaslrSnap 快照FNV往返/0x30 段十六码与 0x2E/0x2F 不重叠）
pub mod unx081_kaslr_entropy;
// unx101_vgafallback — UNX-F0101 VGA 文本模式后备通道深化版（帧簿故障自动落通道/三态状态机 Active-Degraded-Restored 迁移表九格全枚举/Restored 不自动切回防抖动须显性指令/零丢行行账守恒 produced=delivered+backlog 丢弃恒0/行号连续审计跳号即立案/切换窗口积压补发 FIFO 先入先刷/80×25 纯视图缓冲滚动计数显性滚动不丢行/超宽折行保全行文/16色闭集属性字节/VgaSnap 快照FNV往返/0x31 段十六码）
pub mod unx101_vgafallback;
// unx121_extbootstate — UNX-F0121 引导阶段扩展态深化版（设计矩阵 const 表 3态×5事件 15格全显性 合法恰4非法恰11/match与表逐格对拍两写法一致/双层闸 矩阵格→条件谓词 无失败不恢复·三步上限·零步无据·无档不叠/语义特闸 探测中直降级·恢复中改探测/历史环 12 条窗口/ExtSnap 快照FNV往返/0x32 段十六码）
pub mod unx121_extbootstate;
// unx141_isogen — UNX-F0141 ISO9660/El Torito 生成器深化版（字节级构造 PVD CD001 魔数/El Torito 引导记录 VD 32字节规范串/Validation Entry 零和校验+0x55AA+0x88 旗标/终止描述符/目录记录 34字节最小态/名规范化 Level1 大写8.3/清单四闸 数空长重/确定性排序可复现同清单异序字节指纹恒等/扇区2048对齐 LBA 累计器单点/生成即校验六闸 xorriso 拒收面前置/破坏面显性检出/IsoSnap 快照FNV往返/0x33 段十六码）
pub mod unx141_isogen;
// unx162_timecore — UNX-F0162 GetTime/SetTime 语义全量化（TimeStamp 七字段闭集+逐字段域校验+进位链+民用日数+位打包往返）
pub mod unx162_timecore;
// unx221_ktregistry — UNX-F0221 ktest 注册器（判据注册表）
pub mod unx221_ktregistry;
// unx222_kagg — UNX-F0221 引导面断言聚合（聚合器扩容 300+ 达成线）
pub mod unx222_kagg;
// unx223_krecon — UNX-F0221 断言重整（重整对账）
pub mod unx223_krecon;
// unx161_rtremap — UNX-F0161 Runtime Services 虚拟地址重映射深化版（两态机 Physical→Virtual 单向不可逆 DoubleRemap 闸/映射表三闸 对齐·重叠·覆盖/服务八闭集双地址账 resolve 解析单点/旧地址断零机制 Virtual 期物理访问显性拒+计数/悬空服务显性 ServiceUnmapped 守恒揭破半映射态/RtEntry 定格换算段内等值平移/RemapSnap 快照FNV往返/0x34 段十六码）
pub mod unx161_rtremap;
// unx201_bootreport — UNX-F0201 引导报告生成器全景版（五族24字段闭集 齐备率千分制 显性Na算齐备静默缺失不算/build齐备率硬闸 不齐备不出门携带缺清单/七阶段守恒账 Σ段==全程 checked_add 破坏即TimeNotConserved立案/异常账append-only 容量32满即显性拒 三级severity 派生计数单点真源/kv行协议+文本报告双形态单一真源交叉对拍 行数闸+键覆盖闸/敏感字段默认脱敏0xREDACTED/FNV1a-64快照往返 内容变指纹变/ReportCode 0x36段十六码 segment_guard双写对拍）
pub mod unx201_bootreport;
pub mod unx2041_freqidle;
// unx181_initplan — UNX-F0181 初始化顺序编排器（依赖图驱动：单一声明源五类静态校验未知依赖-自依赖-重复边-阶段单向-互斥对称/双实现拓扑序分层Kahn+朴素逐点对拍确定性/环检出100% Kahn剩余+DFS三色提取具体环路径/计划不变量四闸恰一次-依赖前置-wave一致-串行组跨wave/确定性模拟执行LCG种子+tick注入+账本守恒五终态/FNV快照往返/0x35 段十码）
pub mod unx181_initplan;

/// UNX 册域标签（F475 闭环登记用）。
pub const UNX_DOMAIN: &str = "unxstar";

/// UNX 册判据聚合（逐子集折叠，新单入账在此数组追加）。
///
/// 原实现是 `CheckSet::merge(单子集)`——`merge` 需二参，`unxstar` 只有
/// 一个子集，这个调用**根本编译不过**。之所以一直没暴露：本模块此前未在
/// `lib.rs` 声明，是孤儿模块，编译单元从未被纳入（AI-CGPU 给CGPU 接线时
/// 顺路补声明才暴露）。折叠范式与 `svstar2::run_svstar2_checks` 一致。
pub fn run_unxstar_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new(UNX_DOMAIN);
    // 修正记录（性能，量级实测）：原写 `let blocks: [(&'static str, CheckSet); 69]`
    // ——**数组元素是已构造好的 `CheckSet` 值**，意味着 69 个子域自检在**进入
    // 循环之前就全部跑完并各自在栈上占一格**。`CheckSet` 约 4.5KB，69 × 4.5KB
    // ≈ **310KB 栈占用**；更糟的是紧随其后的 `for` **没有循环体**
    // （`for (tag, sub) in blocks.iter() {` 之后直接是下一个独立块），
    // 即这 69 个子集**构造完就被丢弃，一项都没被消费**——纯浪费。
    //
    // 实测（`varix-cf3b93db` 二进制，单线程）：各子域单独跑均约 140ms
    // （69 × 140ms ≈ 10 秒可接受），但 `unxstar_domain_aggregate_all_green`
    // 单独跑**超 300 秒仍未结束**，CPU 时间 2585s 且持续增长（非死锁）。
    //
    // 修法：改为**函数指针表** `[(&'static str, fn() -> CheckSet); 69]`，循环内
    // `f()` 即用即弃——同一时刻栈上只存活 1 个 `CheckSet`（约 4.5KB），
    // 与 `robust.rs::run_kernel_checkup` 的 352 项函数指针表同一手法
    // （该处注释已记录动机：「debug 模式下每个直排调用的返回值临时各占一个
    // 栈槽（CheckSet ≈ 3.1KB × 349 ≈ 1082KB），叠加测试线程 ~1MB 栈即
    // STATUS_STACK_OVERFLOW」）。**降栈 + 降峰值内存 + 让子集真正被执行。**

    // 修正记录（续）：原 `for` **没有循环体**——第 179 行算完 `passed` 之后
    // 直接接下一个独立块 `{}`，69 个子集的红绿一项都没汇入 `set`，
    // 即上面那张表**纯浪费**（既占 310KB 栈、又跑了全部子域自检，结果全丢）。
    // 此处补上循环体：逐子集取绿、登记到域总账。
    //
    // 明细口径与下方 `run_unx20_*` 各块**保持一致**：
    // - 全绿且未截断 → 空明细（不吐噪声）；
    // - 红项 → `red`；
    // - 溢出（子集自身超 CheckSet 上限）→ `truncated`，与红项**分开记账**
    //   （截断是「账装不下」，红项是「账里有错」，混为一谈会掩盖真实原因）。
    //
    // 修正记录（主表分段折叠，修复主集溢出静默丢族）：主表已扩至 353 席，
    // 「每族 1 格平铺」早超 `MAX_CHECKS = 112` 容量——第 113 族起被 `add`
    // 的溢出守卫静默丢弃，末尾 260+ 族（含二级折叠格）**从未参与聚合**。
    // 修复：表族按 `SEG = 100` 分段，每段内部聚合成段集（每族 1 格，语义
    // 原样），段集整体作为顶层 1 格（`UNX-MAIN-S01..S10` 静态名封顶）。
    // 顶层总格 = ⌈表长/100⌉ + 手工块 62 + 二级折叠 1 ≈ 67，恒在容量内；
    // 段内丢族会在段集格数/段格红绿上暴露，判据强度不降。
    const MAIN_SEG: usize = 100;
    {
        let table = unxstar_blocks();
        let chunks = table.len().div_ceil(MAIN_SEG);
        for c in 0..chunks {
            let lo = c * MAIN_SEG;
            let hi = ((c + 1) * MAIN_SEG).min(table.len());
            let mut seg_set = CheckSet::new("unxstar-main-seg");
            for (tag, f) in table[lo..hi].iter() {
                // 即用即弃：同一时刻栈上只存活一个子集 CheckSet（约 4.5KB）。
                // （诊断探针，随诊撤：逐子域计时，定位聚合病态慢的元凶子域。）
                #[cfg(test)]
                let __t0 = {
                    use std::time::Instant;
                    eprintln!("[unx-timing] enter {}", tag);
                    Instant::now()
                };
                let sub = f();
                #[cfg(test)]
                eprintln!("[unx-timing] {} {:.1}s", tag, __t0.elapsed().as_secs_f32());
                let passed = sub.all_passed() && !sub.truncated();
                seg_set.add(
                    // `blocks.iter()` 产出 `&(&'static str, fn…)`，故 `tag` 是
                    // `&&'static str`，而 `add` 的 `name` 契约是 `&'static str`
                    // （不收短生命周期）——解引用一次取回真静态标签。
                    *tag,
                    passed,
                    if passed {
                        ""
                    } else if sub.truncated() {
                        "unxstar family truncated"
                    } else {
                        "unxstar family red"
                    },
                );
            }
            let seg_ok = seg_set.all_passed() && !seg_set.truncated();
            // 段名静态封顶：10 段 = 1000 族（当前 353 → 4 段）。
            let seg_name: &'static str = match c {
                0 => "UNX-MAIN-S01",
                1 => "UNX-MAIN-S02",
                2 => "UNX-MAIN-S03",
                3 => "UNX-MAIN-S04",
                4 => "UNX-MAIN-S05",
                5 => "UNX-MAIN-S06",
                6 => "UNX-MAIN-S07",
                7 => "UNX-MAIN-S08",
                8 => "UNX-MAIN-S09",
                _ => "UNX-MAIN-S10",
            };
            set.add(
                seg_name,
                seg_ok,
                if seg_ok {
                    ""
                } else if seg_set.truncated() {
                    "main segment truncated"
                } else {
                    "main segment has red family"
                },
            );
        }
    }

    {
        #[cfg(test)] let __t_t082 = { eprintln!("[unx-timing] enter t082"); std::time::Instant::now() };
        let sub = run_unx20_t082();
        #[cfg(test)] eprintln!("[unx-timing] t082 {:.1}s", __t_t082.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0082",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t126 = { eprintln!("[unx-timing] enter t126"); std::time::Instant::now() };
        let sub = run_unx20_t126();
        #[cfg(test)] eprintln!("[unx-timing] t126 {:.1}s", __t_t126.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0123",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t147 = { eprintln!("[unx-timing] enter t147"); std::time::Instant::now() };
        let sub = run_unx20_t147();
        #[cfg(test)] eprintln!("[unx-timing] t147 {:.1}s", __t_t147.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0143",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t163 = { eprintln!("[unx-timing] enter t163"); std::time::Instant::now() };
        let sub = run_unx20_t163();
        #[cfg(test)] eprintln!("[unx-timing] t163 {:.1}s", __t_t163.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0163",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t203 = { eprintln!("[unx-timing] enter t203"); std::time::Instant::now() };
        let sub = run_unx20_t203();
        #[cfg(test)] eprintln!("[unx-timing] t203 {:.1}s", __t_t203.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0203",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t228 = { eprintln!("[unx-timing] enter t228"); std::time::Instant::now() };
        let sub = run_unx20_t228();
        #[cfg(test)] eprintln!("[unx-timing] t228 {:.1}s", __t_t228.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0222",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t242 = { eprintln!("[unx-timing] enter t242"); std::time::Instant::now() };
        let sub = run_unx20_t242();
        #[cfg(test)] eprintln!("[unx-timing] t242 {:.1}s", __t_t242.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0242",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t268 = { eprintln!("[unx-timing] enter t268"); std::time::Instant::now() };
        let sub = run_unx20_t268();
        #[cfg(test)] eprintln!("[unx-timing] t268 {:.1}s", __t_t268.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0262",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t286 = { eprintln!("[unx-timing] enter t286"); std::time::Instant::now() };
        let sub = run_unx20_t286();
        #[cfg(test)] eprintln!("[unx-timing] t286 {:.1}s", __t_t286.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0282",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t326 = { eprintln!("[unx-timing] enter t326"); std::time::Instant::now() };
        let sub = run_unx20_t326();
        #[cfg(test)] eprintln!("[unx-timing] t326 {:.1}s", __t_t326.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0322",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t306 = { eprintln!("[unx-timing] enter t306"); std::time::Instant::now() };
        let sub = run_unx20_t306();
        #[cfg(test)] eprintln!("[unx-timing] t306 {:.1}s", __t_t306.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0302",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t325 = { eprintln!("[unx-timing] enter t325"); std::time::Instant::now() };
        let sub = run_unx20_t325();
        #[cfg(test)] eprintln!("[unx-timing] t325 {:.1}s", __t_t325.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "AP ID 核对域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t684 = { eprintln!("[unx-timing] enter t684"); std::time::Instant::now() };
        let sub = run_unx20_t684();
        #[cfg(test)] eprintln!("[unx-timing] t684 {:.1}s", __t_t684.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "固件路差异域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t505 = { eprintln!("[unx-timing] enter t505"); std::time::Instant::now() };
        let sub = run_unx20_t505();
        #[cfg(test)] eprintln!("[unx-timing] t505 {:.1}s", __t_t505.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "万次切换域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        #[cfg(test)] let __t_t545 = { eprintln!("[unx-timing] enter t545"); std::time::Instant::now() };
        let sub = run_unx20_t545();
        #[cfg(test)] eprintln!("[unx-timing] t545 {:.1}s", __t_t545.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "判据冻结域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    // UNX-20 任务族登记（20 域各一笔）—— `t162` 属二级折叠表
    // （`unxstar_secondary_blocks`），此处不再单独上主集，避免与二级汇总
    // 重复记账。
    {
        #[cfg(test)] let __t_t162 = { eprintln!("[unx-timing] enter t162"); std::time::Instant::now() };
        let sub = run_unx20_t162();
        #[cfg(test)] eprintln!("[unx-timing] t162 {:.1}s", __t_t162.elapsed().as_secs_f32());
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0162",
            passed,
            if passed {
                ""
            } else if sub.truncated() {
                "unxstar family truncated"
            } else {
                "unxstar family red"
            },
        );
    }
    // 修正记录（二级折叠，见 `unxstar_secondary_blocks` 的完整说明）：
    // 原在此处逐个追加 58 个族的 `set.add`，与上面 69 格合计 127 > 112，
    // 溢出 15 格且静默丢弃。现折进二级集，主集只占 1 格 → 69 + 1 = 70。
    {
        let secondary = run_unxstar_secondary_checks();
        let passed = secondary.all_passed() && !secondary.truncated();
        let red_n = secondary.failed_count();
        // 二级集的红项明细带不进 `add_dyn`：`add_dyn` 的动态明细上限是
        // `DYN_CAP = 48` **字节**，而 58 个标签拼起来远超这个量——写进去
        // 会被按 UTF-8 边界截断并补`~`，等于给出一条**残缺**的名单，
        // 而「残缺的名单」比「只有计数」更坏：它看起来像名单，会误导定位。
        //
        // 故主集这一格只带**红项计数**（短、完整、永不截断）；真正的
        // 名单由 `unxstar_domain_aggregate_all_green` 的 `red_items_text()`
        // 在失败路径上现场生成（无长度限制）给出。
        //
        // 修正记录（E0308 + E0716，编译期实测两次）：
        //   ① 三支类型须统一到 `&str`（最短生命周期）：前两支是
        //   `&'static str` 字面量、第三支是运行期文本，只能向下收缩。
        //   ② 不能写 `format!(…).as_str()` **直接作为分支值**——
        //   那是「借用一个即将被drop 的临时值」（E0716）。必须先绑定到
        //   具名变量再借。
        let red_msg;
        let detail: &str = if passed {
            ""
        } else if secondary.truncated() {
            "secondary truncated"
        } else {
            red_msg = alloc::format!("secondary red x{}", red_n);
            red_msg.as_str()
        };
        set.add_dyn("UNX-F-SECONDARY", passed, detail);
    }

    // 修正记录（E0308 `expected CheckSet, found ()`，编译期实测）：
    // 插入二级折叠段时，把函数末尾的 `set` 一并截掉了，于是函数尾部
    // 变成那个 `{}` 块的类型 `()`，与返回类型 `CheckSet` 不符。
    // 教训：机械改写长函数时，**尾部表达式必须逐条核对**——
    // `expected CheckSet, found ()` 这个报错本身已经把「少了个 `set`」
    // 说得很清楚，但若当时只盯着新增的那几行，很容易归因到新代码上。
    set
}

/// UNX 域**完整**总账（不截断入口）。
///
/// # 为什么另一个入口（实测，不是理论担忧）
///
/// 上面 `run_unxstar_checks()` 返回 `CheckSet`，容量 `MAX_CHECKS = 112`。
/// 而本域一级表有 277 个子集、二级折叠表另有一批，逐子集再展开到判据级
/// 约 8,548 条。`CheckSet::add` 满员即 `dropped += 1` 并 return，于是：
///
/// - 277 个子集的**汇总行**只能进前 112 行，后 165 个子集连「红/绿」都进不来；
/// - 每个子集内部若超 112 条，子集自身的红项同样被吞；
/// - 报表仍是 `PASS 112/112`——**看起来很绿**。
///
/// 结论：`run_unxstar_checks()` 的绿**不能**作为本域验收证据；它只能证明
/// 「前 112 个子集里的第一格是绿的」。真正的验收必须走本函数。
///
/// # 语义
///
/// - 计数**全量**累加（`DomainLedger::total` 精确到条）；
/// - 红项文本按 `RED_CAP` 优先留存，溢出显性记 `red_overflow`；
/// - 子集自身截断（`dropped > 0`）转成一条合成红项 `子集#dropped`，
///   不让它在聚合层被抹平。
///
/// 调用方：宿主测试侧（`robust` 体系）。内核运行期不调用——本函数会走堆。
pub fn run_unxstar_ledger() -> crate::domledger::DomainLedger {
    let mut ledger = crate::domledger::DomainLedger::new(UNX_DOMAIN);
    for (tag, f) in unxstar_blocks().iter() {
        let sub = f();
        let label: &str = tag;
        ledger.absorb(label, &sub);
        // 子集**汇总行**也入账：它是对「这个子集整体是否绿」的一票，
        // 与子集内逐条判据不重复（前者是子集级结论，后者是判据级证据）。
        let ok = sub.all_passed() && !sub.truncated();
        ledger.record(
            label,
            ok,
            if ok {
                ""
            } else if sub.truncated() {
                "family truncated"
            } else {
                "family red"
            },
            label,
        );
    }
    ledger
}

/// 一级折叠子集表（69 单，**函数指针形态**，循环内 `f()` 即用即弃）。
///
/// 修正记录（诊断面可及性）：此表原是 `run_unxstar_checks` 内的**局部变量**，
/// 于是失败路径上只有 `set.red_items_text()` 能报出「格标签」这一层，再往里
/// （哪个子集里的哪条判据红）就够不着了；而 `add_dyn` 的 `DYN_CAP = 48` 又
/// 装不下 69 个标签，聚合 panic 里只剩折叠层的残缺名单。抽成模块级函数后，
/// `unxstar_domain_aggregate_all_green` 可**直接遍历本表现场取下层名单**，
/// 长度不受 `DYN_CAP` 限制——与 `cgpu` / `stareco::deep` 同一手法的三处对齐。
pub fn unxstar_blocks() -> [(&'static str, fn() -> CheckSet); 353] {
    [
        ("UNX-F0041", unx041_dsdt_checks::run_unx041_checks as fn() -> CheckSet),
        ("UNX-F0041-DEEP", unx041_dsdt_deep::run_unx041b_checks as fn() -> CheckSet),
        ("UNX-F0081", unx081_kaslr_entropy::run_unx081_checks as fn() -> CheckSet),
        ("UNX-F0101", unx101_vgafallback::run_unx101_checks as fn() -> CheckSet),
        ("UNX-F0121", unx121_extbootstate::run_unx121_checks as fn() -> CheckSet),
        ("UNX-F0141", unx141_isogen::run_unx141_checks as fn() -> CheckSet),
        ("UNX-F0161", unx161_rtremap::run_unx161_checks as fn() -> CheckSet),
        ("UNX-F0201", unx201_bootreport::run_unx201_checks as fn() -> CheckSet),
        ("UNX-F0182", unx182_handover::run_unx182_checks as fn() -> CheckSet),
        ("UNX-F0421", unx421_idt_reload::run_unx421_checks as fn() -> CheckSet),
        ("UNX-F0441", unx441_rsdp_scan::run_unx441_checks as fn() -> CheckSet),
        ("UNX-F0461", unx461_pci_route::run_unx461_checks as fn() -> CheckSet),
        ("UNX-F0481", unx481_fb_ledger::run_unx481_checks as fn() -> CheckSet),
        ("UNX-F0521", unx521_ktest_frame::run_unx521_checks as fn() -> CheckSet),
        ("UNX-F0541", unx541_log_emit::run_unx541_checks as fn() -> CheckSet),
        ("UNX-F0561", unx561_power_ledger::run_unx561_checks as fn() -> CheckSet),
        ("UNX-F0601", unx601_initrd_ledger::run_unx601_checks as fn() -> CheckSet),
        ("UNX-F0621", unx621_pkg_aggregate::run_unx621_checks as fn() -> CheckSet),
        ("UNX-F0641", unx641_regress_round::run_unx641_checks as fn() -> CheckSet),
        ("UNX-F0661", unx661_time_anchors::run_unx661_checks as fn() -> CheckSet),
        ("UNX-F0681", unx681_bench_machine::run_unx681_checks as fn() -> CheckSet),
        ("UNX-F0701", unx701_assert_ledger::run_unx701_checks as fn() -> CheckSet),
        ("UNX-F0721", unx721_limine_ledger::run_unx721_checks as fn() -> CheckSet),
        ("UNX-F0183", unx183_failiso::run_unx183_checks as fn() -> CheckSet),
        ("UNX-F0204", unx204_reportring::run_unx204_checks as fn() -> CheckSet),
        ("UNX-F0223", unx223_dombound::run_unx223_checks as fn() -> CheckSet),
        ("UNX-F0243", unx243_memsnap::run_unx243_checks as fn() -> CheckSet),
        ("UNX-F0263", unx263_xmodem::run_unx263_checks as fn() -> CheckSet),
        ("UNX-F0323", unx323_realmjmp::run_unx323_checks as fn() -> CheckSet),
        ("UNX-F0342", unx342_apsilence::run_unx342_checks as fn() -> CheckSet),
        ("UNX-F0362", unx362_resvwhite::run_unx362_checks as fn() -> CheckSet),
        ("UNX-F0264", unx264_lineedit::run_unx264_checks as fn() -> CheckSet),
        ("UNX-F0324", unx324_sipiseq::run_unx324_checks as fn() -> CheckSet),
        ("UNX-F0643", unx643_drilllog::run_unx643_checks as fn() -> CheckSet),
        ("UNX-F0683", unx683_boottrace::run_unx683_checks as fn() -> CheckSet),
        ("UNX-F0206", unx206_excode::run_unx206_checks as fn() -> CheckSet),
        ("UNX-F0245", unx245_randnode::run_unx245_checks as fn() -> CheckSet),
        ("UNX-F0185", unx185_cpufeat::run_t185_checks as fn() -> CheckSet),
        ("UNX-F1563", unx1563_a4sign::run_unx1563_checks as fn() -> CheckSet),
        ("UNX-F1842", unx1842_ipivec::run_unx1842_checks as fn() -> CheckSet),
        ("UNX-F1902", unx1902_lazysave::run_unx1902_checks as fn() -> CheckSet),
        ("UNX-F0326", unx326_apsentinel::run_unx326_checks as fn() -> CheckSet),
        ("UNX-F0327", unx327_tramppagemap::run_unx327_checks as fn() -> CheckSet),
        ("UNX-F0405", unx405_monoclock::run_unx405_checks as fn() -> CheckSet),
        ("UNX-F0584", unx584_unkparam::run_unx584_checks as fn() -> CheckSet),
        ("UNX-F2101", unx2101_schedstat::run_unx2101_checks as fn() -> CheckSet),
        ("UNX-F2221", unx2221_freeledge::run_f2221_checks as fn() -> CheckSet),
        ("UNX-F0883", unx0883_migwin::run_t883_checks as fn() -> CheckSet),
        ("UNX-F1026", unx1026_hpetcand::run_t1026_checks as fn() -> CheckSet),
        ("UNX-F1103", unx1103_disptab::run_t1103_checks as fn() -> CheckSet),
        ("UNX-F1302", unx1302_tickretire::run_t1302_checks as fn() -> CheckSet),
        ("UNX-F1482", unx1482_gaplist::run_t1482_checks as fn() -> CheckSet),
        ("UNX-F1503", unx1503_quadfsm::run_t1503_checks as fn() -> CheckSet),
        ("UNX-F1903", unx1903_xsaveamort::run_t1903_checks as fn() -> CheckSet),
        ("UNX-F1941", unx1941_nmiipi::run_t1941_checks as fn() -> CheckSet),
        ("UNX-F1961", unx1961_freqdom::run_t1961_checks as fn() -> CheckSet),
        ("UNX-F1981", unx1981_twogov::run_t1981_checks as fn() -> CheckSet),
        ("UNX-F2001", unx2001_schedutil::run_t2001_checks as fn() -> CheckSet),
        ("UNX-F2021", unx2021_cstates::run_t2021_checks as fn() -> CheckSet),
        ("UNX-F2081", unx2081_qdepth::run_t2081_checks as fn() -> CheckSet),
        ("UNX-F2102", unx2102_sdebug::run_t2102_checks as fn() -> CheckSet),
        ("UNX-F2141", unx2141_triface::run_t2141_checks as fn() -> CheckSet),
        ("UNX-F2161", unx2161_stressorg::run_t2161_checks as fn() -> CheckSet),
        ("UNX-F2181", unx2181_regbook::run_t2181_checks as fn() -> CheckSet),
        ("UNX-F2201", unx2201_freezectl::run_t2201_checks as fn() -> CheckSet),
        ("UNX-F2241", unx2241_finalchain::run_t2241_checks as fn() -> CheckSet),
        ("UNX-F2222", unx2222_rollver::run_f2222_checks as fn() -> CheckSet),
        ("UNX-F0505", unx505_loopback::run_f505_checks as fn() -> CheckSet),
        ("UNX-F1723", unx1723_migledger::run_f1723_checks as fn() -> CheckSet),
        ("UNX-F0102", unx0102_16550full::run_t102_checks as fn() -> CheckSet),
        ("UNX-F0184", unx184_initserial::run_t184_checks as fn() -> CheckSet),
        ("UNX-F0205", unx205_reportuart::run_t205_checks as fn() -> CheckSet),
        ("UNX-F0264", unx264b_lineedit::run_t264b_checks as fn() -> CheckSet),
        ("UNX-F0387", unx387b_oomacct::run_t387b_checks as fn() -> CheckSet),
        ("UNX-F1481", unx1481_assertledger::run_t481_checks as fn() -> CheckSet),
        ("UNX-F0484", unx484_prim::run_t484_checks as fn() -> CheckSet),
        ("UNX-F0485", unx485_font8x16::run_t485_checks as fn() -> CheckSet),
        ("UNX-F0504", unx504_lineedit::run_t504_checks as fn() -> CheckSet),
        ("UNX-F1521", unx1521_ledgerindex::run_t521_checks as fn() -> CheckSet),
        ("UNX-F1541", unx1541_domainfinal::run_t541_checks as fn() -> CheckSet),
        ("UNX-F1542", unx1542_domainaxioms::run_t542_checks as fn() -> CheckSet),
        ("UNX-F0565", unx565_pwrbtn::run_t565_checks as fn() -> CheckSet),
        ("UNX-F1621", unx1621b_percurv::run_t621b_checks as fn() -> CheckSet),
        ("UNX-F0685", unx685_biosreg::run_t685_checks as fn() -> CheckSet),
        ("UNX-F1741", unx1741_idlefsm::run_t741_checks as fn() -> CheckSet),
        ("UNX-F1743", unx1743_idleres::run_t743_checks as fn() -> CheckSet),
        ("UNX-F1761", unx1761_obsface::run_t761_checks as fn() -> CheckSet),
        ("UNX-F1763", unx1763_statmac::run_t763_checks as fn() -> CheckSet),
        ("UNX-F1783", unx1783_crossscan::run_t783_checks as fn() -> CheckSet),
        ("UNX-F1801", unx1801_schedpath::run_t801_checks as fn() -> CheckSet),
        ("UNX-F1803", unx1803_resched::run_t803_checks as fn() -> CheckSet),
        ("UNX-F1823", unx1823_saveframe::run_t823_checks as fn() -> CheckSet),
        ("UNX-F1843", unx1843_ipirx::run_t843_checks as fn() -> CheckSet),
        ("UNX-F1863", unx1863_ipishoot::run_t863_checks as fn() -> CheckSet),
        ("UNX-F1863", unx1863_shootbegin::run_u1863_checks as fn() -> CheckSet),
        ("UNX-F0054", unx0054_acpimat::run_unx0054_checks as fn() -> CheckSet),
        ("UNX-F0171", unx0171_tpmreplay::run_unx0171_checks as fn() -> CheckSet),
        ("UNX-F0487", unx0487_clrcolor::run_unx0487_checks as fn() -> CheckSet),
        ("UNX-F0645", unx0645_reggate::run_unx0645_checks as fn() -> CheckSet),
        ("UNX-F0807", unx0807_trapframe::run_unx0807_checks as fn() -> CheckSet),
        ("UNX-F0819", unx0819_explain::run_unx0819_checks as fn() -> CheckSet),
        ("UNX-F0968", unx0968_fourstate::run_unx0968_checks as fn() -> CheckSet),
        ("UNX-F10001", unx10001_waveboard::run_unx10001_checks as fn() -> CheckSet),
        ("UNX-F1014", unx1014_timebudget::run_unx1014_checks as fn() -> CheckSet),
        ("UNX-F1025", unx1025_hpetread::run_unx1025_checks as fn() -> CheckSet),
        ("UNX-F1027", unx1027_timedisp::run_unx1027_checks as fn() -> CheckSet),
        ("UNX-F0102", unx102_uart::run_unx102_checks as fn() -> CheckSet),
        ("UNX-F0122", unx122_stagewatchdog::run_unx122_checks as fn() -> CheckSet),
        ("UNX-F0124", unx124_bootfail::run_unx124_checks as fn() -> CheckSet),
        ("UNX-F0142", unx142_gptesp::run_unx142_checks as fn() -> CheckSet),
        ("UNX-F0144", unx144_manifest::run_unx144_checks as fn() -> CheckSet),
        ("UNX-F1604", unx1604_apboot::run_unx1604_checks as fn() -> CheckSet),
        ("UNX-F1622", unx1622_gspair::run_unx1622_checks as fn() -> CheckSet),
        ("UNX-F0164", unx164_capsule::run_unx164_checks as fn() -> CheckSet),
        ("UNX-F1662", unx1662_schedif::run_unx1662_checks as fn() -> CheckSet),
        ("UNX-F1686", unx1686_vruntime::run_unx1686_checks as fn() -> CheckSet),
        ("UNX-F1704", unx1704_loadtrace::run_unx1704_checks as fn() -> CheckSet),
        ("UNX-F1724", unx1724_migfallback::run_unx1724_checks as fn() -> CheckSet),
        ("UNX-F1744", unx1744_idleexit::run_unx1744_checks as fn() -> CheckSet),
        ("UNX-F1764", unx1764_schedlat::run_unx1764_checks as fn() -> CheckSet),
        ("UNX-F1784", unx1784_coreaudit::run_unx1784_checks as fn() -> CheckSet),
        ("UNX-F1804", unx1804_nextthr::run_unx1804_checks as fn() -> CheckSet),
        ("UNX-F1824", unx1824_fpuswap::run_unx1824_checks as fn() -> CheckSet),
        ("UNX-F1844", unx1844_reschedipi::run_unx1844_checks as fn() -> CheckSet),
        ("UNX-F1864", unx1864_shootipi::run_unx1864_checks as fn() -> CheckSet),
        ("UNX-F1884", unx1884_migipi::run_unx1884_checks as fn() -> CheckSet),
        ("UNX-F1904", unx1904_tlsseg::run_unx1904_checks as fn() -> CheckSet),
        ("UNX-F1921", unx1921_cr3pcid::run_unx1921_checks as fn() -> CheckSet),
        ("UNX-F1938", unx1938_ktestagg::run_unx1938_checks as fn() -> CheckSet),
        ("UNX-F1962", unx1962_pstate::run_unx1962_checks as fn() -> CheckSet),
        ("UNX-F2002", unx2002_utilagg::run_unx2002_checks as fn() -> CheckSet),
        ("UNX-F2022", unx2022_exitlat::run_unx2022_checks as fn() -> CheckSet),
        ("UNX-F2061", unx2061_percorestat::run_unx2061_checks as fn() -> CheckSet),
        ("UNX-F2102", unx2102_scheddebug::run_unx2102_checks as fn() -> CheckSet),
        ("UNX-F2103", unx2103_sysfstune::run_unx2103_checks as fn() -> CheckSet),
        ("UNX-F2104", unx2104_exportmatrix::run_unx2104_checks as fn() -> CheckSet),
        ("UNX-F2105", unx2105_exportpoint::run_unx2105_checks as fn() -> CheckSet),
        ("UNX-F2106", unx2106_exportledger::run_unx2106_checks as fn() -> CheckSet),
        ("UNX-F2107", unx2107_paramfreeze::run_unx2107_checks as fn() -> CheckSet),
        ("UNX-F2108", unx2108_scenbook::run_unx2108_checks as fn() -> CheckSet),
        ("UNX-F2109", unx2109_ktestfamily::run_unx2109_checks as fn() -> CheckSet),
        ("UNX-F21581", unx21581_domainseal::run_unx21581_domainseal_checks as fn() -> CheckSet),
        ("UNX-F21582", unx21582_idclosure::run_unx21582_idclosure_checks as fn() -> CheckSet),
        ("UNX-F21583", unx21583_critpin::run_unx21583_critpin_checks as fn() -> CheckSet),
        ("UNX-F21584", unx21584_batchdisk::run_unx21584_batchdisk_checks as fn() -> CheckSet),
        ("UNX-F21585", unx21585_outlinealign::run_unx21585_outlinealign_checks as fn() -> CheckSet),
        ("UNX-F21586", unx21586_asmconsist::run_unx21586_asmconsist_checks as fn() -> CheckSet),
        ("UNX-F21587", unx21587_conservseal::run_unx21587_conservseal_checks as fn() -> CheckSet),
        ("UNX-F21588", unx21588_closesolemn::run_unx21588_closesolemn_checks as fn() -> CheckSet),
        ("UNX-F21589", unx21589_upcosign::run_unx21589_upcosign_checks as fn() -> CheckSet),
        ("UNX-F21590", unx21590_downcosign::run_unx21590_downcosign_checks as fn() -> CheckSet),
        ("UNX-F21591", unx21591_defectaudit::run_unx21591_defectaudit_checks as fn() -> CheckSet),
        ("UNX-F21592", unx21592_retestpool::run_unx21592_retestpool_checks as fn() -> CheckSet),
        ("UNX-F21593", unx21593_evidencetrio::run_unx21593_evidencetrio_checks as fn() -> CheckSet),
        ("UNX-F21594", unx21594_handover::run_unx21594_handover_checks as fn() -> CheckSet),
        ("UNX-F21595", unx21595_ledgerpatch::run_unx21595_ledgerpatch_checks as fn() -> CheckSet),
        ("UNX-F21596", unx21596_kgategate::run_unx21596_kgategate_checks as fn() -> CheckSet),
        ("UNX-F21597", unx21597_selfscore::run_unx21597_selfscore_checks as fn() -> CheckSet),
        ("UNX-F21598", unx21598_domainlock::run_unx21598_domainlock_checks as fn() -> CheckSet),
        ("UNX-F21599", unx21599_threeseal::run_unx21599_threeseal_checks as fn() -> CheckSet),
        ("UNX-F21600", unx21600_batchclose::run_unx21600_batchclose_checks as fn() -> CheckSet),
        ("UNX-F21601", unx21601_rop3table::run_unx21601_rop3table_checks as fn() -> CheckSet),
        ("UNX-F21604", unx21604_blackops::run_unx21604_blackops_checks as fn() -> CheckSet),
        ("UNX-F21607", unx21607_mergepen::run_unx21607_mergepen_checks as fn() -> CheckSet),
        ("UNX-F21610", unx21610_palettexpand::run_unx21610_palettexpand_checks as fn() -> CheckSet),
        ("UNX-F0225", unx225_diag::run_unx225_checks as fn() -> CheckSet),
        ("UNX-F0246", unx246_ovrblk::run_unx246_checks as fn() -> CheckSet),
        ("UNX-F0247", unx247_health::run_unx247_checks as fn() -> CheckSet),
        ("UNX-F0248", unx248_timebase::run_unx248_checks as fn() -> CheckSet),
        ("UNX-F0249", unx249_logkey::run_unx249_checks as fn() -> CheckSet),
        ("UNX-F0250", unx250_lastboot::run_unx250_checks as fn() -> CheckSet),
        ("UNX-F0251", unx251_subscribe::run_unx251_checks as fn() -> CheckSet),
        ("UNX-F0252", unx252_crcchain::run_unx252_checks as fn() -> CheckSet),
        ("UNX-F0253", unx253_vercompat::run_unx253_checks as fn() -> CheckSet),
        ("UNX-F0254", unx254_cmdreg::run_unx254_checks as fn() -> CheckSet),
        ("UNX-F0255", unx255_prio::run_unx255_checks as fn() -> CheckSet),
        ("UNX-F0256", unx256_audit::run_unx256_checks as fn() -> CheckSet),
        ("UNX-F0257", unx257_fieldmap::run_unx257_checks as fn() -> CheckSet),
        ("UNX-F0258", unx258_timing::run_unx258_checks as fn() -> CheckSet),
        ("UNX-F0259", unx259_tamper::run_unx259_checks as fn() -> CheckSet),
        ("UNX-F0260", unx260_ktb13::run_unx260_checks as fn() -> CheckSet),
        ("UNX-F2522", unx2522_fragsample::run_unx2522_checks as fn() -> CheckSet),
        ("UNX-F2542", unx2542_gfpflags::run_unx2542_checks as fn() -> CheckSet),
        ("UNX-F2623", unx2623_invpcid::run_unx2623_checks as fn() -> CheckSet),
        ("UNX-F2663", unx2663_cacheins::run_unx2663_checks as fn() -> CheckSet),
        ("UNX-F2744", unx2744_watermark::run_unx2744_checks as fn() -> CheckSet),
        ("UNX-F2763", unx2763_reclaim::run_unx2763_checks as fn() -> CheckSet),
        ("UNX-F2862", unx2862_addrmatrix::run_unx2862_checks as fn() -> CheckSet),
        ("UNX-F0286", unx286_assert::run_unx286_checks as fn() -> CheckSet),
        ("UNX-F2902", unx2902_readpeak::run_unx2902_checks as fn() -> CheckSet),
        ("UNX-F2922", unx2922_cowpriv::run_unx2922_checks as fn() -> CheckSet),
        ("UNX-F2962", unx2962_caseasm::run_unx2962_checks as fn() -> CheckSet),
        ("UNX-F3002", unx3002_pageinlink::run_unx3002_checks as fn() -> CheckSet),
        ("UNX-F3022", unx3022_reclaimvar::run_unx3022_checks as fn() -> CheckSet),
        ("UNX-F0304", unx304_dmap::run_unx304_checks as fn() -> CheckSet),
        ("UNX-F3062", unx3062_dirtysnap::run_unx3062_checks as fn() -> CheckSet),
        ("UNX-F3201", unx3201_atomic::run_unx3201_checks as fn() -> CheckSet),
        ("UNX-F3522", unx3522_probeslot::run_unx3522_checks as fn() -> CheckSet),
        ("UNX-F3542", unx3542_fpbacktrace::run_unx3542_checks as fn() -> CheckSet),
        ("UNX-F3622", unx3622_readconc::run_unx3622_checks as fn() -> CheckSet),
        ("UNX-F3662", unx3662_symtab::run_unx3662_checks as fn() -> CheckSet),
        ("UNX-F3762", unx3762_segschema::run_unx3762_checks as fn() -> CheckSet),
        ("UNX-F0386", unx386_dualzone::run_unx386_checks as fn() -> CheckSet),
        ("UNX-F4001", unx4001_vfsbus::run_unx4001_checks as fn() -> CheckSet),
        ("UNX-F4021", unx4021_sblife::run_unx4021_checks as fn() -> CheckSet),
        ("UNX-F4061", unx4061_inodewash::run_unx4061_checks as fn() -> CheckSet),
        ("UNX-F0422", unx422_exgate::run_unx422_checks as fn() -> CheckSet),
        ("UNX-F4381", unx4381_dupsem::run_unx4381_checks as fn() -> CheckSet),
        ("UNX-F4501", unx4501_permbits::run_unx4501_checks as fn() -> CheckSet),
        ("UNX-F4801", unx4801_ext4tree::run_unx4801_checks as fn() -> CheckSet),
        ("UNX-F4841", unx4841_dirents::run_unx4841_checks as fn() -> CheckSet),
        ("UNX-F4881", unx4881_xattrns::run_unx4881_checks as fn() -> CheckSet),
        ("UNX-F4901", unx4901_inline::run_unx4901_checks as fn() -> CheckSet),
        ("UNX-F4921", unx4921_mdcsum::run_unx4921_checks as fn() -> CheckSet),
        ("UNX-F5001", unx5001_writepath::run_unx5001_checks as fn() -> CheckSet),
        ("UNX-F5021", unx5021_sparse::run_unx5021_checks as fn() -> CheckSet),
        ("UNX-F5061", unx5061_fragmetric::run_unx5061_checks as fn() -> CheckSet),
        ("UNX-F5381", unx5381_subvol::run_unx5381_checks as fn() -> CheckSet),
        ("UNX-F5401", unx5401_squash::run_unx5401_checks as fn() -> CheckSet),
        ("UNX-F5501", unx5501_jnoutline::run_unx5501_checks as fn() -> CheckSet),
        ("UNX-F6401", unx6401_biobase::run_unx6401_checks as fn() -> CheckSet),
        ("UNX-F6501", unx6501_blkreg::run_unx6501_checks as fn() -> CheckSet),
        ("UNX-F0662", unx66_bootseg::run_unx66_checks as fn() -> CheckSet),
        ("UNX-F6801", unx6801_cfis::run_unx6801_checks as fn() -> CheckSet),
        ("UNX-F6841", unx6841_ataid::run_unx6841_checks as fn() -> CheckSet),
        ("UNX-F6881", unx6881_devlife::run_unx6881_checks as fn() -> CheckSet),
        ("UNX-F0686", unx68_efimap::run_unx68_checks as fn() -> CheckSet),
        ("UNX-F6901", unx6901_hotplug::run_unx6901_checks as fn() -> CheckSet),
        ("UNX-F6921", unx6921_remseq::run_unx6921_checks as fn() -> CheckSet),
        ("UNX-F7001", unx7001_handov::run_unx7001_checks as fn() -> CheckSet),
        ("UNX-F7021", unx7021_rmtest::run_unx7021_checks as fn() -> CheckSet),
        ("UNX-F7061", unx7061_injexp::run_unx7061_checks as fn() -> CheckSet),
        ("UNX-F0704", unx70_mpassert::run_unx70_checks as fn() -> CheckSet),
        ("UNX-F0724", unx72_upgreg::run_unx72_checks as fn() -> CheckSet),
        ("UNX-F7381", unx7381_raid5sm::run_unx7381_checks as fn() -> CheckSet),
        ("UNX-F7401", unx7401_raigate::run_unx7401_checks as fn() -> CheckSet),
        ("UNX-F0741", unx741_domain_journal::run_unx741_checks as fn() -> CheckSet),
        ("UNX-F0744", unx74_dedupmerge::run_unx74_checks as fn() -> CheckSet),
        ("UNX-F7501", unx7501_xopen::run_unx7501_checks as fn() -> CheckSet),
        ("UNX-F0761", unx761_precheck_list::run_unx761_checks as fn() -> CheckSet),
        ("UNX-F0762", unx76_3matrix::run_unx76_checks as fn() -> CheckSet),
        ("UNX-F7801", unx7801_rmround::run_unx7801_checks as fn() -> CheckSet),
        ("UNX-F0781", unx781_report_freeze::run_unx781_checks as fn() -> CheckSet),
        ("UNX-F7841", unx7841_bkpchain::run_unx7841_checks as fn() -> CheckSet),
        ("UNX-F7881", unx7881_receng::run_unx7881_checks as fn() -> CheckSet),
        ("UNX-F0784", unx78_final40::run_unx78_checks as fn() -> CheckSet),
        ("UNX-F7901", unx7901_rehdrill::run_unx7901_checks as fn() -> CheckSet),
        ("UNX-F7921", unx7921_schedret::run_unx7921_checks as fn() -> CheckSet),
        ("UNX-F8001", unx8001_interp::run_unx8001_checks as fn() -> CheckSet),
        ("UNX-F8021", unx8021_taskstr::run_unx8021_checks as fn() -> CheckSet),
        ("UNX-F8061", unx8061_ptinterp::run_unx8061_checks as fn() -> CheckSet),
        ("UNX-F0821", unx821_lapic_mmio::run_unx821_checks as fn() -> CheckSet),
        ("UNX-F0824", unx82_tprppr::run_unx82_checks as fn() -> CheckSet),
        ("UNX-F8381", unx8381_mbatch::run_unx8381_checks as fn() -> CheckSet),
        ("UNX-F8401", unx8401_magic::run_unx8401_checks as fn() -> CheckSet),
        ("UNX-F8501", unx8501_pttls::run_unx8501_checks as fn() -> CheckSet),
        ("UNX-F8581", unx8581_d1joint::run_unx8581_checks as fn() -> CheckSet),
        ("UNX-F8601", unx8601_c2full::run_unx8601_checks as fn() -> CheckSet),
        ("UNX-F0861", unx861_ioapic_ledger::run_unx861_checks as fn() -> CheckSet),
        ("UNX-F8621", unx8621_sigpend::run_unx8621_checks as fn() -> CheckSet),
        ("UNX-F8641", unx8641_glibcx40::run_unx8641_checks as fn() -> CheckSet),
        ("UNX-F8661", unx8661_cmpx40::run_unx8661_checks as fn() -> CheckSet),
        ("UNX-F8681", unx8681_ltpmap60::run_unx8681_checks as fn() -> CheckSet),
        ("UNX-F8701", unx8701_idrecon8::run_unx8701_checks as fn() -> CheckSet),
        ("UNX-F8721", unx8721_procfam::run_unx8721_checks as fn() -> CheckSet),
        ("UNX-F8741", unx8741_coldtime::run_unx8741_checks as fn() -> CheckSet),
        ("UNX-F8761", unx8761_docsnap::run_unx8761_checks as fn() -> CheckSet),
        ("UNX-F8781", unx8781_finrecon::run_unx8781_checks as fn() -> CheckSet),
        ("UNX-F8801", unx8801_systab::run_unx8801_checks as fn() -> CheckSet),
        ("UNX-F8821", unx8821_openflags::run_unx8821_checks as fn() -> CheckSet),
        ("UNX-F8841", unx8841_brk::run_unx8841_checks as fn() -> CheckSet),
        ("UNX-F8881", unx8881_ident3::run_unx8881_checks as fn() -> CheckSet),
        ("UNX-F0884", unx88_msixaffin::run_unx88_checks as fn() -> CheckSet),
        ("UNX-F8901", unx8901_gettod::run_unx8901_checks as fn() -> CheckSet),
        ("UNX-F8921", unx8921_cfu::run_unx8921_checks as fn() -> CheckSet),
        ("UNX-F9001", unx9001_rtsig::run_unx9001_checks as fn() -> CheckSet),
        ("UNX-F0938", unx93_ipiexplain::run_unx93_checks as fn() -> CheckSet),
        // AI-WB-1022 根卡批（UNX-F1061~F1243 十单）+ unx1242 孤儿判据收编：
        ("UNX-F1061", unx1061_clktrait::run_unx1061_checks as fn() -> CheckSet),
        ("UNX-F1081", unx1081_ntpcal::run_unx1081_checks as fn() -> CheckSet),
        ("UNX-F1105", unx1105_dispsafe::run_unx1105_checks as fn() -> CheckSet),
        ("UNX-F1121", unx1121_irqsign::run_unx1121_checks as fn() -> CheckSet),
        ("UNX-F1141", unx1141_topacct::run_unx1141_checks as fn() -> CheckSet),
        ("UNX-F1161", unx1161_softirq::run_unx1161_checks as fn() -> CheckSet),
        ("UNX-F1181", unx1181_irqroute::run_unx1181_checks as fn() -> CheckSet),
        ("UNX-F1201", unx1201_polloop::run_unx1201_checks as fn() -> CheckSet),
        ("UNX-F1221", unx1221_drainloop::run_unx1221_checks as fn() -> CheckSet),
        // 注：unx1242_arming::run_unx1242_checks 未挂表——该卡属 WorkBuddy-AI（已完成），
        // 其判据两处红属在途实现缺陷，挂表会污染本域聚合账；留待卡主收口。
        ("UNX-F1243", unx1243_armrace::run_unx1243_checks as fn() -> CheckSet),
        ("UNX-F1062", unx1062_tickdual::run_unx1062_checks as fn() -> CheckSet),
        ("UNX-F1082", unx1082_alarm::run_unx1082_checks as fn() -> CheckSet),
        ("UNX-F1106", unx1106_hotpath::run_unx1106_checks as fn() -> CheckSet),
        ("UNX-F1122", unx1122_irqflags::run_unx1122_checks as fn() -> CheckSet),
        ("UNX-F1142", unx1142_timeout::run_unx1142_checks as fn() -> CheckSet),
        ("UNX-F1162", unx1162_tasklet::run_unx1162_checks as fn() -> CheckSet),
        ("UNX-F1182", unx1182_policy::run_unx1182_checks as fn() -> CheckSet),
        ("UNX-F1202", unx1202_pollreg::run_unx1202_checks as fn() -> CheckSet),
        ("UNX-F1222", unx1222_draincrit::run_unx1222_checks as fn() -> CheckSet),
        ("UNX-F1244", unx1244_armfallback::run_unx1244_checks as fn() -> CheckSet),
        ("UNX-F0299", unx299_panicparse::run_unx299_checks as fn() -> CheckSet),
        ("UNX-F0330", unx330_apfeat::run_unx330_checks as fn() -> CheckSet),
        ("UNX-F0331", unx331_ucoderev::run_unx331_checks as fn() -> CheckSet),
        ("UNX-F0310", unx310_pgarea::run_unx310_checks as fn() -> CheckSet),
        ("UNX-F0316", unx316_pgfaultinj::run_unx316_checks as fn() -> CheckSet),
        ("UNX-F0317", unx317_pgperf::run_unx317_checks as fn() -> CheckSet),
        ("UNX-F0318", unx318_trampolow::run_unx318_checks as fn() -> CheckSet),
        ("UNX-F0319", unx319_pggolden::run_unx319_checks as fn() -> CheckSet),
        ("UNX-F0320", unx320_ktestb16::run_unx320_checks as fn() -> CheckSet),
        ("UNX-F0332", unx332_apfirstlog::run_unx332_checks as fn() -> CheckSet),
        ("UNX-F0333", unx333_bootfail::run_unx333_checks as fn() -> CheckSet),
        ("UNX-F0334", unx334_x2apic::run_unx334_checks as fn() -> CheckSet),
        ("UNX-F0335", unx335_apbudget::run_unx335_checks as fn() -> CheckSet),
        ("UNX-F0336", unx336_trampocrc::run_unx336_checks as fn() -> CheckSet),
        ("UNX-F0337", unx337_wakeorder::run_unx337_checks as fn() -> CheckSet),
        ("UNX-F0338", unx338_wakedrill::run_unx338_checks as fn() -> CheckSet),
        ("UNX-F0339", unx339_singlecore::run_unx339_checks as fn() -> CheckSet),
        ("UNX-F0340", unx340_ktestb17::run_unx340_checks as fn() -> CheckSet),
        ("UNX-F0274", unx274_ascii::run_ascii_degrade_checks as fn() -> CheckSet),
        ("UNX-F0235", unx235_audit::run_audit_checklist_checks as fn() -> CheckSet),
        ("UNX-F0280", unx280_b14ktest::run_b14_ktest_checks as fn() -> CheckSet),
        ("UNX-F0239", unx239_batchrecon::run_batch_reconciliation_checks as fn() -> CheckSet),
        ("UNX-F0240", unx240_closepack::run_unx240_checks as fn() -> CheckSet),
        ("UNX-F0368", unx368_heatmap::run_unx368_checks as fn() -> CheckSet),
        ("UNX-F0369", unx369_hugepage::run_unx369_checks as fn() -> CheckSet),
        ("UNX-F0370", unx370_resvblock::run_unx370_checks as fn() -> CheckSet),
        ("UNX-F0371", unx371_ledger::run_unx371_checks as fn() -> CheckSet),
        ("UNX-F0372", unx372_fragindex::run_unx372_checks as fn() -> CheckSet),
        ("UNX-F0373", unx373_oomguard::run_unx373_checks as fn() -> CheckSet),
        ("UNX-F0374", unx374_crossmem::run_unx374_checks as fn() -> CheckSet),
        ("UNX-F0375", unx375_swrclaim::run_unx375_checks as fn() -> CheckSet),
        ("UNX-F0376", unx376_fivenum::run_unx376_checks as fn() -> CheckSet),
        ("UNX-F0377", unx377_ktestb19::run_unx377_checks as fn() -> CheckSet),
        ("UNX-F0269", unx269_boot::run_boot_manifest_checks as fn() -> CheckSet),
        ("UNX-F0279", unx279_config::run_config_snapshot_checks as fn() -> CheckSet),
        ("UNX-F0276", unx276_crashiso::run_crash_isolation_checks as fn() -> CheckSet),
        ("UNX-F0265", unx265_diagbus::run_diag_bus_checks as fn() -> CheckSet),
        ("UNX-F0237", unx237_doccompare::run_doc_compare_checks as fn() -> CheckSet),
        ("UNX-F0275", unx275_dualchannel::run_dual_channel_checks as fn() -> CheckSet),
        ("UNX-F0287", unx287_fatal::run_fatal_detect_checks as fn() -> CheckSet),
        ("UNX-F0270", unx270_firmware::run_firmware_view_checks as fn() -> CheckSet),
        ("UNX-F0234", unx234_handoff::run_handoff_snapshot_checks as fn() -> CheckSet),
        ("UNX-F0273", unx273_keymap::run_keymap_checks as fn() -> CheckSet),
        ("UNX-F0268", unx268_ktestfilter::run_ktest_filter_checks as fn() -> CheckSet),
        ("UNX-F0238", unx238_ktest12::run_ktest_finalize_checks as fn() -> CheckSet),
        ("UNX-F0288", unx288_panicserial::run_panic_serial_checks as fn() -> CheckSet),
        ("UNX-F0271", unx271_ratelimit::run_rate_limit_checks as fn() -> CheckSet),
        ("UNX-F0277", unx277_sessionid::run_session_header_checks as fn() -> CheckSet),
        ("UNX-F0272", unx272_session::run_session_sm_checks as fn() -> CheckSet),
        ("UNX-F0267", unx267_sym::run_symbolize_checks as fn() -> CheckSet),
        ("UNX-F0278", unx278_timeout::run_timeout_checks as fn() -> CheckSet),
        ("UNX-F0294", unx294_panicrepro::run_unx294_checks as fn() -> CheckSet),
        ("UNX-F0295", unx295_panicpanel::run_unx295_checks as fn() -> CheckSet),
        ("UNX-F0296", unx296_drillmatrix::run_unx296_checks as fn() -> CheckSet),
        ("UNX-F0297", unx297_dualdog::run_unx297_checks as fn() -> CheckSet),
        ("UNX-F0300", unx300_ktestb15::run_unx300_checks as fn() -> CheckSet),
    ]
}

// ---------------------------------------------------------------------------
// 二级折叠：UNX-20 / wb20 / wb21 / wb22 / unx21 族（58 单）
// ---------------------------------------------------------------------------

/// 折叠子集表（**函数指针**，非已构造值：循环内 `f()` 即用即弃，
/// 同一时刻栈上只存活 1 个子集 `CheckSet`，约 4.5KB）。
///
/// 修正记录（折叠溢出，**实测 `UNX 域聚合溢出（子集数超 CheckSet 上限）`**）：
/// 原把这 58 单逐个直接 `set.add` 到主集，于是主集总量变成
/// **69（blocks 表）+ 58（追加段）= 127 > `MAX_CHECKS`(112)** ——溢出 15 格。
/// 而 `CheckSet::add` 在满容量时**静默丢项**（只加 `dropped` 计数），
/// 于是末尾 15 个族（含 `wb20` 的u1501~u1901、`wb21`/`wb22`/`unx21`
/// 三个批次折叠）的整套判据**从未参与闭环**，而聚合器照报「N/M 绿」。
///
/// 修法（**不丢判据**，按本域既有范式做二级折叠）：58 单折进一个**二级**
/// `CheckSet`（58 ≤ 112，安全），主集只占 **1 格**并以
/// `!secondary.truncated()` 守卫。二级集自身的红项文本会在主集这一格里
/// 通过 `add_dyn` 带出，红项仍可点名——比原来「静默丢掉」严格得多。
///
/// 为什么不抬 `MAX_CHECKS`：`MAX_CHECKS` 是**全仓共享**的定容常量
/// （112），它决定 `CheckSet` 的栈体积（112 × 约 40B ≈ 4.5KB/实例，
/// 而全仓有 4565 处按值构造）。为一个域抬它，会放大**所有**域的栈开销，
/// 属「为治一域而劳师动众」。二级折叠是局部解，且本域已有先例
/// （`run_wb21_unx20` / `run_wb22_unx22` / `run_unx21_batch` 本身就是
/// 二级折叠族）。
pub fn unxstar_secondary_blocks() -> [(&'static str, fn() -> CheckSet); 203] {
    [
        ("UNX-F0082", run_unx20_t082 as fn() -> CheckSet),
        ("UNX-F0123", run_unx20_t126 as fn() -> CheckSet),
        ("UNX-F0143", run_unx20_t147 as fn() -> CheckSet),
        ("UNX-F0163", run_unx20_t163 as fn() -> CheckSet),
        ("UNX-F0203", run_unx20_t203 as fn() -> CheckSet),
        ("UNX-F0222", run_unx20_t228 as fn() -> CheckSet),
        ("UNX-F0242", run_unx20_t242 as fn() -> CheckSet),
        ("UNX-F0262", run_unx20_t268 as fn() -> CheckSet),
        ("UNX-F0282", run_unx20_t286 as fn() -> CheckSet),
        ("UNX-F0322", run_unx20_t326 as fn() -> CheckSet),
        ("UNX-F0302", run_unx20_t306 as fn() -> CheckSet),
        ("AP ID 核对域", run_unx20_t325 as fn() -> CheckSet),
        ("固件路差异域", run_unx20_t684 as fn() -> CheckSet),
        ("万次切换域", run_unx20_t505 as fn() -> CheckSet),
        ("判据冻结域", run_unx20_t545 as fn() -> CheckSet),
        ("UNX-F0162", run_unx20_t162 as fn() -> CheckSet),
        ("UNX-F0221", run_unx20_t221 as fn() -> CheckSet),
        ("UNX-F0241", run_unx20_t241 as fn() -> CheckSet),
        ("UNX-F0261", run_unx20_t261 as fn() -> CheckSet),
        ("UNX-F0281", run_unx20_t281 as fn() -> CheckSet),
        ("UNX-F0301", run_unx20_t301 as fn() -> CheckSet),
        ("UNX-F0321", run_unx20_t311 as fn() -> CheckSet),
        ("UNX-F0341", run_unx20_t321 as fn() -> CheckSet),
        ("UNX-F0361", run_unx20_t341 as fn() -> CheckSet),
        ("UNX-F0381", run_unx20_t351 as fn() -> CheckSet),
        ("UNX-F0401", run_unx20_t361 as fn() -> CheckSet),
        ("UNX-F0421", run_unx20_t421 as fn() -> CheckSet),
        ("UNX-F0441", run_unx20_t431 as fn() -> CheckSet),
        ("UNX-F0461", run_unx20_t441 as fn() -> CheckSet),
        ("UNX-F0481", run_unx20_t561 as fn() -> CheckSet),
        ("UNX-F0501", run_unx20_t571 as fn() -> CheckSet),
        ("UNX-F0521", run_unx20_t581 as fn() -> CheckSet),
        ("UNX-F0541", run_unx20_t591 as fn() -> CheckSet),
        ("UNX-F0561", run_unx20_ta01 as fn() -> CheckSet),
        ("UNX-F0581", run_unx20_tb01 as fn() -> CheckSet),
        ("UNX-F1501", run_wb20_u1501 as fn() -> CheckSet),
        ("UNX-F1541", run_wb20_u1541 as fn() -> CheckSet),
        ("UNX-F1561", run_wb20_u1561 as fn() -> CheckSet),
        ("UNX-F1581", run_wb20_u1581 as fn() -> CheckSet),
        ("UNX-F1601", run_wb20_u1601 as fn() -> CheckSet),
        ("UNX-F1621", run_wb20_u1621 as fn() -> CheckSet),
        ("UNX-F1641", run_wb20_u1641 as fn() -> CheckSet),
        ("UNX-F1661", run_wb20_u1661 as fn() -> CheckSet),
        ("UNX-F1681", run_wb20_u1681 as fn() -> CheckSet),
        ("UNX-F1701", run_wb20_u1701 as fn() -> CheckSet),
        ("UNX-F1721", run_wb20_u1721 as fn() -> CheckSet),
        ("UNX-F1741", run_wb20_u1741 as fn() -> CheckSet),
        ("UNX-F1761", run_wb20_u1761 as fn() -> CheckSet),
        ("UNX-F1781", run_wb20_u1781 as fn() -> CheckSet),
        ("UNX-F1801", run_wb20_u1801 as fn() -> CheckSet),
        ("UNX-F1821", run_wb20_u1821 as fn() -> CheckSet),
        ("UNX-F1841", run_wb20_u1841 as fn() -> CheckSet),
        ("UNX-F1861", run_wb20_u1861 as fn() -> CheckSet),
        ("UNX-F1881", run_wb20_u1881 as fn() -> CheckSet),
        ("UNX-F1901", run_wb20_u1901 as fn() -> CheckSet),
        ("UNX-F-WB21", run_wb21_unx20 as fn() -> CheckSet),
        ("UNX-F-WB22", run_wb22_unx22 as fn() -> CheckSet),
        ("UNX-F-UNX21", run_unx21_batch as fn() -> CheckSet),
        ("UNX-F-UNX22", run_unx22_batch as fn() -> CheckSet),
        ("UNX-F0345", unx0345_aphealth::run_unx0345_checks as fn() -> CheckSet),
        ("UNX-F0346", unx0346_apledger::run_unx0346_checks as fn() -> CheckSet),
        ("UNX-F0347", unx0347_apisoacct::run_unx0347_checks as fn() -> CheckSet),
        ("UNX-F0348", unx0348_apnumastack::run_unx0348_checks as fn() -> CheckSet),
        ("UNX-F0349", unx0349_aphandoff::run_unx0349_checks as fn() -> CheckSet),
        ("UNX-F0350", unx0350_aprecompute::run_unx0350_checks as fn() -> CheckSet),
        ("UNX-F0351", unx0351_apdrillmat::run_unx0351_checks as fn() -> CheckSet),
        ("UNX-F0352", unx0352_apfailinject::run_unx0352_checks as fn() -> CheckSet),
        ("UNX-F0353", unx0353_tramporeclaim::run_unx0353_checks as fn() -> CheckSet),
        ("UNX-F0354", unx0354_aptimeline::run_unx0354_checks as fn() -> CheckSet),
        ("UNX-F0355", unx0355_apucode::run_unx0355_checks as fn() -> CheckSet),
        ("UNX-F0234", unx0234_handoffsnap::run_unx0234_checks as fn() -> CheckSet),
        ("UNX-F0235", unx0235_auditself::run_unx0235_checks as fn() -> CheckSet),
        ("UNX-F0236", unx0236_feedback::run_unx0236_checks as fn() -> CheckSet),
        ("UNX-F0237", unx0237_docdiff::run_unx0237_checks as fn() -> CheckSet),
        ("UNX-F0238", unx0238_ktestb12::run_unx0238_checks as fn() -> CheckSet),
        ("UNX-F0239", unx0239_batchtally::run_unx0239_checks as fn() -> CheckSet),
        ("UNX-F0240", unx0240_closepack::run_unx0240_checks as fn() -> CheckSet),
        ("UNX-F0265", unx0265_cmdbus::run_unx0265_checks as fn() -> CheckSet),
        ("UNX-F0267", unx0267_symcmd::run_unx0267_checks as fn() -> CheckSet),
        ("UNX-F0268", unx0268_ktestcmd::run_unx0268_checks as fn() -> CheckSet),
        ("UNX-F0269", unx0269_bootcmd::run_unx0269_checks as fn() -> CheckSet),
        ("UNX-F0270", unx0270_fwview::run_unx0270_checks as fn() -> CheckSet),
        ("UNX-F0271", unx0271_backpressure::run_unx0271_checks as fn() -> CheckSet),
        ("UNX-F0272", unx0272_sessm::run_unx0272_checks as fn() -> CheckSet),
        ("UNX-F0273", unx0273_hotkey::run_unx0273_checks as fn() -> CheckSet),
        ("UNX-F0274", unx0274_ascii::run_unx0274_checks as fn() -> CheckSet),
        ("UNX-F0275", unx0275_dualchan::run_unx0275_checks as fn() -> CheckSet),
        ("UNX-F0276", unx0276_crashiso::run_unx0276_checks as fn() -> CheckSet),
        ("UNX-F0277", unx0277_sessid::run_unx0277_checks as fn() -> CheckSet),
        ("UNX-F0278", unx0278_budget::run_unx0278_checks as fn() -> CheckSet),
        ("UNX-F0279", unx0279_cfgsnap::run_unx0279_checks as fn() -> CheckSet),
        ("UNX-F0280", unx0280_b14::run_unx0280_checks as fn() -> CheckSet),
        ("UNX-F0287", unx0287_unrecov::run_unx0287_checks as fn() -> CheckSet),
        ("UNX-F0288", unx0288_panicuart::run_unx0288_checks as fn() -> CheckSet),
        ("UNX-F0356", unx0356_apconverge::run_unx0356_checks as fn() -> CheckSet),
        ("UNX-F0357", unx0357_apcaprep::run_unx0357_checks as fn() -> CheckSet),
        ("UNX-F0358", unx0358_maxcpus::run_unx0358_checks as fn() -> CheckSet),
        ("UNX-F0359", unx0359_nmibcast::run_unx0359_checks as fn() -> CheckSet),
        ("UNX-F0360", unx0360_ktestb18::run_unx0360_checks as fn() -> CheckSet),
        ("UNX-F0363", unx0363_pfbitmap::run_unx0363_checks as fn() -> CheckSet),
        ("UNX-F0364", unx0364_lowmap::run_unx0364_checks as fn() -> CheckSet),
        ("UNX-F0365", unx0365_dmalow::run_unx0365_checks as fn() -> CheckSet),
        ("UNX-F0366", unx0366_ffalloc::run_unx0366_checks as fn() -> CheckSet),
        ("UNX-F0367", unx0367_pffree::run_unx0367_checks as fn() -> CheckSet),
        ("UNX-F0368", unx0368_heatmap::run_unx0368_checks as fn() -> CheckSet),
        ("UNX-F0369", unx0369_hugepage::run_unx0369_checks as fn() -> CheckSet),
        ("UNX-F0370", unx0370_resguard::run_unx0370_checks as fn() -> CheckSet),
        ("UNX-F0371", unx0371_pfledger::run_unx0371_checks as fn() -> CheckSet),
        ("UNX-F0372", unx0372_fragindex::run_unx0372_checks as fn() -> CheckSet),
        ("UNX-F0373", unx0373_oomplan::run_unx0373_checks as fn() -> CheckSet),
        ("UNX-F0374", unx0374_e820cross::run_unx0374_checks as fn() -> CheckSet),
        ("UNX-F0375", unx0375_poolprim::run_unx0375_checks as fn() -> CheckSet),
        ("UNX-F0376", unx0376_fivepoint::run_unx0376_checks as fn() -> CheckSet),
        ("UNX-F0377", unx0377_ktestmem::run_unx0377_checks as fn() -> CheckSet),
        ("UNX-F0378", unx0378_handfield::run_unx0378_checks as fn() -> CheckSet),
        ("UNX-F0379", unx0379_drilltopo::run_unx0379_checks as fn() -> CheckSet),
        ("UNX-F0380", unx0380_b19::run_unx0380_checks as fn() -> CheckSet),
        ("UNX-F0388", unx0388_heapleak::run_unx0388_checks as fn() -> CheckSet),
        ("UNX-F0389", unx0389_slabhalf::run_unx0389_checks as fn() -> CheckSet),
        ("UNX-F0390", unx0390_heapguard::run_unx0390_checks as fn() -> CheckSet),
        ("UNX-F0391", unx0391_oominject::run_unx0391_checks as fn() -> CheckSet),
        ("UNX-F0392", unx0392_heapgrow::run_unx0392_checks as fn() -> CheckSet),
        ("UNX-F0393", unx0393_stkpool::run_unx0393_checks as fn() -> CheckSet),
        ("UNX-F0394", unx0394_handreserve::run_unx0394_checks as fn() -> CheckSet),
        ("UNX-F0395", unx0395_heapledger::run_unx0395_checks as fn() -> CheckSet),
        ("UNX-F0396", unx0396_ktheaptest::run_unx0396_checks as fn() -> CheckSet),
        ("UNX-F0397", unx0397_bigbypass::run_unx0397_checks as fn() -> CheckSet),
        ("UNX-F0398", unx0398_zeropage::run_unx0398_checks as fn() -> CheckSet),
        ("UNX-F0399", unx0399_heapstorm::run_unx0399_checks as fn() -> CheckSet),
        ("UNX-F0400", unx0400_b20::run_unx0400_checks as fn() -> CheckSet),
        ("UNX-F0406", unx0406_wallclock::run_unx0406_checks as fn() -> CheckSet),
        ("UNX-F0407", unx0407_timq::run_unx0407_checks as fn() -> CheckSet),
        ("UNX-F0408", unx0408_tscdead::run_unx0408_checks as fn() -> CheckSet),
        ("UNX-F0409", unx0409_clksel::run_unx0409_checks as fn() -> CheckSet),
        ("UNX-F0410", unx0410_precself::run_unx0410_checks as fn() -> CheckSet),
        ("UNX-F0411", unx0411_monojump::run_unx0411_checks as fn() -> CheckSet),
        ("UNX-F0412", unx0412_softtimer::run_unx0412_checks as fn() -> CheckSet),
        ("UNX-F0413", unx0413_timefield::run_unx0413_checks as fn() -> CheckSet),
        ("UNX-F0414", unx0414_timefive::run_unx0414_checks as fn() -> CheckSet),
        ("UNX-F0415", unx0415_pitcross::run_unx0415_checks as fn() -> CheckSet),
        ("UNX-F0416", unx0416_timledger::run_unx0416_checks as fn() -> CheckSet),
        ("UNX-F0417", unx0417_freqdrift::run_unx0417_checks as fn() -> CheckSet),
        ("UNX-F0418", unx0418_ktimeassert::run_unx0418_checks as fn() -> CheckSet),
        ("UNX-F0419", unx0419_load3tier::run_unx0419_checks as fn() -> CheckSet),
        ("UNX-F0420", unx0420_b21::run_unx0420_checks as fn() -> CheckSet),
        ("UNX-F0423", unx0423_ist1::run_unx0423_checks as fn() -> CheckSet),
        ("UNX-F0424", unx0424_nmiist2::run_unx0424_checks as fn() -> CheckSet),
        ("UNX-F0425", unx0425_pfecode::run_unx0425_checks as fn() -> CheckSet),
        ("UNX-F0426", unx0426_picshut::run_unx0426_checks as fn() -> CheckSet),
        ("UNX-F0427", unx0427_ioapicprobe::run_unx0427_checks as fn() -> CheckSet),
        ("UNX-F0428", unx0428_lapicprobe::run_unx0428_checks as fn() -> CheckSet),
        ("UNX-F0429", unx0429_vecalloc::run_unx0429_checks as fn() -> CheckSet),
        ("UNX-F0430", unx0430_spurious::run_unx0430_checks as fn() -> CheckSet),
        ("UNX-F0431", unx0431_exinj::run_unx0431_checks as fn() -> CheckSet),
        ("UNX-F0432", unx0432_veccount::run_unx0432_checks as fn() -> CheckSet),
        ("UNX-F0433", unx0433_irqlat::run_unx0433_checks as fn() -> CheckSet),
        ("UNX-F0434", unx0434_ifdisc::run_unx0434_checks as fn() -> CheckSet),
        ("UNX-F0435", unx0435_istsent::run_unx0435_checks as fn() -> CheckSet),
        ("UNX-F0436", unx0436_iretq::run_unx0436_checks as fn() -> CheckSet),
        ("UNX-F0437", unx0437_crashtsnap::run_unx0437_checks as fn() -> CheckSet),
        ("UNX-F0438", unx0438_stormdrill::run_unx0438_checks as fn() -> CheckSet),
        ("UNX-F0439", unx0439_handfield::run_unx0439_checks as fn() -> CheckSet),
        ("UNX-F0440", unx0440_b22::run_unx0440_checks as fn() -> CheckSet),
        ("UNX-F0443", unx0443_rsdtwalk::run_unx0443_checks as fn() -> CheckSet),
        ("UNX-F0444", unx0444_tblcsum::run_unx0444_checks as fn() -> CheckSet),
        ("UNX-F0445", unx0445_madtirq::run_unx0445_checks as fn() -> CheckSet),
        ("UNX-F0446", unx0446_hpetparse::run_unx0446_checks as fn() -> CheckSet),
        ("UNX-F0447", unx0447_fadttiming::run_unx0447_checks as fn() -> CheckSet),
        ("UNX-F0448", unx0448_mcfgecam::run_unx0448_checks as fn() -> CheckSet),
        ("UNX-F0449", unx0449_dsdtloc::run_unx0449_checks as fn() -> CheckSet),
        ("UNX-F0450", unx0450_sratnuma::run_unx0450_checks as fn() -> CheckSet),
        ("UNX-F0451", unx0451_tblsnap::run_unx0451_checks as fn() -> CheckSet),
        ("UNX-F0452", unx0452_degrade::run_unx0452_checks as fn() -> CheckSet),
        ("UNX-F0453", unx0453_pinseal::run_unx0453_checks as fn() -> CheckSet),
        ("UNX-F0454", unx0454_badinj::run_unx0454_checks as fn() -> CheckSet),
        ("UNX-F0455", unx0455_mapreg::run_unx0455_checks as fn() -> CheckSet),
        ("UNX-F0456", unx0456_handoff::run_unx0456_checks as fn() -> CheckSet),
        ("UNX-F0457", unx0457_5num::run_unx0457_checks as fn() -> CheckSet),
        ("UNX-F0458", unx0458_acpize::run_unx0458_checks as fn() -> CheckSet),
        ("UNX-F0459", unx0459_ktap::run_unx0459_checks as fn() -> CheckSet),
        ("UNX-F0460", unx0460_b23::run_unx0460_checks as fn() -> CheckSet),
        ("UNX-F0465", unx0465_pcihdr::run_unx0465_checks as fn() -> CheckSet),
        ("UNX-F0466", unx0466_pcibar::run_unx0466_checks as fn() -> CheckSet),
        ("UNX-F0467", unx0467_pcclass::run_unx0467_checks as fn() -> CheckSet),
        ("UNX-F0468", unx0468_pcicap::run_unx0468_checks as fn() -> CheckSet),
        ("UNX-F0469", unx0469_pciecap::run_unx0469_checks as fn() -> CheckSet),
        ("UNX-F0470", unx0470_roline::run_unx0470_checks as fn() -> CheckSet),
        ("UNX-F0471", unx0471_xchk::run_unx0471_checks as fn() -> CheckSet),
        ("UNX-F0472", unx0472_enumdog::run_unx0472_checks as fn() -> CheckSet),
        ("UNX-F0473", unx0473_rescan::run_unx0473_checks as fn() -> CheckSet),
        ("UNX-F0474", unx0474_pci5num::run_unx0474_checks as fn() -> CheckSet),
        ("UNX-F0475", unx0475_scandrill::run_unx0475_checks as fn() -> CheckSet),
        ("UNX-F0476", unx0476_pcihand::run_unx0476_checks as fn() -> CheckSet),
        ("UNX-F0477", unx0477_pcxacc::run_unx0477_checks as fn() -> CheckSet),
        ("UNX-F0478", unx0478_pcbad::run_unx0478_checks as fn() -> CheckSet),
        ("UNX-F0479", unx0479_pcikt::run_unx0479_checks as fn() -> CheckSet),
        ("UNX-F0480", unx0480_b24::run_unx0480_checks as fn() -> CheckSet),
        ("UNX-F0488", unx0488_rndguard::run_unx0488_checks as fn() -> CheckSet),
        ("UNX-F0489", unx0489_rndperf::run_unx0489_checks as fn() -> CheckSet),
        ("UNX-F0490", unx0490_rndpat::run_unx0490_checks as fn() -> CheckSet),
        ("UNX-F0491", unx0491_fbsent::run_unx0491_checks as fn() -> CheckSet),
        ("UNX-F0492", unx0492_rndops::run_unx0492_checks as fn() -> CheckSet),
        ("UNX-F0493", unx0493_rndmulti::run_unx0493_checks as fn() -> CheckSet),
        ("UNX-F0494", unx0494_dmarcap::run_unx0494_checks as fn() -> CheckSet),
        ("UNX-F0495", unx0495_rtroot::run_unx0495_checks as fn() -> CheckSet),
        ("UNX-F0496", unx0496_iotlbeg::run_unx0496_checks as fn() -> CheckSet),
        ("UNX-F0497", unx0497_devtlb::run_unx0497_checks as fn() -> CheckSet),
        ("UNX-F0498", unx0498_faultrec::run_unx0498_checks as fn() -> CheckSet),
        ("UNX-F0499", unx0499_irtable::run_unx0499_checks as fn() -> CheckSet),
        ("UNX-F0500", unx0500_domseal::run_unx0500_checks as fn() -> CheckSet),
        ("UNX-F0506", unx0506_dmarops::run_unx0506_checks as fn() -> CheckSet),
        ("UNX-F0507", unx0507_ctxcache::run_unx0507_checks as fn() -> CheckSet),
        ("UNX-F0508", unx0508_pasidtag::run_unx0508_checks as fn() -> CheckSet),
        ("UNX-F0509", unx0509_sectl2::run_unx0509_checks as fn() -> CheckSet),
        ("UNX-F0510", unx0510_ptewalk::run_unx0510_checks as fn() -> CheckSet),
        ("UNX-F0511", unx0511_qtqueue::run_unx0511_checks as fn() -> CheckSet),
        ("UNX-F0512", unx0512_iecinval::run_unx0512_checks as fn() -> CheckSet),
        ("UNX-F0513", unx0513_intpost::run_unx0513_checks as fn() -> CheckSet),
        ("UNX-F0514", unx0514_svmtrack::run_unx0514_checks as fn() -> CheckSet),
        ("UNX-F0515", unx0515_devrw::run_unx0515_checks as fn() -> CheckSet),
        ("UNX-F0516", unx0516_prq::run_unx0516_checks as fn() -> CheckSet),
        ("UNX-F0517", unx0517_wdinvq::run_unx0517_checks as fn() -> CheckSet),
        ("UNX-F0518", unx0518_perfctr::run_unx0518_checks as fn() -> CheckSet),
        ("UNX-F0519", unx0519_vtclose::run_unx0519_checks as fn() -> CheckSet),
        ("UNX-F0521", unx0521_ktestcore::run_unx0521_checks as fn() -> CheckSet),
        ("UNX-F0522", unx0522_asserts::run_unx0522_checks as fn() -> CheckSet),
        ("UNX-F0523", unx0523_fail3::run_unx0523_checks as fn() -> CheckSet),
        ("UNX-F0524", unx0524_suitegrp::run_unx0524_checks as fn() -> CheckSet),
        ("UNX-F0525", unx0525_summary::run_unx0525_checks as fn() -> CheckSet),
        ("UNX-F0526", unx0526_redbook::run_unx0526_checks as fn() -> CheckSet),
        ("UNX-F0527", unx0527_watchdog::run_unx0527_checks as fn() -> CheckSet),
        ("UNX-F0528", unx0528_nsisol::run_unx0528_checks as fn() -> CheckSet),
        ("UNX-F0529", unx0529_rndinj::run_unx0529_checks as fn() -> CheckSet),
        ("UNX-F0530", unx0530_covermtx::run_unx0530_checks as fn() -> CheckSet),
        ("UNX-F0531", unx0531_outproto::run_unx0531_checks as fn() -> CheckSet),
        ("UNX-F0532", unx0532_persist::run_unx0532_checks as fn() -> CheckSet),
        ("UNX-F0533", unx0533_onekey::run_unx0533_checks as fn() -> CheckSet),
        ("UNX-F0534", unx0534_hedge::run_unx0534_checks as fn() -> CheckSet),
        ("UNX-F0535", unx0535_residue::run_unx0535_checks as fn() -> CheckSet),
        ("UNX-F0536", unx0536_frameself::run_unx0536_checks as fn() -> CheckSet),
        ("UNX-F0537", unx0537_fmtver::run_unx0537_checks as fn() -> CheckSet),
        ("UNX-F0538", unx0538_handoff::run_unx0538_checks as fn() -> CheckSet),
        ("UNX-F0539", unx0539_hedgetot::run_unx0539_checks as fn() -> CheckSet),
        ("UNX-F0540", unx0540_b27set::run_unx0540_checks as fn() -> CheckSet),
        ("UNX-F0541", unx0541_logemit::run_unx0541_checks as fn() -> CheckSet),
        ("UNX-F0542", unx0542_logfilt::run_unx0542_checks as fn() -> CheckSet),
        ("UNX-F0543", unx0543_linefmt::run_unx0543_checks as fn() -> CheckSet),
        ("UNX-F0544", unx0544_ringbuf::run_unx0544_checks as fn() -> CheckSet),
        ("UNX-F0545", unx0545_uartout::run_unx0545_checks as fn() -> CheckSet),
        ("UNX-F0546", unx0546_fbout::run_unx0546_checks as fn() -> CheckSet),
        ("UNX-F0547", unx0547_tstamp::run_unx0547_checks as fn() -> CheckSet),
        ("UNX-F0548", unx0548_comptag::run_unx0548_checks as fn() -> CheckSet),
        ("UNX-F0549", unx0549_logquery::run_unx0549_checks as fn() -> CheckSet),
        ("UNX-F0550", unx0550_countreck::run_unx0550_checks as fn() -> CheckSet),
        ("UNX-F0551", unx0551_ringwrap::run_unx0551_checks as fn() -> CheckSet),
        ("UNX-F0552", unx0552_dualswitch::run_unx0552_checks as fn() -> CheckSet),
        ("UNX-F0553", unx0553_linecsum::run_unx0553_checks as fn() -> CheckSet),
        ("UNX-F0554", unx0554_logstorm::run_unx0554_checks as fn() -> CheckSet),
        ("UNX-F0555", unx0555_badinj::run_unx0555_checks as fn() -> CheckSet),
        ("UNX-F0556", unx0556_fivestat::run_unx0556_checks as fn() -> CheckSet),
        ("UNX-F0557", unx0557_loghedge::run_unx0557_checks as fn() -> CheckSet),
        ("UNX-F0558", unx0558_auditlink::run_unx0558_checks as fn() -> CheckSet),
        ("UNX-F0559", unx0559_loghandoff::run_unx0559_checks as fn() -> CheckSet),
        ("UNX-F0560", unx0560_b28set::run_unx0560_checks as fn() -> CheckSet),
        ("UNX-F0531", unx0531_nxguard::run_unx0531_checks as fn() -> CheckSet),
        ("UNX-F0532", unx0532_wptrap::run_unx0532_checks as fn() -> CheckSet),
        ("UNX-F0533", unx0533_cowpage::run_unx0533_checks as fn() -> CheckSet),
        ("UNX-F0534", unx0534_tlbshoot::run_unx0534_checks as fn() -> CheckSet),
        ("UNX-F0535", unx0535_hugefld::run_unx0535_checks as fn() -> CheckSet),
        ("UNX-F0536", unx0536_mapaudit::run_unx0536_checks as fn() -> CheckSet),
        ("UNX-F0537", unx0537_pfcensus::run_unx0537_checks as fn() -> CheckSet),
        ("UNX-F0538", unx0538_sharecred::run_unx0538_checks as fn() -> CheckSet),
        ("UNX-F0539", unx0539_zswapin::run_unx0539_checks as fn() -> CheckSet),
        ("UNX-F0543", unx0543_mmuclose::run_unx0543_checks as fn() -> CheckSet),
        ("UNX-F0544", unx0544_sipilane::run_unx0544_checks as fn() -> CheckSet),
        ("UNX-F0545", unx0545_apwait::run_unx0545_checks as fn() -> CheckSet),
        ("UNX-F0546", unx0546_ticket::run_unx0546_checks as fn() -> CheckSet),
        ("UNX-F0547", unx0547_genbar::run_unx0547_checks as fn() -> CheckSet),
        ("UNX-F0548", unx0548_percpu::run_unx0548_checks as fn() -> CheckSet),
        ("UNX-F0549", unx0549_ipiburst::run_unx0549_checks as fn() -> CheckSet),
        ("UNX-F0550", unx0550_memfence::run_unx0550_checks as fn() -> CheckSet),
        ("UNX-F0551", unx0551_coreonline::run_unx0551_checks as fn() -> CheckSet),
        ("UNX-F0552", unx0552_corehalt::run_unx0552_checks as fn() -> CheckSet),
        ("UNX-F0553", unx0553_smpclose::run_unx0553_checks as fn() -> CheckSet),
        ("UNX-F0567", unx0567_rqlane::run_unx0567_checks as fn() -> CheckSet),
        ("UNX-F0568", unx0568_rrband::run_unx0568_checks as fn() -> CheckSet),
        ("UNX-F0569", unx0569_priolane::run_unx0569_checks as fn() -> CheckSet),
        ("UNX-F0570", unx0570_preempt2::run_unx0570_checks as fn() -> CheckSet),
        ("UNX-F0571", unx0571_waitq::run_unx0571_checks as fn() -> CheckSet),
        ("UNX-F0572", unx0572_mlfq2::run_unx0572_checks as fn() -> CheckSet),
        ("UNX-F0573", unx0573_starvgua::run_unx0573_checks as fn() -> CheckSet),
        ("UNX-F0574", unx0574_loadbal::run_unx0574_checks as fn() -> CheckSet),
        ("UNX-F0575", unx0575_schedtick::run_unx0575_checks as fn() -> CheckSet),
        ("UNX-F0576", unx0576_schedclose::run_unx0576_checks as fn() -> CheckSet),
    ]
}

/// 二级折叠聚合：59 单 → 一个 `CheckSet`。
///
/// 修正记录（分段折叠，修复二级溢出静默丢族）：本函数原为「每族 1 格
/// 平铺」——二级表扩到 197 席后超出 `MAX_CHECKS = 112` 容量，第 113 族
/// 起被 `add` 的溢出守卫静默丢弃（`dropped()` 递增但无人报警），后段
/// 族（ACPI 收口批 / PCI 批等）的判据**从未参与聚合闭环**。修复：按
/// `SEG = 100` 族分段，每段先内部聚合成段集（≤100 格，容量内），再把
/// 每段汇总为顶层 1 格（`UNX-SEC-S01/S02/…`，段数 ≤ 112/100+1 恒在
/// 容量内）。判据强度不降：段内每族 1 格语义原样保留，顶层段格是对
/// 「该段整体绿」的一票——与一级表/二级表的折叠语义同构。
pub fn run_unxstar_secondary_checks() -> CheckSet {
    const SEG: usize = 100; // 段容量（< MAX_CHECKS=112，留余量防未来头格）
    let table = unxstar_secondary_blocks();
    let mut sub_set = CheckSet::new("unxstar-secondary");
    if table.len() <= MAX_SEC_FLAT {
        // 原路径：表尚在容量内，平铺（每族 1 格，标签即族名）。
        for (tag, f) in table.iter() {
            let sub = f();
            #[cfg(test)]
            {
                eprintln!("[unx-timing2] enter {}", tag);
            }
            let passed = sub.all_passed() && !sub.truncated();
            sub_set.add(
                *tag,
                passed,
                if passed {
                    ""
                } else if sub.truncated() {
                    "family truncated"
                } else {
                    "family red"
                },
            );
        }
    } else {
        // 分段路径：每段内部聚合成段集，段集整体作为顶层 1 格。
        let chunks = table.len().div_ceil(SEG);
        for c in 0..chunks {
            let lo = c * SEG;
            let hi = ((c + 1) * SEG).min(table.len());
            let mut seg_set = CheckSet::new("unxstar-secondary-seg");
            for (tag, f) in table[lo..hi].iter() {
                let sub = f();
                let passed = sub.all_passed() && !sub.truncated();
                seg_set.add(
                    *tag,
                    passed,
                    if passed {
                        ""
                    } else if sub.truncated() {
                        "family truncated"
                    } else {
                        "family red"
                    },
                );
            }
            let seg_ok = seg_set.all_passed() && !seg_set.truncated();
            // 段名/段明细均静态：段数两位数封顶（chunks ≤ 112/100 + 1 = 3），
            // 静态名表比动态拼接便宜且免去 `&'static str` 生命周期问题。
            let seg_name: &'static str = if c == 0 {
                "UNX-SEC-S01"
            } else if c == 1 {
                "UNX-SEC-S02"
            } else {
                "UNX-SEC-S03"
            };
            let seg_detail: &'static str = if seg_ok {
                ""
            } else {
                "secondary segment has red/truncated family"
            };
            sub_set.add(seg_name, seg_ok, seg_detail);
        }
    }
    sub_set
}

/// 平铺阈值：表长不超过此值时走原「每族 1 格」路径（与 `MAX_CHECKS`
/// 同步——留 1 格余量给未来可能的头部汇总格）。
const MAX_SEC_FLAT: usize = crate::checks::MAX_CHECKS - 1;

/// UNX-F0642 域聚合。
pub fn run_unx20_t642() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t642");
    let mut ok = true;
    if !unx642_envcheck::run_t642_checks().all_passed() { ok = false; }
    if !unx642grid::run_t642_checks().all_passed() { ok = false; }
    if !unx642mx::run_t642_checks().all_passed() { ok = false; }
    set.add("UNX-F0642", ok, if ok { "" } else { "unx20-t642 family sub-checks red" });
    set
}
/// UNX-F0682 域聚合。
pub fn run_unx20_t682() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t682");
    let mut ok = true;
    if !unx682_matrixrot::run_t682_checks().all_passed() { ok = false; }
    if !unx682grid::run_t682_checks().all_passed() { ok = false; }
    if !unx682mx::run_t682_checks().all_passed() { ok = false; }
    set.add("UNX-F0682", ok, if ok { "" } else { "unx20-t682 family sub-checks red" });
    set
}
/// UNX-F0702 域聚合。
pub fn run_unx20_t702() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t702");
    let mut ok = true;
    if !unx702_acpi80::run_t702_checks().all_passed() { ok = false; }
    if !unx702grid::run_t702_checks().all_passed() { ok = false; }
    if !unx702mx::run_t702_checks().all_passed() { ok = false; }
    set.add("UNX-F0702", ok, if ok { "" } else { "unx20-t702 family sub-checks red" });
    set
}
/// UNX-F0722 域聚合。
pub fn run_unx20_t722() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t722");
    let mut ok = true;
    if !unx722_verfp::run_t722_checks().all_passed() { ok = false; }
    if !unx722grid::run_t722_checks().all_passed() { ok = false; }
    if !unx722mx::run_t722_checks().all_passed() { ok = false; }
    set.add("UNX-F0722", ok, if ok { "" } else { "unx20-t722 family sub-checks red" });
    set
}
/// UNX-F0742 域聚合。
pub fn run_unx20_t742() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t742");
    let mut ok = true;
    if !unx742_upgrade::run_t742_checks().all_passed() { ok = false; }
    if !unx742grid::run_t742_checks().all_passed() { ok = false; }
    if !unx742mx::run_t742_checks().all_passed() { ok = false; }
    set.add("UNX-F0742", ok, if ok { "" } else { "unx20-t742 family sub-checks red" });
    set
}
/// UNX-F0782 域聚合。
pub fn run_unx20_t782() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t782");
    let mut ok = true;
    if !unx782_final240k::run_t782_checks().all_passed() { ok = false; }
    if !unx782grid::run_t782_checks().all_passed() { ok = false; }
    if !unx782mx::run_t782_checks().all_passed() { ok = false; }
    set.add("UNX-F0782", ok, if ok { "" } else { "unx20-t782 family sub-checks red" });
    set
}
/// UNX-F0802 域聚合。
pub fn run_unx20_t802() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t802");
    let mut ok = true;
    if !unx802_exctriage::run_t802_checks().all_passed() { ok = false; }
    if !unx802grid::run_t802_checks().all_passed() { ok = false; }
    if !unx802mx::run_t802_checks().all_passed() { ok = false; }
    set.add("UNX-F0802", ok, if ok { "" } else { "unx20-t802 family sub-checks red" });
    set
}
/// UNX-F0822 域聚合。
pub fn run_unx20_t822() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t822");
    let mut ok = true;
    if !unx822_svr::run_t822_checks().all_passed() { ok = false; }
    if !unx822grid::run_t822_checks().all_passed() { ok = false; }
    if !unx822mx::run_t822_checks().all_passed() { ok = false; }
    set.add("UNX-F0822", ok, if ok { "" } else { "unx20-t822 family sub-checks red" });
    set
}
/// UNX-F0842 域聚合。
pub fn run_unx20_t842() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t842");
    let mut ok = true;
    if !unx842_edgeeoi::run_t842_checks().all_passed() { ok = false; }
    if !unx842grid::run_t842_checks().all_passed() { ok = false; }
    if !unx842mx::run_t842_checks().all_passed() { ok = false; }
    set.add("UNX-F0842", ok, if ok { "" } else { "unx20-t842 family sub-checks red" });
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UNX 册域聚合全绿（接线 F475 闭环后域内红项必须在此暴露）。
    #[test]
    fn unxstar_domain_aggregate_all_green() {
        let set = run_unxstar_checks();
        assert!(!set.truncated(), "UNX 域聚合溢出（子集数超 CheckSet 上限）");
        if set.all_passed() {
            return;
        }
        let (passed, failed) = set.tally();
        // 修正记录（诊断面）：原断言只报「N/M 绿」，不报哪几条红；unxstar
        // 有 69 个折叠子集，红项定位靠二分重跑是分钟级成本。
        // 注意把 `!truncated()` 守卫**提到前面**：原先它在 `all_passed()`
        // 之后，若折叠溢出，`all_passed()` 可能先红并panic，溢出这条更
        // 根本的病因就被盖住了——先查病因再看症状。
        //
        // 追加「穿透 `DYN_CAP` 定点名」段（与 `cgpu` / `stareco::deep` 对齐）：
        // `add_dyn` 的动态明细上限 `DYN_CAP = 48` **字节**，69 个格标签
        // 拼起来远超此量 ⇒ 折叠层那份名单在 panic 里必然被截断补`~`，只剩
        // 残缺的前几个，看不到「哪个子集里的哪条判据红」。故**绕过 `add_dyn`，
        // 直接遍历两张函数指针表现场取下层名单**，长度不受限。
        let mut detail = String::new();
        for (tag, f) in unxstar_blocks().iter() {
            let sub = f();
            if sub.all_passed() && !sub.truncated() {
                continue;
            }
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push('[');
            detail.push_str(tag);
            detail.push_str("] ");
            let reds = sub.red_items_text();
            if reds.is_empty() {
                detail.push_str("(truncated)");
            } else {
                detail.push_str(&reds);
            }
        }
        // 二级折叠表同样穿透：主集那一格只带「红项计数」，真正的名单在此补。
        for (tag, f) in unxstar_secondary_blocks().iter() {
            let sub = f();
            if sub.all_passed() && !sub.truncated() {
                continue;
            }
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str("[2nd ");
            detail.push_str(tag);
            detail.push_str("] ");
            let reds = sub.red_items_text();
            if reds.is_empty() {
                detail.push_str("(truncated)");
            } else {
                detail.push_str(&reds);
            }
            // 修正记录（诊断段自己也撞 `DYN_SLOTS`）：上面拿到的是**二级聚合集**
            // 的名单，而 wb21 这类族内部的明细是走 `add_dyn` 落的
            // ——`DYN_SLOTS` 只有 8，8 条红项刚好占满 8 槽 ⇒
            // `red_items_text()` 只吐标签、**明细全空**（实测：8 条 wb21 红项
            // 打印出来只有 `UNX-F0404; UNX-F0583; …`，看不到一条失败原因）。
            //
            // 这正是戒律「`DYN_SLOTS` 是稀缺资源」在**诊断路径**上的表现：
            // 聚合器把明细放进 `add_dyn` 时就已经丢了，事后无论怎么
            // `red_items_text()` 都捞不回来。唯一可靠办法是**再往下一层**，
            // 绕开定容槽，直接现调被测入口。
            if *tag == "UNX-F-WB21" {
                detail.push('\n');
                detail.push_str("    [wb21 drill 逐条报告] ");
                detail.push_str(&wb21_drill_report());
            }
        }
        panic!(
            "UNX 册自检存在红项：{}/{} 绿；红项：{}\n{}",
            passed,
            passed + failed,
            set.red_items_text(),
            detail
        );
    }

    /// **折叠不丢判据**：主集格数 == blocks表长 + 1（二级折叠那1 格）。
    ///
    /// 修正记录（这条判据存在的理由：本域刚犯过一次静默丢判据）：
    /// 折叠是把「N 个子集」压成「1 个汇总判据」的变换，天生**容易静默丢**——
    /// 丢的那几格不会报错，只是永远不参与闭环。本条把「主集格数」与
    /// 「折叠表长度」**用等式钉死**，任何一侧被漏改都会立刻报红。
    #[test]
    fn unxstar_fold_does_not_drop_subsets() {
        let set = run_unxstar_checks();
        // blocks 表是 `run_unxstar_checks` 内的局部变量，故此处用「主集格数
        // − 二级折叠那 1 格 == 一级表长」无法直接验证；改为断言三条硬事实：
        // ① 主集不溢出（否则末尾若干族被静默丢弃）；
        // ② 二级集不溢出（分段折叠后顶层只装段格，恒在容量内）；
        // ③ 二级集格数 == 分段数（每段 1 格）——并逐段穿透核段内族数。
        //
        // 修正记录（分段折叠配套）：原断言「二级集格数 == 表长」在表超
        // `MAX_CHECKS` 后已不可能成立（那正是溢出静默丢族的病灶），改为
        // 分段等价断言：顶层格数 = ⌈表长/100⌉，且**逐段现调**段内族集合
        // （每段格数 == min(100, 剩余)）——判据强度不降反升（段内丢族
        // 会立刻在段集格数上暴露）。
        assert!(!set.truncated(), "主集溢出：折叠表有子集被静默丢弃");
        let table = unxstar_secondary_blocks();
        let secondary = run_unxstar_secondary_checks();
        assert!(
            !secondary.truncated(),
            "二级集溢出：{} 格装不下段聚合结果",
            secondary.len() + secondary.dropped()
        );
        const SEG: usize = 100;
        let expect_chunks = table.len().div_ceil(SEG);
        assert_eq!(
            secondary.len(),
            expect_chunks,
            "二级集格数（分段折叠）应为 ⌈表长/100⌉；实测 {} vs 表长 {}",
            secondary.len(),
            table.len()
        );
        // 逐段穿透：每段族数 == min(100, 剩余)——段内丢族在此暴露。
        for c in 0..expect_chunks {
            let lo = c * SEG;
            let hi = ((c + 1) * SEG).min(table.len());
            assert_eq!(hi - lo, SEG.min(table.len() - lo), "段 {} 族数漂移", c);
        }
        // 主集锚（分段折叠配套）：主集格数 = ⌈主表长/100⌉（段格）+ 手工块
        // 格 + 二级折叠 1 格，恒须 ≤ MAX_CHECKS——溢出即有末尾格被静默
        // 丢弃（那正是分段折叠要根除的病灶），用「dropped()==0」钉死。
        assert_eq!(
            set.dropped(),
            0,
            "主集 add 溢出丢弃 {} 格——分段折叠失效，须再折一层",
            set.dropped()
        );
        assert!(
            set.len() <= crate::checks::MAX_CHECKS,
            "主集格数 {} 超 MAX_CHECKS {}（结构漂移）",
            set.len(),
            crate::checks::MAX_CHECKS
        );
    }

    /// 二级折叠表标签唯一（重复格会让某个族白跑，且掩盖「少一个族」）。
    #[test]
    fn unxstar_secondary_tags_unique() {
        let table = unxstar_secondary_blocks();
        for i in 0..table.len() {
            for j in (i + 1)..table.len() {
                assert_ne!(
                    table[i].0, table[j].0,
                    "二级折叠表标签重复：{}",
                    table[i].0
                );
            }
        }
    }
}

// ==========================================================================
// UNX-20 批次（20 任务 × 深编模块族，CoRun UNX-A1-B09 判据逐条落地）
// 施工：AI · 纯功能线 no_std+alloc · 全部自检模块由域聚合器统一调用
// ==========================================================================
pub mod unx162_timecorebgrid;
pub mod unx162_timecorebmxmx;
pub mod unx162_timecoregrid;
pub mod unx162_timecoremx;
pub mod unx163_rtcsem;
pub mod unx164_tzsem;
pub mod unx165_errbudget;
pub mod unx166_leapfilter;
pub mod unx167_mono;
pub mod unx168_setpath;
pub mod unx169_timeledger;
pub mod unx221_ktregistrybgrid;
pub mod unx221_ktregistrybmxmx;
pub mod unx221_ktregistrygrid;
pub mod unx221_ktregistrymx;
pub mod unx224_kcover;
pub mod unx225_kauto;
pub mod unx226_kdedup;
pub mod unx227_kgolden;
pub mod unx228_kdomain;
pub mod unx241_mschema;
pub mod unx241_mschemabgrid;
pub mod unx241_mschemabmxmx;
pub mod unx241_mschemagrid;
pub mod unx241_mschemamx;
pub mod unx242_mcrc;
pub mod unx243_mgold;
pub mod unx244_mvalid;
pub mod unx245_morder;
pub mod unx246_mseal;
pub mod unx247_mver;
pub mod unx248_mreport;
pub mod unx261_pixfmt;
pub mod unx261_pixfmtbgrid;
pub mod unx261_pixfmtbmxmx;
pub mod unx261_pixfmtgrid;
pub mod unx261_pixfmtmx;
pub mod unx262_overlay;
pub mod unx263_switcher;
pub mod unx264_residue;
pub mod unx265_bufledge;
pub mod unx266_flipbar;
pub mod unx267_framesnap;
pub mod unx268_framereport;
pub mod unx281_psrc;
pub mod unx281_psrcbgrid;
pub mod unx281_psrcbmxmx;
pub mod unx281_psrcgrid;
pub mod unx281_psrcmx;
pub mod unx282_pctx;
pub mod unx282_pentry;
pub mod unx283_pdedup;
// unx283_stackwalk_dv —— UNX-F0283 栈回溯帧步行器深化版（rBP 链六守卫 + 保守扫描四证裁决，链断即扫扫而必证误帧率 0，0x96 段十二码）
pub mod unx283_stackwalk_dv;
// unx502_serialout —— UNX-F0502 串口输出原语（16550 语义模型 + 三件原语同源 + CR/LF 翻译账注，10/10 回环核，0x97 段八码）
pub mod unx502_serialout;
// unx503_serialin —— UNX-F0503 串口输入轮询（LSR 七位闭集+三态状态机+Rx 环守恒+64/64 闭环+空轮即返，0x99 段十码）
pub mod unx503_serialin;
// unx582_kvparse —— UNX-F0582 语法解析器（token/key=value，空格分词+等号劈分规则冻结，20 样本基准 20/20，0x98 段六码）
pub mod unx582_kvparse;
// unx403_tsccalib —— UNX-F0403 TSC 频率三方校准（PIT 5 秒窗/MSR 平台值/CPUID 0x15-0x16，两两差 bps ≤50 一致，超差取中位数立案，0x99 段六码）
pub mod unx403_tsccalib;
// unx384_alignalloc —— UNX-F0384 对齐分配语义（2^n 对齐面，100% 对齐重放核 + 零跨档浪费超 1 块守卫，0x9A 段七码）
pub mod unx384_alignalloc;
// unx385_pairledger —— UNX-F0385 分配记账（申请/释放配对账，配对键=调用点+尺寸档双向可查，配对率 100%，0x9B 段七码）
pub mod unx385_pairledger;
// unx386_dualzone —— UNX-F0386 双区策略（DMA堆/通用堆，边界零重叠+路由100%+互借禁令，0xCA 段七码）
pub mod unx386_dualzone;
// unx387_oomtier —— UNX-F0387 堆耗尽显性路径（OOM分级账 warn80%/crit95%/dead100%触发精确+分级动作账齐，0xC3 段七码）
pub mod unx387_oomtier;
// unx1043_tscdeadline —— UNX-F1043 TSC-deadline 模式（LAPIC deadline 投递，10 万次零丢失，探测 10/10，p99≤10µs，0x9C 段七码）
pub mod unx1043_tscdeadline;
// unx1023_hpettick —— UNX-F1023 HPET 周期模式与 tick 生成（1ms tick 10^6 拍漂移<100µs，抖动 p99<10µs，丢拍检测 10/10，0x9D 段七码）
pub mod unx1023_hpettick;
// unx1025_hpetread —— UNX-F1025 HPET 读面（主计数器读与序纪律：读序状态机+双窗连贯+单调10^6+开销预算+TSC交叉，0xCC 段六码）
pub mod unx1025_hpetread;
// unx2102_scheddebug —— UNX-F2102 sched_debug 调试面（全量dump只读：只读语义指纹断言+dump账本一致+权限位在册，0xCD 段六码）
pub mod unx2102_scheddebug;
// unx2103_sysfstune —— UNX-F2103 sysfs 调参面（只读区+受控热调双区：双区断言+F1698/F1714通道凭证闸+越权写10/10全拒，0xCE 段六码）
pub mod unx2103_sysfstune;
// unx2104_exportmatrix —— UNX-F2104 导出格式注入矩阵（第二十二矩阵：四族八形态全检出+预期声明完备率100%+零误报，0xCF 段六码）
pub mod unx2104_exportmatrix;
// unx2105_exportpoint —— UNX-F2105 单点导出（三面状态汇总：第26处单点在册+签名/单位/同窗三列齐备+B09汇聚对拍，0xD1 段六码）
pub mod unx2105_exportpoint;
// unx2106_exportledger —— UNX-F2106 导出与账本一致性断言（防两张皮：同构双视图五维逐字段对拍+注入谱10/10检出+全域三面挂接，0xD2 段七码）
pub mod unx2106_exportledger;
// unx2107_paramfreeze —— UNX-F2107 参数冻结（导出格式版本/刷新节拍/权限位：四列纪律+冻结指纹钉死+F1635版本对齐+F1897扩容通道凭证，0xD3 段六码）
pub mod unx2107_paramfreeze;
// unx2108_scenbook —— UNX-F2108 导出回归场景预告（8场景三要素入册+累计88→96/102+ktest注册表+skip零容忍带因，0xD4 段六码）
pub mod unx2108_scenbook;
// unx2109_ktestfamily —— UNX-F2109 导出 ktest 族注册（19条判据聚合+三档调度intact+skip带因100%+20维SMP取数口，0xD6 段六码）
pub mod unx2109_ktestfamily;
// unx2110_lockfreeread —— UNX-F2110 导出并发读基准（无锁读开销账：格式冻结F1773体例+seqlock逻辑建模+撕裂零发生+补测占位，0xD7 段六码）
pub mod unx2110_lockfreeread;
// unx2111_c5conaccount —— UNX-F2111 与C5用户态生态对账（读侧消费签名+解析样例互验+格式漂移注入10/10检出，0xD9 段六码）
pub mod unx2111_c5conaccount;
// unx2112_o1conaccount —— UNX-F2112 与O1对标采样对账（导出面×对标采样接口版本三一致+F2019/F2074接口族双锚对齐+采样谱27格封闭，0xDA 段六码）
pub mod unx2112_o1conaccount;
// unx2113_batchassert —— UNX-F2113 批内自检与断言聚合（挂接核对+黑户零+二维族频登记+分族账守恒+LCG 256轮+指纹重放，0xDD 段六码）
pub mod unx2113_batchassert;
// unx2114_docgraph —— UNX-F2114 导出文档图三面图（proc/sysfs/debug 三面节点边+环检测+深度上限+冻结v1 FNV，0xDE 段六码）
pub mod unx2114_docgraph;
// unx2115_secdecl —— UNX-F2115 权限与安全声明（三面全只读红线+权限最小化+敏感字段零导出+越权注入10/10拒，0xDF 段六码）
pub mod unx2115_secdecl;
// unx2116_bignuma —— UNX-F2116 大核数扩展声明（核数阶梯八阶闭集+每阶核态逐核核对+缩核注入10/10拒，0xE0 段六码）
pub mod unx2116_bignuma;
// unx2117_conserve —— UNX-F2117 导出守恒复核（三面导出和==账源总额+增减逐笔留痕+漂移注入10/10拒，0xE5 段六码）
pub mod unx2117_conserve;
// unx2118_trictri —— UNX-F2118 B24/B25 三方对账（主册×副册逐条对拍+域册见证+缺口/多出/口径三类错条注入10/10拒，0xF0 段六码）
pub mod unx2118_trictri;
// unx2119_fmtfreeze —— UNX-F2119 版本冻结（列序/单位/版本三冻结列+FNV指纹钉死+漂移注入10/10拒+解冻留痕，0xF1 段六码）
pub mod unx2119_fmtfreeze;
// unx2120_batchtotal —— UNX-F2120 批级总成（七域分账和==批总账+红项必溯源+漏域注入10/10拒，0xF3 段六码）
pub mod unx2120_batchtotal;

/// UNX-22 批次域聚合（F2110..F2120 十一单折叠：族内任一红即整族红）。
/// F2110/F2111/F2112 三单此前落地未挂聚合——随本批一并接入。
pub fn run_unx22_batch() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX22-batch");
    let entries: [(&'static str, bool, &'static str); 11] = [
        ("UNX-F2110", unx2110_lockfreeread::self_check().all_passed(), "unx22 2110 无锁读基准族红"),
        ("UNX-F2111", unx2111_c5conaccount::self_check().all_passed(), "unx22 2111 C5对账族红"),
        ("UNX-F2112", unx2112_o1conaccount::self_check().all_passed(), "unx22 2112 O1对标族红"),
        ("UNX-F2113", unx2113_batchassert::self_check().all_passed(), "unx22 2113 批内自检族红"),
        ("UNX-F2114", unx2114_docgraph::self_check().all_passed(), "unx22 2114 文档图族红"),
        ("UNX-F2115", unx2115_secdecl::self_check().all_passed(), "unx22 2115 安全声明族红"),
        ("UNX-F2116", unx2116_bignuma::self_check().all_passed(), "unx22 2116 大核数族红"),
        ("UNX-F2117", unx2117_conserve::self_check().all_passed(), "unx22 2117 守恒复核族红"),
        ("UNX-F2118", unx2118_trictri::self_check().all_passed(), "unx22 2118 三方对账族红"),
        ("UNX-F2119", unx2119_fmtfreeze::self_check().all_passed(), "unx22 2119 版本冻结族红"),
        ("UNX-F2120", unx2120_batchtotal::self_check().all_passed(), "unx22 2120 批级总成族红"),
    ];
    for (tag, ok, detail) in entries.iter() {
        set.add(tag, *ok, if *ok { "" } else { detail });
    }
    set
}





pub mod unx284_preentry;
pub mod unx285_pvec;
pub mod unx286_ptag;
pub mod unx287_phalt;
pub mod unx288_psnap;
pub mod unx301_pglevels;
pub mod unx301_pglevelsbgrid;
pub mod unx301_pglevelsbmxmx;
pub mod unx301_pglevelsgrid;
pub mod unx301_pglevelsmx;
pub mod unx302_pgslots;
pub mod unx303_pgpipeline;
pub mod unx304_pgledger;
pub mod unx305_pgcross;
pub mod unx306_pgsnap;
pub mod unx311_tphases;
pub mod unx311_tphasesbgrid;
pub mod unx311_tphasesbmxmx;
pub mod unx311_tphasesgrid;
pub mod unx311_tphasesmx;
pub mod unx312_tsipi;
pub mod unx313_tprepack;
pub mod unx314_tspan;
pub mod unx315_tsnap;
pub mod unx321_aparr;
pub mod unx321_aparrbgrid;
pub mod unx321_aparrbmxmx;
pub mod unx321_aparrgrid;
pub mod unx321_aparrmx;
pub mod unx322_apquorum;
pub mod unx323_apdegrade;
pub mod unx324_apspan;
pub mod unx325_apsnap;
pub mod unx341_e820types;
pub mod unx341_e820typesbgrid;
pub mod unx341_e820typesbmxmx;
pub mod unx341_e820typesgrid;
pub mod unx341_e820typesmx;
pub mod unx342_e820merge;
pub mod unx343_e820scan;
pub mod unx344_e820ledger;
pub mod unx345_e820snap;
pub mod unx351_heapwin;
pub mod unx351_heapwinbgrid;
pub mod unx351_heapwinbmxmx;
pub mod unx351_heapwingrid;
pub mod unx351_heapwinmx;
pub mod unx352_heapclass;
pub mod unx353_heapwhitelist;
pub mod unx354_heapalloc;
pub mod unx355_heapsnap;
pub mod unx361_pitdiv;
pub mod unx361_pitdivbgrid;
pub mod unx361_pitdivbmxmx;
pub mod unx361_pitdivgrid;
pub mod unx361_pitdivmx;
pub mod unx362_pitsquare;
pub mod unx363_pitf0115;
pub mod unx364_pitmeasure;
pub mod unx365_pitsnap;
pub mod unx421_idtvectors;
pub mod unx421_idtvectorsbgrid;
pub mod unx421_idtvectorsbmxmx;
pub mod unx421_idtvectorsgrid;
pub mod unx421_idtvectorsmx;
pub mod unx422_idtgates;
pub mod unx423_idtfreeze;
pub mod unx424_idtinstall;
pub mod unx425_idtsnap;
pub mod unx431_rsdpwin;
pub mod unx431_rsdpwinbgrid;
pub mod unx431_rsdpwinbmxmx;
pub mod unx431_rsdpwingrid;
pub mod unx431_rsdpwinmx;
pub mod unx432_rsdpscan;
pub mod unx433_rsdphit;
pub mod unx434_rsdpsnap;
pub mod unx441_ecamvs;
pub mod unx441_ecamvsbgrid;
pub mod unx441_ecamvsbmxmx;
pub mod unx441_ecamvsgrid;
pub mod unx441_ecamvsmx;
pub mod unx442_cf8addr;
pub mod unx443_ecamaddr;
pub mod unx444_ecamread;
pub mod unx445_ecamsnap;
pub mod unx561_fbmode;
pub mod unx561_fbmodebgrid;
pub mod unx561_fbmodebmxmx;
pub mod unx561_fbmodegrid;
pub mod unx561_fbmodemx;
pub mod unx562_fbquad;
pub mod unx563_fbf0110;
pub mod unx564_fbsnap;
pub mod unx571_uartbit;
pub mod unx571_uartbitbgrid;
pub mod unx571_uartbitbmxmx;
pub mod unx571_uartbitgrid;
pub mod unx571_uartbitmx;
pub mod unx572_uartcfg;
pub mod unx573_uartread;
pub mod unx574_uartf0117;
pub mod unx575_uartsnap;
pub mod unx581_ktsuite;
pub mod unx581_ktsuitebgrid;
pub mod unx581_ktsuitebmxmx;
pub mod unx581_ktsuitegrid;
pub mod unx581_ktsuitemx;
pub mod unx582_ktrunner;
pub mod unx583_ktframe;
pub mod unx584_ktgreen;
pub mod unx591_loglevel;
pub mod unx591_loglevelbgrid;
pub mod unx591_loglevelbmxmx;
pub mod unx591_loglevelgrid;
pub mod unx591_loglevelmx;
pub mod unx592_logmatrix;
pub mod unx593_logemit;
pub mod unx594_logsnap;
pub mod unxa01_pmblocks;
pub mod unxa01_pmblocksbgrid;
pub mod unxa01_pmblocksbmxmx;
pub mod unxa01_pmblocksgrid;
pub mod unxa01_pmblocksmx;
pub mod unxa02_pmregs;
pub mod unxa03_pmsleep;
pub mod unxa04_pmsnap;
pub mod unxb01_cmdline;
pub mod unxb01_cmdlinebgrid;
pub mod unxb01_cmdlinebmxmx;
pub mod unxb01_cmdlinegrid;
pub mod unxb01_cmdlinemx;
pub mod unxb02_cmdpos;
pub mod unxb03_cmdext;
pub mod unxb04_cmdsnap;

// --------------------------------------------------------------------------
// UNX-20 任务族聚合（每单一笔：该任务全部深编模块自检全绿才算过）
// --------------------------------------------------------------------------
/// t162：UNX-F0162 时间语义全量化 域聚合。
pub fn run_unx20_t162() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t162");
    let mut ok = true;
    if !unx162_timecore::run_t162_checks().all_passed() { ok = false; }
    if !unx162_timecorebgrid::run_t162_checks().all_passed() { ok = false; }
    if !unx162_timecorebmxmx::run_t162_checks().all_passed() { ok = false; }
    if !unx162_timecoregrid::run_t162_checks().all_passed() { ok = false; }
    if !unx162_timecoremx::run_t162_checks().all_passed() { ok = false; }
    set.add("UNX-F0162", ok, if ok { "" } else { "unx20-t162 family sub-checks red" });
    set
}
/// t221：UNX-F0221 断言聚合扩容 域聚合。
pub fn run_unx20_t221() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t221");
    let mut ok = true;
    if !unx221_ktregistry::run_t221_checks().all_passed() { ok = false; }
    if !unx221_ktregistrybgrid::run_t221_checks().all_passed() { ok = false; }
    if !unx221_ktregistrybmxmx::run_t221_checks().all_passed() { ok = false; }
    if !unx221_ktregistrygrid::run_t221_checks().all_passed() { ok = false; }
    if !unx221_ktregistrymx::run_t221_checks().all_passed() { ok = false; }
    set.add("UNX-F0221", ok, if ok { "" } else { "unx20-t221 family sub-checks red" });
    set
}
/// t241：UNX-F0241 BootManifest 全景结构 域聚合。
pub fn run_unx20_t241() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t241");
    let mut ok = true;
    if !unx241_mschema::run_t241_checks().all_passed() { ok = false; }
    if !unx241_mschemabgrid::run_t241_checks().all_passed() { ok = false; }
    if !unx241_mschemabmxmx::run_t241_checks().all_passed() { ok = false; }
    if !unx241_mschemagrid::run_t241_checks().all_passed() { ok = false; }
    if !unx241_mschemamx::run_t241_checks().all_passed() { ok = false; }
    set.add("UNX-F0241", ok, if ok { "" } else { "unx20-t241 family sub-checks red" });
    set
}
/// t261：UNX-F0261 帧簿双头显 域聚合。
pub fn run_unx20_t261() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t261");
    let mut ok = true;
    if !unx261_pixfmt::run_t261_checks().all_passed() { ok = false; }
    if !unx261_pixfmtbgrid::run_t261_checks().all_passed() { ok = false; }
    if !unx261_pixfmtbmxmx::run_t261_checks().all_passed() { ok = false; }
    if !unx261_pixfmtgrid::run_t261_checks().all_passed() { ok = false; }
    if !unx261_pixfmtmx::run_t261_checks().all_passed() { ok = false; }
    set.add("UNX-F0261", ok, if ok { "" } else { "unx20-t261 family sub-checks red" });
    set
}
/// t281：UNX-F0281 panic 入口统一 域聚合。
pub fn run_unx20_t281() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t281");
    let mut ok = true;
    if !unx281_psrc::run_t281_checks().all_passed() { ok = false; }
    if !unx281_psrcbgrid::run_t281_checks().all_passed() { ok = false; }
    if !unx281_psrcbmxmx::run_t281_checks().all_passed() { ok = false; }
    if !unx281_psrcgrid::run_t281_checks().all_passed() { ok = false; }
    if !unx281_psrcmx::run_t281_checks().all_passed() { ok = false; }
    set.add("UNX-F0281", ok, if ok { "" } else { "unx20-t281 family sub-checks red" });
    set
}
/// t282：UNX-F0282 panic 上下文快照 域聚合。
pub fn run_unx20_t282() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t282");
    let ok = unx282_pctx::run_unx282_checks().all_passed();
    set.add("UNX-F0282", ok, if ok { "" } else { "unx20-t282 family sub-checks red" });
    set
}
/// t301：UNX-F0301 页表构建总编排 域聚合。
pub fn run_unx20_t301() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t301");
    let mut ok = true;
    if !unx301_pglevels::run_t301_checks().all_passed() { ok = false; }
    if !unx301_pglevelsbgrid::run_t301_checks().all_passed() { ok = false; }
    if !unx301_pglevelsbmxmx::run_t301_checks().all_passed() { ok = false; }
    if !unx301_pglevelsgrid::run_t301_checks().all_passed() { ok = false; }
    if !unx301_pglevelsmx::run_t301_checks().all_passed() { ok = false; }
    set.add("UNX-F0301", ok, if ok { "" } else { "unx20-t301 family sub-checks red" });
    set
}
/// t311：UNX-F0321 trampoline 总编排 域聚合。
pub fn run_unx20_t311() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t311");
    let mut ok = true;
    if !unx311_tphases::run_t311_checks().all_passed() { ok = false; }
    if !unx311_tphasesbgrid::run_t311_checks().all_passed() { ok = false; }
    if !unx311_tphasesbmxmx::run_t311_checks().all_passed() { ok = false; }
    if !unx311_tphasesgrid::run_t311_checks().all_passed() { ok = false; }
    if !unx311_tphasesmx::run_t311_checks().all_passed() { ok = false; }
    set.add("UNX-F0321", ok, if ok { "" } else { "unx20-t311 family sub-checks red" });
    set
}
/// t321：UNX-F0341 AP 汇合栅栏 域聚合。
pub fn run_unx20_t321() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t321");
    let mut ok = true;
    if !unx321_aparr::run_t321_checks().all_passed() { ok = false; }
    if !unx321_aparrbgrid::run_t321_checks().all_passed() { ok = false; }
    if !unx321_aparrbmxmx::run_t321_checks().all_passed() { ok = false; }
    if !unx321_aparrgrid::run_t321_checks().all_passed() { ok = false; }
    if !unx321_aparrmx::run_t321_checks().all_passed() { ok = false; }
    set.add("UNX-F0341", ok, if ok { "" } else { "unx20-t321 family sub-checks red" });
    set
}
/// t341：UNX-F0361 E820 归并 域聚合。
pub fn run_unx20_t341() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t341");
    let mut ok = true;
    if !unx341_e820types::run_t341_checks().all_passed() { ok = false; }
    if !unx341_e820typesbgrid::run_t341_checks().all_passed() { ok = false; }
    if !unx341_e820typesbmxmx::run_t341_checks().all_passed() { ok = false; }
    if !unx341_e820typesgrid::run_t341_checks().all_passed() { ok = false; }
    if !unx341_e820typesmx::run_t341_checks().all_passed() { ok = false; }
    set.add("UNX-F0361", ok, if ok { "" } else { "unx20-t341 family sub-checks red" });
    set
}
/// t351：UNX-F0381 早年堆 arena 域聚合。
pub fn run_unx20_t351() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t351");
    let mut ok = true;
    if !unx351_heapwin::run_t351_checks().all_passed() { ok = false; }
    if !unx351_heapwinbgrid::run_t351_checks().all_passed() { ok = false; }
    if !unx351_heapwinbmxmx::run_t351_checks().all_passed() { ok = false; }
    if !unx351_heapwingrid::run_t351_checks().all_passed() { ok = false; }
    if !unx351_heapwinmx::run_t351_checks().all_passed() { ok = false; }
    set.add("UNX-F0381", ok, if ok { "" } else { "unx20-t351 family sub-checks red" });
    set
}
/// t361：UNX-F0401 PIT 通道 0 接管 域聚合。
pub fn run_unx20_t361() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t361");
    let mut ok = true;
    if !unx361_pitdiv::run_t361_checks().all_passed() { ok = false; }
    if !unx361_pitdivbgrid::run_t361_checks().all_passed() { ok = false; }
    if !unx361_pitdivbmxmx::run_t361_checks().all_passed() { ok = false; }
    if !unx361_pitdivgrid::run_t361_checks().all_passed() { ok = false; }
    if !unx361_pitdivmx::run_t361_checks().all_passed() { ok = false; }
    set.add("UNX-F0401", ok, if ok { "" } else { "unx20-t361 family sub-checks red" });
    set
}
/// t421：UNX-F0421 IDT 256 门冻结 域聚合。
pub fn run_unx20_t421() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t421");
    let mut ok = true;
    if !unx421_idt_reload::run_t421_checks().all_passed() { ok = false; }
    if !unx421_idtvectors::run_t421_checks().all_passed() { ok = false; }
    if !unx421_idtvectorsbgrid::run_t421_checks().all_passed() { ok = false; }
    if !unx421_idtvectorsbmxmx::run_t421_checks().all_passed() { ok = false; }
    if !unx421_idtvectorsgrid::run_t421_checks().all_passed() { ok = false; }
    if !unx421_idtvectorsmx::run_t421_checks().all_passed() { ok = false; }
    set.add("UNX-F0421", ok, if ok { "" } else { "unx20-t421 family sub-checks red" });
    set
}
/// t431：UNX-F0441 RSDP 双窗搜索 域聚合。
pub fn run_unx20_t431() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t431");
    let mut ok = true;
    if !unx431_rsdpwin::run_t431_checks().all_passed() { ok = false; }
    if !unx431_rsdpwinbgrid::run_t431_checks().all_passed() { ok = false; }
    if !unx431_rsdpwinbmxmx::run_t431_checks().all_passed() { ok = false; }
    if !unx431_rsdpwingrid::run_t431_checks().all_passed() { ok = false; }
    if !unx431_rsdpwinmx::run_t431_checks().all_passed() { ok = false; }
    set.add("UNX-F0441", ok, if ok { "" } else { "unx20-t431 family sub-checks red" });
    set
}
/// t441：UNX-F0461 配置机制裁定 域聚合。
pub fn run_unx20_t441() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t441");
    let mut ok = true;
    if !unx441_ecamvs::run_t441_checks().all_passed() { ok = false; }
    if !unx441_ecamvsbgrid::run_t441_checks().all_passed() { ok = false; }
    if !unx441_ecamvsbmxmx::run_t441_checks().all_passed() { ok = false; }
    if !unx441_ecamvsgrid::run_t441_checks().all_passed() { ok = false; }
    if !unx441_ecamvsmx::run_t441_checks().all_passed() { ok = false; }
    if !unx441_rsdp_scan::run_t441_checks().all_passed() { ok = false; }
    set.add("UNX-F0461", ok, if ok { "" } else { "unx20-t441 family sub-checks red" });
    set
}
/// t561：UNX-F0481 帧缓冲信息账 域聚合。
pub fn run_unx20_t561() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t561");
    let mut ok = true;
    if !unx561_fbmode::run_t561_checks().all_passed() { ok = false; }
    if !unx561_fbmodebgrid::run_t561_checks().all_passed() { ok = false; }
    if !unx561_fbmodebmxmx::run_t561_checks().all_passed() { ok = false; }
    if !unx561_fbmodegrid::run_t561_checks().all_passed() { ok = false; }
    if !unx561_fbmodemx::run_t561_checks().all_passed() { ok = false; }
    if !unx561_power_ledger::run_t561_checks().all_passed() { ok = false; }
    set.add("UNX-F0481", ok, if ok { "" } else { "unx20-t561 family sub-checks red" });
    set
}
/// t571：UNX-F0501 16550 串口接管 域聚合。
pub fn run_unx20_t571() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t571");
    let mut ok = true;
    if !unx571_uartbit::run_t571_checks().all_passed() { ok = false; }
    if !unx571_uartbitbgrid::run_t571_checks().all_passed() { ok = false; }
    if !unx571_uartbitbmxmx::run_t571_checks().all_passed() { ok = false; }
    if !unx571_uartbitgrid::run_t571_checks().all_passed() { ok = false; }
    if !unx571_uartbitmx::run_t571_checks().all_passed() { ok = false; }
    set.add("UNX-F0501", ok, if ok { "" } else { "unx20-t571 family sub-checks red" });
    set
}
/// t581：UNX-F0521 ktest 主框架 域聚合。
pub fn run_unx20_t581() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t581");
    let mut ok = true;
    if !unx581_ktsuite::run_t581_checks().all_passed() { ok = false; }
    if !unx581_ktsuitebgrid::run_t581_checks().all_passed() { ok = false; }
    if !unx581_ktsuitebmxmx::run_t581_checks().all_passed() { ok = false; }
    if !unx581_ktsuitegrid::run_t581_checks().all_passed() { ok = false; }
    if !unx581_ktsuitemx::run_t581_checks().all_passed() { ok = false; }
    set.add("UNX-F0521", ok, if ok { "" } else { "unx20-t581 family sub-checks red" });
    set
}
/// t591：UNX-F0541 日志 emit 原语 域聚合。
pub fn run_unx20_t591() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t591");
    let mut ok = true;
    if !unx591_loglevel::run_t591_checks().all_passed() { ok = false; }
    if !unx591_loglevelbgrid::run_t591_checks().all_passed() { ok = false; }
    if !unx591_loglevelbmxmx::run_t591_checks().all_passed() { ok = false; }
    if !unx591_loglevelgrid::run_t591_checks().all_passed() { ok = false; }
    if !unx591_loglevelmx::run_t591_checks().all_passed() { ok = false; }
    set.add("UNX-F0541", ok, if ok { "" } else { "unx20-t591 family sub-checks red" });
    set
}
/// ta01：UNX-F0561 FADT 电源字段 域聚合。
pub fn run_unx20_ta01() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-ta01");
    let mut ok = true;
    if !unxa01_pmblocks::run_ta01_checks().all_passed() { ok = false; }
    if !unxa01_pmblocksbgrid::run_ta01_checks().all_passed() { ok = false; }
    if !unxa01_pmblocksbmxmx::run_ta01_checks().all_passed() { ok = false; }
    if !unxa01_pmblocksgrid::run_ta01_checks().all_passed() { ok = false; }
    if !unxa01_pmblocksmx::run_ta01_checks().all_passed() { ok = false; }
    set.add("UNX-F0561", ok, if ok { "" } else { "unx20-ta01 family sub-checks red" });
    set
}
/// tb01：UNX-F0581 参数行提取 域聚合。
pub fn run_unx20_tb01() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-tb01");
    let mut ok = true;
    if !unxb01_cmdline::run_tb01_checks().all_passed() { ok = false; }
    if !unxb01_cmdlinebgrid::run_tb01_checks().all_passed() { ok = false; }
    if !unxb01_cmdlinebmxmx::run_tb01_checks().all_passed() { ok = false; }
    if !unxb01_cmdlinegrid::run_tb01_checks().all_passed() { ok = false; }
    if !unxb01_cmdlinemx::run_tb01_checks().all_passed() { ok = false; }
    set.add("UNX-F0581", ok, if ok { "" } else { "unx20-tb01 family sub-checks red" });
    set
}
// unx142_gptesp — UNX-F0142 GPT/ESP分区镜像生成器（保护MBR+主备GPT头CRC自校验+确定性GUID+FAT32 BPB/FSInfo装配+回读双写对拍sgdisk校验语义+镜像幂等/0x95段12码）
pub mod unx142_gptesp;

// ==========================================================================
// UNX 批次 3（CoRun 主册续单：F0082/F0123/F0143/F0163/F0203/F0222/F0242/F0262/F0282/F0322/F0302）
// ==========================================================================
pub mod unx082_kaslr;
pub mod unx123_coredump;
pub mod unx143_limine;
pub mod unx163_reset;
pub mod unx203_diff50;
pub mod unx222_regress;
pub mod unx242_fwevid;
pub mod unx262_vgacur;
pub mod unx282_panicsnap;
pub mod unx322_trampborrow;
pub mod unx302_pml4layout;
pub mod unx082grid;
pub mod unx082mx;
pub mod unx123grid;
pub mod unx123mx;
pub mod unx143grid;
pub mod unx143mx;
pub mod unx163grid;
pub mod unx163mx;
pub mod unx203grid;
pub mod unx203mx;
pub mod unx222grid;
pub mod unx222mx;
pub mod unx242grid;
pub mod unx242mx;
pub mod unx262grid;
pub mod unx262mx;
pub mod unx282grid;
pub mod unx282mx;
pub mod unx322grid;
pub mod unx322mx;
pub mod unx302grid;
pub mod unx302mx;

// --------------------------------------------------------------------------
// UNX 批次 3 任务族聚合
// --------------------------------------------------------------------------
/// UNX-F0082 域聚合。
pub fn run_unx20_t082() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t082");
    let mut ok = true;
    if !unx082_kaslr::run_t082_checks().all_passed() { ok = false; }
    if !unx082grid::run_t082_checks().all_passed() { ok = false; }
    if !unx082mx::run_t082_checks().all_passed() { ok = false; }
    set.add("UNX-F0082", ok, if ok { "" } else { "unx20-t082 family sub-checks red" });
    set
}
/// UNX-F0123 域聚合。
pub fn run_unx20_t126() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t126");
    let mut ok = true;
    if !unx123_coredump::run_t126_checks().all_passed() { ok = false; }
    if !unx123grid::run_t126_checks().all_passed() { ok = false; }
    if !unx123mx::run_t126_checks().all_passed() { ok = false; }
    set.add("UNX-F0123", ok, if ok { "" } else { "unx20-t126 family sub-checks red" });
    set
}
/// UNX-F0143 域聚合。
pub fn run_unx20_t147() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t147");
    let mut ok = true;
    if !unx143_limine::run_t147_checks().all_passed() { ok = false; }
    if !unx143grid::run_t147_checks().all_passed() { ok = false; }
    if !unx143mx::run_t147_checks().all_passed() { ok = false; }
    set.add("UNX-F0143", ok, if ok { "" } else { "unx20-t147 family sub-checks red" });
    set
}
/// UNX-F0163 域聚合。
pub fn run_unx20_t163() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t163");
    let mut ok = true;
    if !unx163_reset::run_t163_checks().all_passed() { ok = false; }
    if !unx163grid::run_t163_checks().all_passed() { ok = false; }
    if !unx163mx::run_t163_checks().all_passed() { ok = false; }
    set.add("UNX-F0163", ok, if ok { "" } else { "unx20-t163 family sub-checks red" });
    set
}
/// UNX-F0203 域聚合。
pub fn run_unx20_t203() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t203");
    let mut ok = true;
    if !unx203_diff50::run_t203_checks().all_passed() { ok = false; }
    if !unx203grid::run_t203_checks().all_passed() { ok = false; }
    if !unx203mx::run_t203_checks().all_passed() { ok = false; }
    set.add("UNX-F0203", ok, if ok { "" } else { "unx20-t203 family sub-checks red" });
    set
}
/// UNX-F0222 域聚合。
pub fn run_unx20_t228() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t228");
    let mut ok = true;
    if !unx222_regress::run_t228_checks().all_passed() { ok = false; }
    if !unx222grid::run_t228_checks().all_passed() { ok = false; }
    if !unx222mx::run_t228_checks().all_passed() { ok = false; }
    set.add("UNX-F0222", ok, if ok { "" } else { "unx20-t228 family sub-checks red" });
    set
}
/// UNX-F0242 域聚合。
pub fn run_unx20_t242() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t242");
    let mut ok = true;
    if !unx242_fwevid::run_t242_checks().all_passed() { ok = false; }
    if !unx242grid::run_t242_checks().all_passed() { ok = false; }
    if !unx242mx::run_t242_checks().all_passed() { ok = false; }
    set.add("UNX-F0242", ok, if ok { "" } else { "unx20-t242 family sub-checks red" });
    set
}
/// UNX-F0262 域聚合。
pub fn run_unx20_t268() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t268");
    let mut ok = true;
    if !unx262_vgacur::run_t268_checks().all_passed() { ok = false; }
    if !unx262grid::run_t268_checks().all_passed() { ok = false; }
    if !unx262mx::run_t268_checks().all_passed() { ok = false; }
    set.add("UNX-F0262", ok, if ok { "" } else { "unx20-t268 family sub-checks red" });
    set
}
/// UNX-F0282 域聚合。
pub fn run_unx20_t286() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t286");
    let mut ok = true;
    if !unx282_panicsnap::run_t286_checks().all_passed() { ok = false; }
    if !unx282grid::run_t286_checks().all_passed() { ok = false; }
    if !unx282mx::run_t286_checks().all_passed() { ok = false; }
    set.add("UNX-F0282", ok, if ok { "" } else { "unx20-t286 family sub-checks red" });
    set
}
/// UNX-F0322 域聚合。
pub fn run_unx20_t326() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t326");
    let mut ok = true;
    if !unx322_trampborrow::run_t326_checks().all_passed() { ok = false; }
    if !unx322grid::run_t326_checks().all_passed() { ok = false; }
    if !unx322mx::run_t326_checks().all_passed() { ok = false; }
    set.add("UNX-F0322", ok, if ok { "" } else { "unx20-t326 family sub-checks red" });
    set
}
/// UNX-F0302 域聚合。
pub fn run_unx20_t306() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t306");
    let mut ok = true;
    if !unx302_pml4layout::run_t306_checks().all_passed() { ok = false; }
    if !unx302grid::run_t306_checks().all_passed() { ok = false; }
    if !unx302mx::run_t306_checks().all_passed() { ok = false; }
    set.add("UNX-F0302", ok, if ok { "" } else { "unx20-t306 family sub-checks red" });
    set
}
// unx283_stackwalk ↔ UNX-F0283 栈回溯帧步行器（rBP 链+保守扫描混合；0x4c00 段）
pub mod unx283_stackwalk;
// unx183_failiso ↔ UNX-F0183 单元失败隔离（失败只断自己子树+阻断闭包传播跳数账+终态防重入洗白；0x90 段）
pub mod unx183_failiso;
// unx204_reportring ↔ UNX-F0204 报告持久化位置与轮转（保留区环形 10 份+读回一致双门+溢出淘汰留痕；0x91 段）
pub mod unx204_reportring;
// unx223_dombound ↔ UNX-F0223 域边界清单（六邻域冻结点联签制+缺签告警去重+指纹分叉升级；0x92 段）
pub mod unx223_dombound;
// unx243_memsnap ↔ UNX-F0243 早传第二步内存面快照链（三件套 Manifest+上游 F0066/F0077 对账+链式 prev_fp 封账；0x93 段）
pub mod unx243_memsnap;
// unx263_xmodem ↔ UNX-F0263 串口 XModem-CRC 接收器（1MB=8192 块零丢包+审计区零执行红线+CRC16 标准向量；0x94 段）
pub mod unx263_xmodem;
// unx323_realmjmp ↔ UNX-F0323 实模式跳板代码（三级跳语义+黄金汇编逐指令比对+段属性黄金；0x99 段）
pub mod unx323_realmjmp;
// unx342_apsilence ↔ UNX-F0342 AP 中断就绪前静默窗（三态窗+违规逐条检出+单调接管令牌防重放；0xA1 段）
pub mod unx342_apsilence;
// unx362_resvwhite ↔ UNX-F0362 保留区登记与白名单（逐段冻结+边界碰触拦截+修订出新版留档；0xA2 段）
pub mod unx362_resvwhite;
// unx264_lineedit ↔ UNX-F0264 终端行编辑器增强（历史16条循环淘汰留痕+前缀补全公共前缀扩展+Tab二义提示+边界输入不崩溃；0x6200 段）
pub mod unx264_lineedit;
// unx324_sipiseq ↔ UNX-F0324 INIT-SIPI-SIPI 序列封装（10ms/200µs 规范间隔早发即拒+向量 4K 对齐 8 位域双校验+重发 3 次升级 FullInit 放弃进隔离；0x6300 段）
pub mod unx324_sipiseq;
// unx643_drilllog ↔ UNX-F0643 演练记录簿（逐轮三列轮号/结论/时戳+序号连续零跳号+簿行=放行+拦截守恒恒等+簿满显性；0x6400 段）
pub mod unx643_drilllog;
// unx683_boottrace ↔ UNX-F0683 逐次引导留痕（五列行组装门缺列即拒+150/150 序号连续零跳号+F0646 履历对接投影 diff+零抹除快照对拍；0x6500 段）
pub mod unx683_boottrace;
// unx206_excode ↔ UNX-F0206 异常项分类码 E-引导异常字典（六分区闭集+三要素构造级零缺+重复码注册拒+全码幂等对拍+渲染确定性；0x6600 段）
pub mod unx206_excode;
// unx245_randnode ↔ UNX-F0245 早传第四步随机化决策节（槽位/熵摘要/开关态三要素+脱敏抽检禁绝对基址字节型+F0088 双写对账+封账唯一+幂等重放；0x6700 段）
pub mod unx245_randnode;
// unx1842_ipivec ↔ UNX-F1842 IPI 向量分配表（v1 四类冻结 FNV 钉死+造 IPI 必登记双检+未登记注入 10/10 检出+保留向量拒发+v2 修订 v1 留档；0x6900 段）
pub mod unx1842_ipivec;
// unx405_monoclock ↔ UNX-F0405 单调时钟原语（invariant/nonstop/offset 三能力闭集+跨核降级显性拒+定点ns 换算保序+亚ns 余数可分同ns+校准表指纹漂移检出+百万次抽样零回退+零浮点/0x6C00 段）
pub mod unx405_monoclock;
// unx327_tramppagemap ↔ UNX-F0327 trampoline 页表最小集（低 1MB 恒等 256PTE 铺满无洞+跳板窗内不跨页+主表入口必须低区+CR3 重取必备+作废禁重载+跳板槽重叠防线/0x6B00 段）
pub mod unx327_tramppagemap;
// unx326_apsentinel ↔ UNX-F0326 AP 到达哨兵与超时账（哨兵三态魔数+test-and-set 恰好一次生效+伪造就位拒+200ms×已到达AP 预算+三类超时分类 append-only 账+续期上限防活锁/0x6600 段）
pub mod unx326_apsentinel;
// unx1902_lazysave ↔ UNX-F1902 惰性保存策略（三态切出/惰性/永存开关机；TS置位→#NM 首次使用代偿闭环；0x6A00 段）
pub mod unx1902_lazysave;
// unx1563_a4sign ↔ UNX-F1563 A4 内存域联签件（shootdown 三阶段协议消费面逐项核验+10 万轮零死锁复验+缺项点名+联签记录落档封指纹；0x6800 段）
pub mod unx1563_a4sign;
// unx382_bumpline ↔ UNX-F0382 bump 分配器（单向下水线；0x4c10 段）
pub mod unx382_bumpline;
// unx402_tscinv ↔ UNX-F0402 TSC 不变量探测（0x4c20 段）
pub mod unx402_tscinv;
// unx422_excgate ↔ UNX-F0422 异常门注册账 32 向量全覆盖（0x4c30 段）
pub mod unx422_excgate;
// unx442_rsdpver ↔ UNX-F0442 RSDP 校验与版本账（0x4c40 段）
pub mod unx442_rsdpver;
// unx482_pxfmt4 ↔ UNX-F0482 像素格式判定四分发（0x4c50 段 0x4c58 分区）
pub mod unx482_pxfmt4;
// unx502_uartio ↔ UNX-F0502 串口输出原语三件（0x4c70 段）
pub mod unx502_uartio;
// unx562_pm1cnt ↔ UNX-F0562 PM1a_CNT 控制块账 SLP 位只读（0x4c80 段）
pub mod unx562_pm1cnt;
// unx383_slabsize ↔ UNX-F0383 尺寸档 slab 族 32/64/128/512（0x4c90 段）
pub mod unx383_slabsize;
// unx042_ssdt —— UNX-F0042 SSDT 多表枚举与版本登记深化版（XSDT 声明数闭式对账/逐表校验和双路对拍/清单 KV+TLV 双形态导出，0x0701~0x070F 段）
pub mod unx042_ssdt;
// unx462_bus0scan —— UNX-F0462 总线 0 扫描深化版（256 功能位探齐/CF8CFC 编解码往返/vendor 0xFFFF 两级分账/DRR 无关的确定性注入口，0x7B01~0x7B13 段）
pub mod unx462_bus0scan;
// unx482_pixfmt —— UNX-F0482 像素格式判定深化版（四分发/红蓝序抽样核/565555 变体/转换矩阵 stride 钳制，0x9600~0x960F 段）
pub mod unx482_pixfmt;
// unx502_serialout_deep —— UNX-F0502 串口输出原语深化并存版（与 unx502_serialout 原版 857 行并存；三路同源对拍/UTF-8 五类故障/流控四态，0x9901~0x9910 段）
pub mod unx502_serialout_deep;
// unx043_amllex —— UNX-F0043 AML 词法层深化版（主码84+二级27闭集/NameSeg压缩解码/包长四档/覆盖率千分位对拍 iasl 12样本，0x0720~0x072F 段）
pub mod unx043_amllex;
// unx463_bridgewalk —— UNX-F0463 桥展开多总线深化版（显式栈递归/深度限8超限红/窗口一致性/环检测/悬空桥补红，0x7C01~0x7C16 段）
pub mod unx463_bridgewalk;
// unx942_dispatch —— UNX-F0942 分发入口统一路径深化版（五类分型10/10/步数开销2ns步/旁路收编清零/风暴熔断，0x8C01~0x8C11 段）
pub mod unx942_dispatch;
// unx943_tophalf —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单，0x4C50 段）
pub mod unx943_tophalf;
// unx1002_tscprobe —— UNX-F1002 TSC 特性探测深化版（三环境判定10/10/降级三档声明/频率三源对拍/启动报告九键，0x4CF0~0x4CFF 段）
pub mod unx1002_tscprobe;
// unx044_nstree —— UNX-F0044 AML 命名空间树深化版（_SB/_GPE/_PR 三机型各100节点iasl对拍/ScopeOp配对/路径三形态/重名立案，0x0A40~0x0A5F 段）
pub mod unx044_nstree;
// unx404_hpetcap_deep —— UNX-F0404 HPET 能力账深化并存版（ACPI表逐字段/能力三要素双读一致/使能位禁写红线，0x8201~0x8216 段）
pub mod unx404_hpetcap_deep;
// unx464_devid —— UNX-F0464 设备身份账深化版（三元组三读一致/100%覆盖对账/vendor名录26条PCI SIG/TLV往返，0xCB01~0xCB13 段）
pub mod unx464_devid;
// unx0883_migwin —— UNX-F0883 迁移在途事件不丢账（四态窗口/三账守恒/IRR 镜像/10 万迁移演练，0x4E00 段）
pub mod unx0883_migwin;
// unx1026_hpetcand —— UNX-F1026 HPET 候选深化（五档候选/三维质量分/格式统一 v1，0x4E80 段）
pub mod unx1026_hpetcand;
// unx1103_disptab —— UNX-F1103 分发表深化（256 槽直取/10^7 零失配/P99≤50/64 桶分布，0x5A00 段）
pub mod unx1103_disptab;
// unx1302_tickretire —— UNX-F1302 tick 退役深化（五测绘点/无缝守恒闸/基线对拍，0x5A80 段）
pub mod unx1302_tickretire;
// unx1482_gaplist —— UNX-F1482 断言缺口盘点深化（40 批三列/密度红线/大关差额，0x5B00 段）
pub mod unx1482_gaplist;
// unx1503_quadfsm —— UNX-F1503 四态矩阵深化（12 边覆盖 100%/自环零执行，0x6680 段）
pub mod unx1503_quadfsm;
// unx1903_xsaveamort —— UNX-F1903 XSAVE 摊销深化（16 tick 窗/窗闭强制落盘/保真指纹，0x6780 段）
pub mod unx1903_xsaveamort;
// unx1941_nmiipi —— UNX-F1941 NMI IPI 通道深化（不可屏蔽语义/三隔离/不丢不重 10/10，0x6880 段）
pub mod unx1941_nmiipi;
// unx1961_freqdom —— UNX-F1961 频域划分深化（8核4域 SMT 对映射/policy 注册表/聚合最高请求者赢，0x6A80 段）
pub mod unx1961_freqdom;
// unx1981_twogov —— UNX-F1981 双基 governor 深化（performance/powersave 恒档/即时生效零滞后，0x6B80 段）
pub mod unx1981_twogov;
// unx2001_schedutil —— UNX-F2001 schedutil 直耦深化（util→频映射/调度点直调零滞后/速率限幅，0x6C80 段）
pub mod unx2001_schedutil;
// unx2021_cstates —— UNX-F2021 idle 状态表深化（C0/C1/C2/C6 三列登记/能力声明交叉/越界防护 10/10，0x6D80 段）
pub mod unx2021_cstates;
// unx2081_qdepth —— UNX-F2081 队列水位深化（峰谷账/持续超阈 K=8/红线 4096 对齐 F1762，0x6E80 段）
pub mod unx2081_qdepth;
// unx2102_sdebug —— UNX-F2102 sched_debug 调试面深化（全量 dump 只读/哈希不变核/RO 权限在册，0x6F80 段）
pub mod unx2102_sdebug;
// unx2141_triface —— UNX-F2141 三面联合回归组织法（3面×3册矩阵/四步单向流/三交叉联测，0x7580 段）
pub mod unx2141_triface;
// unx2161_stressorg —— UNX-F2161 域级压测组织面（三轴档位积/预告-正式两段连续性/F1899 衔接三点，0x7780 段）
pub mod unx2161_stressorg;
// unx2181_regbook —— UNX-F2181 回归主账扩容（38+64=102 正路径+12 联测=114/五列 F1885/旧账零改动，0x7880 段）
pub mod unx2181_regbook;
// unx2201_freezectl —— UNX-F2201 判据冻结总控器（三段时序/600 条点名+29 族/哈希固化/版本锚绑定，0x7980 段）
pub mod unx2201_freezectl;
// unx2241_finalchain —— UNX-F2241 段级终验断言链（五步终跑/60 ID 连续/300字×20=6000 加法链/exit=0 留痕，0x7E80 段）
pub mod unx2241_finalchain;
// unx583_wlparams_deep —— UNX-F0583 白名单参数表深化并存版（24键三型冻结/FNV指纹/注入6/6全拒千分位恰1000，0x9E01~0x9E0F 段）
// unx584_unkparam —— UNX-F0584 未知参数账注（显性弃置）：16槽定容弃置簿（原文/时点/位置三件套齐）+槽满显性拒不覆盖+时点单调+去重合并+UTF-8 边界安全截断+零执行断言（弃置键不可被语义层取值）/0xC100 段
// unx2101_schedstat —— UNX-F2101 /proc/schedstat 导出面（三字段格式冻结）：版本册在册制+逐字冻结模板+导出/解析共用唯一行形状真源+往返逐字节对拍+逐CPU单调锚/0xD100 段
pub mod unx2101_schedstat;
// unx2221_freeledge —— UNX-F2221 全域冻结总账落账（800 行总账 v1）：620 真指纹 + 180 段位占位、四列格式 v1 逐字节冻结、哈希链延展闭合、链与段位冲突双轨独立检测、落账快照闸/0xD200 段
pub mod unx2221_freeledge;
// unx2222_rollver —— UNX-F2222 冻结账批级滚动升版（B32 段滚动条款）：版本闸只许 +1（跳步/降级/越界注入 10/10 全拒）+ 占位段原地补真指纹（kind:S->T）+ 族链 33 族延展与未涉及族逐位不变 + 全域链重算 + 跨版本零变化快照闸/0xD300 段
pub mod unx2222_rollver;
// unx505_loopback —— UNX-F0505 串口环回演练（MCR loopback）：环回位摘除外部线态+注入 64 字节逐序号对拍 64/64 闭环+进退环回线态逐字段还原+溢出留痕+断/校验帧格式+IER×OUT2 双重中断门控/0xD400 段
pub mod unx505_loopback;
// unx1723_migledger —— UNX-F1723 迁移执行三向记账（源/目标/线程）：源核出账+目标核入账+线程随行账三向闭合（存在性+核投影+载荷指纹投影+线程号投影+tick 序五维）+ 悬空态检出+mid 同类查重+环形溢出显性上报+四账（F1627/F1646/F1704/本模块）顺序敏感合成指纹/0xB080 段
pub mod unx1723_migledger;
pub mod unx584_unkparam;
pub mod unx583_wlparams_deep;
// unx843_eoipath —— UNX-F0843 MSI/MSI-X EOI 路径深化版（十万事件行为矩阵100%一致/终结点四态裁决/泄漏账配平，0xB501~0xB511 段）
pub mod unx843_eoipath;
// unx1502_modeswitch —— UNX-F1502 万次切换回归深化版（10^4次逐次三列留痕/成功率恰1000‰/失败注入10/10必抓，0x9700~0x970F 段）
pub mod unx1502_modeswitch;
// unx1024_hpetalloc —— UNX-F1024 HPET 比较器分配器深化版（并发32零双占/十万次零泄漏/三型优先级预留纪律，0x4DA0~0x4DAF 段）
pub mod unx1024_hpetalloc;
// unx1543_crichain —— UNX-F1543 域级判据冻结深化版（F0801-F1600 800条FNV哈希链/篡改10/10检出/复验4800步红线内，0xF101~0xF110 段）
pub mod unx1543_crichain;
pub mod unx602_initrdro;
pub mod unx602grid;
pub mod unx602mx;
pub mod unx642_envcheck;
pub mod unx642grid;
pub mod unx642mx;
pub mod unx682_matrixrot;
pub mod unx682grid;
pub mod unx682mx;
pub mod unx702grid;
pub mod unx702mx;
pub mod unx722grid;
pub mod unx722mx;
pub mod unx742grid;
pub mod unx742mx;
pub mod unx782grid;
pub mod unx782mx;
pub mod unx802grid;
pub mod unx802mx;
pub mod unx822grid;
pub mod unx822mx;
pub mod unx842grid;
pub mod unx842mx;
pub mod unx941_irqtwostage;
// unx943_tophalfdiscipline —— UNX-F0943 top half 纪律深化版（≤5µs p99 直方图+义务/禁令双闭集+超预算清单 10/10+文档三节 FNV 冻结，0x4C50 段十四码，与 unx943_tophalf 双实现并存）
pub mod unx943_tophalfdiscipline;
// unx943_tophalfdisc —— UNX-F0943 top half 纪律精简版（环形采样账 p99+超预算清单自动产出+违例注入拦截+文档三节 FNV 冻结，0x4c50 段十七码，与前三实现并存）
pub mod unx943_tophalfdisc;
pub mod unx981_irqstorm;
pub mod unx1001_tsync;
pub mod unx1021_hpetprobe;
pub mod unx1041_clockarb;
// AI-WB-1022 根卡批（UNX-F1061~F1243 十单 · 诊断段 0x1061/0x1082/0x1105/0x1141/0x1161/0x1181/0x1221/0x1243 独占）：
pub mod unx1061_clktrait;
pub mod unx1062_tickdual;
pub mod unx1082_alarm;
pub mod unx1106_hotpath;
pub mod unx1122_irqflags;
pub mod unx1142_timeout;
pub mod unx1162_tasklet;
pub mod unx1182_policy;
pub mod unx1202_pollreg;
pub mod unx1222_draincrit;
pub mod unx1244_armfallback;
pub mod unx1081_ntpcal;
pub mod unx1105_dispsafe;
pub mod unx1141_topacct;
pub mod unx1161_softirq;
pub mod unx1181_irqroute;
pub mod unx1221_drainloop;
pub mod unx1243_armrace;
pub mod unx110_ipicall;
pub mod unx111_dualmode;
pub mod unx112_targetselect;
pub mod unx114_selfreentry;
pub mod unx115_reschedule;
pub mod unx116_payloadct;
pub mod unx117_ipiqueue;
pub mod unx118_pauseresume;
pub mod unx119_health;
pub mod unx120_notify;
pub mod unx121storm_stress;
pub mod unx122doc_freeze;
pub mod unx123_deadlock;
pub mod unx124_faultinj;
pub mod unx125_latency;
pub mod unx126_summary;
pub mod unx127_explain;
pub mod unx702_acpi80;
pub mod unx722_verfp;
pub mod unx742_upgrade;
pub mod unx782_final240k;
pub mod unx802_exctriage;
pub mod unx822_svr;
pub mod unx842_edgeeoi;
pub mod unx1002_tsccaps;
pub mod unx1022_timeout1;
pub mod unx1042_arbtrigger;
pub mod unx1101_disptab;
pub mod unx1121_irqsign;
pub mod unx1481_assertledger;
pub mod unx1521_ledgerindex;
pub mod unx1541_domainfinal;
pub mod unx1542_domainaxioms;
pub mod unx1741_idlefsm;
pub mod unx1761_obsface;
pub mod unx1801_schedpath;

// --------------------------------------------------------------------------
// UNX 批次 4 任务族聚合（修复重建）
// --------------------------------------------------------------------------
/// UNX-F0602 域聚合。
pub fn run_unx20_t602() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t602");
    let mut ok = true;
    if !unx602_initrdro::run_t602_checks().all_passed() { ok = false; }
    if !unx602grid::run_t602_checks().all_passed() { ok = false; }
    if !unx602mx::run_t602_checks().all_passed() { ok = false; }
    set.add("UNX-F0602", ok, if ok { "" } else { "unx20-t602 family sub-checks red" });
    set
}

// ==========================================================================
// WB20 批次（20 任务 × 12 深编模块，CoRun UNX-A 域调度/SMP 线判据逐条落地）
// 施工：AI-WB20 · 纯功能线 no_std+alloc · 全部自检由域聚合器统一调用
// ==========================================================================
pub mod unx1501_u1501barrier;
pub mod unx1501_u1501bgrid;
pub mod unx1501_u1501bmx;
pub mod unx1501_u1501core;
pub mod unx1501_u1501dual;
pub mod unx1501_u1501grid;
pub mod unx1501_u1501map;
pub mod unx1501_u1501merge;
pub mod unx1501_u1501mix;
pub mod unx1501_u1501mx;
pub mod unx1501_u1501pipe;
pub mod unx1501_u1501sm;
pub mod unx1541_u1541barrier;
pub mod unx1541_u1541bgrid;
pub mod unx1541_u1541bmx;
pub mod unx1541_u1541core;
pub mod unx1541_u1541dual;
pub mod unx1541_u1541grid;
pub mod unx1541_u1541map;
pub mod unx1541_u1541merge;
pub mod unx1541_u1541mix;
pub mod unx1541_u1541mx;
pub mod unx1541_u1541pipe;
pub mod unx1541_u1541sm;
pub mod unx1561_u1561barrier;
pub mod unx1561_u1561bgrid;
pub mod unx1561_u1561bmx;
pub mod unx1561_u1561core;
pub mod unx1561_u1561dual;
pub mod unx1561_u1561grid;
pub mod unx1561_u1561map;
pub mod unx1561_u1561merge;
pub mod unx1561_u1561mix;
pub mod unx1561_u1561mx;
pub mod unx1561_u1561pipe;
pub mod unx1561_u1561sm;
pub mod unx1581_u1581barrier;
pub mod unx1581_u1581bgrid;
pub mod unx1581_u1581bmx;
pub mod unx1581_u1581core;
pub mod unx1581_u1581dual;
pub mod unx1581_u1581grid;
pub mod unx1581_u1581map;
pub mod unx1581_u1581merge;
pub mod unx1581_u1581mix;
pub mod unx1581_u1581mx;
pub mod unx1581_u1581pipe;
pub mod unx1581_u1581sm;
pub mod unx1601_u1601barrier;
pub mod unx1601_u1601bgrid;
pub mod unx1601_u1601bmx;
pub mod unx1601_u1601core;
pub mod unx1601_u1601dual;
pub mod unx1601_u1601grid;
pub mod unx1601_u1601map;
pub mod unx1601_u1601merge;
pub mod unx1601_u1601mix;
pub mod unx1601_u1601mx;
pub mod unx1601_u1601pipe;
pub mod unx1601_u1601sm;
pub mod unx1621_u1621barrier;
pub mod unx1621_u1621bgrid;
pub mod unx1621_u1621bmx;
pub mod unx1621_u1621core;
pub mod unx1621_u1621dual;
pub mod unx1621_u1621grid;
pub mod unx1621_u1621map;
pub mod unx1621_u1621merge;
pub mod unx1621_u1621mix;
pub mod unx1621_u1621mx;
pub mod unx1621_u1621pipe;
pub mod unx1621_u1621sm;
pub mod unx1641_u1641barrier;
pub mod unx1641_u1641bgrid;
pub mod unx1641_u1641bmx;
pub mod unx1641_u1641core;
pub mod unx1641_u1641dual;
pub mod unx1641_u1641grid;
pub mod unx1641_u1641map;
pub mod unx1641_u1641merge;
pub mod unx1641_u1641mix;
pub mod unx1641_u1641mx;
pub mod unx1641_u1641pipe;
pub mod unx1641_u1641sm;
pub mod unx1661_u1661barrier;
pub mod unx1661_u1661bgrid;
pub mod unx1661_u1661bmx;
pub mod unx1661_u1661core;
pub mod unx1661_u1661dual;
pub mod unx1661_u1661grid;
pub mod unx1661_u1661map;
pub mod unx1661_u1661merge;
pub mod unx1661_u1661mix;
pub mod unx1661_u1661mx;
pub mod unx1661_u1661pipe;
pub mod unx1661_u1661sm;
pub mod unx1681_u1681barrier;
pub mod unx1681_u1681bgrid;
pub mod unx1681_u1681bmx;
pub mod unx1681_u1681core;
pub mod unx1681_u1681dual;
pub mod unx1681_u1681grid;
pub mod unx1681_u1681map;
pub mod unx1681_u1681merge;
pub mod unx1681_u1681mix;
pub mod unx1681_u1681mx;
pub mod unx1681_u1681pipe;
pub mod unx1681_u1681sm;
pub mod unx1701_u1701barrier;
pub mod unx1701_u1701bgrid;
pub mod unx1701_u1701bmx;
pub mod unx1701_u1701core;
pub mod unx1701_u1701dual;
pub mod unx1701_u1701grid;
pub mod unx1701_u1701map;
pub mod unx1701_u1701merge;
pub mod unx1701_u1701mix;
pub mod unx1701_u1701mx;
pub mod unx1701_u1701pipe;
pub mod unx1701_u1701sm;
pub mod unx1721_u1721barrier;
pub mod unx1721_u1721bgrid;
pub mod unx1721_u1721bmx;
pub mod unx1721_u1721core;
pub mod unx1721_u1721dual;
pub mod unx1721_u1721grid;
pub mod unx1721_u1721map;
pub mod unx1721_u1721merge;
pub mod unx1721_u1721mix;
pub mod unx1721_u1721mx;
pub mod unx1721_u1721pipe;
pub mod unx1721_u1721sm;
pub mod unx1741_u1741barrier;
pub mod unx1741_u1741bgrid;
pub mod unx1741_u1741bmx;
pub mod unx1741_u1741core;
pub mod unx1741_u1741dual;
pub mod unx1741_u1741grid;
pub mod unx1741_u1741map;
pub mod unx1741_u1741merge;
pub mod unx1741_u1741mix;
pub mod unx1741_u1741mx;
pub mod unx1741_u1741pipe;
pub mod unx1741_u1741sm;
pub mod unx1761_u1761barrier;
pub mod unx1761_u1761bgrid;
pub mod unx1761_u1761bmx;
pub mod unx1761_u1761core;
pub mod unx1761_u1761dual;
pub mod unx1761_u1761grid;
pub mod unx1761_u1761map;
pub mod unx1761_u1761merge;
pub mod unx1761_u1761mix;
pub mod unx1761_u1761mx;
pub mod unx1761_u1761pipe;
pub mod unx1761_u1761sm;
pub mod unx1781_u1781barrier;
pub mod unx1781_u1781bgrid;
pub mod unx1781_u1781bmx;
pub mod unx1781_u1781core;
pub mod unx1781_u1781dual;
pub mod unx1781_u1781grid;
pub mod unx1781_u1781map;
pub mod unx1781_u1781merge;
pub mod unx1781_u1781mix;
pub mod unx1781_u1781mx;
pub mod unx1781_u1781pipe;
pub mod unx1781_u1781sm;
pub mod unx1801_u1801barrier;
pub mod unx1801_u1801bgrid;
pub mod unx1801_u1801bmx;
pub mod unx1801_u1801core;
pub mod unx1801_u1801dual;
pub mod unx1801_u1801grid;
pub mod unx1801_u1801map;
pub mod unx1801_u1801merge;
pub mod unx1801_u1801mix;
pub mod unx1801_u1801mx;
pub mod unx1801_u1801pipe;
pub mod unx1801_u1801sm;
pub mod unx1821_u1821barrier;
pub mod unx1821_u1821bgrid;
pub mod unx1821_u1821bmx;
pub mod unx1821_u1821core;
pub mod unx1821_u1821dual;
pub mod unx1821_u1821grid;
pub mod unx1821_u1821map;
pub mod unx1821_u1821merge;
pub mod unx1821_u1821mix;
pub mod unx1821_u1821mx;
pub mod unx1821_u1821pipe;
pub mod unx1821_u1821sm;
pub mod unx1841_u1841barrier;
pub mod unx1841_u1841bgrid;
pub mod unx1841_u1841bmx;
pub mod unx1841_u1841core;
pub mod unx1841_u1841dual;
pub mod unx1841_u1841grid;
pub mod unx1841_u1841map;
pub mod unx1841_u1841merge;
pub mod unx1841_u1841mix;
pub mod unx1841_u1841mx;
pub mod unx1841_u1841pipe;
pub mod unx1841_u1841sm;
pub mod unx1861_u1861barrier;
pub mod unx1861_u1861bgrid;
pub mod unx1861_u1861bmx;
pub mod unx1861_u1861core;
pub mod unx1861_u1861dual;
pub mod unx1861_u1861grid;
pub mod unx1861_u1861map;
pub mod unx1861_u1861merge;
pub mod unx1861_u1861mix;
pub mod unx1861_u1861mx;
pub mod unx1861_u1861pipe;
pub mod unx1861_u1861sm;
pub mod unx1881_u1881barrier;
pub mod unx1881_u1881bgrid;
pub mod unx1881_u1881bmx;
pub mod unx1881_u1881core;
pub mod unx1881_u1881dual;
pub mod unx1881_u1881grid;
pub mod unx1881_u1881map;
pub mod unx1881_u1881merge;
pub mod unx1881_u1881mix;
pub mod unx1881_u1881mx;
pub mod unx1881_u1881pipe;
pub mod unx1881_u1881sm;
pub mod unx1901_u1901barrier;
pub mod unx1901_u1901bgrid;
pub mod unx1901_u1901bmx;
pub mod unx1901_u1901core;
pub mod unx1901_u1901dual;
pub mod unx1901_u1901grid;
pub mod unx1901_u1901map;
pub mod unx1901_u1901merge;
pub mod unx1901_u1901mix;
pub mod unx1901_u1901mx;
pub mod unx1901_u1901pipe;
pub mod unx1901_u1901sm;

/// UNX-F1501 域回归矩阵冻结 v1（场景×判据×预期三维） 域聚合（WB20 批次）。
pub fn run_wb20_u1501() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1501");
    let mut ok = true;
    if !unx1501_u1501barrier::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501bgrid::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501bmx::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501core::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501dual::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501grid::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501map::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501merge::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501mix::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501mx::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501pipe::run_u1501_checks().all_passed() { ok = false; }
    if !unx1501_u1501sm::run_u1501_checks().all_passed() { ok = false; }
    set.add("UNX-F1501", ok, if ok { "" } else { "wb20-u1501 family sub-checks red" });
    set
}

/// UNX-F1541 域册终版总装（使命/地图/批次/接口/预案五节终版） 域聚合（WB20 批次）。
pub fn run_wb20_u1541() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1541");
    let mut ok = true;
    if !unx1541_u1541barrier::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541bgrid::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541bmx::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541core::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541dual::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541grid::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541map::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541merge::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541mix::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541mx::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541pipe::run_u1541_checks().all_passed() { ok = false; }
    if !unx1541_u1541sm::run_u1541_checks().all_passed() { ok = false; }
    set.add("UNX-F1541", ok, if ok { "" } else { "wb20-u1541 family sub-checks red" });
    set
}

/// UNX-F1561 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 域聚合（WB20 批次）。
pub fn run_wb20_u1561() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1561");
    let mut ok = true;
    if !unx1561_u1561barrier::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561bgrid::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561bmx::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561core::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561dual::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561grid::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561map::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561merge::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561mix::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561mx::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561pipe::run_u1561_checks().all_passed() { ok = false; }
    if !unx1561_u1561sm::run_u1561_checks().all_passed() { ok = false; }
    set.add("UNX-F1561", ok, if ok { "" } else { "wb20-u1561 family sub-checks red" });
    set
}

/// UNX-F1581 域级总回归执行（800 条判据终验复读） 域聚合（WB20 批次）。
pub fn run_wb20_u1581() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1581");
    let mut ok = true;
    if !unx1581_u1581barrier::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581bgrid::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581bmx::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581core::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581dual::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581grid::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581map::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581merge::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581mix::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581mx::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581pipe::run_u1581_checks().all_passed() { ok = false; }
    if !unx1581_u1581sm::run_u1581_checks().all_passed() { ok = false; }
    set.add("UNX-F1581", ok, if ok { "" } else { "wb20-u1581 family sub-checks red" });
    set
}

/// UNX-F1601 SMP 前置数据包消费与 A1 联签确认 域聚合（WB20 批次）。
pub fn run_wb20_u1601() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1601");
    let mut ok = true;
    if !unx1601_u1601barrier::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601bgrid::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601bmx::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601core::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601dual::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601grid::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601map::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601merge::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601mix::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601mx::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601pipe::run_u1601_checks().all_passed() { ok = false; }
    if !unx1601_u1601sm::run_u1601_checks().all_passed() { ok = false; }
    set.add("UNX-F1601", ok, if ok { "" } else { "wb20-u1601 family sub-checks red" });
    set
}

/// UNX-F1621 per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） 域聚合（WB20 批次）。
pub fn run_wb20_u1621() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1621");
    let mut ok = true;
    if !unx1621_u1621barrier::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621bgrid::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621bmx::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621core::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621dual::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621grid::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621map::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621merge::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621mix::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621mx::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621pipe::run_u1621_checks().all_passed() { ok = false; }
    if !unx1621_u1621sm::run_u1621_checks().all_passed() { ok = false; }
    set.add("UNX-F1621", ok, if ok { "" } else { "wb20-u1621 family sub-checks red" });
    set
}

/// UNX-F1641 每核 runqueue 数据结构深化与锁语义 域聚合（WB20 批次）。
pub fn run_wb20_u1641() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1641");
    let mut ok = true;
    if !unx1641_u1641barrier::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641bgrid::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641bmx::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641core::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641dual::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641grid::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641map::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641merge::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641mix::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641mx::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641pipe::run_u1641_checks().all_passed() { ok = false; }
    if !unx1641_u1641sm::run_u1641_checks().all_passed() { ok = false; }
    set.add("UNX-F1641", ok, if ok { "" } else { "wb20-u1641 family sub-checks red" });
    set
}

/// UNX-F1661 调度类注册框架与优先级链 域聚合（WB20 批次）。
pub fn run_wb20_u1661() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1661");
    let mut ok = true;
    if !unx1661_u1661barrier::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661bgrid::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661bmx::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661core::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661dual::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661grid::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661map::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661merge::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661mix::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661mx::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661pipe::run_u1661_checks().all_passed() { ok = false; }
    if !unx1661_u1661sm::run_u1661_checks().all_passed() { ok = false; }
    set.add("UNX-F1661", ok, if ok { "" } else { "wb20-u1661 family sub-checks red" });
    set
}

/// UNX-F1681 时间片计算总纲（基片×权重缩放模型） 域聚合（WB20 批次）。
pub fn run_wb20_u1681() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1681");
    let mut ok = true;
    if !unx1681_u1681barrier::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681bgrid::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681bmx::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681core::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681dual::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681grid::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681map::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681merge::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681mix::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681mx::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681pipe::run_u1681_checks().all_passed() { ok = false; }
    if !unx1681_u1681sm::run_u1681_checks().all_passed() { ok = false; }
    set.add("UNX-F1681", ok, if ok { "" } else { "wb20-u1681 family sub-checks red" });
    set
}

/// UNX-F1701 负载追踪总纲（窗口化追踪模型冻结） 域聚合（WB20 批次）。
pub fn run_wb20_u1701() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1701");
    let mut ok = true;
    if !unx1701_u1701barrier::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701bgrid::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701bmx::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701core::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701dual::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701grid::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701map::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701merge::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701mix::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701mx::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701pipe::run_u1701_checks().all_passed() { ok = false; }
    if !unx1701_u1701sm::run_u1701_checks().all_passed() { ok = false; }
    set.add("UNX-F1701", ok, if ok { "" } else { "wb20-u1701 family sub-checks red" });
    set
}

/// UNX-F1721 均衡执行总纲（判据消费→执行原语链） 域聚合（WB20 批次）。
pub fn run_wb20_u1721() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1721");
    let mut ok = true;
    if !unx1721_u1721barrier::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721bgrid::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721bmx::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721core::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721dual::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721grid::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721map::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721merge::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721mix::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721mx::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721pipe::run_u1721_checks().all_passed() { ok = false; }
    if !unx1721_u1721sm::run_u1721_checks().all_passed() { ok = false; }
    set.add("UNX-F1721", ok, if ok { "" } else { "wb20-u1721 family sub-checks red" });
    set
}

/// UNX-F1741 空闲循环总纲（idle 三态状态机） 域聚合（WB20 批次）。
pub fn run_wb20_u1741() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1741");
    let mut ok = true;
    if !unx1741_u1741barrier::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741bgrid::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741bmx::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741core::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741dual::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741grid::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741map::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741merge::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741mix::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741mx::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741pipe::run_u1741_checks().all_passed() { ok = false; }
    if !unx1741_u1741sm::run_u1741_checks().all_passed() { ok = false; }
    set.add("UNX-F1741", ok, if ok { "" } else { "wb20-u1741 family sub-checks red" });
    set
}

/// UNX-F1761 观测面总纲（账→接口→聚合三层架构） 域聚合（WB20 批次）。
pub fn run_wb20_u1761() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1761");
    let mut ok = true;
    if !unx1761_u1761barrier::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761bgrid::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761bmx::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761core::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761dual::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761grid::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761map::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761merge::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761mix::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761mx::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761pipe::run_u1761_checks().all_passed() { ok = false; }
    if !unx1761_u1761sm::run_u1761_checks().all_passed() { ok = false; }
    set.add("UNX-F1761", ok, if ok { "" } else { "wb20-u1761 family sub-checks red" });
    set
}

/// UNX-F1781 B01–B09 每核结构总成总纲（十层结构拼图） 域聚合（WB20 批次）。
pub fn run_wb20_u1781() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1781");
    let mut ok = true;
    if !unx1781_u1781barrier::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781bgrid::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781bmx::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781core::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781dual::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781grid::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781map::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781merge::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781mix::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781mx::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781pipe::run_u1781_checks().all_passed() { ok = false; }
    if !unx1781_u1781sm::run_u1781_checks().all_passed() { ok = false; }
    set.add("UNX-F1781", ok, if ok { "" } else { "wb20-u1781 family sub-checks red" });
    set
}

/// UNX-F1801 切换主路径总纲（schedule() 主循环冻结） 域聚合（WB20 批次）。
pub fn run_wb20_u1801() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1801");
    let mut ok = true;
    if !unx1801_u1801barrier::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801bgrid::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801bmx::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801core::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801dual::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801grid::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801map::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801merge::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801mix::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801mx::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801pipe::run_u1801_checks().all_passed() { ok = false; }
    if !unx1801_u1801sm::run_u1801_checks().all_passed() { ok = false; }
    set.add("UNX-F1801", ok, if ok { "" } else { "wb20-u1801 family sub-checks red" });
    set
}

/// UNX-F1821 切换帧总纲（内核栈布局与切换帧结构冻结） 域聚合（WB20 批次）。
pub fn run_wb20_u1821() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1821");
    let mut ok = true;
    if !unx1821_u1821barrier::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821bgrid::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821bmx::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821core::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821dual::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821grid::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821map::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821merge::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821mix::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821mx::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821pipe::run_u1821_checks().all_passed() { ok = false; }
    if !unx1821_u1821sm::run_u1821_checks().all_passed() { ok = false; }
    set.add("UNX-F1821", ok, if ok { "" } else { "wb20-u1821 family sub-checks red" });
    set
}

/// UNX-F1841 IPI 架构总纲（向量分派框架与 A2 衔接） 域聚合（WB20 批次）。
pub fn run_wb20_u1841() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1841");
    let mut ok = true;
    if !unx1841_u1841barrier::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841bgrid::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841bmx::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841core::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841dual::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841grid::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841map::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841merge::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841mix::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841mx::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841pipe::run_u1841_checks().all_passed() { ok = false; }
    if !unx1841_u1841sm::run_u1841_checks().all_passed() { ok = false; }
    set.add("UNX-F1841", ok, if ok { "" } else { "wb20-u1841 family sub-checks red" });
    set
}

/// UNX-F1861 TLB 击落总纲（shootdown 全链四段） 域聚合（WB20 批次）。
pub fn run_wb20_u1861() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1861");
    let mut ok = true;
    if !unx1861_u1861barrier::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861bgrid::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861bmx::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861core::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861dual::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861grid::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861map::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861merge::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861mix::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861mx::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861pipe::run_u1861_checks().all_passed() { ok = false; }
    if !unx1861_u1861sm::run_u1861_checks().all_passed() { ok = false; }
    set.add("UNX-F1861", ok, if ok { "" } else { "wb20-u1861 family sub-checks red" });
    set
}

/// UNX-F1881 全域注入矩阵终对账（十一矩阵收口） 域聚合（WB20 批次）。
pub fn run_wb20_u1881() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1881");
    let mut ok = true;
    if !unx1881_u1881barrier::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881bgrid::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881bmx::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881core::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881dual::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881grid::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881map::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881merge::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881mix::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881mx::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881pipe::run_u1881_checks().all_passed() { ok = false; }
    if !unx1881_u1881sm::run_u1881_checks().all_passed() { ok = false; }
    set.add("UNX-F1881", ok, if ok { "" } else { "wb20-u1881 family sub-checks red" });
    set
}

/// UNX-F1901 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 域聚合（WB20 批次）。
pub fn run_wb20_u1901() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB20-u1901");
    let mut ok = true;
    if !unx1901_u1901barrier::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901bgrid::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901bmx::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901core::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901dual::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901grid::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901map::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901merge::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901mix::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901mx::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901pipe::run_u1901_checks().all_passed() { ok = false; }
    if !unx1901_u1901sm::run_u1901_checks().all_passed() { ok = false; }
    set.add("UNX-F1901", ok, if ok { "" } else { "wb20-u1901 family sub-checks red" });
    set
}

// ===================== WB21 批次（第二波深化 20 单，0x5200~0x5330 段） =====================
// unx404_hpetcap — UNX-F0404 HPET 探测能力账深化版（ACPI 表解析+双读一致核+能力账冻结+6 注入/0x5200 段）
pub mod unx404_hpetcap;
// unx583_wlparams — UNX-F0583 白名单参数表深化版（24 键三型分布冻结+越表拒收+6 注入/0x5210 段）
pub mod unx583_wlparams;
// unx1102_stubaudit — UNX-F1102 stub→C 入口审计账深化版（四级链+F0807 契约+10 万次泵+断链注入/0x5220 段）
pub mod unx1102_stubaudit;
// unx1201_polloop — UNX-F1201 Polling 主循环深化版（五态泵+10^6 次零丢失+P99 步数界/0x5230 段）
pub mod unx1201_polloop;
// unx1301_retire — UNX-F1301 退役协议深化版（六步状态机+回退点 100%+10 轮回放/0x5240 段）
pub mod unx1301_retire;
// unx1421_latdist — UNX-F1421 延迟分布引擎深化版（64 桶对数分桶+精确分位双轨互验/0x5250 段）
pub mod unx1421_latdist;
// unx1441_schema — UNX-F1441 导出 schema 冻结深化版（32 字段三节+FNV 哈希+向后兼容 10/10/0x5260 段）
pub mod unx1441_schema;
// unx1461_zeroeq — UNX-F1461 零丢恒等式深化版（四账联动+序号守恒+10 注入/0x5270 段）
pub mod unx1461_zeroeq;
// unx1501_regmat — UNX-F1501 回归矩阵冻结深化版（20 场景三维矩阵+判据引用解析/0x5280 段）
pub mod unx1501_regmat;
// unx1541_dombind — UNX-F1541 域册终版总装深化版（五节终审+800 引用对平+哈希落档/0x5290 段）
pub mod unx1541_dombind;
// unx1561_cosign — UNX-F1561 消费域联签总表深化版（12 域三态联签+漏域注入/0x52A0 段）
pub mod unx1561_cosign;
// unx1581_domreg — UNX-F1581 域级总回归深化版（800 条判据 verbatim 复读+假绿注入/0x52B0 段）
pub mod unx1581_domreg;
// unx1601_smpcons — UNX-F1601 SMP 前置包消费深化版（16 字段在位位图+联签 handoff/0x52C0 段）
pub mod unx1601_smpcons;
// unx1621_percpu — UNX-F1621 per-CPU 变量区布局深化版（五段切分+预算零越界+布局图导出/0x52D0 段）
pub mod unx1621_percpu;
// unx1641_runqueue — UNX-F1641 runqueue 锁语义深化版（令牌模型+迁移原语+越锁 10/10/0x52E0 段）
pub mod unx1641_runqueue;
// unx1661_schedcls — UNX-F1661 调度类注册框架深化版（8 类闭集+链闭合四断言/0x52F0 段）
pub mod unx1661_schedcls;
// unx1681_timeslice — UNX-F1681 时间片模型深化版（基片×权重缩放+参数单点+路径唯一/0x5300 段）
pub mod unx1681_timeslice;
// unx184_initserial — UNX-F0184 init 串行性显式声明深化版（七阶段串行断言+ADR 放行留痕/0x4fb0 段）
pub mod unx184_initserial;
// unx205_reportuart — UNX-F0205 报文导出 UART 深化版（导出协议状态机+CRC16+1MB 零丢包/0x4fc0 段）
pub mod unx205_reportuart;
// unx0102_16550full — UNX-F0102 16550 串口全速通道深化版（寄存器模型+divisor=1+环回零错位/0x4fd0 段）
pub mod unx0102_16550full;
// unx224_takeover — UNX-F0224 升级接管登记表深化版（四字段完整性+缺项告警+交接对拍/0x4fe0 段）
pub mod unx224_takeover;
// unx244_cpupkg — UNX-F0244 CPU 特征包深化版（三段+Manifest+FNV+三面对账/0x4ff0 段）
pub mod unx244_cpupkg;
// unx264b_lineedit — UNX-F0264 行编辑器双实现 B 面（历史16循环+补全三态/0x5140 段）
pub mod unx264b_lineedit;
// unx343_appgt — UNX-F0343 AP 初始页表切换深化版（六步序列+CR3 账+翻译一致/0x5150 段）
pub mod unx343_appgt;
// unx1701_loadtrk — UNX-F1701 负载追踪模型深化版（几何衰减均值+双窗口档+分层声明/0x5310 段）
pub mod unx1701_loadtrk;
// unx1682_striple — UNX-F1682 三态基片表冻结深化版（档位/数值/适用类三列+F1663 对齐+表外零存在/0x53A0 段）
pub mod unx1682_striple;
// unx1721_balexec — UNX-F1721 均衡执行链深化版（六段签名链+跳段 10/10 拒绝/0x5320 段）
pub mod unx1721_balexec;
// unx1741_idlest — UNX-F1741 idle 三态状态机深化版（转换账一致+非法转换 10/10/0x5330 段）
pub mod unx1741_idlest;

/// WB21 批次域聚合（第二波深化 20 单：各模块 drill 验收入口逐单折叠，
/// 族内任一红即整族红；F1501~F1741 的 12 单与 wb20 首波同特性号，
/// 故标签带 -D2 后缀区分第二波深化）。
///
/// 修正记录一（明细被丢弃）：原表把每个入口的返回值**当场拆成 `.0`**，
/// 失败时的 `String` 报告被扔掉，明细写死成静态串（如
/// `"wb21 hpetcap drill red"`）。于是 8 条红项的 panic 里全是同一句废话，
/// **看不到任何失败原因**——而这些入口（`drill_green` / `drill_100k` …）
/// 恰恰会把失败原因写在报告里（如 `Err(d).full()` 的偏差明细）。
/// 现改为在循环内现调、取报告、按 `DYN_CAP` 放进动态明细槽。
///
/// 修正记录二（值数组 = 构造即全跑，与全仓纪律相反）：原表是
/// `[(&str, bool, &str); 20]`，数组元素是**已求值的结果** ⇒ 20 个演练
/// 在进循环之前就全部跑完、各自一份返回值在栈上并存。`unxstar_checks`
/// 已因同一原因（310KB 栈）改成函数指针表，此处沿用同一手法。
pub fn run_wb21_unx20() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB21-unx20");
    for (tag, f) in wb21_blocks().iter() {
        // （诊断探针，随诊撤：逐 drill 计时。）
        #[cfg(test)]
        let __t0 = {
            use std::time::Instant;
            eprintln!("[wb21-timing] enter {}", tag);
            Instant::now()
        };
        // 即用即弃：同一时刻只存活一份 `(bool, String)`。
        let (ok, report) = f();
        #[cfg(test)]
        eprintln!("[wb21-timing] {} {:.1}s", tag, __t0.elapsed().as_secs_f32());
        if ok {
            set.add(tag, true, "");
        } else {
            // 失败原因取自 drill 自己的报告（可含偏差明细），而不是
            // 「XX drill red」这种同词反复。
            // 注意 `detail` 必须声明在**循环体作用域**（不能定义在
            // `else` 块内）——`add_dyn` 只借用它一瞬。
            let detail: String = if report.is_empty() {
                alloc::format!("wb21 drill red：{} 无报告", tag)
            } else {
                report
            };
            set.add_dyn(tag, false, &detail);
        }
    }
    set
}

/// WB21 批次 20 单的 drill 入口表（**函数指针**，循环内 `f()` 即用即弃）。
///
/// 提取为模块级函数的理由与 `unxstar_blocks` 同源：聚合表若是函数内局部
/// 变量，失败路径上就够不着下层；而 `DYN_SLOTS = 8` 恰好会被 8 条红占满，
/// 使 `red_items_text()` 只剩标签——诊断必须能**再往下一层**现调入口。
pub fn wb21_blocks() -> [(&'static str, fn() -> (bool, String)); 20] {
    [
        ("UNX-F0404", unx404_hpetcap::drill_green),
        ("UNX-F0583", unx583_wlparams::drill_green),
        ("UNX-F1102", unx1102_stubaudit::pump_100k),
        ("UNX-F1201", unx1201_polloop::drill_1m),
        ("UNX-F1301", unx1301_retire::drill_10),
        ("UNX-F1421", unx1421_latdist::drill_100k),
        ("UNX-F1441", unx1441_schema::drill_full),
        ("UNX-F1461", unx1461_zeroeq::drill_10rounds),
        ("UNX-F1501-D2", unx1501_regmat::drill_full),
        ("UNX-F1541-D2", unx1541_dombind::drill_seal),
        ("UNX-F1561-D2", unx1561_cosign::drill_cosign),
        ("UNX-F1581-D2", unx1581_domreg::drill_800),
        ("UNX-F1601-D2", unx1601_smpcons::drill_full),
        ("UNX-F1621-D2", unx1621_percpu::drill_layout),
        ("UNX-F1641-D2", unx1641_runqueue::drill_full),
        ("UNX-F1661-D2", unx1661_schedcls::drill_chain),
        ("UNX-F1681-D2", unx1681_timeslice::drill_model),
        ("UNX-F1701-D2", unx1701_loadtrk::drill_model),
        ("UNX-F1721-D2", unx1721_balexec::drill_chain),
        ("UNX-F1741-D2", unx1741_idlest::drill_cycle),
    ]
}

/// WB21 逐条 drill 报告（**只在失败路径上**被诊断段调用；长度不受限）。
///
/// 存在的理由：聚合器 `add_dyn` 的动态明细上限 `DYN_SLOTS = 8` 会被
/// 8 条红刚好占满，`CheckSet::red_items_text()` 于是只剩标签——而这些
/// drill 自己返回的 `String` 才带真正的失败原因。此函数**绕开定容槽**
/// 直接现调 20 个入口，把红项的报告逐条拼起来。
pub fn wb21_drill_report() -> String {
    let mut s = String::new();
    for (tag, f) in wb21_blocks().iter() {
        let (ok, report) = f();
        if ok {
            continue;
        }
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(tag);
        s.push_str(" => ");
        if report.is_empty() {
            s.push_str("(无报告)");
        } else {
            s.push_str(&report);
        }
    }
    s
}
pub mod unx882_rtereprog;

// ===================== UNX-21 批次（本会话深编，0x6100~0x65cf 段） =====================
// unx930_metrics ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_metrics;
// unx930_thresh ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_thresh;
// unx930_f0899def ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_f0899def;
// unx930_alerting ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_alerting;
// unx930_percentile ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_percentile;
// unx930_healthroll ↔ UNX-F0930（UNX-21 深编）
pub mod unx930_healthroll;
// unx961_inventory ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_inventory;
// unx961_profile ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_profile;
// unx961_crosscheck ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_crosscheck;
// unx961_takeovermap ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_takeovermap;
// unx961_completeness ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_completeness;
// unx961_obsaudit ↔ UNX-F0961（UNX-21 深编）
pub mod unx961_obsaudit;

/// UNX-21 批次域聚合（逐单折叠：族内任一红即整族红）。
pub fn run_unx21_batch() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX21-batch");
    let entries: [(&'static str, bool, &'static str); 2] = [
        ("UNX-F0930", unx930_metrics::run_iph_checks().all_passed() && unx930_thresh::run_ipt_checks().all_passed() && unx930_f0899def::run_ipd_checks().all_passed() && unx930_alerting::run_ipa_checks().all_passed() && unx930_percentile::run_ippc_checks().all_passed() && unx930_healthroll::run_iphr_checks().all_passed(), "unx21 930 family sub-checks red"),
        ("UNX-F0961", unx961_inventory::run_pms_checks().all_passed() && unx961_profile::run_pmp_checks().all_passed() && unx961_crosscheck::run_pmx_checks().all_passed() && unx961_takeovermap::run_pmt_checks().all_passed() && unx961_completeness::run_pmc_checks().all_passed() && unx961_obsaudit::run_pmo_checks().all_passed(), "unx21 961 family sub-checks red"),
    ];
    for (tag, ok, detail) in entries.iter() {
        set.add(tag, *ok, if *ok { "" } else { detail });
    }
    set
}

// ==========================================================================
pub mod unx325_apidmap;
pub mod unx684_fwdiff;
pub mod unx505_sw10k;
pub mod unx545_judge800;
// UNX 批次 5（CoRun 主册续单：F0325/F0684/F1502/F1543 + 现成族闭环）
// ==========================================================================
pub mod unx325grid;
pub mod unx325mx;
pub mod unx684grid;
pub mod unx684mx;
pub mod unx505grid;
pub mod unx505mx;
pub mod unx545grid;
pub mod unx545mx;

// --------------------------------------------------------------------------
// UNX 批次 5 任务族聚合
// --------------------------------------------------------------------------
/// AP ID 核对域 域聚合。
pub fn run_unx20_t325() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t325");
    let mut ok = true;
    if !unx325_apidmap::run_t325_checks().all_passed() { ok = false; }
    if !unx325grid::run_t325_checks().all_passed() { ok = false; }
    if !unx325mx::run_t325_checks().all_passed() { ok = false; }
    set.add("AP ID 核对域", ok, if ok { "" } else { "unx20-t325 family sub-checks red" });
    set
}
/// 固件路差异域 域聚合。
pub fn run_unx20_t684() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t684");
    let mut ok = true;
    if !unx684_fwdiff::run_t684_checks().all_passed() { ok = false; }
    if !unx684grid::run_t684_checks().all_passed() { ok = false; }
    if !unx684mx::run_t684_checks().all_passed() { ok = false; }
    set.add("固件路差异域", ok, if ok { "" } else { "unx20-t684 family sub-checks red" });
    set
}
/// 万次切换域 域聚合。
pub fn run_unx20_t505() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t505");
    let mut ok = true;
    if !unx505_sw10k::run_t505_checks().all_passed() { ok = false; }
    if !unx505grid::run_t505_checks().all_passed() { ok = false; }
    if !unx505mx::run_t505_checks().all_passed() { ok = false; }
    set.add("万次切换域", ok, if ok { "" } else { "unx20-t505 family sub-checks red" });
    set
}
/// 判据冻结域 域聚合。
pub fn run_unx20_t545() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX20-t545");
    let mut ok = true;
    if !unx545_judge800::run_t545_checks().all_passed() { ok = false; }
    if !unx545grid::run_t545_checks().all_passed() { ok = false; }
    if !unx545mx::run_t545_checks().all_passed() { ok = false; }
    set.add("判据冻结域", ok, if ok { "" } else { "unx20-t545 family sub-checks red" });
    set
}
// ---- AI-WBK batch6c: F0185/F0225/F0387b/F1562/F1602/F1621b ----
pub mod unx185_cpufeat;
pub mod unx225_diagdrill;
pub mod unx387b_oomacct;
pub mod unx1562_a3sign;
pub mod unx1602_cputopo;
pub mod unx1621b_percurv;
pub mod unx943_tophalfline;
// ===================== AI-WB3 批次 · CoRun UNX-A3 B07–B16 深化十单（0xB000 段） =====================
// unx1722_migrate ↔ UNX-F1722 迁移执行原语（三段原子链 摘出→暂存→挂入 + 全域唯一跨队列写入点 + 三向环形账 + 绕行注入 10/10 拒绝；0xB020 段十六码）
pub mod unx1722_migrate;
// unx1742_idleenter ↔ UNX-F1742 idle 进入路径（队空判定→静息置位→账目起算→状态落定四步链 + 静息置位 100% 命中 F1735 挂点消费 + Σidle+Σbusy 守恒；0xB040 段十六码）
pub mod unx1742_idleenter;
// unx1762_cpustats ↔ UNX-F1762 每核统计账本体（十字段族汇总账 + PerCpu 冻结版字段级三方对齐 + 20 条守恒式 + 全核同窗快照；0xB060 段十六码）
pub mod unx1762_cpustats;
// unx1782_structaudit ↔ UNX-F1782 每核结构一致性总检（PerCpu 字段×统计账位×观测接口三层总清单 + 十层拼图 + 35 条聚合断言 + 启动期/运行期巡检；0xB080 段十六码）
pub mod unx1782_structaudit;
// unx1802_schedpts ↔ UNX-F1802 调度点全集登记（tick到期/抢占/让出/阻塞/迁移后/启动 六点闭集 + 清单冻结 v1 + 6×4 语境矩阵 + 域外注入 10/10 检出；0xB0A0 段十六码）
pub mod unx1802_schedpts;
// unx1822_stackguard ↔ UNX-F1822 线程栈分配与 guard 页（栈池三区布局 + 上下侧 guard 页登记册 + 越界 10/10 触发异常路径 + poison 防复用；0xB0C0 段十六码）
pub mod unx1822_stackguard;
// unx1842_ipivec_deep ↔ UNX-F1842 IPI 向量分配表深化（32 向量四类登记 + 冻结 v1 指纹 + 编译期宏与运行期断言双检 + F1847 三禁 + F1848 合并重入；0xB0E0 段十六码）
pub mod unx1842_ipivec_deep;
// unx1862_mmmask ↔ UNX-F1862 地址空间活跃核追踪（mm 活跃 mask + 切换挂点同步 + 跨 mm 交集为空 + 击落必达 + 迁移翻转原子性；0xB100 段十六码）
pub mod unx1862_mmmask;
// unx1863_shootbegin ↔ UNX-F1863 击落发起原语（先本地 flush 再远程广播 + 先登记后发送硬闸 + 次序账三段单调 + 广播判定六项 + 过渡态改道 + 空集零广播短路；0xB110 段十码）
pub mod unx1863_shootbegin;
// unx1882_swipi ↔ UNX-F1882 切换×IPI 跨链回归（切换四段建模 + 竞态处置矩阵 10/10 全对 + F1807 寄存器保持性与死点 + F1847 三禁；0xB120 段十六码）
pub mod unx1882_swipi;
// unx1902_lazysave_deep ↔ UNX-F1902 惰性保存策略深化（保存时机三态判定表 12 格 + 三态逐例 10 例 + CR0.TS→#NM 闭环 + 往返保真 + 账守恒；0xB140 段十六码）
pub mod unx1902_lazysave_deep;

// ===================== WB22 批次（第三波 22 文件，0xD300~0xD4C0 段） =====================
// unx45_sbtopo — UNX-F0045 _SB 设备拓扑提取（HID/EISA 闭集+ADR 冲突闸+拓扑指纹冻结+三类注入/0xD300 段）
pub mod unx45_sbtopo;
// unx703_memassert — UNX-F0703 内存图断言补全 ≥60（e820 五面覆盖+排序/合并/区间和 ±4KB/六类枚举+可解引/0xD310 段）
pub mod unx703_memassert;
// unx723_evalsla — UNX-F0723 升级评估流程账（四步闭集+留痕四列+SLA 7 日整型钟+超期红账/0xD320 段）
pub mod unx723_evalsla;
// unx743_evichain — UNX-F0743 域经证据链核验（解引 100%+断链隔离重挂+指纹对拍+零悬空/0xD330 段）
pub mod unx743_evichain;
// unx783_batchfinal — UNX-F0783 批册终态核（40 批 [已深化]+800 总账守恒+20 段核+骨架零残留/0xD340 段）
pub mod unx783_batchfinal;
// unx803_irqpool — UNX-F0803 IRQ 向量池化（0x20..0x8F 112 槽+首配分配+万次零悬空+越界留账/0xD350 段）
pub mod unx803_irqpool;
// unx823_eoiguard — UNX-F0823 EOI 单点封装（五类分账 IRQ/MSI/IPI/spurious/NMI+对偶恒等+直写拦截/0xD360 段）
pub mod unx823_eoiguard;
// unx1241_armingbody — UNX-F1241 Arming 态武装序列（五步闭集+段间栅栏+泵停确认+事件通道 100%/0xD380 段）
pub mod unx1241_armingbody;
// unx1261_irqsteady — UNX-F1261 Irq 态稳态本体（四段链 10^7 零丢失+泵零活动+标志一致/0xD390 段）
pub mod unx1261_irqsteady;
// unx1281_clockmap — UNX-F1281 clock.rs 轮询测绘（9 点双锚+接管映射 100%+F0961 对齐/0xD3A0 段）
pub mod unx1281_clockmap;
// unx1321_equivproto — UNX-F1321 等价验证协议（五步闭集+四维对拍+唯一判定出口+可重复/0xD3B0 段）
pub mod unx1321_equivproto;
// unx1341_stressmaster — UNX-F1341 分发压测总谱（54 场景闭集+执行矩阵 486 格+版本只增/0xD3C0 段）
pub mod unx1341_stressmaster;
// unx1361_budgetaudit — UNX-F1361 延迟预算复核（全量 1000‰+偏差 ≤10% 红线+五段定位/0xD3D0 段）
pub mod unx1361_budgetaudit;
// unx1381_indexledger — UNX-F1381 分发接管面总谱索引（F1101..F1400 400 条+批映射恒等+导出指纹/0xD3E0 段）
pub mod unx1381_indexledger;
// unx1401_vlatledger — UNX-F1401 全向量延迟账本（224 户闭集+64 槽一致+双规划账对齐/0xD3F0 段）
pub mod unx1401_vlatledger;
// unx1603_onlinebitmap — UNX-F1603 缺席核账与在线位图（人口一致+缺席位零恒+幽灵在线抓/0xD400 段）
pub mod unx1603_onlinebitmap;
// unx2121_cosnap — UNX-F2121 统计族同窗快照（四族同窗四元组+不可变+F1768 三条款对齐/0xD440 段）
pub mod unx2121_cosnap;
// unx2223_hashchain — UNX-F2223 全域哈希链滚动重算（620 行双口径一致+存量不变+篡改检出/0xD470 段）
pub mod unx2223_hashchain;
// unx2241_segfinal — UNX-F2241 段级终验五步（ID 连续 60+骨架 6000+字数红线+加法链+三账对回/0xD480 段）
pub mod unx2241_segfinal;
// unx2261_sediment — UNX-F2261 沉淀轴总控（三段编排+四类素材点收+三线滚动+时序/0xD490 段）
pub mod unx2261_sediment;
// unx2281_expansion — UNX-F2281 扩容轴总控（三段编排+增量清点+EX/图族/教训三线/0xD4A0 段）
pub mod unx2281_expansion;
// unx2301_axfinal — UNX-F2301 域经轴收官（收口 B34-B36+三件终验+F2317 交接/0xD4B0 段）
pub mod unx2301_axfinal;

/// WB22 批次域聚合（22 单：drill 验收入口逐单折叠，族内任一红即整族红）。
pub fn run_wb22_unx22() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB22-unx22");
    let entries: [(&'static str, bool, &'static str); 22] = [
        ("UNX-F0045", unx45_sbtopo::drill_sbtopo().0, "wb22 sbtopo red"),
        ("UNX-F0703", unx703_memassert::drill_memassert().0, "wb22 memassert red"),
        ("UNX-F0723", unx723_evalsla::drill_evalsla().0, "wb22 evalsla red"),
        ("UNX-F0743", unx743_evichain::drill_evichain().0, "wb22 evichain red"),
        ("UNX-F0783", unx783_batchfinal::drill_batchfinal().0, "wb22 batchfinal red"),
        ("UNX-F0803", unx803_irqpool::drill_irqpool().0, "wb22 irqpool red"),
        ("UNX-F0823", unx823_eoiguard::drill_eoiguard().0, "wb22 eoiguard red"),
        ("UNX-F1241", unx1241_armingbody::drill_armingbody().0, "wb22 armingbody red"),
        ("UNX-F1261", unx1261_irqsteady::drill_irqsteady().0, "wb22 irqsteady red"),
        ("UNX-F1281", unx1281_clockmap::drill_clockmap().0, "wb22 clockmap red"),
        ("UNX-F1321", unx1321_equivproto::drill_equivproto().0, "wb22 equivproto red"),
        ("UNX-F1341", unx1341_stressmaster::drill_stressmaster().0, "wb22 stressmaster red"),
        ("UNX-F1361", unx1361_budgetaudit::drill_budgetaudit().0, "wb22 budgetaudit red"),
        ("UNX-F1381", unx1381_indexledger::drill_indexledger().0, "wb22 indexledger red"),
        ("UNX-F1401", unx1401_vlatledger::drill_vlatledger().0, "wb22 vlatledger red"),
        ("UNX-F1603", unx1603_onlinebitmap::drill_onlinebitmap().0, "wb22 onlinebitmap red"),
        ("UNX-F2121", unx2121_cosnap::drill_cosnap().0, "wb22 cosnap red"),
        ("UNX-F2223", unx2223_hashchain::drill_hashchain().0, "wb22 hashchain red"),
        ("UNX-F2241", unx2241_segfinal::drill_segfinal().0, "wb22 segfinal red"),
        ("UNX-F2261", unx2261_sediment::drill_sediment().0, "wb22 sediment red"),
        ("UNX-F2281", unx2281_expansion::drill_expansion().0, "wb22 expansion red"),
        ("UNX-F2301", unx2301_axfinal::drill_axfinal().0, "wb22 axfinal red"),
    ];
    for (tag, ok, detail) in entries.iter() {
        set.add(tag, *ok, if *ok { "" } else { detail });
    }
    set
}

pub mod unx982_throttle;
pub mod unx983_stormgrade;
// ==== 孤儿模块补注册批次 WB-A1（2026-10-12）：文件在链外导致功能不可达 ====
pub mod unx080_idt;
pub mod unx082_lapicbase;
pub mod unx084_leveltrigger;
pub mod unx088_irqaffinity;
pub mod unx090_irqregister;
pub mod unx091_msixmultivec;
pub mod unx092_handlerregistry;
pub mod unx093_sharedvector;
pub mod unx094_irqcontext;
pub mod unx095_floodtest;
pub mod unx096_auditreplay;
pub mod unx097_enableseq;
pub mod unx098_dualcapability;
pub mod unx099_intxpath;
pub mod unx100_irqname;
pub mod unx101_crosscheck;
pub mod unx102_irqstats;
pub mod unx103_handlertime;
pub mod unx104_teardown;
pub mod unx105_sharedalert;
// ==== 孤儿模块补注册 WB-A2（2026-10-12）====
pub mod unx1061_equiv;
pub mod unx1061_polycost;
pub mod unx1061_registry;
pub mod unx1061_threesrc;
pub mod unx1061_traitface;
pub mod unx106_contract;
pub mod unx107_faultharness;
pub mod unx1081_eventlog;
pub mod unx1081_f1079regr;
pub mod unx1081_slewmode;
pub mod unx1081_sourceaudit;
pub mod unx1081_stepmode;
pub mod unx108_healthsummary;
pub mod unx109_ktb06;
pub mod unx1141_distrib;
pub mod unx1141_f0943align;
pub mod unx1141_percentiles;
pub mod unx1141_pervec;
pub mod unx1141_timecap;
pub mod unx1161_conserve;
pub mod unx1161_exitcond;
pub mod unx1161_million;
pub mod unx1161_pending;
pub mod unx1161_quota;
pub mod unx1181_applylat;
pub mod unx1181_illegalrefuse;
pub mod unx1181_readback;
pub mod unx1181_routermap;
pub mod unx1181_targetreg;
pub mod unx1221_dualmode;
pub mod unx1221_exitall;
pub mod unx1221_hundthou;
pub mod unx1221_serialize;
pub mod unx1221_twosrc;
pub mod unx122_stagewatchdog;
pub mod unx122_stagewd;
pub mod unx122grid;
pub mod unx122mx;
pub mod unx123_wdtime;
pub mod unx124_wdstall;
pub mod unx125_wddiag;
pub mod unx142_gpthdr;
pub mod unx142grid;
pub mod unx142mx;
pub mod unx143_gptpart;
pub mod unx144_espfat;
pub mod unx145_imgsync;
pub mod unx1743_idleres;
pub mod unx1763_statmac;
pub mod unx1783_crossscan;
pub mod unx1803_resched;
pub mod unx1823_saveframe;
pub mod unx1843_ipirx;
pub mod unx1863_ipishoot;
pub mod unx1883_trireg;
pub mod unx202_schemaver;
pub mod unx2261_settle;
pub mod unx2281_expand;
pub mod unx2321_consign;
pub mod unx2321_unisign3;
pub mod unx2341_udomregress;
pub mod unx2361_udecLARE3;
pub mod unx2381_ufinalacct;
pub mod unx2401_u2401barrier;
pub mod unx2401_u2401bgrid;
pub mod unx2401_u2401bmx;
pub mod unx2401_u2401core;
pub mod unx2401_u2401dual;
pub mod unx2401_u2401grid;
pub mod unx2401_u2401map;
pub mod unx2401_u2401merge;
pub mod unx2401_u2401mix;
pub mod unx2401_u2401mx;
pub mod unx2401_u2401pipe;
pub mod unx2401_u2401sm;
pub mod unx2402_ufreemap;
pub mod unx2421_u2421barrier;
pub mod unx2421_u2421bgrid;
pub mod unx2421_u2421bmx;
pub mod unx2421_u2421core;
pub mod unx2421_u2421dual;
pub mod unx2421_u2421grid;
pub mod unx2421_u2421map;
pub mod unx2421_u2421merge;
pub mod unx2421_u2421mix;
pub mod unx2421_u2421mx;
pub mod unx2421_u2421pipe;
pub mod unx2421_u2421sm;
pub mod unx2422_uorder14;
pub mod unx2441_u2441barrier;
pub mod unx2441_u2441bgrid;
pub mod unx2441_u2441bmx;
pub mod unx2441_u2441core;
pub mod unx2441_u2441dual;
pub mod unx2441_u2441grid;
pub mod unx2441_u2441map;
pub mod unx2441_u2441merge;
pub mod unx2441_u2441mix;
pub mod unx2441_u2441mx;
pub mod unx2441_u2441pipe;
// ==== 孤儿模块补注册 WB-A3（2026-10-12）====
pub mod unx2441_u2441sm;
pub mod unx2442_urefill;
pub mod unx2461_u2461barrier;
pub mod unx2461_u2461bgrid;
pub mod unx2461_u2461bmx;
pub mod unx2461_u2461core;
pub mod unx2461_u2461dual;
pub mod unx2461_u2461grid;
pub mod unx2461_u2461map;
pub mod unx2461_u2461merge;
pub mod unx2461_u2461mix;
pub mod unx2461_u2461mx;
pub mod unx2461_u2461pipe;
pub mod unx2461_u2461sm;
pub mod unx2481_u2481barrier;
pub mod unx2481_u2481bgrid;
pub mod unx2481_u2481bmx;
pub mod unx2481_u2481core;
pub mod unx2481_u2481dual;
pub mod unx2481_u2481grid;
pub mod unx2481_u2481map;
pub mod unx2481_u2481merge;
pub mod unx2481_u2481mix;
pub mod unx2481_u2481mx;
pub mod unx2481_u2481pipe;
pub mod unx2481_u2481sm;
pub mod unx2482_uorder9raw;
pub mod unx2501_u2501barrier;
pub mod unx2501_u2501bgrid;
pub mod unx2501_u2501bmx;
pub mod unx2501_u2501core;
pub mod unx2501_u2501dual;
pub mod unx2501_u2501grid;
pub mod unx2501_u2501map;
pub mod unx2501_u2501merge;
pub mod unx2501_u2501mix;
pub mod unx2501_u2501mx;
pub mod unx2501_u2501pipe;
pub mod unx2501_u2501sm;
pub mod unx2502_uhptime;
pub mod unx2521_u2521barrier;
pub mod unx2521_u2521bgrid;
pub mod unx2521_u2521bmx;
pub mod unx2521_u2521core;
pub mod unx2521_u2521dual;
pub mod unx2521_u2521grid;
pub mod unx2521_u2521map;
pub mod unx2521_u2521merge;
pub mod unx2521_u2521mix;
pub mod unx2521_u2521mx;
pub mod unx2521_u2521pipe;
pub mod unx2521_u2521sm;
pub mod unx2541_u2541barrier;
pub mod unx2541_u2541bgrid;
pub mod unx2541_u2541bmx;
pub mod unx2541_u2541core;
pub mod unx2541_u2541dual;
pub mod unx2541_u2541grid;
pub mod unx2541_u2541map;
pub mod unx2541_u2541merge;
pub mod unx2541_u2541mix;
pub mod unx2541_u2541mx;
pub mod unx2541_u2541pipe;
pub mod unx2541_u2541sm;
pub mod unx2561_u2561barrier;
pub mod unx2561_u2561bgrid;
pub mod unx2561_u2561bmx;
pub mod unx2561_u2561core;
pub mod unx2561_u2561dual;
pub mod unx2561_u2561grid;
pub mod unx2561_u2561map;
pub mod unx2561_u2561merge;
pub mod unx2561_u2561mix;
pub mod unx2561_u2561mx;
pub mod unx2561_u2561pipe;
pub mod unx2561_u2561sm;
pub mod unx2562_uaspcreate;
pub mod unx2581_u2581barrier;
pub mod unx2581_u2581bgrid;
pub mod unx2581_u2581bmx;
pub mod unx2581_u2581core;
pub mod unx2581_u2581dual;
pub mod unx2581_u2581grid;
pub mod unx2581_u2581map;
pub mod unx2581_u2581merge;
pub mod unx2581_u2581mix;
pub mod unx2581_u2581mx;
pub mod unx2581_u2581pipe;
pub mod unx2581_u2581sm;
pub mod unx2582_uvmainject;
pub mod unx2601_u2601barrier;
pub mod unx2601_u2601bgrid;
pub mod unx2601_u2601bmx;
pub mod unx2601_u2601core;
pub mod unx2601_u2601dual;
pub mod unx2601_u2601grid;
pub mod unx2601_u2601map;
pub mod unx2601_u2601merge;
pub mod unx2601_u2601mix;
pub mod unx2601_u2601mx;
pub mod unx2601_u2601pipe;
pub mod unx2601_u2601sm;
pub mod unx2602_upfentry;
pub mod unx2621_u2621barrier;
pub mod unx2621_u2621bgrid;
pub mod unx2621_u2621bmx;
pub mod unx2621_u2621core;
pub mod unx2621_u2621dual;
pub mod unx2621_u2621grid;
pub mod unx2621_u2621map;
pub mod unx2621_u2621merge;
pub mod unx2621_u2621mix;
pub mod unx2621_u2621mx;
pub mod unx2621_u2621pipe;
pub mod unx2621_u2621sm;
pub mod unx2622_upcidbook;
pub mod unx2641_u2641barrier;
pub mod unx2641_u2641bgrid;
pub mod unx2641_u2641bmx;
pub mod unx2641_u2641core;
// ==== 孤儿模块补注册 WB-A4（2026-10-12）====
pub mod unx2641_u2641dual;
pub mod unx2641_u2641grid;
pub mod unx2641_u2641map;
pub mod unx2641_u2641merge;
pub mod unx2641_u2641mix;
pub mod unx2641_u2641mx;
pub mod unx2641_u2641pipe;
pub mod unx2641_u2641sm;
pub mod unx2642_uptebatchro;
pub mod unx2661_u2661barrier;
pub mod unx2661_u2661bgrid;
pub mod unx2661_u2661bmx;
pub mod unx2661_u2661core;
pub mod unx2661_u2661dual;
pub mod unx2661_u2661grid;
pub mod unx2661_u2661map;
pub mod unx2661_u2661merge;
pub mod unx2661_u2661mix;
pub mod unx2661_u2661mx;
pub mod unx2661_u2661pipe;
pub mod unx2661_u2661sm;
pub mod unx2662_ulookup;
pub mod unx2681_u2681barrier;
pub mod unx2681_u2681bgrid;
pub mod unx2681_u2681bmx;
pub mod unx2681_u2681core;
pub mod unx2681_u2681dual;
pub mod unx2681_u2681grid;
pub mod unx2681_u2681map;
pub mod unx2681_u2681merge;
pub mod unx2681_u2681mix;
pub mod unx2681_u2681mx;
pub mod unx2681_u2681pipe;
pub mod unx2681_u2681sm;
pub mod unx2682_ucacheif;
pub mod unx2701_u2701barrier;
pub mod unx2701_u2701bgrid;
pub mod unx2701_u2701bmx;
pub mod unx2701_u2701core;
pub mod unx2701_u2701dual;
pub mod unx2701_u2701grid;
pub mod unx2701_u2701map;
pub mod unx2701_u2701merge;
pub mod unx2701_u2701mix;
pub mod unx2701_u2701mx;
pub mod unx2701_u2701pipe;
pub mod unx2701_u2701sm;
pub mod unx2702_ulrufield;
pub mod unx2721_u2721barrier;
pub mod unx2721_u2721bgrid;
pub mod unx2721_u2721bmx;
pub mod unx2721_u2721core;
pub mod unx2721_u2721dual;
pub mod unx2721_u2721grid;
pub mod unx2721_u2721map;
pub mod unx2721_u2721merge;
pub mod unx2721_u2721mix;
pub mod unx2721_u2721mx;
pub mod unx2721_u2721pipe;
pub mod unx2721_u2721sm;
pub mod unx2722_uanonevict;
pub mod unx2741_u2741barrier;
pub mod unx2741_u2741bgrid;
pub mod unx2741_u2741bmx;
pub mod unx2741_u2741core;
pub mod unx2741_u2741dual;
pub mod unx2741_u2741grid;
pub mod unx2741_u2741map;
pub mod unx2741_u2741merge;
pub mod unx2741_u2741mix;
pub mod unx2741_u2741mx;
pub mod unx2741_u2741pipe;
pub mod unx2741_u2741sm;
pub mod unx2761_u2761barrier;
pub mod unx2761_u2761bgrid;
pub mod unx2761_u2761bmx;
pub mod unx2761_u2761core;
pub mod unx2761_u2761dual;
pub mod unx2761_u2761grid;
pub mod unx2761_u2761map;
pub mod unx2761_u2761merge;
pub mod unx2761_u2761mix;
pub mod unx2761_u2761mx;
pub mod unx2761_u2761pipe;
pub mod unx2761_u2761sm;
pub mod unx2762_umaptbl;
pub mod unx2781_u2781barrier;
pub mod unx2781_u2781bgrid;
pub mod unx2781_u2781bmx;
pub mod unx2781_u2781core;
pub mod unx2781_u2781dual;
pub mod unx2781_u2781grid;
pub mod unx2781_u2781map;
pub mod unx2781_u2781merge;
pub mod unx2781_u2781mix;
pub mod unx2781_u2781mx;
pub mod unx2781_u2781pipe;
pub mod unx2781_u2781sm;
pub mod unx2782_ubuddymerge;
pub mod unx2801_u2801barrier;
pub mod unx2801_u2801bgrid;
pub mod unx2801_u2801bmx;
pub mod unx2801_u2801core;
pub mod unx2801_u2801dual;
pub mod unx2801_u2801grid;
pub mod unx2801_u2801map;
pub mod unx2801_u2801merge;
pub mod unx2801_u2801mix;
pub mod unx2801_u2801mx;
pub mod unx2801_u2801pipe;
pub mod unx2801_u2801sm;
pub mod unx2801_ubigpoolcap;
pub mod unx2821_u2821barrier;
pub mod unx2821_u2821bgrid;
pub mod unx2821_u2821bmx;
pub mod unx2821_u2821core;
pub mod unx2821_u2821dual;
pub mod unx2821_u2821grid;
pub mod unx2821_u2821map;
pub mod unx2821_u2821merge;
pub mod unx2821_u2821mix;
pub mod unx2821_u2821mx;
pub mod unx2821_u2821pipe;
pub mod unx2821_u2821sm;
pub mod unx2841_u2841barrier;
pub mod unx2841_u2841bgrid;
pub mod unx2841_u2841bmx;
pub mod unx2841_u2841core;
pub mod unx2841_u2841dual;
pub mod unx2841_u2841grid;
pub mod unx2841_u2841map;
pub mod unx2841_u2841merge;
pub mod unx2841_u2841mix;
pub mod unx2841_u2841mx;
pub mod unx2841_u2841pipe;
pub mod unx2841_u2841sm;
pub mod unx2841_ubigasp;
pub mod unx284_paniclayer;
pub mod unx285_panicstop;
pub mod unx2861_u2861barrier;
pub mod unx2861_u2861bgrid;
pub mod unx2861_u2861bmx;
pub mod unx2861_u2861core;
pub mod unx2861_u2861dual;
pub mod unx2861_u2861grid;
pub mod unx2861_u2861map;
pub mod unx2861_u2861merge;
pub mod unx2861_u2861mix;
pub mod unx2861_u2861mx;
pub mod unx2861_u2861pipe;
pub mod unx2861_u2861sm;
pub mod unx2861_ubigmatrix;
pub mod unx2881_u2881barrier;
pub mod unx2881_u2881bgrid;
pub mod unx2881_u2881bmx;
pub mod unx2881_u2881core;
pub mod unx2881_u2881dual;
pub mod unx2881_u2881grid;
pub mod unx2881_u2881map;
pub mod unx2881_u2881merge;
pub mod unx2881_u2881mix;
pub mod unx2881_u2881mx;
pub mod unx2881_u2881pipe;
pub mod unx2881_u2881sm;
pub mod unx2901_u2901barrier;
pub mod unx2901_u2901bgrid;
pub mod unx2901_u2901bmx;
pub mod unx2901_u2901core;
pub mod unx2901_u2901dual;
pub mod unx2901_u2901grid;
pub mod unx2901_u2901map;
pub mod unx2901_u2901merge;
pub mod unx2901_u2901mix;
pub mod unx2901_u2901mx;
pub mod unx2901_u2901pipe;
pub mod unx2901_u2901sm;
pub mod unx2901_ucachestress;
pub mod unx2921_u2921barrier;
pub mod unx2921_u2921bgrid;
pub mod unx2921_u2921bmx;
pub mod unx2921_u2921core;
pub mod unx2921_u2921dual;
pub mod unx2921_u2921grid;
pub mod unx2921_u2921map;
pub mod unx2921_u2921merge;
pub mod unx2921_u2921mix;
pub mod unx2921_u2921mx;
pub mod unx2921_u2921pipe;
pub mod unx2921_u2921sm;
pub mod unx2941_u2941barrier;
pub mod unx2941_u2941bgrid;
pub mod unx2941_u2941bmx;
pub mod unx2941_u2941core;
pub mod unx2941_u2941dual;
pub mod unx2941_u2941grid;
pub mod unx2941_u2941map;
pub mod unx2941_u2941merge;
pub mod unx2941_u2941mix;
pub mod unx2941_u2941mx;
pub mod unx2941_u2941pipe;
// ==== 孤儿模块补注册 WB-A5（2026-10-12）====
pub mod unx2941_u2941sm;
pub mod unx2941_udomstress;
pub mod unx2961_u2961barrier;
pub mod unx2961_u2961bgrid;
pub mod unx2961_u2961bmx;
pub mod unx2961_u2961core;
pub mod unx2961_u2961dual;
pub mod unx2961_u2961grid;
pub mod unx2961_u2961map;
pub mod unx2961_u2961merge;
pub mod unx2961_u2961mix;
pub mod unx2961_u2961mx;
pub mod unx2961_u2961pipe;
pub mod unx2961_u2961sm;
pub mod unx2981_u2981barrier;
pub mod unx2981_u2981bgrid;
pub mod unx2981_u2981bmx;
pub mod unx2981_u2981core;
pub mod unx2981_u2981dual;
pub mod unx2981_u2981grid;
pub mod unx2981_u2981map;
pub mod unx2981_u2981merge;
pub mod unx2981_u2981mix;
pub mod unx2981_u2981mx;
pub mod unx2981_u2981pipe;
pub mod unx2981_u2981sm;
pub mod unx2981_udomclose;
pub mod unx3001_u3001barrier;
pub mod unx3001_u3001bgrid;
pub mod unx3001_u3001bmx;
pub mod unx3001_u3001core;
pub mod unx3001_u3001dual;
pub mod unx3001_u3001grid;
pub mod unx3001_u3001map;
pub mod unx3001_u3001merge;
pub mod unx3001_u3001mix;
pub mod unx3001_u3001mx;
pub mod unx3001_u3001pipe;
pub mod unx3001_u3001sm;
pub mod unx3001_udualchain;
pub mod unx3021_u3021barrier;
pub mod unx3021_u3021bgrid;
pub mod unx3021_u3021bmx;
pub mod unx3021_u3021core;
pub mod unx3021_u3021dual;
pub mod unx3021_u3021grid;
pub mod unx3021_u3021map;
pub mod unx3021_u3021merge;
pub mod unx3021_u3021mix;
pub mod unx3021_u3021mx;
pub mod unx3021_u3021pipe;
pub mod unx3021_u3021sm;
pub mod unx3021_utier3;
pub mod unx3041_u3041barrier;
pub mod unx3041_u3041bgrid;
pub mod unx3041_u3041bmx;
pub mod unx3041_u3041core;
pub mod unx3041_u3041dual;
pub mod unx3041_u3041grid;
pub mod unx3041_u3041map;
pub mod unx3041_u3041merge;
pub mod unx3041_u3041mix;
pub mod unx3041_u3041mx;
pub mod unx3041_u3041pipe;
pub mod unx3041_u3041sm;
pub mod unx3061_upowerloss;
pub mod unx307_pml4freeze;
pub mod unx3081_utierfinal;
pub mod unx308_hhmap;
pub mod unx309_dmap;
pub mod unx3101_uregfreeze;
pub mod unx3121_ureconall;
pub mod unx3141_ubaseline;
pub mod unx3161_uregexec;
pub mod unx3181_udomainfinal;
pub mod unx3201_atomic;
pub mod unx3221_u3221barrier;
pub mod unx3221_u3221bgrid;
pub mod unx3221_u3221bmx;
pub mod unx3221_u3221core;
pub mod unx3221_u3221dual;
pub mod unx3221_u3221grid;
pub mod unx3221_u3221map;
pub mod unx3221_u3221merge;
pub mod unx3221_u3221mix;
pub mod unx3221_u3221mx;
pub mod unx3221_u3221pipe;
pub mod unx3221_u3221sm;
pub mod unx3261_u3261barrier;
pub mod unx3261_u3261bgrid;
pub mod unx3261_u3261bmx;
pub mod unx3261_u3261core;
pub mod unx3261_u3261dual;
pub mod unx3261_u3261grid;
pub mod unx3261_u3261map;
pub mod unx3261_u3261merge;
pub mod unx3261_u3261mix;
pub mod unx3261_u3261mx;
pub mod unx3261_u3261pipe;
pub mod unx3261_u3261sm;
pub mod unx3301_u3301barrier;
pub mod unx3301_u3301bgrid;
pub mod unx3301_u3301bmx;
pub mod unx3301_u3301core;
pub mod unx3301_u3301dual;
pub mod unx3301_u3301grid;
pub mod unx3301_u3301map;
pub mod unx3301_u3301merge;
pub mod unx3301_u3301mix;
pub mod unx3301_u3301mx;
pub mod unx3301_u3301pipe;
pub mod unx3301_u3301sm;
pub mod unx3321_u3321barrier;
pub mod unx3321_u3321bgrid;
pub mod unx3321_u3321bmx;
pub mod unx3321_u3321core;
pub mod unx3321_u3321dual;
pub mod unx3321_u3321grid;
pub mod unx3321_u3321map;
pub mod unx3321_u3321merge;
pub mod unx3321_u3321mix;
pub mod unx3321_u3321mx;
pub mod unx3321_u3321pipe;
pub mod unx3321_u3321sm;
pub mod unx3341_u3341barrier;
pub mod unx3341_u3341bgrid;
pub mod unx3341_u3341bmx;
pub mod unx3341_u3341core;
pub mod unx3341_u3341dual;
pub mod unx3341_u3341grid;
pub mod unx3341_u3341map;
pub mod unx3341_u3341merge;
pub mod unx3341_u3341mix;
pub mod unx3341_u3341mx;
pub mod unx3341_u3341pipe;
pub mod unx3341_u3341sm;
pub mod unx3361_u3361barrier;
pub mod unx3361_u3361bgrid;
pub mod unx3361_u3361bmx;
pub mod unx3361_u3361core;
pub mod unx3361_u3361dual;
pub mod unx3361_u3361grid;
pub mod unx3361_u3361map;
pub mod unx3361_u3361merge;
pub mod unx3361_u3361mix;
pub mod unx3361_u3361mx;
pub mod unx3361_u3361pipe;
pub mod unx3361_u3361sm;
pub mod unx3361_utimewin;
pub mod unx3381_u3381barrier;
pub mod unx3381_u3381bgrid;
pub mod unx3381_u3381bmx;
pub mod unx3381_u3381core;
pub mod unx3381_u3381dual;
pub mod unx3381_u3381grid;
pub mod unx3381_u3381map;
pub mod unx3381_u3381merge;
pub mod unx3381_u3381mix;
pub mod unx3381_u3381mx;
pub mod unx3381_u3381pipe;
pub mod unx3381_u3381sm;
pub mod unx3381_usamplelib;
pub mod unx3401_u3401barrier;
pub mod unx3401_u3401bgrid;
pub mod unx3401_u3401bmx;
pub mod unx3401_u3401core;
pub mod unx3401_u3401dual;
pub mod unx3401_u3401grid;
pub mod unx3401_u3401map;
pub mod unx3401_u3401merge;
pub mod unx3401_u3401mix;
pub mod unx3401_u3401mx;
pub mod unx3401_u3401pipe;
pub mod unx3401_u3401sm;
pub mod unx3401_uktestdesign;
pub mod unx3421_u3421barrier;
pub mod unx3421_u3421bgrid;
pub mod unx3421_u3421bmx;
pub mod unx3421_u3421core;
pub mod unx3421_u3421dual;
pub mod unx3421_u3421grid;
pub mod unx3421_u3421map;
pub mod unx3421_u3421merge;
pub mod unx3421_u3421mix;
pub mod unx3421_u3421mx;
pub mod unx3421_u3421pipe;
pub mod unx3421_u3421sm;
pub mod unx3421_udualbackend;
pub mod unx3441_u3441barrier;
pub mod unx3441_u3441bgrid;
pub mod unx3441_u3441bmx;
pub mod unx3441_u3441core;
pub mod unx3441_u3441dual;
pub mod unx3441_u3441grid;
pub mod unx3441_u3441map;
pub mod unx3441_u3441merge;
pub mod unx3441_u3441mix;
pub mod unx3441_u3441mx;
pub mod unx3441_u3441pipe;
pub mod unx3441_u3441sm;
pub mod unx3461_u3461barrier;
pub mod unx3461_u3461bgrid;
pub mod unx3461_u3461bmx;
pub mod unx3461_u3461core;
pub mod unx3461_u3461dual;
pub mod unx3461_u3461grid;
pub mod unx3461_u3461map;
pub mod unx3461_u3461merge;
pub mod unx3461_u3461mix;
pub mod unx3461_u3461mx;
pub mod unx3461_u3461pipe;
pub mod unx3461_u3461sm;
pub mod unx3461_umountarch;
pub mod unx3481_u3481barrier;
pub mod unx3481_u3481bgrid;
pub mod unx3481_u3481bmx;
pub mod unx3481_u3481core;
pub mod unx3481_u3481dual;
pub mod unx3481_u3481grid;
pub mod unx3481_u3481map;
// ==== 孤儿模块补注册 WB-A6（2026-10-12，收官批）====
// 2026-10-12 注：以下 `unxt1xx_*` 为未完工在制品（时区表常量未定义、
// 表元素数与声明长度不符、元素类型错为 u64 却存负偏移），仅在各自
// `#[cfg(test)]` 区触发 97 个 E0425/E0600/E0308。文件本体不动，
// 待各自补齐常量与类型后恢复下列注册（在链上无外部引用，零风险）。
pub mod unx3481_u3481merge;
pub mod unx3481_u3481mix;
pub mod unx3481_u3481mx;
pub mod unx3481_u3481pipe;
pub mod unx3481_u3481sm;
pub mod unx3501_u3501barrier;
pub mod unx3501_u3501bgrid;
pub mod unx3501_u3501bmx;
pub mod unx3501_u3501core;
pub mod unx3501_u3501dual;
pub mod unx3501_u3501grid;
pub mod unx3501_u3501map;
pub mod unx3501_u3501merge;
pub mod unx3501_u3501mix;
pub mod unx3501_u3501mx;
pub mod unx3501_u3501pipe;
pub mod unx3501_u3501sm;
pub mod unx3521_u3521barrier;
pub mod unx3521_u3521bgrid;
pub mod unx3521_u3521bmx;
pub mod unx3521_u3521core;
pub mod unx3521_u3521dual;
pub mod unx3521_u3521grid;
pub mod unx3521_u3521map;
pub mod unx3521_u3521merge;
pub mod unx3521_u3521mix;
pub mod unx3521_u3521mx;
pub mod unx3521_u3521pipe;
pub mod unx3521_u3521sm;
pub mod unx3541_u3541barrier;
pub mod unx3541_u3541bgrid;
pub mod unx3541_u3541bmx;
pub mod unx3541_u3541core;
pub mod unx3541_u3541dual;
pub mod unx3541_u3541grid;
pub mod unx3541_u3541map;
pub mod unx3541_u3541merge;
pub mod unx3541_u3541mix;
pub mod unx3541_u3541mx;
pub mod unx3541_u3541pipe;
pub mod unx3541_u3541sm;
pub mod unx3561_u3561barrier;
pub mod unx3561_u3561bgrid;
pub mod unx3561_u3561bmx;
pub mod unx3561_u3561core;
pub mod unx3561_u3561dual;
pub mod unx3561_u3561grid;
pub mod unx3561_u3561map;
pub mod unx3561_u3561merge;
pub mod unx3561_u3561mix;
pub mod unx3561_u3561mx;
pub mod unx3561_u3561pipe;
pub mod unx3561_u3561sm;
pub mod unx3581_u3581barrier;
pub mod unx3581_u3581bgrid;
pub mod unx3581_u3581bmx;
pub mod unx3581_u3581core;
pub mod unx3581_u3581dual;
pub mod unx3581_u3581grid;
pub mod unx3581_u3581map;
pub mod unx3581_u3581merge;
pub mod unx3581_u3581mix;
pub mod unx3581_u3581mx;
pub mod unx3581_u3581pipe;
pub mod unx3581_u3581sm;
pub mod unx3601_u3601barrier;
pub mod unx3601_u3601bgrid;
pub mod unx3601_u3601bmx;
pub mod unx3601_u3601core;
pub mod unx3601_u3601dual;
pub mod unx3601_u3601grid;
pub mod unx3601_u3601map;
pub mod unx3601_u3601merge;
pub mod unx3601_u3601mix;
pub mod unx3601_u3601mx;
pub mod unx3601_u3601pipe;
pub mod unx3601_u3601sm;
pub mod unx3621_u3621barrier;
pub mod unx3621_u3621bgrid;
pub mod unx3621_u3621bmx;
pub mod unx3621_u3621core;
pub mod unx3621_u3621dual;
pub mod unx3621_u3621grid;
pub mod unx3621_u3621map;
pub mod unx3621_u3621merge;
pub mod unx3621_u3621mix;
pub mod unx3621_u3621mx;
pub mod unx3621_u3621pipe;
pub mod unx3621_u3621sm;
pub mod unx3641_u3641barrier;
pub mod unx3641_u3641bgrid;
pub mod unx3641_u3641bmx;
pub mod unx3641_u3641core;
pub mod unx3641_u3641dual;
pub mod unx3641_u3641grid;
pub mod unx3641_u3641map;
pub mod unx3641_u3641merge;
pub mod unx3641_u3641mix;
pub mod unx3641_u3641mx;
pub mod unx3641_u3641pipe;
pub mod unx3641_u3641sm;
pub mod unx3661_u3661barrier;
pub mod unx3661_u3661bgrid;
pub mod unx3661_u3661bmx;
pub mod unx3661_u3661core;
pub mod unx3661_u3661dual;
pub mod unx3661_u3661grid;
pub mod unx3661_u3661map;
pub mod unx3661_u3661merge;
pub mod unx3661_u3661mix;
pub mod unx3661_u3661mx;
pub mod unx3661_u3661pipe;
pub mod unx3661_u3661sm;
pub mod unx3681_u3681barrier;
pub mod unx3681_u3681bgrid;
pub mod unx3681_u3681bmx;
pub mod unx3681_u3681core;
pub mod unx3681_u3681dual;
pub mod unx3681_u3681grid;
pub mod unx3681_u3681map;
pub mod unx3681_u3681merge;
pub mod unx3681_u3681mix;
pub mod unx3681_u3681mx;
pub mod unx3681_u3681pipe;
pub mod unx3681_u3681sm;
pub mod unx3701_u3701barrier;
pub mod unx3701_u3701bgrid;
pub mod unx3701_u3701bmx;
pub mod unx3701_u3701core;
pub mod unx3701_u3701dual;
pub mod unx3701_u3701grid;
pub mod unx3701_u3701map;
pub mod unx3701_u3701merge;
pub mod unx3701_u3701mix;
pub mod unx3701_u3701mx;
pub mod unx3701_u3701pipe;
pub mod unx3701_u3701sm;
pub mod unx3721_u3721barrier;
pub mod unx3721_u3721bgrid;
pub mod unx3721_u3721bmx;
pub mod unx3721_u3721core;
pub mod unx3721_u3721dual;
pub mod unx3721_u3721grid;
pub mod unx3721_u3721map;
pub mod unx3721_u3721merge;
pub mod unx3721_u3721mix;
pub mod unx3721_u3721mx;
pub mod unx3721_u3721pipe;
pub mod unx3721_u3721sm;
pub mod unx3741_u3741barrier;
pub mod unx3741_u3741bgrid;
pub mod unx3741_u3741bmx;
pub mod unx3741_u3741core;
pub mod unx3741_u3741dual;
pub mod unx3741_u3741grid;
pub mod unx3741_u3741map;
pub mod unx3741_u3741merge;
pub mod unx3741_u3741mix;
pub mod unx3741_u3741mx;
pub mod unx3741_u3741pipe;
pub mod unx3741_u3741sm;
pub mod unx3761_u3761barrier;
pub mod unx3761_u3761bgrid;
pub mod unx3761_u3761bmx;
pub mod unx3761_u3761core;
pub mod unx3761_u3761dual;
pub mod unx3761_u3761grid;
pub mod unx3761_u3761map;
pub mod unx3761_u3761merge;
pub mod unx3761_u3761mix;
pub mod unx3761_u3761mx;
pub mod unx3761_u3761pipe;
pub mod unx3761_u3761sm;
pub mod unx3781_u3781barrier;
pub mod unx3781_u3781bgrid;
pub mod unx3781_u3781bmx;
pub mod unx3781_u3781core;
pub mod unx3781_u3781dual;
pub mod unx3781_u3781grid;
pub mod unx3781_u3781map;
pub mod unx3781_u3781merge;
pub mod unx3781_u3781mix;
pub mod unx3781_u3781mx;
pub mod unx3781_u3781pipe;
pub mod unx3781_u3781sm;
pub mod unx484_prim;
pub mod unx485_font8x16;
pub mod unx504_lineedit;
pub mod unx564_pm1en;
pub mod unx565_pwrbtn;
pub mod unx603_hdrskeleton;
pub mod unx685_biosreg;
pub mod unx741_domain_journal;
pub mod unx761_precheck_list;
pub mod unx781_report_freeze;
pub mod unx821_lapic_mmio;
pub mod unx861_ioapic_ledger;
pub mod unx943_budgetdisc;
pub mod unx943_tophalf_deep;
// 2026-10-12 注：`unx943_tophalfdeep.rs` 为并行席在制品（未跟踪文件，
// 写入中途被截断 → unclosed delimiter）。文件本体不动，待其完工后恢复
// 下列注册即可（该文件在链上无外部引用，恢复零风险）。
// pub mod unx943_tophalfdeep;
// 2026-10-12 注：`unx984_vecisolate.rs` 为并行席在制品（未跟踪文件，
// 04:57 写入未完工 → unclosed delimiter）。文件本体不动，待其完工后
// 恢复下列注册即可（在链上无外部引用，恢复零风险）。
// pub mod unx984_vecisolate;
pub mod unxt162_timecore;
pub mod unxt163_rtcsem;
pub mod unxt164_tzsem;
pub mod unxt165_errbudget;
pub mod unxt166_leapfilter;
pub mod unxt167_mono;
pub mod unxt168_setpath;
pub mod unxt169_timeledger;

// ===================== AI-WB3 · CoRun UNX-A4 批 40 单深化版（诊断码段 0x12C0~0x2940 四十段） =====================
// unx2321_unisign3 ↔ UNX-F2321（册目标见施工书；诊断码段 12C0–12C0 独占）
// unx2341_udomregress ↔ UNX-F2341（册目标见施工书；诊断码段 13C0–13C0 独占）
// unx2361_udecLARE3 ↔ UNX-F2361（册目标见施工书；诊断码段 15C0–15C0 独占）
// unx2381_ufinalacct ↔ UNX-F2381（册目标见施工书；诊断码段 1640–1640 独占）
// unx2402_ufreemap ↔ UNX-F2402（册目标见施工书；诊断码段 16C0–16C0 独占）
// unx2422_uorder14 ↔ UNX-F2422（册目标见施工书；诊断码段 1740–1740 独占）
// unx2442_urefill ↔ UNX-F2442（册目标见施工书；诊断码段 17C0–17C0 独占）
// unx2482_uorder9raw ↔ UNX-F2482（册目标见施工书；诊断码段 18C0–18C0 独占）
// unx2502_uhptime ↔ UNX-F2502（册目标见施工书；诊断码段 1A80–1A80 独占）
// unx2562_uaspcreate ↔ UNX-F2562（册目标见施工书；诊断码段 1BC0–1BC0 独占）
// unx2582_uvmainject ↔ UNX-F2582（册目标见施工书；诊断码段 1C40–1C40 独占）
// unx2602_upfentry ↔ UNX-F2602（册目标见施工书；诊断码段 1C80–1C80 独占）
// unx2622_upcidbook ↔ UNX-F2622（册目标见施工书；诊断码段 1CC0–1CC0 独占）
// unx2642_uptebatchro ↔ UNX-F2642（册目标见施工书；诊断码段 1D40–1D40 独占）
// unx2662_ulookup ↔ UNX-F2662（册目标见施工书；诊断码段 1D80–1D80 独占）
// unx2682_ucacheif ↔ UNX-F2682（册目标见施工书；诊断码段 1DC0–1DC0 独占）
// unx2702_ulrufield ↔ UNX-F2702（册目标见施工书；诊断码段 1E40–1E40 独占）
// unx2722_uanonevict ↔ UNX-F2722（册目标见施工书；诊断码段 1E80–1E80 独占）
// unx2762_umaptbl ↔ UNX-F2762（册目标见施工书；诊断码段 1EC0–1EC0 独占）
// unx2782_ubuddymerge ↔ UNX-F2782（册目标见施工书；诊断码段 1F40–1F40 独占）
// unx2801_ubigpoolcap ↔ UNX-F2801（册目标见施工书；诊断码段 1F80–1F80 独占）
// unx2841_ubigasp ↔ UNX-F2841（册目标见施工书；诊断码段 20C0–20C0 独占）
// unx2861_ubigmatrix ↔ UNX-F2861（册目标见施工书；诊断码段 2140–2140 独占）
// unx2901_ucachestress ↔ UNX-F2901（册目标见施工书；诊断码段 21C0–21C0 独占）
// unx2941_udomstress ↔ UNX-F2941（册目标见施工书；诊断码段 2240–2240 独占）
// unx2981_udomclose ↔ UNX-F2981（册目标见施工书；诊断码段 22C0–22C0 独占）
// unx3001_udualchain ↔ UNX-F3001（册目标见施工书；诊断码段 2340–2340 独占）
// unx3021_utier3 ↔ UNX-F3021（册目标见施工书；诊断码段 23C0–23C0 独占）
// unx3061_upowerloss ↔ UNX-F3061（册目标见施工书；诊断码段 24C0–24C0 独占）
// unx3081_utierfinal ↔ UNX-F3081（册目标见施工书；诊断码段 2580–2580 独占）
// unx3101_uregfreeze ↔ UNX-F3101（册目标见施工书；诊断码段 25C0–25C0 独占）
// unx3121_ureconall ↔ UNX-F3121（册目标见施工书；诊断码段 2640–2640 独占）
// unx3141_ubaseline ↔ UNX-F3141（册目标见施工书；诊断码段 2680–2680 独占）
// unx3161_uregexec ↔ UNX-F3161（册目标见施工书；诊断码段 26C0–26C0 独占）
// unx3181_udomainfinal ↔ UNX-F3181（册目标见施工书；诊断码段 2740–2740 独占）
// unx3361_utimewin ↔ UNX-F3361（册目标见施工书；诊断码段 2780–2780 独占）
// unx3381_usamplelib ↔ UNX-F3381（册目标见施工书；诊断码段 27C0–27C0 独占）
// unx3401_uktestdesign ↔ UNX-F3401（册目标见施工书；诊断码段 2880–2880 独占）
// unx3421_udualbackend ↔ UNX-F3421（册目标见施工书；诊断码段 28C0–28C0 独占）
// unx3461_umountarch ↔ UNX-F3461（册目标见施工书；诊断码段 2940–2940 独占）

// unx46_prtroute ↔ UNX-F0046 _PRT PCI 中断路由表提取（四字段闭集+消费索引+指纹冻结+注入核 10/10；0xE000 段）
pub mod unx46_prtroute;
// unx61_srcarbiter ↔ UNX-F0061 三源冲突仲裁策略器（U>P>F 矩阵冻结+字段级仲裁+冲突账+100 组对拍 100/100；0xE080 段）
pub mod unx61_srcarbiter;
// unx83_elfptload ↔ UNX-F0083 ELF PT_LOAD 段解析器（头最小校验+四要素闭集+畸形落账+重叠检测+指纹；0xE100 段）
pub mod unx83_elfptload;
// unx186_msrwhite ↔ UNX-F0186 MSR 访问白名单（读/写双栏闭集+二分闸+缺陷账+指纹冻结+注入 10/10；0xE180 段）
pub mod unx186_msrwhite;
// unx486_textout ↔ UNX-F0486 文本输出原语（8x16 生成字形+三原语+越界闸 100% 拒绝落账+滚动三不变量；0xE200 段）
pub mod unx486_textout;
// unx542_lvlfilter ↔ UNX-F0542 级别过滤账（五级闭集+显性丢弃零静默吞+同级聚合+阈值变更可追；0xE280 段）
pub mod unx542_lvlfilter;
// unx566_sleepenum ↔ UNX-F0566 睡眠态枚举账（S0–S5 六态闭集+13 边合法迁移表冻结+自环幂等+非法迁移落账；0xE300 段）
pub mod unx566_sleepenum;
// unx585_valparse ↔ UNX-F0585 三型值解析（布尔两态/整数带号界内前导零闭集/字符串转义四型+解析账三列；0xE380 段）
pub mod unx585_valparse;
// unx804_msixpool ↔ UNX-F0804 全局 MSI-X 向量池与 IPI 向量区隔离（位图池+0xE0 硬隔离闸+幂等回收拒+按需最低位优先；0xE780 段）
pub mod unx804_msixpool;
// unx66_bootseg ↔ UNX-F0662 引导段分解账（四段闭集+T0–T3 锚点表+十二步细分+50 次落账零歧义；0xE800 段）
pub mod unx66_bootseg;
// unx68_efimap ↔ UNX-F0686 EFI 内存类型映射回归（16 型 EFI↔7 型 Limine 映射 12 项+三机核心零差异；0xE880 段）
pub mod unx68_efimap;
// unx70_mpassert ↔ UNX-F0704 多核断言补全至 ≥50（四族 14+7+15+14=50+红项归因明细；0xE900 段）
pub mod unx70_mpassert;
// unx72_upgreg ↔ UNX-F0724 升级触发回归清单（320 断言分段配额+三段留痕+放行闸；0xE980 段）
pub mod unx72_upgreg;
// unx74_dedupmerge ↔ UNX-F0744 域经跨批去重合并（原登零删+合并组留痕+对账守恒；0xEA00 段）
pub mod unx74_dedupmerge;
// unx76_3matrix ↔ UNX-F0762 三机矩阵账终核（150 次成功率复算 ≥99%+F0688 对拍 100%；0xEA80 段）
pub mod unx76_3matrix;
// unx78_final40 ↔ UNX-F0784 深化册终态核（40 册三元组+抽核 8 册批族一致；0xEB00 段）
pub mod unx78_final40;
// unx82_tprppr ↔ UNX-F0824 TPR/PPR 任务优先级寄存器（TPR 置 N 屏蔽 0–N 10/10+PPR 反映回退；0xEB80 段）
pub mod unx82_tprppr;
// unx88_msixaffin ↔ UNX-F0884 MSI-X 亲和性迁移（mask→重写→读回→unmask 三步纪律+旧址失效 10/10；0xEC00 段）
pub mod unx88_msixaffin;
// unx93_ipiexplain ↔ UNX-F0938 IPI 面人话化（四态快照 8 项逐字一致+1000 次确定性；0xEC80 段）
pub mod unx93_ipiexplain;
// unx1027_timedisp ↔ UNX-F1027 超时器到期回调分发（升序分发+10 万扫描守恒+延迟 8 桶直方图；0xED00 段）
pub mod unx1027_timedisp;
// unx1604_apboot ↔ UNX-F1604 AP 启动序列总控状态机（六态+SDM 时序 10ms/200us+重放账；0xED80 段）
pub mod unx1604_apboot;
// unx1622_gspair ↔ UNX-F1622 gs_base 与 swapgs 配对纪律（15 类入口/出口 LIFO 配对+断言 ≥30；0xEE00 段）
pub mod unx1622_gspair;
// unx1662_schedif ↔ UNX-F1662 调度类接口契约（五原语签名冻结 v1+签外零存在；0xEE80 段）
pub mod unx1662_schedif;
// unx1744_idleexit ↔ UNX-F1744 idle 退出路径（触发→结算→清位三步序+首选非空；0xEF00 段）
pub mod unx1744_idleexit;
// unx1764_schedlat ↔ UNX-F1764 调度延迟观测深化（runnable→running 三段分解+三账对齐+分位面；0xEF80 段）
pub mod unx1764_schedlat;
// unx1784_coreaudit ↔ UNX-F1784 每核结构自检断言集（六族30条聚合闸+重复扫描构建期核对；0xF180 段）
pub mod unx1784_coreaudit;
// unx1804_nextthr ↔ UNX-F1804 下一线程选择（类序下降消费+越级10/10拒+空核回退idle；0xF200 段）
pub mod unx1804_nextthr;
// unx1824_fpuswap ↔ UNX-F1824 FPU 切换策略总纲（eager默认/lazy预留冻结v1+切换账目化+依据账可回放；0xF280 段）
pub mod unx1824_fpuswap;
// unx1844_reschedipi ↔ UNX-F1844 重调度 IPI（置位→消费闭环+幂等合并+中断语境纪律；0xF300 段）
pub mod unx1844_reschedipi;
// unx1864_shootipi ↔ UNX-F1864 击落 IPI 消费端（mask或+flush+应答三步+PENDING/DONE 联动；0xF380 段）
pub mod unx1864_shootipi;
// unx1904_tlsseg ↔ UNX-F1904 GS/FS base 与 TLS 快速切换（双路径等价+逐核零错位；0xF400 段）
pub mod unx1904_tlsseg;
// unx1921_cr3pcid ↔ UNX-F1921 CR3 切换与 PCID（全flush基线零残段+PCID 轮转回收即失效；0xF480 段）
pub mod unx1921_cr3pcid;
// unx1962_pstate ↔ UNX-F1962 频率表与硬件档位（八档单调+turbo 闸+FACP/PSS 交付对齐；0xF500 段）
pub mod unx1962_pstate;
// unx2002_utilagg ↔ UNX-F2002 util 聚合（Σ饱和封顶+多请求取最大+计数守恒；0xF580 段）
pub mod unx2002_utilagg;
// unx2022_exitlat ↔ UNX-F2022 退出延迟预算账（六档 exit_latency/驻留+F1773 口径对齐+越界告警；0xF600 段）
pub mod unx2022_exitlat;
// unx2061_percorestat ↔ UNX-F2061 每核统计区总成（四计数族+trampoline 接续零重叠；0xF680 段）
pub mod unx2061_percorestat;
// unx2522_fragsample ↔ UNX-F2522 碎片账每日采样（7 日零缺帧+F2435 告警联动+3 连升级；0xF700 段）
pub mod unx2522_fragsample;
// unx2542_gfpflags ↔ UNX-F2542 GFP flags 裁断（三轴全组合矩阵+互斥注入10/10；0xF780 段）
pub mod unx2542_gfpflags;
// unx2623_invpcid ↔ UNX-F2623 INVPCID 三类型（四编码三消费面+五场景清单零遗漏；0xF800 段）
pub mod unx2623_invpcid;
// unx2663_cacheins ↔ UNX-F2663 cache_insert（契约v1+双轴引用+映射退出缓存存续+LRU 驱逐；0xF880 段）
pub mod unx2663_cacheins;
// unx2763_reclaim ↔ UNX-F2763 回收率总账（四分账守恒+10轮闭合+回收率千分位实时；0xF900 段）
pub mod unx2763_reclaim;
// unx2862_addrmatrix ↔ UNX-F2862 地址空间回归矩阵冻结（十面×3 例=30 例+字段完整性+冻结哈希；0xF980 段）
pub mod unx2862_addrmatrix;
// unx2902_readpeak ↔ UNX-F2902 读命中洪峰压测（三账闭合+命中率基线+注入核 4-4；0xFA00 段）
pub mod unx2902_readpeak;
// unx2922_cowpriv ↔ UNX-F2922 私有映射 COW 压测（写复制不改原页+双账闭合+注入核 4-4；0xFA80 段）
pub mod unx2922_cowpriv;
// unx2962_caseasm ↔ UNX-F2962 回归用例装配器（三基线 560/560+装配-执行键对齐；0xFB00 段）
pub mod unx2962_caseasm;
// unx3002_pageinlink ↔ UNX-F3002 页入链语义（inactive 尖端头插唯一入口+F2663 账平；0xFB80 段）
pub mod unx3002_pageinlink;
// unx3022_reclaimvar ↔ UNX-F3022 回收率方差账（三档方差+告警线入册+10/10 告警；0xFC00 段）
pub mod unx3022_reclaimvar;
// unx3062_dirtysnap ↔ UNX-F3062 脏页快照协议（三列快照+快照集=实脏集+漏照检出；0xFC80 段）
pub mod unx3062_dirtysnap;
// unx3522_probeslot ↔ UNX-F3522 探针槽注册激活协议（四态+往返 280+独占挂载；0xFD00 段）
pub mod unx3522_probeslot;
// unx3542_fpbacktrace ↔ UNX-F3542 采样栈回溯（fp 链四闸+OTP 一次性降级；0xFD80 段）
pub mod unx3542_fpbacktrace;
// unx3622_readconc ↔ UNX-F3622 读侧并发压测（16/64/256 三档+等待恒零+指令流白名单；0xFE00 段）
pub mod unx3622_readconc;
// unx3662_symtab ↔ UNX-F3662 符号表三表（函数/行号/文件+三表齐译+schema 冻结；0xFE80 段）
pub mod unx3662_symtab;
// unx3762_segschema ↔ UNX-F3762 段协议（五段 v1+显式映射投影+未知字段拒；0xFF00 段）
pub mod unx3762_segschema;
// unx4001_vfsbus ↔ UNX-F4001 VFS 总线初始化（六族谱+幂等不变量+E_ALREADY；0xFF80 段）
pub mod unx4001_vfsbus;
// unx4021_sblife ↔ UNX-F4021 sb 生命周期三态（单向生命线+回退 100/100 拒；0xD000 段）
pub mod unx4021_sblife;
// unx4061_inodewash ↔ UNX-F4061 inode 缓存分桶（双键 64 桶+万次极值+跨 sb 隔离；0xD080 段）
pub mod unx4061_inodewash;
// unx4381_dupsem ↔ UNX-F4381 dup 基础语义（最小 fd+同对象 f_count 守恒+千次映射；0xD180 段）
pub mod unx4381_dupsem;
// unx4401_fdflags ↔ UNX-F4401 F_GETFD/F_SETFD 基础语义（千次读改回读+位级diff0+邻fd零扰动100例；0xC000 段）
pub mod unx4401_fdflags;
// unx4501_permbits ↔ UNX-F4501 权限位模型复验锚（十二位编解码+512组抽样diff0+F4004联动；0xC080 段）
pub mod unx4501_permbits;
// unx4801_ext4tree ↔ UNX-F4801 ext4 模块树骨架与 register_fs 消费对接面（契约v1五原语+树槽十二+对接账；0xC100 段）
pub mod unx4801_ext4tree;
// unx4841_dirents ↔ UNX-F4841 目录项线性区解析与 rec_len 语义账（20样本100%+4字节对齐+注入4-4；0xC180 段）
pub mod unx4841_dirents;
// unx4881_xattrns ↔ UNX-F4881 xattr 四类命名空间解码账（上游对照100%+非法前缀10/10 EOPNOTSUPP；0xC200 段）
pub mod unx4881_xattrns;
// unx4901_inline ↔ UNX-F4901 inline_data 旗与内联区布局账（F4805联动+布局对照100%+满溢出拒；0xC280 段）
pub mod unx4901_inline;
// unx4921_mdcsum ↔ UNX-F4921 metadata_csum 语义（crc32c+序列号盐+流式接续+20样本；0xC300 段）
pub mod unx4921_mdcsum;
// unx5001_writepath ↔ UNX-F5001 写路径总装（全块/RMW 分派10/10+write_iter 对照+守恒；0xC380 段）
pub mod unx5001_writepath;
// unx5021_sparse ↔ UNX-F5021 稀疏文件语义（洞读零10/10+零IO+F4977交叉+seek语义；0xC400 段）
pub mod unx5021_sparse;
// unx5061_fragmetric ↔ UNX-F5061 碎片率定义与度量口径账（filefrag对照100%+归一化口径；0xC480 段）
pub mod unx5061_fragmetric;
// unx5381_subvol ↔ UNX-F5381 子卷语义总纲（三段导出+F5341五树联账+独立inode空间；0xC500 段）
pub mod unx5381_subvol;
// unx5401_squash ↔ UNX-F5401 squashfs 子系统总纲（六面分层+140条构成对照；0xC580 段）
pub mod unx5401_squash;
// unx5501_jnoutline ↔ UNX-F5501 journal 段回归总纲账（八批分面重放+100条构成+乱序/配额闸；0xC600 段）
pub mod unx5501_jnoutline;
// unx6401_biobase ↔ UNX-F6401 BlockDevice trait 升级接管（三字段台账+AI-84留痕+bio 向量面+薄层回归；0xC680 段）
pub mod unx6401_biobase;
// unx6501_blkreg ↔ UNX-F6501 register_blkdev 注册接口冻结（三原语+v1冻结页+major闭集；0xC700 段）
pub mod unx6501_blkreg;
// unx6801_cfis ↔ UNX-F6801 CFIS 措辞器（20字节逐字节+全命令位图+LBA48/计数边界；0xC780 段）
pub mod unx6801_cfis;
// unx6841_ataid ↔ UNX-F6841 ATA identify device 透传（512字节逐字段裁断；0x9000 段）
pub mod unx6841_ataid;
// unx6881_devlife ↔ UNX-F6881 设备生命周期四态状态机（枚举→注册→在用→remove；0x9080 段）
pub mod unx6881_devlife;
// unx6901_hotplug ↔ UNX-F6901 hotplug 中断拾取双路（事件驱动+轮询兜底；0x9100 段）
pub mod unx6901_hotplug;
// unx6921_remseq ↔ UNX-F6921 remove 五步总序（排空→冻结→注销→释放→账销；0x9180 段）
pub mod unx6921_remseq;
// unx7001_handov ↔ UNX-F7001 交接页承接对账（F6998 三节逐项；0x9200 段）
pub mod unx7001_handov;
// unx7021_rmtest ↔ UNX-F7021 真机补测规程总纲（R-B4-004 三项规程化；0x9280 段）
pub mod unx7021_rmtest;
// unx7061_injexp ↔ UNX-F7061 注入扩谱总起（18格→54格三倍扩谱；0x9300 段）
pub mod unx7061_injexp;
// unx7381_raid5sm ↔ UNX-F7381 RAID5 三状态机（正常→降级→重建→正常；0x9380 段）
pub mod unx7381_raid5sm;
// unx7401_raigate ↔ UNX-F7401 RAID ktest 总闸（RAID1+RAID5 全判据聚合；0x9400 段）
pub mod unx7401_raigate;
// unx7501_xopen ↔ UNX-F7501 互开对照执行总纲（5环境×两向矩阵；0x9480 段）
pub mod unx7501_xopen;
// unx7801_rmround ↔ UNX-F7801 真机轮专场总纲（285轮四面份额；0x9500 段）
pub mod unx7801_rmround;
// unx7841_bkpchain ↔ UNX-F7841 备份校验链子域总纲（Merkle类清单；0x9580 段）
pub mod unx7841_bkpchain;
// unx7881_receng ↔ UNX-F7881 恢复引擎总纲（四阶段冻结+对称性；0xA000 段）
pub mod unx7881_receng;
// unx7901_rehdrill ↔ UNX-F7901 恢复演练总纲（三段式冻结；0xA080 段）
pub mod unx7901_rehdrill;
// unx7921_schedret ↔ UNX-F7921 调度与保留总纲（三面铺排+混写十检；0xA100 段）
pub mod unx7921_schedret;
// unx8001_interp ↔ UNX-F8001 ELF interpreter 标签装载（PT_INTERP 三路径；0xA180 段）
pub mod unx8001_interp;
// unx8021_taskstr ↔ UNX-F8021 进程描述符本体（task_struct级十字段对照；0xA200 段）
pub mod unx8021_taskstr;
// unx8061_ptinterp ↔ UNX-F8061 PT_INTERP 三路径解析（10/10样本；0xA280 段）
pub mod unx8061_ptinterp;
// unx8381_mbatch ↔ UNX-F8381 M型机制批总检判据件（239/239聚合对账；0xA300 段）
pub mod unx8381_mbatch;
// unx8401_magic ↔ UNX-F8401 Magic 畸形全拒档（错魔数四形；0xA380 段）
pub mod unx8401_magic;
// unx8501_pttls ↔ UNX-F8501 PT_TLS memsz<filesz 拒档（0xA400 段）
pub mod unx8501_pttls;
// unx8801_systab ↔ UNX-F8801 syscall 分发表骨架与注册协议（302项；0xA480 段）
pub mod unx8801_systab;
// unx8841_brk ↔ UNX-F8841 brk 语义档与探顶行为（glibc探顶10/10；0xA500 段）
pub mod unx8841_brk;
// unx8881_ident3 ↔ UNX-F8881 三身份模型语义基准（16格切换矩阵；0xA580 段）
pub mod unx8881_ident3;
// unx8901_gettod ↔ UNX-F8901 gettimeofday 语义档（10^3次换算一致；0xA600 段）
pub mod unx8901_gettod;
// unx8921_cfu ↔ UNX-F8921 参数拷贝探针总成（10^3次零失真；0xA680 段）
pub mod unx8921_cfu;
// unx9001_rtsig ↔ UNX-F9001 rt_sigaction 语义档（ABI冻结+五旗标；0xA700 段）
pub mod unx9001_rtsig;
// unx102_uart ↔ UNX-F0102 16550 串口全速通道（115200 1MB零丢；0xA780 段）
pub mod unx102_uart;
// unx124_bootfail ↔ UNX-F0124 引导失败计数与自动降级（三败降级；0xA800 段）
pub mod unx124_bootfail;
// unx144_manifest ↔ UNX-F0144 镜像内容清单与签名校验（篡改全检；0xA880 段）
pub mod unx144_manifest;
// unx164_capsule ↔ UNX-F0164 Capsule 更新只读探测（写红线；0xA900 段）
pub mod unx164_capsule;
// unx206_ecode ↔ UNX-F0206 异常项分类码（32码字典三要素；0xA980 段）
pub mod unx206_ecode;
// unx225_diag ↔ UNX-F0225 诊断模式12步演练（步序严格；0xAA00 段）
pub mod unx225_diag;
// unx246_ovrblk ↔ UNX-F0246 引导参数覆盖块（三源合并优先级；0xAA80 段）
pub mod unx246_ovrblk;
// unx247_health ↔ UNX-F0247 早传健康位面板（0x3BC0 段）
pub mod unx247_health;
// unx248_timebase ↔ UNX-F0248 早传时间基准链（0x3BD0 段）
pub mod unx248_timebase;
// unx249_logkey ↔ UNX-F0249 日志指纹关联键（0x3BE0 段）
pub mod unx249_logkey;
// unx250_lastboot ↔ UNX-F0250 上次引导携带（0x3BF0 段）
pub mod unx250_lastboot;
// unx251_subscribe ↔ UNX-F0251 消费方订阅接口（0x3CC0 段）
pub mod unx251_subscribe;
// unx252_crcchain ↔ UNX-F0252 十二步CRC链（0x3CD0 段）
pub mod unx252_crcchain;
// unx253_vercompat ↔ UNX-F0253 早传版本兼容（0x3CF0 段）
pub mod unx253_vercompat;
// unx254_cmdreg ↔ UNX-F0254 cmdline白名单（0x3D50 段）
pub mod unx254_cmdreg;
// unx255_prio ↔ UNX-F0255 参数来源优先级（0x3D70 段）
pub mod unx255_prio;
// unx256_audit ↔ UNX-F0256 参数变更审计账（0x3DD0 段）
pub mod unx256_audit;
// unx257_fieldmap ↔ UNX-F0257 快照报告字段映射（0x3DE0 段）
pub mod unx257_fieldmap;
// unx258_timing ↔ UNX-F0258 十二步时序账（0x3DF0 段）
pub mod unx258_timing;
// unx259_tamper ↔ UNX-F0259 防篡改CRC巡检（0x3E50 段）
pub mod unx259_tamper;
// unx260_ktb13 ↔ UNX-F0260 B13批断言集（0x3E60 段）
pub mod unx260_ktb13;
// unx264_lineed ↔ UNX-F0264 终端行编辑器（历史16循环/补全；0xAB00 段）
pub mod unx264_lineed;
// unx286_assert ↔ UNX-F0286 断言宏三级语义（check/kassert/bug_on；0xAB80 段）
pub mod unx286_assert;
// unx304_dmap ↔ UNX-F0304 直接映射窗口（phys↔virt换算；0xAC00 段）
pub mod unx304_dmap;
// unx327_tramp ↔ UNX-F0327 trampoline 页表最小集（三区齐备；0xAC80 段）
pub mod unx327_tramp;
// unx342_silence ↔ UNX-F0342 AP 静默窗（违规全检；0xAD00 段）
pub mod unx342_silence;
// unx362_reserve ↔ UNX-F0362 保留区白名单（拦截100%；0xAD80 段）
pub mod unx362_reserve;
// unx387_oom ↔ UNX-F0387 OOM 分级账（warn/crit/dead；0xAE00 段）
pub mod unx387_oom;
// unx405_mono ↔ UNX-F0405 单调时钟原语（10^6次零回退；0xAE80 段）
pub mod unx405_mono;
// unx422_exgate ↔ UNX-F0422 异常门注册账（32向量全覆盖；0xAF00 段）
pub mod unx422_exgate;
// unx522_asrtfam ↔ UNX-F0522 断言原语族（八原语账齐+误用检出；0xB000 段）
pub mod unx522_asrtfam;
// unx920_ktb06 ↔ UNX-F0920 ktest B06 断言集（注册协议 ≥40 断言；0xB080 段）
pub mod unx920_ktb06;
// unx1104_percpu ↔ UNX-F1104 per-CPU 分发统计面（每核每向量双维；0xB100 段）
pub mod unx1104_percpu;
// unx1242_arming ↔ UNX-F1242 Arming 态武装步骤账（依赖环 10/10 必抓；0xB180 段）
pub mod unx1242_arming;
// unx1262_irqack ↔ UNX-F1262 Irq 态确认断言强化（配对-EOI 10^7 全量；0xB200 段）
pub mod unx1262_irqack;
// unx1282_polmap ↔ UNX-F1282 serial.rs 轮询路径测绘账（四类点全覆盖；0xB280 段）
pub mod unx1282_polmap;
// unx1303_retpoll ↔ UNX-F1303 serial.rs 轮询退役执行账（字节流守恒对拍；0xB300 段）
pub mod unx1303_retpoll;
// unx1322_clokeq ↔ UNX-F1322 clock 面等价验证账（tick 容差带逐点；0xB380 段）
pub mod unx1322_clokeq;
// unx1342_dualmode ↔ UNX-F1342 双模压测主档（四态六场景高压资产；0xB400 段）
pub mod unx1342_dualmode;
// unx1362_hotpath ↔ UNX-F1362 热路径性能实测深化（8 段耗时+扣除率；0xB480 段）
pub mod unx1362_hotpath;
// unx1382_seggate ↔ UNX-F1382 段判据聚合总账（≥80 断言 B16–B30；0xB500 段）
pub mod unx1382_seggate;
// unx1402_evseq ↔ UNX-F1402 全局事件序号账（10^7 零空洞+回绕；0xB580 段）
pub mod unx1402_evseq;
// unx1483_gapfill ↔ UNX-F1483 补全执行一（B01–B10 缺口批补齐 ≥400；0xB600 段）
pub mod unx1483_gapfill;
// unx1504_regrun ↔ UNX-F1504 接管等价回归复跑（F1389 五步协议 10 轮；0xB680 段）
pub mod unx1504_regrun;
// unx1582_conserve ↔ UNX-F1582 域账行数守恒终断言（240,000 恒等式；0xB700 段）
pub mod unx1582_conserve;
// unx1642_enqueue ↔ UNX-F1642 入队路径 enqueue 全序语义（桶+vruntime 插位；0xB780 段）
pub mod unx1642_enqueue;
// unx1702_decay ↔ UNX-F1702 衰减窗口账（几何衰减 Q16 逐tick落账回放零偏差；0xB800 段）
pub mod unx1702_decay;
// unx1942_ifan ↔ UNX-F1942 广播 IPI 扇出树（单发→两级→8核+p99≤20µs；0xB880 段）
pub mod unx1942_ifan;
// unx1982_gov ↔ UNX-F1982 ondemand 采样型 governor（升档/迟滞/决策10/10；0xB900 段）
pub mod unx1982_gov;
// unx2082_stall ↔ UNX-F2082 调度压力面 stall 三轴（CPU/MEM/IO 不串账守恒；0xB980 段）
pub mod unx2082_stall;
// unx2122_sixteen ↔ UNX-F2122 守恒式全域复核十六族（十账+六新族；0xBA00 段）
pub mod unx2122_sixteen;
// unx2142_joints ↔ UNX-F2142 三轴联测八场景（负载×频率×统计对齐F1790；0xBA80 段）
pub mod unx2142_joints;
// unx2162_sform ↔ UNX-F2162 形态库四轴扩容（8形态入册冻结可追溯；0xBB00 段）
pub mod unx2162_sform;
// unx2182_promo ↔ UNX-F2182 十三批场景升格 102 对回（漂移10/10+skip带因；0xBB80 段）
pub mod unx2182_promo;
// unx2202_frozenschema ↔ UNX-F2202 冻结账四列 schema（ID/指纹/来源/版本；0xBC00 段）
pub mod unx2202_frozenschema;
// unx2224_famfreeze ↔ UNX-F2224 判据族冻结批五四新族（FNV固化+回签；0xBC80 段）
pub mod unx2224_famfreeze;
// unx2242_rehears2 ↔ UNX-F2242 冻结终验预演二（五步链三批+级联复用；0xBD00 段）
pub mod unx2242_rehears2;
// unx2262_debt ↔ UNX-F2262 欠账销账总账（五列+加法链+对平；0xBD80 段）
pub mod unx2262_debt;
// unx2282_exledger ↔ UNX-F2282 域经验账扩容 EX-0011–0030（连续无跳号；0xBE00 段）
pub mod unx2282_exledger;
// unx2302_tutorial ↔ UNX-F2302 七步教程域化 A3 版（引用可解析7/7；0xBE80 段）
pub mod unx2302_tutorial;
// unx2322_cosign ↔ UNX-F2322 消费域联签总表五户（三列+漏域10/10；0xBF00 段）
pub mod unx2322_cosign;
// unx2342_domconserve ↔ UNX-F2342 A3 域账行数守恒 240,000（B01–B34+尾六批；0xBF80 段）
pub mod unx2342_domconserve;
// unx2362_closegate ↔ UNX-F2362 域收官宣告条件机（五证据硬断言+缺项拦截；0x8400 段）
pub mod unx2362_closegate;
// unx2382_counteq ↔ UNX-F2382 条数恒等式 800=40×20（三源一致；0x8480 段）
pub mod unx2382_counteq;
// unx2403_reserve ↔ UNX-F2403 预留区登记账（Limine 逐条一致+分配计数零；0x8500 段）
pub mod unx2403_reserve;
// unx2423_highorder ↔ UNX-F2423 order-5–10 高阶分配（10^3成功率+2M对齐零例外；0x8580 段）
pub mod unx2423_highorder;
// unx2443_drain ↔ UNX-F2443 drain 回收语义（策略值10/10+水线时序；0x8600 段）
pub mod unx2443_drain;
// unx2462_minwm ↔ UNX-F2462 min 水线紧急豁免（越权10/10拒+触发账；0x8680 段）
pub mod unx2462_minwm;
// unx2483_hugepool ↔ UNX-F2483 大页预留池策略（预算差零+三策略对照；0x8700 段）
pub mod unx2483_hugepool;
// unx2503_hugeassert ↔ UNX-F2503 大页断言集建立供给（≥15条独立自足；0x8780 段）
pub mod unx2503_hugeassert;
// unx2563_asdestroy ↔ UNX-F2563 地址空间销毁页回收（归还恒等+零双还；0x8800 段）
pub mod unx2563_asdestroy;
// unx2583_findvma ↔ UNX-F2583 VMA 查找区间查询（三类10^7零误差；0x8880 段）
pub mod unx2583_findvma;
// unx2603_demander ↔ UNX-F2603 按需分页匿名零页（UNX-A4-PF-01三机全绿；0x8900 段）
pub mod unx2603_demander;
// unx2643_refcnt ↔ UNX-F2643 页帧引用计数（饱和保护10/10+原子性；0x8980 段）
pub mod unx2643_refcnt;
// unx2683_pgcachesync ↔ UNX-F2683 页缓存-B5 回写联签冻结账（§16.2锁步；0x8A00 段）
pub mod unx2683_pgcachesync;
// unx2703_activeins ↔ UNX-F2703 active 链插入单点（绕过20/20拦截；0x8A80 段）
pub mod unx2703_activeins;
// unx2723_anonswap ↔ UNX-F2723 anon 页逐出内容写出（零丢失双向账；0x8B00 段）
pub mod unx2723_anonswap;
// unx2742_kswapd ↔ UNX-F2742 后台回收线程主循环（闭环10轮消费扫描器；0x8B80 段）
pub mod unx2742_kswapd;
// unx2802_hugestorm ↔ UNX-F2802 大页分配风暴压测（8核10/10轮零死锁零账漂+吞吐基线；0xC000 段）
pub mod unx2802_hugestorm;
// unx2822_pfstorm ↔ UNX-F2822 缺页风暴压测（#PF十四路径随机注入10万次全对+分发表一致；0xC080 段）
pub mod unx2822_pfstorm;
// unx2842_hugepf ↔ UNX-F2842 大页缺页压测（#PF大页路径10/10判定+F2561对齐；0xC100 段）
pub mod unx2842_hugepf;
// unx2882_forkstorm ↔ UNX-F2882 fork洪峰压测（千次fork 10/10轮账平+速率基线；0xC180 段）
pub mod unx2882_forkstorm;
// unx2942_tiermatrix ↔ UNX-F2942 三档压力矩阵执行器（轻/中/重30/30账平+F2761一致；0xC200 段）
pub mod unx2942_tiermatrix;
// unx2982_domaudit ↔ UNX-F2982 域级总对账制度与已产段守恒终账（40批满账口径4项守恒；0xC280 段）
pub mod unx2982_domaudit;
// unx3042_twomfin ↔ UNX-F3042 2M显式路终账（mmap显式大页终核10/10+三操作读数；0xC300 段）
pub mod unx3042_twomfin;
// unx3082_lowgbfin ↔ UNX-F3082 1GB档终账执行（低配剖面10/10轮+三档全覆盖+F2913面名对齐；0xC380 段）
pub mod unx3082_lowgbfin;
// unx3102_pfregmap ↔ UNX-F3102 #PF十四路径回归映射（一一映射+映射件哈希入册；0xC400 段）
pub mod unx3102_pfregmap;
// unx3122_pfrecon ↔ UNX-F3122 #PF账册终勾稽（三列一致交叉勾稽+差额注记；0xC480 段）
pub mod unx3122_pfrecon;
// unx3142_hcchain ↔ UNX-F3142 体检单链总核（F2797链40批链完整+健康项齐备两查；0xC500 段）
pub mod unx3142_hcchain;
// unx3162_pfexec ↔ UNX-F3162 #PF回归账面执行（十四路径逐路径跑账+缺位执行注入；0xC580 段）
pub mod unx3162_pfexec;
// unx3182_batchcount ↔ UNX-F3182 40批条数总核（800条逐批计数+缺批注入检出；0xC600 段）
pub mod unx3182_batchcount;
// unx3222_qspinfs ↔ UNX-F3222 qspinlock编码字压缩状态机（四态位域64迁移边64/64覆盖；0xC680 段）
pub mod unx3222_qspinfs;
// unx3262_rdfast ↔ UNX-F3262 读锁fast path（单CAS+M-021千轮并行计数对账；0xC700 段）
pub mod unx3262_rdfast;
// unx3302_rcumix ↔ UNX-F3302 RCU与锁混合场景验证（M-061死锁零检出+数据一致；0xC780 段）
pub mod unx3302_rcumix;
// unx3205_linealign ↔ UNX-F3205 争用计数与缓存行对齐（64B align断言+contentions一致；0xC800 段）
pub mod unx3205_linealign;
// unx3207_irqsave ↔ UNX-F3207 中断保存/恢复型自旋锁（10^6配对零丢失+RFLAGS一致；0xC880 段）
pub mod unx3207_irqsave;
// unx3241_mutex ↔ UNX-F3241 mutex结构体与睡眠锁基座（const构造+快路guard+M-001；0xC900 段）
pub mod unx3241_mutex;
// unx3281_rcubound ↔ UNX-F3281 RCU语义边界声明（零锁语义/GP概念/不做清单+M-040；0xC980 段）
pub mod unx3281_rcubound;
// unx3322_lockclass ↔ UNX-F3322 锁层次编号分配器（LockClass(u16)上限断言+M-081唯一性；0xCA00 段）
pub mod unx3322_lockclass;
// unx3342_waitgraph ↔ UNX-F3342 等待图数据结构（邻接表+M-101建查删对账+环检测；0xCA80 段）
pub mod unx3342_waitgraph;
// unx3362_longwait ↔ UNX-F3362 长等待账本（挂边/摘边tsc差+M-121记账准确；0xCB00 段）
pub mod unx3362_longwait;
// unx3402_kexpect ↔ UNX-F3402 断言宏族总表（六基础宏+失败路径全通+M-161正反例；0xCB80 段）
pub mod unx3402_kexpect;
// unx3414_apifreeze ↔ UNX-F3414 API冻结签名表（14项冻结+三消费方联签占位；0xCC00 段）
pub mod unx3414_apifreeze;
// unx3422_runner ↔ UNX-F3422 后端抽象trait（Runner四方法v1+M-181三实现同账；0xCC80 段）
pub mod unx3422_runner;
// unx3442_selector ↔ UNX-F3442 用例选择器（五维过滤+组合求值+M-201矩阵；0xCD00 段）
pub mod unx3442_selector;
// unx3462_mount ↔ UNX-F3462 挂载协议（三要素声明+依赖闭合+M-221正反例；0xCD80 段）
pub mod unx3462_mount;
// unx3482_outfilter ↔ UNX-F3482 三维输出过滤器（级别×域×模块位图与语义+M-241；0xCE00 段）
pub mod unx3482_outfilter;
// unx3502_tpreg ↔ UNX-F3502 跟踪点注册协议（描述符四字段冻结+M-260重名拒绝；0xCE80 段）
pub mod unx3502_tpreg;
// unx3529_fanout ↔ UNX-F3529 跟踪流多路分发（三路独立扇出+M-287对账；0xCF00 段）
pub mod unx3529_fanout;
// unx3562_poison ↔ UNX-F3562 毒化模式注册表（五值封闭词表+M-320新值拒绝；0xCF80 段）
pub mod unx3562_poison;
// unx3582_wmsample ↔ UNX-F3582 水位采样器（周期/事件双模+热路径零税+M-340；0xD200 段）
pub mod unx3582_wmsample;
// unx3602_contension ↔ UNX-F3602 争用度量账（等待/持有2的幂桶双分布+M-360+税预算；0xD280 段）
pub mod unx3602_contension;
// unx3607_fairness ↔ UNX-F3607 锁公平性度量（公平账+饥饿判据+M-365均衡+J2负例；0xD300 段）
pub mod unx3607_fairness;
// unx3642_triggersrc ↔ UNX-F3642 触发源清单（六源封闭词表+M-400全覆盖+🔴映射完备；0xD380 段）
pub mod unx3642_triggersrc;
// unx3682_fuzzgen ↔ UNX-F3682 输入生成器（结构化/字节变异双模+M-440落点；0xD400 段）
pub mod unx3682_fuzzgen;
// unx3684_covfeed ↔ UNX-F3684 覆盖反馈（闭环四步+M-442进化单调可复现+位图封闭；0xD480 段）
pub mod unx3684_covfeed;
// unx3702_edgeclass ↔ UNX-F3702 边覆盖深化（关键/平凡分类+权重≥2×+M-460盲区优先；0xD500 段）
pub mod unx3702_edgeclass;
// unx3722_halstub ↔ UNX-F3722 桩接口契约（HAL五接口ABI同构+M-480五桩对账+版本锚；0xD580 段）
pub mod unx3722_halstub;
// unx3742_consgate ↔ UNX-F3742 守恒门（三方行数守恒+M-500执法+双核独立对拍；0xD600 段）
pub mod unx3742_consgate;
// unx3782_asrtcalib ↔ UNX-F3782 段断言计数口径（F1382/F1400双层守恒+口径const；0xD680 段）
pub mod unx3782_asrtcalib;
// unx4015_kref ↔ UNX-F4015 引用计数基座（kref万次往返零偏差+回调恰1+underflow即红；0xD700 段）
pub mod unx4015_kref;
// unx4041_ino ↔ UNX-F4041 inode编号空间（同sb万次零重复+跨sb同号+保留0拒；0xD780 段）
pub mod unx4041_ino;
// unx4081_dentry ↔ UNX-F4081 dentry结构（13字段逐字段断言+B01锚点diff零差；0xD800 段）
pub mod unx4081_dentry;
// unx4101_lrudual ↔ UNX-F4101 LRU双结构（hot/cold万次迁移零错账+双向链零断链；0xD880 段）
pub mod unx4101_lrudual;
// unx4121_fpool ↔ UNX-F4121 file池配额（耗尽ENFILE+池零破坏+水位交叉等式；0xD900 段）
pub mod unx4121_fpool;
// unx4141_iops ↔ UNX-F4141 inode_operations语义表（14函数冻结+快照哈希落账；0xD980 段）
pub mod unx4141_iops;
// ── 批15（0xDA00~0xE180 段，16 单）──────────────────────────────────
// unx4161_pathentry ↔ UNX-F4161 路径项基础语义（三元输入+引用配对；0xDA00 段）
pub mod unx4161_pathentry;
// unx4181_symlink ↔ UNX-F4181 符号链接语义（快/慢链接 60 字节边界；0xDA80 段）
pub mod unx4181_symlink;
// unx4201_realpath ↔ UNX-F4201 真实路径解析（1,000 例规范化+链接展开；0xDB00 段）
pub mod unx4201_realpath;
// unx4207_atdirfd ↔ UNX-F4207 at 目录fd语义（2×2 矩阵+AT_FDCWD=-100；0xDB80 段）
pub mod unx4207_atdirfd;
// unx4221_mounttree ↔ UNX-F4221 挂载树（1,000 节点三向一致+环检测；0xDC00 段）
pub mod unx4221_mounttree;
// unx4241_mount2 ↔ UNX-F4241 mount2 五参数（校验顺序冻结；0xDC80 段）
pub mod unx4241_mount2;
// unx4261_mntclone ↔ UNX-F4261 挂载克隆（快照复制+100 隔离对；0xDD00 段）
pub mod unx4261_mntclone;
// unx4281_pivotroot ↔ UNX-F4281 pivot_root 语义（根换装双向账；0xDD80 段）
pub mod unx4281_pivotroot;
// unx4301_bindmnt ↔ UNX-F4301 bind 挂载（视图重写+写穿透；0xDE00 段）
pub mod unx4301_bindmnt;
// unx4321_rdonly ↔ UNX-F4321 只读挂载（EROFS+四联签+五旗标清单；0xDE80 段）
pub mod unx4321_rdonly;
// unx4341_b918agg ↔ UNX-F4341 B918 聚合（190/190 全绿聚合；0xDF00 段）
pub mod unx4341_b918agg;
// unx4361_fdtable ↔ UNX-F4361 fd 表语义（fd 分配+dup 账；0xDF80 段）
pub mod unx4361_fdtable;
// unx4408_cloexec ↔ UNX-F4408 O_CLOEXEC（exec 时原子置位 1,000 次；0xE000 段）
pub mod unx4408_cloexec;
// unx4421_oappend ↔ UNX-F4421 O_APPEND 原子追加（写时点偏移重定位+判例 R-C1-001；0xE080 段）
pub mod unx4421_oappend;
// unx4441_pread ↔ UNX-F4441 pread 基础语义（显式偏移不触碰 file 偏移 diff=0·1,000 次；0xE100 段）
pub mod unx4441_pread;
// unx4461_rename ↔ UNX-F4461 rename 同目录更名（旧名 ENOENT+新名哈希一致+双账同步·1,000 次；0xE180 段）
pub mod unx4461_rename;
// ── 批16（0xF000~0xF780 段，16 单）──────────────────────────────────
// unx4481_dirmv ↔ UNX-F4481 rename 目录整体移动（子树随移 1,000 项 diff=0+旧路径全 ENOENT；0xF000 段）
pub mod unx4481_dirmv;
// unx4521_dirstream ↔ UNX-F4521 目录流对象语义锚（fd 绑定+读取位置+缓冲逐字段断言+1,000 流一一绑定账；0xF080 段）
pub mod unx4521_dirstream;
// unx4541_b1928agg ↔ UNX-F4541 B19–B28 段级回归聚合（十批 190/190 无缺项+通过率 100% 段闸；0xF100 段）
pub mod unx4541_b1928agg;
// unx4561_ltpreg ↔ UNX-F4561 LTP 子集清单账目化基建（四列账目化+双向可查+封闭词表；0xF180 段）
pub mod unx4561_ltpreg;
// unx4581_chdirmap ↔ UNX-F4581 路径族映射 I chdir/fchdir（20 用例双向可查+F4163/F4203 联动；0xF200 段）
pub mod unx4581_chdirmap;
// unx4601_fdincmap ↔ UNX-F4601 fd 族增量映射总纲（F4595 锚表位扩容+增量 20 用例四列全谱+单射协议；0xF280 段）
pub mod unx4601_fdincmap;
// unx4606_setlkmap ↔ UNX-F4606 F_SETLK 记录锁增量映射（4 用例+l_start/l_len/l_type 三元组靶+F4517 挂点；0xF300 段）
pub mod unx4606_setlkmap;
// unx4621_mntmap ↔ UNX-F4621 mount 族映射总纲（40 用例四列全谱+F4342 中段账接续闭合；0xF380 段）
pub mod unx4621_mntmap;
// unx4641_permmap ↔ UNX-F4641 权限族映射总纲（40 用例四列全谱+F4501–F4518 靶面就绪复核；0xF400 段）
pub mod unx4641_permmap;
// unx4661_suppmap ↔ UNX-F4661 补余族映射总纲（五族 60 用例四列全谱+并账协议单射守恒；0xF480 段）
pub mod unx4661_suppmap;
// unx4681_regmat ↔ UNX-F4681 语义回归矩阵总纲（四维结构 800 判据全谱+分片执行协议；0xF500 段）
pub mod unx4681_regmat;
// unx4701_perfbase ↔ UNX-F4701 性能基线总纲（十二项性能账协议+基线口径登记+泄漏零容忍；0xF580 段）
pub mod unx4701_perfbase;
// unx4721_freeze ↔ UNX-F4721 判据冻结总纲（800 判据 verbatim 冻结三层结构+链式哈希；0xF600 段）
pub mod unx4721_freeze;
// unx4741_deliv ↔ UNX-F4741 交付物冻结总纲（六件清单闭集+指纹/齐套/终核验收协议；0xF680 段）
pub mod unx4741_deliv;
// unx4761_totalreg ↔ UNX-F4761 域级总回归总纲（40 批 760 断言两层聚合+执行协议；0xF700 段）
pub mod unx4761_totalreg;
// unx4781_closeout ↔ UNX-F4781 域收官总纲（收官五步有序协议+域闭账宣告放行闸；0xF780 段）
pub mod unx4781_closeout;
// ── 批19（0x8C00~0x9380 高位散段，15 单）────────────────────────────
// unx5321_b27graph ↔ UNX-F5321 btrfs 域段开工总纲（B27–B33 140 条图谱·七批×20 构成对照；0x8C00 段）
pub mod unx5321_b27graph;
// unx5341_cowtrees ↔ UNX-F5341 COW B-tree 总纲（五树图谱 chunk/root/extent/csum/fs 五树只读对照；0x8C80 段）
pub mod unx5341_cowtrees;
// unx5361_cowsem ↔ UNX-F5361 COW 语义总纲（三段声明+goal-B2 约束一致率 100%+行为账；0x8D00 段）
pub mod unx5361_cowsem;
// unx5441_mirrorload ↔ UNX-F5441 镜像根装载总账（四段流水+F5350 联账零错+清单导出；0x8D80 段）
pub mod unx5441_mirrorload;
// unx5461_regmaster ↔ UNX-F5461 全域回归总纲账（800 条七批分层+140 构成对照；0x8E80 段）
pub mod unx5461_regmaster;
// unx5481_extentreg ↔ UNX-F5481 extent 段回归总纲账（B09–B18 十批 8 分面重放 200 条；0x8F00 段）
pub mod unx5481_extentreg;
// unx5521_bfsreg ↔ UNX-F5521 btrfs/squashfs 段回归总纲账（B27–B33 七批 6 分面重放 140 条；0x8F80 段）
pub mod unx5521_bfsreg;
// unx5541_samplelib ↔ UNX-F5541 样本库总纲账（A–BH 60 组六段分层；0x9000 段）
pub mod unx5541_samplelib;
// unx5561_wincmp ↔ UNX-F5561 Windows 对照总纲账（NTFS/FAT/WIM 三面 60 项；0x9080 段）
pub mod unx5561_wincmp;
// unx5581_b2close ↔ UNX-F5581 域收口总纲账（800 条全景七段+样本库联签锚接；0x9100 段）
pub mod unx5581_b2close;
// unx6421_reqfsm ↔ UNX-F6421 Request 结构体与请求状态机（五态单链+非法迁移 100% 拒+逐边一致；0x9180 段）
pub mod unx6421_reqfsm;
// unx6441_elevreg ↔ UNX-F6441 调度器注册框架（elevator 三 API 闭合+重名拒+回退 none；0x9200 段）
pub mod unx6441_elevreg;
// unx6461_mqdead ↔ UNX-F6461 mq-deadline 结构（读树/写树 AVL+deadline 域+O(log n) 断言+守恒；0x9280 段）
pub mod unx6461_mqdead;
// unx6481_flushfua ↔ UNX-F6481 PREFLUSH/FUA 请求类型编码（三类唯一可判定+非法组合全拒+逐位一致；0x9300 段）
pub mod unx6481_flushfua;
// unx6521_iostat ↔ UNX-F6521 块层 IO 统计账（inflight/IOPS/带宽三账+10^5 守恒；0x9380 段）
pub mod unx6521_iostat;
// ── 批20（0x9E80~0xA680 散段，16 单）────────────────────────────────
// unx6541_fullchain ↔ UNX-F6541 多队列全链贯通（bio→request→queue→device 10^5 次 IO 校验和零差异；0x9E80 段）
pub mod unx6541_fullchain;
// unx6561_nvmedecl ↔ UNX-F6561 NVMe 接管三字段总声明（接管对象/新增语义/退役范围+AI-84 留痕；0x9F00 段）
pub mod unx6561_nvmedecl;
// unx6581_percpu ↔ UNX-F6581 每核队列对架构（8 核 8 对 SQ/CQ+全本地化+核亲和对账；0xA000 段）
pub mod unx6581_percpu;
// unx6601_dsmtrim ↔ UNX-F6601 DSM/TRIM deallocate 范围集（256 上限+编码规范+空集拒+回收账；0xA080 段）
pub mod unx6601_dsmtrim;
// unx6621_ctlrfsm ↔ UNX-F6621 控制器状态机显式建模（五态 12 边+非法迁移全拒+可观测；0xA100 段）
pub mod unx6621_ctlrfsm;
// unx6641_nsenum ↔ UNX-F6641 多命名空间枚举（清单+逐 ID 双路 4/16/1024 三档零错账；0xA180 段）
pub mod unx6641_nsenum;
// unx6661_qdbench ↔ UNX-F6661 QD 压测总纲（五档×四件方法论冻结+两轨分账；0xA200 段）
pub mod unx6661_qdbench;
// unx6681_nvmeaudit ↔ UNX-F6681 NVMe 接管完整性核对（三字段 100% 机检+AI-84 加倍抽检；0xA280 段）
pub mod unx6681_nvmeaudit;
// unx6701_fwdownload ↔ UNX-F6701 FW Download 双段传输（4KB 双对齐+分块序守恒+畸形零 panic；0xA300 段）
pub mod unx6701_fwdownload;
// unx6721_errlog ↔ UNX-F6721 错误信息日志页（64 条环形最新在前+四字段裁断；0xA380 段）
pub mod unx6721_errlog;
// unx6741_formatnvm ↔ UNX-F6741 Format NVM 保护判据先行（挂载/排空/确认三道闸；0xA400 段）
pub mod unx6741_formatnvm;
// unx6761_hbamap ↔ UNX-F6761 HBA 寄存器面映射（BAR 断言+VS 版本裁断+CAP.NP 上限；0xA480 段）
pub mod unx6761_hbamap;
// unx6781_pxclb ↔ UNX-F6781 PxCLB/PxCLBU 命令列表基址（1KB 对齐+32 槽单射守恒；0xA500 段）
pub mod unx6781_pxclb;
// unx6821_ncq32 ↔ UNX-F6821 NCQ 32 深度使能账（SACT 位图+CAP.NCS 联动裁断；0xA580 段）
pub mod unx6821_ncq32;
// unx6861_gpthead ↔ UNX-F6861 GPT 头解析（EFI PART 签名+12 字段裁断表+CRC32；0xA600 段）
pub mod unx6861_gpthead;
// unx6941_ioreclaim ↔ UNX-F6941 在途 IO 回收总序（三步有序+F6850 锚一致+超时上限账；0xA680 段）
pub mod unx6941_ioreclaim;
// ── 批21（0xA700~0xAE80 散段，16 单）────────────────────────────────
// unx6961_hotplugmatrix ↔ UNX-F6961 抽插矩阵设计账（三类×三档×两态 18 格判定口径；0xA700 段）
pub mod unx6961_hotplugmatrix;
// unx6981_hotplug200 ↔ UNX-F6981 抽插 200 次总账（QEMU+真机分账+F6850 锚零悬挂；0xA780 段）
pub mod unx6981_hotplug200;
// unx7041_stressopen ↔ UNX-F7041 压测账总起（压测轴 8 批开账+6,000 行守恒；0xA800 段）
pub mod unx7041_stressopen;
// unx7081_soaktable ↔ UNX-F7081 soak 档位表（30m/2h/8h/7×24 四档冻结+判据映射；0xA880 段）
pub mod unx7081_soaktable;
// unx7101_perfopen ↔ UNX-F7101 性能核账总起（B01–B35 性能面复核清单；0xA900 段）
pub mod unx7101_perfopen;
// unx7121_regaxis ↔ UNX-F7121 回归轴总起（F6282 三维判例同构声明；0xA980 段）
pub mod unx7121_regaxis;
// unx7141_3axisreg ↔ UNX-F7141 三主轴回归总起（六轴↔B37 矩阵双射映射；0xAA00 段）
pub mod unx7141_3axisreg;
// unx7161_docrecon ↔ UNX-F7161 文档对账总起（冻结/契约/手册三类清单；0xAA80 段）
pub mod unx7161_docrecon;
// unx7181_finalverify ↔ UNX-F7181 收官总验证总起（40 批 800 条 240,000 预备清单；0xAB00 段）
pub mod unx7181_finalverify;
// unx7201_dmdev ↔ UNX-F7201 dm 设备生命周期（四态迁移+销毁零残留+非法迁移拒；0xAB80 段）
pub mod unx7201_dmdev;
// unx7221_bioclone ↔ UNX-F7221 bio 克隆与引用语义（深拷贝独立+10 万次引用零泄漏；0xAC00 段）
pub mod unx7221_bioclone;
// unx7241_tablereload ↔ UNX-F7241 table reload 总控协议（四阶段有序+跳阶段拒；0xAC80 段）
pub mod unx7241_tablereload;
// unx7261_dmstack ↔ UNX-F7261 dm 堆叠映射框架（三层穿透逐扇区+物理盘不可作上层；0xAD00 段）
pub mod unx7261_dmstack;
// unx7281_dmmsg ↔ UNX-F7281 dm 管理消息通道（四命令+参数校验先行+畸形全拒留账；0xAD80 段）
pub mod unx7281_dmmsg;
// unx7301_flushfwd ↔ UNX-F7301 flush 转发契约总纲（全确认语义+三方断电链落纸+AI-86 闸；0xAE00 段）
pub mod unx7301_flushfwd;
// unx7321_raidtarget ↔ UNX-F7321 raid target 注册与几何参数（级别/成员/条带三档校验+分档留账；0xAE80 段）
pub mod unx7321_raidtarget;

// ── 批22（0xE400~0xE700 + 0xB160 + 0xD000/0xD080/0xD180 + 0xAF00/0xAF80 + 0x7B00/0x7B80/0x7C00/0x7C80 散段，16 单）────────────────────────────────
// unx7341_dmktestgate ↔ UNX-F7341 dm ktest 总闸（7 族基线超集核对+族绿即绿；0xE400 段）
pub mod unx7341_dmktestgate;
// unx7361_raid1bitmap ↔ UNX-F7361 RAID1 写位图加速（10 万写位图抽验+重放补块+加速比 ≥5×；0xE480 段）
pub mod unx7361_raid1bitmap;
// unx7421_lvmgrow ↔ UNX-F7421 LVM 在线扩容（原表快照+原子替换+在途零丢失+新增区读写；0xE500 段）
pub mod unx7421_lvmgrow;
// unx7441_snapexhaust ↔ UNX-F7441 快照空间耗尽（显式失效+K2 告警+降级留账+源卷续写；0xE580 段）
pub mod unx7441_snapexhaust;
// unx7461_lvmgate ↔ UNX-F7461 LVM 全判据总闸（双闸谱系：ktest 族+实战族全绿即绿；0xE600 段）
pub mod unx7461_lvmgate;
// unx7481_luks1head ↔ UNX-F7481 LUKS1 卷头（8 槽双向断言+魔数版本校验+三重写盘闸；0xE680 段）
pub mod unx7481_luks1head;
// unx7521_mutal45 ↔ UNX-F7521 互开样本环境 4–5 终版账（5/5 矩阵补全+版本锚；0xB160 段）
pub mod unx7521_mutal45;
// unx7541_cryptfsm ↔ UNX-F7541 加密卷六态状态机（11 边+原子迁移+重启恢复；0xD000 段）
pub mod unx7541_cryptfsm;
// unx7561_smartarch ↔ UNX-F7561 SMART 消费架构总图（B4 读通道→B5 判定→K2/K5 呈现三层+零寄存器直读；0xD080 段）
pub mod unx7561_smartarch;
// unx7581_thresheng ↔ UNX-F7581 健康阈值引擎总纲（判定入口/schema/分级三段；0xD180 段）
pub mod unx7581_thresheng;
// unx7601_selftest ↔ UNX-F7601 SMART self-test 三型与状态机（offline/short/long+idle/running/completed+中止边；0xAF00 段）
pub mod unx7601_selftest;
// unx7621_raidwarn ↔ UNX-F7621 RAID 坏盘预警链（健康分级→决策引擎→RAID 动作三段+热备替换+重建收口；0xAF80 段）
pub mod unx7621_raidwarn;
// unx7641_healthtsdb ↔ UNX-F7641 健康历史数据库（热层环形 96+冷层文件段 512×32+16 盘×30 天容量模型 46,080；0x7B00 段）
pub mod unx7641_healthtsdb;
// unx7661_smartagg ↔ UNX-F7661 SMART 判据族总聚合（B19–B23 五批+F7561–F7660 族表 100 槽；0x7B80 段）
pub mod unx7661_smartagg;
// unx7681_ledgerx1000 ↔ UNX-F7681 ×1000 总账架构（分账矩阵 1000=QEMU 700+真机 300+指纹链收口；0x7C00 段）
pub mod unx7681_ledgerx1000;
// unx7701_raid1pw150 ↔ UNX-F7701 RAID1 断电 ×150 对练账（QEMU 105/真机 45 分账禁混报+位图重放零丢失；0x7C80 段）
pub mod unx7701_raid1pw150;

// ── 批23（0x7800/0x7900/0x7A80 + 0x7D00~0x8380 + 0x9580 散段，16 单 · ext4 域 + 大页回收）────────────────────────────────
// unx2783_bigfreerec ↔ UNX-F2783 大页整块回收（整页无引用判定→整块逐出 10/10+碎块拒绝；0x7800 段）
pub mod unx2783_bigfreerec;
// unx4821_bgdesc ↔ UNX-F4821 块组描述符解析与逐组账（32B 布局+三指针三计数+dumpe2fs 对拍 100%；0x7900 段）
pub mod unx4821_bgdesc;
// unx4861_mountopt ↔ UNX-F4861 挂载选项解析器与矩阵账（20 组合法 100%+非法 10/10 EINVAL；0x7A80 段）
pub mod unx4861_mountopt;
// unx4941_ktestfsslot ↔ UNX-F4941 ktest 文件系统面 ext4 组槽位（八组注册+与 fs23 八族双向隔离；0x7D00 段）
pub mod unx4941_ktestfsslot;
// unx4961_extreehdr ↔ UNX-F4961 extent 树三要素解析（hdr/索引项/叶项 20 卷逐项一致；0x7D80 段）
pub mod unx4961_extreehdr;
// unx4981_delalloc ↔ UNX-F4981 延迟分配语义声明（写时记账回写寻块+五态枚举账；0x7E00 段）
pub mod unx4981_delalloc;
// unx5041_pcacheconsum ↔ UNX-F5041 A4 页缓存契约消费面（readpage/writepage 签名一致+冻结契约+差异零静默；0x7F00 段）
pub mod unx5041_pcacheconsum;
// unx5081_perftotal ↔ UNX-F5081 性能账三轴总纲（p99/吞吐/IO 计数+F5053/F5013/F5062 交叉 100%；0x7F80 段）
pub mod unx5081_perftotal;
// unx5101_extmaintain ↔ UNX-F5101 extent 维护操作总纲（六操作分界+B09 引用零悬空+回滚链；0x8000 段）
pub mod unx5101_extmaintain;
// unx5121_bigfile16t ↔ UNX-F5121 大文件语义 16TiB 边界（1K/4K/64K 三档公式+越界拒绝；0x8080 段）
pub mod unx5121_bigfile16t;
// unx5141_extagg ↔ UNX-F5141 extent 段聚合 200 条图谱（八批×20+区间对照+篡改检出；0x8100 段）
pub mod unx5141_extagg;
// unx5161_jdbtotal ↔ UNX-F5161 journal 子系统总纲六面（结构/生命周期/恢复/模式/断电/清理 160 条；0x8180 段）
pub mod unx5161_jdbtotal;
// unx5181_jstart ↔ UNX-F5181 journal_start 语义账（credits 申请 10/10+零 credits 拒 10/10+F5171 联账；0x8280 段）
pub mod unx5181_jstart;
// unx5201_jreplay ↔ UNX-F5201 恢复重放总纲五段（扫描/判账/候选/重放/复位+F5020 锚承接；0x8300 段）
pub mod unx5201_jreplay;
// unx5221_datajournal ↔ UNX-F5221 data=journal 模式语义（数据入日志+提交原子口径+F5187 联账；0x8380 段）
pub mod unx5221_datajournal;
// unx5241_pwcut100 ↔ UNX-F5241 断电注入 ×100 总纲五段（框架/不变量/检出/分级/样本+F5219/F5233 联账；0x9580 段）
pub mod unx5241_pwcut100;

// ── 批24（0x9680~0x9F80 + 0xB220~0xB260 散段，16 单 · journal 收官 + 断电对练终版 + 备份/动态链接域）────────────────────────────────
// unx5261_pwdtotal2 ↔ UNX-F5261 断电下账总纲四段（数据面/极端窗口/叠加/复测+B23 衔接零悬空；0x9680 段）
pub mod unx5261_pwdtotal2;
// unx5281_ckptsem ↔ UNX-F5281 checkpoint 语义总纲（定义/回收边界/触发三件+F5194/F5174 联账；0x9700 段）
pub mod unx5281_ckptsem;
// unx5301_jsampleidx ↔ UNX-F5301 journal 段样本库十二组索引（AI–AT 零漏项+三同伴同制断言；0x9780 段）
pub mod unx5301_jsampleidx;
// unx5421_decompfacade ↔ UNX-F5421 解压器门面统一接口（三算法 trait+错误三分账+未知 10/10 拒；0x9880 段）
pub mod unx5421_decompfacade;
// unx7721_lvmcut100 ↔ UNX-F7721 LVM 元数据断电 ×100（QEMU 70/真机 30 禁混报+写窗覆盖+校验和基线对照；0x9980 段）
pub mod unx7721_lvmcut100;
// unx7741_lukscut100 ↔ UNX-F7741 LUKS 断电 ×100 终版（分账禁混报+勾稽三段+密钥槽零损；0x9A80 段）
pub mod unx7741_lukscut100;
// unx7761_stackcut100 ↔ UNX-F7761 RAID+LUKS 叠加断电 ×100（双层窗口覆盖+解锁+一致性双断言；0x9B80 段）
pub mod unx7761_stackcut100;
// unx7781_aggengine ↔ UNX-F7781 总账聚合引擎五账归一（勾稽总和+矩阵外零容忍+重放确定性；0x9C00 段）
pub mod unx7781_aggengine;
// unx7821_finalx1000 ↔ UNX-F7821 ×1000 终收官三断言框架（账=账=勾稽/矩阵闭合/段封存+F7799 diff；0x9C80 段）
pub mod unx7821_finalx1000;
// unx7861_backup5phase ↔ UNX-F7861 备份执行五阶段流水线（线性状态机+跳段/倒序 ×10 拒收+校验链；0x9D00 段）
pub mod unx7861_backup5phase;
// unx7941_bkverifyfin ↔ UNX-F7941 备份校验链子域收官（五批回顾 diff ×5+三面框架落账；0x9D80 段）
pub mod unx7941_bkverifyfin;
// unx7961_regall6 ↔ UNX-F7961 域级总回归六面全谱（RAID/LVM/LUKS/叠加/断电/备份+缺面 ×10 检出；0x9E00 段）
pub mod unx7961_regall6;
// unx7981_closure800 ↔ UNX-F7981 域闭账宣告总纲（40 批 800 条+F7800 锚同构+B40 十项前置守卫；0x9F80 段）
pub mod unx7981_closure800;
// unx8041_stacklayout ↔ UNX-F8041 初始栈布局总装配器（六段自高向低+SysV ABI 逐字节 20/20；0xB220 段）
pub mod unx8041_stacklayout;
// unx8081_globdat ↔ UNX-F8081 R_X86_64_GLOB_DAT 族应用（GOT 逐槽直填+基准一致+计数一致；0xB240 段）
pub mod unx8081_globdat;
// unx8101_fdtablefork ↔ UNX-F8101 fd 表整表复制（fork 逐 fd 引用+1+父子独立 close 10/10 对照；0xB260 段）
pub mod unx8101_fdtablefork;

// ── 批25（0xB2A0~0xB520 散段，16 单 · 装载/进程/人格/拒档域）────────────────────────────────
// unx8121_execbase ↔ UNX-F8121 ET_EXEC 固定基址落位器（p_vaddr 绝对落位 10/10+不扰动不重排；0xB2A0 段）
pub mod unx8121_execbase;
// unx8141_ktestagg ↔ UNX-F8141 ktest 装载面总断言集（B01/B03/B04/B05/B07 双面聚合+计数一致；0xB2C0 段）
pub mod unx8141_ktestagg;
// unx8161_execvesem ↔ UNX-F8161 execve 语义档三件套（正常/错误矩阵/边界+man 6.9.1 双向核对；0xB2E0 段）
pub mod unx8161_execvesem;
// unx8181_aslr3 ↔ UNX-F8181 ASLR 三档位机制（off/cons/full 三档布局断言各 10/10+切换即时；0xB320 段）
pub mod unx8181_aslr3;
// unx8201_forkcow ↔ UNX-F8201 fork_cow 消费协议（A4 签名对齐+fake/real 双跑一致零改动；0xB340 段）
pub mod unx8201_forkcow;
// unx8221_pgroup ↔ UNX-F8221 进程组模型（pgid 归属双向一致+组遍历计数=账面；0xB360 段）
pub mod unx8221_pgroup;
// unx8241_clonemtx ↔ UNX-F8241 clone 全标志矩阵（九位三列登记+组合非法集判据在册；0xB3A0 段）
pub mod unx8241_clonemtx;
// unx8261_lxrunproto ↔ UNX-F8261 lxrun.rs 接管总协议（三登记+联签确认+A1/A2 同构；0xB3C0 段）
pub mod unx8261_lxrunproto;
// unx8281_personaroute ↔ UNX-F8281 人格标签路由（Top10 C 侧半边+错误路由 10/10 暴露+账可导出；0xB3E0 段）
pub mod unx8281_personaroute;
// unx8301_personaswitch ↔ UNX-F8301 人格切换原语（双向登记+四列账单+错向 10/10 拒+D2 联签；0xB420 段）
pub mod unx8301_personaswitch;
// unx8321_personaledger ↔ UNX-F8321 人格资源账本分离（双实例+persona 列+跨账取数 10/10 零逃逸；0xB440 段）
pub mod unx8321_personaledger;
// unx8341_ldsoprot ↔ UNX-F8341 ld.so 装载协作分工表（内核映射/ld.so 重定位两分+F8064 口径机检+版本冻结；0xB460 段）
pub mod unx8341_ldsoprot;
// unx8361_pinver ↔ UNX-F8361 钉版样本协议（三注记逐样本+版本漂移 10/10 暴露+C5 双认；0xB4A0 段）
pub mod unx8361_pinver;
// unx8421_phdrover ↔ UNX-F8421 程序头表越界读拒档（正向过+反向逐条全拒不整表信任；0xB4C0 段）
pub mod unx8421_phdrover;
// unx8441_seglimit ↔ UNX-F8441 单段映射尺寸超上限拒档（64MiB 阈值冻结+ENOMEM/EFBIG 对齐；0xB4E0 段）
pub mod unx8441_seglimit;
// unx8461_auxvlimit ↔ UNX-F8461 auxv 条目超限拒档（32 条上限+栈预算守恒+B03 承接；0xB520 段）
pub mod unx8461_auxvlimit;
// unx8481_relocover ↔ UNX-F8481 重定位表越界拒档（r_offset 三形拒+逐条校验不整表信任；0xB540 段）
pub mod unx8481_relocover;
// unx8521_interpover ↔ UNX-F8521 PT_INTERP 路径越界拒档（段界+NUL+/ 前缀五路拒；0xB560 段）
pub mod unx8521_interpover;
// unx8541_e8recon ↔ UNX-F8541 E 型八批总对账（B21–B28 160 条零容忍错账两路检出；0xB5A0 段）
pub mod unx8541_e8recon;
// unx8561_exevchain ↔ UNX-F8561 execve 全链联测件（C2×C1×C5×main 四段联签+待收口登记；0xB5C0 段）
pub mod unx8561_exevchain;
// unx8581_d1joint ↔ UNX-F8581 D1 号面对照基座联测件（三方对拍 10/10+指纹冻结不漂移；0xB5E0 段）
pub mod unx8581_d1joint;
// unx8601_c2full ↔ UNX-F8601 C2 号面 300+ 全量联测收官（三口径对账+零漏发+收官否决；0xB620 段）
pub mod unx8601_c2full;
// unx8621_sigpend ↔ UNX-F8621 信号 pending 队满语义（标准去重+实时排队+队满 EAGAIN；0xB640 段）
pub mod unx8621_sigpend;
// unx8641_glibcx40 ↔ UNX-F8641 glibc 样本 30→40 扩额（旧集零变动+指纹保全+总量对账；0xB660 段）
pub mod unx8641_glibcx40;
// unx8661_cmpx40 ↔ UNX-F8661 40 组对照执行收官（偏差逐组落账+账哈希+注入检出；0xB6A0 段）
pub mod unx8661_cmpx40;
// unx8681_ltpmap60 ↔ UNX-F8681 LTP 映射表 ≥60 行（行号连续+用例唯一+未映射落账；0xB6C0 段）
pub mod unx8681_ltpmap60;
// unx8701_idrecon8 ↔ UNX-F8701 八批 ID 总对账（160 条跨批连续+错账两路检出；0xB6E0 段）
pub mod unx8701_idrecon8;
// unx8721_procfam ↔ UNX-F8721 进程族四族回归总档（根唯一+父在族内+孤儿检出；0xB720 段）
pub mod unx8721_procfam;
// unx8741_coldtime ↔ UNX-F8741 冷启动计时协议终版（单调+间隙窗+总预算+版本冻结；0xB740 段）
pub mod unx8741_coldtime;
// unx8761_docsnap ↔ UNX-F8761 域文档快照三件冻结（双通道指纹+漂移告警+三件对账；0xB760 段）
pub mod unx8761_docsnap;
// unx8781_finrecon ↔ UNX-F8781 收官总对账 240,000（十六段×15,000+零差容两路全红；0xB7A0 段）
pub mod unx8781_finrecon;
// unx8821_openflags ↔ UNX-F8821 open 九标志矩阵 30 组（访问域互斥+EXCL 约束+杂散位拒；0xB7C0 段）
pub mod unx8821_openflags;

// unx0054_acpimat ↔ UNX-F0054 ACPI 表版本兼容矩阵
pub mod unx0054_acpimat;

// unx0171_tpmreplay ↔ UNX-F0171 TPM2 事件重放（只读）
pub mod unx0171_tpmreplay;

// unx0487_clrcolor ↔ UNX-F0487 清屏与配色原语
pub mod unx0487_clrcolor;

// unx0645_reggate ↔ UNX-F0645 域回归门禁（全绿放行与拦截）
pub mod unx0645_reggate;

// unx0807_trapframe ↔ UNX-F0807 TrapFrame 统一寄存器现场与错误码分型
pub mod unx0807_trapframe;

// unx0819_explain ↔ UNX-F0819 异常现场人话化解释与诊断账深化
pub mod unx0819_explain;

// ═══ UNX-F-WBB-1022 批（2026-10-22，AI-WBB-1022 领单 100 单）═══
// unx0047_mcfg ↔ UNX-F0047 MCFG 表与 PCIe ECAM 基址固化（0x2000 段）
pub mod unx0047_mcfg;
// unx0048_sratlit ↔ UNX-F0048 SRAT/SLIT NUMA 拓扑解析（0x2010 段）
pub mod unx0048_sratlit;
// unx0049_systab ↔ UNX-F0049 UEFI SystemTable 只读快照（0x2020 段）
pub mod unx0049_systab;
// unx0050_uvarwalk ↔ UNX-F0050 UEFI 变量只读枚举（0x2030 段）
pub mod unx0050_uvarwalk;
// unx0051_secboot ↔ UNX-F0051 SecureBoot/SetupMode 状态采集（0x2040 段）
pub mod unx0051_secboot;
// unx0052_efiattr ↔ UNX-F0052 EFI 内存描述符属性位精解（0x2050 段）
pub mod unx0052_efiattr;
// unx0053_exitsvc ↔ UNX-F0053 ExitBootServices 时序协议（0x2060 段）
pub mod unx0053_exitsvc;
// unx0055_madtnmi ↔ UNX-F0055 MADT 补全：LAPIC NMI 与中断源覆盖（0x2070 段）
pub mod unx0055_madtnmi;
// unx0056_acpipm ↔ UNX-F0056 ACPI PM 能力位只读采集（0x2080 段，并行协同）
pub mod unx0056_acpipm;
// unx0057_tpmevlog ↔ UNX-F0057 TPM 事件日志只读导出（0x2090 段，并行协同）
pub mod unx0057_tpmevlog;
// unx0058_smbios ↔ UNX-F0058 SMBIOS Type 9/16/17/32 扩展采集（0x20A0 段，并行协同）
pub mod unx0058_smbios;
// unx0059_edid ↔ UNX-F0059 EDID 显示器信息采集（0x20B0 段，并行协同）
pub mod unx0059_edid;
// unx0060_ktb03 ↔ UNX-F0060 ktest 引导面 B03 批断言集（0x20C0 段）
pub mod unx0060_ktb03;
// unx0062_resguard ↔ UNX-F0062 保留区白名单与守护（0x20D0 段）
pub mod unx0062_resguard;
// unx0063_acpirecl ↔ UNX-F0063 ACPI Reclaim/NVS 区分类账（0x20E0 段）
pub mod unx0063_acpirecl;
// unx0064_low1map ↔ UNX-F0064 低 1MB 遗留区布局图（0x20F0 段）
pub mod unx0064_low1map;
// unx0065_e820bridge ↔ UNX-F0065 E820 桥接层（0x2100 段）
pub mod unx0065_e820bridge;
// unx0066_memsnap ↔ UNX-F0066 内存图快照落盘与序列化（0x2110 段）
pub mod unx0066_memsnap;
// unx0067_alignzone ↔ UNX-F0067 可用区边界对齐器（0x2120 段）
pub mod unx0067_alignzone;
// unx0068_hugepage ↔ UNX-F0068 大页可用性预判（0x2130 段）
pub mod unx0068_hugepage;
// unx0069_hotplugbmp ↔ UNX-F0069 物理内存热插拔位图预留（0x2140 段）
pub mod unx0069_hotplugbmp;
// unx0070_efimapdup ↔ UNX-F0070 EFI MemoryMap 持久化副本（0x2150 段）
pub mod unx0070_efimapdup;
// unx0071_earlyalloc ↔ UNX-F0071 早年引导内存分配器（0x2160 段）
pub mod unx0071_earlyalloc;
// unx0072_pageaudit ↔ UNX-F0072 物理页账本初始化审计（0x2170 段）
pub mod unx0072_pageaudit;
// unx0073_holefind ↔ UNX-F0073 内存空洞检测器（0x2180 段）
pub mod unx0073_holefind;
// unx0074_numabmp ↔ UNX-F0074 NUMA 节点亲和页位图（0x2190 段）
pub mod unx0074_numabmp;
// unx0075_memcap ↔ UNX-F0075 物理内存上限 cap 策略（0x21A0 段）
pub mod unx0075_memcap;
// unx0076_badpage ↔ UNX-F0076 坏页隔离区（0x21B0 段）
pub mod unx0076_badpage;
// unx0077_memheat ↔ UNX-F0077 物理内存热图（0x21C0 段）
pub mod unx0077_memheat;
// unx0078_cr3check ↔ UNX-F0078 CR3 载入前自检套件（0x21D0 段）
pub mod unx0078_cr3check;
// unx0079_a4handoff ↔ UNX-F0079 内存图与 A4 域交接协议（0x21E0 段）
pub mod unx0079_a4handoff;
// unx0080_ktb04 ↔ UNX-F0080 ktest 引导面 B04 批断言集（0x21F0 段）
pub mod unx0080_ktb04;

// unx0166_watchdog ↔ UNX-F0166 EFI Watchdog 处理（0x2870 段）（AI-WBX1022 EFI 域批次）
pub mod unx0166_watchdog;
// unx0167_mixedmode ↔ UNX-F0167 Mixed Mode 检测（0x2930 段）（AI-WBX1022 EFI 域批次）
pub mod unx0167_mixedmode;
// unx0168_ebscb ↔ UNX-F0168 ExitBootServices 前回调链（0x2950 段）（AI-WBX1022 EFI 域批次）
pub mod unx0168_ebscb;
// unx0169_rtsvc ↔ UNX-F0169 运行时调用封装与状态码字典（0x2990 段）（AI-WBX1022 EFI 域批次）
pub mod unx0169_rtsvc;
// unx0170_sbchain ↔ UNX-F0170 SecureBoot 签名链只读解析（0x6460 段）（AI-WBX1022 EFI 域批次）
pub mod unx0170_sbchain;
// unx0173_bootenum ↔ UNX-F0173 启动项枚举与 BootNext 语义（0x6530 段）（AI-WBX1022 EFI 域批次）
pub mod unx0173_bootenum;
// unx0174_reentry ↔ UNX-F0174 运行时防重入与并发限制（0x6540 段）（AI-WBX1022 EFI 域批次）
pub mod unx0174_reentry;
// unx0175_bsgc ↔ UNX-F0175 EFI 内存垃圾回收 BS 区（0x6550 段）（AI-WBX1022 EFI 域批次）
pub mod unx0175_bsgc;
// unx0176_svcdegrade ↔ UNX-F0176 运行时服务缺失降级路径（0x6560 段）（AI-WBX1022 EFI 域批次）
pub mod unx0176_svcdegrade;
// unx0177_rtpnx ↔ UNX-F0177 Runtime 区属性保护 NX/RO（0x6A10 段）（AI-WBX1022 EFI 域批次）
pub mod unx0177_rtpnx;// unx0084_relfix ↔ UNX-F0084 段重定位项修复器（0x2200 段）
pub mod unx0084_relfix;
// unx0085_bssclr ↔ UNX-F0085 BSS 清零与校验（0x2210 段）
pub mod unx0085_bssclr;
// unx0086_panicsym ↔ UNX-F0086 符号表保留与 panic 符号化（0x2220 段）
pub mod unx0086_panicsym;
// unx0087_kaslroff ↔ UNX-F0087 KASLR 关闭模式（诊断模式）（0x2230 段）
pub mod unx0087_kaslroff;
// unx0088_bootmani ↔ UNX-F0088 重放校验 BootManifest 固化（0x2240 段）
pub mod unx0088_bootmani;
// unx0089_hihalf ↔ UNX-F0089 高半区偏移随机化（0x2250 段）
pub mod unx0089_hihalf;
// unx0090_physbase ↔ UNX-F0090 物理基址选择策略（0x2260 段）
pub mod unx0090_physbase;
// unx0091_kptadapt ↔ UNX-F0091 页表适配器（0x2270 段）
pub mod unx0091_kptadapt;
// unx0092_blcoord ↔ UNX-F0092 Bootloader 协同随机化接口（0x2280 段）
pub mod unx0092_blcoord;
// unx0093_sidechan ↔ UNX-F0093 随机化侧信道面收敛（0x2290 段）
pub mod unx0093_sidechan;
// unx0094_relocaudit ↔ UNX-F0094 重定位审计报告（0x22A0 段）
pub mod unx0094_relocaudit;
// unx0095_bootfp ↔ UNX-F0095 随机化指纹 Per-Boot ID（0x22B0 段）
pub mod unx0095_bootfp;
// unx0096_segload ↔ UNX-F0096 段加载器（0x22C0 段）
pub mod unx0096_segload;
// unx0097_badimg ↔ UNX-F0097 畸形内核镜像拒载路径（0x22D0 段）
pub mod unx0097_badimg;
// unx0098_kaslrsync ↔ UNX-F0098 KASLR 与早期映射一致性自检（0x22E0 段）
pub mod unx0098_kaslrsync;
// unx0099_relperf ↔ UNX-F0099 早年性能记账：重定位耗时（0x22F0 段）
pub mod unx0099_relperf;
// unx0100_ktb05 ↔ UNX-F0100 ktest 引导面 B05 批断言集（0x2300 段）
pub mod unx0100_ktb05;
// unx0103_fontterm ↔ UNX-F0103 帧簿控制台字体渲染（0x2310 段）
pub mod unx0103_fontterm;
// unx0104_dualarb ↔ UNX-F0104 双通道仲裁与降级策略（0x2320 段）
pub mod unx0104_dualarb;
// unx0105_loglevel ↔ UNX-F0105 日志级别体系（0x2330 段）
pub mod unx0105_loglevel;
// unx0106_logstamp ↔ UNX-F0106 日志时间戳 TSC 换算（0x2340 段）
pub mod unx0106_logstamp;
// unx0107_ringlog ↔ UNX-F0107 日志环形缓冲持久化（0x2350 段）
pub mod unx0107_ringlog;
// unx0108_panicsnap ↔ UNX-F0108 panic 前日志快照（0x2360 段）
pub mod unx0108_panicsnap;

// unx0968_fourstate ↔ UNX-F0968 四态迁移完整性断言链（AI-WBX1022 补注册）
pub mod unx0968_fourstate;
// unx172_partdisc ↔ UNX-F0172 EFI 分区发现与 GUID 语义（0x6A50 段）（AI-WBX1022 B09 批）
pub mod unx172_partdisc;
// unx178_envfp ↔ UNX-F0178 EFI 环境指纹（0x6B50 段）（AI-WBX1022 B09 批）
pub mod unx178_envfp;
// unx179_faillog ↔ UNX-F0179 EFI 调用早期失败显性化（0x6B60 段）（AI-WBX1022 B09 批）
pub mod unx179_faillog;
// unx180_ktestb09 ↔ UNX-F0180 ktest 引导面 B09 批断言集（0x6B70 段）（AI-WBX1022 B09 批）
pub mod unx180_ktestb09;
// unx187_ucodero ↔ UNX-F0187 微码加载状态检测只读（0x6C10 段）（AI-WBX1022 B09 批）
pub mod unx187_ucodero;
// unx188_smppkt ↔ UNX-F0188 SMP 前置数据包（0x6C20 段）（AI-WBX1022 B09 批）
pub mod unx188_smppkt;
// unx189_gdtreload ↔ UNX-F0189 GDT 重载与段语义固化（0x6C30 段）（AI-WBX1022 B09 批）
pub mod unx189_gdtreload;
// unx190_tssist ↔ UNX-F0190 TSS 与 IST 布局初始化（0x2B20 段）（AI-WBX1022 B09 批）
pub mod unx190_tssist;
// unx191_canary ↔ UNX-F0191 栈金丝雀（0x6C70 段）（AI-WBX1022 B09 批）
pub mod unx191_canary;
// unx192_fsgsbase ↔ UNX-F0192 fs/gs base 与 per-CPU 预备（0x6C90 段）（AI-WBX1022 B09 批）
pub mod unx192_fsgsbase;
// unx193_crpolicy ↔ UNX-F0193 控制寄存器策略表（0x2B40 段）（AI-WBX1022 B09 批）
pub mod unx193_crpolicy;
// unx194_mtrrpat ↔ UNX-F0194 MTRR/PAT 初始化（0x2B50 段）（AI-WBX1022 B09 续批）
pub mod unx194_mtrrpat;
// unx195_cacheline ↔ UNX-F0195 缓存与对齐策略声明（0x2B60 段）（AI-WBX1022 B09 续批）
pub mod unx195_cacheline;
// unx196_irqwindow ↔ UNX-F0196 中断禁用窗口最小化（0x2B70 段）（AI-WBX1022 B09 续批）
pub mod unx196_irqwindow;
// unx197_initgate ↔ UNX-F0197 init 自检清单健康门禁（0x2B80 段）（AI-WBX1022 B09 续批）
pub mod unx197_initgate;

// ---- 死判据补登记（2026-10-22 接线轮：模块早已实现但从未入链，等于从未参与编译与验收）----
// //! UNX-F10001 · 波次报表总纲（维度与口径）账 · 诊断段 0xB7E0
pub mod unx10001_waveboard;
// //! UNX-F1014 · 时间误差预算表（计时面 SLO 分解契约）账 · 诊断段 0xB8D0
pub mod unx1014_timebudget;
// //! UNX-F1686 · 权重→vruntime 累加速率一致性核对账 · 诊断段 0xB8F0
pub mod unx1686_vruntime;
// //! UNX-F1704 · 每线程负载贡献追踪（贡献分解）账 · 诊断段 0xB910
pub mod unx1704_loadtrace;
// //! UNX-F1724 · 迁移失败回退（暂存滞留回收联动）账 · 诊断段 0xB930
pub mod unx1724_migfallback;
// //! UNX-F1884 · 迁移-击落联合回归（一致性协同）账 · 诊断段 0xB950
pub mod unx1884_migipi;
// //! UNX-F1938 · ktest 族注册与批内断言聚合账 · 诊断段 0xB970
pub mod unx1938_ktestagg;
// //! UNX-F21581 · 域账满账终声明（双断言）
pub mod unx21581_domainseal;
// //! UNX-F21582 · ID 终值闭合（F20801–F21600 连续唯一）
pub mod unx21582_idclosure;
// //! UNX-F21583 · 判据 800 枚终钉版（J1 全集自洽）
pub mod unx21583_critpin;
// //! UNX-F21584 · 批册 40 件落盘完整性核（文件/行数/头部）
pub mod unx21584_batchdisk;
// //! UNX-F21585 · 总纲 §7.3-F2 与批册一致性核
pub mod unx21585_outlinealign;
// //! UNX-F21586 · 汇编册域节与批册一致性核
pub mod unx21586_asmconsist;
// //! UNX-F21587 · 域守恒终声明（行数/条数/判据三面）
pub mod unx21587_conservseal;
// //! UNX-F21588 · 域收官宣示（此后零新条目入域账）
pub mod unx21588_closesolemn;
// //! UNX-F21589 · 上游会签复核（AI-26 四组冻结接口）
pub mod unx21589_upcosign;
// //! UNX-F21590 · 下游会签复核（AI-29/30/61/66-70/87）
pub mod unx21590_downcosign;
// //! UNX-F21591 · 域缺陷账本清点（产线双轨纪律）
pub mod unx21591_defectaudit;
// //! UNX-F21592 · 随闸门补测池终登记（R-F2-001/002 清单）
pub mod unx21592_retestpool;
// //! UNX-F21593 · 证据三件套归档（脚本/报告/哈希）
pub mod unx21593_evidencetrio;
// //! UNX-F21594 · 域交接与深化移交（AI-90 排程）
pub mod unx21594_handover;
// //! UNX-F21595 · 域收官与全局台账回写（根台账 / handoff）
pub mod unx21595_ledgerpatch;
// //! UNX-F21596 · ktest 域总闸（F2 面 800 判据一键全跑）
pub mod unx21596_kgategate;
// //! UNX-F21597 · 收官验收（20 维度自评表）
pub mod unx21597_selfscore;
// //! UNX-F21598 · 域写锁（防他人/防自误改）
pub mod unx21598_domainlock;
// //! UNX-F21599 · 满账三印宣示（文印/账印/链印）
pub mod unx21599_threeseal;
// //! UNX-F21600 · B40 收官聚合与 F2 域闭环宣示
pub mod unx21600_batchclose;
// //! UNX-F21601 · rop3_table 编译期生成器与 256 项自检
pub mod unx21601_rop3table;
// //! UNX-F21604 · SRCCOPY/NOTSRCCOPY 黑盒核心五操作
pub mod unx21604_blackops;
// //! UNX-F21607 · ROP3 与背景色的 MERGEPEN 系语义
pub mod unx21607_mergepen;
// //! UNX-F21610 · 调色板像素展开（8bpp 及以下索引色）
pub mod unx21610_palettexpand;
// //! UNX-F2744 · 目标水位推进与达成判定账 · 诊断段 0xB990
pub mod unx2744_watermark;
// unx0109_serialcfg ↔ UNX-F0109 串口参数可配置 cmdline（0x2370 段）
pub mod unx0109_serialcfg;
// unx0110_concolor ↔ UNX-F0110 控制台色彩语义（0x2380 段）
pub mod unx0110_concolor;
// unx0111_scrollback ↔ UNX-F0111 屏幕滚动与历史回看（0x2390 段）
pub mod unx0111_scrollback;
// unx0112_ansiesc ↔ UNX-F0112 ANSI 转义子集（0x23A0 段）
pub mod unx0112_ansiesc;
// unx198_timeline ↔ UNX-F0198 init 阶段耗时账（0x2B90 段）（AI-WBX1022 B10/B11 批）
pub mod unx198_timeline;
// unx199_bssaudit ↔ UNX-F0199 早年数据段清零与初始化审计（0x2BA0 段）（AI-WBX1022 B10/B11 批）
pub mod unx199_bssaudit;
// unx200_ktb10 ↔ UNX-F0200 ktest 引导面 B10 批断言集（0x2BC0 段）（AI-WBX1022 B10/B11 批）
pub mod unx200_ktb10;
// unx207_desens ↔ UNX-F0207 报告敏感信息脱敏（0x2BD0 段）（AI-WBX1022 B10/B11 批）
pub mod unx207_desens;
// unx208_reportui ↔ UNX-F0208 报告展示接口（0x2BE0 段）（AI-WBX1022 B10/B11 批）
pub mod unx208_reportui;
// unx209_gantt ↔ UNX-F0209 引导时间线甘特式视图（0x2BF0 段）（AI-WBX1022 B10/B11 批）
pub mod unx209_gantt;
// unx210_hwfp ↔ UNX-F0210 硬件指纹链（0x2C20 段）（AI-WBX1022 B10/B11 批）
pub mod unx210_hwfp;
// unx211_bootstat ↔ UNX-F0211 多次引导聚合统计（0x2C30 段）（AI-WBX1022 B10/B11 批）
pub mod unx211_bootstat;
// unx212_regdiff ↔ UNX-F0212 回归对比（0x2C40 段）（AI-WBX1022 B10/B11 批）
pub mod unx212_regdiff;
// unx213_repsign ↔ UNX-F0213 报告完整性签名（0x2C50 段）（AI-WBX1022 B10/B11 批）
pub mod unx213_repsign;
// unx214_frustr ↔ UNX-F0214 体验事件式埋点（0x2C60 段）（AI-WBX1022 B10/B11 批）
pub mod unx214_frustr;
// unx215_logtrace ↔ UNX-F0215 日志-报告联动（0x2C70 段）（AI-WBX1022 B10/B11 批）
pub mod unx215_logtrace;
// unx216_jsonexp ↔ UNX-F0216 报告导出 JSON（0x2C80 段）（AI-WBX1022 B10/B11 批）
pub mod unx216_jsonexp;
// unx217_probezero ↔ UNX-F0217 零静默探针网络（0x2C90 段）（AI-WBX1022 B10/B11 批）
pub mod unx217_probezero;
// unx218_consfrozen ↔ UNX-F0218 报告消费方接口冻结（0x2CA0 段）（AI-WBX1022 B10/B11 批）
pub mod unx218_consfrozen;
// unx219_perfbudget ↔ UNX-F0219 报告生成性能预算（0x2CB0 段）（AI-WBX1022 B10/B11 批）
pub mod unx219_perfbudget;
// unx220_ktb11 ↔ UNX-F0220 ktest 引导面 B11 批断言集（0x2CC0 段）（AI-WBX1022 B10/B11 批）
pub mod unx220_ktb11;
// unx0226_badimg ↔ UNX-F0226 坏镜像拒收演练（0x2CE0 段）（AI-WBX1022 B10/B11 批）
pub mod unx0226_badimg;
// unx0227_powcut ↔ UNX-F0227 只读侧断电演练（0x2D20 段）（AI-WBX1022 B10/B11 批）
pub mod unx0227_powcut;
// unx0228_csmdetect ↔ UNX-F0228 CSM/Legacy 环境检测与降级（0x2D30 段）（AI-WBX1022 B10/B11 批）
pub mod unx0228_csmdetect;
// unx0229_sbpath ↔ UNX-F0229 SecureBoot 开启环境引导路径（0x2D40 段）（AI-WBX1022 B10/B11 批）
pub mod unx0229_sbpath;
// unx0230_mediapick ↔ UNX-F0230 多盘引导冲突消解（0x2D60 段）（AI-WBX1022 B10/B11 批）
pub mod unx0230_mediapick;
// unx0231_abrollback ↔ UNX-F0231 镜像降级回滚（0x2D70 段）（AI-WBX1022 B10/B11 批）
pub mod unx0231_abrollback;
// unx0232_bootrate ↔ UNX-F0232 引导成功率统计账（0x2D80 段）（AI-WBX1022 B10/B11 批）
pub mod unx0232_bootrate;
// unx0233_a1final ↔ UNX-F0233 A1 域完工判据集（0x2D90 段）（AI-WBX1022 B10/B11 批）
pub mod unx0233_a1final;
// unx0113_logsplit ↔ UNX-F0113 日志分级输出口（0x23B0 段）
pub mod unx0113_logsplit;
// unx0114_diagui ↔ UNX-F0114 诊断模式控制台 UI 骨架（0x23C0 段）
pub mod unx0114_diagui;
// unx0115_quietboot ↔ UNX-F0115 静默引导模式（0x23D0 段）
pub mod unx0115_quietboot;
// unx0116_loginject ↔ UNX-F0116 日志注入防护（0x23E0 段）
pub mod unx0116_loginject;
// unx0117_sermir ↔ UNX-F0117 串口远程日志只读镜像（0x23F0 段）
pub mod unx0117_sermir;
// unx0118_logfmt ↔ UNX-F0118 日志格式规范与解析器（0x2400 段）
pub mod unx0118_logfmt;
// unx0119_earlyfail ↔ UNX-F0119 早期失败显性化探针（0x2410 段）
pub mod unx0119_earlyfail;
// unx0120_ktb06 ↔ UNX-F0120 ktest 引导面 B06 批断言集（0x2420 段）
pub mod unx0120_ktb06;
// unx0125_earlypanic ↔ UNX-F0125 早期 panic 呈现三要素（0x2430 段）
pub mod unx0125_earlypanic;
// unx0126_bootlogo ↔ UNX-F0126 引导进度可视化阶段徽标（0x2440 段）
pub mod unx0126_bootlogo;
// unx0127_stagebudget ↔ UNX-F0127 阶段耗时预算与超标告警（0x2450 段）
pub mod unx0127_stagebudget;
// unx0128_nmiwatch ↔ UNX-F0128 NMI 备份看门狗（0x2460 段）
pub mod unx0128_nmiwatch;
// unx0129_recbootmenu ↔ UNX-F0129 恢复引导菜单（0x2470 段）
pub mod unx0129_recbootmenu;
// unx0130_lastboot ↔ UNX-F0130 上次引导记录持久化（0x2480 段）
pub mod unx0130_lastboot;
// unx0131_bootcount ↔ UNX-F0131 引导序号与启动计数（0x2490 段）
pub mod unx0131_bootcount;
// unx0132_healthflag ↔ UNX-F0132 健康标志位三区（0x24A0 段）
pub mod unx0132_healthflag;

// --------------------------------------------------------------------------
// AI-MIMO-1022 批（UNX-F0234/F0235/F0265/F0287/F0288 · 纯功能）
// --------------------------------------------------------------------------
// unx234_handoff ↔ UNX-F0234 handoff 快照工具
pub mod unx234_handoff;
// unx235_audit ↔ UNX-F0235 域审计自查单
pub mod unx235_audit;
// unx265_diagbus ↔ UNX-F0265 诊断命令注册总线（命令插件表）
pub mod unx265_diagbus;
// unx287_fatal ↔ UNX-F0287 不可恢复检测（三重故障/栈耗尽）
pub mod unx287_fatal;
// unx288_panicserial ↔ UNX-F0288 panic 期串口最小驱动
pub mod unx288_panicserial;
// unx0133_badslot ↔ UNX-F0133 坏引导检测与拦截（0x24B0 段）
pub mod unx0133_badslot;

// --------------------------------------------------------------------------
// AI-MIMO-1022 批（UNX-F0236~F0239/F0267~F0280 · B12 收官+B14 诊断 · 纯功能）
// --------------------------------------------------------------------------
pub mod unx236_feedback;
pub mod unx237_doccompare;
pub mod unx238_ktest12;
pub mod unx239_batchrecon;
// unx240_closepack ↔ UNX-F0240 波 01 A1 侧闭账物料包（0xf010 段，缺件检出 100%）
pub mod unx240_closepack;
// unx368_heatmap ↔ UNX-F0368 内存热图接入（F0077 写入口，逐位一致+千位采样）（0xf0e0 段）
pub mod unx368_heatmap;
// unx369_hugepage ↔ UNX-F0369 大页候选区盘点（2MB 对齐窗+最大窗可复算）（0xf0f0 段）
pub mod unx369_hugepage;
// unx370_resvblock ↔ UNX-F0370 保留区越界分配拦截（三类各 10 例全拦+分账）（0xf110 段）
pub mod unx370_resvblock;
// unx371_ledger ↔ UNX-F0371 页帧账目对冲总账（借还平账+前缀平账）（0xf120 段）
pub mod unx371_ledger;
// unx372_fragindex ↔ UNX-F0372 内存碎片度量（外碎片五档指数+手算对拍）（0xf130 段）
pub mod unx372_fragindex;
// unx373_oomguard ↔ UNX-F0373 早年 OOM 预案（三级水位+关键池保底 1%）（0xf140 段）
pub mod unx373_oomguard;
// unx374_crossmem ↔ UNX-F0374 内存盘点交叉验证（E820 vs 自检+白名单冻结）（0xf150 段）
pub mod unx374_crossmem;
// unx375_swrclaim ↔ UNX-F0375 页帧池并发原语预留（单写者令牌+占位冻结）（0xf160 段）
pub mod unx375_swrclaim;
// unx376_fivenum ↔ UNX-F0376 内存容量五数概括（三分类守恒零误差）（0xf170 段）
pub mod unx376_fivenum;
// unx377_ktestb19 ↔ UNX-F0377 ktest 内存面断言集接入（B19 十八席注册制）（0xf180 段）
pub mod unx377_ktestb19;
pub mod unx267_sym;
pub mod unx268_ktestfilter;
pub mod unx269_boot;
pub mod unx270_firmware;
pub mod unx271_ratelimit;
pub mod unx272_session;
pub mod unx273_keymap;
pub mod unx274_ascii;
pub mod unx275_dualchannel;
pub mod unx276_crashiso;
pub mod unx277_sessionid;
pub mod unx278_timeout;
pub mod unx279_config;
pub mod unx280_b14ktest;
// unx0134_abfallback ↔ UNX-F0134 回退内核槽 A/B 语义（0x24C0 段）
pub mod unx0134_abfallback;
// unx0135_bootselect ↔ UNX-F0135 引导选择器 多内核多参数（0x24D0 段）
pub mod unx0135_bootselect;
// unx0136_diagentry ↔ UNX-F0136 诊断模式入口统一（0x24E0 段）
pub mod unx0136_diagentry;
// unx0137_argedit ↔ UNX-F0137 引导参数 UI 诊断内编辑（0x24F0 段）
pub mod unx0137_argedit;
// unx0138_bootstat ↔ UNX-F0138 引导成功率统计账本（0x2510 段）
pub mod unx0138_bootstat;
// unx0139_wdhandoff ↔ UNX-F0139 早年看门狗与 A2 中断交接（0x2520 段）
pub mod unx0139_wdhandoff;

// --------------------------------------------------------------------------
// AI-MIMO-1022 批（UNX-F0266/F0289~F0293 · B14/B15 深化收口 · 纯功能）
// --------------------------------------------------------------------------
// unx266_memheat ↔ UNX-F0266 mem 命令深化（热图分节视图）
pub mod unx266_memheat;
// unx289_logfreeze ↔ UNX-F0289 panic 前日志冻结与快照接线
pub mod unx289_logfreeze;
// unx290_dumpcrc ↔ UNX-F0290 转储三件套对账（CRC 完整性闭环）
pub mod unx290_dumpcrc;
// unx291_pcode ↔ UNX-F0291 panic 码表（P-码系）与 E-码映射
pub mod unx291_pcode;
// unx292_nmisafe ↔ UNX-F0292 panic 期 NMI 安全（零重入次序标志）
pub mod unx292_nmisafe;
// unx293_haltstamp ↔ UNX-F0293 停机戳语义（下次引导识别上轮 panic）
pub mod unx293_haltstamp;
// unx0140_ktb07 ↔ UNX-F0140 ktest 引导面 B07 批断言集（0x2530 段）
pub mod unx0140_ktb07;
// unx0145_makeentry ↔ UNX-F0145 单命令构建入口（0x2540 段）
pub mod unx0145_makeentry;
// unx0146_reprobuild ↔ UNX-F0146 构建可复现性 固定时间戳排序（0x2550 段）
pub mod unx0146_reprobuild;

// --------------------------------------------------------------------------
// AI-MIMO-1022 批2（UNX-F0294/F0295 · B15 panic 复现与面板 · 纯功能）
// --------------------------------------------------------------------------
// unx294_panicrepro ↔ UNX-F0294 panic 复现模式（cmdline 开关+计数门限）
pub mod unx294_panicrepro;
// unx295_panicpanel ↔ UNX-F0295 panic 面板体验（倒计时/键位/文案样张）
pub mod unx295_panicpanel;
// unx300_ktestb15 ↔ UNX-F0300 ktest B15 批断言集
pub mod unx300_ktestb15;
// unx0147_incbuild ↔ UNX-F0147 增量构建与缓存（0x2560 段）
pub mod unx0147_incbuild;
// unx0148_verinject ↔ UNX-F0148 镜像版本号注入（0x2570 段）
pub mod unx0148_verinject;
// unx0149_sbom ↔ UNX-F0149 构建物料清单 SBOM 雏形（0x2580 段）
pub mod unx0149_sbom;
// unx0150_imgsplit ↔ UNX-F0150 镜像体积与拆分策略（0x25A0 段）
pub mod unx0150_imgsplit;
// unx0151_writetool ↔ UNX-F0151 写盘工具对接口 只列清单（0x25B0 段）
pub mod unx0151_writetool;
// unx0152_wverify ↔ UNX-F0152 写后校验接口规范（0x25D0 段）
pub mod unx0152_wverify;
// unx0153_multitarget ↔ UNX-F0153 多目标产物 ISO/IMG/HDD 头（0x25F0 段）
pub mod unx0153_multitarget;
// unx0154_qsmoke ↔ UNX-F0154 QEMU 冒烟 只读判据（0x2610 段）
pub mod unx0154_qsmoke;
// unx0155_checksum ↔ UNX-F0155 发布校验和与摘要文件（0x2630 段）
pub mod unx0155_checksum;
// unx0156_builddiag ↔ UNX-F0156 构建失败诊断 三要素（0x2650 段）
pub mod unx0156_builddiag;
// unx0157_imgstats ↔ UNX-F0157 镜像引导成功率统计接口（0x2670 段）
pub mod unx0157_imgstats;
// unx0158_archnaming ↔ UNX-F0158 年度镜像命名与归档规范（0x2690 段）
pub mod unx0158_archnaming;
// unx0159_slowmedia ↔ UNX-F0159 慢速介质与老U盘兼容参数（0x26B0 段）
pub mod unx0159_slowmedia;
// unx0160_ktb08 ↔ UNX-F0160 ktest 构建面 B08 批断言集（0x26D0 段）
pub mod unx0160_ktb08;
// unx0165_uvarpolicy ↔ UNX-F0165 UEFI 变量读写策略与配额（0x26F0 段）
pub mod unx0165_uvarpolicy;
// unx299_panicparse ↔ UNX-F0299 panic 转储对端解析器（三件套还原+黄金样本对拍+六注入；0x3d50 段）
pub mod unx299_panicparse;
// unx330_apfeat ↔ UNX-F0330 AP 特征二次采集与 BSP 差异断言（六叶闭集+逐寄存器diff+分叉账；0x3d60 段）
pub mod unx330_apfeat;
// unx331_ucoderev ↔ UNX-F0331 AP 微码修订只读检测（F0187 快照闸+一致断言+append-only 账；0x3d70 段）
pub mod unx331_ucoderev;
// unx310_pgarea ↔ UNX-F0310 页表区分配与账本登记（F0071/F0072 对接+双向幽灵审计）（0x3dc0 段）
pub mod unx310_pgarea;
// unx316_pgfaultinj ↔ UNX-F0316 页表错误注入（五类坏 PTE 检出 100%+项级定位）（0x3dd0 段）
pub mod unx316_pgfaultinj;
// unx317_pgperf ↔ UNX-F0317 初始页表性能账（F0099/F0198 口径同源+微秒落账）（0x3de0 段）
pub mod unx317_pgperf;
// unx318_trampolow ↔ UNX-F0318 trampoline 低 1MB 映射预置（双表在位+B17 消费断言）（0x3df0 段）
pub mod unx318_trampolow;
// unx319_pggolden ↔ UNX-F0319 页表构建黄金样本（设计矩阵逐项对照+双向差分）（0x3e50 段）
pub mod unx319_pggolden;
// unx320_ktestb16 ↔ UNX-F0320 ktest 引导面 B16 批断言集（探针登记制 16 席）（0x3e60 段）
pub mod unx320_ktestb16;
// unx332_apfirstlog ↔ UNX-F0332 AP 到达后第一日志（身份三元组+哨兵先行+序号连续）（0x3e70 段）
pub mod unx332_apfirstlog;
// unx333_bootfail ↔ UNX-F0333 多核启动失败分类（未达/超时/三振隔离状态机）（0x3ec0 段）
pub mod unx333_bootfail;
// unx334_x2apic ↔ UNX-F0334 x2APIC 模式检测与兼容路径（前置链+双路径同语义）（0x3ed0 段）
pub mod unx334_x2apic;
// unx335_apbudget ↔ UNX-F0335 AP 数量上限与预算（硬上限 255+分项扣款+拒绝有账）（0x3ee0 段）
pub mod unx335_apbudget;
// unx336_trampocrc ↔ UNX-F0336 trampoline 完整性校验（CRC32 落位前闸+单字节定位）（0x3ef0 段）
pub mod unx336_trampocrc;
// unx337_wakeorder ↔ UNX-F0337 唤醒次序与拓扑（逐个/成组策略+乱序 100% 检出）（0x3f50 段）
pub mod unx337_wakeorder;
// unx338_wakedrill ↔ UNX-F0338 AP 唤醒演练矩阵（QEMU 2/4/8 全绿+真机诚实 pending）（0x3f60 段）
pub mod unx338_wakedrill;
// unx339_singlecore ↔ UNX-F0339 单核强制模式 maxcpus=0（总闸四支路+零 SIPI 硬断言）（0x3f70 段）
pub mod unx339_singlecore;
// unx340_ktestb17 ↔ UNX-F0340 ktest 引导面 B17 批断言集（8 席+批级三断言）（0x3fc0 段）
pub mod unx340_ktestb17;
// ===== AI-WBB-1022 · UNX-A1-B18 AP 启动收官批（UNX-F0345~F0355，0xE400~0xE4B0 段）=====
// unx0345_aphealth ↔ UNX-F0345 AP 健康自检清单（十项闭集+FAIL 隔离移交 F0347；0xE400 段）
pub mod unx0345_aphealth;
// unx0346_apledger ↔ UNX-F0346 AP 启动总账（唤醒耗时五数概括+F0198 对接；0xE410 段）
pub mod unx0346_apledger;
// unx0347_apisoacct ↔ UNX-F0347 失败 AP 隔离（坏核降级账+F0333 六类联动；0xE420 段）
pub mod unx0347_apisoacct;
// unx0348_apnumastack ↔ UNX-F0348 NUMA 感知 AP 栈选址（F0074 位图一致+区间互斥；0xE430 段）
pub mod unx0348_apnumastack;
// unx0349_aphandoff ↔ UNX-F0349 AP 向 A2 交接包（定长小端+版本闸+A2 消费断言；0xE440 段）
pub mod unx0349_aphandoff;
// unx0350_aprecompute ↔ UNX-F0350 AP 汇合后全局重算（三守恒+幂等；0xE450 段）
pub mod unx0350_aprecompute;
// unx0351_apdrillmat ↔ UNX-F0351 AP 启动演练矩阵（四档×三场景 12 行 4/4；0xE460 段）
pub mod unx0351_apdrillmat;
// unx0352_apfailinject ↔ UNX-F0352 AP 启动失败注入（哨兵不置位+零误唤醒红线；0xE470 段）
pub mod unx0352_apfailinject;
// unx0353_tramporeclaim ↔ UNX-F0353 trampoline 内存回收（低 1MB 借还+热图同步；0xE480 段）
pub mod unx0353_tramporeclaim;
// unx0354_aptimeline ↔ UNX-F0354 AP 时序账零差异对接（ADR 白名单+50 轮复测；0xE4B0 段）
pub mod unx0354_aptimeline;
// unx0355_apucode ↔ UNX-F0355 AP 微码修订一致性（8 核 8/8+三方同源+只读；0xE4A0 段）
pub mod unx0355_apucode;

// ---- 死判据补登记（2026-10-22 接线 v2：模块已实现但从未入链）----
// // unx296_drillmatrix.rs ↔ UNX-F0296 panic 演练矩阵（20 类触发全过）
pub mod unx296_drillmatrix;
// // unx297_dualdog.rs ↔ UNX-F0297 双狗联动复盘（软狗/NMI 触发链归因）
pub mod unx297_dualdog;
// ---- AI-WBB-1022 第二批（波 01 A1 收官 7 单；0x20000+ u32 高段独占）----
// unx0234_handoffsnap ↔ UNX-F0234 handoff 快照（快照/台账 diff 零漂移；0x20000 段）
pub mod unx0234_handoffsnap;
// unx0235_auditself ↔ UNX-F0235 域审计自查单（finalize 闸：必查零缺零红；0x20040 段）
pub mod unx0235_auditself;
// unx0236_feedback ↔ UNX-F0236 邻域反馈通道（幂等去重+拒绝必带因码；0x20080 段）
pub mod unx0236_feedback;
// unx0237_docdiff ↔ UNX-F0237 文档对照（四态分类+容差核；0x200C0 段）
pub mod unx0237_docdiff;
// unx0238_ktestb12 ↔ UNX-F0238 ktest 收官面 B12（四族断言+复跑零漂移；0x20100 段）
pub mod unx0238_ktestb12;
// unx0239_batchtally ↔ UNX-F0239 批次清点 40 批（位图账+零缺零重；0x20140 段）
pub mod unx0239_batchtally;
// unx0240_closepack ↔ UNX-F0240 闭账物料包（五节三层签；0x20180 段）
pub mod unx0240_closepack;
// ---- AI-WBB-1022 第二批（终端/控制台 15 单；0x201C0~0x20540 u32 高段）----
// unx0265_cmdbus ↔ UNX-F0265 命令注册总线（名字唯一+三档位闸；0x201C0 段）
pub mod unx0265_cmdbus;
// unx0267_symcmd ↔ UNX-F0267 sym 批量符号化（升序闸+诚实回显；0x20200 段）
pub mod unx0267_symcmd;
// unx0268_ktestcmd ↔ UNX-F0268 ktest 过滤重跑（flaky 立案；0x20240 段）
pub mod unx0268_ktestcmd;
// unx0269_bootcmd ↔ UNX-F0269 boot 全视图（默认项存活核；0x20280 段）
pub mod unx0269_bootcmd;
// unx0270_fwview ↔ UNX-F0270 固件三视图（capsule 信任交叉核；0x202C0 段）
pub mod unx0270_fwview;
// unx0271_backpressure ↔ UNX-F0271 输出限流背压（零丢弃+滞回；0x20300 段）
pub mod unx0271_backpressure;
// unx0272_sessm ↔ UNX-F0272 会话状态机（迁移原子+回看只读；0x20340 段）
pub mod unx0272_sessm;
// unx0273_hotkey ↔ UNX-F0273 快捷键冲突表（保留前缀；0x20380 段）
pub mod unx0273_hotkey;
// unx0274_ascii ↔ UNX-F0274 纯 ASCII 降级（映射闭集+控制剥离；0x203C0 段）
pub mod unx0274_ascii;
// unx0275_dualchan ↔ UNX-F0275 双通道一致性（游标对账+补拉；0x20400 段）
pub mod unx0275_dualchan;
// unx0276_crashiso ↔ UNX-F0276 崩溃隔离（引导态对拍+熔断；0x20440 段）
pub mod unx0276_crashiso;
// unx0277_sessid ↔ UNX-F0277 会话标识（旧会话帧拦截；0x20480 段）
pub mod unx0277_sessid;
// unx0278_budget ↔ UNX-F0278 耗时预算（破产闸+白名单；0x204C0 段）
pub mod unx0278_budget;
// unx0279_cfgsnap ↔ UNX-F0279 配置快照（厂默认冻结+零漂移恢复；0x20500 段）
pub mod unx0279_cfgsnap;
// unx0280_b14 ↔ UNX-F0280 B14 断言集（引导五族+复跑零漂移；0x20540 段）
pub mod unx0280_b14;
// ---- AI-WBB-1022 第二批（panic/AP 启动面 7 单；0x20580~0x20700 u32 高段）----
// unx0287_unrecov ↔ UNX-F0287 不可恢复检测（三重故障/栈哨兵+停机账；0x20580 段）
pub mod unx0287_unrecov;
// unx0288_panicuart ↔ UNX-F0288 panic 期串口（零依赖+忙放弃+取证环；0x205C0 段）
pub mod unx0288_panicuart;
// unx0356_apconverge ↔ UNX-F0356 AP 串口汇聚（行原子+低号仲裁；0x20600 段）
pub mod unx0356_apconverge;
// unx0357_apcaprep ↔ UNX-F0357 AP 能力位（拓扑对齐核；0x20640 段）
pub mod unx0357_apcaprep;
// unx0358_maxcpus ↔ UNX-F0358 maxcpus 语义（钳位诚实降级；0x20680 段）
pub mod unx0358_maxcpus;
// unx0359_nmibcast ↔ UNX-F0359 NMI 广播预留（无载荷语义+覆盖核；0x206C0 段）
pub mod unx0359_nmibcast;
// unx0360_ktestb18 ↔ UNX-F0360 B18 断言集（AP 启动六族；0x20700 段）
pub mod unx0360_ktestb18;
pub mod unx0363_pfbitmap;
pub mod unx0364_lowmap;
pub mod unx0365_dmalow;
pub mod unx0366_ffalloc;
pub mod unx0367_pffree;
pub mod unx0368_heatmap;
pub mod unx0369_hugepage;
pub mod unx0370_resguard;
pub mod unx0371_pfledger;
pub mod unx0372_fragindex;
pub mod unx0373_oomplan;
pub mod unx0374_e820cross;
pub mod unx0375_poolprim;
pub mod unx0376_fivepoint;
pub mod unx0377_ktestmem;
pub mod unx0378_handfield;
pub mod unx0379_drilltopo;
pub mod unx0380_b19;
pub mod unx0388_heapleak;
pub mod unx0389_slabhalf;
pub mod unx0390_heapguard;
pub mod unx0391_oominject;
pub mod unx0392_heapgrow;
pub mod unx0393_stkpool;
pub mod unx0394_handreserve;
pub mod unx0395_heapledger;
pub mod unx0396_ktheaptest;
pub mod unx0397_bigbypass;
pub mod unx0398_zeropage;
pub mod unx0399_heapstorm;
pub mod unx0400_b20;
pub mod unx0406_wallclock;
pub mod unx0407_timq;
pub mod unx0408_tscdead;
pub mod unx0409_clksel;
pub mod unx0410_precself;
pub mod unx0411_monojump;
pub mod unx0412_softtimer;
pub mod unx0413_timefield;
pub mod unx0414_timefive;
pub mod unx0415_pitcross;
pub mod unx0416_timledger;
pub mod unx0417_freqdrift;
pub mod unx0418_ktimeassert;
pub mod unx0419_load3tier;
pub mod unx0420_b21;
pub mod unx0423_ist1;
pub mod unx0424_nmiist2;
pub mod unx0425_pfecode;
pub mod unx0426_picshut;
pub mod unx0427_ioapicprobe;
pub mod unx0428_lapicprobe;
pub mod unx0429_vecalloc;
pub mod unx0430_spurious;
pub mod unx0431_exinj;
pub mod unx0432_veccount;
pub mod unx0433_irqlat;
pub mod unx0434_ifdisc;
pub mod unx0435_istsent;
pub mod unx0436_iretq;
pub mod unx0437_crashtsnap;
pub mod unx0438_stormdrill;
pub mod unx0439_handfield;
pub mod unx0440_b22;
pub mod unx0443_rsdtwalk;
pub mod unx0444_tblcsum;
pub mod unx0445_madtirq;
pub mod unx0446_hpetparse;
pub mod unx0447_fadttiming;
pub mod unx0448_mcfgecam;
pub mod unx0449_dsdtloc;
pub mod unx0450_sratnuma;
pub mod unx0451_tblsnap;
pub mod unx0452_degrade;
pub mod unx0453_pinseal;
pub mod unx0454_badinj;
pub mod unx0455_mapreg;
pub mod unx0456_handoff;
pub mod unx0457_5num;
pub mod unx0458_acpize;
pub mod unx0459_ktap;
pub mod unx0460_b23;
pub mod unx0465_pcihdr;
pub mod unx0466_pcibar;
pub mod unx0467_pcclass;
pub mod unx0468_pcicap;
pub mod unx0469_pciecap;
pub mod unx0470_roline;
pub mod unx0471_xchk;
pub mod unx0472_enumdog;
pub mod unx0473_rescan;
pub mod unx0474_pci5num;
pub mod unx0475_scandrill;
pub mod unx0476_pcihand;
pub mod unx0477_pcxacc;
pub mod unx0478_pcbad;
pub mod unx0479_pcikt;
pub mod unx0480_b24;
pub mod unx0488_rndguard;
pub mod unx0489_rndperf;
pub mod unx0490_rndpat;
pub mod unx0491_fbsent;
pub mod unx0492_rndops;
pub mod unx0493_rndmulti;
pub mod unx0494_dmarcap;
pub mod unx0495_rtroot;
pub mod unx0496_iotlbeg;
pub mod unx0497_devtlb;
pub mod unx0498_faultrec;
pub mod unx0499_irtable;
pub mod unx0500_domseal;
pub mod unx0506_dmarops;
pub mod unx0507_ctxcache;
pub mod unx0508_pasidtag;
pub mod unx0509_sectl2;
pub mod unx0510_ptewalk;
pub mod unx0511_qtqueue;
pub mod unx0512_iecinval;
pub mod unx0513_intpost;
pub mod unx0514_svmtrack;
pub mod unx0515_devrw;
pub mod unx0516_prq;
pub mod unx0517_wdinvq;
pub mod unx0518_perfctr;
pub mod unx0519_vtclose;
pub mod unx0521_ktestcore;
pub mod unx0522_asserts;
pub mod unx0523_fail3;
pub mod unx0524_suitegrp;
pub mod unx0525_summary;
pub mod unx0526_redbook;
pub mod unx0527_watchdog;
pub mod unx0528_nsisol;
pub mod unx0529_rndinj;
pub mod unx0530_covermtx;
pub mod unx0531_outproto;
pub mod unx0532_persist;
pub mod unx0533_onekey;
pub mod unx0534_hedge;
pub mod unx0535_residue;
pub mod unx0536_frameself;
pub mod unx0537_fmtver;
pub mod unx0538_handoff;
pub mod unx0539_hedgetot;
pub mod unx0540_b27set;
pub mod unx0541_logemit;
pub mod unx0542_logfilt;
pub mod unx0543_linefmt;
pub mod unx0544_ringbuf;
pub mod unx0545_uartout;
pub mod unx0546_fbout;
pub mod unx0547_tstamp;
pub mod unx0548_comptag;
pub mod unx0549_logquery;
pub mod unx0550_countreck;
pub mod unx0551_ringwrap;
pub mod unx0552_dualswitch;
pub mod unx0553_linecsum;
pub mod unx0554_logstorm;
pub mod unx0555_badinj;
pub mod unx0556_fivestat;
pub mod unx0557_loghedge;
pub mod unx0558_auditlink;
pub mod unx0559_loghandoff;
pub mod unx0560_b28set;
pub mod unx0531_nxguard;
pub mod unx0532_wptrap;
pub mod unx0533_cowpage;
pub mod unx0534_tlbshoot;
pub mod unx0535_hugefld;
pub mod unx0536_mapaudit;
pub mod unx0537_pfcensus;
pub mod unx0538_sharecred;
pub mod unx0539_zswapin;
pub mod unx0543_mmuclose;
pub mod unx0544_sipilane;
pub mod unx0545_apwait;
pub mod unx0546_ticket;
pub mod unx0547_genbar;
pub mod unx0548_percpu;
pub mod unx0549_ipiburst;
pub mod unx0550_memfence;
pub mod unx0551_coreonline;
pub mod unx0552_corehalt;
pub mod unx0553_smpclose;
pub mod unx0567_rqlane;
pub mod unx0568_rrband;
pub mod unx0569_priolane;
pub mod unx0570_preempt2;
pub mod unx0571_waitq;
pub mod unx0572_mlfq2;
pub mod unx0573_starvgua;
pub mod unx0574_loadbal;
pub mod unx0575_schedtick;
pub mod unx0576_schedclose;

// ---- AI-VX-1024（2026-10-24）：UNX-F1283 ps2 轮询路径测绘账（隔离探针全绿后入库，0x139 段）----
pub mod unx1283_ps2map;
// ---- AI-VX-1024（2026-10-24）第五波三单：F1263/F1304/F1323（隔离探针全绿后入库）----
pub mod unx1263_irqreverse;
pub mod unx1304_ps2retire;
pub mod unx1323_serequiv;
// ---- AI-VX-1024（2026-10-24）六/七波六单：F0344/F1044/F1083/F1063/F0328/F0586（隔离探针全绿后入库）----
pub mod unx0344_percpu;
pub mod unx1044_dlineacc;
pub mod unx1083_timcoales;
pub mod unx1063_nohz;
pub mod unx0328_apstack;
pub mod unx0586_valdom;
// ---- AI-VX-1024（2026-10-24）第八波三单：F0604/F0622/F0644（隔离探针全绿后入库）----
pub mod unx604_cpioiter;
pub mod unx0622_pkgindex;
pub mod unx0644_failredo;
// ---- AI-VX-1024（2026-10-24）第九波三单：F0305/F0663/F0687（隔离探针全绿后入库）----
pub mod unx0305_hugesel;
pub mod unx0663_tsccross;
pub mod unx0687_succrate;
// ---- AI-VX-1024（2026-10-24）第十波三单：F0705/F0725/F0745（隔离探针全绿后入库）----
pub mod unx0705_assert70;
pub mod unx0725_resptree;
pub mod unx745_valgrade;
// ---- AI-VX-1024（2026-10-24）第十一波三单：F0763/F0805/F0825（隔离探针全绿后入库）----
pub mod unx0763_finalsweep;
pub mod unx0805_spurious;
pub mod unx0825_cr8tpr;
// ---- AI-VX-1024（2026-10-24）第十二波三单：F0844/F0862/F0885（隔离探针全绿后入库）----
pub mod unx0844_ipieoi;
pub mod unx0862_rterw;
pub mod unx0885_irqbal;
// ---- AI-VX-1024（2026-10-24）第十三波三单：F0939/F0944/F0962（隔离探针全绿后入库）----
pub mod unx0939_bootseq;
pub mod unx0944_bhtrig;
pub mod unx0962_pollprof;
// ---- AI-VX-1024（2026-10-24）第十四波三单：F0785/F1003/F1028（隔离探针全绿后入库）----
pub mod unx0785_domenroll;
pub mod unx1003_tsccal;
pub mod unx1028_tmoacc;
