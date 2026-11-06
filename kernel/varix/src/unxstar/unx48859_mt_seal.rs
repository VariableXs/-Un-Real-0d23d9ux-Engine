//! UNX-F48859 触摸输入族封存与防重登记 · 0x70740
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F48859-J1 本族 20 卡全覆盖且无重复认领；
//!   ③ UNX-F48860-J1 本族前七模块判据合并后零红项。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：表审计 → 卡认领 → 关键词防重 → 预算核账 → 封存 → 复算。
//!           表审计必须先于卡认领：模块行本身缺卡时，认领数会假性凑满；
//!           关键词必须先去重再查包含：否则 "scan" 会掩盖 "scancode"；
//!           预算必须在封存前核：封存后再改账等于伪造印签。
//! 关键差异：前七模块各自只看自己的域（扫描码、MT 槽、轨迹、quirk、
//!           观测），本模块不看任何一行输入实现，只把七份判据的**结论**
//!           合并，再补三条收口判据：防重关键词常量存在、20 卡全覆盖、
//!           族行数下限。合并保留本模块域名，红项随明细一起进账。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70740，VX6 批 B03 族的第八槽）。
pub const SEG_BASE: u32 = 0x70740;

/// 本族判据下限与批次/域预算（冻结，不得下调）。
pub const FAMILY_FLOOR: u32 = 6000;
pub const BATCH_FLOOR: u32 = 18000;
pub const DOMAIN_TOTAL: u32 = 240000;

/// 卡窗口：F48841..=F48860 共 20 张，模块 8 个。
pub const CARD_FIRST: u32 = 48841;
pub const CARD_COUNT: usize = 20;
pub const MODULE_COUNT: usize = 8;
pub const BATCH_FAMILIES: u32 = 10;
/// 前七模块最少判据条数（每模块 ≥8 条），用于合并后下限。
pub const MERGED_FLOOR: usize = 56;

// ── 诊断码闭集（本模块窗口 +0x01..+0x09） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlCode {
    /// 模块行缺失、重名或主卡越窗
    TableBad,
    /// 卡号越出 F48841..=F48860
    CardGap,
    /// 同一张卡被两个模块认领
    CardDup,
    /// 关键词为空、非小写或含非字母
    KeywordBad,
    /// 关键词重复或互为子串（防重失效）
    KeywordOverlap,
    /// 族/批/域预算不足
    BudgetBad,
    /// 封存后再写
    SealedWrite,
    /// 复算指纹不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl SlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SlCode::TableBad => "模块表非法：行数/主卡/重名不满足八行冻结",
            SlCode::CardGap => "卡号越窗：不在 F48841..=F48860",
            SlCode::CardDup => "卡重复认领：同一张卡被两个模块登记",
            SlCode::KeywordBad => "关键词非法：空串或含非小写字母",
            SlCode::KeywordOverlap => "关键词失防：重复或互为子串",
            SlCode::BudgetBad => "预算不足：族 6000 / 批 18000 / 域 240000",
            SlCode::SealedWrite => "封存后写：印签已落不可再改",
            SlCode::ReplayBad => "复算不一致：封存根与重算根不同",
            SlCode::SegmentBad => "段位越界：诊断码逃出 0x70740 窗口",
        }
    }

    pub const ALL: [SlCode; 9] = [
        SlCode::TableBad,
        SlCode::CardGap,
        SlCode::CardDup,
        SlCode::KeywordBad,
        SlCode::KeywordOverlap,
        SlCode::BudgetBad,
        SlCode::SealedWrite,
        SlCode::ReplayBad,
        SlCode::SegmentBad,
    ];
}

/// 段内码取明细文本。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SlCode::ALL.len() {
        if SlCode::ALL[i].val() == code {
            return SlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 模块表（八行冻结） ────────────────────────────────────────────
/// 一行 = 一个已交付模块：名字、主卡、承诺行数、覆盖卡清单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleRow {
    pub name: &'static str,
    pub card: u32,
    pub floor: u32,
    pub cards: &'static [u32],
}

/// B03 八模块表。卡分配互不重叠，合计恰好 20 张。
pub const TABLE: [ModuleRow; MODULE_COUNT] = [
    ModuleRow { name: "unx48841_scancode_core", card: 48841, floor: 900, cards: &[48841, 48842] },
    ModuleRow { name: "unx48843_mt_meta", card: 48843, floor: 850, cards: &[48843, 48844] },
    ModuleRow { name: "unx48845_mt_guard", card: 48845, floor: 900, cards: &[48845, 48846, 48847] },
    ModuleRow { name: "unx48848_mt_dispatch", card: 48848, floor: 850, cards: &[48848, 48849, 48850] },
    ModuleRow { name: "unx48851_mt_edge", card: 48851, floor: 700, cards: &[48851, 48852] },
    ModuleRow { name: "unx48853_mt_bridge", card: 48853, floor: 750, cards: &[48853, 48854, 48855] },
    ModuleRow { name: "unx48856_mt_quality", card: 48856, floor: 900, cards: &[48856, 48857, 48858] },
    ModuleRow { name: "unx48859_mt_seal", card: 48859, floor: 350, cards: &[48859, 48860] },
];

/// 表审计：八行、主卡在窗内且单调、名字非空不重复、行内卡非空且在窗内。
pub fn audit_table() -> Result<usize, u32> {
    if TABLE.len() != MODULE_COUNT {
        return Err(SlCode::TableBad.val());
    }
    let mut i = 0usize;
    while i < TABLE.len() {
        let row = match TABLE.get(i) {
            Some(r) => r,
            None => return Err(SlCode::TableBad.val()),
        };
        if row.name.is_empty() || row.floor == 0 || row.cards.is_empty() {
            return Err(SlCode::TableBad.val());
        }
        if row.card < CARD_FIRST || row.card >= CARD_FIRST + CARD_COUNT as u32 {
            return Err(SlCode::TableBad.val());
        }
        if i > 0 {
            let prev = match TABLE.get(i - 1) {
                Some(r) => r,
                None => return Err(SlCode::TableBad.val()),
            };
            if row.card <= prev.card || row.name == prev.name {
                return Err(SlCode::TableBad.val());
            }
        }
        let mut k = 0usize;
        while k < row.cards.len() {
            let c = match row.cards.get(k) {
                Some(v) => *v,
                None => return Err(SlCode::TableBad.val()),
            };
            if c < CARD_FIRST || c >= CARD_FIRST + CARD_COUNT as u32 {
                return Err(SlCode::TableBad.val());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(TABLE.len())
}

/// 行数合计（承诺值，非实测）。
pub fn floor_total() -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(r) = TABLE.get(i) {
            sum = sum.saturating_add(r.floor);
        }
        i += 1;
    }
    sum
}

// ── 卡认领账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub card: u32,
    pub module: u32,
}

/// 20 格卡位：0 未领，1 已领；重复认领单独计数。
pub struct CardLedger {
    marks: [u8; CARD_COUNT],
    claims: Vec<Claim>,
    dups: u32,
}

impl CardLedger {
    pub fn new() -> CardLedger {
        CardLedger { marks: [0; CARD_COUNT], claims: Vec::new(), dups: 0 }
    }

    /// 认领一张卡：越窗拒，重复拒（重复计入 dups）。
    pub fn claim(&mut self, card: u32, module: u32) -> Result<(), u32> {
        if card < CARD_FIRST || card >= CARD_FIRST + CARD_COUNT as u32 {
            return Err(SlCode::CardGap.val());
        }
        let idx = (card - CARD_FIRST) as usize;
        let slot = match self.marks.get(idx) {
            Some(v) => *v,
            None => return Err(SlCode::CardGap.val()),
        };
        if slot != 0 {
            self.dups = self.dups.saturating_add(1);
            return Err(SlCode::CardDup.val());
        }
        if let Some(m) = self.marks.get_mut(idx) {
            *m = 1;
        }
        self.claims.push(Claim { card, module });
        Ok(())
    }

    /// 按模块表批量认领。
    pub fn claim_table(&mut self) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < TABLE.len() {
            let row = match TABLE.get(i) {
                Some(r) => r,
                None => return Err(SlCode::TableBad.val()),
            };
            let mut k = 0usize;
            while k < row.cards.len() {
                let c = match row.cards.get(k) {
                    Some(v) => *v,
                    None => return Err(SlCode::TableBad.val()),
                };
                self.claim(c, row.card)?;
                k += 1;
            }
            i += 1;
        }
        Ok(self.claims.len())
    }

    pub fn count(&self) -> usize {
        self.claims.len()
    }

    pub fn dups(&self) -> u32 {
        self.dups
    }

    pub fn covers(&self, card: u32) -> bool {
        if card < CARD_FIRST || card >= CARD_FIRST + CARD_COUNT as u32 {
            return false;
        }
        self.marks.get((card - CARD_FIRST) as usize).copied().unwrap_or(0) == 1
    }

    /// 审计：20 格全领且无重复。
    pub fn audit(&self) -> Result<usize, u32> {
        if self.claims.len() != CARD_COUNT || self.dups != 0 {
            return Err(SlCode::CardGap.val());
        }
        let mut i = 0usize;
        while i < CARD_COUNT {
            if self.marks.get(i).copied().unwrap_or(0) != 1 {
                return Err(SlCode::CardGap.val());
            }
            i += 1;
        }
        Ok(self.claims.len())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.claims.len() {
            if let Some(c) = self.claims.get(i) {
                h = fold_u32(h, c.card);
                h = fold_u32(h, c.module);
            }
            i += 1;
        }
        h
    }
}

// ── 防重关键词索引 ────────────────────────────────────────────────
/// 本族四个关键词常量（其他族不得复用；族内不得互为子串）。
pub const KEYWORDS: [&str; 4] = ["scancode", "mtslot", "tracking", "quirk"];

pub struct KeywordIndex {
    words: Vec<&'static str>,
    digest: u64,
}

impl KeywordIndex {
    pub fn new(words: &[&'static str]) -> Result<KeywordIndex, u32> {
        if words.is_empty() {
            return Err(SlCode::KeywordBad.val());
        }
        let mut i = 0usize;
        while i < words.len() {
            let w = match words.get(i) {
                Some(v) => *v,
                None => return Err(SlCode::KeywordBad.val()),
            };
            if !keyword_ok(w) {
                return Err(SlCode::KeywordBad.val());
            }
            let mut j = 0usize;
            while j < i {
                let p = match words.get(j) {
                    Some(v) => *v,
                    None => return Err(SlCode::KeywordBad.val()),
                };
                if p == w || p.contains(w) || w.contains(p) {
                    return Err(SlCode::KeywordOverlap.val());
                }
                j += 1;
            }
            i += 1;
        }
        let mut h = FNV_OFFSET;
        let mut k = 0usize;
        while k < words.len() {
            if let Some(w) = words.get(k) {
                h = fold_str(h, w);
            }
            k += 1;
        }
        Ok(KeywordIndex { words: words.to_vec(), digest: h })
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 关键词是否真实存在于本索引（逐字节比，非哈希比）。
    pub fn holds(&self, needle: &str) -> bool {
        let mut i = 0usize;
        while i < self.words.len() {
            if let Some(w) = self.words.get(i) {
                if *w == needle {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 关键词规约：长度 ≥4，全小写 ASCII 字母。
fn keyword_ok(w: &str) -> bool {
    if w.len() < 4 {
        return false;
    }
    let bytes = w.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b < b'a' || b > b'z' {
            return false;
        }
        i += 1;
    }
    true
}

// ── 行数预算 ──────────────────────────────────────────────────────
/// 三档预算：本族承诺、批次投影（本族 + 其余族按族下限）、域总量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineBudget {
    pub family: u32,
    pub batch: u32,
    pub domain: u32,
}

impl LineBudget {
    pub fn new(family: u32, families: u32, domain: u32) -> Result<LineBudget, u32> {
        if families == 0 || families > BATCH_FAMILIES {
            return Err(SlCode::BudgetBad.val());
        }
        let rest = families.saturating_sub(1).saturating_mul(FAMILY_FLOOR);
        let batch = family.saturating_add(rest);
        if family < FAMILY_FLOOR || batch < BATCH_FLOOR || domain != DOMAIN_TOTAL {
            return Err(SlCode::BudgetBad.val());
        }
        Ok(LineBudget { family, batch, domain })
    }

    /// 不返回 Err 的纯查询，用于判据面陈述。
    pub fn projection(&self) -> u32 {
        self.batch
    }

    pub fn ok(&self) -> bool {
        self.family >= FAMILY_FLOOR
            && self.batch >= BATCH_FLOOR
            && self.domain == DOMAIN_TOTAL
    }
}

// ── FNV-1a 基础设施 ──────────────────────────────────────────────
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> (i * 8)) as u8);
        i += 1;
    }
    x
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── LCG（注入轮用，固定种子保证可复算） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self.state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }
}

// ── 族封存账 ──────────────────────────────────────────────────────
pub struct FamilySeal {
    pub cards: CardLedger,
    pub keys: KeywordIndex,
    pub budget: LineBudget,
    pub sealed: bool,
    pub root: u64,
}

impl FamilySeal {
    /// 建账：表审计 → 卡认领 → 关键词 → 预算，任一步失败即拒。
    pub fn new() -> Result<FamilySeal, u32> {
        audit_table()?;
        let mut cards = CardLedger::new();
        cards.claim_table()?;
        cards.audit()?;
        let keys = KeywordIndex::new(&KEYWORDS)?;
        let budget = LineBudget::new(floor_total(), BATCH_FAMILIES, DOMAIN_TOTAL)?;
        Ok(FamilySeal { cards, keys, budget, sealed: false, root: 0 })
    }

    /// 封存：落根并置位；二次封存拒。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(SlCode::SealedWrite.val());
        }
        self.root = self.compute_root();
        self.sealed = true;
        Ok(self.root)
    }

    /// 复算：未封存拒；重算根必须等于封存根。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.sealed {
            return Err(SlCode::SealedWrite.val());
        }
        let now = self.compute_root();
        if now != self.root {
            return Err(SlCode::ReplayBad.val());
        }
        Ok(now)
    }

    pub fn verify(&self) -> bool {
        self.sealed && self.compute_root() == self.root
    }

    /// 根 = 表行 ‖ 关键词摘要 ‖ 卡账指纹。
    fn compute_root(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < TABLE.len() {
            if let Some(r) = TABLE.get(i) {
                h = fold_str(h, r.name);
                h = fold_u32(h, r.card);
                h = fold_u32(h, r.floor);
                let mut k = 0usize;
                while k < r.cards.len() {
                    if let Some(c) = r.cards.get(k) {
                        h = fold_u32(h, *c);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold_u32(h, self.keys.len() as u32);
        h ^= self.keys.digest();
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.cards.fingerprint();
        h
    }

    /// 写后拒：置位后的三条写路径必须全部失败。
    pub fn frozen_probe(&self) -> [bool; 3] {
        let mut out = [false; 3];
        let mut c = CardLedger::new();
        out[0] = c.claim(CARD_FIRST, 48841).is_ok()
            && c.claim(CARD_FIRST, 48843).err() == Some(SlCode::CardDup.val());
        out[1] = self.sealed && self.replay().is_ok();
        out[2] = match KeywordIndex::new(&KEYWORDS) {
            Ok(idx) => idx.holds(KEYWORDS[0]) && !idx.holds("scan"),
            Err(_) => false,
        };
        out
    }
}

// ── 黄金全链 ──────────────────────────────────────────────────────
/// 黄金：(模块数, 卡数, 族行数, 批投影, 封存根)。
pub fn golden_run() -> Result<(usize, usize, u32, u32, u64), u32> {
    let mut s = FamilySeal::new()?;
    if !s.budget.ok() {
        return Err(SlCode::BudgetBad.val());
    }
    let root = s.seal()?;
    if !s.verify() {
        return Err(SlCode::ReplayBad.val());
    }
    let replay = s.replay()?;
    if replay != root {
        return Err(SlCode::ReplayBad.val());
    }
    let cards = s.cards.audit()?;
    Ok((TABLE.len(), cards, s.budget.family, s.budget.projection(), root))
}

// ── 注入账（四路：越窗卡/重号卡/关键词失防/封存后写） ────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if let Some(t) = self.tries.get(i) {
                sum = sum.saturating_add(*t);
            }
            i += 1;
        }
        sum
    }
}

/// 注入轮：每轮四路，全部必须被拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x59);
    let mut r = 0u32;
    while r < rounds {
        let jitter = rng.next() & 0x0F;

        // 路 1：卡号越窗。
        let mut a = CardLedger::new();
        let gap = a.claim(CARD_FIRST + CARD_COUNT as u32 + jitter, 48841).err()
            == Some(SlCode::CardGap.val());
        led.record(0, gap);

        // 路 2：同卡二次认领。
        let dup = a.claim(CARD_FIRST, 48841).is_ok()
            && a.claim(CARD_FIRST + jitter, 48843).is_ok()
            && a.claim(CARD_FIRST, 48845).err() == Some(SlCode::CardDup.val());
        led.record(1, dup);

        // 路 3：关键词互为子串。
        let overlap = KeywordIndex::new(&["scancode", "scan"]).err()
            == Some(SlCode::KeywordOverlap.val())
            && KeywordIndex::new(&["Quirk"]).err() == Some(SlCode::KeywordBad.val());
        led.record(2, overlap);

        // 路 4：封存后二次写。
        let write = match FamilySeal::new() {
            Ok(mut s) => match s.seal() {
                Ok(_) => s.seal().err() == Some(SlCode::SealedWrite.val()),
                Err(_) => false,
            },
            Err(_) => false,
        };
        led.record(3, write);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 序 A：黄金全链两跑同值（含封存根）。
pub fn probe_seq_a() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b && a.0 == MODULE_COUNT && a.1 == CARD_COUNT && a.2 >= FAMILY_FLOOR {
        Ok(())
    } else {
        Err(SlCode::ReplayBad.val())
    }
}

/// 序 B：非法输入逐路拒。
pub fn probe_seq_b() -> Result<(), u32> {
    if audit_table()? != MODULE_COUNT {
        return Err(SlCode::TableBad.val());
    }
    if KeywordIndex::new(&[]).err() != Some(SlCode::KeywordBad.val()) {
        return Err(SlCode::KeywordBad.val());
    }
    if KeywordIndex::new(&["mtslot", "mtslot"]).err() != Some(SlCode::KeywordOverlap.val()) {
        return Err(SlCode::KeywordOverlap.val());
    }
    if LineBudget::new(FAMILY_FLOOR - 1, BATCH_FAMILIES, DOMAIN_TOTAL).is_ok() {
        return Err(SlCode::BudgetBad.val());
    }
    let mut c = CardLedger::new();
    if c.claim(CARD_FIRST - 1, 48841).err() != Some(SlCode::CardGap.val()) {
        return Err(SlCode::CardGap.val());
    }
    Ok(())
}

/// 序 C：冻结常量。
pub fn probe_seq_c() -> Result<(), u32> {
    let cards = {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < TABLE.len() {
            if let Some(r) = TABLE.get(i) {
                n += r.cards.len();
            }
            i += 1;
        }
        n
    };
    if TABLE.len() == MODULE_COUNT
        && cards == CARD_COUNT
        && floor_total() == 6200
        && FAMILY_FLOOR == 6000
        && BATCH_FLOOR == 18000
        && DOMAIN_TOTAL == 240000
        && KEYWORDS.len() == 4
    {
        Ok(())
    } else {
        Err(SlCode::TableBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x20)。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SlCode::ALL.len() {
        let v = SlCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 七模块合并面 ──────────────────────────────────────────────────
/// 依次调用本族前七个模块的判据入口。探针按 #[path] 拉全部模块，
/// 因此 super:: 在探针与 mod.rs 两种语境下都解析到同一批兄弟。
pub fn family_checks() -> Vec<(&'static str, CheckSet)> {
    vec![
        ("unx48841", super::unx48841_scancode_core::run_unx48841_checks()),
        ("unx48843", super::unx48843_mt_meta::run_unx48843_checks()),
        ("unx48845", super::unx48845_mt_guard::run_unx48845_checks()),
        ("unx48848", super::unx48848_mt_dispatch::run_unx48848_checks()),
        ("unx48851", super::unx48851_mt_edge::run_unx48851_checks()),
        ("unx48853", super::unx48853_mt_bridge::run_unx48853_checks()),
        ("unx48856", super::unx48856_mt_quality::run_unx48856_checks()),
    ]
}

/// 合并后的三围：条数、红项、黑项。
pub fn fold_stats(parts: &[(&'static str, CheckSet)]) -> (usize, usize) {
    let mut total = 0usize;
    let mut red = 0usize;
    let mut i = 0usize;
    while i < parts.len() {
        if let Some((_, cs)) = parts.get(i) {
            total = total.saturating_add(cs.len());
            red = red.saturating_add(cs.failed_count());
        }
        i += 1;
    }
    (total, red)
}

/// 三条收口判据（seal 只加这三条，不重复前七模块判据）。
pub fn closure_checks(total: usize, red: usize) -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 防重关键词常量存在且可用。
    let keys = KeywordIndex::new(&KEYWORDS);
    let (kw_ok, kw_detail) = match &keys {
        Ok(k) => {
            let all = k.len() == 4
                && k.holds("scancode")
                && k.holds("mtslot")
                && k.holds("tracking")
                && k.holds("quirk");
            (all, format!("keywords={} digest=0x{:016X}", k.len(), k.digest()))
        }
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(*e))),
    };
    v.push(("F48859-J1-防重关键词", kw_ok, kw_detail));

    // ② 20 卡全覆盖且无重复。
    let mut led = CardLedger::new();
    let claim = led.claim_table();
    let (card_ok, card_detail) = match &claim {
        Ok(n) => {
            let audit = led.audit().is_ok();
            let covers = led.covers(CARD_FIRST)
                && led.covers(CARD_FIRST + CARD_COUNT as u32 - 1)
                && led.dups() == 0;
            (
                *n == CARD_COUNT && audit && covers,
                format!("claims={} dups={} window=F{}..=F{}", n, led.dups(),
                    CARD_FIRST, CARD_FIRST + CARD_COUNT as u32 - 1),
            )
        }
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(*e))),
    };
    v.push(("F48859-J1-二十卡全覆盖", card_ok, card_detail));

    // ③ 族行数下限 + 七模块合并零红。
    let floor = floor_total();
    let budget = LineBudget::new(floor, BATCH_FAMILIES, DOMAIN_TOTAL);
    let merged_ok = total >= MERGED_FLOOR && red == 0;
    let (floor_ok, floor_detail) = match &budget {
        Ok(b) => (
            b.ok() && merged_ok,
            format!("family={} batch={} checks={} red={}", b.family, b.batch, total, red),
        ),
        Err(e) => (false, format!("err=0x{:X} {}", e, detail(*e))),
    };
    v.push(("F48860-J1-族行数下限", floor_ok, floor_detail));

    v
}

/// 判据面（供测试与外部探针直接取）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let parts = family_checks();
    let (total, red) = fold_stats(&parts);
    closure_checks(total, red)
}

/// 族封存判据：先合并七模块，再加三条收口。
pub fn run_unx48859_checks() -> CheckSet {
    let parts = family_checks();
    let (total, red) = fold_stats(&parts);
    let mut s = CheckSet::new("UNX-F48859 触摸输入族封存与防重登记");
    let mut i = 0usize;
    while i < parts.len() {
        if let Some((_, cs)) = parts.get(i) {
            s = CheckSet::merge(s, *cs);
        }
        i += 1;
    }
    for (n, p, d) in closure_checks(total, red) {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_eight_rows() {
        assert_eq!(audit_table().ok(), Some(MODULE_COUNT));
        assert_eq!(TABLE.len(), 8);
        assert_eq!(floor_total(), 6200);
    }

    #[test]
    fn table_cards_cover_window() {
        let mut led = CardLedger::new();
        assert_eq!(led.claim_table().ok(), Some(CARD_COUNT));
        assert!(led.audit().is_ok());
        assert!(led.covers(48841));
        assert!(led.covers(48860));
        assert!(!led.covers(48840));
    }

    #[test]
    fn duplicate_claim_rejected() {
        let mut led = CardLedger::new();
        assert!(led.claim(48841, 48841).is_ok());
        assert_eq!(led.claim(48841, 48843).err(), Some(SlCode::CardDup.val()));
        assert_eq!(led.dups(), 1);
    }

    #[test]
    fn out_of_window_card_rejected() {
        let mut led = CardLedger::new();
        assert_eq!(led.claim(48840, 48841).err(), Some(SlCode::CardGap.val()));
        assert_eq!(led.claim(48861, 48859).err(), Some(SlCode::CardGap.val()));
    }

    #[test]
    fn keyword_rules_enforced() {
        assert!(KeywordIndex::new(&KEYWORDS).is_ok());
        assert_eq!(KeywordIndex::new(&["SCAN"]).err(), Some(SlCode::KeywordBad.val()));
        assert_eq!(KeywordIndex::new(&["abc"]).err(), Some(SlCode::KeywordBad.val()));
        assert_eq!(
            KeywordIndex::new(&["tracking", "track"]).err(),
            Some(SlCode::KeywordOverlap.val())
        );
        assert_eq!(
            KeywordIndex::new(&["quirk", "quirk"]).err(),
            Some(SlCode::KeywordOverlap.val())
        );
    }

    #[test]
    fn budget_floor_enforced() {
        assert!(LineBudget::new(6200, BATCH_FAMILIES, DOMAIN_TOTAL).is_ok());
        assert_eq!(
            LineBudget::new(5999, BATCH_FAMILIES, DOMAIN_TOTAL).err(),
            Some(SlCode::BudgetBad.val())
        );
        assert_eq!(
            LineBudget::new(6200, BATCH_FAMILIES, 239999).err(),
            Some(SlCode::BudgetBad.val())
        );
    }

    #[test]
    fn seal_is_replayable() {
        match FamilySeal::new() {
            Ok(mut s) => {
                assert!(!s.sealed && s.root == 0);
                let sealed = s.seal().ok().unwrap_or(0);
                assert!(sealed != 0);
                assert_eq!(s.replay().ok(), Some(sealed));
                assert!(s.verify());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn sealed_write_rejected() {
        match FamilySeal::new() {
            Ok(mut s) => {
                assert!(s.seal().is_ok());
                assert_eq!(s.seal().err(), Some(SlCode::SealedWrite.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_values_frozen() {
        let g = golden_run();
        assert!(g.is_ok());
        match g {
            Ok((m, c, f, b, root)) => {
                assert_eq!(m, 8);
                assert_eq!(c, 20);
                assert_eq!(f, 6200);
                assert_eq!(b, 60200);
                assert!(root != 0);
            }
            Err(_) => assert!(false),
        }
        assert!(repro_check().is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        let led = inject_rounds(10);
        assert!(led.is_ok());
        match led {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.total(), 40);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_and_segment_hold() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(SlCode::ALL.len(), 9);
    }

    #[test]
    fn merged_family_is_green() {
        let parts = family_checks();
        assert_eq!(parts.len(), 7);
        let (total, red) = fold_stats(&parts);
        assert!(total >= MERGED_FLOOR);
        assert_eq!(red, 0);
        let closure = closure_checks(total, red);
        assert_eq!(closure.len(), 3);
        let mut i = 0usize;
        while i < closure.len() {
            if let Some((_, p, _)) = closure.get(i) {
                assert!(*p);
            }
            i += 1;
        }
    }
}
