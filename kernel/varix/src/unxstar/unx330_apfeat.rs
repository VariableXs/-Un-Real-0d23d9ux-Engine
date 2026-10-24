//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0330 · AP 特征二次采集与 BSP 差异断言 · 诊断段 0x3d60
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0330-J1 AP 端 CPUID 重采集与 BSP 特征表 diff 零差异
//!   （同构断言）。
//! 难点分析：
//!   AP 唤醒后 CPUID 必须重采一次——BSP 的表是「登记」不是「真相」，
//!   微码/热插拔/虚拟化逃逸都可能让 AP 与 BSP 出现特征分叉。分叉不可怕，
//!   可怕的是静默分叉：调度器按 BSP 假设派发 SSE/AVX 任务到不支持的
//!   AP 上就是 #UD。所以断言分三层：采集完整性（必需叶闭集）、
//!   逐叶 diff（同构零差异）、分叉账（异构时逐叶留账可查）。
//! 纪律约束：
//!   - 采集面闭集：必需叶缺一即拒，重复叶即拒，不许「差不多」；
//!   - diff 逐叶逐寄存器，异构账带叶号+寄存器+双值；
//!   - 同构断言是硬门：异构时调用方必须显式走降级路径；
//!   - 采集器零副作用：probe 闭包注入，方便构建侧对拍。
//! 成本记账：采集 O(叶数)；diff O(叶数)；账 O(分叉数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（全仓普查 0x3d60 空闲后占段）。
pub const SEG_BASE: u16 = 0x3d60;

// ─────────────────────────────────────────────────────────────────────
// 一、CPUID 叶与特征表
// ─────────────────────────────────────────────────────────────────────

/// 一片 CPUID 结果（leaf, subleaf, eax, ebx, ecx, edx）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuidLeaf {
    /// 主叶号。
    pub leaf: u32,
    /// 子叶号。
    pub subleaf: u32,
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

impl CpuidLeaf {
    /// 四寄存器指纹（FNV-1a 32 位）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for v in [self.leaf, self.subleaf, self.eax, self.ebx, self.ecx, self.edx] {
            for b in v.to_le_bytes() {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        h
    }
}

/// 必需叶闭集（AP 必须全部上报；来源：启动链登记的最小可调度面）。
pub const REQUIRED_LEAVES: [u32; 6] = [
    0x0000_0000, // 最高叶号与厂商标串
    0x0000_0001, // 家族/模型/步进 + 基础特征
    0x0000_0007, // 结构化扩展特征（subleaf 0）
    0x8000_0000, // 扩展最高叶号
    0x8000_0001, // 扩展特征（LongMode 等）
    0x8000_0002, // 品牌串第一段
];

/// 特征表：必需叶的有序集（按 leaf 升序，同 leaf 按 subleaf）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureTable {
    /// 采到的叶（已排序、无重复）。
    pub leaves: Vec<CpuidLeaf>,
    /// 采集方身份（0=BSP，其余=AP ID）。
    pub who: u32,
}

/// 采集错误（闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectErr {
    /// 缺叶：required 中有叶未上报。
    MissingLeaf,
    /// 重复叶：同 (leaf, subleaf) 出现两次。
    DupLeaf,
    /// 超量：上报叶数越过上限（探测面被污染）。
    TooManyLeaves,
    /// probe 失败：底层读数不可用。
    ProbeFail,
}

impl CollectErr {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                CollectErr::MissingLeaf => 1,
                CollectErr::DupLeaf => 2,
                CollectErr::TooManyLeaves => 3,
                CollectErr::ProbeFail => 4,
            }
    }

    /// 人话 + 建议。
    pub fn detail(self) -> &'static str {
        match self {
            CollectErr::MissingLeaf =>
                "必需叶缺席——AP 的 CPUID 读面不完整。建议核对唤醒协议是否完整跑完。",
            CollectErr::DupLeaf =>
                "同叶重复上报——采集协议被重放。建议查二次采集是否被意外重入。",
            CollectErr::TooManyLeaves =>
                "上报叶数越上限——探测面被污染。建议按损坏归档本次采集。",
            CollectErr::ProbeFail =>
                "probe 失败——底层读数不可用。建议确认 MSR/CPUID 门禁状态。",
        }
    }
}

/// 单核采集上限（防御污染）。
pub const MAX_LEAVES: usize = 64;

/// 从 probe 结果建表（probe 由调用方注入，本模块不触真机）。
pub fn collect(
    who: u32,
    probe: &dyn Fn(u32, u32) -> Option<CpuidLeaf>,
) -> Result<FeatureTable, CollectErr> {
    let mut leaves: Vec<CpuidLeaf> = Vec::new();
    for &req in REQUIRED_LEAVES.iter() {
        let sub = if req == 0x0000_0007 { 0u32 } else { 0u32 };
        match probe(req, sub) {
            Some(l) => leaves.push(l),
            None => return Err(CollectErr::MissingLeaf),
        }
        if leaves.len() > MAX_LEAVES {
            return Err(CollectErr::TooManyLeaves);
        }
    }
    // 排序 + 查重
    leaves.sort_by_key(|l| (l.leaf, l.subleaf));
    for w in leaves.windows(2) {
        if w[0].leaf == w[1].leaf && w[0].subleaf == w[1].subleaf {
            return Err(CollectErr::DupLeaf);
        }
    }
    Ok(FeatureTable { leaves, who })
}

// ─────────────────────────────────────────────────────────────────────
// 二、diff 与同构断言
// ─────────────────────────────────────────────────────────────────────

/// 一处分叉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffEntry {
    /// 叶号。
    pub leaf: u32,
    /// 子叶号。
    pub subleaf: u32,
    /// 寄存器名（0=eax 1=ebx 2=ecx 3=edx）。
    pub reg: u8,
    /// BSP 值。
    pub bsp: u32,
    /// AP 值。
    pub ap: u32,
}

/// diff 结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// 逐寄存器分叉账。
    pub entries: Vec<DiffEntry>,
    /// BSP 表身份。
    pub bsp_who: u32,
    /// AP 表身份。
    pub ap_who: u32,
}

impl DiffReport {
    /// 同构：零分叉。
    pub fn homogeneous(&self) -> bool {
        self.entries.is_empty()
    }
}

/// BSP×AP 逐寄存器 diff（两表必须同为完整采集，缺叶按分叉入账）。
pub fn diff(bsp: &FeatureTable, ap: &FeatureTable) -> Result<DiffReport, CollectErr> {
    if bsp.leaves.len() != REQUIRED_LEAVES.len()
        || ap.leaves.len() != REQUIRED_LEAVES.len()
    {
        return Err(CollectErr::MissingLeaf);
    }
    let mut entries = Vec::new();
    for (b, a) in bsp.leaves.iter().zip(ap.leaves.iter()) {
        if b.leaf != a.leaf || b.subleaf != a.subleaf {
            // 对位错乱视为缺叶（调用方表没排序）
            return Err(CollectErr::MissingLeaf);
        }
        let pairs = [
            (0u8, b.eax, a.eax),
            (1, b.ebx, a.ebx),
            (2, b.ecx, a.ecx),
            (3, b.edx, a.edx),
        ];
        for (reg, bv, av) in pairs {
            if bv != av {
                entries.push(DiffEntry {
                    leaf: b.leaf,
                    subleaf: b.subleaf,
                    reg,
                    bsp: bv,
                    ap: av,
                });
            }
        }
    }
    Ok(DiffReport { entries, bsp_who: bsp.who, ap_who: ap.who })
}

// ─────────────────────────────────────────────────────────────────────
// 三、BSP 参考表（黄金基准）与 AP 探针夹具
// ─────────────────────────────────────────────────────────────────────

/// BSP 黄金特征行（确定值，来源：启动链登记快照的投影）。
pub fn golden_bsp_leaves() -> Vec<CpuidLeaf> {
    vec![
        CpuidLeaf { leaf: 0x0000_0000, subleaf: 0, eax: 0x0000_0016, ebx: 0x756E_6547, ecx: 0x6C65_746E, edx: 0x4965_6E69 },
        CpuidLeaf { leaf: 0x0000_0001, subleaf: 0, eax: 0x0009_06A3, ebx: 0x0010_0800, ecx: 0x7FFA_BBFF, edx: 0xBFEB_FBFF },
        CpuidLeaf { leaf: 0x0000_0007, subleaf: 0, eax: 0x0000_0000, ebx: 0x029C_6FBF, ecx: 0x0000_0000, edx: 0xBC10_0400 },
        CpuidLeaf { leaf: 0x8000_0000, subleaf: 0, eax: 0x8000_0008, ebx: 0, ecx: 0, edx: 0 },
        CpuidLeaf { leaf: 0x8000_0001, subleaf: 0, eax: 0, ebx: 0, ecx: 0x0000_0121, edx: 0x2C10_0800 },
        CpuidLeaf { leaf: 0x8000_0002, subleaf: 0, eax: 0x2020_2049, ebx: 0x4920_2020, ecx: 0x2020_2D36, edx: 0x3632_3531 },
    ]
}

/// 构造一个按黄金表应答的 probe（模拟同构 AP）。
pub fn homogeneous_probe() -> impl Fn(u32, u32) -> Option<CpuidLeaf> {
    let table = golden_bsp_leaves();
    move |leaf, _sub| {
        table
            .iter()
            .find(|l| l.leaf == leaf)
            .copied()
    }
}

/// 构造一个指定叶指定寄存器被篡改的 probe（模拟分叉 AP）。
pub fn divergent_probe(leaf: u32, reg: u8, new_val: u32) -> impl Fn(u32, u32) -> Option<CpuidLeaf> {
    let table = golden_bsp_leaves();
    move |l, _sub| {
        let mut row = table.iter().find(|x| x.leaf == l).copied()?;
        if row.leaf == leaf {
            match reg {
                0 => row.eax = new_val,
                1 => row.ebx = new_val,
                2 => row.ecx = new_val,
                3 => row.edx = new_val,
                _ => {}
            }
        }
        Some(row)
    }
}

/// 缺叶 probe（指定叶拒答）。
pub fn missing_leaf_probe(leaf: u32) -> impl Fn(u32, u32) -> Option<CpuidLeaf> {
    let table = golden_bsp_leaves();
    move |l, _sub| {
        if l == leaf {
            None
        } else {
            table.iter().find(|x| x.leaf == l).copied()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// 四、分叉账（异构时的逐叶留账）
// ─────────────────────────────────────────────────────────────────────

/// 分叉账条目：分叉 + 处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForkLedgerRow {
    /// 分叉明细。
    pub entry: DiffEntry,
    /// 是否入降级名单（true=该 AP 不收浮点/向量任务）。
    pub demoted: bool,
}

/// 分叉账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkLedger {
    pub rows: Vec<ForkLedgerRow>,
}

impl ForkLedger {
    /// 从 diff 报告建账：有分叉的 AP 全叶降级（保守纪律）。
    pub fn build(rpt: &DiffReport) -> ForkLedger {
        let rows = rpt
            .entries
            .iter()
            .map(|e| ForkLedgerRow { entry: *e, demoted: true })
            .collect();
        ForkLedger { rows }
    }

    /// 账行数与 diff 一致（账不丢项）。
    pub fn coherent(&self, rpt: &DiffReport) -> bool {
        self.rows.len() == rpt.entries.len()
    }
}

// ─────────────────────────────────────────────────────────────────────
// 四·五、多核巡检扫描（AP 全员重采集汇总）
// ─────────────────────────────────────────────────────────────────────

/// 单 AP 巡检结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApSweepRow {
    /// AP 身份。
    pub who: u32,
    /// 采集是否成功。
    pub collected: bool,
    /// 与 BSP 的分叉数（采集失败时为 0，另见 collected）。
    pub fork_count: usize,
}

/// 巡检汇总。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApSweep {
    pub rows: Vec<ApSweepRow>,
}

impl ApSweep {
    /// 全员同构：全部采集成功且零分叉。
    pub fn homogeneous(&self) -> bool {
        self.rows.iter().all(|r| r.collected && r.fork_count == 0)
    }

    /// 采集失败核名单。
    pub fn failed_ids(&self) -> Vec<u32> {
        self.rows
            .iter()
            .filter(|r| !r.collected)
            .map(|r| r.who)
            .collect()
    }
}

/// 对一组 AP probe 逐个采集并对 BSP diff，出巡检汇总。
pub fn sweep(bsp: &FeatureTable, aps: &[(u32, &dyn Fn(u32, u32) -> Option<CpuidLeaf>)]) -> ApSweep {
    let mut rows = Vec::new();
    for (who, probe) in aps {
        match collect(*who, probe) {
            Ok(t) => {
                let forks = match diff(bsp, &t) {
                    Ok(d) => d.entries.len(),
                    Err(_) => usize::MAX, // 表残缺按最大分叉计（保守降级）
                };
                rows.push(ApSweepRow { who: *who, collected: true, fork_count: forks });
            }
            Err(_) => {
                rows.push(ApSweepRow { who: *who, collected: false, fork_count: 0 });
            }
        }
    }
    ApSweep { rows }
}

// ─────────────────────────────────────────────────────────────────────
// 五、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0330 判据：AP 重采集与 BSP diff 零差异（同构断言）。
pub fn run_unx330_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0330 AP 特征二次采集与 BSP 差异断言");

    // J1 主判据：同构 AP 重采集 → diff 零差异（BSP 黄金表静态可证可采，失败走降级账）
    let bsp = match collect(0, &homogeneous_probe()) {
        Ok(t) => t,
        Err(e) => {
            cs.add("J1 同构 diff 零差异", false, e.detail());
            return cs;
        }
    };
    let ap = collect(2, &homogeneous_probe());
    match ap {
        Ok(t) => {
            let rpt = diff(&bsp, &t);
            match rpt {
                Ok(d) => {
                    cs.add("J1 同构 diff 零差异", d.homogeneous(), "同构断言");
                    cs.add("指纹逐叶一致",
                        bsp.leaves.iter().zip(t.leaves.iter())
                            .all(|(b, a)| b.fingerprint() == a.fingerprint()),
                        "FNV 逐叶");
                    let lg = ForkLedger::build(&d);
                    cs.add("零分叉账空", lg.rows.is_empty() && lg.coherent(&d), "账空");
                }
                Err(e) => cs.add("J1 同构 diff 零差异", false, e.detail()),
            }
        }
        Err(e) => cs.add("J1 同构 diff 零差异", false, e.detail()),
    }

    // BSP 参考表本身自洽：六叶闭集 + 有序
    cs.add("BSP 六叶闭集", bsp.leaves.len() == REQUIRED_LEAVES.len(), "必需叶齐");
    cs.add("BSP 表有序", bsp.leaves.windows(2).all(|w| w[0].leaf < w[1].leaf), "升序");
    cs.add("BSP 身份=0", bsp.who == 0, "BSP=0");

    // 注入 1：缺叶 → MissingLeaf
    let miss = collect(3, &missing_leaf_probe(0x0000_0007));
    cs.add("注入1 缺叶被拒", miss == Err(CollectErr::MissingLeaf), CollectErr::MissingLeaf.detail());

    // 注入 2：分叉 AP（leaf1 ecx 翻一位）→ diff 抓到且账一致
    let ap2 = collect(4, &divergent_probe(0x0000_0001, 2, 0x7FFA_BBFE));
    match ap2 {
        Ok(t) => match diff(&bsp, &t) {
            Ok(d) => {
                cs.add("注入2 分叉被抓", !d.homogeneous() && d.entries.len() == 1, "恰一处");
                if d.entries.len() == 1 {
                    let e0 = d.entries[0];
                    cs.add("注入2 定位准确",
                        e0.leaf == 0x0000_0001 && e0.reg == 2 && e0.bsp == 0x7FFA_BBFF && e0.ap == 0x7FFA_BBFE,
                        "叶+寄存器+双值");
                } else {
                    cs.add("注入2 定位准确", false, "分叉数异常");
                }
                let lg = ForkLedger::build(&d);
                cs.add("注入2 账不丢项", lg.coherent(&d) && lg.rows.iter().all(|r| r.demoted), "全降级");
            }
            Err(e) => cs.add("注入2 分叉被抓", false, e.detail()),
        },
        Err(e) => cs.add("注入2 分叉被抓", false, e.detail()),
    }

    // 注入 3：单寄存器分叉 → 恰一处（四寄存器全量对账语义）
    let ap3 = collect(5, &divergent_probe(0x0000_0001, 0, 0xDEAD_BEEF));
    match ap3 {
        Ok(t) => match diff(&bsp, &t) {
            Ok(d) => cs.add("注入3 单寄存器分叉=1", d.entries.len() == 1, "eax 一处"),
            Err(e) => cs.add("注入3 单寄存器分叉=1", false, e.detail()),
        },
        Err(e) => cs.add("注入3 单寄存器分叉=1", false, e.detail()),
    }

    // 注入 4：异构表对位错乱防护（BSP 与 AP 叶序不同 → 缺叶拒绝而非误 diff）
    let mut shuffled = golden_bsp_leaves();
    shuffled.swap(0, 1);
    let ap4 = FeatureTable { leaves: shuffled, who: 6 };
    cs.add("注入4 对位错乱拒diff", diff(&bsp, &ap4) == Err(CollectErr::MissingLeaf), "不静默误比");

    // 多核巡检：3 个同构 AP + 1 个分叉 AP + 1 个缺叶 AP
    let ap_a = homogeneous_probe();
    let ap_b = homogeneous_probe();
    let ap_c = homogeneous_probe();
    let ap_d = divergent_probe(0x0000_0001, 2, 0x7FFA_BBFE);
    let ap_e = missing_leaf_probe(0x8000_0002);
    let sw = sweep(
        &bsp,
        &[
            (10, &ap_a as &dyn Fn(u32, u32) -> Option<CpuidLeaf>),
            (11, &ap_b as &dyn Fn(u32, u32) -> Option<CpuidLeaf>),
            (12, &ap_c as &dyn Fn(u32, u32) -> Option<CpuidLeaf>),
            (13, &ap_d as &dyn Fn(u32, u32) -> Option<CpuidLeaf>),
            (14, &ap_e as &dyn Fn(u32, u32) -> Option<CpuidLeaf>),
        ],
    );
    cs.add("巡检5核行数=5", sw.rows.len() == 5, "全员在册");
    cs.add("巡检非同构", !sw.homogeneous(), "有分叉有缺叶");
    cs.add("巡检同构3核", sw.rows.iter().filter(|r| r.collected && r.fork_count == 0).count() == 3, "10/11/12");
    cs.add("巡检分叉1核", sw.rows.iter().filter(|r| r.collected && r.fork_count == 1).count() == 1, "13 分叉一处");
    cs.add("巡检缺叶1核", sw.failed_ids() == vec![14], "14 采集失败");
    cs.add("巡检失败不混入分叉", sw.rows.iter().filter(|r| !r.collected).all(|r| r.fork_count == 0), "两账分立");

    // 诊断码闭集互异
    let codes = [
        CollectErr::MissingLeaf, CollectErr::DupLeaf,
        CollectErr::TooManyLeaves, CollectErr::ProbeFail,
    ];
    cs.add("采集错误码互异",
        codes.iter().all(|a| codes.iter().all(|b| a == b || a.code() != b.code())),
        "四码闭集");

    cs
}
