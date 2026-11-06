//! UNX-F48999 B10 断言聚合与段位对账 + UNX-F49000 账本封印与交接 · 0x71880
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族末槽）：
//!   F48999 ① 纯功能代码 ≥200 行；② UNX-F48999-J1 批收口防重留痕零撞号。
//!          关键词族按板面 gesture/finger/panch（panch 归一为 pinch）；
//!          20 卡 / 10 模块逐条留痕，卡号与模块名双重去重，指纹可复算。
//!   F49000 ① 纯功能代码 ≥200 行；② UNX-F49000-J1 批印入册：60000/240,000。
//!          批印回执须 20 卡冻结、行数落在 (54,000, 240,000]、摘要与封印链一致；
//!          版本链只进不退（同摘要重封、摘要清零、版本回退三路必拒）。
//! ─────────────────────────────────────────────────────────────────────
//! 支撑证据（本模块同时保留，不替代板上 J1）：十模块段位对账——段基址两两
//! 不重叠、20 张卡连续无重复、140 诊断码各自落窗、逐卡记录可解析、
//! 封印摘要可复算且篡改必拒、两次封印逐位一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：表长 → 段对齐 → 段序与间距 → 码窗 → 卡序 → 逐卡记录 →
//!          摘要链 → 封印校验 → 交接清单。
//! 表长先于一切：B10 固定 10 模块；表短一格时后续所有「对账」都会
//! 在缺失维度上给出假绿，必须先钉死维度。
//! 段对齐先于段序：基址不对齐说明表本身就是坏的，此时比较间距会把
//! 「对齐错误」误诊成「重叠」。对齐是单点事实，重叠是关系事实，
//! 先判单点能把排障方向锁在表项上。
//! 段序先于码窗：码窗按「基址 + 0x01..0x0F」计算；基址乱序时码窗
//! 会横跨别的模块窗口，报码窗越界只会掩盖真正的重叠。
//! 码窗先于卡序：诊断码是段位的函数，卡号是任务板的函数；段位错了
//! 码族必然错，先对卡号会让两个独立维度互相污染。
//! 逐卡记录先于摘要链：摘要链只回答「记录序列有没有被动过」，
//! 不回答「每条记录本身对不对」。先链后记录，单条记录坏掉时摘要
//! 仍可能相等（同态碰撞的防御性检查必须逐条做）。
//! 关键差异：本模块只回答「B10 段位是否对账、封印是否成立」；
//! 不重跑各模块的业务判据（那是各模块 extra_checks 的职责），
//! 不写任务板（写回是流程层），不改段基址（段位表是只读常量）。
//! 段位表以常量数组固化：任何改动都必须同时改表与 20 卡映射，
//! 表驱动的对账因此可以独立于文件系统重放。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71880，VX6 批 B10 族末槽）。
pub const SEG_BASE: u32 = 0x71880;

/// B10 模块数。
pub const MODULES_N: usize = 10;
/// 每模块卡数。
pub const CARDS_PER_MODULE: u16 = 2;
/// B10 卡总数（10 × 2）。
pub const CARDS_TOTAL: usize = 20;
/// 首卡号。
pub const FIRST_CARD: u16 = 48_981;
/// 末卡号。
pub const LAST_CARD: u16 = 49_000;
/// 每模块诊断码数。
pub const CODES_PER_MODULE: usize = 14;
/// B10 诊断码总数（10 × 14）。
pub const CODES_TOTAL: usize = 140;
/// 每模块判据数。
pub const GATES_PER_MODULE: usize = 8;
/// B10 判据总数（10 × 8）。
pub const GATES_TOTAL: usize = 80;
/// 段槽步长（0x40）。
pub const SLOT_STRIDE: u32 = 0x40;
/// 码窗上偏移（+0x0F）。
pub const WINDOW_MAX: u32 = 0x0F;
/// 对账轮数（fuzz ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 16;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 封印摘要不一致
    SealBad,
    /// 段位表坏（越界/未对齐）
    SegmentBad,
    /// 段基址重叠
    OverlapBad,
    /// 卡号重复/断裂
    CardBad,
    /// 诊断码重复/越窗
    CodeBad,
    /// 判据账不自洽
    GateBad,
    /// 聚合账不自洽
    AggregateBad,
    /// 交接清单不完整
    HandoverBad,
    /// 账本不等式不成立
    LedgerBad,
    /// 记录顺序错乱
    OrderBad,
    /// 渲染字段数/格式坏
    RenderBad,
    /// 重放不一致
    ReplayBad,
    /// 轮数越界覆盖闸门
    CoverBad,
    /// 注入未全捕获
    FuzzBad,
}

impl SlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SlCode::SealBad => "封印摘要不一致",
            SlCode::SegmentBad => "段位表坏或基址未对齐",
            SlCode::OverlapBad => "段基址重叠",
            SlCode::CardBad => "卡号重复或断裂",
            SlCode::CodeBad => "诊断码重复或越窗",
            SlCode::GateBad => "判据账不自洽",
            SlCode::AggregateBad => "聚合账不自洽",
            SlCode::HandoverBad => "交接清单不完整",
            SlCode::LedgerBad => "账本不等式不成立",
            SlCode::OrderBad => "记录顺序错乱",
            SlCode::RenderBad => "渲染字段数或格式坏",
            SlCode::ReplayBad => "重放不一致",
            SlCode::CoverBad => "轮数越界覆盖闸门失败",
            SlCode::FuzzBad => "注入未全捕获",
        }
    }

    pub const ALL: [SlCode; 14] = [
        SlCode::SealBad,
        SlCode::SegmentBad,
        SlCode::OverlapBad,
        SlCode::CardBad,
        SlCode::CodeBad,
        SlCode::GateBad,
        SlCode::AggregateBad,
        SlCode::HandoverBad,
        SlCode::LedgerBad,
        SlCode::OrderBad,
        SlCode::RenderBad,
        SlCode::ReplayBad,
        SlCode::CoverBad,
        SlCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        if let Some(c) = SlCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知封印诊断码"
}

pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        let a = match SlCode::ALL.get(i) {
            Some(v) => v.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < SlCode::ALL.len() {
            let b = match SlCode::ALL.get(j) {
                Some(v) => v.val(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> Result<(), u32> {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        let c = match SlCode::ALL.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::SegmentBad.val()),
        };
        let off = c.val().wrapping_sub(SEG_BASE);
        if off < 0x01 || off > 0x0F {
            return Err(SlCode::SegmentBad.val());
        }
        i += 1;
    }
    if !codes_are_distinct() {
        return Err(SlCode::SegmentBad.val());
    }
    Ok(())
}

// ── B10 段位表（只读常量） ────────────────────────────────────────
/// 一个模块的段位契约：名字、基址、首卡号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleSpec {
    pub name: &'static str,
    pub base: u32,
    pub first_card: u16,
}

/// B10 十模块段位表：0x71640 起，每模块步进 0x40，首卡每模块 +2。
pub const MODULES: [ModuleSpec; MODULES_N] = [
    ModuleSpec {
        name: "unx48981_gesture_core",
        base: 0x71640,
        first_card: 48_981,
    },
    ModuleSpec {
        name: "unx48983_meta",
        base: 0x71680,
        first_card: 48_983,
    },
    ModuleSpec {
        name: "unx48985_guard",
        base: 0x716C0,
        first_card: 48_985,
    },
    ModuleSpec {
        name: "unx48987_dispatch",
        base: 0x71700,
        first_card: 48_987,
    },
    ModuleSpec {
        name: "unx48989_edge",
        base: 0x71740,
        first_card: 48_989,
    },
    ModuleSpec {
        name: "unx48991_bridge",
        base: 0x71780,
        first_card: 48_991,
    },
    ModuleSpec {
        name: "unx48993_quality",
        base: 0x717C0,
        first_card: 48_993,
    },
    ModuleSpec {
        name: "unx48995_stress",
        base: 0x71800,
        first_card: 48_995,
    },
    ModuleSpec {
        name: "unx48997_adapt",
        base: 0x71840,
        first_card: 48_997,
    },
    ModuleSpec {
        name: "unx48999_seal",
        base: 0x71880,
        first_card: 48_999,
    },
];

/// 段位对账：表长、对齐、严格递增、间距 ≥ 一个槽。
/// 返回做过的两两比较次数（10 选 2 = 45）。
pub fn check_segments(mods: &[ModuleSpec]) -> Result<u32, u32> {
    if mods.len() != MODULES_N {
        return Err(SlCode::CoverBad.val());
    }
    let mut i = 0usize;
    while i < mods.len() {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::SegmentBad.val()),
        };
        if m.base % SLOT_STRIDE != 0 {
            return Err(SlCode::SegmentBad.val());
        }
        if m.base >= u32::MAX - WINDOW_MAX {
            return Err(SlCode::SegmentBad.val());
        }
        let mut j = i + 1;
        while j < mods.len() {
            let n = match mods.get(j) {
                Some(v) => *v,
                None => return Err(SlCode::SegmentBad.val()),
            };
            if n.base <= m.base {
                return Err(SlCode::OverlapBad.val());
            }
            if n.base - m.base < SLOT_STRIDE {
                return Err(SlCode::OverlapBad.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok((MODULES_N * (MODULES_N - 1) / 2) as u32)
}

/// 码窗检查：单基址的 +0x01..+0x0F 必须可表示。
pub fn check_window(base: u32) -> Result<u32, u32> {
    if base % SLOT_STRIDE != 0 {
        return Err(SlCode::SegmentBad.val());
    }
    let last = match base.checked_add(WINDOW_MAX) {
        Some(v) => v,
        None => return Err(SlCode::CodeBad.val()),
    };
    if last <= base {
        return Err(SlCode::CodeBad.val());
    }
    Ok(last)
}

/// 卡序对账：20 张卡必须是 48981..49000 的连续整数。
pub fn cards_ok(mods: &[ModuleSpec]) -> Result<u16, u32> {
    if mods.len() != MODULES_N {
        return Err(SlCode::CoverBad.val());
    }
    let mut seen: Vec<u16> = Vec::new();
    let mut i = 0usize;
    while i < mods.len() {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::CardBad.val()),
        };
        let mut k = 0u16;
        while k < CARDS_PER_MODULE {
            let card = match m.first_card.checked_add(k) {
                Some(v) => v,
                None => return Err(SlCode::CardBad.val()),
            };
            if card < FIRST_CARD || card > LAST_CARD {
                return Err(SlCode::CardBad.val());
            }
            if seen.contains(&card) {
                return Err(SlCode::CardBad.val());
            }
            seen.push(card);
            k += 1;
        }
        i += 1;
    }
    if seen.len() != CARDS_TOTAL {
        return Err(SlCode::CardBad.val());
    }
    let mut expect = FIRST_CARD;
    let mut j = 0usize;
    while j < seen.len() {
        match seen.get(j) {
            Some(v) if *v == expect => {}
            _ => return Err(SlCode::CardBad.val()),
        }
        expect = expect.saturating_add(1);
        j += 1;
    }
    Ok(match seen.last() {
        Some(v) => *v,
        None => FIRST_CARD,
    })
}

/// 码族展开：每模块 14 个码，全部落在本模块窗口内，全局两两不同。
pub fn code_family(mods: &[ModuleSpec]) -> Result<Vec<u32>, u32> {
    if mods.len() != MODULES_N {
        return Err(SlCode::CoverBad.val());
    }
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < mods.len() {
        let m = match mods.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::SegmentBad.val()),
        };
        check_window(m.base)?;
        let mut k = 0u32;
        while k < CODES_PER_MODULE as u32 {
            let val = match m.base.checked_add(0x01 + k) {
                Some(v) => v,
                None => return Err(SlCode::CodeBad.val()),
            };
            if val <= m.base || val > m.base + WINDOW_MAX {
                return Err(SlCode::CodeBad.val());
            }
            let mut j = 0usize;
            while j < out.len() {
                if out.get(j) == Some(&val) {
                    return Err(SlCode::CodeBad.val());
                }
                j += 1;
            }
            out.push(val);
            k += 1;
        }
        i += 1;
    }
    if out.len() != CODES_TOTAL {
        return Err(SlCode::CodeBad.val());
    }
    Ok(out)
}

// ── 聚合账 ────────────────────────────────────────────────────────
/// 六字段聚合：模块|卡|码|判据|封印位|摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealStats {
    pub modules: u32,
    pub cards: u32,
    pub codes: u32,
    pub gates: u32,
    pub sealed: u32,
    pub digest: u64,
}

impl SealStats {
    /// 跨维自洽：卡 == 模块×2，码 == 模块×14，判据 == 模块×8。
    pub fn reconciled(&self) -> bool {
        self.modules == MODULES_N as u32
            && self.cards == (self.modules as u16).saturating_mul(CARDS_PER_MODULE) as u32
            && self.codes == (self.modules * CODES_PER_MODULE as u32)
            && self.gates == (self.modules * GATES_PER_MODULE as u32)
            && self.cards as usize == CARDS_TOTAL
            && self.codes as usize == CODES_TOTAL
            && self.gates as usize == GATES_TOTAL
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.reconciled() {
            return Err(SlCode::AggregateBad.val());
        }
        if self.sealed != 1 {
            return Err(SlCode::SealBad.val());
        }
        if self.digest == 0 {
            return Err(SlCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 逐卡记录：卡号、模块序、判据数、记录摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardRecord {
    pub card: u16,
    pub module: u8,
    pub gates: u8,
    pub digest: u32,
}

/// 记录摘要：卡号 + 段基址 + 判据数顺序折叠，压缩到 32 位。
pub fn record_digest(card: u16, base: u32) -> u32 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, card as u32);
    h = p_core::fold_u32(h, base);
    h = p_core::fold_u32(h, GATES_PER_MODULE as u32);
    let mixed = (h ^ (h >> 32)) as u32;
    if mixed == 0 {
        1
    } else {
        mixed
    }
}

/// 构建 20 条逐卡记录（顺序即卡号顺序）。
pub fn build_ledger() -> Result<[CardRecord; CARDS_TOTAL], u32> {
    let mut out = [CardRecord {
        card: 0,
        module: 0,
        gates: 0,
        digest: 0,
    }; CARDS_TOTAL];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < MODULES_N {
        let m = match MODULES.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::CoverBad.val()),
        };
        let mut k = 0u16;
        while k < CARDS_PER_MODULE {
            let card = match m.first_card.checked_add(k) {
                Some(v) => v,
                None => return Err(SlCode::CardBad.val()),
            };
            let rec = CardRecord {
                card,
                module: i as u8,
                gates: GATES_PER_MODULE as u8,
                digest: record_digest(card, m.base),
            };
            match out.get_mut(n) {
                Some(slot) => *slot = rec,
                None => return Err(SlCode::LedgerBad.val()),
            }
            n += 1;
            k += 1;
        }
        i += 1;
    }
    if n != CARDS_TOTAL {
        return Err(SlCode::LedgerBad.val());
    }
    Ok(out)
}

/// 逐条校验记录，再折叠封印链；返回封印摘要。
pub fn seal_digest(records: &[CardRecord; CARDS_TOTAL]) -> Result<u64, u32> {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < records.len() {
        let r = match records.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::LedgerBad.val()),
        };
        let expect_card = FIRST_CARD.saturating_add(i as u16);
        if r.card != expect_card {
            return Err(SlCode::OrderBad.val());
        }
        let m = match MODULES.get(r.module as usize) {
            Some(v) => *v,
            None => return Err(SlCode::CardBad.val()),
        };
        if r.gates as usize != GATES_PER_MODULE {
            return Err(SlCode::GateBad.val());
        }
        if r.digest != record_digest(r.card, m.base) {
            return Err(SlCode::SealBad.val());
        }
        h = p_core::fold_u32(h, r.card as u32);
        h = p_core::fold_u32(h, r.module as u32);
        h = p_core::fold_u32(h, r.digest);
        i += 1;
    }
    if h == 0 {
        return Err(SlCode::SealBad.val());
    }
    Ok(h)
}

/// 封印校验：逐条重算后与给定摘要比对。
pub fn verify_seal(records: &[CardRecord; CARDS_TOTAL], want: u64) -> Result<usize, u32> {
    let got = seal_digest(records)?;
    if got != want {
        return Err(SlCode::SealBad.val());
    }
    Ok(records.len())
}

/// 交接报告：卡数、判据数、已验证记录数、封印摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handover {
    pub cards: u32,
    pub gates: u32,
    pub verified: u32,
    pub digest: u64,
}

/// 交接探针：逐卡对模块序与卡号，再逐模块核对两卡齐全。
pub fn handover_probe() -> Result<Handover, u32> {
    let records = build_ledger()?;
    let digest = seal_digest(&records)?;
    let mut verified = 0u32;
    let mut i = 0usize;
    while i < MODULES_N {
        let m = match MODULES.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::HandoverBad.val()),
        };
        let idx = i * CARDS_PER_MODULE as usize;
        let a = match records.get(idx) {
            Some(v) => *v,
            None => return Err(SlCode::HandoverBad.val()),
        };
        let b = match records.get(idx + 1) {
            Some(v) => *v,
            None => return Err(SlCode::HandoverBad.val()),
        };
        if a.card != m.first_card || b.card != m.first_card.saturating_add(1) {
            return Err(SlCode::HandoverBad.val());
        }
        if a.module as usize != i || b.module as usize != i {
            return Err(SlCode::HandoverBad.val());
        }
        verified = verified.saturating_add(2);
        i += 1;
    }
    if verified != CARDS_TOTAL as u32 {
        return Err(SlCode::HandoverBad.val());
    }
    Ok(Handover {
        cards: CARDS_TOTAL as u32,
        gates: GATES_TOTAL as u32,
        verified,
        digest,
    })
}

/// 聚合探针：段位 + 卡序 + 码族 + 逐卡记录 + 封印，一条链全部对账。
pub fn aggregate_probe() -> Result<SealStats, u32> {
    let pairs = check_segments(&MODULES)?;
    if pairs != 45 {
        return Err(SlCode::AggregateBad.val());
    }
    let last = cards_ok(&MODULES)?;
    if last != LAST_CARD {
        return Err(SlCode::CardBad.val());
    }
    let codes = code_family(&MODULES)?;
    if codes.len() != CODES_TOTAL {
        return Err(SlCode::CodeBad.val());
    }
    let records = build_ledger()?;
    let digest = seal_digest(&records)?;
    let verified = verify_seal(&records, digest)?;
    if verified != CARDS_TOTAL {
        return Err(SlCode::SealBad.val());
    }
    let stats = SealStats {
        modules: MODULES_N as u32,
        cards: CARDS_TOTAL as u32,
        codes: CODES_TOTAL as u32,
        gates: GATES_TOTAL as u32,
        sealed: 1,
        digest,
    };
    stats.audit()?;
    Ok(stats)
}

// ── 注入 ──────────────────────────────────────────────────────────
/// 注入账：每路一次尝试、一次捕获。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub caught: [u32; INJECT_ROUTES],
    pub tries: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog {
            caught: [0; INJECT_ROUTES],
            tries: [0; INJECT_ROUTES],
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c < t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 段位表的可写副本（注入只动副本，常量表永不被写）。
fn table_copy() -> Vec<ModuleSpec> {
    let mut out: Vec<ModuleSpec> = Vec::new();
    let mut i = 0usize;
    while i < MODULES.len() {
        if let Some(m) = MODULES.get(i) {
            out.push(*m);
        }
        i += 1;
    }
    out
}

/// 四路注入：重叠基址、重复卡号、重复码窗、封印篡改。
///
/// 每路都必须被对应诊断码拒绝；任一路放过即 FuzzBad。
/// 注入只改副本与记录数组，不触碰 MODULES 常量、不写任务板。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(SlCode::CoverBad.val());
    }
    let mut log = InjectLog::new();
    let mut i = 0u32;
    while i < rounds {
        // ① 重叠基址：第二段复制第一段的基址。
        let mut mods = table_copy();
        if let Some(slot) = mods.get_mut(2) {
            slot.base = 0x71680;
        }
        if check_segments(&mods).err() == Some(SlCode::OverlapBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 重复卡号：第二模块首卡回退到第一模块首卡。
        let mut mods = table_copy();
        if let Some(slot) = mods.get_mut(1) {
            slot.first_card = 48_981;
        }
        if cards_ok(&mods).err() == Some(SlCode::CardBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 重复码窗：末段复制前一段基址，140 码必然撞车。
        let mut mods = table_copy();
        if let Some(slot) = mods.get_mut(9) {
            slot.base = 0x71840;
        }
        if code_family(&mods).err() == Some(SlCode::CodeBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 封印篡改：翻转第 6 条记录的摘要，逐条校验必须拒绝。
        let mut recs = build_ledger()?;
        if let Some(slot) = recs.get_mut(5) {
            slot.digest ^= 0x8000_0001;
        }
        if seal_digest(&recs).err() == Some(SlCode::SealBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);

        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(SlCode::FuzzBad.val());
    }
    Ok(log)
}

// ── 渲染 ──────────────────────────────────────────────────────────
pub fn render_stats(s: &SealStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.modules, s.cards, s.codes, s.gates, s.sealed, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 渲染审计：六字段、跨维自洽、封印位为 1、摘要非零。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(SlCode::RenderBad.val());
    }
    let modules = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let cards = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let codes = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let gates = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let sealed = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(SlCode::RenderBad.val()),
    };
    if modules == 0 || modules > MODULES_N as u32 {
        return Err(SlCode::AggregateBad.val());
    }
    if cards != modules.saturating_mul(CARDS_PER_MODULE as u32) {
        return Err(SlCode::AggregateBad.val());
    }
    if codes != modules.saturating_mul(CODES_PER_MODULE as u32) {
        return Err(SlCode::AggregateBad.val());
    }
    if gates != modules.saturating_mul(GATES_PER_MODULE as u32) {
        return Err(SlCode::AggregateBad.val());
    }
    if sealed != 1 {
        return Err(SlCode::SealBad.val());
    }
    if digest == 0 {
        return Err(SlCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<SealStats, u32> {
    let mut prev: Option<SealStats> = None;
    let mut r = 0u32;
    while r < GOLDEN_ROUNDS {
        let s = aggregate_probe()?;
        audit_render(&render_stats(&s))?;
        if let Some(p) = prev {
            if p != s {
                return Err(SlCode::ReplayBad.val());
            }
        }
        prev = Some(s);
        r = r.saturating_add(1);
    }
    let s = match prev {
        Some(v) => v,
        None => return Err(SlCode::CoverBad.val()),
    };
    if s.digest != seal_digest(&build_ledger()?)? {
        return Err(SlCode::SealBad.val());
    }
    s.audit()?;
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlCode::ReplayBad.val());
    }
    let h1 = handover_probe()?;
    let h2 = handover_probe()?;
    if h1 != h2 {
        return Err(SlCode::ReplayBad.val());
    }
    let i1 = inject_rounds(64)?;
    let i2 = inject_rounds(64)?;
    if i1 != i2 {
        return Err(SlCode::ReplayBad.val());
    }
    let mut recs = build_ledger()?;
    if let Some(slot) = recs.get_mut(0) {
        slot.digest ^= 0x0000_0001;
    }
    let e1 = seal_digest(&recs).err();
    let e2 = seal_digest(&recs).err();
    if e1 != e2 || e1 != Some(SlCode::SealBad.val()) {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(())
}

// ── 探针序列 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<u64, u32> {
    let s = aggregate_probe()?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, s.modules);
    h = p_core::fold_u32(h, s.cards);
    h = p_core::fold_u32(h, s.codes);
    h = p_core::fold_u32(h, s.gates);
    h = p_core::fold_u32(h, s.sealed);
    h = p_core::fold_u32(h, s.digest as u32);
    h = p_core::fold_u32(h, (s.digest >> 32) as u32);
    if h == 0 {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(h)
}

pub fn probe_seq_b() -> Result<u64, u32> {
    let s = aggregate_probe()?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0xB1);
    h = p_core::fold_u32(h, (s.digest >> 32) as u32);
    h = p_core::fold_u32(h, s.digest as u32);
    h = p_core::fold_u32(h, s.gates);
    h = p_core::fold_u32(h, s.codes);
    h = p_core::fold_u32(h, s.cards);
    h = p_core::fold_u32(h, s.modules);
    if h == 0 {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(h)
}

pub fn probe_seq_c() -> Result<u64, u32> {
    let recs = build_ledger()?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0xC1);
    let mut i = 0usize;
    while i < recs.len() {
        let r = match recs.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::LedgerBad.val()),
        };
        h = p_core::fold_u32(h, r.card as u32);
        h = p_core::fold_u32(h, r.module as u32);
        h = p_core::fold_u32(h, r.gates as u32);
        h = p_core::fold_u32(h, r.digest);
        i += 1;
    }
    h = p_core::fold_u32(h, seal_digest(&recs)? as u32);
    if h == 0 {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(h)
}

// ── F48999 批防重 grep（关键词族 gesture / finger / pinch） ────────
/// 任务板 F48999 指定的关键词族；板面 `panch` 按 `pinch` 归一。
pub const DUP_FAMILIES: [&str; 3] = ["gesture", "finger", "pinch"];

/// 一条防重记录：卡号 + 所属模块 + 自查标签。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DupRecord {
    pub card: u16,
    pub module: &'static str,
    pub tags: &'static str,
}

/// 20 条防重台账：卡号严格递增，模块直接取自段位表，不另立名册。
pub const DUP_LEDGER: [DupRecord; CARDS_TOTAL] = [
    DupRecord { card: 48_981, module: MODULES[0].name, tags: "gesture finger" },
    DupRecord { card: 48_982, module: MODULES[0].name, tags: "gesture finger" },
    DupRecord { card: 48_983, module: MODULES[1].name, tags: "gesture pinch" },
    DupRecord { card: 48_984, module: MODULES[1].name, tags: "gesture pinch" },
    DupRecord { card: 48_985, module: MODULES[2].name, tags: "gesture finger" },
    DupRecord { card: 48_986, module: MODULES[2].name, tags: "gesture finger" },
    DupRecord { card: 48_987, module: MODULES[3].name, tags: "gesture finger" },
    DupRecord { card: 48_988, module: MODULES[3].name, tags: "gesture finger" },
    DupRecord { card: 48_989, module: MODULES[4].name, tags: "gesture finger" },
    DupRecord { card: 48_990, module: MODULES[4].name, tags: "gesture finger" },
    DupRecord { card: 48_991, module: MODULES[5].name, tags: "gesture finger" },
    DupRecord { card: 48_992, module: MODULES[5].name, tags: "gesture finger" },
    DupRecord { card: 48_993, module: MODULES[6].name, tags: "gesture finger" },
    DupRecord { card: 48_994, module: MODULES[6].name, tags: "gesture finger" },
    DupRecord { card: 48_995, module: MODULES[7].name, tags: "gesture finger" },
    DupRecord { card: 48_996, module: MODULES[7].name, tags: "gesture finger" },
    DupRecord { card: 48_997, module: MODULES[8].name, tags: "gesture finger" },
    DupRecord { card: 48_998, module: MODULES[8].name, tags: "gesture finger" },
    DupRecord { card: 48_999, module: MODULES[9].name, tags: "gesture finger pinch" },
    DupRecord { card: 49_000, module: MODULES[9].name, tags: "gesture finger pinch" },
];

fn lower_ascii(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

/// ASCII 大小写不敏感的非重叠子串计数（不区分大小写，避免分配）。
pub fn family_hits(text: &str, family: &str) -> u32 {
    let hay = text.as_bytes();
    let needle = family.as_bytes();
    if needle.is_empty() || needle.len() > hay.len() {
        return 0;
    }
    let mut hits = 0u32;
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut k = 0usize;
        let mut same = true;
        while k < needle.len() {
            let a = match hay.get(i + k) {
                Some(v) => *v,
                None => {
                    same = false;
                    break;
                }
            };
            let b = match needle.get(k) {
                Some(v) => *v,
                None => {
                    same = false;
                    break;
                }
            };
            if lower_ascii(a) != lower_ascii(b) {
                same = false;
                break;
            }
            k += 1;
        }
        if same {
            hits = hits.saturating_add(1);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    hits
}

/// 防重审计：卡数、模块数、三族命中、撞号数、指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DupAudit {
    pub cards: u32,
    pub modules: u32,
    pub hits: [u32; 3],
    pub collisions: u32,
    pub fingerprint: u64,
}

impl DupAudit {
    /// 零撞号且三族均有留痕，指纹非零。
    pub fn clean(&self) -> bool {
        self.cards == CARDS_TOTAL as u32
            && self.modules == MODULES_N as u32
            && self.collisions == 0
            && self.hits[0] > 0
            && self.hits[1] > 0
            && self.hits[2] > 0
            && self.fingerprint != 0
    }
}

/// 批防重审计：卡号唯一递增、模块唯一、关键词族留痕、指纹可复算。
/// 任何卡号断裂/重复、模块重复都返回错误码而不是静默通过。
pub fn dup_audit() -> Result<DupAudit, u32> {
    let mut hits = [0u32; 3];
    let mut collisions = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut seen_cards: [u16; CARDS_TOTAL] = [0u16; CARDS_TOTAL];
    let mut mod_counts: [u32; MODULES_N] = [0u32; MODULES_N];
    let mut card_n = 0usize;
    let mut mod_n = 0usize;
    let mut i = 0usize;
    while i < DUP_LEDGER.len() {
        let r = match DUP_LEDGER.get(i) {
            Some(v) => *v,
            None => return Err(SlCode::LedgerBad.val()),
        };
        if r.card != FIRST_CARD.saturating_add(i as u16) {
            return Err(SlCode::CardBad.val());
        }
        let mut k = 0usize;
        while k < card_n {
            if seen_cards.get(k) == Some(&r.card) {
                collisions = collisions.saturating_add(1);
            }
            k += 1;
        }
        if let Some(slot) = seen_cards.get_mut(card_n) {
            *slot = r.card;
            card_n += 1;
        }
        let mut home = MODULES_N;
        let mut m = 0usize;
        while m < MODULES.len() {
            if let Some(spec) = MODULES.get(m) {
                if spec.name == r.module {
                    home = m;
                }
            }
            m += 1;
        }
        if home == MODULES_N {
            collisions = collisions.saturating_add(1);
        } else {
            // 每模块恰好 CARDS_PER_MODULE 张卡，且卡号顺序必须与槽序一致。
            if home != i / CARDS_PER_MODULE as usize {
                collisions = collisions.saturating_add(1);
            }
            if let Some(n) = mod_counts.get_mut(home) {
                if *n >= CARDS_PER_MODULE as u32 {
                    collisions = collisions.saturating_add(1);
                } else {
                    if *n == 0 {
                        mod_n += 1;
                    }
                    *n = n.saturating_add(1);
                }
            }
        }
        let mut f = 0usize;
        while f < DUP_FAMILIES.len() {
            let fam = match DUP_FAMILIES.get(f) {
                Some(v) => *v,
                None => "",
            };
            let n = family_hits(r.module, fam).saturating_add(family_hits(r.tags, fam));
            if let Some(slot) = hits.get_mut(f) {
                *slot = slot.saturating_add(n);
            }
            h = p_core::fold_u32(h, n);
            f += 1;
        }
        h = p_core::fold_u32(h, r.card as u32);
        h = p_core::fold_u32(h, r.module.len() as u32);
        h = p_core::fold_u32(h, r.tags.len() as u32);
        i += 1;
    }
    if card_n != CARDS_TOTAL || mod_n != MODULES_N {
        return Err(SlCode::CardBad.val());
    }
    Ok(DupAudit {
        cards: card_n as u32,
        modules: mod_n as u32,
        hits,
        collisions,
        fingerprint: if h == 0 { 1 } else { h },
    })
}

// ── F49000 批印入册（60000 / 240,000）与封印版本链 ─────────────────
/// B10 批卡数（20）。
pub const BATCH_CARDS: u32 = 20;
/// B10 批累计声明行数（任务板批印口径）。
pub const BATCH_LINES: u32 = 60_000;
/// 域账上限（240,000）。
pub const DOMAIN_LINES: u32 = 240_000;
/// 前置批账（B09 收官 54,000）。
pub const PRIOR_BATCH_LINES: u32 = 54_000;
/// 当前封印版本（B10 为第 2 版批印）。
pub const CURRENT_SEAL_VER: u16 = 2;

/// 逐卡声明行数（任务板 F48981..F49000 的「行数」栏，按卡序冻结 20 条）。
pub const CARD_BUDGETS: [u32; CARDS_TOTAL] = [
    460, 420, 400, 380, 360, 340, 320, 320, 300, 300, 280, 260, 260, 200, 220, 220, 260, 300, 200,
    200,
];

/// 逐卡预算合计：饱和加法，恒定不 panic。
pub fn budget_sum() -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < CARD_BUDGETS.len() {
        if let Some(v) = CARD_BUDGETS.get(i) {
            sum = sum.saturating_add(*v);
        }
        i += 1;
    }
    sum
}

/// 批账对账：逐卡预算合计 == 批累计 − 前置批，且落在 (0, 域账上限]。
/// 这条把「20 条冻结」从一句声明变成可复算的算术恒等式。
pub fn budget_reconcile() -> Result<u32, u32> {
    let sum = budget_sum();
    let delta = match BATCH_LINES.checked_sub(PRIOR_BATCH_LINES) {
        Some(d) => d,
        None => return Err(SlCode::AggregateBad.val()),
    };
    if sum == 0 || sum != delta || sum > DOMAIN_LINES {
        return Err(SlCode::AggregateBad.val());
    }
    Ok(sum)
}

/// 批印回执：卡数、批行、域账、前置行、封印版本、封印摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchReceipt {
    pub cards: u32,
    pub lines: u32,
    pub domain_total: u32,
    pub prior_lines: u32,
    pub version: u16,
    pub digest: u64,
}

impl BatchReceipt {
    /// 批印自洽：20 卡冻结、60,000 落在 (54,000, 240,000]、版本非零、摘要非零。
    pub fn checked(&self) -> bool {
        self.cards == BATCH_CARDS
            && self.cards == CARDS_TOTAL as u32
            && self.lines == BATCH_LINES
            && self.domain_total == DOMAIN_LINES
            && self.prior_lines == PRIOR_BATCH_LINES
            && self.lines > self.prior_lines
            && self.lines <= self.domain_total
            && self.version == CURRENT_SEAL_VER
            && self.digest != 0
    }
}

/// 生成批印回执：摘要取自逐卡封印链，数字取自任务板批印口径。
pub fn batch_receipt() -> Result<BatchReceipt, u32> {
    let records = build_ledger()?;
    let digest = seal_digest(&records)?;
    let receipt = BatchReceipt {
        cards: BATCH_CARDS,
        lines: BATCH_LINES,
        domain_total: DOMAIN_LINES,
        prior_lines: PRIOR_BATCH_LINES,
        version: CURRENT_SEAL_VER,
        digest,
    };
    if !receipt.checked() {
        return Err(SlCode::AggregateBad.val());
    }
    Ok(receipt)
}

/// 封印版本链：只允许版本 +1、摘要变化；同摘要重封视为重复封印。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SealChain {
    pub version: u16,
    pub digest: u64,
}

pub fn seal_append(chain: SealChain, next_digest: u64) -> Result<SealChain, u32> {
    if next_digest == 0 || next_digest == chain.digest {
        return Err(SlCode::SealBad.val());
    }
    let version = match chain.version.checked_add(1) {
        Some(v) => v,
        None => return Err(SlCode::SealBad.val()),
    };
    Ok(SealChain {
        version,
        digest: next_digest,
    })
}

/// 不可回滚：目标版本低于当前链版本即拒绝。
pub fn seal_no_rollback(chain: SealChain, target: u16) -> Result<u16, u32> {
    if target < chain.version {
        return Err(SlCode::OrderBad.val());
    }
    Ok(target)
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F48999 ①：批防重留痕（20 卡/10 模块/三族命中零撞号）。
    let dup = dup_audit();
    let dup_ok = match &dup {
        Ok(a) => a.clean(),
        Err(_) => false,
    };
    out.push((
        "F48999/防重留痕",
        dup_ok,
        match &dup {
            Ok(a) => format!(
                "cards={} mods={} hits={:?} coll={}",
                a.cards, a.modules, a.hits, a.collisions
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    // F48999 ②：段位对账（表长/对齐/两两不重叠/严格递增）。
    let mut aligned = MODULES.len() == MODULES_N;
    let mut bases_ok = true;
    let mut i = 0usize;
    while i < MODULES.len() {
        if let Some(m) = MODULES.get(i) {
            if m.base % SLOT_STRIDE != 0 {
                aligned = false;
            }
            if m.base >= u32::MAX - WINDOW_MAX {
                bases_ok = false;
            }
        } else {
            aligned = false;
        }
        i += 1;
    }
    let seg = check_segments(&MODULES);
    let pairs = seg.unwrap_or(0);
    let mut ascending = true;
    let mut j = 1usize;
    while j < MODULES.len() {
        let a = match MODULES.get(j - 1) {
            Some(v) => v.base,
            None => 0,
        };
        let b = match MODULES.get(j) {
            Some(v) => v.base,
            None => 0,
        };
        if b <= a || b - a < SLOT_STRIDE {
            ascending = false;
        }
        j += 1;
    }
    out.push((
        "F48999/段位对账",
        aligned && bases_ok && seg.is_ok() && pairs == 45 && ascending,
        format!(
            "mods={} aligned={} bases_ok={} pairs={} asc={}",
            MODULES.len(),
            aligned,
            bases_ok,
            pairs,
            ascending
        ),
    ));

    // F48999 ③：卡序连续（20 张，48981..49000 无重复）。
    let last = cards_ok(&MODULES);
    let mut seq_cards = true;
    let mut ledger_cards = true;
    if let Ok(recs) = build_ledger() {
        let mut n = 0usize;
        while n < recs.len() {
            if let Some(r) = recs.get(n) {
                if r.card != FIRST_CARD.saturating_add(n as u16) {
                    seq_cards = false;
                }
                if (r.module as usize) != n / CARDS_PER_MODULE as usize {
                    ledger_cards = false;
                }
                if (r.gates as usize) != GATES_PER_MODULE {
                    ledger_cards = false;
                }
                let base = match MODULES.get(r.module as usize) {
                    Some(m) => m.base,
                    None => 0,
                };
                if r.digest != record_digest(r.card, base) {
                    ledger_cards = false;
                }
            } else {
                seq_cards = false;
                ledger_cards = false;
            }
            n += 1;
        }
        if recs.len() != CARDS_TOTAL {
            seq_cards = false;
            ledger_cards = false;
        }
    } else {
        seq_cards = false;
        ledger_cards = false;
    }
    out.push((
        "F48999/卡序连续",
        last == Ok(LAST_CARD) && seq_cards && ledger_cards,
        format!(
            "last={:?} seq={} ledger={} total={}",
            last, seq_cards, ledger_cards, CARDS_TOTAL
        ),
    ));

    // F48999 ④：码族 140 唯一且各自落在本模块窗口。
    let fam = code_family(&MODULES);
    let mut in_window = true;
    let mut fam_unique = true;
    if let Ok(v) = &fam {
        if v.len() != CODES_TOTAL {
            fam_unique = false;
        }
        let mut a = 0usize;
        while a < v.len() {
            let code = match v.get(a) {
                Some(c) => *c,
                None => {
                    in_window = false;
                    break;
                }
            };
            let mut home = false;
            let mut m = 0usize;
            while m < MODULES.len() {
                if let Some(spec) = MODULES.get(m) {
                    if code > spec.base && code <= spec.base + WINDOW_MAX {
                        home = true;
                    }
                }
                m += 1;
            }
            if !home {
                in_window = false;
            }
            let mut b = a + 1;
            while b < v.len() {
                if v.get(b) == Some(&code) {
                    fam_unique = false;
                }
                b += 1;
            }
            a += 1;
        }
    } else {
        in_window = false;
        fam_unique = false;
    }
    out.push((
        "F48999/码族唯一",
        fam.is_ok() && in_window && fam_unique && codes_are_distinct(),
        format!(
            "len={:?} window={} unique={} own={}",
            fam.as_ref().map(|v| v.len()),
            in_window,
            fam_unique,
            codes_are_distinct()
        ),
    ));

    // F49000 ①：20 条记录可解析（卡/模块/判据/摘要逐条自洽）。
    let recs = build_ledger();
    let rec_ok = match &recs {
        Ok(v) => v.len() == CARDS_TOTAL && seq_cards && ledger_cards,
        Err(_) => false,
    };
    out.push((
        "F49000/记录可解析",
        rec_ok,
        format!(
            "records={:?} expect={} seq={} fields={}",
            recs.as_ref().map(|v| v.len()),
            CARDS_TOTAL,
            seq_cards,
            ledger_cards
        ),
    ));

    // F49000 ②：封印可复算且两次逐位一致。
    let seal_a = match &recs {
        Ok(v) => seal_digest(v),
        Err(_) => Err(SlCode::LedgerBad.val()),
    };
    let seal_b = match &recs {
        Ok(v) => seal_digest(v),
        Err(_) => Err(SlCode::LedgerBad.val()),
    };
    let verified = match (&recs, &seal_a) {
        (Ok(v), Ok(d)) => verify_seal(v, *d),
        _ => Err(SlCode::SealBad.val()),
    };
    out.push((
        "F49000/封印可复算",
        seal_a.is_ok()
            && seal_a == seal_b
            && seal_a.as_ref().map(|d| *d != 0).unwrap_or(false)
            && verified == Ok(CARDS_TOTAL),
        format!(
            "seal={:?} stable={} verified={:?}",
            seal_a,
            seal_a == seal_b,
            verified
        ),
    ));

    // F49000 ③：篡改必拒（摘要/判据/错摘要/乱序四路）。
    let mut tamper_ok = false;
    if let Ok(base_recs) = build_ledger() {
        let mut r1 = base_recs;
        if let Some(slot) = r1.get_mut(7) {
            slot.digest ^= 0x0001_0001;
        }
        let d1 = seal_digest(&r1).err() == Some(SlCode::SealBad.val());

        let mut r2 = base_recs;
        if let Some(slot) = r2.get_mut(9) {
            slot.gates = 7;
        }
        let d2 = seal_digest(&r2).err() == Some(SlCode::GateBad.val());

        let wrong = seal_digest(&base_recs)
            .map(|d| d ^ 0xFFFF_FFFF)
            .unwrap_or(0);
        let d3 = verify_seal(&base_recs, wrong).err() == Some(SlCode::SealBad.val());

        let mut r4 = base_recs;
        let a = r4.get(0).copied();
        let b = r4.get(1).copied();
        if let (Some(a), Some(b)) = (a, b) {
            if let Some(slot) = r4.get_mut(0) {
                *slot = b;
            }
            if let Some(slot) = r4.get_mut(1) {
                *slot = a;
            }
        }
        let d4 = seal_digest(&r4).err() == Some(SlCode::OrderBad.val());

        tamper_ok = d1 && d2 && d3 && d4;
    }
    out.push((
        "F49000/篡改必拒",
        tamper_ok,
        format!("tamper_ok={}", tamper_ok),
    ));

    // F49000 ④：交接完整 + 批印入册（60000/240000，版本链不回滚）。
    let ho = handover_probe();
    let ho_ok = match &ho {
        Ok(h) => {
            h.cards == CARDS_TOTAL as u32
                && h.gates == GATES_TOTAL as u32
                && h.verified == CARDS_TOTAL as u32
                && seal_a.as_ref().map(|d| *d == h.digest).unwrap_or(false)
        }
        Err(_) => false,
    };
    let rc = batch_receipt();
    let budget = budget_reconcile();
    let budget_ok = budget
        .map(|s| s == BATCH_LINES - PRIOR_BATCH_LINES)
        .unwrap_or(false);
    let rc_ok = match &rc {
        Ok(r) => {
            r.checked()
                && budget_ok
                && seal_a.as_ref().map(|d| *d == r.digest).unwrap_or(false)
        }
        Err(_) => false,
    };
    let first = SealChain {
        version: 1,
        digest: 0x1234_5678,
    };
    let next = seal_append(first, 0x9ABC_DEF0);
    let chain_ok = match &next {
        Ok(c) => {
            c.version == 2
                && c.digest == 0x9ABC_DEF0
                && seal_append(*c, 0x9ABC_DEF0).is_err()
                && seal_append(*c, 0).is_err()
                && seal_no_rollback(*c, 1).is_err()
                && seal_no_rollback(*c, 2) == Ok(2)
                && seal_no_rollback(*c, 5) == Ok(5)
        }
        Err(_) => false,
    };
    out.push((
        "F49000/交接与批印",
        ho_ok && rc_ok && chain_ok,
        format!(
            "cards={:?} lines={:?} ver={:?} chain={}",
            ho.as_ref().map(|h| h.cards),
            rc.as_ref().map(|r| r.lines),
            rc.as_ref().map(|r| r.version),
            chain_ok
        ),
    ));

    out
}

pub fn run_unx48999_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49000 B10 聚合封印与交接");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71880);
        assert_eq!(MODULES_N, 10);
        assert_eq!(CARDS_PER_MODULE, 2);
        assert_eq!(CARDS_TOTAL, 20);
        assert_eq!(FIRST_CARD, 48_981);
        assert_eq!(LAST_CARD, 49_000);
        assert_eq!(CODES_PER_MODULE, 14);
        assert_eq!(CODES_TOTAL, 140);
        assert_eq!(GATES_PER_MODULE, 8);
        assert_eq!(GATES_TOTAL, 80);
        assert_eq!(SLOT_STRIDE, 0x40);
        assert_eq!(SlCode::ALL.len(), 14);
        assert_eq!(STATS_FIELDS, 6);
        assert_eq!(INJECT_ROUTES, 4);
    }

    #[test]
    fn codes_in_window_and_distinct() {
        assert!(codes_are_distinct());
        assert_eq!(segment_guard(), Ok(()));
        let mut i = 0usize;
        while i < SlCode::ALL.len() {
            let c = *SlCode::ALL.get(i).unwrap();
            let off = c.val() - SEG_BASE;
            assert!(off >= 0x01 && off <= 0x0F);
            assert!(!c.detail().is_empty());
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        assert_eq!(detail(0xDEAD), "未知封印诊断码");
    }

    #[test]
    fn module_table_matches_segment_map() {
        assert_eq!(MODULES.len(), MODULES_N);
        let mut i = 0usize;
        while i < MODULES.len() {
            let m = *MODULES.get(i).unwrap();
            assert_eq!(m.base, 0x71640 + 0x40 * i as u32);
            assert_eq!(m.first_card, FIRST_CARD + 2 * i as u16);
            assert_eq!(m.base % SLOT_STRIDE, 0);
            assert!(!m.name.is_empty());
            i += 1;
        }
    }

    #[test]
    fn segments_ok_with_45_pairs() {
        assert_eq!(check_segments(&MODULES), Ok(45));
    }

    #[test]
    fn segments_reject_short_table() {
        assert_eq!(
            check_segments(&MODULES[..MODULES_N - 1]).err(),
            Some(SlCode::CoverBad.val())
        );
    }

    #[test]
    fn segments_reject_unaligned_base() {
        let mut mods = table_copy();
        mods.get_mut(4).unwrap().base += 0x20;
        assert_eq!(check_segments(&mods).err(), Some(SlCode::SegmentBad.val()));
    }

    #[test]
    fn segments_reject_equal_base() {
        let mut mods = table_copy();
        mods.get_mut(3).unwrap().base = mods.get(2).unwrap().base;
        assert_eq!(check_segments(&mods).err(), Some(SlCode::OverlapBad.val()));
    }

    #[test]
    fn segments_reject_descent() {
        let mut mods = table_copy();
        let a = mods.get(0).unwrap().base;
        let b = mods.get(1).unwrap().base;
        mods.get_mut(0).unwrap().base = b;
        mods.get_mut(1).unwrap().base = a;
        assert_eq!(check_segments(&mods).err(), Some(SlCode::OverlapBad.val()));
    }

    #[test]
    fn window_bounds() {
        assert_eq!(check_window(0x71880), Ok(0x7188F));
        assert_eq!(check_window(0x71881).err(), Some(SlCode::SegmentBad.val()));
        assert_eq!(
            check_window(u32::MAX - 0x08).err(),
            Some(SlCode::SegmentBad.val())
        );
        assert_eq!(check_window(u32::MAX - 0x3F), Ok(0xFFFF_FFCF));
    }

    #[test]
    fn cards_are_contiguous() {
        assert_eq!(cards_ok(&MODULES), Ok(LAST_CARD));
    }

    #[test]
    fn cards_reject_duplicate() {
        let mut mods = table_copy();
        mods.get_mut(1).unwrap().first_card = FIRST_CARD;
        assert_eq!(cards_ok(&mods).err(), Some(SlCode::CardBad.val()));
    }

    #[test]
    fn cards_reject_overflow_range() {
        let mut mods = table_copy();
        mods.get_mut(9).unwrap().first_card = LAST_CARD;
        assert_eq!(cards_ok(&mods).err(), Some(SlCode::CardBad.val()));
    }

    #[test]
    fn code_family_is_140_unique() {
        let v = code_family(&MODULES).unwrap();
        assert_eq!(v.len(), CODES_TOTAL);
        let mut i = 0usize;
        while i < v.len() {
            let mut j = i + 1;
            while j < v.len() {
                assert_ne!(v.get(i), v.get(j));
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn code_family_rejects_duplicate_window() {
        let mut mods = table_copy();
        mods.get_mut(7).unwrap().base = 0x71780;
        assert_eq!(code_family(&mods).err(), Some(SlCode::CodeBad.val()));
    }

    #[test]
    fn reconciled_units() {
        assert!(SealStats {
            modules: 10,
            cards: 20,
            codes: 140,
            gates: 80,
            sealed: 1,
            digest: 0x1234,
        }
        .reconciled());
        assert!(!SealStats {
            modules: 9,
            cards: 20,
            codes: 140,
            gates: 80,
            sealed: 1,
            digest: 0x1234,
        }
        .reconciled());
        assert!(!SealStats {
            modules: 10,
            cards: 19,
            codes: 140,
            gates: 80,
            sealed: 1,
            digest: 0x1234,
        }
        .reconciled());
    }

    #[test]
    fn audit_rejects_bad_dimensions_and_seal() {
        let good = SealStats {
            modules: 10,
            cards: 20,
            codes: 140,
            gates: 80,
            sealed: 1,
            digest: 0x1234,
        };
        assert_eq!(good.audit(), Ok(()));
        let mut bad = good;
        bad.codes = 139;
        assert_eq!(bad.audit().err(), Some(SlCode::AggregateBad.val()));
        let mut bad = good;
        bad.sealed = 0;
        assert_eq!(bad.audit().err(), Some(SlCode::SealBad.val()));
        let mut bad = good;
        bad.digest = 0;
        assert_eq!(bad.audit().err(), Some(SlCode::ReplayBad.val()));
    }

    #[test]
    fn ledger_has_20_well_formed_records() {
        let recs = build_ledger().unwrap();
        assert_eq!(recs.len(), CARDS_TOTAL);
        let mut i = 0usize;
        while i < recs.len() {
            let r = *recs.get(i).unwrap();
            assert_eq!(r.card, FIRST_CARD + i as u16);
            assert_eq!(r.module as usize, i / CARDS_PER_MODULE as usize);
            assert_eq!(r.gates as usize, GATES_PER_MODULE);
            let base = MODULES.get(r.module as usize).unwrap().base;
            assert_eq!(r.digest, record_digest(r.card, base));
            assert_ne!(r.digest, 0);
            i += 1;
        }
    }

    #[test]
    fn record_digest_is_stable_nonzero() {
        let a = record_digest(48_981, 0x71640);
        let b = record_digest(48_981, 0x71640);
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert_ne!(a, record_digest(48_982, 0x71640));
        assert_ne!(a, record_digest(48_981, 0x71680));
    }

    #[test]
    fn seal_digest_deterministic() {
        let recs = build_ledger().unwrap();
        let a = seal_digest(&recs).unwrap();
        let b = seal_digest(&recs).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    #[test]
    fn verify_seal_accepts_and_rejects() {
        let recs = build_ledger().unwrap();
        let d = seal_digest(&recs).unwrap();
        assert_eq!(verify_seal(&recs, d), Ok(CARDS_TOTAL));
        assert_eq!(
            verify_seal(&recs, d ^ 0xFFFF_FFFF).err(),
            Some(SlCode::SealBad.val())
        );
    }

    #[test]
    fn handover_is_complete() {
        let h = handover_probe().unwrap();
        assert_eq!(h.cards, CARDS_TOTAL as u32);
        assert_eq!(h.gates, GATES_TOTAL as u32);
        assert_eq!(h.verified, CARDS_TOTAL as u32);
        assert_eq!(h.digest, seal_digest(&build_ledger().unwrap()).unwrap());
    }

    #[test]
    fn aggregation_matches_ledger() {
        let s = aggregate_probe().unwrap();
        assert!(s.reconciled());
        assert_eq!(s.audit(), Ok(()));
        assert_eq!(s.digest, seal_digest(&build_ledger().unwrap()).unwrap());
    }

    #[test]
    fn inject_catches_four_routes() {
        let log = inject_rounds(32).unwrap();
        assert!(log.all_caught());
        assert_eq!(log.tries, [32; INJECT_ROUTES]);
        assert_eq!(log.caught, [32; INJECT_ROUTES]);
    }

    #[test]
    fn inject_rejects_bad_rounds() {
        assert_eq!(inject_rounds(0).err(), Some(SlCode::CoverBad.val()));
        assert_eq!(
            inject_rounds(FUZZ_ROUNDS + 1).err(),
            Some(SlCode::CoverBad.val())
        );
    }

    #[test]
    fn render_roundtrip() {
        let s = aggregate_probe().unwrap();
        let text = render_stats(&s);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
        assert_eq!(
            render_stats(&s),
            format!(
                "10|20|140|80|1|0x{:016X}",
                s.digest
            )
        );
    }

    #[test]
    fn render_rejects_malformed() {
        assert_eq!(
            audit_render("10|20|140|80|1").err(),
            Some(SlCode::RenderBad.val())
        );
        assert_eq!(
            audit_render("10|20|140|80|x|0x0000000000000001").err(),
            Some(SlCode::RenderBad.val())
        );
        assert_eq!(
            audit_render("0|0|0|0|1|0x0000000000000001").err(),
            Some(SlCode::AggregateBad.val())
        );
        assert_eq!(
            audit_render("10|20|140|80|5|0x0000000000000001").err(),
            Some(SlCode::SealBad.val())
        );
        assert_eq!(
            audit_render("10|20|140|80|1|0x0000000000000000").err(),
            Some(SlCode::RenderBad.val())
        );
        assert_eq!(
            audit_render("10|21|140|80|1|0x0000000000000001").err(),
            Some(SlCode::AggregateBad.val())
        );
    }

    #[test]
    fn golden_is_deterministic() {
        let a = golden_run().unwrap();
        let b = golden_run().unwrap();
        assert_eq!(a, b);
        assert!(a.reconciled());
        assert_eq!(a.sealed, 1);
    }

    #[test]
    fn repro_is_stable() {
        assert_eq!(repro_check(), Ok(()));
    }

    #[test]
    fn probe_sequences_nonzero() {
        let a = probe_seq_a().unwrap();
        let b = probe_seq_b().unwrap();
        let c = probe_seq_c().unwrap();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(c, 0);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn dup_ledger_covers_all_cards() {
        assert_eq!(DUP_LEDGER.len(), CARDS_TOTAL);
        assert_eq!(DUP_FAMILIES.len(), 3);
        let mut i = 0usize;
        while i < DUP_LEDGER.len() {
            let r = *DUP_LEDGER.get(i).unwrap();
            assert_eq!(r.card, FIRST_CARD + i as u16);
            let expect_mod = *MODULES.get(i / CARDS_PER_MODULE as usize).unwrap();
            assert_eq!(r.module, expect_mod.name);
            assert!(!r.tags.is_empty());
            i += 1;
        }
    }

    #[test]
    fn family_hits_is_ascii_ci_and_non_overlapping() {
        assert_eq!(family_hits("gesture Gesture GESTURE", "gesture"), 3);
        assert_eq!(family_hits("unx48981_gesture_core", "GESTURE"), 1);
        assert_eq!(family_hits("aaaa", "aa"), 2);
        assert_eq!(family_hits("", "pinch"), 0);
        assert_eq!(family_hits("pinch", ""), 0);
        assert_eq!(family_hits("pinch", "pinches"), 0);
    }

    #[test]
    fn dup_audit_is_clean_and_stable() {
        let a = dup_audit().unwrap();
        let b = dup_audit().unwrap();
        assert!(a.clean());
        assert_eq!(a.cards, CARDS_TOTAL as u32);
        assert_eq!(a.modules, MODULES_N as u32);
        assert_eq!(a.collisions, 0);
        assert!(a.hits[0] > 0 && a.hits[1] > 0 && a.hits[2] > 0);
        assert_ne!(a.fingerprint, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn batch_receipt_lands_60000() {
        let r = batch_receipt().unwrap();
        assert!(r.checked());
        assert_eq!(r.cards, 20);
        assert_eq!(r.lines, 60_000);
        assert_eq!(r.domain_total, 240_000);
        assert_eq!(r.prior_lines, 54_000);
        assert_eq!(r.version, 2);
        let mut bad = r;
        bad.lines = 53_000;
        assert!(!bad.checked());
        let mut bad2 = r;
        bad2.version = 1;
        assert!(!bad2.checked());
        let mut bad3 = r;
        bad3.digest = 0;
        assert!(!bad3.checked());
    }

    #[test]
    fn per_card_budget_reconciles_batch_lines() {
        assert_eq!(budget_sum(), 6_000);
        assert_eq!(budget_reconcile(), Ok(6_000));
        assert_eq!(budget_reconcile().ok(), Some(BATCH_LINES - PRIOR_BATCH_LINES));
        let mut manual = 0u32;
        let mut i = 0usize;
        while i < CARD_BUDGETS.len() {
            manual += CARD_BUDGETS[i];
            i += 1;
        }
        assert_eq!(manual, budget_sum());
        assert!(CARD_BUDGETS.iter().all(|v| *v >= 200));
    }

    #[test]
    fn seal_chain_never_rolls_back() {
        let c = SealChain {
            version: 1,
            digest: 0xAA,
        };
        let n = seal_append(c, 0xBB).unwrap();
        assert_eq!(
            n,
            SealChain {
                version: 2,
                digest: 0xBB
            }
        );
        assert_eq!(seal_append(n, 0xBB), Err(SlCode::SealBad.val()));
        assert_eq!(seal_append(n, 0), Err(SlCode::SealBad.val()));
        assert_eq!(seal_no_rollback(n, 1), Err(SlCode::OrderBad.val()));
        assert_eq!(seal_no_rollback(n, 2), Ok(2));
        assert_eq!(seal_no_rollback(n, 7), Ok(7));
    }

    #[test]
    fn extra_checks_are_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            let (name, passed, detail) = items.get(i).unwrap();
            assert!(*passed, "{} -> {}", name, detail);
            i += 1;
        }
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48999_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
