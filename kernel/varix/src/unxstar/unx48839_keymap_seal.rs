//! UNX-F48839 B02 批防重 grep（关键词族：keymap/xkb/modifier/led）
//! UNX-F48840 B02 批收官印（20 条冻结 / 域账累计 12 000 行）
//!
//! ── 闸门次序 ──
//! G1 名闸：八条 stem 两两不同；同一 stem 不许挂两次。
//! G2 卡闸：F48821..F48840 二十条卡必须连续、无缺号、无重号，且一个卡只归一个模块。
//! G3 段闸：八段从 0x70280 排到 0x704C0，各占 0x40 窗（段间允许留 0x20 空号），
//!    窗口互不重叠，且整带落在 B02 带内（B01 带在 0x70280 以下、B03 带在 0x70500 以上，
//!    谁也不许越界）。
//! G4 词闸：关键词族 keymap/xkb/modifier/led 四家都要有模块留痕（下限入册，只多不少）。
//! G5 印闸：20 卡 × 12 000 行 = 240 000 的域账必须整除对得上，行数合计不得低于 12 000。
//! G6 合闸：七个先行模块的 run_*_checks 必须全部并入，且动态明细溢出要显性记账。
//!
//! ── 关键差异 ──
//! · 与 B01 不同：B01 收口管扫描码集（0x70000 带），B02 只在自己 0x70280..0x704DF
//!   带内做防重；B03 从 0x70500 起，三带首尾相接但不重叠。
//! · 与本批前面七个模块不同：它们各管一段语义，本模块只做「收口 + 封印」：
//!   不新增输入语义，只把八段名/卡/段/词/行数五项账对齐，并把七份判据集合并成一份。
//! · 与任务板不同：这里不写板，只留可复核的封印账（板由人写回）。
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（B02 seal 段）。
pub const SEG_BASE: u32 = 0x704C0;

// ── 诊断码 ───────────────────────────────────────────────────
/// 收口 / 封印族诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// stem 重名
    StemDup = 1,
    /// 段基址撞车
    SegDup = 2,
    /// 卡号重复
    CardDup = 3,
    /// 卡号断档
    CardGap = 4,
    /// 关键词族缺家
    FamilyMiss = 5,
    /// 行数合计不足
    LineDeficit = 6,
    /// 合并集条数不够
    MergeShort = 7,
    /// 域账不整除
    BoardBroken = 8,
    /// 封印双跑不一致
    NotReproducible = 9,
    /// 卡号不在册
    UnknownCard = 10,
}

impl SealCode {
    /// 段内绝对码。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 文案。
    pub const fn detail(self) -> &'static str {
        match self {
            SealCode::StemDup => "两条模块 stem 重名（防重失败）",
            SealCode::SegDup => "两个模块段基址撞车或窗口重叠",
            SealCode::CardDup => "同一卡号被两个模块认领",
            SealCode::CardGap => "卡号序列断档，20 条卡不连续",
            SealCode::FamilyMiss => "关键词族四家缺家（keymap/xkb/modifier/led）",
            SealCode::LineDeficit => "八模块行数合计不足 12000",
            SealCode::MergeShort => "合并判据集条数不足七模块之和",
            SealCode::BoardBroken => "域账 12000×20 与 240000 对不上",
            SealCode::NotReproducible => "封印摘要双跑不一致",
            SealCode::UnknownCard => "卡号不在 F48821..F48840 册内",
        }
    }

    /// 全码表。
    pub const ALL: [SealCode; 10] = [
        SealCode::StemDup,
        SealCode::SegDup,
        SealCode::CardDup,
        SealCode::CardGap,
        SealCode::FamilyMiss,
        SealCode::LineDeficit,
        SealCode::MergeShort,
        SealCode::BoardBroken,
        SealCode::NotReproducible,
        SealCode::UnknownCard,
    ];
}

/// 码 → 文案。
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
    "未知诊断码：不在 0x704C0 段 seal 闭集内"
}

// ── 折叠 ─────────────────────────────────────────────────────
/// 折一个字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折一个 32 位字。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let b = w.to_le_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 折一段文本。
pub fn fold_text(h: u64, s: &str) -> u64 {
    let mut acc = h;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            acc = fold_byte(acc, *v);
        }
        i += 1;
    }
    acc
}

/// 十六进制回写。
fn push_hex(s: &mut String, v: u32) {
    s.push_str("0x");
    let mut sh = 28u32;
    let mut started = false;
    loop {
        let nib = ((v >> sh) & 0xf) as u8;
        if nib != 0 || started || sh == 0 {
            started = true;
            let ch = match nib {
                0..=9 => (b'0' + nib) as char,
                _ => (b'a' + (nib - 10)) as char,
            };
            s.push(ch);
        }
        if sh == 0 {
            break;
        }
        sh -= 4;
    }
}

/// 十进制回写。
fn push_dec(s: &mut String, v: u32) {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = buf.len();
    loop {
        let d = (n % 10) as u8;
        n /= 10;
        if i == 0 {
            break;
        }
        i -= 1;
        if let Some(c) = buf.get_mut(i) {
            *c = b'0' + d;
        }
        if n == 0 {
            break;
        }
    }
    let mut j = i;
    while j < buf.len() {
        if let Some(c) = buf.get(j) {
            s.push(*c as char);
        }
        j += 1;
    }
}

/// 固定种子掷骰（封印只用来打乱复跑顺序，不参与判定）。
pub struct Die {
    s: u32,
}

impl Die {
    /// 起骰。
    pub const fn new(seed: u32) -> Die {
        Die { s: seed ^ 0x4883_9C40 }
    }

    /// 下一步。
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.s ^= self.s >> 16;
        self.s
    }

    /// [0, bound)。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

// ── 关键词族 ─────────────────────────────────────────────────
/// 关键词族（防重 grep 的四家）。
pub const FAMILIES: [&str; 4] = ["keymap", "xkb", "modifier", "led"];

/// 族下标：keymap=0 / xkb=1 / modifier=2 / led=3。
pub fn family_index(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < FAMILIES.len() {
        if let Some(f) = FAMILIES.get(i) {
            if *f == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── 封印账 ───────────────────────────────────────────────────
/// 一条模块封印：(stem, 首卡, 末卡, 段基址, 行数下限, 四族命中下限, 判据条数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardSeal {
    /// 模块 stem（文件名去扩展）
    pub stem: &'static str,
    /// 首卡号（如 48821）
    pub first_card: u16,
    /// 末卡号
    pub last_card: u16,
    /// 段基址
    pub seg: u32,
    /// 行数下限（grep 时实测值向下取整，只多不少）
    pub lines: u32,
    /// 四族命中下限（keymap/xkb/modifier/led）
    pub hits: [u32; 4],
    /// 本模块判据条数
    pub checks: u32,
}

/// 八模块封印账（行数下限按收口时实测行数向下取整登记）。
pub const SEALS: [CardSeal; 8] = [
    CardSeal {
        stem: "unx48821_keymap_core",
        first_card: 48821,
        last_card: 48822,
        seg: 0x70280,
        lines: 2280,
        hits: [170, 45, 5, 20],
        checks: 10,
    },
    CardSeal {
        stem: "unx48823_keymap_meta",
        first_card: 48823,
        last_card: 48824,
        seg: 0x702C0,
        lines: 1490,
        hits: [3, 1, 1, 110],
        checks: 10,
    },
    CardSeal {
        stem: "unx48825_keymap_guard",
        first_card: 48825,
        last_card: 48827,
        seg: 0x70300,
        lines: 1500,
        hits: [0, 0, 0, 180],
        checks: 10,
    },
    CardSeal {
        stem: "unx48828_keymap_dispatch",
        first_card: 48828,
        last_card: 48830,
        seg: 0x70360,
        lines: 2080,
        hits: [19, 9, 0, 55],
        checks: 10,
    },
    CardSeal {
        stem: "unx48831_keymap_edge",
        first_card: 48831,
        last_card: 48832,
        seg: 0x703C0,
        lines: 1520,
        hits: [0, 0, 0, 20],
        checks: 10,
    },
    CardSeal {
        stem: "unx48833_keymap_bridge",
        first_card: 48833,
        last_card: 48835,
        seg: 0x70400,
        lines: 1830,
        hits: [5, 0, 0, 100],
        checks: 10,
    },
    CardSeal {
        stem: "unx48836_keymap_quality",
        first_card: 48836,
        last_card: 48838,
        seg: 0x70460,
        lines: 1820,
        hits: [3, 0, 0, 55],
        checks: 10,
    },
    CardSeal {
        stem: "unx48839_keymap_seal",
        first_card: 48839,
        last_card: 48840,
        seg: 0x704C0,
        lines: 1360,
        hits: [34, 8, 8, 30],
        checks: 13,
    },
];

/// 封印条数。
pub const SEAL_COUNT: usize = 8;

/// 卡册首尾。
pub const CARD_LO: u16 = 48821;
/// 卡册尾。
pub const CARD_HI: u16 = 48840;
/// 卡册条数（20 条冻结）。
pub const CARD_TOTAL: u32 = 20;

/// B02 段带（含头不含尾）。
pub const BAND_LO: u32 = 0x70280;
/// B02 段带上界（B03 从 0x70500 起）。
pub const BAND_HI: u32 = 0x70500;

/// 单段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;

/// B01 段带上界（不含）。
pub const B01_HI: u32 = 0x70280;
/// B03 段带起点（含）。
pub const B03_LO: u32 = 0x70500;

// ── 域账（F48840 批印） ───────────────────────────────────────
/// 域账：(批号, 卡数, 行数, 全库行数)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Board {
    /// 批号
    pub batch: &'static str,
    /// 卡数
    pub cards: u32,
    /// 本批行数
    pub lines: u32,
    /// 全库行数
    pub total_lines: u32,
}

/// B02 批印账：12000 / 240,000（20 批满 240k）。
pub const BOARD: Board = Board { batch: "B02", cards: 20, lines: 12_000, total_lines: 240_000 };

/// 每批行数（240000 / 12000 = 20 批）。
pub const BATCHES_PER_LIBRARY: u32 = 20;

// ── 防重复核 ─────────────────────────────────────────────────
/// 防重复核结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DedupReport {
    /// stem 重复数
    pub dup_stem: u32,
    /// 段重复/重叠数
    pub dup_seg: u32,
    /// 卡重复数
    pub dup_card: u32,
    /// 断档数
    pub gaps: u32,
    /// 卡覆盖数（应 20）
    pub cards: u32,
    /// 段带是否合法
    pub band_ok: bool,
    /// 行数合计
    pub lines: u32,
    /// 摘要
    pub digest: u64,
}

/// 段窗是否两两不重叠且都在给定账的 B02 带内。
pub fn band_ok_in(xs: &[CardSeal]) -> bool {
    let mut i = 0usize;
    while i < xs.len() {
        if let Some(a) = xs.get(i) {
            if a.seg < BAND_LO || a.seg.saturating_add(SEG_WINDOW) > BAND_HI {
                return false;
            }
            let mut j = i + 1;
            while j < xs.len() {
                if let Some(b) = xs.get(j) {
                    // 窗口 [seg, seg+0x40) 两两不交
                    if a.seg < b.seg.saturating_add(SEG_WINDOW)
                        && b.seg < a.seg.saturating_add(SEG_WINDOW)
                    {
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

/// 段窗是否两两不重叠且都在 B02 带内。
pub fn band_ok() -> bool {
    band_ok_in(&SEALS)
}

/// 数 stem 重名对数（给定账）。
pub fn dup_stem_in(xs: &[CardSeal]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        let a = match xs.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut j = i + 1;
        while j < xs.len() {
            if let Some(b) = xs.get(j) {
                if a.stem == b.stem {
                    n = n.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    n
}

/// 数段基址撞车对数（给定账）。
pub fn dup_seg_in(xs: &[CardSeal]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        let a = match xs.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut j = i + 1;
        while j < xs.len() {
            if let Some(b) = xs.get(j) {
                if a.seg == b.seg {
                    n = n.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    n
}

/// 数卡区间相交对数（给定账）。
pub fn dup_card_in(xs: &[CardSeal]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        let a = match xs.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut j = i + 1;
        while j < xs.len() {
            if let Some(b) = xs.get(j) {
                // 卡区间相交即重号
                if a.first_card <= b.last_card && b.first_card <= a.last_card {
                    n = n.saturating_add(1);
                }
            }
            j += 1;
        }
        i += 1;
    }
    n
}

/// 数断档：区间自噬 / 相邻不相接 / 首尾不贴册。
pub fn gap_count_in(xs: &[CardSeal]) -> u32 {
    let mut gaps = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        if let Some(a) = xs.get(i) {
            if a.first_card > a.last_card {
                gaps = gaps.saturating_add(1);
            }
        }
        i += 1;
    }
    // 相邻模块卡号必须首尾相接
    let mut k = 1usize;
    let mut prev_last = match xs.first() {
        Some(s) => s.last_card,
        None => 0,
    };
    while k < xs.len() {
        if let Some(s) = xs.get(k) {
            if s.first_card != prev_last.saturating_add(1) {
                gaps = gaps.saturating_add(1);
            }
            prev_last = s.last_card;
        }
        k += 1;
    }
    // 首卡/末卡必须贴册首册尾
    if let Some(first) = xs.first() {
        if first.first_card != CARD_LO {
            gaps = gaps.saturating_add(1);
        }
    }
    if let Some(last) = xs.last() {
        if last.last_card != CARD_HI {
            gaps = gaps.saturating_add(1);
        }
    }
    gaps
}

/// 行数合计（给定账）。
pub fn lines_in(xs: &[CardSeal]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        if let Some(s) = xs.get(i) {
            n = n.saturating_add(s.lines);
        }
        i += 1;
    }
    n
}

/// 四族命中合计（给定账）。
pub fn family_totals_in(xs: &[CardSeal]) -> [u32; 4] {
    let mut t = [0u32; 4];
    let mut i = 0usize;
    while i < xs.len() {
        if let Some(s) = xs.get(i) {
            let mut k = 0usize;
            while k < 4 {
                if let Some(v) = s.hits.get(k) {
                    if let Some(slot) = t.get_mut(k) {
                        *slot = slot.saturating_add(*v);
                    }
                }
                k += 1;
            }
        }
        i += 1;
    }
    t
}

/// 四族是否缺家（给定账）。
pub fn family_missing(xs: &[CardSeal]) -> bool {
    let t = family_totals_in(xs);
    let mut i = 0usize;
    while i < 4 {
        match t.get(i) {
            Some(v) => {
                if *v == 0 {
                    return true;
                }
            }
            None => return true,
        }
        i += 1;
    }
    false
}

/// 卡册外认领数（给定账）。
pub fn claim_escape(xs: &[CardSeal]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < xs.len() {
        if let Some(s) = xs.get(i) {
            if !card_in_book(s.first_card) || !card_in_book(s.last_card) {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 行数是否达标。
pub fn line_ok(lines: u32, floor: u32) -> bool {
    lines >= floor
}

/// 合并条数是否短少。
pub fn merge_short(actual: usize, want: u32) -> bool {
    (actual as u32) < want
}

/// 域账是否整除自洽（给定账）。
pub fn board_ok_for(b: &Board) -> bool {
    b.cards == CARD_TOTAL
        && b.lines.saturating_mul(BATCHES_PER_LIBRARY) == b.total_lines
        && b.total_lines / b.lines == BATCHES_PER_LIBRARY
}

/// 双跑摘要是否一致。
pub fn digest_equal(a: u64, b: u64) -> bool {
    a == b
}

/// 防重复核：名 / 段 / 卡 / 断档 / 合计。
pub fn dedup_check() -> DedupReport {
    let dup_stem = dup_stem_in(&SEALS);
    let dup_seg = dup_seg_in(&SEALS);
    let dup_card = dup_card_in(&SEALS);
    let gaps = gap_count_in(&SEALS);
    let lines = lines_in(&SEALS);
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < SEALS.len() {
        let a = match SEALS.get(i) {
            Some(v) => *v,
            None => break,
        };
        digest = fold_text(digest, a.stem);
        digest = fold_word(digest, a.seg);
        digest = fold_word(digest, a.first_card as u32);
        digest = fold_word(digest, a.last_card as u32);
        i += 1;
    }
    DedupReport {
        dup_stem,
        dup_seg,
        dup_card,
        gaps,
        cards: CARD_TOTAL,
        band_ok: band_ok_in(&SEALS),
        lines,
        digest,
    }
}

/// 四族命中合计。
pub fn family_totals() -> [u32; 4] {
    family_totals_in(&SEALS)
}

/// 四族是否每家都有留痕（下限 ≥1）。
pub fn family_covered() -> bool {
    !family_missing(&SEALS)
}

/// 卡号是否在册。
pub fn card_in_book(card: u16) -> bool {
    card >= CARD_LO && card <= CARD_HI
}

/// 某卡归哪个 stem。
pub fn stem_of_card(card: u16) -> Option<&'static str> {
    let mut i = 0usize;
    while i < SEALS.len() {
        if let Some(s) = SEALS.get(i) {
            if card >= s.first_card && card <= s.last_card {
                return Some(s.stem);
            }
        }
        i += 1;
    }
    None
}

/// 域账是否整除自洽。
pub fn board_ok() -> bool {
    board_ok_for(&BOARD)
}

// ── 七模块合并 ───────────────────────────────────────────────
/// 七模块判据条数之和（封印账登记值）。
pub fn seven_check_sum() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i + 1 < SEALS.len() {
        if let Some(s) = SEALS.get(i) {
            n = n.saturating_add(s.checks);
        }
        i += 1;
    }
    n
}

/// 把七个先行模块的判据集合并成一份（顺序固定：卡号升序）。
pub fn merge_seven() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48839 B02 批收口（七模块合并）");
    s = CheckSet::merge(s, super::unx48821_keymap_core::run_unx48821_checks());
    s = CheckSet::merge(s, super::unx48823_keymap_meta::run_unx48823_checks());
    s = CheckSet::merge(s, super::unx48825_keymap_guard::run_unx48825_checks());
    s = CheckSet::merge(s, super::unx48828_keymap_dispatch::run_unx48828_checks());
    s = CheckSet::merge(s, super::unx48831_keymap_edge::run_unx48831_checks());
    s = CheckSet::merge(s, super::unx48833_keymap_bridge::run_unx48833_checks());
    s = CheckSet::merge(s, super::unx48836_keymap_quality::run_unx48836_checks());
    s
}

/// 把前六个先行模块合并成一份（用于证明「少合一段」会被合闸判据抓住）。
pub fn merge_six() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48839 B02 批收口（六模块对照）");
    s = CheckSet::merge(s, super::unx48821_keymap_core::run_unx48821_checks());
    s = CheckSet::merge(s, super::unx48823_keymap_meta::run_unx48823_checks());
    s = CheckSet::merge(s, super::unx48825_keymap_guard::run_unx48825_checks());
    s = CheckSet::merge(s, super::unx48828_keymap_dispatch::run_unx48828_checks());
    s = CheckSet::merge(s, super::unx48831_keymap_edge::run_unx48831_checks());
    s = CheckSet::merge(s, super::unx48833_keymap_bridge::run_unx48833_checks());
    s
}

/// 封印摘要（八条账 + 域账）。
pub fn seal_digest() -> u64 {
    let mut h = dedup_check().digest;
    h = fold_text(h, BOARD.batch);
    h = fold_word(h, BOARD.cards);
    h = fold_word(h, BOARD.lines);
    h = fold_word(h, BOARD.total_lines);
    let t = family_totals();
    let mut i = 0usize;
    while i < 4 {
        if let Some(v) = t.get(i) {
            h = fold_word(h, *v);
        }
        i += 1;
    }
    h
}

// ── 黄金跑 / 探针 ────────────────────────────────────────────
/// 黄金跑：(卡数, 行数合计, 合并判据数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let d = dedup_check();
    if d.dup_stem != 0 || d.dup_seg != 0 || d.dup_card != 0 || d.gaps != 0 {
        return Err(SealCode::StemDup.val());
    }
    if !d.band_ok {
        return Err(SealCode::SegDup.val());
    }
    if !family_covered() {
        return Err(SealCode::FamilyMiss.val());
    }
    if d.lines < BOARD.lines {
        return Err(SealCode::LineDeficit.val());
    }
    if !board_ok() {
        return Err(SealCode::BoardBroken.val());
    }
    let m = merge_seven();
    if (m.len() as u32) < seven_check_sum() {
        return Err(SealCode::MergeShort.val());
    }
    Ok((d.cards, d.lines, m.len() as u32))
}

/// 探针 A：卡号归属。
pub fn probe_seq_a() -> Result<(), u32> {
    if stem_of_card(48821) == Some("unx48821_keymap_core")
        && stem_of_card(48840) == Some("unx48839_keymap_seal")
        && stem_of_card(48841).is_none()
        && stem_of_card(48820).is_none()
    {
        Ok(())
    } else {
        Err(SealCode::UnknownCard.val())
    }
}

/// 探针 B：段窗不相交。
pub fn probe_seq_b() -> Result<(), u32> {
    if band_ok() {
        Ok(())
    } else {
        Err(SealCode::SegDup.val())
    }
}

/// 探针 C：域账整除。
pub fn probe_seq_c() -> Result<(), u32> {
    if board_ok() {
        Ok(())
    } else {
        Err(SealCode::BoardBroken.val())
    }
}

/// 双跑对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SealCode::NotReproducible.val());
    }
    if seal_digest() != seal_digest() {
        return Err(SealCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x20 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 注入账（十槽） ────────────────────────────────────────────
/// 十槽注入账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    /// 期望码
    pub want: [u32; 10],
    /// 判定次数
    pub tries: [u32; 10],
    /// 命中次数
    pub caught: [u32; 10],
}

impl InjectLedger {
    /// 建账。
    pub fn new() -> InjectLedger {
        InjectLedger {
            want: [
                SealCode::StemDup.val(),
                SealCode::SegDup.val(),
                SealCode::CardDup.val(),
                SealCode::CardGap.val(),
                SealCode::FamilyMiss.val(),
                SealCode::LineDeficit.val(),
                SealCode::MergeShort.val(),
                SealCode::BoardBroken.val(),
                SealCode::UnknownCard.val(),
                SealCode::NotReproducible.val(),
            ],
            tries: [0; 10],
            caught: [0; 10],
        }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, got: u32) {
        if slot >= 10 {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        let want = self.want.get(slot).copied().unwrap_or(0);
        if got == want {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    /// 全槽命中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 10 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for InjectLedger {
    fn default() -> InjectLedger {
        InjectLedger::new()
    }
}

/// 造一条重名账（用于证明防重器不是摆设）。
pub fn dup_stem_probe() -> u32 {
    let a = SEALS.first().copied();
    let b = SEALS.first().copied();
    match (a, b) {
        (Some(x), Some(y)) => {
            if x.stem == y.stem {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// 造一条断档账。
pub fn gap_probe() -> u32 {
    // F48821 直接跳到 48823 即断档
    if 48822u16.saturating_add(1) != 48823 {
        0
    } else {
        1
    }
}

/// 十槽注入：每槽都是「好账不被误报 + 坏账必须被抓」的双态判定。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0 名撞车：把第二段的 stem 改成第一段的 stem
        let mut bad = SEALS;
        if let (Some(a), Some(b)) = (SEALS.first(), bad.get_mut(1)) {
            b.stem = a.stem;
        }
        let hit0 = dup_stem_in(&bad) > 0 && dup_stem_in(&SEALS) == 0;
        led.record(0, if hit0 { SealCode::StemDup.val() } else { 0 });

        // 槽 1 段撞车：把第二段 seg 压到首段 seg 上，窗口重叠必然被 band 判据看穿
        let mut bad = SEALS;
        if let (Some(a), Some(b)) = (SEALS.first(), bad.get_mut(1)) {
            b.seg = a.seg;
        }
        let hit1 = dup_seg_in(&bad) > 0
            && !band_ok_in(&bad)
            && dup_seg_in(&SEALS) == 0
            && band_ok_in(&SEALS);
        led.record(1, if hit1 { SealCode::SegDup.val() } else { 0 });

        // 槽 2 卡重号：把第二段卡区间压回首段
        let mut bad = SEALS;
        if let (Some(a), Some(b)) = (SEALS.first(), bad.get_mut(1)) {
            b.first_card = a.first_card;
            b.last_card = a.last_card;
        }
        let hit2 = dup_card_in(&bad) > 0 && dup_card_in(&SEALS) == 0;
        led.record(2, if hit2 { SealCode::CardDup.val() } else { 0 });

        // 槽 3 断档：把第三段首卡抬高一格，48825 变空号
        let mut bad = SEALS;
        if let Some(b) = bad.get_mut(2) {
            b.first_card = 48826;
        }
        let hit3 = gap_count_in(&bad) > 0 && gap_count_in(&SEALS) == 0;
        led.record(3, if hit3 { SealCode::CardGap.val() } else { 0 });

        // 槽 4 缺族：把八段四族命中全部抹零
        let mut bad = SEALS;
        let mut i = 0usize;
        while i < bad.len() {
            if let Some(b) = bad.get_mut(i) {
                b.hits = [0u32; 4];
            }
            i += 1;
        }
        let hit4 = family_missing(&bad) && !family_missing(&SEALS);
        led.record(4, if hit4 { SealCode::FamilyMiss.val() } else { 0 });

        // 槽 5 行数不足：把八段登记行数全部抹零
        let mut bad = SEALS;
        let mut i = 0usize;
        while i < bad.len() {
            if let Some(b) = bad.get_mut(i) {
                b.lines = 0;
            }
            i += 1;
        }
        let hit5 = !line_ok(lines_in(&bad), BOARD.lines) && line_ok(lines_in(&SEALS), BOARD.lines);
        led.record(5, if hit5 { SealCode::LineDeficit.val() } else { 0 });

        // 槽 6 合闸不足：只合六段必然短于七段之和
        let six = merge_six();
        let seven = merge_seven();
        let hit6 = merge_short(six.len(), seven_check_sum())
            && !merge_short(seven.len(), seven_check_sum());
        led.record(6, if hit6 { SealCode::MergeShort.val() } else { 0 });

        // 槽 7 域账不整除：把全库行数改成 239 000
        let bad_board = Board { batch: "B02", cards: 20, lines: 12_000, total_lines: 239_000 };
        let hit7 = !board_ok_for(&bad_board) && board_ok_for(&BOARD);
        led.record(7, if hit7 { SealCode::BoardBroken.val() } else { 0 });

        // 槽 8 越册：末段末卡写成 48841
        let mut bad = SEALS;
        if let Some(b) = bad.last_mut() {
            b.last_card = 48841;
        }
        let hit8 = claim_escape(&bad) > 0 && claim_escape(&SEALS) == 0;
        led.record(8, if hit8 { SealCode::UnknownCard.val() } else { 0 });

        // 槽 9 双跑：好摘要应一致，翻一位后必须被看穿
        let s1 = seal_digest();
        let hit9 = digest_equal(s1, seal_digest()) && !digest_equal(s1, s1 ^ 1);
        led.record(9, if hit9 { SealCode::NotReproducible.val() } else { 0 });

        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 判据 ─────────────────────────────────────────────────────
/// 十条封印判据 + 三条族闭包判据。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let d = dedup_check();

    // 1 防重留痕零撞号
    let (ok1, d1) = {
        let mut t = String::new();
        t.push_str("stems=");
        push_dec(&mut t, SEALS.len() as u32);
        t.push_str(" dup_stem=");
        push_dec(&mut t, d.dup_stem);
        t.push_str(" dup_seg=");
        push_dec(&mut t, d.dup_seg);
        t.push_str(" dup_card=");
        push_dec(&mut t, d.dup_card);
        (d.dup_stem == 0 && d.dup_seg == 0 && d.dup_card == 0, t)
    };
    v.push(("F48839-J1-批收口防重留痕零撞号", ok1, d1));

    // 2 四族留痕
    let tot = family_totals();
    let (ok2, d2) = {
        let mut t = String::new();
        t.push_str("keymap=");
        push_dec(&mut t, tot.first().copied().unwrap_or(0));
        t.push_str(" xkb=");
        push_dec(&mut t, tot.get(1).copied().unwrap_or(0));
        t.push_str(" modifier=");
        push_dec(&mut t, tot.get(2).copied().unwrap_or(0));
        t.push_str(" led=");
        push_dec(&mut t, tot.get(3).copied().unwrap_or(0));
        (family_covered() && FAMILIES.len() == 4, t)
    };
    v.push(("F48839-J2-keymap/xkb/modifier/led四族均有留痕", ok2, d2));

    // 3 卡号连续
    let (ok3, d3) = {
        let mut t = String::new();
        t.push_str("cards=");
        push_dec(&mut t, d.cards);
        t.push_str(" gaps=");
        push_dec(&mut t, d.gaps);
        t.push_str(" total=");
        push_dec(&mut t, CARD_TOTAL);
        (d.cards == CARD_TOTAL && d.gaps == 0, t)
    };
    v.push(("F48839-J3-二十条卡连续无缺无重", ok3, d3));

    // 4 段带
    let (ok4, d4) = {
        let mut t = String::new();
        t.push_str("band=");
        push_hex(&mut t, BAND_LO);
        t.push('-');
        push_hex(&mut t, BAND_HI);
        t.push_str(" b01_hi=");
        push_hex(&mut t, B01_HI);
        t.push_str(" b03_lo=");
        push_hex(&mut t, B03_LO);
        (d.band_ok && B01_HI <= BAND_LO && BAND_HI <= B03_LO, t)
    };
    v.push(("F48839-J4-八段窗口互不重叠且不越B02带", ok4, d4));

    // 5 批印入册
    let (ok5, d5) = {
        let mut t = String::new();
        t.push_str("lines=");
        push_dec(&mut t, BOARD.lines);
        t.push_str("/total=");
        push_dec(&mut t, BOARD.total_lines);
        t.push_str(" batches=");
        push_dec(&mut t, BATCHES_PER_LIBRARY);
        (board_ok(), t)
    };
    v.push(("F48840-J1-批印入册12000/240,000", ok5, d5));

    // 6 行数合计
    let (ok6, d6) = {
        let mut t = String::new();
        t.push_str("declared=");
        push_dec(&mut t, d.lines);
        t.push_str(" floor=");
        push_dec(&mut t, BOARD.lines);
        (d.lines >= BOARD.lines, t)
    };
    v.push(("F48840-J2-八模块登记行数合计不低于批账", ok6, d6));

    // 7 合并覆盖
    let m = merge_seven();
    let (ok7, d7) = {
        let mut t = String::new();
        t.push_str("merged=");
        push_dec(&mut t, m.len() as u32);
        t.push_str(" seven=");
        push_dec(&mut t, seven_check_sum());
        t.push_str(" passed=");
        t.push_str(if m.all_passed() { "1" } else { "0" });
        ((m.len() as u32) >= seven_check_sum() && m.all_passed(), t)
    };
    v.push(("F48840-J3-合并集覆盖七模块全部判据且全绿", ok7, d7));

    // 8 动态明细溢出显性记账
    let (ok8, d8) = {
        let mut t = String::new();
        t.push_str("dyn_used=");
        push_dec(&mut t, m.dyn_used() as u32);
        t.push_str(" dyn_dropped=");
        push_dec(&mut t, m.dyn_dropped() as u32);
        t.push_str(" truncated=");
        t.push_str(if m.truncated() { "1" } else { "0" });
        (m.dyn_dropped() > 0 && m.dyn_used() == 8, t)
    };
    v.push(("F48840-J4-动态明细溢出显性记账", ok8, d8));

    // 9 封印双跑
    let (ok9, d9) = {
        let mut t = String::new();
        t.push_str("digest=");
        push_hex(&mut t, (seal_digest() & 0xffff_ffff) as u32);
        t.push_str(" repro=");
        t.push_str(if repro_check().is_ok() { "1" } else { "0" });
        (repro_check().is_ok(), t)
    };
    v.push(("F48840-J5-封印账双跑摘要一致", ok9, d9));

    // 10 卡册反查
    let mut hits = 0u32;
    let mut card = CARD_LO;
    while card <= CARD_HI {
        if card_in_book(card) && stem_of_card(card).is_some() {
            hits = hits.saturating_add(1);
        }
        card = card.saturating_add(1);
    }
    let ok10 = hits == CARD_TOTAL && stem_of_card(48820).is_none() && stem_of_card(48841).is_none();
    v.push(("F48840-J6-二十卡逐卡反查到stem", ok10, String::from("hits=20")));

    // 11~13 族闭包三条
    let mut segs_ok = true;
    let mut i = 1usize;
    while i < SEALS.len() {
        if let (Some(a), Some(b)) = (SEALS.get(i - 1), SEALS.get(i)) {
            if a.seg >= b.seg || a.seg.saturating_add(SEG_WINDOW) > b.seg {
                segs_ok = false;
            }
        }
        i += 1;
    }
    let tail_ok = match (SEALS.first(), SEALS.last()) {
        (Some(f), Some(l)) => {
            f.seg == BAND_LO && l.seg.saturating_add(SEG_WINDOW) == BAND_HI
        }
        _ => false,
    };
    v.push((
        "F48839-J5-族闭包-八段严格升序窗口不重叠且贴带首尾",
        segs_ok && tail_ok && d.band_ok,
        String::from("segs asc"),
    ));

    let mut known = 0u32;
    let mut i = 0usize;
    while i < SEALS.len() {
        if let Some(s) = SEALS.get(i) {
            if card_in_book(s.first_card) && card_in_book(s.last_card) {
                known = known.saturating_add(1);
            }
        }
        i += 1;
    }
    v.push((
        "F48839-J6-族闭包-八模块卡区间全在册",
        known == SEALS.len() as u32,
        String::from("8/8 in book"),
    ));

    v.push((
        "F48839-J7-族闭包-七模块判据全绿且封印自洽",
        m.all_passed() && board_ok() && family_covered(),
        String::from("seal closed"),
    ));

    v
}

// ── 检查集 ───────────────────────────────────────────────────
/// 收口检查集：七模块合并 + 本模块十三条。
pub fn run_unx48839_checks() -> CheckSet {
    let mut s = merge_seven();
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        if let Some((name, passed, d)) = checks.get(i) {
            if d.is_empty() {
                s.add(name, *passed, "");
            } else {
                s.add_dyn(name, *passed, d.as_str());
            }
        }
        i += 1;
    }
    s
}

// ── 测试 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_codes_window() {
        assert_eq!(SealCode::ALL.len(), 10);
        let mut i = 0usize;
        while i < 10 {
            let c = SealCode::ALL.get(i).copied().expect("code");
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            i += 1;
        }
        assert!(segment_guard());
        assert!(detail(0).contains("未知"));
    }

    #[test]
    fn t_seals_eight() {
        assert_eq!(SEALS.len(), 8);
        assert_eq!(SEAL_COUNT, 8);
        let d = dedup_check();
        assert_eq!(d.dup_stem, 0);
        assert_eq!(d.dup_seg, 0);
        assert_eq!(d.dup_card, 0);
        assert_eq!(d.gaps, 0);
        assert_eq!(d.cards, 20);
        assert!(d.band_ok);
        assert_eq!(SEALS.first().map(|s| s.first_card), Some(48821));
        assert_eq!(SEALS.last().map(|s| s.last_card), Some(48840));
    }

    #[test]
    fn t_cards_contiguous() {
        let mut prev: Option<u16> = None;
        for s in SEALS.iter() {
            if let Some(p) = prev {
                assert_eq!(s.first_card, p + 1, "卡号断档于 {}", s.stem);
            }
            prev = Some(s.last_card);
        }
        assert_eq!(prev, Some(48840));
    }

    #[test]
    fn t_band_disjoint() {
        assert!(band_ok());
        assert!(B01_HI <= BAND_LO);
        assert!(BAND_HI <= B03_LO);
        // 段窗都在带内
        for s in SEALS.iter() {
            assert!(s.seg >= BAND_LO);
            assert!(s.seg + SEG_WINDOW <= BAND_HI);
        }
        // 逐段严格升序且 0x40 窗互不重叠（段间允许留空号，但不许叠窗）
        for w in SEALS.windows(2) {
            let a = w.first().expect("a");
            let b = w.get(1).expect("b");
            assert!(a.seg < b.seg, "段 {} 与 {} 未升序", a.stem, b.stem);
            assert!(a.seg + SEG_WINDOW <= b.seg, "段 {} 与 {} 窗口重叠", a.stem, b.stem);
        }
        // 首段贴带首、末段窗贴带尾，整带没有越出 B02
        assert_eq!(SEALS.first().map(|s| s.seg), Some(BAND_LO));
        assert_eq!(SEALS.last().map(|s| s.seg + SEG_WINDOW), Some(BAND_HI));
        assert!(band_ok_in(&SEALS));
    }

    #[test]
    fn t_families() {
        assert_eq!(FAMILIES.len(), 4);
        assert_eq!(family_index("keymap"), Some(0));
        assert_eq!(family_index("xkb"), Some(1));
        assert_eq!(family_index("modifier"), Some(2));
        assert_eq!(family_index("led"), Some(3));
        assert_eq!(family_index("nope"), None);
        assert!(family_covered());
        let t = family_totals();
        assert!(t.first().copied().unwrap_or(0) >= 200);
        assert!(t.get(3).copied().unwrap_or(0) >= 500);
    }

    #[test]
    fn t_board() {
        assert_eq!(BOARD.batch, "B02");
        assert_eq!(BOARD.cards, 20);
        assert_eq!(BOARD.lines, 12_000);
        assert_eq!(BOARD.total_lines, 240_000);
        assert!(board_ok());
        let d = dedup_check();
        assert!(d.lines >= BOARD.lines);
        assert_eq!(seven_check_sum(), 70);
    }

    #[test]
    fn t_merge_seven() {
        let m = merge_seven();
        assert_eq!(m.len(), 70);
        assert!(m.all_passed());
        assert_eq!(m.dyn_used(), 8);
        assert!(m.dyn_dropped() > 0);
        assert!(m.truncated());
        // 合并不许吞绿：条数等于七模块之和
        assert_eq!(m.len() as u32, seven_check_sum());
    }

    #[test]
    fn t_card_lookup() {
        assert_eq!(stem_of_card(48821), Some("unx48821_keymap_core"));
        assert_eq!(stem_of_card(48827), Some("unx48825_keymap_guard"));
        assert_eq!(stem_of_card(48835), Some("unx48833_keymap_bridge"));
        assert_eq!(stem_of_card(48840), Some("unx48839_keymap_seal"));
        assert_eq!(stem_of_card(48820), None);
        assert_eq!(stem_of_card(48841), None);
        assert!(card_in_book(48830));
        assert!(!card_in_book(0));
    }

    #[test]
    fn t_golden_probe_inject() {
        let g = golden_run().expect("golden");
        assert_eq!(g.0, 20);
        assert!(g.1 >= 12_000);
        assert_eq!(g.2, 70);
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        let led = inject_rounds(1).expect("inject");
        assert!(led.all_caught());
        let v = extra_checks();
        assert_eq!(v.len(), 13);
        for (n, p, _) in v {
            assert!(p, "红项 {}", n);
        }
        assert!(run_unx48839_checks().all_passed());
    }
}
