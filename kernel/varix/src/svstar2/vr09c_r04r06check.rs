//! VE-F3763 · R04-R06 组自查核验
//!
//! 锚点：R04-R06 组自查核验——三组联合核验（R04 个性化：覆盖禁令 P0/四权可达/
//! 独立解析/同步关闭审计——20 项；R05 生态：出售 P0/受损 P0/钱对账/公示/独立
//! P0——20 项；R06 质量：同源 P0/本地扫描 P0/偏科断言/对拍 10%——20 项；三段
//! 核验复刻）；特色专项（隐私 P0 群核验（画像上云/过度收集/出售/资源化/云端扫描
//! ——R 域隐私 P0 群的网络审计证据核验（隐私红线复述；经济对账核验（结算对账
//! 全量月审记录核验（钱红线复述联动。
//!
//! 数据结构：三组核验表（60 项）；隐私 P0 网络审计核验；对账记录核验。
//! 跨批对接点：三组档案；F3697/F3715 对端；F3765 输入。
//!
//! 诊断码段：独占 0x9Axx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Axx）
// ─────────────────────────────────────────────────────────────

/// F3763 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V9Code(pub u16);

impl V9Code {
    /// 隐私 P0 漏检——实测红线。
    pub const PRIVACY_P0_MISS: V9Code = V9Code(0x9A01);
    /// 对账缺——钱红线。
    pub const RECONCILE_MISS: V9Code = V9Code(0x9A02);
    /// 60 项缺——补核验。
    pub const ITEM_GAP: V9Code = V9Code(0x9A03);
    /// 三段缺——复刻。
    pub const PHASE_MISS: V9Code = V9Code(0x9A04);
    /// 网络审计缺——隐私红线。
    pub const NET_AUDIT_MISS: V9Code = V9Code(0x9A05);
    /// 分歧未立案。
    pub const CONFLICT_OPEN: V9Code = V9Code(0x9A06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9A01 => "隐私 P0 群漏检，网络审计实测红线不通过。",
            0x9A02 => "经济对账缺失，钱红线不通过。",
            0x9A03 => "三组 60 项核验有缺口。",
            0x9A04 => "三段核验缺段，复刻不完整。",
            0x9A05 => "网络审计证据缺失。",
            0x9A06 => "核验分歧未立案。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三组核验（R04/R05/R06 × 20 项）
// ─────────────────────────────────────────────────────────────

/// 组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R456Group {
    /// R04 个性化。
    R04,
    /// R05 生态。
    R05,
    /// R06 质量。
    R06,
}

impl R456Group {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            R456Group::R04 => 0,
            R456Group::R05 => 1,
            R456Group::R06 => 2,
        }
    }

    /// 全集。
    pub fn all() -> [R456Group; 3] {
        [R456Group::R04, R456Group::R05, R456Group::R06]
    }
}

/// 组核验表。
#[derive(Clone, Debug)]
pub struct G456Check {
    checked: [u32; 3],
    phases: [[bool; 3]; 3],
}

impl G456Check {
    /// 空表。
    pub fn new() -> G456Check {
        G456Check {
            checked: [0; 3],
            phases: [[false; 3]; 3],
        }
    }

    /// 记一项。
    pub fn check_item(&mut self, g: R456Group) -> u32 {
        let i = g.index() as usize;
        self.checked[i] += 1;
        self.checked[i]
    }

    /// 记一段。
    pub fn mark_phase(&mut self, g: R456Group, phase: u8) -> bool {
        if phase >= 3 {
            return false;
        }
        self.phases[g.index() as usize][phase as usize] = true;
        true
    }

    /// 60 项齐。
    pub fn all_complete(&self) -> bool {
        self.total_items() >= 60
    }

    /// 总项。
    pub fn total_items(&self) -> u32 {
        self.checked.iter().sum()
    }

    /// 三段齐。
    pub fn all_phases(&self) -> bool {
        for row in self.phases.iter() {
            if !row.iter().all(|b| *b) {
                return false;
            }
        }
        true
    }
}

impl Default for G456Check {
    fn default() -> G456Check {
        G456Check::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 隐私 P0 群（五隐私 P0 + 网络审计）
// ─────────────────────────────────────────────────────────────

/// 隐私 P0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivacyP0 {
    /// 画像上云。
    ProfileCloud,
    /// 过度收集。
    OverCollect,
    /// 出售数据。
    SellData,
    /// 资源化。
    Monetize,
    /// 云端扫描。
    CloudScan,
}

impl PrivacyP0 {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            PrivacyP0::ProfileCloud => 0,
            PrivacyP0::OverCollect => 1,
            PrivacyP0::SellData => 2,
            PrivacyP0::Monetize => 3,
            PrivacyP0::CloudScan => 4,
        }
    }

    /// 全集（五隐私 P0）。
    pub fn all() -> [PrivacyP0; 5] {
        [
            PrivacyP0::ProfileCloud,
            PrivacyP0::OverCollect,
            PrivacyP0::SellData,
            PrivacyP0::Monetize,
            PrivacyP0::CloudScan,
        ]
    }
}

/// 网络审计记录。
#[derive(Clone, Debug)]
pub struct NetAuditRecord {
    /// 隐私 P0。
    pub kind: PrivacyP0,
    /// 是否有网络审计证据。
    pub audited: bool,
    /// 是否无违规。
    pub clean: bool,
}

impl NetAuditRecord {
    /// 构造。
    pub fn new(kind: PrivacyP0) -> NetAuditRecord {
        NetAuditRecord {
            kind,
            audited: false,
            clean: true,
        }
    }

    /// 合规：有审计 + 无违规。
    pub fn ok(&self) -> bool {
        self.audited && self.clean
    }
}

/// 隐私 P0 审计器。
#[derive(Clone, Debug, Default)]
pub struct PrivacyAuditor {
    records: Vec<NetAuditRecord>,
}

impl PrivacyAuditor {
    /// 空。
    pub fn new() -> PrivacyAuditor {
        PrivacyAuditor {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, r: NetAuditRecord) {
        self.records.push(r);
    }

    /// 扫描漏检（无审计或有违规）。
    pub fn scan_miss(&self) -> Vec<PrivacyP0> {
        let mut bad = Vec::new();
        for r in self.records.iter() {
            if !r.ok() {
                bad.push(r.kind);
            }
        }
        bad
    }

    /// 五 P0 齐。
    pub fn all_registered(&self) -> bool {
        for k in PrivacyP0::all() {
            if !self.records.iter().any(|r| r.kind == k) {
                return false;
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────
// 经济对账（月审记录）
// ─────────────────────────────────────────────────────────────

/// 月审记录。
#[derive(Clone, Debug)]
pub struct MonthlyReconcile {
    /// 月份。
    pub month: u32,
    /// 是否全量对账。
    pub full: bool,
    /// 差额（0=平账）。
    pub delta: i64,
}

impl MonthlyReconcile {
    /// 构造。
    pub fn new(month: u32, full: bool, delta: i64) -> MonthlyReconcile {
        MonthlyReconcile {
            month,
            full,
            delta,
        }
    }

    /// 合规：全量 + 平账。
    pub fn ok(&self) -> bool {
        self.full && self.delta == 0
    }
}

/// 对账台账。
#[derive(Clone, Debug, Default)]
pub struct ReconcileLedger {
    records: Vec<MonthlyReconcile>,
}

impl ReconcileLedger {
    /// 空。
    pub fn new() -> ReconcileLedger {
        ReconcileLedger {
            records: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, r: MonthlyReconcile) {
        self.records.push(r);
    }

    /// 扫描不合规。
    pub fn scan_bad(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for r in self.records.iter() {
            if !r.ok() {
                bad.push(r.month);
            }
        }
        bad
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3763 判据集。
pub fn run_vr09c_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3763");

    // 0x9A03 60 项
    {
        let mut g = G456Check::new();
        out.add("0x9A03-空表不全", !g.all_complete(), "");
        for _ in 0..20 {
            for grp in R456Group::all() {
                g.check_item(grp);
            }
        }
        out.add("0x9A03-60项齐", g.all_complete(), "");
    }

    // 0x9A04 三段
    {
        let mut g = G456Check::new();
        for grp in R456Group::all() {
            for ph in 0..3u8 {
                g.mark_phase(grp, ph);
            }
        }
        out.add("0x9A04-三段齐", g.all_phases(), "");
    }

    // 0x9A01/0x9A05 隐私 P0
    {
        let mut a = PrivacyAuditor::new();
        let mut r = NetAuditRecord::new(PrivacyP0::ProfileCloud);
        r.audited = false;
        a.add(r);
        out.add("0x9A05-无审计检出", a.scan_miss().len() == 1, "");
        for k in PrivacyP0::all() {
            let mut c = NetAuditRecord::new(k);
            c.audited = true;
            c.clean = true;
            a.add(c);
        }
        out.add("0x9A01-五P0齐", a.all_registered(), "");
    }

    // 0x9A02 经济对账
    {
        let mut l = ReconcileLedger::new();
        l.add(MonthlyReconcile::new(1, true, 0));
        out.add("0x9A02-平账过", l.scan_bad().is_empty(), "");
        l.add(MonthlyReconcile::new(2, true, 100));
        out.add("0x9A02-差额检出", l.scan_bad().len() == 1, "");
    }

    out
}
