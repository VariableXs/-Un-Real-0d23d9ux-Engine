//! UNX-F0458 ACPI 账对冲总账 · 0x75440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   四账（表清单/项账/登记段/交接字段）对冲平（表数=快照行=登记段
//!   数 三等），误差 0；注入（任一账单独虚增）必破平。
//! 设计要点：
//!   1. 四账建模：表清单账（签名序）/ 项账（快照行：签名+长度+校验
//!      状态+MADT 项数）/ 登记段账（签名+物理基址+长度）/ 交接字段
//!      账（表总数+MADT 项数直读）；
//!   2. 三等对冲：表清单数 = 快照行数 = 登记段数（12=12=12），且与
//!      交接 table_total 直读一致（四点一线）；
//!   3. MADT 对冲：项账 APIC 行携带 MADT 项数 24，与交接字段直读值
//!      一致（直读不改写）；
//!   4. 签名序对拍：三账签名序列逐位一致（防串位虚增）；
//!   5. 注入闭集：任一账单独虚增（清单 +1 / 快照 +1 / 登记段 +1 /
//!      交接 table_total +1）必破平，误差显性非 0；
//!   6. 空账边界：空四账数值上 0=0=0 平，但判无效账（空账不认绿）；
//!      双跑一致红线：同语料两轮对冲结果全等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75440，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75440;

/// 表数基准（四账对冲基准值）。
pub const TABLE_TOTAL: usize = 12;
/// MADT 项数基准（与项账 APIC 行及交接直读对齐）。
pub const MADT_ITEMS: u32 = 24;
/// 交接字段直读：表总数。
pub const HANDOFF_TABLE_TOTAL: u32 = 12;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconCode {
    /// 三等破平（表数/快照行/登记段数不等）
    ThreeWayBroken,
    /// MADT 对冲失配
    MadtMismatch,
    /// 交接直读失配（表总数/MADT 与账面不等）
    HandoffBad,
    /// 签名序对拍失配
    SigSeqMismatch,
    /// 对冲误差非 0
    ErrNonZero,
    /// 空账判平（空账不得认有效平账）
    EmptyAccount,
    /// 双跑不一致
    NotReproducible,
}

impl ReconCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ReconCode::ThreeWayBroken => "三等破平：表数/快照行/登记段数不等；建议逐账清点补齐",
            ReconCode::MadtMismatch => "MADT 对冲失配：项账 APIC 行与交接直读不等；建议按型重数",
            ReconCode::HandoffBad => "交接直读失配：table_total/MADT 与账面不等；建议重读交接字段",
            ReconCode::SigSeqMismatch => "签名序失配：三账签名序不齐；建议按清单序重排",
            ReconCode::ErrNonZero => "对冲误差非 0：存在虚增/漏记；建议以误差项定位",
            ReconCode::EmptyAccount => "空账判平：空账不得认有效平账；建议先装四账再对冲",
            ReconCode::NotReproducible => "双跑不一致：对冲两轮不等；建议冻结四账快照",
        }
    }
    pub const ALL: [ReconCode; 7] = [
        ReconCode::ThreeWayBroken,
        ReconCode::MadtMismatch,
        ReconCode::HandoffBad,
        ReconCode::SigSeqMismatch,
        ReconCode::ErrNonZero,
        ReconCode::EmptyAccount,
        ReconCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ReconCode::ALL.len() {
        if let Some(c) = ReconCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75440 段闭集内"
}

// ── 四账域模型 ──────────────────────────────────────────────────
/// 账一：表清单（签名序）。
pub type TableList = Vec<[u8; 4]>;

/// 账二：项账快照行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapRow {
    pub sig: [u8; 4],
    pub len: u32,
    pub csum_ok: bool,
    /// APIC 行携带 MADT 项数（其余行为 0）。
    pub madt_items: u32,
}

/// 账三：登记段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegRef {
    pub sig: [u8; 4],
    pub phys: u64,
    pub len: u32,
}

/// 账四：交接字段直读（只读投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconHandoff {
    pub table_total: u32,
    pub madt_items: u32,
}

/// 对冲结果（误差显性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReconResult {
    /// 三等是否成立。
    pub three_way: bool,
    /// MADT 对冲是否一致。
    pub madt_ok: bool,
    /// 交接直读是否一致。
    pub handoff_ok: bool,
    /// 签名序是否齐。
    pub seq_ok: bool,
    /// 对冲误差项数（0 = 平）。
    pub errs: u32,
}

impl ReconResult {
    /// 全平判定：三等 + MADT + 交接 + 签名序 + 误差 0。
    pub fn flat(&self) -> bool {
        self.three_way && self.madt_ok && self.handoff_ok && self.seq_ok && self.errs == 0
    }
}

/// 四账对冲核心（纯函数，不落账）。
pub fn reconcile(list: &TableList, rows: &[SnapRow], segs: &[SegRef], h: &ReconHandoff) -> ReconResult {
    let n1 = list.len();
    let n2 = rows.len();
    let n3 = segs.len();
    let three_way = n1 == n2 && n2 == n3;
    // 交接直读：表总数与三等基准一致，MADT 与项账 APIC 行一致。
    let mut madt_row: u32 = 0;
    for r in rows.iter() {
        if r.sig == *b"APIC" {
            madt_row = r.madt_items;
        }
    }
    let madt_ok = madt_row == h.madt_items;
    let handoff_ok = h.table_total == n1 as u32 && h.madt_items == MADT_ITEMS;
    // 签名序对拍（以最短长对位，防越界）。
    let mut seq_ok = three_way;
    let mut i = 0;
    while seq_ok && i < n1 && i < n2 && i < n3 {
        let a = match list.get(i) {
            Some(a) => *a,
            None => break,
        };
        let b = match rows.get(i) {
            Some(r) => r.sig,
            None => break,
        };
        let c = match segs.get(i) {
            Some(s) => s.sig,
            None => break,
        };
        if a != b || b != c {
            seq_ok = false;
        }
        i += 1;
    }
    // 误差项计数：三等差 + MADT 差 + 交接差 + 签名序差。
    let mut errs: u32 = 0;
    if !three_way {
        errs += (n1 as u32).saturating_sub(n2 as u32)
            + (n2 as u32).saturating_sub(n1 as u32)
            + (n2 as u32).saturating_sub(n3 as u32)
            + (n3 as u32).saturating_sub(n2 as u32);
        if errs == 0 {
            errs = 1;
        }
    }
    if !madt_ok {
        errs += 1;
    }
    if !handoff_ok {
        errs += 1;
    }
    if !seq_ok {
        errs += 1;
    }
    ReconResult { three_way, madt_ok, handoff_ok, seq_ok, errs }
}

// ── 机器语料装配（四账一致源） ──────────────────────────────────
pub const MACHINE_SIGS: [[u8; 4]; 12] = [
    *b"XSDT", *b"FADT", *b"APIC", *b"HPET", *b"MCFG", *b"SRAT",
    *b"SLIT", *b"BGRT", *b"DSDT", *b"FACS", *b"SSDT", *b"RSDT",
];

const MACHINE_LENS: [u32; 12] = [
    0x100, 0x200, 0x800, 0x80, 0x100, 0x180,
    0x140, 0x80, 0x400, 0x40, 0x300, 0xC0,
];

/// 账一：表清单（12 签名序）。
pub fn build_list() -> TableList {
    let mut v: TableList = Vec::new();
    for s in MACHINE_SIGS.iter() {
        v.push(*s);
    }
    v
}

/// 账二：项账快照（12 行，APIC 行带 MADT 项数 24）。
pub fn build_rows() -> Vec<SnapRow> {
    let mut v: Vec<SnapRow> = Vec::new();
    let mut i = 0;
    while i < MACHINE_SIGS.len() {
        let sig = match MACHINE_SIGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let len = match MACHINE_LENS.get(i) {
            Some(l) => *l,
            None => break,
        };
        let madt = if sig == *b"APIC" { MADT_ITEMS } else { 0 };
        v.push(SnapRow { sig, len, csum_ok: true, madt_items: madt });
        i += 1;
    }
    v
}

/// 账三：登记段（12 段，页对齐 4 页间隔）。
pub fn build_segs() -> Vec<SegRef> {
    let mut v: Vec<SegRef> = Vec::new();
    let mut i = 0;
    while i < MACHINE_SIGS.len() {
        let sig = match MACHINE_SIGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let len = match MACHINE_LENS.get(i) {
            Some(l) => *l,
            None => break,
        };
        let phys = 0x7F000000u64 + (i as u64) * 0x4000;
        v.push(SegRef { sig, phys, len });
        i += 1;
    }
    v
}

/// 账四：交接直读（12 表 / 24 MADT）。
pub fn build_handoff() -> ReconHandoff {
    ReconHandoff { table_total: HANDOFF_TABLE_TOTAL, madt_items: MADT_ITEMS }
}

/// 四账全装（一致源）。
pub fn build_all() -> (TableList, Vec<SnapRow>, Vec<SegRef>, ReconHandoff) {
    (build_list(), build_rows(), build_segs(), build_handoff())
}

// ── 注入闭集：任一账单独虚增 ────────────────────────────────────
/// 注入一：表清单虚增 1（多记一条 RSDT）。
pub fn inject_list() -> (TableList, Vec<SnapRow>, Vec<SegRef>, ReconHandoff) {
    let (mut l, r, s, h) = build_all();
    l.push(*b"RSDT");
    (l, r, s, h)
}
/// 注入二：快照行虚增 1。
pub fn inject_rows() -> (TableList, Vec<SnapRow>, Vec<SegRef>, ReconHandoff) {
    let (l, mut r, s, h) = build_all();
    r.push(SnapRow { sig: *b"RSDT", len: 0x40, csum_ok: true, madt_items: 0 });
    (l, r, s, h)
}
/// 注入三：登记段虚增 1。
pub fn inject_segs() -> (TableList, Vec<SnapRow>, Vec<SegRef>, ReconHandoff) {
    let (l, r, mut s, h) = build_all();
    s.push(SegRef { sig: *b"RSDT", phys: 0x7F0030000, len: 0x100 });
    (l, r, s, h)
}
/// 注入四：交接字段虚增（table_total +1）。
pub fn inject_handoff() -> (TableList, Vec<SnapRow>, Vec<SegRef>, ReconHandoff) {
    let (l, r, s, mut h) = build_all();
    h.table_total = h.table_total.saturating_add(1);
    (l, r, s, h)
}

/// 四路注入逐一确认破平（返回命中数，应 4）。
pub fn inject_all_broken() -> u32 {
    let mut hit = 0;
    let a = inject_list();
    if !reconcile(&a.0, &a.1, &a.2, &a.3).flat() {
        hit += 1;
    }
    let b = inject_rows();
    if !reconcile(&b.0, &b.1, &b.2, &b.3).flat() {
        hit += 1;
    }
    let c = inject_segs();
    if !reconcile(&c.0, &c.1, &c.2, &c.3).flat() {
        hit += 1;
    }
    let d = inject_handoff();
    if !reconcile(&d.0, &d.1, &d.2, &d.3).flat() {
        hit += 1;
    }
    hit
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    golden_run() == golden_run()
}

/// 黄金全链：装四账 → 对冲平（误差 0）→ 四路注入全破平 → 空账无效。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let (l, r, s, h) = build_all();
    let res = reconcile(&l, &r, &s, &h);
    if !res.flat() || res.errs != 0 {
        return Err(ReconCode::ErrNonZero.val());
    }
    let hit = inject_all_broken();
    if hit != 4 {
        return Err(ReconCode::ThreeWayBroken.val());
    }
    // 空四账：数值平但判无效。
    let empty = reconcile(&Vec::new(), &[], &[], &ReconHandoff { table_total: 0, madt_items: 0 });
    if empty.three_way != true || empty.flat() != false {
        return Err(ReconCode::EmptyAccount.val());
    }
    Ok((res.errs, hit))
}

/// 空账边界（0=0=0 数值平但 flat 判无效）。
pub fn empty_flat_invalid() -> bool {
    let e = reconcile(&Vec::new(), &[], &[], &ReconHandoff { table_total: 0, madt_items: 0 });
    e.three_way && e.madt_ok && !e.flat()
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in ReconCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn flat_ok() -> bool {
    let (l, r, s, h) = build_all();
    let res = reconcile(&l, &r, &s, &h);
    res.flat() && res.errs == 0
}
fn three_way_ok() -> bool {
    let (l, r, s, _) = build_all();
    l.len() == TABLE_TOTAL && r.len() == TABLE_TOTAL && s.len() == TABLE_TOTAL
}
fn handoff_read_ok() -> bool {
    let (_, _, _, h) = build_all();
    h.table_total == HANDOFF_TABLE_TOTAL && h.table_total == TABLE_TOTAL as u32
}
fn madt_direct_ok() -> bool {
    let (_, r, _, h) = build_all();
    let mut madt_row: u32 = 0;
    for row in r.iter() {
        if row.sig == *b"APIC" {
            madt_row = row.madt_items;
        }
    }
    madt_row == MADT_ITEMS && madt_row == h.madt_items
}
fn seq_ok() -> bool {
    let (l, r, s, _) = build_all();
    reconcile(&l, &r, &s, &build_handoff()).seq_ok
}
fn codeset_ok() -> bool {
    for c in ReconCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (l, r, s, h) = build_all();
    let res = reconcile(&l, &r, &s, &h);
    v.push((
        "J1-四账对冲平误差0",
        flat_ok(),
        format!("平={} 误差={}", res.flat(), res.errs),
    ));
    v.push((
        "J1-三等对冲12=12=12",
        three_way_ok() && res.three_way,
        format!("清单={} 快照={} 段={}", l.len(), r.len(), s.len()),
    ));
    v.push((
        "J1-交接表总数直读一致",
        handoff_read_ok(),
        format!("交接={}", h.table_total),
    ));
    v.push((
        "J1-MADT项数直读一致",
        madt_direct_ok(),
        format!("MADT={}", h.madt_items),
    ));
    v.push((
        "J1-签名序三账对拍",
        seq_ok() && res.seq_ok,
        "三账签名逐位齐".to_string(),
    ));
    v.push((
        "J1-注入表清单虚增必破",
        !reconcile(&inject_list().0, &inject_list().1, &inject_list().2, &inject_list().3).flat(),
        "清单+1 立破平".to_string(),
    ));
    v.push((
        "J1-注入快照行虚增必破",
        !reconcile(&inject_rows().0, &inject_rows().1, &inject_rows().2, &inject_rows().3).flat(),
        "快照+1 立破平".to_string(),
    ));
    v.push((
        "J1-注入登记段虚增必破",
        !reconcile(&inject_segs().0, &inject_segs().1, &inject_segs().2, &inject_segs().3).flat(),
        "登记段+1 立破平".to_string(),
    ));
    v.push((
        "J1-注入交接字段虚增必破",
        !reconcile(&inject_handoff().0, &inject_handoff().1, &inject_handoff().2, &inject_handoff().3).flat(),
        "交接+1 立破平".to_string(),
    ));
    v.push((
        "J1-四路注入汇总4/4",
        inject_all_broken() == 4,
        format!("破平={}/4", inject_all_broken()),
    ));
    v.push((
        "J2-空账边界",
        empty_flat_invalid(),
        "0=0=0 数值平但判无效".to_string(),
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
        format!("{} 码全查表命中", ReconCode::ALL.len()),
    ));
    v
}

pub fn run_unx0458_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0458 ACPI 账对冲总账");
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
    fn test_four_accounts_flat() {
        assert!(flat_ok());
        let (l, r, s, h) = build_all();
        let res = reconcile(&l, &r, &s, &h);
        assert!(res.flat());
        assert_eq!(res.errs, 0);
    }

    #[test]
    fn test_three_way_equal() {
        assert!(three_way_ok());
        assert_eq!(TABLE_TOTAL, 12);
    }

    #[test]
    fn test_handoff_direct_read() {
        assert!(handoff_read_ok());
        assert!(madt_direct_ok());
    }

    #[test]
    fn test_sig_sequence_aligned() {
        assert!(seq_ok());
        let (l, r, s, h) = build_all();
        assert!(reconcile(&l, &r, &s, &h).seq_ok);
    }

    #[test]
    fn test_injections_all_break_flat() {
        assert_eq!(inject_all_broken(), 4);
        let a = inject_list();
        assert!(!reconcile(&a.0, &a.1, &a.2, &a.3).flat());
        let b = inject_rows();
        assert!(!reconcile(&b.0, &b.1, &b.2, &b.3).flat());
        let c = inject_segs();
        assert!(!reconcile(&c.0, &c.1, &c.2, &c.3).flat());
        let d = inject_handoff();
        assert!(!reconcile(&d.0, &d.1, &d.2, &d.3).flat());
    }

    #[test]
    fn test_empty_account_invalid_flat() {
        assert!(empty_flat_invalid());
        let e = reconcile(&Vec::new(), &[], &[], &ReconHandoff { table_total: 0, madt_items: 0 });
        assert!(e.three_way);
        assert!(!e.flat());
    }

    #[test]
    fn test_err_count_positive_on_inflate() {
        let a = inject_list();
        let res = reconcile(&a.0, &a.1, &a.2, &a.3);
        assert!(!res.three_way);
        assert!(res.errs > 0);
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x7547F), "未知诊断码：不在 0x75440 段闭集内");
    }
}
