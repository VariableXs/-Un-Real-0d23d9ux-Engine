//! UNX 册模块登记（CoRun Varix STAR II · Unxreal —— UNX-A1 引导与早年初始化）。
//!
//! 本册与主内核 acpi.rs（F008 RSDP/XSDT/FADT/MADT 基础解析）为分层深化：
//! 本册做 ACPI 表专项（DSDT/SSDT/AML/拓扑/UEFI 态），基础解析不重复实现，
//! 直接复用 `crate::acpi` 的 SdtHeader/checksum_ok/SDT_HEADER_LEN。
//! 后续各单在此追加注册——先推模块本体再补注册行，防孤儿登记 E0433。

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
    let blocks: [(&'static str, CheckSet); 23] = [
    let blocks: [(&'static str, CheckSet); 18] = [
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
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
    
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
pub mod unx282_pentry;
pub mod unx283_pdedup;
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
