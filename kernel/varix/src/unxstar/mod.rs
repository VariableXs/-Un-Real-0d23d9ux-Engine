//! UNX 册模块登记（CoRun Varix STAR II · Unxreal —— UNX-A1 引导与早年初始化）。
//!
//! 本册与主内核 acpi.rs（F008 RSDP/XSDT/FADT/MADT 基础解析）为分层深化：
//! 本册做 ACPI 表专项（DSDT/SSDT/AML/拓扑/UEFI 态），基础解析不重复实现，
//! 直接复用 `crate::acpi` 的 SdtHeader/checksum_ok/SDT_HEADER_LEN。
//! 后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433。

use alloc::string::String;
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
// unx521_ktest_frame ↔ UNX-F0521 ktest 主框架总装（注册器名唯一定容 32 + 运行器泵动单步/全量同状态机 + 接入核审计注册⇄运行 6/6 + 截断显性拦截假绿；0x85 段）
pub mod unx521_ktest_frame;
// unx541_log_emit ↔ UNX-F0541 日志面总装（四级别冻结 Trace/Info/Warn/Fatal + emit 三参数（级别/组件/格式化文）+ 级别闸丢弃逐格记账 + 环满显性溢出不覆盖 + 10/10 全链接入核；0x86 段）
pub mod unx541_log_emit;
// unx561_power_ledger ↔ UNX-F0561 电源面定位总账（FADT 五块 PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 基址+长度 + 直读一致 5/5 + IO/MMIO/FixedHw 空间分派 + 宽 1/2/4 合法 + 封条指纹掉电复验；0x87 段）
pub mod unx561_power_ledger;
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
    use crate::checks::CheckSet;
    let mut set = CheckSet::new(UNX_DOMAIN);
    let blocks: [(&'static str, CheckSet); 31] = [
        ("UNX-F0041", unx041_dsdt_checks::run_unx041_checks()),
        ("UNX-F0041-DEEP", unx041_dsdt_deep::run_unx041b_checks()),
        ("UNX-F0081", unx081_kaslr_entropy::run_unx081_checks()),
        ("UNX-F0101", unx101_vgafallback::run_unx101_checks()),
        ("UNX-F0121", unx121_extbootstate::run_unx121_checks()),
        ("UNX-F0141", unx141_isogen::run_unx141_checks()),
        ("UNX-F0161", unx161_rtremap::run_unx161_checks()),
        ("UNX-F0201", unx201_bootreport::run_unx201_checks()),
        ("UNX-F0182", unx182_handover::run_unx182_checks()),
        ("UNX-F0421", unx421_idt_reload::run_unx421_checks()),
        ("UNX-F0441", unx441_rsdp_scan::run_unx441_checks()),
        ("UNX-F0461", unx461_pci_route::run_unx461_checks()),
        ("UNX-F0481", unx481_fb_ledger::run_unx481_checks()),
        ("UNX-F0521", unx521_ktest_frame::run_unx521_checks()),
        ("UNX-F0541", unx541_log_emit::run_unx541_checks()),
        ("UNX-F0561", unx561_power_ledger::run_unx561_checks()),
        ("UNX-F0601", unx601_initrd_ledger::run_unx601_checks()),
        ("UNX-F0621", unx621_pkg_aggregate::run_unx621_checks()),
        ("UNX-F0641", unx641_regress_round::run_unx641_checks()),
        ("UNX-F0661", unx661_time_anchors::run_unx661_checks()),
        ("UNX-F0681", unx681_bench_machine::run_unx681_checks()),
        ("UNX-F0701", unx701_assert_ledger::run_unx701_checks()),
        ("UNX-F0721", unx721_limine_ledger::run_unx721_checks()),
        ("UNX-F0183", unx183_failiso::run_unx183_checks()),
        ("UNX-F0204", unx204_reportring::run_unx204_checks()),
        ("UNX-F0223", unx223_dombound::run_unx223_checks()),
        ("UNX-F0243", unx243_memsnap::run_unx243_checks()),
        ("UNX-F0263", unx263_xmodem::run_unx263_checks()),
        ("UNX-F0323", unx323_realmjmp::run_unx323_checks()),
        ("UNX-F0342", unx342_apsilence::run_unx342_checks()),
        ("UNX-F0362", unx362_resvwhite::run_unx362_checks()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
    
    {
        let sub = run_unx20_t082();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0082",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t126();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0123",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t147();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0143",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t163();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0163",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t203();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0203",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t228();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0222",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t242();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0242",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t268();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0262",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t286();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0282",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t326();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0322",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t306();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0302",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    // UNX-20 任务族登记（20 域各一笔，总量 8+20=28 ≤ MAX_CHECKS 112）
    {
        let sub = run_unx20_t162();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0162",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t221();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0221",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t241();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0241",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t261();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0261",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t281();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0281",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t301();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0301",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t311();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0321",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t321();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0341",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t341();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0361",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t351();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0381",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t361();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0401",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t421();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0421",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t431();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0441",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t441();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0461",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t561();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0481",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t571();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0501",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t581();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0521",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t591();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0541",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_ta01();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0561",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_tb01();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F0581",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    set.add(
            tag,
            passed,
            if passed {
                ""
            } else if sub.truncated() {
                "sub-checks truncated"
            } else {
                "sub-checks red"
            },
        );
    }
    {
        let sub = run_wb20_u1501();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1501",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1541();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1541",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1561();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1561",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1581();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1581",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1601();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1601",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1621();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1621",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1641();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1641",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1661();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1661",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1681();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1681",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1701();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1701",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1721();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1721",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1741();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1741",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1761();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1761",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1781();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1781",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1801();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1801",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1821();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1821",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1841();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1841",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1861();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1861",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1881();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1881",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    {
        let sub = run_wb20_u1901();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F1901",
            passed,
            if passed { "" } else if sub.truncated() { "wb20 family truncated" } else { "wb20 family red" },
        );
    }
    set
}
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
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "UNX 册自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert!(!set.truncated(), "UNX 域聚合溢出（子集数超 CheckSet 上限）");
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
// unx582_kvparse —— UNX-F0582 语法解析器（token/key=value，空格分词+等号劈分规则冻结，20 样本基准 20/20，0x98 段六码）
pub mod unx582_kvparse;
// unx403_tsccalib —— UNX-F0403 TSC 频率三方校准（PIT 5 秒窗/MSR 平台值/CPUID 0x15-0x16，两两差 bps ≤50 一致，超差取中位数立案，0x99 段六码）
pub mod unx403_tsccalib;
// unx384_alignalloc —— UNX-F0384 对齐分配语义（2^n 对齐面，100% 对齐重放核 + 零跨档浪费超 1 块守卫，0x9A 段七码）
pub mod unx384_alignalloc;
// unx385_pairledger —— UNX-F0385 分配记账（申请/释放配对账，配对键=调用点+尺寸档双向可查，配对率 100%，0x9B 段七码）
pub mod unx385_pairledger;
// unx1043_tscdeadline —— UNX-F1043 TSC-deadline 模式（LAPIC deadline 投递，10 万次零丢失，探测 10/10，p99≤10µs，0x9C 段七码）
pub mod unx1043_tscdeadline;
// unx1023_hpettick —— UNX-F1023 HPET 周期模式与 tick 生成（1ms tick 10^6 拍漂移<100µs，抖动 p99<10µs，丢拍检测 10/10，0x9D 段七码）
pub mod unx1023_hpettick;





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
// unx1002_tscprobe —— UNX-F1002 TSC 特性探测深化版（三环境判定10/10/降级三档声明/频率三源对拍/启动报告九键，0x4CF0~0x4CFF 段）
pub mod unx1002_tscprobe;
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
pub mod unx981_irqstorm;
pub mod unx1001_tsync;
pub mod unx1021_hpetprobe;
pub mod unx1041_clockarb;
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
