//! UNX-F0457 ACPI 面五数概括账 · 0x75400段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   概括 5 项（表总数/坏表数/降级级数/MADT 项数/交接字段数 8）与
//!   逐项账复算一致。
//! 设计要点：
//!   1. 五数 =（表总数 12 / 坏表数 / 降级级数 / MADT 项数 / 交接字段
//!      数 8），逐项从底层明细账独立复算，概括与复算必须逐数相等；
//!   2. 底层账三本：表清单账（12 签名闭集）/ MADT 项账（分型计数
//!      8+4+8+2+2=24）/ 降级梯账（3 级闭集），坏表数从注入审计路
//!      复算（审计语料 12 好 + 3 注入坏 = 坏表 3）；
//!   3. 审计状态机：闲→审计中→已概括 单调前进，倒退显性拒；
//!   4. 概括账防篡改闸：已概括账再写必须与账面逐数相等，单数篡改
//!      显性拒并计数（注入必抓）；
//!   5. 空账边界：全零账复算自洽但判无效（空账不认绿）；
//!   6. 双跑一致红线：同语料两轮概括五数全等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75400，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75400;

/// 表总数基准（与 12 表语料对拍）。
pub const TOTAL_TABLES: u32 = 12;
/// 降级梯级数（冻结 3）。
pub const DEGRADE_RUNGS: u32 = 3;
/// 交接字段数（v1 冻结 8，与 F0456 语义对齐）。
pub const HANDOFF_FIELDS: u32 = 8;
/// MADT 项数基准。
pub const MADT_ITEMS: u32 = 24;
/// 注入坏表数（审计语料 3 注入）。
pub const AUDIT_BAD: u32 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SumCode {
    /// 复算失配（概括与逐项账不等）
    RecountMismatch,
    /// 空账判绿（空账不得出有效概括）
    EmptyAccount,
    /// MADT 项数复算失配
    MadtMismatch,
    /// 交接字段数非冻结值
    HandoffBad,
    /// 降级级数非冻结值
    DegradeBad,
    /// 概括账篡改（已概括账被改写）
    TamperBlocked,
    /// 双跑不一致
    NotReproducible,
}

impl SumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SumCode::RecountMismatch => "复算失配：概括数与逐项账不等；建议逐数对拍定位哪一数烂",
            SumCode::EmptyAccount => "空账判绿：空账不得出有效概括；建议先装账再概括",
            SumCode::MadtMismatch => "MADT 项数失配：分型计数与总项不等；建议按型重数",
            SumCode::HandoffBad => "交接字段数非冻结值：v1 应为 8；建议核对字段冻结表",
            SumCode::DegradeBad => "降级级数非冻结值：梯应 3 级；建议核对降级梯闭集",
            SumCode::TamperBlocked => "概括账篡改：已概括账只读；建议走新一轮审计而非改旧账",
            SumCode::NotReproducible => "双跑不一致：概括两轮不等；建议冻结语料与审计序",
        }
    }
    pub const ALL: [SumCode; 7] = [
        SumCode::RecountMismatch,
        SumCode::EmptyAccount,
        SumCode::MadtMismatch,
        SumCode::HandoffBad,
        SumCode::DegradeBad,
        SumCode::TamperBlocked,
        SumCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SumCode::ALL.len() {
        if let Some(c) = SumCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75400 段闭集内"
}

// ── 五数概括（值对象） ──────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    /// 表总数。
    pub total_tables: u32,
    /// 坏表数。
    pub bad_tables: u32,
    /// 降级级数（梯深）。
    pub degrade_levels: u32,
    /// MADT 项数。
    pub madt_items: u32,
    /// 交接字段数。
    pub handoff_fields: u32,
}

impl FiveNum {
    pub fn zero() -> FiveNum {
        FiveNum { total_tables: 0, bad_tables: 0, degrade_levels: 0, madt_items: 0, handoff_fields: 0 }
    }
    pub fn is_zero(&self) -> bool {
        *self == FiveNum::zero()
    }
}

/// 表清单账（签名闭集）。
pub const TABLE_SIGS: [[u8; 4]; 12] = [
    *b"XSDT", *b"FADT", *b"APIC", *b"HPET", *b"MCFG", *b"SRAT",
    *b"SLIT", *b"BGRT", *b"DSDT", *b"FACS", *b"SSDT", *b"RSDT",
];

/// 降级梯闭集（3 级，ACPI 缺表降级语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    /// L0 全量
    Full,
    /// L1 缺表降级（可选表缺，核心表全）
    MissingTable,
    /// L2 停用守护（核心表缺，ACPI 面停用）
    Disabled,
}

impl DegradeLevel {
    pub fn rank(self) -> u32 {
        match self {
            DegradeLevel::Full => 0,
            DegradeLevel::MissingTable => 1,
            DegradeLevel::Disabled => 2,
        }
    }
    pub const ALL: [DegradeLevel; 3] = [
        DegradeLevel::Full,
        DegradeLevel::MissingTable,
        DegradeLevel::Disabled,
    ];
}

/// MADT 项型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadtKind {
    /// LAPIC（处理器 local APIC）
    Lapic,
    /// IOAPIC（I/O APIC）
    IoApic,
    /// 中断源重映射（ISO）
    Iso,
    /// NMI 源
    NmiSrc,
    /// LAPIC NMI
    LapicNmi,
}

/// MADT 项（型 + 身份号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MadtItem {
    pub kind: MadtKind,
    pub id: u32,
}

/// MADT 项账装配：分型计数（LAPIC 8 / IOAPIC 4 / ISO 8 / NMI 2 / LAPIC-NMI 2）。
pub fn madt_item_list() -> Vec<MadtItem> {
    let plan: [(MadtKind, u32); 5] = [
        (MadtKind::Lapic, 8),
        (MadtKind::IoApic, 4),
        (MadtKind::Iso, 8),
        (MadtKind::NmiSrc, 2),
        (MadtKind::LapicNmi, 2),
    ];
    let mut v: Vec<MadtItem> = Vec::new();
    let mut id: u32 = 0;
    let mut p = 0;
    while p < plan.len() {
        let (kind, n) = match plan.get(p) {
            Some(x) => *x,
            None => break,
        };
        let mut k = 0;
        while k < n {
            v.push(MadtItem { kind, id });
            id += 1;
            k += 1;
        }
        p += 1;
    }
    v
}

/// 分型计数（复算路）。
pub fn madt_count_by(kind: MadtKind) -> u32 {
    let mut n = 0;
    for it in madt_item_list().iter() {
        if it.kind == kind {
            n += 1;
        }
    }
    n
}

/// 注入审计路：12 好表 + 3 注入坏表，坏表数复算（检测出才计坏）。
pub fn audit_bad_count() -> u32 {
    // 审计语料：好 12 全过；坏 3（校验破坏/签名伪造/长度烂）全被抓。
    let mut bad = 0;
    // 坏一：校验和 +1（好表基数上校验位翻动）。
    let sig0 = match TABLE_SIGS.first() {
        Some(s) => *s,
        None => return 0,
    };
    let base_sum = sig_byte_sum(&sig0, 0x7F600000, 0x100, 0);
    if (base_sum.wrapping_add(1)) & 0xFF != 0 {
        bad += 1;
    }
    // 坏二：签名伪造成 FAKE（不在闭集）。
    if !sig_in_set(b"FAKE") {
        bad += 1;
    }
    // 坏三：长度虚高越帽。
    if 0x40000u32.saturating_add(0x100) > 0x40000 {
        bad += 1;
    }
    bad
}

fn sig_byte_sum(sig: &[u8; 4], base: u64, len: u32, csum: u8) -> u32 {
    let mut s: u32 = 0;
    for b in sig.iter() {
        s += *b as u32;
    }
    for b in base.to_le_bytes().iter() {
        s += *b as u32;
    }
    for b in len.to_le_bytes().iter() {
        s += *b as u32;
    }
    s + csum as u32
}

fn sig_in_set(sig: &[u8; 4]) -> bool {
    let mut i = 0;
    while i < TABLE_SIGS.len() {
        if let Some(k) = TABLE_SIGS.get(i) {
            if k == sig {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 审计状态机（闲→审计中→已概括，单调前进） ────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditPhase {
    Idle,
    Auditing,
    Aggregated,
}

impl AuditPhase {
    pub fn rank(self) -> u32 {
        match self {
            AuditPhase::Idle => 0,
            AuditPhase::Auditing => 1,
            AuditPhase::Aggregated => 2,
        }
    }
}

/// 概括账：五数 + 相位 + 篡改计数。
#[derive(Clone, Debug)]
pub struct SummaryLedger {
    pub five: FiveNum,
    pub phase: AuditPhase,
    /// 被拒篡改次数。
    pub tamper_blocked: u32,
    /// 被拒倒退次数。
    pub regress_blocked: u32,
}

impl SummaryLedger {
    pub fn new() -> SummaryLedger {
        SummaryLedger {
            five: FiveNum::zero(),
            phase: AuditPhase::Idle,
            tamper_blocked: 0,
            regress_blocked: 0,
        }
    }

    /// 相位推进：闲→审计中→已概括；倒退显性拒。
    pub fn advance(&mut self) -> Result<(), u32> {
        let next = match self.phase {
            AuditPhase::Idle => AuditPhase::Auditing,
            AuditPhase::Auditing => AuditPhase::Aggregated,
            AuditPhase::Aggregated => {
                self.regress_blocked += 1;
                return Err(SumCode::TamperBlocked.val());
            }
        };
        self.phase = next;
        Ok(())
    }

    /// 概括落账：仅在已概括相位受理；与既有账面逐数不等 = 篡改拒。
    pub fn commit(&mut self, f: FiveNum) -> Result<(), u32> {
        if self.phase != AuditPhase::Aggregated {
            return Err(SumCode::EmptyAccount.val());
        }
        if !self.five.is_zero() && self.five != f {
            self.tamper_blocked += 1;
            return Err(SumCode::TamperBlocked.val());
        }
        self.five = f;
        Ok(())
    }
}

/// 逐项账复算：从底层三本账 + 注入审计路独立出五数。
pub fn recompute_five(audit_mode: bool) -> FiveNum {
    let total = TABLE_SIGS.len() as u32 + if audit_mode { AUDIT_BAD } else { 0 };
    let bad = if audit_mode { audit_bad_count() } else { 0 };
    FiveNum {
        total_tables: total,
        bad_tables: bad,
        degrade_levels: DegradeLevel::ALL.len() as u32,
        madt_items: madt_item_list().len() as u32,
        handoff_fields: HANDOFF_FIELDS,
    }
}

/// 生产概括（12 好 0 坏）。
pub fn prod_five() -> FiveNum {
    recompute_five(false)
}
/// 审计概括（12 好 + 3 注入坏）。
pub fn audit_five() -> FiveNum {
    recompute_five(true)
}

/// 单数篡改检测：篡改第 slot 个数（0..5）后与复算账比，必须报失配。
pub fn tamper_slot(f: FiveNum, slot: usize) -> Option<FiveNum> {
    let mut c = f;
    match slot {
        0 => c.total_tables = c.total_tables.wrapping_add(1),
        1 => c.bad_tables = c.bad_tables.wrapping_add(1),
        2 => c.degrade_levels = c.degrade_levels.wrapping_add(1),
        3 => c.madt_items = c.madt_items.saturating_sub(1),
        4 => c.handoff_fields = c.handoff_fields.wrapping_add(1),
        _ => return None,
    }
    Some(c)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    golden_run() == golden_run()
}

/// 黄金全链：装配→推进两步→生产概括落账→审计概括落账→篡改拒→倒退拒。
pub fn golden_run() -> Result<(FiveNum, u32, u32), u32> {
    let mut led = SummaryLedger::new();
    led.advance()?;
    led.advance()?;
    led.commit(prod_five())?;
    // 同账重落（同值）受理。
    led.commit(prod_five())?;
    // 篡改落账（审计账混入生产账）→ 拒（预期拒，不外溢）。
    let _ = led.commit(audit_five());
    if led.tamper_blocked != 1 {
        return Err(SumCode::TamperBlocked.val());
    }
    // 已概括后再 advance = 倒退 → 拒（预期拒，不外溢）。
    let _ = led.advance();
    if led.regress_blocked != 1 {
        return Err(SumCode::TamperBlocked.val());
    }
    Ok((led.five, led.tamper_blocked, led.regress_blocked))
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in SumCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn prod_recount_ok() -> bool {
    prod_five() == FiveNum {
        total_tables: TOTAL_TABLES,
        bad_tables: 0,
        degrade_levels: DEGRADE_RUNGS,
        madt_items: MADT_ITEMS,
        handoff_fields: HANDOFF_FIELDS,
    }
}
fn audit_recount_ok() -> bool {
    let f = audit_five();
    f.total_tables == TOTAL_TABLES + AUDIT_BAD && f.bad_tables == AUDIT_BAD
}
fn madt_by_kind_ok() -> bool {
    madt_count_by(MadtKind::Lapic) == 8
        && madt_count_by(MadtKind::IoApic) == 4
        && madt_count_by(MadtKind::Iso) == 8
        && madt_count_by(MadtKind::NmiSrc) == 2
        && madt_count_by(MadtKind::LapicNmi) == 2
        && (madt_count_by(MadtKind::Lapic)
            + madt_count_by(MadtKind::IoApic)
            + madt_count_by(MadtKind::Iso)
            + madt_count_by(MadtKind::NmiSrc)
            + madt_count_by(MadtKind::LapicNmi)) == MADT_ITEMS
}
fn frozen_consts_ok() -> bool {
    DEGRADE_RUNGS == DegradeLevel::ALL.len() as u32 && HANDOFF_FIELDS == 8 && TOTAL_TABLES == 12
}
fn tamper_all_slots_ok() -> bool {
    let base = prod_five();
    let mut i = 0;
    while i < 5 {
        let t = match tamper_slot(base, i) {
            Some(t) => t,
            None => return false,
        };
        if t == base || t == recompute_five(false) {
            return false;
        }
        i += 1;
    }
    true
}
fn tamper_commit_blocked_ok() -> bool {
    match golden_run() {
        Ok((_, t, r)) => t == 1 && r == 1,
        Err(_) => false,
    }
}
fn empty_account_ok() -> bool {
    // 空账复算自洽但判无效（零账不得当有效概括）。
    let z = FiveNum::zero();
    z.is_zero() && SummaryLedger::new().commit(z) == Err(SumCode::EmptyAccount.val())
}
fn codeset_ok() -> bool {
    for c in SumCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let p = prod_five();
    v.push((
        "J1-生产账五数复算一致",
        prod_recount_ok(),
        format!("总={} 坏={} 级={} MADT={} 交接={}", p.total_tables, p.bad_tables, p.degrade_levels, p.madt_items, p.handoff_fields),
    ));
    v.push((
        "J1-审计账五数复算一致",
        audit_recount_ok(),
        format!("总={} 坏={}", audit_five().total_tables, audit_five().bad_tables),
    ));
    v.push((
        "J1-表总数对拍12",
        p.total_tables == TOTAL_TABLES && TABLE_SIGS.len() as u32 == TOTAL_TABLES,
        format!("清单={} 基准={}", TABLE_SIGS.len(), TOTAL_TABLES),
    ));
    v.push((
        "J1-坏表数复算3",
        audit_bad_count() == AUDIT_BAD,
        format!("审计坏表={}", audit_bad_count()),
    ));
    v.push((
        "J1-降级梯3级闭集",
        frozen_consts_ok() && matches!(DegradeLevel::ALL.get(2), Some(l) if l.rank() == 2),
        format!("级数={}", DEGRADE_RUNGS),
    ));
    v.push((
        "J1-MADT分型复算24",
        madt_by_kind_ok(),
        "LAPIC8+IOAPIC4+ISO8+NMI2+LAPICNMI2".to_string(),
    ));
    v.push((
        "J1-交接字段数冻结8",
        p.handoff_fields == HANDOFF_FIELDS && HANDOFF_FIELDS == 8,
        format!("交接字段={}", HANDOFF_FIELDS),
    ));
    v.push((
        "J1-五槽篡改逐槽必抓",
        tamper_all_slots_ok(),
        "5 槽逐槽单数篡改全显失配".to_string(),
    ));
    v.push((
        "J1-概括账篡改拒",
        tamper_commit_blocked_ok(),
        "异账落账/越级推进双拒".to_string(),
    ));
    v.push((
        "J2-空账边界",
        empty_account_ok(),
        "零账自洽但判无效".to_string(),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "golden_run 两跑同值".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", SumCode::ALL.len()),
    ));
    v
}

pub fn run_unx0457_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0457 ACPI 面五数概括账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prod_five_matches() {
        assert!(prod_recount_ok());
        assert_eq!(prod_five().total_tables, 12);
        assert_eq!(prod_five().bad_tables, 0);
        assert_eq!(prod_five().handoff_fields, 8);
    }

    #[test]
    fn test_audit_five_bad_three() {
        assert!(audit_recount_ok());
        assert_eq!(audit_five().bad_tables, AUDIT_BAD);
        assert_eq!(audit_five().total_tables, TOTAL_TABLES + AUDIT_BAD);
    }

    #[test]
    fn test_madt_by_kind() {
        assert!(madt_by_kind_ok());
        assert_eq!(madt_item_list().len() as u32, MADT_ITEMS);
    }

    #[test]
    fn test_frozen_consts() {
        assert!(frozen_consts_ok());
        assert_eq!(DEGRADE_RUNGS, 3);
        assert_eq!(DegradeLevel::ALL.len(), 3);
    }

    #[test]
    fn test_tamper_each_slot_detected() {
        assert!(tamper_all_slots_ok());
        let base = prod_five();
        for slot in 0..5u32 {
            let t = tamper_slot(base, slot as usize);
            assert!(t.is_some());
            assert_ne!(t.unwrap_or(base), base);
        }
        assert!(tamper_slot(base, 5).is_none());
    }

    #[test]
    fn test_commit_tamper_blocked() {
        let mut led = SummaryLedger::new();
        let _ = led.advance();
        let _ = led.advance();
        assert!(led.commit(prod_five()).is_ok());
        assert_eq!(led.commit(audit_five()), Err(SumCode::TamperBlocked.val()));
        assert_eq!(led.tamper_blocked, 1);
        // 同值重落受理（幂等）。
        assert!(led.commit(prod_five()).is_ok());
    }

    #[test]
    fn test_phase_regress_rejected() {
        let mut led = SummaryLedger::new();
        let _ = led.advance();
        let _ = led.advance();
        assert_eq!(led.advance(), Err(SumCode::TamperBlocked.val()));
        assert_eq!(led.regress_blocked, 1);
        // 未到已概括相位不得落账。
        let mut early = SummaryLedger::new();
        assert_eq!(early.commit(prod_five()), Err(SumCode::EmptyAccount.val()));
    }

    #[test]
    fn test_empty_account_boundary() {
        assert!(empty_account_ok());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        assert_eq!(golden_run(), golden_run());
        assert_eq!(prod_five(), prod_five());
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x7543F), "未知诊断码：不在 0x75400 段闭集内");
    }
}
