//! VE-F3761 · R09 开工与自查范围
//!
//! 锚点：R09 开工与自查范围——R 域自查组开工（O09/P09/Q09 模板第七次复用声明
//! ——家族治理流程复用）；范围声明（R01-R08 八组×20 项=160+组间 28 对对账+
//! 七维汇总（API/测试/性能/安全/无障碍/文档/预算）；R 特色维（双维无障碍拆分
//! （工具维+产出维各入色标（F3756 分册断言联动）+心流维（F3736 心流指标入色标
//! ——R 域三增维声明（增维登记红线复述：每增维声明理由与标准——工具无障碍/
//! 产出无障碍/心流三理由登记）；证据引用制复述。
//!
//! 数据结构：自查计划（组×维矩阵）；三增维声明；证据指针表。
//! 错误路径与降级矩阵：证据缺→阻断+回补（复述）；过期→增量复查（复述）；
//! 增维无标准→先立标准（复述红线实测）。
//! 跨批对接点：八组档案；O09/P09/Q09 四模板单源；R10 衔接。
//!
//! 诊断码段：独占 0x98xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x98xx）
// ─────────────────────────────────────────────────────────────

/// F3761 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R9Code(pub u16);

impl R9Code {
    /// 证据缺失——阻断。
    pub const EVIDENCE_MISS: R9Code = R9Code(0x9801);
    /// 增维无标准——先立标准红线。
    pub const DIM_NO_STD: R9Code = R9Code(0x9802);
    /// 计划范围不全——160+28+七维。
    pub const SCOPE_INCOMPLETE: R9Code = R9Code(0x9803);
    /// 证据过期——增量复查。
    pub const EVIDENCE_STALE: R9Code = R9Code(0x9804);
    /// 模板复用断言失败——家族治理。
    pub const TEMPLATE_DRIFT: R9Code = R9Code(0x9805);
    /// 增维理由缺失——登记红线。
    pub const DIM_NO_REASON: R9Code = R9Code(0x9806);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9801 => "证据缺失，阻断并需回补。",
            0x9802 => "增维无标准，先立标准再执行。",
            0x9803 => "自查计划范围不全（160+28+七维）。",
            0x9804 => "证据过期，需增量复查。",
            0x9805 => "家族模板复用漂移，需对账。",
            0x9806 => "增维理由缺失，登记红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 家族模板复用（O09/P09/Q09 第七次）
// ─────────────────────────────────────────────────────────────

/// 模板类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TplKind {
    /// O09（范围声明）。
    O09,
    /// P09（计划）。
    P09,
    /// Q09（核验）。
    Q09,
}

impl TplKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            TplKind::O09 => 0,
            TplKind::P09 => 1,
            TplKind::Q09 => 2,
        }
    }

    /// 全集。
    pub fn all() -> [TplKind; 3] {
        [TplKind::O09, TplKind::P09, TplKind::Q09]
    }
}

/// 模板复用登记。
#[derive(Clone, Debug, Default)]
pub struct TplReuse {
    /// 每模板已用次数。
    uses: [u32; 3],
    /// 是否单源对齐。
    aligned: bool,
}

impl TplReuse {
    /// 构造。
    pub fn new() -> TplReuse {
        TplReuse {
            uses: [0; 3],
            aligned: true,
        }
    }

    /// 记一次复用。
    pub fn reuse(&mut self, kind: TplKind) -> u32 {
        let i = kind.index() as usize;
        self.uses[i] += 1;
        self.uses[i]
    }

    /// 全部至少七次（第七次复用声明）。
    pub fn seven_use_ok(&self) -> bool {
        self.uses.iter().all(|n| *n >= 7)
    }

    /// 最少复用次数。
    pub fn min_use(&self) -> u32 {
        let mut m = u32::MAX;
        for n in self.uses.iter() {
            if *n < m {
                m = *n;
            }
        }
        if m == u32::MAX {
            0
        } else {
            m
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 自查范围（160 + 28 + 七维）
// ─────────────────────────────────────────────────────────────

/// 七维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditDim {
    /// API。
    Api,
    /// 测试。
    Test,
    /// 性能。
    Perf,
    /// 安全。
    Security,
    /// 无障碍。
    A11y,
    /// 文档。
    Doc,
    /// 预算。
    Budget,
}

impl AuditDim {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            AuditDim::Api => 0,
            AuditDim::Test => 1,
            AuditDim::Perf => 2,
            AuditDim::Security => 3,
            AuditDim::A11y => 4,
            AuditDim::Doc => 5,
            AuditDim::Budget => 6,
        }
    }

    /// 全集（七维）。
    pub fn all() -> [AuditDim; 7] {
        [
            AuditDim::Api,
            AuditDim::Test,
            AuditDim::Perf,
            AuditDim::Security,
            AuditDim::A11y,
            AuditDim::Doc,
            AuditDim::Budget,
        ]
    }
}

/// 自查范围。
#[derive(Clone, Debug, Default)]
pub struct AuditScope {
    /// 八组×20 项=160 核查项。
    pub group_items: u32,
    /// 28 对对账点。
    pub pair_checks: u32,
    /// 七维覆盖位。
    dims: [bool; 7],
}

impl AuditScope {
    /// 构造。
    pub fn new() -> AuditScope {
        AuditScope {
            group_items: 0,
            pair_checks: 0,
            dims: [false; 7],
        }
    }

    /// 登记组项。
    pub fn add_group_item(&mut self, n: u32) {
        self.group_items += n;
    }

    /// 登记对账。
    pub fn add_pair_check(&mut self, n: u32) {
        self.pair_checks += n;
    }

    /// 覆盖一维。
    pub fn cover_dim(&mut self, d: AuditDim) {
        self.dims[d.index() as usize] = true;
    }

    /// 范围完整：≥160 项 + ≥28 对 + 七维全覆。
    pub fn complete(&self) -> bool {
        self.group_items >= 160
            && self.pair_checks >= 28
            && self.dims.iter().all(|b| *b)
    }

    /// 缺维清单。
    pub fn missing_dims(&self) -> Vec<AuditDim> {
        let mut bad = Vec::new();
        for d in AuditDim::all() {
            if !self.dims[d.index() as usize] {
                bad.push(d);
            }
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// R 域三增维（双维无障碍 + 心流）
// ─────────────────────────────────────────────────────────────

/// 增维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtraDim {
    /// 工具无障碍。
    ToolA11y,
    /// 产出无障碍。
    OutputA11y,
    /// 心流。
    Flow,
}

impl ExtraDim {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            ExtraDim::ToolA11y => 0,
            ExtraDim::OutputA11y => 1,
            ExtraDim::Flow => 2,
        }
    }

    /// 全集（三增维）。
    pub fn all() -> [ExtraDim; 3] {
        [ExtraDim::ToolA11y, ExtraDim::OutputA11y, ExtraDim::Flow]
    }
}

/// 增维登记。
#[derive(Clone, Debug)]
pub struct ExtraDimDecl {
    /// 增维。
    pub dim: ExtraDim,
    /// 理由（空=缺失）。
    pub reason_ok: bool,
    /// 标准（空=缺失）。
    pub std_ok: bool,
}

impl ExtraDimDecl {
    /// 构造。
    pub fn new(dim: ExtraDim) -> ExtraDimDecl {
        ExtraDimDecl {
            dim,
            reason_ok: false,
            std_ok: false,
        }
    }

    /// 登记合规：有理由 + 有标准。
    pub fn registered_ok(&self) -> bool {
        self.reason_ok && self.std_ok
    }
}

/// 增维登记表。
#[derive(Clone, Debug, Default)]
pub struct ExtraDimRegistry {
    decls: Vec<ExtraDimDecl>,
}

impl ExtraDimRegistry {
    /// 空表。
    pub fn new() -> ExtraDimRegistry {
        ExtraDimRegistry { decls: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, d: ExtraDimDecl) {
        self.decls.push(d);
    }

    /// 扫描不合规。
    pub fn scan_bad(&self) -> Vec<ExtraDim> {
        let mut bad = Vec::new();
        for d in self.decls.iter() {
            if !d.registered_ok() {
                bad.push(d.dim);
            }
        }
        bad
    }

    /// 全三增维齐备且合规。
    pub fn all_three_ok(&self) -> bool {
        for dim in ExtraDim::all() {
            match self.decls.iter().find(|d| d.dim == dim) {
                Some(d) => {
                    if !d.registered_ok() {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────
// 证据引用
// ─────────────────────────────────────────────────────────────

/// 证据指针。
#[derive(Clone, Debug, Default)]
pub struct EvidencePtr {
    /// 证据 id。
    pub id: u32,
    /// 是否有效。
    pub valid: bool,
    /// 过期逻辑钟。
    pub ttl: u64,
}

impl EvidencePtr {
    /// 构造。
    pub fn new(id: u32, ttl: u64) -> EvidencePtr {
        EvidencePtr {
            id,
            valid: true,
            ttl,
        }
    }

    /// 过期判断。
    pub fn stale(&self, now: u64) -> bool {
        self.ttl > 0 && now >= self.ttl
    }
}

/// 证据表。
#[derive(Clone, Debug, Default)]
pub struct EvidenceTable {
    entries: Vec<EvidencePtr>,
}

impl EvidenceTable {
    /// 空表。
    pub fn new() -> EvidenceTable {
        EvidenceTable {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: EvidencePtr) {
        self.entries.push(e);
    }

    /// 扫描缺失。
    pub fn scan_invalid(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.valid {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描过期。
    pub fn scan_stale(&self, now: u64) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if e.stale(now) {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3761 判据集。
pub fn run_vr09a_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3761");

    // 0x9805 模板七用
    {
        let mut t = TplReuse::new();
        out.add("0x9805-初始不足七用", !t.seven_use_ok(), "");
        for _ in 0..7 {
            for k in TplKind::all() {
                t.reuse(k);
            }
        }
        out.add("0x9805-七用过", t.seven_use_ok() && t.min_use() >= 7, "");
    }

    // 0x9803 范围完整
    {
        let mut s = AuditScope::new();
        out.add("0x9803-空范围拦", !s.complete(), "");
        s.add_group_item(160);
        s.add_pair_check(28);
        for d in AuditDim::all() {
            s.cover_dim(d);
        }
        out.add("0x9803-160+28+七维过", s.complete() && s.missing_dims().is_empty(), "");
    }

    // 0x9802/0x9806 增维登记
    {
        let mut r = ExtraDimRegistry::new();
        let mut d1 = ExtraDimDecl::new(ExtraDim::ToolA11y);
        d1.reason_ok = true;
        d1.std_ok = true;
        r.add(d1);
        out.add("0x9806-未齐三增维", !r.all_three_ok(), "");
        let d2 = ExtraDimDecl::new(ExtraDim::OutputA11y);
        r.add(d2);
        out.add("0x9806-无理由检出", !r.scan_bad().is_empty(), "");
    }

    // 0x9801/0x9804 证据
    {
        let mut t = EvidenceTable::new();
        t.add(EvidencePtr::new(1, 100));
        out.add("0x9801-有效证据", t.scan_invalid().is_empty(), "");
        out.add("0x9804-未过期", t.scan_stale(50).is_empty(), "");
        out.add("0x9804-过期检出", t.scan_stale(150).len() == 1, "");
    }

    out
}
