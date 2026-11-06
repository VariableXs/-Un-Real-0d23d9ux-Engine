//! UNX-F48819~F48820：B01 批收口防重 grep 与 20 条冻结 / 6000 行收官印。
//!
//! ## 闸门次序（收官只认这四道闸，按序短路）
//!
//! 1. **关键词归属闸（F48819）**：`input_event` / `evbit` / `frame` / `syn`
//!    四个关键词各只允许一个属主（模块 + 卡号），重复即 KeywordDup；
//!    属主卡必须落在本批 48801~48820，且卡号与模块文件的对应关系必须
//!    与冻结台账一致，错配即 CardMismatch。
//! 2. **撞号闸（F48819）**：八个模块的段窗口按基址升序、两两不重叠，
//!    每窗跨度 ≥0x20（诊断码只占窗口前 0x20 个码位），窗间允许预留空隙。
//!    重叠即 CodeCollision，乱序/跨度不足即 WindowGap；末窗
//!    `0x70240+0x40` 必须正好压在 B02 的 `0x70280`，这是 B01 与 B02
//!    的硬边界。
//! 3. **台账闸（F48820）**：20 条卡号连续无洞、每条行数目标 ≥200、
//!    合计必须恰为 6000（卡面「6000/240,000」的分子）；模块底线台账
//!    合计 6200，必须 ≥6000。
//! 4. **印文闸（F48820）**：收官印两份副本逐字节相等且指纹等于冻结值，
//!    任何一字节改写都让印失效（StampDrift）。
//!
//! ## 关键差异（对 B01 内其他段）
//!
//! * `unx48813` 锚定的是「AI-02 联签声明」，本模块锚定的是「整批台账 +
//!   印文」；两者都用双副本 + 冻结指纹，但锚的对象不同。
//! * `unx48816` 判的是运行期观测质量，本模块判的是**静态面**：词、窗、
//!   账、印，全部是编译期常量，收官时逐条核对，不跑业务流。
//! * 本模块是 B01 唯一汇总点：`run_unx48819_checks()` 通过
//!   `CheckSet::merge` 把前 7 个模块的自检集并入自己的印，任何一段红
//!   都会在总印上现形。合并后检查项 83 条，低于 `MAX_CHECKS=112`，
//!   不会被截断吞红。
//!
//! 卡面口径：
//! * F48819-J1 批收口防重留痕零撞号（锚定 §6.1）。
//! * F48820-J1 批印入册：6000/240,000（锚定 §AI-62 §7）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x70240;

/// 错误码族条数。
pub const CODE_FAMILY_LEN: usize = 10;
/// 关键词族条数（卡面钉死四个词）。
pub const KEYWORD_COUNT: usize = 4;
/// 本批模块数（7 个功能段 + 本收官段）。
pub const MODULE_COUNT: usize = 8;
/// 本批卡数。
pub const CARD_COUNT: usize = 20;
/// 首卡号。
pub const CARD_BASE: u16 = 48801;
/// 20 条卡面行数目标合计（F48820 的分子）。
pub const LEDGER_FROZEN_SUM: u32 = 6_000;
/// 8 个模块的 PLAN 底线合计。
pub const FLOOR_TARGET_SUM: u32 = 6_200;
/// 全批行数预算（F48820 的分母）。
pub const BATCH_ROW_BUDGET: u32 = 240_000;
/// B02 的段基址（本批末窗必须恰好压在这里）。
pub const NEXT_BATCH_SEG: u32 = 0x70280;

// ── 错误码族 ──────────────────────────────────────────────────
/// 收官面诊断码（窗口 `[SEG_BASE+1, SEG_BASE+0x20)`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 关键词重复登记（两个属主）。
    KeywordDup = 0,
    /// 关键词无属主或属主不合法。
    KeywordUnowned,
    /// 段窗口重叠（撞号）。
    CodeCollision,
    /// 段窗口留缝（台账不连续）。
    WindowGap,
    /// 卡台账有洞（条数不足/卡号跳号）。
    LedgerHole,
    /// 行数合计不足或单卡目标清零。
    LedgerShort,
    /// 卡号与模块对应关系错配。
    CardMismatch,
    /// 印文被改写（双副本不等或指纹不符）。
    StampDrift,
    /// 合并后自检集为空（前段集体失联）。
    MergeEmpty,
    /// 收官印自检失败（兜底码）。
    SealBroken,
}

impl SealCode {
    /// 全族。
    pub const ALL: [SealCode; CODE_FAMILY_LEN] = [
        SealCode::KeywordDup,
        SealCode::KeywordUnowned,
        SealCode::CodeCollision,
        SealCode::WindowGap,
        SealCode::LedgerHole,
        SealCode::LedgerShort,
        SealCode::CardMismatch,
        SealCode::StampDrift,
        SealCode::MergeEmpty,
        SealCode::SealBroken,
    ];

    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 判据文本。
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::KeywordDup => "关键词重复登记",
            SealCode::KeywordUnowned => "关键词属主非法",
            SealCode::CodeCollision => "段窗口重叠撞号",
            SealCode::WindowGap => "段窗口留缝",
            SealCode::LedgerHole => "卡台账有洞",
            SealCode::LedgerShort => "行数合计不足",
            SealCode::CardMismatch => "卡号与模块错配",
            SealCode::StampDrift => "收官印被改写",
            SealCode::MergeEmpty => "合并自检集为空",
            SealCode::SealBroken => "收官印自检失败",
        }
    }
}

/// 码值反查（段外空串）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    ""
}

/// 码族自检：连续 + 窗口内 + 文本非空。
pub fn code_family_check() -> Result<u32, u32> {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 {
                return Err(SealCode::SealBroken.val());
            }
            if c.val() >= SEG_BASE + 0x20 || c.detail().is_empty() {
                return Err(SealCode::SealBroken.val());
            }
        }
        i += 1;
    }
    Ok(SealCode::ALL.len() as u32)
}

// ── 指纹基础设施（本域按「印纹」命名）────────────────────────
/// 印纹初值。
pub fn stamp_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn stamp_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 32 位字。
pub fn stamp_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = stamp_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入字符串。
pub fn stamp_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc = stamp_byte(acc, *b);
        }
        i += 1;
    }
    acc
}

// ── 关键词族（F48819 防重 grep 的静态锚）────────────────────
/// 关键词登记行：一个词只有一个属主。
#[derive(Clone, Copy, Debug)]
pub struct KeywordDef {
    /// 关键词（小写）。
    pub word: &'static str,
    /// 属主模块文件。
    pub owner_stem: &'static str,
    /// 属主卡号。
    pub owner_card: u16,
    /// 该词在此模块里的语义注记（非空）。
    pub note: &'static str,
}

/// 四个关键词的冻结归属。
pub const KEYWORDS: [KeywordDef; KEYWORD_COUNT] = [
    KeywordDef {
        word: "input_event",
        owner_stem: "unx48801_input_core",
        owner_card: 48801,
        note: "十族事件结构体与 timestamp/type/code/value 四字段",
    },
    KeywordDef {
        word: "evbit",
        owner_stem: "unx48801_input_core",
        owner_card: 48802,
        note: "EVIOCGBIT 位图读取面与一次性缓存",
    },
    KeywordDef {
        word: "frame",
        owner_stem: "unx48803_input_meta",
        owner_card: 48804,
        note: "EV_SYN 帧边界与半帧不可见",
    },
    KeywordDef {
        word: "syn",
        owner_stem: "unx48805_input_guard",
        owner_card: 48805,
        note: "缺 SYN 100ms 超时强制合成",
    },
];

/// 卡号 → 模块文件（冻结映射，错配即 CardMismatch）。
pub fn module_of_card(card: u16) -> &'static str {
    match card {
        48801 | 48802 => "unx48801_input_core",
        48803 | 48804 => "unx48803_input_meta",
        48805 | 48806 | 48807 => "unx48805_input_guard",
        48808 | 48809 | 48810 => "unx48808_input_dispatch",
        48811 | 48812 => "unx48811_input_edge",
        48813 | 48814 | 48815 => "unx48813_input_bridge",
        48816 | 48817 | 48818 => "unx48816_input_quality",
        48819 | 48820 => "unx48819_input_seal",
        _ => "",
    }
}

/// 关键词是否全为小写字母/数字/下划线。
fn is_word(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            let ok = (*b >= b'a' && *b <= b'z') || (*b >= b'0' && *b <= b'9') || *b == b'_';
            if !ok {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 关键词行审校（可注入任意行集，供演练复跑）。
pub fn keyword_check_rows(rows: &[KeywordDef]) -> Result<u32, u32> {
    if rows.len() != KEYWORD_COUNT {
        return Err(SealCode::KeywordUnowned.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let a = match rows.get(i) {
            Some(v) => v,
            None => return Err(SealCode::KeywordUnowned.val()),
        };
        if !is_word(a.word) || a.note.is_empty() || a.owner_stem.is_empty() {
            return Err(SealCode::KeywordUnowned.val());
        }
        if module_of_card(a.owner_card) != a.owner_stem {
            return Err(SealCode::CardMismatch.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            if let Some(b) = rows.get(j) {
                if b.word == a.word {
                    return Err(SealCode::KeywordDup.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(rows.len() as u32)
}

/// 冻结关键词族审校。
pub fn keyword_registry_check() -> Result<u32, u32> {
    keyword_check_rows(&KEYWORDS)
}

// ── 段窗口族（撞号闸）────────────────────────────────────────
/// 模块段窗口。
#[derive(Clone, Copy, Debug)]
pub struct WindowDef {
    /// 模块文件。
    pub stem: &'static str,
    /// 段基址。
    pub seg: u32,
    /// 窗口跨度（必须 ≥0x20，按 0x40 相邻）。
    pub span: u32,
}

/// 八个模块的冻结段窗口。
pub const WINDOWS: [WindowDef; MODULE_COUNT] = [
    WindowDef { stem: "unx48801_input_core", seg: 0x70000, span: 0x40 },
    WindowDef { stem: "unx48803_input_meta", seg: 0x70040, span: 0x40 },
    WindowDef { stem: "unx48805_input_guard", seg: 0x70080, span: 0x40 },
    WindowDef { stem: "unx48808_input_dispatch", seg: 0x700E0, span: 0x40 },
    WindowDef { stem: "unx48811_input_edge", seg: 0x70140, span: 0x40 },
    WindowDef { stem: "unx48813_input_bridge", seg: 0x70180, span: 0x40 },
    WindowDef { stem: "unx48816_input_quality", seg: 0x701E0, span: 0x40 },
    WindowDef { stem: "unx48819_input_seal", seg: 0x70240, span: 0x40 },
];

/// 段窗口审校：互不重叠、按基址升序、跨度合法、末窗压在 B02 边界。
///
/// 注意：相邻窗口之间允许留「预留空隙」（本批实际布局 0x70080→0x700E0
/// 间隔 0x60），空隙不是错误；只有**重叠**才撞号。真正的连续性由
/// 「每个窗口前 0x20 装诊断码、窗口跨度 ≥0x20」与「末窗顶到 0x70280」
/// 两条共同保证。
pub fn window_check_rows(rows: &[WindowDef]) -> Result<u32, u32> {
    if rows.len() != MODULE_COUNT {
        return Err(SealCode::WindowGap.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let cur = match rows.get(i) {
            Some(v) => v,
            None => return Err(SealCode::WindowGap.val()),
        };
        if cur.stem.is_empty() || cur.span < 0x20 {
            return Err(SealCode::WindowGap.val());
        }
        if cur.seg % 0x20 != 0 {
            return Err(SealCode::WindowGap.val());
        }
        if i > 0 {
            let prev = match rows.get(i - 1) {
                Some(v) => v,
                None => return Err(SealCode::WindowGap.val()),
            };
            let end = match prev.seg.checked_add(prev.span) {
                Some(v) => v,
                None => return Err(SealCode::SealBroken.val()),
            };
            if cur.seg <= prev.seg {
                return Err(SealCode::WindowGap.val());
            }
            if cur.seg < end {
                return Err(SealCode::CodeCollision.val());
            }
        }
        i += 1;
    }
    // 末窗必须恰好顶在 B02 段基址上（批间零撞号）。
    let last = match rows.last() {
        Some(v) => v,
        None => return Err(SealCode::WindowGap.val()),
    };
    if last.seg != SEG_BASE {
        return Err(SealCode::WindowGap.val());
    }
    match last.seg.checked_add(last.span) {
        Some(v) if v == NEXT_BATCH_SEG => {}
        _ => return Err(SealCode::WindowGap.val()),
    }
    Ok(rows.len() as u32)
}

/// 冻结段窗口审校。
pub fn window_check() -> Result<u32, u32> {
    window_check_rows(&WINDOWS)
}

// ── 20 条卡台账（F48820 的分子）──────────────────────────────
/// 卡台账行：卡面行数目标 + 归属模块。
#[derive(Clone, Copy, Debug)]
pub struct CardRow {
    /// 卡号（48801..48820）。
    pub card: u16,
    /// 归属模块。
    pub stem: &'static str,
    /// 卡面行数目标。
    pub target: u32,
}

/// 20 条卡的冻结台账（目标合计 6000）。
pub const CARDS: [CardRow; CARD_COUNT] = [
    CardRow { card: 48801, stem: "unx48801_input_core", target: 420 },
    CardRow { card: 48802, stem: "unx48801_input_core", target: 460 },
    CardRow { card: 48803, stem: "unx48803_input_meta", target: 400 },
    CardRow { card: 48804, stem: "unx48803_input_meta", target: 380 },
    CardRow { card: 48805, stem: "unx48805_input_guard", target: 360 },
    CardRow { card: 48806, stem: "unx48805_input_guard", target: 340 },
    CardRow { card: 48807, stem: "unx48805_input_guard", target: 320 },
    CardRow { card: 48808, stem: "unx48808_input_dispatch", target: 320 },
    CardRow { card: 48809, stem: "unx48808_input_dispatch", target: 300 },
    CardRow { card: 48810, stem: "unx48808_input_dispatch", target: 300 },
    CardRow { card: 48811, stem: "unx48811_input_edge", target: 280 },
    CardRow { card: 48812, stem: "unx48811_input_edge", target: 260 },
    CardRow { card: 48813, stem: "unx48813_input_bridge", target: 260 },
    CardRow { card: 48814, stem: "unx48813_input_bridge", target: 200 },
    CardRow { card: 48815, stem: "unx48813_input_bridge", target: 220 },
    CardRow { card: 48816, stem: "unx48816_input_quality", target: 220 },
    CardRow { card: 48817, stem: "unx48816_input_quality", target: 260 },
    CardRow { card: 48818, stem: "unx48816_input_quality", target: 300 },
    CardRow { card: 48819, stem: "unx48819_input_seal", target: 200 },
    CardRow { card: 48820, stem: "unx48819_input_seal", target: 200 },
];

/// 8 个模块的 PLAN 底线（合计 6200）。
pub const MODULE_FLOORS: [(&str, u32); MODULE_COUNT] = [
    ("unx48801_input_core", 900),
    ("unx48803_input_meta", 850),
    ("unx48805_input_guard", 900),
    ("unx48808_input_dispatch", 850),
    ("unx48811_input_edge", 700),
    ("unx48813_input_bridge", 750),
    ("unx48816_input_quality", 900),
    ("unx48819_input_seal", 350),
];

/// 卡台账审校：连续、无洞、模块匹配、合计恰为 6000。
pub fn ledger_check_rows(rows: &[CardRow]) -> Result<u32, u32> {
    if rows.len() != CARD_COUNT {
        return Err(SealCode::LedgerHole.val());
    }
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return Err(SealCode::LedgerHole.val()),
        };
        let want = CARD_BASE + i as u16;
        if r.card != want {
            return Err(SealCode::LedgerHole.val());
        }
        if r.target < 200 {
            return Err(SealCode::LedgerShort.val());
        }
        if r.stem.is_empty() || module_of_card(r.card) != r.stem {
            return Err(SealCode::CardMismatch.val());
        }
        sum = match sum.checked_add(r.target) {
            Some(v) => v,
            None => return Err(SealCode::SealBroken.val()),
        };
        i += 1;
    }
    if sum != LEDGER_FROZEN_SUM {
        return Err(SealCode::LedgerShort.val());
    }
    Ok(sum)
}

/// 冻结卡台账审校。
pub fn ledger_check() -> Result<u32, u32> {
    ledger_check_rows(&CARDS)
}

/// 模块底线合计。
pub fn floor_sum() -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < MODULE_FLOORS.len() {
        if let Some((_, v)) = MODULE_FLOORS.get(i) {
            sum = sum.saturating_add(*v);
        }
        i += 1;
    }
    sum
}

// ── 收官印文（F48820 的印）────────────────────────────────────
/// 事件层持有的印文副本（A）。
pub const STAMP_LINE_A: &str =
    "B01 seal: 20 cards frozen, 6000 rows declared; owners input_event/evbit/frame/syn";
/// 审计方持有的印文副本（B）。
pub const STAMP_LINE_B: &str =
    "B01 seal: 20 cards frozen, 6000 rows declared; owners input_event/evbit/frame/syn";
/// 印文冻结指纹（离线算定后钉死）。
pub const STAMP_HASH_FROZEN: u64 = 0x9A6C_DAB6_F0FC_CE23;

/// 计算印文指纹。
pub fn stamp_hash(text: &str) -> u64 {
    stamp_text(stamp_init(), text)
}

/// 印文审校：双副本逐字节相等 + 指纹等于冻结值。
pub fn stamp_check_text(a: &str, b: &str) -> Result<(), u32> {
    if a.as_bytes() != b.as_bytes() {
        return Err(SealCode::StampDrift.val());
    }
    let ha = stamp_hash(a);
    let hb = stamp_hash(b);
    if ha != hb || ha != STAMP_HASH_FROZEN {
        return Err(SealCode::StampDrift.val());
    }
    Ok(())
}

/// 冻结印文零改写判据。
pub fn stamp_zero_rewrite() -> Result<(), u32> {
    stamp_check_text(STAMP_LINE_A, STAMP_LINE_B)
}

// ── 收官汇总 ─────────────────────────────────────────────────
/// 收官报告。
#[derive(Clone, Copy, Debug)]
pub struct SealReport {
    /// 关键词条数。
    pub keywords: u32,
    /// 窗口条数。
    pub windows: u32,
    /// 卡台账条数。
    pub cards: u32,
    /// 台账合计行数。
    pub rows: u32,
    /// 模块底线合计。
    pub floors: u32,
    /// 总判定。
    pub ok: bool,
    /// 印纹。
    pub digest: u64,
}

/// 关键词族与卡台账的印纹。
fn ledger_digest() -> u64 {
    let mut h = stamp_init();
    let mut i = 0usize;
    while i < KEYWORDS.len() {
        if let Some(k) = KEYWORDS.get(i) {
            h = stamp_text(h, k.word);
            h = stamp_word(h, k.owner_card as u32);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < WINDOWS.len() {
        if let Some(w) = WINDOWS.get(j) {
            h = stamp_text(h, w.stem);
            h = stamp_word(h, w.seg);
            h = stamp_word(h, w.span);
        }
        j += 1;
    }
    let mut k = 0usize;
    while k < CARDS.len() {
        if let Some(c) = CARDS.get(k) {
            h = stamp_word(h, c.card as u32);
            h = stamp_word(h, c.target);
        }
        k += 1;
    }
    h = stamp_word(h, stamp_hash(STAMP_LINE_A) as u32);
    h = stamp_word(h, (stamp_hash(STAMP_LINE_A) >> 32) as u32);
    h
}

/// 黄金收官流：四闸全绿 + 台账合计 + 底线合计。
pub fn golden_run() -> SealReport {
    let kw = keyword_registry_check();
    let wd = window_check();
    let lg = ledger_check();
    let st = stamp_zero_rewrite();
    let floors = floor_sum();
    let ok = kw.is_ok()
        && wd.is_ok()
        && lg.is_ok()
        && st.is_ok()
        && floors >= LEDGER_FROZEN_SUM
        && LEDGER_FROZEN_SUM <= BATCH_ROW_BUDGET
        && code_family_check().is_ok();
    SealReport {
        keywords: kw.unwrap_or(0),
        windows: wd.unwrap_or(0),
        cards: CARDS.len() as u32,
        rows: lg.unwrap_or(0),
        floors,
        ok,
        digest: ledger_digest(),
    }
}

// ── 注入演练（4 槽 ×10 轮）────────────────────────────────────
/// 一条收官注入命中。
#[derive(Clone, Copy, Debug)]
pub struct SealInject {
    /// 槽号（0 词重复 / 1 窗重叠 / 2 台账有洞 / 3 印文漂移）。
    pub slot: u32,
    /// 判据码。
    pub code: u32,
    /// 是否精确命中。
    pub caught: bool,
}

/// 复制关键词行以便改坏（演练只动副本）。
fn keyword_rows_vec() -> Vec<KeywordDef> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < KEYWORDS.len() {
        if let Some(k) = KEYWORDS.get(i) {
            v.push(*k);
        }
        i += 1;
    }
    v
}

/// 复制窗口行。
fn window_rows_vec() -> Vec<WindowDef> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < WINDOWS.len() {
        if let Some(w) = WINDOWS.get(i) {
            v.push(*w);
        }
        i += 1;
    }
    v
}

/// 复制台账行。
fn card_rows_vec() -> Vec<CardRow> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < CARDS.len() {
        if let Some(c) = CARDS.get(i) {
            v.push(*c);
        }
        i += 1;
    }
    v
}

/// 十轮 × 四槽注入（每次都用全新副本，轮间零污染）。
pub fn inject_rounds() -> Vec<SealInject> {
    let mut out = Vec::new();
    let mut round = 0u32;
    while round < 10 {
        // 槽 0：把第二个词改成与第一个相同 → KeywordDup。
        let mut kw = keyword_rows_vec();
        if let Some(first) = kw.first().copied() {
            if let Some(second) = kw.get_mut(1) {
                *second = KeywordDef {
                    word: first.word,
                    owner_stem: second.owner_stem,
                    owner_card: second.owner_card,
                    note: second.note,
                };
            }
        }
        let r = keyword_check_rows(&kw);
        out.push(SealInject {
            slot: 0,
            code: r.err().unwrap_or(0),
            caught: r == Err(SealCode::KeywordDup.val()),
        });
        // 槽 1：把第二窗整体前移到首窗内部 → CodeCollision。
        let mut wd = window_rows_vec();
        if let Some(first) = wd.first().copied() {
            if let Some(second) = wd.get_mut(1) {
                *second = WindowDef {
                    stem: second.stem,
                    seg: first.seg + 0x20,
                    span: second.span,
                };
            }
        }
        let r = window_check_rows(&wd);
        out.push(SealInject {
            slot: 1,
            code: r.err().unwrap_or(0),
            caught: r == Err(SealCode::CodeCollision.val()),
        });
        // 槽 2：台账抽掉末行 → LedgerHole。
        let mut cards = card_rows_vec();
        cards.pop();
        let r = ledger_check_rows(&cards);
        out.push(SealInject {
            slot: 2,
            code: r.err().unwrap_or(0),
            caught: r == Err(SealCode::LedgerHole.val()),
        });
        // 槽 3：印文副本改写一字节 → StampDrift。
        let r = stamp_check_text(STAMP_LINE_A, "B01 seal: 20 cards frozen, 6000 rows declared");
        out.push(SealInject {
            slot: 3,
            code: r.err().unwrap_or(0),
            caught: r == Err(SealCode::StampDrift.val()),
        });
        round += 1;
    }
    out
}

/// 注入是否 40 条全中。
pub fn inject_ok() -> bool {
    let v = inject_rounds();
    if v.len() != 40 {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < v.len() {
        if let Some(h) = v.get(i) {
            let want = match h.slot {
                0 => SealCode::KeywordDup.val(),
                1 => SealCode::CodeCollision.val(),
                2 => SealCode::LedgerHole.val(),
                _ => SealCode::StampDrift.val(),
            };
            ok = ok && h.caught && h.code == want;
        }
        i += 1;
    }
    ok
}

// ── 探针与复跑 ───────────────────────────────────────────────
/// 探针核：四闸 + 四槽注入的确定性折叠。
fn probe_core(seed: u32) -> u64 {
    let mut h = stamp_init();
    h = stamp_word(h, seed);
    let kw = keyword_registry_check();
    h = stamp_word(h, kw.unwrap_or(0));
    let wd = window_check();
    h = stamp_word(h, wd.unwrap_or(0));
    let lg = ledger_check();
    h = stamp_word(h, lg.unwrap_or(0));
    h = stamp_word(h, stamp_zero_rewrite().is_ok() as u32);
    let mut round = 0u32;
    while round < 20 {
        let mut cards = card_rows_vec();
        if let Some(last) = cards.last_mut() {
            last.target = last.target.wrapping_add(round % 3);
        }
        let r = ledger_check_rows(&cards);
        h = stamp_word(h, r.unwrap_or(0));
        h = stamp_word(h, round);
        round += 1;
    }
    h = stamp_word(h, inject_rounds().len() as u32);
    h
}

/// 探针 A。
pub fn probe_seq_a() -> u64 {
    probe_core(0xA1)
}

/// 探针 B（同源双跑）。
pub fn probe_seq_b() -> u64 {
    probe_core(0xA1)
}

/// 探针 C（异源，自洽）。
pub fn probe_seq_c() -> u64 {
    probe_core(0xC3)
}

/// 复跑判据。
pub fn repro_check() -> Result<(), u32> {
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c1 = probe_seq_c();
    let c2 = probe_seq_c();
    if a != b || c1 != c2 {
        return Err(SealCode::SealBroken.val());
    }
    if a == 0 || a == c1 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(())
}

/// 段位自检。
pub fn segment_guard() -> bool {
    match code_family_check() {
        Ok(n) => {
            n as usize == CODE_FAMILY_LEN
                && SealCode::ALL.iter().all(|c| {
                    let v = c.val();
                    v > SEG_BASE && v < SEG_BASE + 0x20 && !c.detail().is_empty()
                })
                && WINDOWS.last().map(|w| w.seg).unwrap_or(0) == SEG_BASE
        }
        Err(_) => false,
    }
}

// ── 判据清单 ─────────────────────────────────────────────────
/// 本模块 10 条判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48819-J1：四个词各一属主、无重复。
    v.push((
        "F48819-J1-关键词四条唯一归属",
        keyword_registry_check() == Ok(KEYWORD_COUNT as u32) && inject_ok(),
        format!("words={} 注入40条全中", KEYWORDS.len()),
    ));

    // F48819-J1：属主卡在批内且卡号-模块对应正确。
    let owners_ok = KEYWORDS.iter().all(|k| {
        k.owner_card >= CARD_BASE
            && k.owner_card < CARD_BASE + CARD_COUNT as u16
            && module_of_card(k.owner_card) == k.owner_stem
    });
    v.push((
        "F48819-J1-属主卡在批内且模块匹配",
        owners_ok,
        String::from("48801/48802/48804/48805 四个属主"),
    ));

    // F48819-J1：八窗互不重叠、升序、跨度合法。
    v.push((
        "F48819-J1-八模块窗口零重叠",
        window_check() == Ok(MODULE_COUNT as u32),
        format!("segs={} 跨度均>=0x20", WINDOWS.len()),
    ));

    // F48819-J1：末窗正好压在 B02 段基址（批间边界）。
    let boundary_ok = WINDOWS
        .last()
        .map(|w| w.seg + w.span == NEXT_BATCH_SEG)
        .unwrap_or(false);
    v.push((
        "F48819-J1-末窗压B02边界",
        boundary_ok && segment_guard(),
        format!("0x{:X}=0x{:X}", NEXT_BATCH_SEG, NEXT_BATCH_SEG),
    ));

    // F48819-J2：20 卡连续无洞。
    let cards_ok = CARDS.iter().enumerate().all(|(i, c)| {
        c.card == CARD_BASE + i as u16 && c.target >= 200 && module_of_card(c.card) == c.stem
    });
    v.push((
        "F48819-J2-20卡台账连续无洞",
        cards_ok && CARDS.len() == CARD_COUNT,
        format!("cards={} 首末={}", CARDS.len(), CARD_BASE + CARD_COUNT as u16 - 1),
    ));

    // F48820-J1：台账合计 6000。
    let lg = ledger_check();
    v.push((
        "F48820-J1-台账合计6000",
        lg == Ok(LEDGER_FROZEN_SUM),
        format!("sum={:?} frozen={}", lg, LEDGER_FROZEN_SUM),
    ));

    // F48820-J1：模块底线 6200 ≥ 6000，且不超 240000 预算。
    let floors = floor_sum();
    v.push((
        "F48820-J1-底线6200不低于6000",
        floors == FLOOR_TARGET_SUM && floors >= LEDGER_FROZEN_SUM && LEDGER_FROZEN_SUM <= BATCH_ROW_BUDGET,
        format!("floors={} budget={}", floors, BATCH_ROW_BUDGET),
    ));

    // F48820-J1：印文零改写。
    v.push((
        "F48820-J1-收官印零改写",
        stamp_zero_rewrite().is_ok() && stamp_hash(STAMP_LINE_A) == STAMP_HASH_FROZEN,
        format!("stamp={:x}", STAMP_HASH_FROZEN),
    ));

    // F48819-J3：注入 4 槽 ×10 全中。
    let inj = inject_rounds();
    let inj_ok = inject_ok() && inj.len() == 40;
    v.push((
        "F48819-J3-注入4槽10轮全中",
        inj_ok,
        format!("hits={} caught={}", inj.len(), inj.iter().filter(|h| h.caught).count()),
    ));

    // F48820-J2：双跑与段位。
    let g = golden_run();
    v.push((
        "F48820-J2-双跑与段位",
        g.ok && repro_check().is_ok() && segment_guard(),
        format!("golden={:x} rows={}", g.digest, g.rows),
    ));

    v
}

/// 检查集里是否存在名字含某片段的自检项。
fn has_check(cs: &CheckSet, needle: &str) -> bool {
    let (items, count) = cs.red_items();
    let mut i = 0usize;
    while i < count {
        if let Some(Some(c)) = items.get(i) {
            if c.name.contains(needle) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// B01 收官入口：本段 2 条卡判据 + 10 条细则，再并入前 7 段自检。
///
/// 合并顺序固定为 48801→48816；`CheckSet::merge` 保留本段域名，
/// 任何一段红都会体现在 `all_passed()` 上。
pub fn run_unx48819_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48819 B01 批收口：防重 grep 与 6000 行收官印");
    s.add(
        "UNX-F48819-J1 批收口防重留痕零撞号",
        keyword_registry_check().is_ok()
            && window_check().is_ok()
            && ledger_check().is_ok()
            && inject_ok()
            && segment_guard(),
        "四词唯一归属；八窗互不重叠；末窗顶在 0x70280",
    );
    s.add(
        "UNX-F48820-J1 批印入册：6000/240,000",
        ledger_check() == Ok(LEDGER_FROZEN_SUM)
            && floor_sum() >= LEDGER_FROZEN_SUM
            && floor_sum() == FLOOR_TARGET_SUM
            && stamp_zero_rewrite().is_ok(),
        "20 卡台账合计 6000；8 模块底线 6200；印文冻结指纹相符",
    );
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    let mut merged = CheckSet::merge(s, super::unx48801_input_core::run_unx48801_checks());
    merged = CheckSet::merge(merged, super::unx48803_input_meta::run_unx48803_checks());
    merged = CheckSet::merge(merged, super::unx48805_input_guard::run_unx48805_checks());
    merged = CheckSet::merge(merged, super::unx48808_input_dispatch::run_unx48808_checks());
    merged = CheckSet::merge(merged, super::unx48811_input_edge::run_unx48811_checks());
    merged = CheckSet::merge(merged, super::unx48813_input_bridge::run_unx48813_checks());
    merged = CheckSet::merge(merged, super::unx48816_input_quality::run_unx48816_checks());
    // 合并后必须真的带上 7 段的东西，否则说明合并链路断了。
    if !has_check(&merged, "F48801-J") || !has_check(&merged, "F48816-J1") {
        merged.add(
            "UNX-F48819-J2 七段自检并入验收",
            false,
            "合并后找不到 F48801/F48816 的判据行",
        );
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_unique_and_owned() {
        assert_eq!(keyword_registry_check(), Ok(KEYWORD_COUNT as u32));
        assert_eq!(KEYWORDS.len(), 4);
        let mut i = 0usize;
        while i < KEYWORDS.len() {
            let a = KEYWORDS.get(i).expect("关键词行");
            assert!(is_word(a.word));
            assert!(!a.note.is_empty());
            assert_eq!(module_of_card(a.owner_card), a.owner_stem);
            let mut j = i + 1;
            while j < KEYWORDS.len() {
                let b = KEYWORDS.get(j).expect("关键词行");
                assert_ne!(a.word, b.word);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(KEYWORDS.first().expect("首词").word, "input_event");
        assert_eq!(KEYWORDS.last().expect("末词").word, "syn");
    }

    #[test]
    fn keyword_dup_detected() {
        let mut rows = keyword_rows_vec();
        let first = *rows.first().expect("首行");
        let second = rows.get_mut(1).expect("二行");
        *second = KeywordDef {
            word: first.word,
            owner_stem: second.owner_stem,
            owner_card: second.owner_card,
            note: second.note,
        };
        assert_eq!(keyword_check_rows(&rows), Err(SealCode::KeywordDup.val()));
        // 属主错配也要红。
        let mut rows2 = keyword_rows_vec();
        if let Some(r) = rows2.get_mut(0) {
            r.owner_card = 48805;
        }
        assert_eq!(keyword_check_rows(&rows2), Err(SealCode::CardMismatch.val()));
        // 非小写词要红。
        let mut rows3 = keyword_rows_vec();
        if let Some(r) = rows3.get_mut(0) {
            r.word = "Input_Event";
        }
        assert_eq!(keyword_check_rows(&rows3), Err(SealCode::KeywordUnowned.val()));
    }

    #[test]
    fn windows_disjoint_and_b02_boundary() {
        assert_eq!(window_check(), Ok(MODULE_COUNT as u32));
        let mut i = 0usize;
        while i < WINDOWS.len() {
            let w = WINDOWS.get(i).expect("窗口");
            assert!(w.span >= 0x20);
            if i > 0 {
                let p = WINDOWS.get(i - 1).expect("前窗");
                assert!(p.seg + p.span <= w.seg, "窗口不得重叠");
                assert_eq!(w.seg % 0x20, 0);
            }
            i += 1;
        }
        let last = WINDOWS.last().expect("末窗");
        assert_eq!(last.seg, SEG_BASE);
        assert_eq!(last.seg + last.span, NEXT_BATCH_SEG);
        assert_eq!(segment_guard(), true);
    }

    #[test]
    fn window_overlap_detected() {
        let mut wd = window_rows_vec();
        let first = *wd.first().expect("首窗");
        let second = wd.get_mut(1).expect("二窗");
        *second = WindowDef {
            stem: second.stem,
            seg: first.seg + 0x20,
            span: second.span,
        };
        assert_eq!(window_check_rows(&wd), Err(SealCode::CodeCollision.val()));
        // 跨度不足 0x20 → WindowGap。
        let mut wd2 = window_rows_vec();
        let second2 = wd2.get_mut(1).expect("二窗");
        *second2 = WindowDef {
            stem: second2.stem,
            seg: second2.seg,
            span: 0x10,
        };
        assert_eq!(window_check_rows(&wd2), Err(SealCode::WindowGap.val()));
    }

    #[test]
    fn ledger_sums_six_thousand() {
        assert_eq!(ledger_check(), Ok(6000));
        assert_eq!(floor_sum(), FLOOR_TARGET_SUM);
        assert!(floor_sum() >= LEDGER_FROZEN_SUM);
        assert!(LEDGER_FROZEN_SUM <= BATCH_ROW_BUDGET);
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < CARDS.len() {
            sum += CARDS.get(i).expect("卡行").target;
            i += 1;
        }
        assert_eq!(sum, 6000);
        assert_eq!(module_of_card(48820), "unx48819_input_seal");
        assert_eq!(module_of_card(48800), "");
        assert_eq!(module_of_card(48821), "");
    }

    #[test]
    fn ledger_hole_detected() {
        let mut rows = card_rows_vec();
        rows.pop();
        assert_eq!(ledger_check_rows(&rows), Err(SealCode::LedgerHole.val()));
        let mut rows2 = card_rows_vec();
        if let Some(r) = rows2.get_mut(0) {
            r.target = 100;
        }
        assert_eq!(ledger_check_rows(&rows2), Err(SealCode::LedgerShort.val()));
        let mut rows3 = card_rows_vec();
        if let Some(r) = rows3.get_mut(3) {
            r.stem = "unx48801_input_core";
        }
        assert_eq!(ledger_check_rows(&rows3), Err(SealCode::CardMismatch.val()));
    }

    #[test]
    fn stamp_zero_rewrite_ok() {
        assert!(stamp_zero_rewrite().is_ok());
        assert_eq!(stamp_hash(STAMP_LINE_A), STAMP_HASH_FROZEN);
        assert_eq!(STAMP_LINE_A.as_bytes(), STAMP_LINE_B.as_bytes());
    }

    #[test]
    fn stamp_drift_detected() {
        assert_eq!(
            stamp_check_text(STAMP_LINE_A, "B01 seal: 20 cards frozen"),
            Err(SealCode::StampDrift.val())
        );
        // 双副本同步改写也会被冻结指纹抓住。
        let tweak = "B01 seal: 20 cards frozen, 6000 rows declared; owners input_event/evbit/frame/syn ";
        assert_eq!(
            stamp_check_text(tweak, tweak),
            Err(SealCode::StampDrift.val())
        );
    }

    #[test]
    fn inject_forty_all_caught() {
        assert!(inject_ok(), "注入未全中: {:?}", inject_rounds());
        let v = inject_rounds();
        assert_eq!(v.len(), 40);
        let mut slots = [0u32; 4];
        for h in v.iter() {
            if let Some(c) = slots.get_mut(h.slot as usize) {
                *c += 1;
            }
            assert!(h.caught);
        }
        assert_eq!(slots, [10, 10, 10, 10]);
    }

    #[test]
    fn merged_checks_cover_seven_modules() {
        let cs = run_unx48819_checks();
        let (_, count) = cs.red_items();
        assert!(cs.all_passed(), "收官印必须全绿: {}", cs.red_items_text());
        assert!(count > 70 && count <= 112, "合并后检查项数 {}", count);
        assert!(has_check(&cs, "UNX-F48819-J1"));
        assert!(has_check(&cs, "UNX-F48820-J1"));
        assert!(has_check(&cs, "F48801-J"));
        assert!(has_check(&cs, "F48803-J"));
        assert!(has_check(&cs, "F48805-J"));
        assert!(has_check(&cs, "F48808-J"));
        assert!(has_check(&cs, "F48811-J"));
        assert!(has_check(&cs, "F48813-J"));
        assert!(has_check(&cs, "F48816-J1"));
    }

    #[test]
    fn segment_and_probe_repro() {
        assert!(segment_guard());
        assert_eq!(code_family_check(), Ok(CODE_FAMILY_LEN as u32));
        assert_eq!(SealCode::KeywordDup.val(), SEG_BASE + 1);
        assert_eq!(SealCode::SealBroken.val(), SEG_BASE + CODE_FAMILY_LEN as u32);
        assert_eq!(detail(SealCode::StampDrift.val()), "收官印被改写");
        assert_eq!(detail(0xBEEF), "");
        assert!(repro_check().is_ok());
        assert_eq!(probe_seq_a(), probe_seq_b());
        assert_ne!(probe_seq_a(), probe_seq_c());
    }

    #[test]
    fn golden_seal_report() {
        let g = golden_run();
        assert!(g.ok);
        assert_eq!(g.keywords, 4);
        assert_eq!(g.windows, MODULE_COUNT as u32);
        assert_eq!(g.cards, CARD_COUNT as u32);
        assert_eq!(g.rows, 6000);
        assert_eq!(g.floors, FLOOR_TARGET_SUM);
        assert_eq!(g.digest, golden_run().digest);
    }

    #[test]
    fn floor_table_covers_all_modules() {
        assert_eq!(MODULE_FLOORS.len(), MODULE_COUNT);
        let mut i = 0usize;
        while i < MODULE_FLOORS.len() {
            let (stem, floor) = MODULE_FLOORS.get(i).expect("底线行");
            assert!(*floor >= 350);
            assert!(WINDOWS.iter().any(|w| w.stem == *stem));
            i += 1;
        }
        assert_eq!(WINDOWS.len(), MODULE_FLOORS.len());
    }
}
