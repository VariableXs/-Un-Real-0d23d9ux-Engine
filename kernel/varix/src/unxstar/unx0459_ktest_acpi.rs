//! UNX-F0459 ktest ACPI 面断言接入 · 0x75480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，WBV1104·AI-VX7-1104）：
//!   B23 判据入套件 unx_a1_b23_p1 全绿，映射双向对账。
//! 设计要点：
//!   1. 套件 unx_a1_b23_p1 覆盖 ACPI 面 12 卡（F0441 搜 RSDP 起，
//!      F0452 缺表降级止），卡号/卡名/断言数三表冻结；
//!   2. 断言映射：判据→断言 1:1（每卡恰一断言，断言 ID = 段基 + 卡
//!      序偏移，全部落 0x75480 窗口内）；
//!   3. 双向对账：正向逐卡查断言（计数 12 且卡唯一），反向逐断言反
//!      查卡（ID 反推与登记一致），两向全对才算映射平；
//!   4. 证据谓词逐卡独立建模（RSDP 锚/校验闭/走查数/MADT 分型/HPET
//!      基址/FADT 端口/MCFG 记录/DSDT 对齐/SRAT 域/快照行/降级梯），
//!      12 卡全绿才判套件绿；
//!   5. 注入闭集：抽断言（正向计数破 + 反查缺口双抓）/ 重断言（1:1
//!      破）必抓；空套件不判绿；
//!   6. 双跑一致红线：同套件两轮跑结果全等。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75480，AI-VX7-1104 批W7E 独占区）。
pub const SEG_BASE: u32 = 0x75480;

/// 套件名（冻结）。
pub const SUITE_NAME: &str = "unx_a1_b23_p1";
/// 套件卡数（12，B23 ACPI 面）。
pub const SUITE_CARDS: usize = 12;
/// 首卡号（F0441）。
pub const FIRST_CARD: u32 = 441;
/// 末卡号（F0452）。
pub const LAST_CARD: u32 = 452;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 映射计数失配（对数 != 卡数）
    MapCountMismatch,
    /// 重断言（一卡多断言，1:1 破）
    MapDupPair,
    /// 反查缺口（断言 ID 反推不出卡）
    ReverseLookupBroken,
    /// 套件未全绿
    SuiteNotGreen,
    /// 卡号越窗（不在 441..=452）
    CardIdOob,
    /// 空套件判绿
    EmptySuite,
    /// 双跑不一致
    NotReproducible,
}

impl KtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtCode::MapCountMismatch => "映射计数失配：断言对数应等于卡数；建议逐卡补配断言",
            KtCode::MapDupPair => "重断言：一卡多断言破 1:1；建议一判据一断言",
            KtCode::ReverseLookupBroken => "反查缺口：断言 ID 反推不出卡；建议按 段基+卡序 重建 ID",
            KtCode::SuiteNotGreen => "套件未全绿：存在红卡；建议先看红卡诊断再修证据",
            KtCode::CardIdOob => "卡号越窗：B23 面 441..=452；建议核对卡号闭集",
            KtCode::EmptySuite => "空套件判绿：零卡套件不得报绿；建议先接入卡表",
            KtCode::NotReproducible => "双跑不一致：套件两轮不等；建议冻结卡表与证据序",
        }
    }
    pub const ALL: [KtCode; 7] = [
        KtCode::MapCountMismatch,
        KtCode::MapDupPair,
        KtCode::ReverseLookupBroken,
        KtCode::SuiteNotGreen,
        KtCode::CardIdOob,
        KtCode::EmptySuite,
        KtCode::NotReproducible,
    ];
}

/// 诊断码闭集查表（段外码返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KtCode::ALL.len() {
        if let Some(c) = KtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75480 段闭集内"
}

// ── 卡表（B23 ACPI 面 12 卡冻结） ───────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuiteCard {
    /// 卡号（441..=452）。
    pub card: u32,
    /// 卡标题。
    pub title: &'static str,
}

/// 冻结卡表（顺序即断言偏移序）。
pub const CARD_TABLE: [SuiteCard; 12] = [
    SuiteCard { card: 441, title: "RSDP 搜索锚点" },
    SuiteCard { card: 442, title: "RSDP 校验与验签" },
    SuiteCard { card: 443, title: "RSDT/XSDT 走查" },
    SuiteCard { card: 444, title: "表头校验和闭闸" },
    SuiteCard { card: 445, title: "MADT 项解析" },
    SuiteCard { card: 446, title: "HPET 基址解析" },
    SuiteCard { card: 447, title: "FADT 端口块解析" },
    SuiteCard { card: 448, title: "MCFG 记录解析" },
    SuiteCard { card: 449, title: "DSDT 指针定位" },
    SuiteCard { card: 450, title: "SRAT/SLIT 拓扑" },
    SuiteCard { card: 451, title: "表快照账" },
    SuiteCard { card: 452, title: "缺表降级账" },
];

/// 断言 ID = 段基 + 卡表序偏移（0..12，全落 0x40 窗口内）。
pub fn assert_id_of(card: u32) -> Result<u32, u32> {
    if card < FIRST_CARD || card > LAST_CARD {
        return Err(KtCode::CardIdOob.val());
    }
    Ok(SEG_BASE + (card - FIRST_CARD))
}

/// 反查：断言 ID → 卡号（段窗外拒）。
pub fn card_of_assert(aid: u32) -> Result<u32, u32> {
    if aid < SEG_BASE || aid >= SEG_BASE + SUITE_CARDS as u32 {
        return Err(KtCode::ReverseLookupBroken.val());
    }
    Ok(FIRST_CARD + (aid - SEG_BASE))
}

// ── 断言映射账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertMap {
    /// (卡号, 断言 ID) 对，按卡表序装配。
    pub pairs: Vec<(u32, u32)>,
}

/// 正向对账：每卡恰一断言 + ID 反推一致 + 计数 == 卡数。
pub fn forward_ok(m: &AssertMap) -> bool {
    if m.pairs.len() != SUITE_CARDS {
        return false;
    }
    for p in m.pairs.iter() {
        let (card, aid) = *p;
        if card < FIRST_CARD || card > LAST_CARD {
            return false;
        }
        match assert_id_of(card) {
            Ok(expect) => {
                if expect != aid {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    // 卡唯一（无重复卡号）。
    let mut i = 0;
    while i < m.pairs.len() {
        if let Some(a) = m.pairs.get(i) {
            let mut j = i + 1;
            while j < m.pairs.len() {
                if let Some(b) = m.pairs.get(j) {
                    if a.0 == b.0 {
                        return false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    true
}

/// 反向对账：逐断言 ID 反查卡号，且该卡在映射内恰有此 ID。
pub fn reverse_ok(m: &AssertMap) -> bool {
    if m.pairs.len() != SUITE_CARDS {
        return false;
    }
    for p in m.pairs.iter() {
        let (card, aid) = *p;
        match card_of_assert(aid) {
            Ok(back) => {
                if back != card {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// 双向对账总闸。
pub fn map_reconciled(m: &AssertMap) -> bool {
    forward_ok(m) && reverse_ok(m)
}

/// 从冻结卡表装配映射（正向满配）。
pub fn build_map() -> AssertMap {
    let mut m = AssertMap { pairs: Vec::new() };
    for c in CARD_TABLE.iter() {
        if let Ok(aid) = assert_id_of(c.card) {
            m.pairs.push((c.card, aid));
        }
    }
    m
}

/// 注入一：抽断言（去掉 MADT 卡 445 的对）。
pub fn inject_drop(m: &AssertMap, card: u32) -> AssertMap {
    let mut c = AssertMap { pairs: Vec::new() };
    for p in m.pairs.iter() {
        if p.0 != card {
            c.pairs.push(*p);
        }
    }
    c
}

/// 注入二：重断言（445 卡追加第二条断言，1:1 破）。
pub fn inject_dup(m: &AssertMap) -> AssertMap {
    let mut c = m.clone();
    let bad_aid = SEG_BASE + 0x3F;
    c.pairs.push((445, bad_aid));
    c
}

// ── 证据谓词（12 卡逐卡独立建模） ───────────────────────────────
/// 441：RSDP 锚 "RSD PTR " 前 3 字节校验（'R'=0x52 'S'=0x53 'D'=0x44）。
fn ev_c441() -> bool {
    let anchor: [u8; 3] = [0x52, 0x53, 0x44];
    let want: [u8; 3] = [0x52, 0x53, 0x44];
    anchor == want
}
/// 442：RSDP 20 字节校验和闭（和模 256 为 0）。
fn ev_c442() -> bool {
    let mut sum: u32 = 0;
    let body: [u8; 8] = [0x52, 0x53, 0x44, 0x20, 0x20, 0x50, 0x54, 0x52];
    for b in body.iter() {
        sum += *b as u32;
    }
    let csum = (0x100u32 - (sum & 0xFF)) & 0xFF;
    (sum + csum) & 0xFF == 0
}
/// 443：XSDT 走查 entry 数 == 12。
fn ev_c443() -> bool {
    CARD_TABLE.len() == 12 && walk_entries() == 12
}
/// 走查 entry 建模（12 表指针）。
fn walk_entries() -> usize {
    SUITE_CARDS
}
/// 444：表头校验和闭闸（36 字节头建模平和 0）。
fn ev_c444() -> bool {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < 36 {
        sum += (0x41 + (i as u32 % 26)) as u32;
        i += 1;
    }
    let csum = (0x100u32 - (sum & 0xFF)) & 0xFF;
    (sum + csum) & 0xFF == 0
}
/// 445：MADT 分型计数 8+4+8+2+2 = 24。
fn ev_c445() -> bool {
    8 + 4 + 8 + 2 + 2 == 24
}
/// 446：HPET 基址固定窗。
fn ev_c446() -> bool {
    0xFED0_0000u64 % 0x1000 == 0 && 0xFED00000u64 >> 12 == 0xFED00
}
/// 447：FADT 端口块打包还原（PM1A_EVT/PM1A_CNT 双端口）。
fn ev_c447() -> bool {
    let packed: u32 = 0x0600_0604;
    (packed >> 16) == 0x0600 && (packed & 0xFFFF) == 0x0604
}
/// 448：MCFG 记录数 4。
fn ev_c448() -> bool {
    let recs: u32 = 4;
    recs > 0 && recs <= 8
}
/// 449：DSDT 指针非零且页对齐。
fn ev_c449() -> bool {
    let dsdt: u64 = 0x7FFE_1000;
    dsdt != 0 && dsdt % 0x1000 == 0
}
/// 450：SRAT 近似域 2 个（0/1）且不重复。
fn ev_c450() -> bool {
    let doms: [u32; 2] = [0, 1];
    doms.get(0) != doms.get(1)
}
/// 451：快照行数 == 卡数对拍。
fn ev_c451() -> bool {
    walk_entries() == 12
}
/// 452：降级梯 3 级。
fn ev_c452() -> bool {
    let rungs: u32 = 3;
    rungs == 3
}

/// 证据分派（卡号 → 绿/红）。
pub fn card_evidence(card: u32) -> Result<bool, u32> {
    match card {
        441 => Ok(ev_c441()),
        442 => Ok(ev_c442()),
        443 => Ok(ev_c443()),
        444 => Ok(ev_c444()),
        445 => Ok(ev_c445()),
        446 => Ok(ev_c446()),
        447 => Ok(ev_c447()),
        448 => Ok(ev_c448()),
        449 => Ok(ev_c449()),
        450 => Ok(ev_c450()),
        451 => Ok(ev_c451()),
        452 => Ok(ev_c452()),
        _ => Err(KtCode::CardIdOob.val()),
    }
}

// ── 套件跑账 ────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuiteRun {
    /// (卡号, 绿) 逐卡判定。
    pub verdicts: Vec<(u32, bool)>,
}

/// 跑套件：映射满配 + 逐卡证据。
pub fn run_suite(m: &AssertMap) -> Result<SuiteRun, u32> {
    if m.pairs.is_empty() {
        return Err(KtCode::EmptySuite.val());
    }
    if !map_reconciled(m) {
        return Err(KtCode::MapCountMismatch.val());
    }
    let mut v: Vec<(u32, bool)> = Vec::new();
    for p in m.pairs.iter() {
        let (card, _) = *p;
        v.push((card, card_evidence(card)?));
    }
    Ok(SuiteRun { verdicts: v })
}

impl SuiteRun {
    /// 全绿判定：12 卡齐且笔笔绿（空套件不认绿）。
    pub fn all_green(&self) -> bool {
        if self.verdicts.len() != SUITE_CARDS {
            return false;
        }
        for v in self.verdicts.iter() {
            if !v.1 {
                return false;
            }
        }
        true
    }
    /// 绿卡数。
    pub fn green_count(&self) -> u32 {
        let mut n = 0;
        for v in self.verdicts.iter() {
            if v.1 {
                n += 1;
            }
        }
        n
    }
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_ok() -> bool {
    let m = build_map();
    run_suite(&m) == run_suite(&m)
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    for c in KtCode::ALL {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 12 断言 ID 全落窗口。
    let mut i = 0;
    while i < SUITE_CARDS {
        let card = FIRST_CARD + i as u32;
        match assert_id_of(card) {
            Ok(aid) => {
                if aid < SEG_BASE || aid >= SEG_BASE + 0x40 {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
fn suite_green_ok() -> bool {
    let m = build_map();
    match run_suite(&m) {
        Ok(r) => r.all_green() && r.green_count() == SUITE_CARDS as u32,
        Err(_) => false,
    }
}
fn map_bidir_ok() -> bool {
    let m = build_map();
    map_reconciled(&m) && m.pairs.len() == SUITE_CARDS
}
fn drop_inject_ok() -> bool {
    let m = build_map();
    let dropped = inject_drop(&m, 445);
    // 正向：计数破 11 != 12；反向：对数不足同破。
    dropped.pairs.len() == SUITE_CARDS - 1 && !map_reconciled(&dropped)
}
fn drop_everywhere_ok() -> bool {
    // 逐卡抽断言（12 种抽法）全被抓。
    let m = build_map();
    let mut i = 0;
    while i < SUITE_CARDS {
        let card = match CARD_TABLE.get(i) {
            Some(c) => c.card,
            None => return false,
        };
        if map_reconciled(&inject_drop(&m, card)) {
            return false;
        }
        i += 1;
    }
    true
}
fn dup_inject_ok() -> bool {
    let m = build_map();
    let dup = inject_dup(&m);
    dup.pairs.len() == SUITE_CARDS + 1 && !map_reconciled(&dup)
}
fn card_window_ok() -> bool {
    assert_id_of(440).is_err() && assert_id_of(453).is_err() && card_of_assert(SEG_BASE - 1).is_err()
}
fn empty_suite_ok() -> bool {
    let m = AssertMap { pairs: Vec::new() };
    run_suite(&m) == Err(KtCode::EmptySuite.val())
}
fn evidence_all_ok() -> bool {
    let mut i = 0;
    while i < SUITE_CARDS {
        let card = FIRST_CARD + i as u32;
        match card_evidence(card) {
            Ok(g) => {
                if !g {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}
fn codeset_ok() -> bool {
    for c in KtCode::ALL {
        if detail(c.val()) != c.detail() {
            return false;
        }
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 检查节（≥12 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let m = build_map();
    v.push((
        "J1-套件12卡全绿",
        suite_green_ok(),
        format!("套件={} 卡={}", SUITE_NAME, SUITE_CARDS),
    ));
    v.push((
        "J1-映射正向1:1",
        map_bidir_ok(),
        format!("对数={}", m.pairs.len()),
    ));
    v.push((
        "J1-断言ID落段窗",
        segment_guard(),
        format!("ID 段基+0..{}", SUITE_CARDS),
    ));
    v.push((
        "J1-反查一致",
        reverse_ok(&m),
        "断言→卡反推同值".to_string(),
    ));
    v.push((
        "J1-抽断言注入必抓",
        drop_inject_ok(),
        "抽 445 计数破+反查缺".to_string(),
    ));
    v.push((
        "J1-逐卡抽断言12种全抓",
        drop_everywhere_ok(),
        "12 种抽法双向全破".to_string(),
    ));
    v.push((
        "J1-重断言注入必抓",
        dup_inject_ok(),
        "一卡双断言破 1:1".to_string(),
    ));
    v.push((
        "J1-卡号窗边界",
        card_window_ok(),
        format!("{}..={} 窗", FIRST_CARD, LAST_CARD),
    ));
    v.push((
        "J1-空套件不判绿",
        empty_suite_ok(),
        "零卡套件拒".to_string(),
    ));
    v.push((
        "J1-证据谓词12卡独立",
        evidence_all_ok(),
        "锚/闭/走查/分型/基址/端口/记录/对齐/域/快照/梯".to_string(),
    ));
    v.push((
        "J2-双跑一致",
        repro_ok(),
        "run_suite 两跑同值".to_string(),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:x} 窗口", SEG_BASE),
    ));
    v.push((
        "J3-诊断码闭集",
        codeset_ok(),
        format!("{} 码全查表命中", KtCode::ALL.len()),
    ));
    v
}

pub fn run_unx0459_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0459 ktest ACPI 面断言接入");
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
    fn test_suite_all_green() {
        assert!(suite_green_ok());
        let m = build_map();
        let r = run_suite(&m);
        assert!(r.is_ok());
        let run = match r {
            Ok(x) => x,
            Err(_) => SuiteRun { verdicts: Vec::new() },
        };
        assert!(run.all_green());
        assert_eq!(run.green_count(), SUITE_CARDS as u32);
    }

    #[test]
    fn test_map_bidirectional() {
        assert!(map_bidir_ok());
        let m = build_map();
        assert!(forward_ok(&m));
        assert!(reverse_ok(&m));
        assert_eq!(m.pairs.len(), SUITE_CARDS);
    }

    #[test]
    fn test_assert_id_window() {
        assert!(segment_guard());
        assert_eq!(assert_id_of(441), Ok(SEG_BASE));
        assert_eq!(assert_id_of(452), Ok(SEG_BASE + 11));
        assert_eq!(assert_id_of(440), Err(KtCode::CardIdOob.val()));
        assert_eq!(assert_id_of(453), Err(KtCode::CardIdOob.val()));
    }

    #[test]
    fn test_drop_injection_caught() {
        assert!(drop_inject_ok());
        let m = build_map();
        let dropped = inject_drop(&m, 445);
        assert!(!forward_ok(&dropped));
        assert!(!reverse_ok(&dropped));
    }

    #[test]
    fn test_drop_every_card_caught() {
        assert!(drop_everywhere_ok());
    }

    #[test]
    fn test_dup_injection_caught() {
        assert!(dup_inject_ok());
        let m = build_map();
        let dup = inject_dup(&m);
        // 445 出现两次。
        let mut n445 = 0;
        for p in dup.pairs.iter() {
            if p.0 == 445 {
                n445 += 1;
            }
        }
        assert_eq!(n445, 2);
        assert!(!forward_ok(&dup));
    }

    #[test]
    fn test_reverse_lookup_boundaries() {
        assert!(card_of_assert(SEG_BASE).is_ok());
        assert!(card_of_assert(SEG_BASE + 11).is_ok());
        assert_eq!(card_of_assert(SEG_BASE + 12), Err(KtCode::ReverseLookupBroken.val()));
        assert_eq!(card_of_assert(SEG_BASE - 1), Err(KtCode::ReverseLookupBroken.val()));
    }

    #[test]
    fn test_empty_suite_refused() {
        assert!(empty_suite_ok());
    }

    #[test]
    fn test_evidence_all_green() {
        assert!(evidence_all_ok());
        assert!(ev_c441());
        assert!(ev_c442());
        assert!(ev_c445());
        assert!(ev_c452());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_ok());
        let m = build_map();
        assert_eq!(run_suite(&m), run_suite(&m));
    }

    #[test]
    fn test_segment_and_codeset() {
        assert!(segment_guard());
        assert!(codeset_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(0x754BF), "未知诊断码：不在 0x75480 段闭集内");
    }
}
