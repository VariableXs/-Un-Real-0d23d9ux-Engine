//! UNX 册模块登记（CoRun Varix STAR II · Unxreal —— UNX-A1 引导与早年初始化）。
//!
//! 本册与主内核 acpi.rs（F008 RSDP/XSDT/FADT/MADT 基础解析）为分层深化：
//! 本册做 ACPI 表专项（DSDT/SSDT/AML/拓扑/UEFI 态），基础解析不重复实现，
//! 直接复用 `crate::acpi` 的 SdtHeader/checksum_ok/SDT_HEADER_LEN。
//! 后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433。

use alloc::string::String;
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
    use crate::checks::CheckSet;
    let mut set = CheckSet::new(UNX_DOMAIN);
    let blocks: [(&'static str, CheckSet); 69] = [
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
        ("UNX-F0264", unx264_lineedit::run_unx264_checks()),
        ("UNX-F0324", unx324_sipiseq::run_unx324_checks()),
        ("UNX-F0643", unx643_drilllog::run_unx643_checks()),
        ("UNX-F0683", unx683_boottrace::run_unx683_checks()),
        ("UNX-F0206", unx206_excode::run_unx206_checks()),
        ("UNX-F0245", unx245_randnode::run_unx245_checks()),
        ("UNX-F0185", unx185_cpufeat::run_t185_checks()),
        ("UNX-F1563", unx1563_a4sign::run_unx1563_checks()),
        ("UNX-F1842", unx1842_ipivec::run_unx1842_checks()),
        ("UNX-F1902", unx1902_lazysave::run_unx1902_checks()),
        ("UNX-F0326", unx326_apsentinel::run_unx326_checks()),
        ("UNX-F0327", unx327_tramppagemap::run_unx327_checks()),
        ("UNX-F0405", unx405_monoclock::run_unx405_checks()),
        ("UNX-F0584", unx584_unkparam::run_unx584_checks()),
        ("UNX-F2101", unx2101_schedstat::run_unx2101_checks()),
        ("UNX-F2221", unx2221_freeledge::run_f2221_checks()),
        ("UNX-F0883", unx0883_migwin::run_t883_checks()),
        ("UNX-F1026", unx1026_hpetcand::run_t1026_checks()),
        ("UNX-F1103", unx1103_disptab::run_t1103_checks()),
        ("UNX-F1302", unx1302_tickretire::run_t1302_checks()),
        ("UNX-F1482", unx1482_gaplist::run_t1482_checks()),
        ("UNX-F1503", unx1503_quadfsm::run_t1503_checks()),
        ("UNX-F1903", unx1903_xsaveamort::run_t1903_checks()),
        ("UNX-F1941", unx1941_nmiipi::run_t1941_checks()),
        ("UNX-F1961", unx1961_freqdom::run_t1961_checks()),
        ("UNX-F1981", unx1981_twogov::run_t1981_checks()),
        ("UNX-F2001", unx2001_schedutil::run_t2001_checks()),
        ("UNX-F2021", unx2021_cstates::run_t2021_checks()),
        ("UNX-F2081", unx2081_qdepth::run_t2081_checks()),
        ("UNX-F2102", unx2102_sdebug::run_t2102_checks()),
        ("UNX-F2141", unx2141_triface::run_t2141_checks()),
        ("UNX-F2161", unx2161_stressorg::run_t2161_checks()),
        ("UNX-F2181", unx2181_regbook::run_t2181_checks()),
        ("UNX-F2201", unx2201_freezectl::run_t2201_checks()),
        ("UNX-F2241", unx2241_finalchain::run_t2241_checks()),
        ("UNX-F2222", unx2222_rollver::run_f2222_checks()),
        ("UNX-F0505", unx505_loopback::run_f505_checks()),
        ("UNX-F1723", unx1723_migledger::run_f1723_checks()),
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
    {
        let sub = run_unx20_t325();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "AP ID 核对域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t684();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "固件路差异域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t505();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "万次切换域",
            passed,
            if passed { "" } else if sub.truncated() { "unx20 family truncated" } else { "unx20 family red" },
        );
    }
    {
        let sub = run_unx20_t545();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "判据冻结域",
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
    {
        // WB21 批次（第二波深化 20 单）：drill 入口逐单折叠为一个族集，
        // 族内任一红即整族红（与 wb20 family 折叠范式一致）。
        let sub = run_wb21_unx20();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F-WB21",
            passed,
            if passed { "" } else if sub.truncated() { "wb21 family truncated" } else { "wb21 family red" },
        );
    }
    {
        // WB22 批次（第三波 22 单）：drill 入口逐单折叠为一个族集。
        let sub = run_wb22_unx22();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F-WB22",
            passed,
            if passed { "" } else if sub.truncated() { "wb22 family truncated" } else { "wb22 family red" },
        );
    }
    {
        // UNX-21 批次（本会话深编）：逐单折叠为一个族集。
        let sub = run_unx21_batch();
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            "UNX-F-UNX21",
            passed,
            if passed { "" } else if sub.truncated() { "unx21 family truncated" } else { "unx21 family red" },
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
pub fn run_wb21_unx20() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("WB21-unx20");
    let entries: [(&'static str, bool, &'static str); 20] = [
        ("UNX-F0404", unx404_hpetcap::drill_green().0, "wb21 hpetcap drill red"),
        ("UNX-F0583", unx583_wlparams::drill_green().0, "wb21 wlparams drill red"),
        ("UNX-F1102", unx1102_stubaudit::pump_100k().0, "wb21 stubaudit pump red"),
        ("UNX-F1201", unx1201_polloop::drill_1m().0, "wb21 polloop drill red"),
        ("UNX-F1301", unx1301_retire::drill_10().0, "wb21 retire drill red"),
        ("UNX-F1421", unx1421_latdist::drill_100k().0, "wb21 latdist drill red"),
        ("UNX-F1441", unx1441_schema::drill_full().0, "wb21 schema drill red"),
        ("UNX-F1461", unx1461_zeroeq::drill_10rounds().0, "wb21 zeroeq drill red"),
        ("UNX-F1501-D2", unx1501_regmat::drill_full().0, "wb21 regmat drill red"),
        ("UNX-F1541-D2", unx1541_dombind::drill_seal().0, "wb21 dombind drill red"),
        ("UNX-F1561-D2", unx1561_cosign::drill_cosign().0, "wb21 cosign drill red"),
        ("UNX-F1581-D2", unx1581_domreg::drill_800().0, "wb21 domreg drill red"),
        ("UNX-F1601-D2", unx1601_smpcons::drill_full().0, "wb21 smpcons drill red"),
        ("UNX-F1621-D2", unx1621_percpu::drill_layout().0, "wb21 percpu drill red"),
        ("UNX-F1641-D2", unx1641_runqueue::drill_full().0, "wb21 runqueue drill red"),
        ("UNX-F1661-D2", unx1661_schedcls::drill_chain().0, "wb21 schedcls drill red"),
        ("UNX-F1681-D2", unx1681_timeslice::drill_model().0, "wb21 timeslice drill red"),
        ("UNX-F1701-D2", unx1701_loadtrk::drill_model().0, "wb21 loadtrk drill red"),
        ("UNX-F1721-D2", unx1721_balexec::drill_chain().0, "wb21 balexec drill red"),
        ("UNX-F1741-D2", unx1741_idlest::drill_cycle().0, "wb21 idlest drill red"),
    ];
    for (tag, ok, detail) in entries.iter() {
        set.add(tag, *ok, if *ok { "" } else { detail });
    }
    set
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

// ===================== AI-WB3 · CoRun UNX-A4 批 40 单深化版（诊断码段 0x12C0~0x2940 四十段） =====================
// unx2321_unisign3 ↔ UNX-F2321（册目标见施工书；诊断码段 12C0–12C0 独占）
pub mod unx2321_unisign3;
// unx2341_udomregress ↔ UNX-F2341（册目标见施工书；诊断码段 13C0–13C0 独占）
pub mod unx2341_udomregress;
// unx2361_udecLARE3 ↔ UNX-F2361（册目标见施工书；诊断码段 15C0–15C0 独占）
pub mod unx2361_udecLARE3;
// unx2381_ufinalacct ↔ UNX-F2381（册目标见施工书；诊断码段 1640–1640 独占）
pub mod unx2381_ufinalacct;
// unx2402_ufreemap ↔ UNX-F2402（册目标见施工书；诊断码段 16C0–16C0 独占）
pub mod unx2402_ufreemap;
// unx2422_uorder14 ↔ UNX-F2422（册目标见施工书；诊断码段 1740–1740 独占）
pub mod unx2422_uorder14;
// unx2442_urefill ↔ UNX-F2442（册目标见施工书；诊断码段 17C0–17C0 独占）
pub mod unx2442_urefill;
// unx2482_uorder9raw ↔ UNX-F2482（册目标见施工书；诊断码段 18C0–18C0 独占）
pub mod unx2482_uorder9raw;
// unx2502_uhptime ↔ UNX-F2502（册目标见施工书；诊断码段 1A80–1A80 独占）
pub mod unx2502_uhptime;
// unx2562_uaspcreate ↔ UNX-F2562（册目标见施工书；诊断码段 1BC0–1BC0 独占）
pub mod unx2562_uaspcreate;
// unx2582_uvmainject ↔ UNX-F2582（册目标见施工书；诊断码段 1C40–1C40 独占）
pub mod unx2582_uvmainject;
// unx2602_upfentry ↔ UNX-F2602（册目标见施工书；诊断码段 1C80–1C80 独占）
pub mod unx2602_upfentry;
// unx2622_upcidbook ↔ UNX-F2622（册目标见施工书；诊断码段 1CC0–1CC0 独占）
pub mod unx2622_upcidbook;
// unx2642_uptebatchro ↔ UNX-F2642（册目标见施工书；诊断码段 1D40–1D40 独占）
pub mod unx2642_uptebatchro;
// unx2662_ulookup ↔ UNX-F2662（册目标见施工书；诊断码段 1D80–1D80 独占）
pub mod unx2662_ulookup;
// unx2682_ucacheif ↔ UNX-F2682（册目标见施工书；诊断码段 1DC0–1DC0 独占）
pub mod unx2682_ucacheif;
// unx2702_ulrufield ↔ UNX-F2702（册目标见施工书；诊断码段 1E40–1E40 独占）
pub mod unx2702_ulrufield;
// unx2722_uanonevict ↔ UNX-F2722（册目标见施工书；诊断码段 1E80–1E80 独占）
pub mod unx2722_uanonevict;
// unx2762_umaptbl ↔ UNX-F2762（册目标见施工书；诊断码段 1EC0–1EC0 独占）
pub mod unx2762_umaptbl;
// unx2782_ubuddymerge ↔ UNX-F2782（册目标见施工书；诊断码段 1F40–1F40 独占）
pub mod unx2782_ubuddymerge;
// unx2801_ubigpoolcap ↔ UNX-F2801（册目标见施工书；诊断码段 1F80–1F80 独占）
pub mod unx2801_ubigpoolcap;
// unx2841_ubigasp ↔ UNX-F2841（册目标见施工书；诊断码段 20C0–20C0 独占）
pub mod unx2841_ubigasp;
// unx2861_ubigmatrix ↔ UNX-F2861（册目标见施工书；诊断码段 2140–2140 独占）
pub mod unx2861_ubigmatrix;
// unx2901_ucachestress ↔ UNX-F2901（册目标见施工书；诊断码段 21C0–21C0 独占）
pub mod unx2901_ucachestress;
// unx2941_udomstress ↔ UNX-F2941（册目标见施工书；诊断码段 2240–2240 独占）
pub mod unx2941_udomstress;
// unx2981_udomclose ↔ UNX-F2981（册目标见施工书；诊断码段 22C0–22C0 独占）
pub mod unx2981_udomclose;
// unx3001_udualchain ↔ UNX-F3001（册目标见施工书；诊断码段 2340–2340 独占）
pub mod unx3001_udualchain;
// unx3021_utier3 ↔ UNX-F3021（册目标见施工书；诊断码段 23C0–23C0 独占）
pub mod unx3021_utier3;
// unx3061_upowerloss ↔ UNX-F3061（册目标见施工书；诊断码段 24C0–24C0 独占）
pub mod unx3061_upowerloss;
// unx3081_utierfinal ↔ UNX-F3081（册目标见施工书；诊断码段 2580–2580 独占）
pub mod unx3081_utierfinal;
// unx3101_uregfreeze ↔ UNX-F3101（册目标见施工书；诊断码段 25C0–25C0 独占）
pub mod unx3101_uregfreeze;
// unx3121_ureconall ↔ UNX-F3121（册目标见施工书；诊断码段 2640–2640 独占）
pub mod unx3121_ureconall;
// unx3141_ubaseline ↔ UNX-F3141（册目标见施工书；诊断码段 2680–2680 独占）
pub mod unx3141_ubaseline;
// unx3161_uregexec ↔ UNX-F3161（册目标见施工书；诊断码段 26C0–26C0 独占）
pub mod unx3161_uregexec;
// unx3181_udomainfinal ↔ UNX-F3181（册目标见施工书；诊断码段 2740–2740 独占）
pub mod unx3181_udomainfinal;
// unx3361_utimewin ↔ UNX-F3361（册目标见施工书；诊断码段 2780–2780 独占）
pub mod unx3361_utimewin;
// unx3381_usamplelib ↔ UNX-F3381（册目标见施工书；诊断码段 27C0–27C0 独占）
pub mod unx3381_usamplelib;
// unx3401_uktestdesign ↔ UNX-F3401（册目标见施工书；诊断码段 2880–2880 独占）
pub mod unx3401_uktestdesign;
// unx3421_udualbackend ↔ UNX-F3421（册目标见施工书；诊断码段 28C0–28C0 独占）
pub mod unx3421_udualbackend;
// unx3461_umountarch ↔ UNX-F3461（册目标见施工书；诊断码段 2940–2940 独占）
pub mod unx3461_umountarch;
