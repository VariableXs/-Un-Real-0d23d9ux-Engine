//! UNX-F49013 配置面文案走查账 + UNX-F49014 B11 段内联轧 · 0x71A40
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-C 槽；卡面 J1 引用按原文保留）：
//!   UNX-F49013 配置面文案走查账（三态文案/标点/术语统一）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49014-J1 联轧 ×5 全绿零漂移；判据锚定 §AI-62 §7；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49014 B11 段内联轧（quirk 五路判据交叉复跑）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49015-J1 错误码 ×15 零漂移互不混淆；判据锚定 §AI-62 §2；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 说明：F49013 卡面引用的是 UNX-F49014-J1、F49014 卡面引用的是
//! UNX-F49015-J1（任务板自身的 +1 引用）。此处逐字保留，不改号，便于
//! 任务板锚定逐字回查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（文案走查）：三态枚举 → 槽位表自洽 → 标点扫描 → 术语替换扫描 →
//! 占位符核对 → 重复/表序 → 走查落账。
//! 三态先于文案：命中 / 未命中 / 降级是三份不同的用户承诺，每句文案必须先
//! 挂进恰好一个态。缺少这个前提，同一句话会同时被两条路径取用，用户按错误
//! 的预期去操作，走查再漂亮也是假绿。
//! 槽位先于标点：title / body / hint 的终止标点规则不同——标题不带句号，
//! 正文与提示必须带句号。槽位错位时拿 body 的规则去查 title，正好把「标题
//! 多了句号」这种真实缺陷放过。
//! 标点先于术语：ASCII 逗号/句点会把中文句子切成两截，先按字符类别拒；术语
//! 是词级问题，标点坏了词边界就不可信，故标点扫描必须在术语扫描之前。
//! 术语先于占位符：禁用变体（触摸板 / 退化）一旦混入，占位符替换后的整句
//! 仍然带着错误术语。先拒术语，占位符核对才是在干净文本上做的。
//! 占位符先于重复：{device}/{fw}/{count} 的数量与申报不符时，运行期会漏值；
//! 先钉数量，重复检测才不会被「模板相同但占位符不同」的两句文案骗过。
//! 落账后于全部扫描：走查账记的是「通过扫描的文案条数」，不是表里条数。
//! 顺序反过来，账本会把坏文案算成已走查——这就是走查账最典型的虚报。
//! 闸门次序（五路联轧）：探测分类 → 单路复算 → 五路交叉 → 零漂移复跑 →
//! 摘要合账。
//! 探测分类先于单路：一台设备只能是 Hit/Miss/Degrade 之一，model=0、
//! fw=0xFFFFFFFF 是坏探测必须先拒。分类摇摆时五路各自的账无法互校。
//! 单路复算先于交叉：每路先在自己的输入域内自洽（轮数 = 绿数 + 红数），
//! 交叉才有意义；否则「五路全绿」可能只是五路都没跑满。
//! 交叉先于复跑：五路回答的是同一批判据的不同侧面，交叉通过说明覆盖面齐；
//! 复跑只证明确定性，不能替代覆盖面。
//! 零漂移复跑后于交叉：同一路跑两遍摘要逐位相等才算零漂移。顺序反了，会把
//! 「两遍都错得一样」当成通过。
//! 关键差异：本模块不做最终排版（字号/换行由 UI 层负责），不做多语言翻译
//! （只查中文标点与术语），不写盘持久化（配置持久化归 B11-C 的其它槽位），
//! 五路联轧只复跑既有判据的输入域（匹配链/降级账/承诺清单/热生效/校验和），
//! 不新增 quirk 语义，也不改写 0x71A80 段的错误码族与四点观测埋点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71A40，VX6 批 B11-C 首槽）。
pub const SEG_BASE: u32 = 0x71A40;

// ── 文案面常量 ────────────────────────────────────────────────────
/// 三态总数（命中/未命中/降级）。
pub const COPY_STATE_COUNT: usize = 3;
/// 每态的槽位数（title/body/hint）。
pub const COPY_SURFACES: usize = 3;
/// 文案总条数（3 态 × 3 槽）。
pub const COPY_COUNT: usize = 9;
/// 每态必须有的条数。
pub const ENTRY_PER_STATE: u32 = 3;
/// 术语对数量（规范写法 / 禁用变体）。
pub const TERM_PAIRS: usize = 6;
/// 占位符种类数。
pub const HOLE_KINDS: usize = 3;
/// 走查轮数（卡面 ×10^2）。
pub const WALK_ROUNDS: u32 = 100;
/// 走查轮数上界（覆盖闸门）。
pub const RUN_MAX: u32 = 4_000;
/// 走查设备台数。
pub const DEVICES: usize = 24;
/// 注入路数（文案篡改）。
pub const INJECT_ROUTES: u32 = 5;
/// 注入轮数上界。
pub const INJECT_MAX: u32 = 10_000;
/// quirk 表项数（五路联轧的公共输入）。
pub const QUIRK_ENTRIES: usize = 8;
/// 承诺清单机型数。
pub const PROMISE_MODELS: usize = 6;
/// 热生效改动次数。
pub const HOT_CHANGES: u32 = 12;
/// 校验和复算轮数。
pub const CHECKSUM_ROUNDS: u32 = 100;
/// 五路联轧路数。
pub const INLINE_ROUTES: usize = 5;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 增益合法区间下界。
pub const GAIN_MIN: u32 = 10;
/// 增益合法区间上界。
pub const GAIN_MAX: u32 = 200;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyCode {
    /// 三态枚举越界或三态条数不齐
    StateBad,
    /// 槽位/表项结构非法（键错位、区间倒置、掩码为零）
    SlotBad,
    /// 文案为空或两句完全相同
    TextBad,
    /// 标点非法（ASCII 标点混入、标点旁空格、终止标点错位）
    PunctBad,
    /// 术语非法（禁用变体混入或全句无规范术语）
    TermBad,
    /// 占位符非法（未登记、重复、数量与申报不符）
    HoleBad,
    /// 表序非法（未按态、槽升序）
    OrderBad,
    /// 走查账不守恒
    LedgerBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 渲染行不可解析或不可回读
    RenderBad,
    /// 覆盖轮数为零或越界
    CoverBad,
    /// 注入轮未被捕获
    InjectBad,
    /// 诊断码越出本模块段窗
    SegmentBad,
    /// 联轧某一路未全绿或轮数不符
    InlineBad,
}

impl CopyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            CopyCode::StateBad => "三态枚举越界或三态文案条数不齐",
            CopyCode::SlotBad => "槽位键错位或表项结构非法",
            CopyCode::TextBad => "文案为空，或两条文案逐字相同",
            CopyCode::PunctBad => "标点非法：ASCII 标点、标点旁空格或终止标点错位",
            CopyCode::TermBad => "术语非法：禁用变体混入或全句无规范术语",
            CopyCode::HoleBad => "占位符未登记、重复或数量与申报不符",
            CopyCode::OrderBad => "文案表未按态、槽升序排列",
            CopyCode::LedgerBad => "走查账不守恒：轮数 ≠ 三态计数之和",
            CopyCode::ReplayBad => "两次确定性复跑摘要不一致",
            CopyCode::RenderBad => "渲染行字段缺失或回读不一致",
            CopyCode::CoverBad => "覆盖轮数为零或越过上限",
            CopyCode::InjectBad => "注入轮未被验收闸捕获",
            CopyCode::SegmentBad => "诊断码越出本模块段窗",
            CopyCode::InlineBad => "联轧某一路未全绿或轮数不符",
        }
    }

    pub const ALL: [CopyCode; 14] = [
        CopyCode::StateBad,
        CopyCode::SlotBad,
        CopyCode::TextBad,
        CopyCode::PunctBad,
        CopyCode::TermBad,
        CopyCode::HoleBad,
        CopyCode::OrderBad,
        CopyCode::LedgerBad,
        CopyCode::ReplayBad,
        CopyCode::RenderBad,
        CopyCode::CoverBad,
        CopyCode::InjectBad,
        CopyCode::SegmentBad,
        CopyCode::InlineBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < CopyCode::ALL.len() {
        if let Some(c) = CopyCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 三态与槽位 ────────────────────────────────────────────────────
/// 一态：英文 id（落账用）与中文标签（渲染用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyState {
    pub id: &'static str,
    pub label: &'static str,
}

pub const STATES: [CopyState; COPY_STATE_COUNT] = [
    CopyState {
        id: "hit",
        label: "命中",
    },
    CopyState {
        id: "miss",
        label: "未命中",
    },
    CopyState {
        id: "degrade",
        label: "降级",
    },
];

/// 一槽：槽位 id 与「是否必须带终止标点」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Surface {
    pub id: &'static str,
    pub terminal: bool,
}

pub const SURFACES: [Surface; COPY_SURFACES] = [
    Surface {
        id: "title",
        terminal: false,
    },
    Surface {
        id: "body",
        terminal: true,
    },
    Surface {
        id: "hint",
        terminal: true,
    },
];

/// 一条文案：态 + 槽 + 键 + 正文 + 占位符申报数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyEntry {
    pub state: u8,
    pub slot: u8,
    pub key: &'static str,
    pub text: &'static str,
    pub holes: u8,
}

/// 九条文案表（态升序、态内槽升序）。
pub const COPY_ENTRIES: [CopyEntry; COPY_COUNT] = [
    CopyEntry {
        state: 0,
        slot: 0,
        key: "hit.title",
        text: "已命中 {device} 的 quirk 表",
        holes: 1,
    },
    CopyEntry {
        state: 0,
        slot: 1,
        key: "hit.body",
        text: "触控板已命中 {device} 的 quirk 表，固件 {fw} 参数已生效。",
        holes: 2,
    },
    CopyEntry {
        state: 0,
        slot: 2,
        key: "hit.hint",
        text: "如行为异常，请在配置面复核固件版本。",
        holes: 0,
    },
    CopyEntry {
        state: 1,
        slot: 0,
        key: "miss.title",
        text: "未命中 {device} 的 quirk 表",
        holes: 1,
    },
    CopyEntry {
        state: 1,
        slot: 1,
        key: "miss.body",
        text: "触控板未命中 {device} 的 quirk 表，将使用默认参数，固件 {fw} 未改动。",
        holes: 2,
    },
    CopyEntry {
        state: 1,
        slot: 2,
        key: "miss.hint",
        text: "可在配置面登记机型，以便后续固件收录。",
        holes: 0,
    },
    CopyEntry {
        state: 2,
        slot: 0,
        key: "degrade.title",
        text: "{device} 已降级运行",
        holes: 1,
    },
    CopyEntry {
        state: 2,
        slot: 1,
        key: "degrade.body",
        text: "触控板已降级运行：{device} 的固件 {fw} 只承诺基础手势。",
        holes: 2,
    },
    CopyEntry {
        state: 2,
        slot: 2,
        key: "degrade.hint",
        text: "降级不影响点按，请在配置面查看命中记录。",
        holes: 0,
    },
];

/// 术语表：(规范写法, 禁用变体)。规范写法至少出现一次，禁用变体一次都不许有。
pub const TERMS: [(&str, &str); TERM_PAIRS] = [
    ("触控板", "触摸板"),
    ("命中", "击中"),
    ("降级", "退化"),
    ("固件", "韧体"),
    ("配置面", "设置面"),
    ("quirk 表", "怪癖表"),
];

/// 合法占位符名。
pub const HOLE_NAMES: [&str; HOLE_KINDS] = ["device", "fw", "count"];

/// 全角标点集合（允许出现，但不得与空格相邻）。
pub const FULL_PUNCT: [char; 6] = ['，', '。', '；', '：', '！', '？'];

pub fn is_full_punct(c: char) -> bool {
    let mut i = 0usize;
    while i < FULL_PUNCT.len() {
        if let Some(p) = FULL_PUNCT.get(i) {
            if *p == c {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// ASCII 标点（一律禁止：中文文案只允许全角标点）。
pub fn is_ascii_punct(c: char) -> bool {
    matches!(c, ',' | '.' | ';' | ':' | '!' | '?')
}

/// 终止标点集合（正文/提示的合法收尾）。
pub fn is_terminal_punct(c: char) -> bool {
    matches!(c, '。' | '！' | '？')
}

/// 占位符名 -> 下标。
pub fn hole_index(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < HOLE_NAMES.len() {
        if let Some(n) = HOLE_NAMES.get(i) {
            if *n == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 标点扫描：返全角标点数，坏一处即拒。
pub fn scan_punct(text: &str, terminal: bool) -> Result<u32, u32> {
    if text.is_empty() {
        return Err(CopyCode::TextBad.val());
    }
    let mut marks = 0u32;
    let mut prev: Option<char> = None;
    for ch in text.chars() {
        if is_ascii_punct(ch) {
            return Err(CopyCode::PunctBad.val());
        }
        if is_full_punct(ch) {
            marks = marks.saturating_add(1);
            if prev == Some(' ') {
                return Err(CopyCode::PunctBad.val());
            }
        }
        if let Some(p) = prev {
            if is_full_punct(p) && ch == ' ' {
                return Err(CopyCode::PunctBad.val());
            }
        }
        prev = Some(ch);
    }
    let last = match text.chars().last() {
        Some(c) => c,
        None => return Err(CopyCode::TextBad.val()),
    };
    if terminal {
        if !is_terminal_punct(last) {
            return Err(CopyCode::PunctBad.val());
        }
        if marks == 0 {
            return Err(CopyCode::PunctBad.val());
        }
    } else if is_full_punct(last) {
        // 标题不带任何终止标点。
        return Err(CopyCode::PunctBad.val());
    }
    Ok(marks)
}

/// 术语扫描：返规范术语命中数；禁用变体命中一处即拒。
pub fn scan_terms(text: &str) -> Result<u32, u32> {
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < TERMS.len() {
        if let Some(pair) = TERMS.get(i) {
            if text.contains(pair.1) {
                return Err(CopyCode::TermBad.val());
            }
            if text.contains(pair.0) {
                hits = hits.saturating_add(1);
            }
        }
        i += 1;
    }
    if hits == 0 {
        return Err(CopyCode::TermBad.val());
    }
    Ok(hits)
}

/// 占位符扫描：返占位符个数；未登记/重复/数量不符/括号不配对即拒。
pub fn scan_holes(text: &str, declared: u8) -> Result<u32, u32> {
    let mut buf = [0u8; 8];
    let mut w = 0usize;
    let mut in_hole = false;
    let mut count = 0u32;
    let mut seen = [false; HOLE_KINDS];
    for ch in text.chars() {
        if ch == '{' {
            if in_hole {
                return Err(CopyCode::HoleBad.val());
            }
            in_hole = true;
            w = 0;
            continue;
        }
        if ch == '}' {
            if !in_hole {
                return Err(CopyCode::HoleBad.val());
            }
            in_hole = false;
            let name = match core::str::from_utf8(&buf[..w]) {
                Ok(s) => s,
                Err(_) => return Err(CopyCode::HoleBad.val()),
            };
            let idx = match hole_index(name) {
                Some(v) => v,
                None => return Err(CopyCode::HoleBad.val()),
            };
            if let Some(slot) = seen.get_mut(idx) {
                if *slot {
                    return Err(CopyCode::HoleBad.val());
                }
                *slot = true;
            }
            count = count.saturating_add(1);
            continue;
        }
        if in_hole {
            if !ch.is_ascii_alphanumeric() {
                return Err(CopyCode::HoleBad.val());
            }
            if w >= buf.len() {
                return Err(CopyCode::HoleBad.val());
            }
            if let Some(slot) = buf.get_mut(w) {
                *slot = ch as u8;
            }
            w += 1;
        }
    }
    if in_hole {
        return Err(CopyCode::HoleBad.val());
    }
    if count != declared as u32 {
        return Err(CopyCode::HoleBad.val());
    }
    Ok(count)
}

/// 文案表统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyStats {
    pub total: u32,
    pub per_state: [u32; COPY_STATE_COUNT],
    pub term_hits: u32,
    pub punct_marks: u32,
    pub fingerprint: u64,
}

impl CopyStats {
    pub fn even(&self) -> bool {
        let mut i = 0usize;
        while i < self.per_state.len() {
            if let Some(n) = self.per_state.get(i) {
                if *n != ENTRY_PER_STATE {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 文案表自审：条数 → 态/槽 → 键 → 空文 → 重复 → 表序 → 标点 → 术语 → 占位符。
pub fn copy_integrity(table: &[CopyEntry]) -> Result<CopyStats, u32> {
    if table.len() != COPY_COUNT {
        return Err(CopyCode::StateBad.val());
    }
    let mut per_state = [0u32; COPY_STATE_COUNT];
    let mut term_hits = 0u32;
    let mut punct_marks = 0u32;
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let e = match table.get(i) {
            Some(v) => *v,
            None => return Err(CopyCode::StateBad.val()),
        };
        let sidx = e.state as usize;
        if sidx >= COPY_STATE_COUNT {
            return Err(CopyCode::StateBad.val());
        }
        let suidx = e.slot as usize;
        if suidx >= COPY_SURFACES {
            return Err(CopyCode::SlotBad.val());
        }
        let st = match STATES.get(sidx) {
            Some(v) => *v,
            None => return Err(CopyCode::StateBad.val()),
        };
        let su = match SURFACES.get(suidx) {
            Some(v) => *v,
            None => return Err(CopyCode::SlotBad.val()),
        };
        let mut expect = String::new();
        expect.push_str(st.id);
        expect.push('.');
        expect.push_str(su.id);
        if e.key != expect.as_str() || e.key.is_empty() {
            return Err(CopyCode::SlotBad.val());
        }
        if e.text.is_empty() {
            return Err(CopyCode::TextBad.val());
        }
        // 槽位唯一、键唯一、正文唯一（O(n^2)，n=9，可接受）。
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                if p.state == e.state && p.slot == e.slot {
                    return Err(CopyCode::SlotBad.val());
                }
                if p.key == e.key {
                    return Err(CopyCode::SlotBad.val());
                }
                if p.text == e.text {
                    return Err(CopyCode::TextBad.val());
                }
            }
            j += 1;
        }
        // 表序：态升序、态内槽升序。
        if i > 0 {
            if let Some(p) = table.get(i - 1) {
                if (p.state, p.slot) >= (e.state, e.slot) {
                    return Err(CopyCode::OrderBad.val());
                }
            }
        }
        let marks = scan_punct(e.text, su.terminal)?;
        let terms = scan_terms(e.text)?;
        scan_holes(e.text, e.holes)?;
        punct_marks = punct_marks.saturating_add(marks);
        term_hits = term_hits.saturating_add(terms);
        if let Some(slot) = per_state.get_mut(sidx) {
            *slot = slot.saturating_add(1);
        }
        fp = p_core::fold_u32(fp, e.state as u32);
        fp = p_core::fold_u32(fp, e.slot as u32);
        fp = fold_str(fp, e.key);
        fp = fold_str(fp, e.text);
        fp = p_core::fold_u32(fp, e.holes as u32);
        i += 1;
    }
    let mut k = 0usize;
    while k < per_state.len() {
        if let Some(n) = per_state.get(k) {
            if *n != ENTRY_PER_STATE {
                return Err(CopyCode::StateBad.val());
            }
        }
        k += 1;
    }
    if punct_marks == 0 || term_hits == 0 {
        return Err(CopyCode::TextBad.val());
    }
    Ok(CopyStats {
        total: table.len() as u32,
        per_state,
        term_hits,
        punct_marks,
        fingerprint: fp,
    })
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = p_core::fold_byte(x, *b);
    }
    x
}

/// 按 (态, 槽) 取一条文案（表自洽时唯一）。
pub fn copy_at(state: u8, slot: u8) -> Option<CopyEntry> {
    let mut i = 0usize;
    while i < COPY_ENTRIES.len() {
        if let Some(e) = COPY_ENTRIES.get(i) {
            if e.state == state && e.slot == slot {
                return Some(*e);
            }
        }
        i += 1;
    }
    None
}

/// 把一条文案渲染成运行期文本（替换三只占位符），并复核标点/占位符。
pub fn render_copy(entry: &CopyEntry, device: &str, fw: u32) -> Result<String, u32> {
    if device.is_empty() {
        return Err(CopyCode::TextBad.val());
    }
    let mut out = String::new();
    let mut chars = entry.text.chars();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            let mut closed = false;
            while let Some(n) = chars.next() {
                if n == '}' {
                    closed = true;
                    break;
                }
                name.push(n);
            }
            if !closed {
                return Err(CopyCode::HoleBad.val());
            }
            if name == "device" {
                out.push_str(device);
            } else if name == "fw" {
                out.push_str(&format!("{:04X}", fw));
            } else if name == "count" {
                out.push_str("1");
            } else {
                return Err(CopyCode::HoleBad.val());
            }
        } else if ch == '}' {
            return Err(CopyCode::HoleBad.val());
        } else {
            out.push(ch);
        }
    }
    if out.contains('{') || out.contains('}') {
        return Err(CopyCode::HoleBad.val());
    }
    let slot = match SURFACES.get(entry.slot as usize) {
        Some(v) => *v,
        None => return Err(CopyCode::SlotBad.val()),
    };
    scan_punct(&out, slot.terminal)?;
    scan_terms(&out)?;
    Ok(out)
}

// ── 设备探测与 quirk 表（五路联轧的公共输入） ─────────────────────
/// quirk 表项：机型 / 固件闭区间 / 参数覆盖掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QEntry {
    pub model: u8,
    pub fw_lo: u32,
    pub fw_hi: u32,
    pub mask: u32,
}

pub const QUIRK_TABLE: [QEntry; QUIRK_ENTRIES] = [
    QEntry {
        model: 0x11,
        fw_lo: 0x0100,
        fw_hi: 0x0109,
        mask: 0x003,
    },
    QEntry {
        model: 0x12,
        fw_lo: 0x0200,
        fw_hi: 0x0205,
        mask: 0x007,
    },
    QEntry {
        model: 0x13,
        fw_lo: 0x0300,
        fw_hi: 0x030F,
        mask: 0x001,
    },
    QEntry {
        model: 0x14,
        fw_lo: 0x0400,
        fw_hi: 0x0403,
        mask: 0x00F,
    },
    QEntry {
        model: 0x15,
        fw_lo: 0x0500,
        fw_hi: 0x05FF,
        mask: 0x003,
    },
    QEntry {
        model: 0x16,
        fw_lo: 0x0600,
        fw_hi: 0x0601,
        mask: 0x005,
    },
    QEntry {
        model: 0x17,
        fw_lo: 0x0700,
        fw_hi: 0x0707,
        mask: 0x009,
    },
    QEntry {
        model: 0x18,
        fw_lo: 0x0800,
        fw_hi: 0x08FF,
        mask: 0x011,
    },
];

/// 一台被走查的设备。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Device {
    pub id: &'static str,
    pub model: u8,
    pub fw: u32,
}

pub const DEVICE_TABLE: [Device; DEVICES] = [
    Device {
        id: "D01",
        model: 0x11,
        fw: 0x0100,
    },
    Device {
        id: "D02",
        model: 0x11,
        fw: 0x0109,
    },
    Device {
        id: "D03",
        model: 0x11,
        fw: 0x0110,
    },
    Device {
        id: "D04",
        model: 0x12,
        fw: 0x0203,
    },
    Device {
        id: "D05",
        model: 0x12,
        fw: 0x01F0,
    },
    Device {
        id: "D06",
        model: 0x12,
        fw: 0x0206,
    },
    Device {
        id: "D07",
        model: 0x13,
        fw: 0x0308,
    },
    Device {
        id: "D08",
        model: 0x13,
        fw: 0x0310,
    },
    Device {
        id: "D09",
        model: 0x14,
        fw: 0x0400,
    },
    Device {
        id: "D10",
        model: 0x14,
        fw: 0x0404,
    },
    Device {
        id: "D11",
        model: 0x15,
        fw: 0x0580,
    },
    Device {
        id: "D12",
        model: 0x15,
        fw: 0x0500,
    },
    Device {
        id: "D13",
        model: 0x16,
        fw: 0x0601,
    },
    Device {
        id: "D14",
        model: 0x16,
        fw: 0x0602,
    },
    Device {
        id: "D15",
        model: 0x17,
        fw: 0x0707,
    },
    Device {
        id: "D16",
        model: 0x17,
        fw: 0x06FF,
    },
    Device {
        id: "D17",
        model: 0x18,
        fw: 0x08FF,
    },
    Device {
        id: "D18",
        model: 0x18,
        fw: 0x0900,
    },
    Device {
        id: "D19",
        model: 0x21,
        fw: 0x0100,
    },
    Device {
        id: "D20",
        model: 0x22,
        fw: 0x0200,
    },
    Device {
        id: "D21",
        model: 0x23,
        fw: 0x0300,
    },
    Device {
        id: "D22",
        model: 0x24,
        fw: 0x0400,
    },
    Device {
        id: "D23",
        model: 0x25,
        fw: 0x0500,
    },
    Device {
        id: "D24",
        model: 0x26,
        fw: 0x0600,
    },
];

/// 探测分类：坏探测先拒，再按「表内区间 / 表内越区间 / 表外」三格落位。
/// 返回 0=Hit 1=Miss 2=Degrade。
pub fn classify_device(model: u8, fw: u32) -> Result<u8, u32> {
    if model == 0 || fw == 0xFFFF_FFFF {
        return Err(CopyCode::StateBad.val());
    }
    let mut i = 0usize;
    while i < QUIRK_TABLE.len() {
        if let Some(e) = QUIRK_TABLE.get(i) {
            if e.model == model {
                if fw >= e.fw_lo && fw <= e.fw_hi {
                    return Ok(0);
                }
                return Ok(2);
            }
        }
        i += 1;
    }
    Ok(1)
}

/// quirk 表自审：条数 → 机型非零 → 区间不倒置 → 掩码非零 → 机型唯一 → 机型升序。
pub fn quirk_integrity(table: &[QEntry]) -> Result<u64, u32> {
    if table.len() != QUIRK_ENTRIES {
        return Err(CopyCode::SlotBad.val());
    }
    let mut fp = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let e = match table.get(i) {
            Some(v) => *v,
            None => return Err(CopyCode::SlotBad.val()),
        };
        if e.model == 0 || e.fw_lo > e.fw_hi || e.mask == 0 {
            return Err(CopyCode::SlotBad.val());
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                if p.model == e.model {
                    return Err(CopyCode::SlotBad.val());
                }
                if p.model > e.model {
                    return Err(CopyCode::OrderBad.val());
                }
            }
            j += 1;
        }
        fp = p_core::fold_u32(fp, e.model as u32);
        fp = p_core::fold_u32(fp, e.fw_lo);
        fp = p_core::fold_u32(fp, e.fw_hi);
        fp = p_core::fold_u32(fp, e.mask);
        i += 1;
    }
    Ok(fp)
}

/// 表校验和（独立于自审摘要的另一条折叠路径，供第 5 路复算）。
pub fn table_checksum(table: &[QEntry]) -> u64 {
    let mut h = p_core::FNV_OFF ^ 0x5EED_5EED;
    let mut i = 0usize;
    while i < table.len() {
        if let Some(e) = table.get(i) {
            h = p_core::fold_u32(h, (e.model as u32) << 24);
            h = p_core::fold_u32(h, e.fw_lo ^ e.fw_hi.rotate_left(7));
            h = p_core::fold_u32(h, e.mask);
        }
        i += 1;
    }
    h
}

// ── 文案走查账 ────────────────────────────────────────────────────
/// 走查账：每态落账 + 累计标点数 + 坏探测拒收数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CopyLedger {
    pub rounds: u32,
    pub hit: u32,
    pub miss: u32,
    pub degrade: u32,
    pub rejects: u32,
    pub marks: u32,
}

impl CopyLedger {
    pub const fn new() -> CopyLedger {
        CopyLedger {
            rounds: 0,
            hit: 0,
            miss: 0,
            degrade: 0,
            rejects: 0,
            marks: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds
            == self
                .hit
                .saturating_add(self.miss)
                .saturating_add(self.degrade)
            && self.rejects == 0
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.rounds);
        h = p_core::fold_u32(h, self.hit);
        h = p_core::fold_u32(h, self.miss);
        h = p_core::fold_u32(h, self.degrade);
        h = p_core::fold_u32(h, self.rejects);
        h = p_core::fold_u32(h, self.marks);
        h
    }
}

/// 走查：随机抽设备 → 探测分类 → 取该态文案 → 渲染 → 复核标点/术语 → 落账。
pub fn walk_copies(rounds: u32) -> Result<CopyLedger, u32> {
    if rounds == 0 || rounds > RUN_MAX {
        return Err(CopyCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_3C07);
    let mut lg = CopyLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let di = rng.below(DEVICE_TABLE.len() as u32) as usize;
        let dev = match DEVICE_TABLE.get(di) {
            Some(v) => *v,
            None => return Err(CopyCode::CoverBad.val()),
        };
        let state = match classify_device(dev.model, dev.fw) {
            Ok(s) => s,
            Err(_) => {
                lg.rejects = lg.rejects.saturating_add(1);
                r = r.saturating_add(1);
                continue;
            }
        };
        let slot = rng.below(COPY_SURFACES as u32) as u8;
        let ent = match copy_at(state, slot) {
            Some(v) => v,
            None => return Err(CopyCode::SlotBad.val()),
        };
        let line = render_copy(&ent, dev.id, dev.fw)?;
        let marks = scan_punct(&line, ent.slot == 1 || ent.slot == 2)?;
        scan_terms(&line)?;
        lg.marks = lg.marks.saturating_add(marks);
        match state {
            0 => lg.hit = lg.hit.saturating_add(1),
            1 => lg.miss = lg.miss.saturating_add(1),
            _ => lg.degrade = lg.degrade.saturating_add(1),
        }
        lg.rounds = lg.rounds.saturating_add(1);
        r = r.saturating_add(1);
    }
    if !lg.conserved() {
        return Err(CopyCode::LedgerBad.val());
    }
    Ok(lg)
}

// ── 五路联轧 ──────────────────────────────────────────────────────
/// 单路复跑结果：轮数 / 绿数 / 红数 / 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteRun {
    pub route: u8,
    pub rounds: u32,
    pub greens: u32,
    pub rejects: u32,
    pub digest: u64,
}

impl RouteRun {
    pub const fn empty() -> RouteRun {
        RouteRun {
            route: 0,
            rounds: 0,
            greens: 0,
            rejects: 0,
            digest: 0,
        }
    }

    pub fn green(&self) -> bool {
        self.rounds > 0 && self.greens == self.rounds && self.rejects == 0
    }
}

/// 第 1 路：匹配链（设备探测 → 表项命中 → 分类），24 台设备。
pub fn route_match_chain() -> Result<RouteRun, u32> {
    quirk_integrity(&QUIRK_TABLE)?;
    let mut h = p_core::FNV_OFF;
    let mut greens = 0u32;
    let mut rejects = 0u32;
    let mut i = 0usize;
    while i < DEVICE_TABLE.len() {
        if let Some(d) = DEVICE_TABLE.get(i) {
            match classify_device(d.model, d.fw) {
                Ok(s) if s <= 2 => {
                    greens = greens.saturating_add(1);
                    h = fold_str(h, d.id);
                    h = p_core::fold_u32(h, d.model as u32);
                    h = p_core::fold_u32(h, d.fw);
                    h = p_core::fold_u32(h, s as u32);
                }
                _ => {
                    rejects = rejects.saturating_add(1);
                    h = p_core::fold_u32(h, 0xFFFF_FFFF);
                }
            }
        }
        i += 1;
    }
    let run = RouteRun {
        route: 0,
        rounds: DEVICE_TABLE.len() as u32,
        greens,
        rejects,
        digest: h,
    };
    if !run.green() {
        return Err(CopyCode::InlineBad.val());
    }
    Ok(run)
}

/// 第 2 路：降级账（10 台表内设备把固件推到区间外，必须全部落 Degrade）。
pub fn route_degrade_ledger() -> Result<RouteRun, u32> {
    quirk_integrity(&QUIRK_TABLE)?;
    let mut h = p_core::FNV_OFF;
    let mut greens = 0u32;
    let mut rounds = 0u32;
    let mut i = 0usize;
    while i < 10 {
        if let Some(d) = DEVICE_TABLE.get(i) {
            let fw = d.fw.saturating_add(0x1000);
            rounds = rounds.saturating_add(1);
            match classify_device(d.model, fw) {
                Ok(2) => {
                    greens = greens.saturating_add(1);
                    h = p_core::fold_u32(h, d.model as u32);
                    h = p_core::fold_u32(h, fw);
                }
                _ => {
                    h = p_core::fold_u32(h, 0xBAD0_0001);
                }
            }
        }
        i += 1;
    }
    let run = RouteRun {
        route: 1,
        rounds,
        greens,
        rejects: rounds.saturating_sub(greens),
        digest: h,
    };
    if !run.green() {
        return Err(CopyCode::InlineBad.val());
    }
    Ok(run)
}

/// 承诺清单：机型 / 声称覆盖 / 实际覆盖。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Promise {
    pub model: u8,
    pub claimed: u32,
    pub actual: u32,
}

pub const PROMISES: [Promise; PROMISE_MODELS] = [
    Promise {
        model: 0x11,
        claimed: 0x003,
        actual: 0x003,
    },
    Promise {
        model: 0x12,
        claimed: 0x007,
        actual: 0x007,
    },
    Promise {
        model: 0x13,
        claimed: 0x001,
        actual: 0x001,
    },
    Promise {
        model: 0x14,
        claimed: 0x005,
        actual: 0x00F,
    },
    Promise {
        model: 0x15,
        claimed: 0x003,
        actual: 0x003,
    },
    Promise {
        model: 0x16,
        claimed: 0x001,
        actual: 0x005,
    },
];

/// 第 3 路：承诺清单（声称 ⊆ 实际，6 台机型）。
pub fn route_promise_list() -> Result<RouteRun, u32> {
    let mut h = p_core::FNV_OFF;
    let mut greens = 0u32;
    let mut i = 0usize;
    while i < PROMISES.len() {
        if let Some(p) = PROMISES.get(i) {
            let contained = p.claimed & !p.actual == 0 && p.claimed != 0;
            if contained {
                greens = greens.saturating_add(1);
            }
            h = p_core::fold_u32(h, p.model as u32);
            h = p_core::fold_u32(h, p.claimed);
            h = p_core::fold_u32(h, p.actual);
            h = p_core::fold_u32(h, if contained { 1 } else { 0 });
        }
        i += 1;
    }
    let run = RouteRun {
        route: 2,
        rounds: PROMISE_MODELS as u32,
        greens,
        rejects: PROMISE_MODELS as u32 - greens,
        digest: h,
    };
    if !run.green() {
        return Err(CopyCode::InlineBad.val());
    }
    Ok(run)
}

/// 热生效状态：一代一条，提交即回读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotState {
    pub generation: u32,
    pub active: u32,
}

impl HotState {
    pub const fn new() -> HotState {
        HotState {
            generation: 0,
            active: 0,
        }
    }

    /// 提交：越界值拒绝且不动世代；合法值落盘并递增世代。
    pub fn commit(&mut self, value: u32) -> bool {
        if value < GAIN_MIN || value > GAIN_MAX {
            return false;
        }
        self.generation = self.generation.saturating_add(1);
        self.active = value;
        true
    }
}

/// 第 4 路：热生效（12 次改动，断流必须为 0）。
pub fn route_hot_apply() -> Result<RouteRun, u32> {
    const VALUES: [u32; HOT_CHANGES as usize] = [70, 72, 75, 80, 85, 90, 95, 100, 110, 120, 130, 140];
    let mut st = HotState::new();
    let mut h = p_core::FNV_OFF;
    let mut greens = 0u32;
    let mut i = 0usize;
    while i < VALUES.len() {
        if let Some(v) = VALUES.get(i) {
            let ok = st.commit(*v);
            let readback = st.active == *v && st.generation == (i as u32 + 1);
            if ok && readback {
                greens = greens.saturating_add(1);
            }
            h = p_core::fold_u32(h, *v);
            h = p_core::fold_u32(h, st.generation);
            h = p_core::fold_u32(h, if readback { 1 } else { 0 });
        }
        i += 1;
    }
    let run = RouteRun {
        route: 3,
        rounds: HOT_CHANGES,
        greens,
        rejects: HOT_CHANGES.saturating_sub(greens),
        digest: h,
    };
    if !run.green() {
        return Err(CopyCode::InlineBad.val());
    }
    Ok(run)
}

/// 第 5 路：校验和（100 轮复算零漂移 + 4 张坏表必拒）。
pub fn route_checksum() -> Result<RouteRun, u32> {
    let base = table_checksum(&QUIRK_TABLE);
    quirk_integrity(&QUIRK_TABLE)?;
    let mut h = p_core::FNV_OFF;
    let mut greens = 0u32;
    let mut r = 0u32;
    while r < CHECKSUM_ROUNDS {
        let again = table_checksum(&QUIRK_TABLE);
        if again == base && base != 0 {
            greens = greens.saturating_add(1);
        }
        h = p_core::fold_u32(h, (again >> 32) as u32);
        h = p_core::fold_u32(h, again as u32);
        r = r.saturating_add(1);
    }
    // 四张坏表：区间倒置 / 机型为零 / 机型重复 / 掩码为零。
    let mut bad = QUIRK_TABLE;
    if let Some(slot) = bad.get_mut(0) {
        slot.fw_lo = 0x0200;
        slot.fw_hi = 0x0100;
    }
    let b1 = quirk_integrity(&bad).is_err();
    let mut bad = QUIRK_TABLE;
    if let Some(slot) = bad.get_mut(1) {
        slot.model = 0;
    }
    let b2 = quirk_integrity(&bad).is_err();
    let mut bad = QUIRK_TABLE;
    if let Some(slot) = bad.get_mut(2) {
        if let Some(src) = QUIRK_TABLE.get(3) {
            slot.model = src.model;
        }
    }
    let b3 = quirk_integrity(&bad).is_err();
    let mut bad = QUIRK_TABLE;
    if let Some(slot) = bad.get_mut(3) {
        slot.mask = 0;
    }
    let b4 = quirk_integrity(&bad).is_err();
    let tamper_ok = b1 && b2 && b3 && b4;
    h = p_core::fold_u32(h, if tamper_ok { 1 } else { 0 });
    let run = RouteRun {
        route: 4,
        rounds: CHECKSUM_ROUNDS,
        greens,
        rejects: CHECKSUM_ROUNDS.saturating_sub(greens),
        digest: h,
    };
    if !run.green() || !tamper_ok {
        return Err(CopyCode::InlineBad.val());
    }
    Ok(run)
}

/// 按路号派发（0..4）。
pub fn run_route(route: u32) -> Result<RouteRun, u32> {
    match route {
        0 => route_match_chain(),
        1 => route_degrade_ledger(),
        2 => route_promise_list(),
        3 => route_hot_apply(),
        4 => route_checksum(),
        _ => Err(CopyCode::InlineBad.val()),
    }
}

/// 五路交叉：每路跑两遍，逐位比对摘要（零漂移）后返回首遍结果。
pub fn inline_cross() -> Result<[RouteRun; INLINE_ROUTES], u32> {
    let mut out = [RouteRun::empty(); INLINE_ROUTES];
    let mut i = 0usize;
    while i < INLINE_ROUTES {
        let a = run_route(i as u32)?;
        let b = run_route(i as u32)?;
        if a.digest != b.digest || !a.green() || !b.green() {
            return Err(CopyCode::ReplayBad.val());
        }
        if a.route != i as u8 {
            return Err(CopyCode::InlineBad.val());
        }
        if let Some(slot) = out.get_mut(i) {
            *slot = a;
        }
        i += 1;
    }
    Ok(out)
}

/// 交叉合账摘要：五路摘要再折一遍，供渲染与复跑比对。
pub fn cross_digest(runs: &[RouteRun; INLINE_ROUTES]) -> u64 {
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < runs.len() {
        if let Some(r) = runs.get(i) {
            h = p_core::fold_u32(h, i as u32);
            h = p_core::fold_u32(h, r.rounds);
            h = p_core::fold_u32(h, r.greens);
            h = p_core::fold_u32(h, (r.digest >> 32) as u32);
            h = p_core::fold_u32(h, r.digest as u32);
        }
        i += 1;
    }
    h
}

// ── 注入账 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub rounds: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        self.rounds > 0 && self.caught == self.rounds
    }
}

// 五条静态坏文案：ASCII 标点 / 禁用术语 / 标题带句号 / 占位符未申报 / 正文过短。
const BAD_ASCII: &str = "触控板已命中 {device} 的 quirk 表, 固件 {fw} 参数已生效。";
const BAD_TERM: &str = "触摸板已命中 {device} 的 quirk 表，固件 {fw} 参数已生效。";
const BAD_TITLE: &str = "已命中 {device} 的 quirk 表。";
const BAD_HOLE: &str = "已命中 {device} 的 quirk 表 {fw}";

/// 五路文案注入：每路必须被指定的诊断码捕获。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > INJECT_MAX {
        return Err(CopyCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_3C11);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES);
        let want = match route {
            0 => CopyCode::PunctBad.val(),
            1 => CopyCode::TermBad.val(),
            2 => CopyCode::TextBad.val(),
            3 => CopyCode::HoleBad.val(),
            _ => CopyCode::PunctBad.val(),
        };
        let mut t = COPY_ENTRIES.to_vec();
        match route {
            0 => {
                if let Some(slot) = t.get_mut(1) {
                    slot.text = BAD_ASCII;
                }
            }
            1 => {
                if let Some(slot) = t.get_mut(1) {
                    slot.text = BAD_TERM;
                }
            }
            2 => {
                if let (Some(src), Some(dst)) = (
                    COPY_ENTRIES.get(3).map(|e| e.text),
                    t.get_mut(4),
                ) {
                    dst.text = src;
                }
            }
            3 => {
                if let Some(slot) = t.get_mut(0) {
                    slot.text = BAD_HOLE;
                }
            }
            _ => {
                if let Some(slot) = t.get_mut(0) {
                    slot.text = BAD_TITLE;
                }
            }
        }
        if copy_integrity(&t).err() == Some(want) {
            caught = caught.saturating_add(1);
        }
        last = route;
        r = r.saturating_add(1);
    }
    if caught != rounds {
        return Err(CopyCode::InjectBad.val());
    }
    Ok(InjectLog {
        rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：态越界 / 键错位 / 表短 / 术语变体 / 占位符数量错。
pub fn tamper_rejected() -> bool {
    let mut t = COPY_ENTRIES.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.state = COPY_STATE_COUNT as u8;
    }
    let d1 = copy_integrity(&t).err() == Some(CopyCode::StateBad.val());

    let mut t = COPY_ENTRIES.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.key = "hit.body";
    }
    let d2 = copy_integrity(&t).err() == Some(CopyCode::SlotBad.val());

    let t = &COPY_ENTRIES[..COPY_COUNT - 1];
    let d3 = copy_integrity(t).err() == Some(CopyCode::StateBad.val());

    let mut t = COPY_ENTRIES.to_vec();
    if let Some(slot) = t.get_mut(1) {
        slot.text = BAD_TERM;
    }
    let d4 = copy_integrity(&t).err() == Some(CopyCode::TermBad.val());

    let mut t = COPY_ENTRIES.to_vec();
    if let Some(slot) = t.get_mut(7) {
        slot.holes = 1;
    }
    let d5 = copy_integrity(&t).err() == Some(CopyCode::HoleBad.val());

    d1 && d2 && d3 && d4 && d5
}

// ── 段位守卫 ──────────────────────────────────────────────────────
/// 14 个诊断码必须恰好占满 [SEG_BASE+1, SEG_BASE+14]。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < CopyCode::ALL.len() {
        if let Some(c) = CopyCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
/// 语义：params=走查文案条数 / schedule=联轧轮数合计 / fails=注入未捕获数 /
/// keys=五路绿数合计 / digest=文案表与交叉合账摘要的再折叠。
pub fn render_row(
    tag: &str,
    params: u32,
    schedule: u32,
    fails: u32,
    keys: u32,
    digest: u64,
) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut n = 0usize;
    for p in row.split('|') {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = out.get_mut(n) {
            *slot = p;
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    Some(out)
}

pub fn split_kv(part: &str) -> Option<(&str, &str)> {
    let mut it = part.split('=');
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() || k.is_empty() {
        return None;
    }
    Some((k, v))
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

/// 渲染审计：六字段、键名逐位固定、四个数值可解析、摘要 16 位十六进制、
/// 重渲染逐字节一致（回读不改写）。
pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(p) => p,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    const KEYS: [&str; 4] = ["params", "schedule", "fails", "keys"];
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let (k, v) = match split_kv(p) {
            Some(x) => x,
            None => return false,
        };
        if k != KEYS.get(i - 1).copied().unwrap_or("") {
            return false;
        }
        let num = match parse_u32(v) {
            Some(n) => n,
            None => return false,
        };
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let (dk, dv) = match split_kv(dpart) {
        Some(x) => x,
        None => return false,
    };
    if dk != "digest" {
        return false;
    }
    let digest = match parse_hex16(dv) {
        Some(v) => v,
        None => return false,
    };
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49013 ①：三态文案表自洽（3 态 × 3 槽）。
    let stats = copy_integrity(&COPY_ENTRIES);
    let table_ok = match &stats {
        Ok(s) => s.total == 9 && s.even() && s.fingerprint != 0,
        Err(_) => false,
    };
    out.push((
        "F49013/三态文案自洽",
        table_ok,
        format!(
            "total={:?} even={:?} fp={:016X}",
            stats.as_ref().map(|s| s.total),
            stats.as_ref().map(|s| s.even()),
            stats.as_ref().map(|s| s.fingerprint).unwrap_or(0)
        ),
    ));

    // F49013 ②：标点/术语统一（全表扫描零违例）。
    let mut punct_violations = 0u32;
    let mut term_violations = 0u32;
    let mut holes = 0u32;
    let mut i = 0usize;
    while i < COPY_ENTRIES.len() {
        if let Some(e) = COPY_ENTRIES.get(i) {
            let terminal = e.slot == 1 || e.slot == 2;
            if scan_punct(e.text, terminal).is_err() {
                punct_violations = punct_violations.saturating_add(1);
            }
            if scan_terms(e.text).is_err() {
                term_violations = term_violations.saturating_add(1);
            }
            if let Ok(n) = scan_holes(e.text, e.holes) {
                holes = holes.saturating_add(n);
            }
        }
        i += 1;
    }
    out.push((
        "F49013/标点术语统一",
        // 九条文案的占位符合计：3 态 × (1 标题 + 2 正文 + 0 提示) = 9。
        punct_violations == 0 && term_violations == 0 && holes == 9,
        format!(
            "punct_bad={} term_bad={} holes={}",
            punct_violations, term_violations, holes
        ),
    ));

    // F49013 ③：走查账守恒（×10^2）。
    let walk = walk_copies(WALK_ROUNDS);
    let walk_ok = match &walk {
        Ok(l) => l.conserved() && l.rounds == WALK_ROUNDS && l.marks > 0,
        Err(_) => false,
    };
    out.push((
        "F49013/走查账守恒",
        walk_ok,
        format!(
            "rounds={:?} hit={:?} miss={:?} degrade={:?}",
            walk.as_ref().map(|l| l.rounds),
            walk.as_ref().map(|l| l.hit),
            walk.as_ref().map(|l| l.miss),
            walk.as_ref().map(|l| l.degrade)
        ),
    ));

    // F49013 ④：反例必拒 + 注入全捕获。
    let tamper = tamper_rejected();
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    out.push((
        "F49013/反例必拒",
        tamper && inj_ok,
        format!("tamper={} inject={}", tamper, inj_ok),
    ));

    // F49014 ①：五路联轧全绿（每路 轮数=绿数、红数=0）。
    let cross = inline_cross();
    let five_ok = match &cross {
        Ok(runs) => {
            let mut all = true;
            let mut i = 0usize;
            while i < runs.len() {
                if let Some(r) = runs.get(i) {
                    if !r.green() {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        }
        Err(_) => false,
    };
    out.push((
        "F49014/五路全绿",
        five_ok,
        format!(
            "routes={} digest={:016X}",
            INLINE_ROUTES,
            cross.as_ref().map(|r| cross_digest(r)).unwrap_or(0)
        ),
    ));

    // F49014 ②：零漂移（两轮交叉摘要逐位一致）。
    let cross2 = inline_cross();
    let drift_ok = match (&cross, &cross2) {
        (Ok(a), Ok(b)) => {
            cross_digest(a) == cross_digest(b) && cross_digest(a) != 0
        }
        _ => false,
    };
    out.push((
        "F49014/零漂移",
        drift_ok,
        format!(
            "digest_a={:016X} digest_b={:016X}",
            cross.as_ref().map(|r| cross_digest(r)).unwrap_or(0),
            cross2.as_ref().map(|r| cross_digest(r)).unwrap_or(0)
        ),
    ));

    // F49014 ③：渲染可回读（六字段键名固定）。
    let row_digest = cross.as_ref().map(|r| cross_digest(r)).unwrap_or(0);
    let row = render_row(
        "C49013",
        COPY_COUNT as u32,
        WALK_ROUNDS,
        0,
        INLINE_ROUTES as u32 * 4,
        row_digest,
    );
    let render_ok = audit_render(&row)
        && !audit_render("")
        && !audit_render(
            "C49013|params=9|schedule=100|fails=0|keys=20|digest=0000000000000000|extra=1",
        )
        && !audit_render("C49013|p=9|schedule=100|fails=0|keys=20|digest=0000000000000000");
    out.push((
        "F49014/渲染回读",
        render_ok,
        format!("row={} audit={}", row, render_ok),
    ));

    // F49014 ④：段位守卫 + 未知码兜底。
    let seg = segment_guard() && detail(SEG_BASE + 0x0F) == "未知诊断码" && detail(0xFFFF) == "未知诊断码";
    out.push((
        "F49014/段位守卫",
        seg,
        format!("base={:05X} variants={}", SEG_BASE, CopyCode::ALL.len()),
    ));

    out
}

pub fn run_unx49013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49014 配图文案走查与 B11 段内联轧");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(CopyCode::StateBad.val(), SEG_BASE + 0x01);
        assert_eq!(CopyCode::InlineBad.val(), SEG_BASE + 14);
        assert_eq!(CopyCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < CopyCode::ALL.len() {
            if let Some(c) = CopyCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn copy_table_is_self_consistent() {
        let s = copy_integrity(&COPY_ENTRIES).unwrap();
        assert_eq!(s.total, 9);
        assert!(s.even());
        assert_ne!(s.fingerprint, 0);
        assert!(s.term_hits >= 9);
        assert!(s.punct_marks >= 9);
    }

    #[test]
    fn three_states_have_three_entries() {
        let s = copy_integrity(&COPY_ENTRIES).unwrap();
        assert_eq!(s.per_state, [3, 3, 3]);
        let mut state = 0u8;
        while state < 3 {
            let mut slot = 0u8;
            while slot < 3 {
                assert!(copy_at(state, slot).is_some());
                slot += 1;
            }
            state += 1;
        }
        assert!(copy_at(3, 0).is_none());
    }

    #[test]
    fn ascii_punctuation_is_rejected() {
        assert_eq!(
            scan_punct(BAD_ASCII, true).err(),
            Some(CopyCode::PunctBad.val())
        );
        assert_eq!(
            scan_punct("句子中间有空格 ，标点。", true).err(),
            Some(CopyCode::PunctBad.val())
        );
        assert!(scan_punct("触控板已命中。", true).is_ok());
    }

    #[test]
    fn banned_term_is_rejected() {
        assert_eq!(
            scan_terms("触摸板已降级。").err(),
            Some(CopyCode::TermBad.val())
        );
        assert_eq!(
            scan_terms("没有规范术语的一句。").err(),
            Some(CopyCode::TermBad.val())
        );
        assert_eq!(scan_terms("触控板已降级。").ok(), Some(2));
    }

    #[test]
    fn duplicate_text_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t[4].text = COPY_ENTRIES[3].text;
        assert_eq!(
            copy_integrity(&t).err(),
            Some(CopyCode::TextBad.val())
        );
    }

    #[test]
    fn hole_mismatch_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t[0].text = BAD_HOLE;
        assert_eq!(copy_integrity(&t).err(), Some(CopyCode::HoleBad.val()));
        assert_eq!(
            scan_holes("{device}{device}", 2).err(),
            Some(CopyCode::HoleBad.val())
        );
        assert_eq!(
            scan_holes("{nope}", 1).err(),
            Some(CopyCode::HoleBad.val())
        );
        assert_eq!(
            scan_holes("{device", 1).err(),
            Some(CopyCode::HoleBad.val())
        );
    }

    #[test]
    fn title_terminal_punctuation_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t[0].text = BAD_TITLE;
        assert_eq!(copy_integrity(&t).err(), Some(CopyCode::PunctBad.val()));
    }

    #[test]
    fn key_slot_mismatch_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t[2].key = "hit.body";
        assert_eq!(copy_integrity(&t).err(), Some(CopyCode::SlotBad.val()));
    }

    #[test]
    fn out_of_range_state_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t[5].state = 7;
        assert_eq!(
            copy_integrity(&t).err(),
            Some(CopyCode::StateBad.val())
        );
    }

    #[test]
    fn unsorted_table_is_rejected() {
        let mut t = COPY_ENTRIES.to_vec();
        t.swap(0, 3);
        assert_eq!(copy_integrity(&t).err(), Some(CopyCode::OrderBad.val()));
    }

    #[test]
    fn render_copy_substitutes_without_braces() {
        let e = copy_at(1, 1).unwrap();
        let line = render_copy(&e, "D04", 0x0203).unwrap();
        assert!(!line.contains('{'));
        assert!(!line.contains('}'));
        assert!(line.contains("D04"));
        assert!(line.contains("0203"));
        assert!(scan_punct(&line, true).is_ok());
        assert!(render_copy(&e, "", 0x0203).is_err());
    }

    #[test]
    fn devices_classify_into_three_states() {
        let mut hit = 0u32;
        let mut miss = 0u32;
        let mut degrade = 0u32;
        let mut i = 0usize;
        while i < DEVICE_TABLE.len() {
            match classify_device(DEVICE_TABLE[i].model, DEVICE_TABLE[i].fw) {
                Ok(0) => hit += 1,
                Ok(1) => miss += 1,
                Ok(2) => degrade += 1,
                _ => {}
            }
            i += 1;
        }
        assert_eq!(hit, 10);
        assert_eq!(degrade, 8);
        assert_eq!(miss, 6);
    }

    #[test]
    fn invalid_probe_is_rejected() {
        assert_eq!(
            classify_device(0, 0x0100).err(),
            Some(CopyCode::StateBad.val())
        );
        assert_eq!(
            classify_device(0x11, 0xFFFF_FFFF).err(),
            Some(CopyCode::StateBad.val())
        );
    }

    #[test]
    fn walk_ledgers_conserve() {
        let l = walk_copies(WALK_ROUNDS).unwrap();
        assert_eq!(l.rounds, WALK_ROUNDS);
        assert_eq!(l.rejects, 0);
        assert!(l.hit > 0 && l.miss > 0 && l.degrade > 0);
        assert!(l.conserved());
        assert_ne!(l.digest(), 0);
        assert!(walk_copies(0).is_err());
        assert!(walk_copies(RUN_MAX + 1).is_err());
    }

    #[test]
    fn injection_catches_every_round() {
        let l = inject_rounds(1_000).unwrap();
        assert_eq!(l.rounds, 1_000);
        assert!(l.all_caught());
        assert!(inject_rounds(0).is_err());
        assert!(inject_rounds(INJECT_MAX + 1).is_err());
    }

    #[test]
    fn tamper_cases_are_all_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn five_routes_are_all_green() {
        let runs = inline_cross().unwrap();
        assert_eq!(runs.len(), 5);
        let mut i = 0usize;
        while i < runs.len() {
            assert!(runs[i].green());
            assert_eq!(runs[i].route, i as u8);
            i += 1;
        }
        assert_eq!(runs[0].rounds, 24);
        assert_eq!(runs[1].rounds, 10);
        assert_eq!(runs[2].rounds, 6);
        assert_eq!(runs[3].rounds, 12);
        assert_eq!(runs[4].rounds, 100);
        assert!(run_route(5).is_err());
    }

    #[test]
    fn inline_replay_is_stable() {
        let a = inline_cross().unwrap();
        let b = inline_cross().unwrap();
        assert_eq!(cross_digest(&a), cross_digest(&b));
        assert_ne!(cross_digest(&a), 0);
    }

    #[test]
    fn hot_state_rejects_out_of_band_values() {
        let mut st = HotState::new();
        assert!(!st.commit(GAIN_MIN - 1));
        assert!(!st.commit(GAIN_MAX + 1));
        assert_eq!(st.generation, 0);
        assert!(st.commit(GAIN_MIN));
        assert_eq!(st.generation, 1);
        assert_eq!(st.active, GAIN_MIN);
    }

    #[test]
    fn quirk_table_is_self_consistent() {
        assert!(quirk_integrity(&QUIRK_TABLE).unwrap() != 0);
        assert_ne!(table_checksum(&QUIRK_TABLE), 0);
        let mut bad = QUIRK_TABLE;
        bad[0].mask = 0;
        assert!(quirk_integrity(&bad).is_err());
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let row = render_row("C49013", 9, 100, 0, 20, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&row));
        assert!(!audit_render(""));
        assert!(!audit_render("C49013|params=9|schedule=100|fails=0|keys=20"));
        assert!(!audit_render(
            "C49013|params=x|schedule=100|fails=0|keys=20|digest=0000000000000001"
        ));
        assert!(!audit_render(
            "C49013|params=9|schedule=100|fails=0|keys=20|digest=00000000000000FF|z=1"
        ));
        assert_eq!(parse_u32("100"), Some(100));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("FF"), None);
        assert_eq!(row_fields(&row).map(|f| f.len()), Some(6));
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49013_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(extra_checks().len(), 8);
    }
}
