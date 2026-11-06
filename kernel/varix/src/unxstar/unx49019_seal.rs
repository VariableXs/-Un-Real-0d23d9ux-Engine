//! UNX-F49019 B11 批防重 grep + UNX-F49020 B11 批收官印 · 0x71B00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-D 末槽；卡面 J1 引用按原文保留）：
//!   UNX-F49019 B11 批防重 grep（关键词族：quirk/config/profile）:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49020-J1 批印入册：66000/240,000；判据锚定 §AI-62 §7；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49020 B11 批收官印（20 条冻结/域账累计 66 000 行）:
//!          ① 纯功能代码 ≥420 行；
//!          ② 判据原文：UNX-F49021-J1 104 键逐键全绿（判据 J-4 载体）；判据锚定 §AI-62 §5 J-4；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 说明：F49019 卡面引用 UNX-F49020-J1、F49020 卡面引用 UNX-F49021-J1（板上
//! 自身的 +1 引用）。此处逐字保留，不改号，便于任务板锚定逐字回查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（防重 grep）：关键词族自洽 → 令牌归属唯一 → 卡册结构 → 单卡命中 →
//! 主族判定 → 撞号统计 → 摘要。
//! 族先于令牌：quirk/config/profile 三族是先验分类，令牌必须先被恰一族认领。
//! 族表都没对齐时，后面的「命中」只是在三张互相重叠的词表里随便挑一个。
//! 令牌归属先于卡册：同一令牌若被两族同时认领，一张卡的主族就取决于族表
//! 顺序而不是卡的内容，这正是「撞号」的根源，必须先证明归属唯一。
//! 卡册结构先于单卡命中：编号/批次/配额是卡的身份。身份重复时，同一个编号
//! 会被两条卡各自 grep 出结果，账本看起来两倍丰满，实际只冻结了一条。
//! 单卡命中先于主族：每张卡至少要命中一个关键词，否则「该卡属于哪一族」
//! 无从谈起。零命中的卡是防重网的洞，不是「归到默认族」的候选。
//! 主族判定先于撞号统计：主族按「命中数严格最大且唯一」判定，出现并列即
//! 撞号。先统计后判定，并列会被最大值的先到者吃掉，撞号就静默了。
//! 摘要后于全部结构：摘要只回答「同一批卡算出来的指纹是否稳定」，它不能
//! 替代结构检查；先算摘要会把结构病折进一个漂亮但不解释任何东西的十六进制。
//! 闸门次序（收官印）：冻结条数 → 批次配额 → 域账合计 → 封印摘要 → 复跑。
//! 冻结条数先于配额：20 条是「批收口」的硬门槛，少一条时配额合计仍可能凑出
//! 目标数，先钉条数才不会被「数字对了、卡少了」骗过。
//! 批次配额先于域账合计：五个子批各 4 条是冻结的解剖结构。批次不齐时，
//! 合计 72 000 行只是五段里某一段超发的结果，解剖结构比总数更难伪造。
//! 域账合计后于配额：总行数由「每卡卡面配额 × 12」逐卡累加，再与批合计
//! 交叉验证。先有合计再回填配额，等于让总数反过来定义每张卡的账。
//! 封印摘要后于域账：封印把「20 个编号 + 5 段批账 + 3 族主归属」折成一位
//! 指纹，只有前面的账都成立，指纹才有封印意义。
//! 复跑后于封印：三遍摘要逐位一致才算「印」可复现；只算一遍的封印是一次
//! 性的观测，不是可回查的印。
//! 关键差异：本模块只建模 B11 批的记账与防重（关键词族 / 卡册 / 行数配额 /
//! 封印摘要），不声称任务板已写回、不引用任何外部状态、不改写卡面原文。
//! 卡面 J1 原文里的「66000/240,000」按逐字锚定保留在常量 CARD_SEAL_TEXT 中；
//! 本模块自己的域账口径是「20 卡配额合计 6 000 × 12 = 72 000 行」，两者是
//! 不同记账面，互不替代。观测埋点（0x71A80）、用例集（0x71AC0）、文案走查
//! （0x71A40）都在各自槽位，本模块不重算它们。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71B00，VX6 批 B11-D 末槽）。
pub const SEG_BASE: u32 = 0x71B00;

/// 卡册条数（B11 批 20 条）。
pub const CARDS_TOTAL: usize = 20;
/// 子批数（A..E）。
pub const BATCHES: usize = 5;
/// 每子批条数。
pub const CARDS_PER_BATCH: usize = 4;
/// 关键词族数。
pub const FAMILIES: usize = 3;
/// 每族令牌数。
pub const TOKENS_PER_FAMILY: usize = 6;
/// 令牌总数（3 × 6，必须两两不同）。
pub const TOKENS_TOTAL: usize = 18;
/// 每卡关键词数。
pub const CARD_KEYWORDS: usize = 3;
/// 行数放大系数：卡面配额 → 域账行数。
pub const LINE_MULTIPLIER: u32 = 12;
/// 单卡配额下界。
pub const CARD_QUOTA_MIN: u32 = 200;
/// 单卡配额上界。
pub const CARD_QUOTA_MAX: u32 = 420;
/// 域账目标总行数（20 卡合计 6 000 × 12）。
pub const DOMAIN_TOTAL_LINES: u32 = 72_000;
/// 卡面配额合计（逐卡卡面行数之和）。
pub const QUOTA_TOTAL: u32 = 6_000;
/// 首卡编号。
pub const CARD_ID_FIRST: u16 = 49_001;
/// 封印复跑遍数。
pub const REPLAY_PASSES: u32 = 3;
/// 注入路数。
pub const INJECT_ROUTES: u32 = 5;
/// 注入轮数上界。
pub const INJECT_MAX: u32 = 10_000;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;

/// 卡面 J1 原文（逐字保留，不由本模块改写）。
pub const CARD_SEAL_TEXT: &str = "UNX-F49020-J1 批印入册：66000/240,000";

/// 子批名。
pub const BATCH_NAMES: [&str; BATCHES] = ["B11-A", "B11-B", "B11-C", "B11-D", "B11-E"];

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 卡册条数不是 20
    CountBad,
    /// 批次号/批次条数非法
    BatchBad,
    /// 卡面配额为零或越界
    QuotaBad,
    /// slug 形状非法或重复
    SlugBad,
    /// 卡编号重复
    DupBad,
    /// 关键词令牌无族认领或为空
    KeywordBad,
    /// 主族并列（撞号）或令牌被两族认领
    CollisionBad,
    /// 域账行数不守恒
    LedgerBad,
    /// 封印摘要漂移
    ReplayBad,
    /// 渲染行不可解析
    RenderBad,
    /// 注入轮数越界
    CoverBad,
    /// 冻结条数或批次解剖不符
    FreezeBad,
    /// 诊断码越出本模块段窗
    SegmentBad,
    /// 单卡域账行数 ≠ 配额 × 12
    SealBad,
}

impl SealCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            SealCode::CountBad => "卡册条数不是 20",
            SealCode::BatchBad => "批次号越出 0..4 或批次条数不是 4",
            SealCode::QuotaBad => "卡面配额为零或越出 200..=420",
            SealCode::SlugBad => "slug 不是小写 snake_case 或两条卡 slug 重复",
            SealCode::DupBad => "两张卡共用同一编号",
            SealCode::KeywordBad => "关键词为空或没有任何族认领",
            SealCode::CollisionBad => "主族并列撞号，或同一令牌被两族认领",
            SealCode::LedgerBad => "域账合计 ≠ 各批之和或 ≠ 配额 × 12",
            SealCode::ReplayBad => "封印摘要复跑漂移",
            SealCode::RenderBad => "渲染行字段缺失或回读不一致",
            SealCode::CoverBad => "注入轮数为零或越过上限",
            SealCode::FreezeBad => "冻结条数不是 20 或批次解剖不符",
            SealCode::SegmentBad => "诊断码越出本模块段窗",
            SealCode::SealBad => "单卡域账行数 ≠ 配额 × 12",
        }
    }

    pub const ALL: [SealCode; 14] = [
        SealCode::CountBad,
        SealCode::BatchBad,
        SealCode::QuotaBad,
        SealCode::SlugBad,
        SealCode::DupBad,
        SealCode::KeywordBad,
        SealCode::CollisionBad,
        SealCode::LedgerBad,
        SealCode::ReplayBad,
        SealCode::RenderBad,
        SealCode::CoverBad,
        SealCode::FreezeBad,
        SealCode::SegmentBad,
        SealCode::SealBad,
    ];
}

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
    "未知诊断码"
}

// ── 关键词族 ──────────────────────────────────────────────────────
/// 一族关键词：族 id + 人读标题 + 6 个令牌。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeywordFamily {
    pub id: &'static str,
    pub title: &'static str,
    pub tokens: [&'static str; TOKENS_PER_FAMILY],
}

pub const KEYWORD_FAMILIES: [KeywordFamily; FAMILIES] = [
    KeywordFamily {
        id: "quirk",
        title: "quirk 表",
        tokens: ["quirk", "表项", "命中", "降级", "匹配", "固件"],
    },
    KeywordFamily {
        id: "config",
        title: "配置面",
        tokens: ["config", "配置", "持久化", "热生效", "隔离", "校验和"],
    },
    KeywordFamily {
        id: "profile",
        title: "机型档案",
        tokens: ["profile", "档案", "机型", "清单", "承诺", "档位"],
    },
];

/// 令牌 -> 族号（族表自洽时唯一）。
pub fn token_owner(token: &str) -> Option<u8> {
    let mut f = 0usize;
    while f < KEYWORD_FAMILIES.len() {
        if let Some(fam) = KEYWORD_FAMILIES.get(f) {
            let mut t = 0usize;
            while t < fam.tokens.len() {
                if let Some(tok) = fam.tokens.get(t) {
                    if *tok == token {
                        return Some(f as u8);
                    }
                }
                t += 1;
            }
        }
        f += 1;
    }
    None
}

/// 族表自审：族 id 唯一非空 → 令牌非空 → 18 个令牌两两不同 → 每条都能回查。
pub fn family_integrity() -> Result<u64, u32> {
    let mut fp = p_core::FNV_OFF;
    let mut f = 0usize;
    while f < KEYWORD_FAMILIES.len() {
        let fam = match KEYWORD_FAMILIES.get(f) {
            Some(v) => *v,
            None => return Err(SealCode::KeywordBad.val()),
        };
        if fam.id.is_empty() || fam.title.is_empty() {
            return Err(SealCode::KeywordBad.val());
        }
        let mut t = 0usize;
        while t < fam.tokens.len() {
            let tok = match fam.tokens.get(t) {
                Some(v) => *v,
                None => return Err(SealCode::KeywordBad.val()),
            };
            if tok.is_empty() {
                return Err(SealCode::KeywordBad.val());
            }
            // 令牌必须恰好被本族认领（回查编号一致）。
            if token_owner(tok) != Some(f as u8) {
                return Err(SealCode::CollisionBad.val());
            }
            // 与本族前面的令牌不同。
            let mut k = 0usize;
            while k < t {
                if let Some(prev) = fam.tokens.get(k) {
                    if *prev == tok {
                        return Err(SealCode::CollisionBad.val());
                    }
                }
                k += 1;
            }
            // 与前面所有族的令牌不同（跨族撞词即为撞号）。
            let mut g = 0usize;
            while g < f {
                if let Some(other) = KEYWORD_FAMILIES.get(g) {
                    let mut m = 0usize;
                    while m < other.tokens.len() {
                        if let Some(prev) = other.tokens.get(m) {
                            if *prev == tok {
                                return Err(SealCode::CollisionBad.val());
                            }
                        }
                        m += 1;
                    }
                }
                g += 1;
            }
            fp = fold_str(fp, tok);
            t += 1;
        }
        fp = fold_str(fp, fam.id);
        f += 1;
    }
    Ok(fp)
}

/// 令牌总数（自洽时 18）。
pub fn token_count() -> u32 {
    (FAMILIES * TOKENS_PER_FAMILY) as u32
}

// ── 卡册 ──────────────────────────────────────────────────────────
/// 一条卡：编号 / slug / 卡面配额 / 子批 / 三个关键词。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub id: u16,
    pub slug: &'static str,
    pub quota: u32,
    pub batch: u8,
    pub keywords: [&'static str; CARD_KEYWORDS],
}

pub const CARD_TABLE: [Card; CARDS_TOTAL] = [
    Card { id: 49001, slug: "quirk_engine", quota: 400, batch: 0, keywords: ["quirk", "表项", "匹配"] },
    Card { id: 49002, slug: "quirk_matcher", quota: 380, batch: 0, keywords: ["quirk", "匹配", "固件"] },
    Card { id: 49003, slug: "quirk_degrade", quota: 360, batch: 0, keywords: ["quirk", "降级", "命中"] },
    Card { id: 49004, slug: "t1_promise", quota: 340, batch: 0, keywords: ["机型", "清单", "承诺"] },
    Card { id: 49005, slug: "pad_config_panel", quota: 360, batch: 1, keywords: ["配置", "档位", "热生效"] },
    Card { id: 49006, slug: "config_persist", quota: 340, batch: 1, keywords: ["配置", "持久化", "热生效"] },
    Card { id: 49007, slug: "config_isolation", quota: 320, batch: 1, keywords: ["配置", "隔离", "校验和"] },
    Card { id: 49008, slug: "quirk_version", quota: 300, batch: 1, keywords: ["表项", "固件", "quirk"] },
    Card { id: 49009, slug: "quirk_fuzz", quota: 300, batch: 2, keywords: ["quirk", "匹配", "降级"] },
    Card { id: 49010, slug: "config_error_path", quota: 280, batch: 2, keywords: ["配置", "校验和", "持久化"] },
    Card { id: 49011, slug: "quirk_hit_ledger", quota: 260, batch: 2, keywords: ["quirk", "命中", "降级"] },
    Card { id: 49012, slug: "domain_ledger_slot", quota: 260, batch: 2, keywords: ["表项", "命中", "quirk"] },
    Card { id: 49013, slug: "copy_walk", quota: 280, batch: 3, keywords: ["配置", "持久化", "校验和"] },
    Card { id: 49014, slug: "b11_inline_roll", quota: 220, batch: 3, keywords: ["quirk", "匹配", "命中"] },
    Card { id: 49015, slug: "err_family", quota: 220, batch: 3, keywords: ["表项", "固件", "quirk"] },
    Card { id: 49016, slug: "observe_points", quota: 260, batch: 3, keywords: ["配置", "热生效", "隔离"] },
    Card { id: 49017, slug: "fuzz_face", quota: 300, batch: 4, keywords: ["quirk", "降级", "固件"] },
    Card { id: 49018, slug: "case_registry", quota: 200, batch: 4, keywords: ["清单", "档案", "机型"] },
    Card { id: 49019, slug: "dedup_grep", quota: 200, batch: 4, keywords: ["档案", "档位", "profile"] },
    Card { id: 49020, slug: "batch_seal", quota: 420, batch: 4, keywords: ["机型", "承诺", "profile"] },
];

/// slug 形状：小写字母开头，只含小写字母/数字/单下划线，长度 4..=32。
pub fn slug_shape_ok(slug: &str) -> bool {
    let b = slug.as_bytes();
    if b.len() < 4 || b.len() > 32 {
        return false;
    }
    let mut prev_us = false;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i).copied() {
            Some(v) => v,
            None => return false,
        };
        let ok = if i == 0 {
            c.is_ascii_lowercase()
        } else {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'
        };
        if !ok {
            return false;
        }
        if c == b'_' {
            if prev_us {
                return false;
            }
            prev_us = true;
        } else {
            prev_us = false;
        }
        i += 1;
    }
    match b.last() {
        Some(c) => *c != b'_',
        None => false,
    }
}

/// 单卡 grep 结果：主族 / 命中数 / 每族命中分布。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepHit {
    pub card: u16,
    pub primary: u8,
    pub hits: u32,
    pub per_family: [u32; FAMILIES],
}

/// 单卡 grep：三关键词按族归位 → 零命中拒 → 主族严格唯一。
pub fn grep_card(card: &Card) -> Result<GrepHit, u32> {
    let mut per_family = [0u32; FAMILIES];
    let mut hits = 0u32;
    let mut k = 0usize;
    while k < card.keywords.len() {
        let tok = match card.keywords.get(k) {
            Some(v) => *v,
            None => return Err(SealCode::KeywordBad.val()),
        };
        if tok.is_empty() {
            return Err(SealCode::KeywordBad.val());
        }
        let fam = match token_owner(tok) {
            Some(f) => f,
            None => return Err(SealCode::KeywordBad.val()),
        };
        if let Some(slot) = per_family.get_mut(fam as usize) {
            *slot = slot.saturating_add(1);
        }
        hits = hits.saturating_add(1);
        k += 1;
    }
    if hits == 0 {
        return Err(SealCode::KeywordBad.val());
    }
    // 主族：命中数严格最大且唯一。
    let mut best = 0u32;
    let mut best_n = 0u32;
    let mut tie = 0u32;
    let mut f = 0usize;
    while f < per_family.len() {
        if let Some(n) = per_family.get(f) {
            if *n > best {
                best = *n;
                best_n = f as u32;
                tie = 1;
            } else if *n == best {
                tie = tie.saturating_add(1);
            }
        }
        f += 1;
    }
    if best == 0 || tie != 1 {
        return Err(SealCode::CollisionBad.val());
    }
    Ok(GrepHit {
        card: card.id,
        primary: best_n as u8,
        hits,
        per_family,
    })
}

/// grep 汇总：每族卡片数 / 总命中 / 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepSummary {
    pub cards: u32,
    pub per_family: [u32; FAMILIES],
    pub hits: u32,
    pub digest: u64,
}

/// 批检查统计：冻结条数 / 批次解剖 / 主族分布 / 配额合计 / 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchStats {
    pub frozen: u32,
    pub per_batch: [u32; BATCHES],
    pub per_family: [u32; FAMILIES],
    pub quota_total: u32,
    pub hits: u32,
    pub digest: u64,
}

impl BatchStats {
    pub fn batch_even(&self) -> bool {
        let mut i = 0usize;
        while i < self.per_batch.len() {
            if let Some(n) = self.per_batch.get(i) {
                if *n != CARDS_PER_BATCH as u32 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    pub fn family_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.per_family.len() {
            if let Some(v) = self.per_family.get(i) {
                n = n.saturating_add(*v);
            }
            i += 1;
        }
        n
    }
}

/// 卡册自审：条数 → 编号重复 → slug 形状/重复 → 编号连续 → 批次/配额 →
/// 单卡 grep（关键词族与主族）→ 批次解剖 → 主族分布 → 摘要。
pub fn card_integrity(table: &[Card]) -> Result<BatchStats, u32> {
    if table.len() != CARDS_TOTAL {
        return Err(SealCode::CountBad.val());
    }
    let mut per_batch = [0u32; BATCHES];
    let mut per_family = [0u32; FAMILIES];
    let mut quota_total = 0u32;
    let mut hits = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < table.len() {
        let c = match table.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::CountBad.val()),
        };
        // 编号重复先于编号连续（重复才是根因）。
        let mut j = 0usize;
        while j < i {
            if let Some(p) = table.get(j) {
                if p.id == c.id {
                    return Err(SealCode::DupBad.val());
                }
                if p.slug == c.slug {
                    return Err(SealCode::SlugBad.val());
                }
            }
            j += 1;
        }
        if !slug_shape_ok(c.slug) {
            return Err(SealCode::SlugBad.val());
        }
        if c.id != CARD_ID_FIRST.saturating_add(i as u16) {
            return Err(SealCode::CountBad.val());
        }
        let b = c.batch as usize;
        if b >= BATCHES {
            return Err(SealCode::BatchBad.val());
        }
        if c.quota < CARD_QUOTA_MIN || c.quota > CARD_QUOTA_MAX {
            return Err(SealCode::QuotaBad.val());
        }
        let g = grep_card(&c)?;
        if let Some(slot) = per_batch.get_mut(b) {
            *slot = slot.saturating_add(1);
        }
        if let Some(slot) = per_family.get_mut(g.primary as usize) {
            *slot = slot.saturating_add(1);
        }
        quota_total = quota_total.saturating_add(c.quota);
        hits = hits.saturating_add(g.hits);
        digest = p_core::fold_u32(digest, c.id as u32);
        digest = fold_str(digest, c.slug);
        digest = p_core::fold_u32(digest, c.quota);
        digest = p_core::fold_u32(digest, c.batch as u32);
        digest = p_core::fold_u32(digest, g.primary as u32);
        digest = p_core::fold_u32(digest, g.hits);
        i += 1;
    }
    let mut k = 0usize;
    while k < per_batch.len() {
        if let Some(n) = per_batch.get(k) {
            if *n != CARDS_PER_BATCH as u32 {
                return Err(SealCode::FreezeBad.val());
            }
        }
        k += 1;
    }
    let mut f = 0usize;
    while f < per_family.len() {
        if let Some(n) = per_family.get(f) {
            if *n == 0 {
                return Err(SealCode::CollisionBad.val());
            }
        }
        f += 1;
    }
    if quota_total != QUOTA_TOTAL {
        return Err(SealCode::SealBad.val());
    }
    Ok(BatchStats {
        frozen: table.len() as u32,
        per_batch,
        per_family,
        quota_total,
        hits,
        digest,
    })
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = p_core::fold_byte(x, *b);
    }
    x
}

/// grep 全册汇总（不重算卡册自审，只做族分布与命中统计）。
pub fn grep_all() -> Result<GrepSummary, u32> {
    let mut per_family = [0u32; FAMILIES];
    let mut hits = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CARD_TABLE.len() {
        let c = match CARD_TABLE.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::CountBad.val()),
        };
        let g = grep_card(&c)?;
        if let Some(slot) = per_family.get_mut(g.primary as usize) {
            *slot = slot.saturating_add(1);
        }
        hits = hits.saturating_add(g.hits);
        digest = p_core::fold_u32(digest, g.card as u32);
        digest = p_core::fold_u32(digest, g.primary as u32);
        digest = p_core::fold_u32(digest, g.hits);
        i += 1;
    }
    Ok(GrepSummary {
        cards: CARD_TABLE.len() as u32,
        per_family,
        hits,
        digest,
    })
}

// ── 域账（行数） ──────────────────────────────────────────────────
/// 行数账：每批行数 + 合计 + 配额合计 + 冻结条数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineLedger {
    pub per_batch: [u32; BATCHES],
    pub total: u32,
    pub quota_total: u32,
    pub frozen: u32,
}

impl LineLedger {
    pub const fn empty() -> LineLedger {
        LineLedger {
            per_batch: [0; BATCHES],
            total: 0,
            quota_total: 0,
            frozen: 0,
        }
    }

    pub fn batch_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.per_batch.len() {
            if let Some(v) = self.per_batch.get(i) {
                n = n.saturating_add(*v);
            }
            i += 1;
        }
        n
    }

    /// 守恒：合计 = 各批之和 = 配额合计 × 12 = 72 000，且冻结 20 条。
    pub fn conserved(&self) -> bool {
        self.frozen == CARDS_TOTAL as u32
            && self.quota_total == QUOTA_TOTAL
            && self.total == self.batch_sum()
            && self.total == self.quota_total.saturating_mul(LINE_MULTIPLIER)
            && self.total == DOMAIN_TOTAL_LINES
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.per_batch.len() {
            if let Some(v) = self.per_batch.get(i) {
                h = p_core::fold_u32(h, *v);
            }
            i += 1;
        }
        h = p_core::fold_u32(h, self.total);
        h = p_core::fold_u32(h, self.quota_total);
        h = p_core::fold_u32(h, self.frozen);
        h
    }
}

/// 单卡域账行数：配额 × 12（越界返回 None，绝不静默钳位）。
pub fn card_lines(quota: u32) -> Option<u32> {
    quota.checked_mul(LINE_MULTIPLIER)
}

/// 域账合计：逐卡配额 × 12 → 按批累加；任何一卡算不出即为 SealBad。
pub fn line_ledger() -> Result<LineLedger, u32> {
    if CARD_TABLE.len() != CARDS_TOTAL {
        return Err(SealCode::CountBad.val());
    }
    let mut lg = LineLedger::empty();
    let mut i = 0usize;
    while i < CARD_TABLE.len() {
        let c = match CARD_TABLE.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::CountBad.val()),
        };
        let b = c.batch as usize;
        if b >= BATCHES {
            return Err(SealCode::BatchBad.val());
        }
        let lines = match card_lines(c.quota) {
            Some(v) => v,
            None => return Err(SealCode::SealBad.val()),
        };
        // 单卡行数必须与「配额 × 12」逐卡对齐；再折进批合计。
        if lines != c.quota.saturating_mul(LINE_MULTIPLIER) {
            return Err(SealCode::SealBad.val());
        }
        if let Some(slot) = lg.per_batch.get_mut(b) {
            *slot = slot.saturating_add(lines);
        }
        lg.total = lg.total.saturating_add(lines);
        lg.quota_total = lg.quota_total.saturating_add(c.quota);
        lg.frozen = lg.frozen.saturating_add(1);
        i += 1;
    }
    if !lg.conserved() {
        return Err(SealCode::LedgerBad.val());
    }
    Ok(lg)
}

/// 批解剖：每批的条数与行数（与 line_ledger 交叉验证）。
pub fn batch_profile() -> Result<([u32; BATCHES], [u32; BATCHES]), u32> {
    let mut count = [0u32; BATCHES];
    let mut lines = [0u32; BATCHES];
    let mut i = 0usize;
    while i < CARD_TABLE.len() {
        if let Some(c) = CARD_TABLE.get(i) {
            let b = c.batch as usize;
            if b >= BATCHES {
                return Err(SealCode::BatchBad.val());
            }
            let l = match card_lines(c.quota) {
                Some(v) => v,
                None => return Err(SealCode::SealBad.val()),
            };
            if let Some(slot) = count.get_mut(b) {
                *slot = slot.saturating_add(1);
            }
            if let Some(slot) = lines.get_mut(b) {
                *slot = slot.saturating_add(l);
            }
        }
        i += 1;
    }
    Ok((count, lines))
}

// ── 封印摘要 ──────────────────────────────────────────────────────
/// 封印摘要：卡册摘要 + 族表摘要 + 域账摘要 + 卡面原文摘要再折一遍。
pub fn seal_digest() -> Result<u64, u32> {
    let fam = family_integrity()?;
    let cards = card_integrity(&CARD_TABLE)?;
    let lg = line_ledger()?;
    let g = grep_all()?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, (fam >> 32) as u32);
    h = p_core::fold_u32(h, fam as u32);
    h = p_core::fold_u32(h, cards.frozen);
    h = p_core::fold_u32(h, cards.quota_total);
    h = p_core::fold_u32(h, cards.hits);
    h = p_core::fold_u32(h, (cards.digest >> 32) as u32);
    h = p_core::fold_u32(h, cards.digest as u32);
    h = p_core::fold_u32(h, lg.total);
    h = p_core::fold_u32(h, (lg.digest() >> 32) as u32);
    h = p_core::fold_u32(h, lg.digest() as u32);
    h = p_core::fold_u32(h, g.hits);
    h = fold_str(h, CARD_SEAL_TEXT);
    if h == 0 {
        return Err(SealCode::SealBad.val());
    }
    Ok(h)
}

/// 封印复跑：三遍摘要逐位一致，返回再折叠后的回查摘要。
pub fn replay_seal() -> Result<u64, u32> {
    let base = seal_digest()?;
    let mut h = base;
    let mut pass = 0u32;
    while pass < REPLAY_PASSES {
        let again = seal_digest()?;
        if again != base {
            return Err(SealCode::ReplayBad.val());
        }
        h = p_core::fold_u32(h, pass);
        h = p_core::fold_u32(h, (again >> 32) as u32);
        h = p_core::fold_u32(h, again as u32);
        pass = pass.saturating_add(1);
    }
    if h == 0 {
        return Err(SealCode::ReplayBad.val());
    }
    Ok(h)
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

/// 五路卡册注入：重号 / 重 slug / 配额零 / 未知关键词 / 批次越界。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > INJECT_MAX {
        return Err(SealCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_9E01);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(INJECT_ROUTES);
        let want = match route {
            0 => SealCode::DupBad.val(),
            1 => SealCode::SlugBad.val(),
            2 => SealCode::QuotaBad.val(),
            3 => SealCode::KeywordBad.val(),
            _ => SealCode::BatchBad.val(),
        };
        let mut t = CARD_TABLE.to_vec();
        let idx = rng.below(CARDS_TOTAL as u32) as usize;
        match route {
            0 => {
                if let (Some(src), Some(dst)) = (
                    CARD_TABLE.get(0).map(|c| c.id),
                    t.get_mut(idx),
                ) {
                    dst.id = src;
                }
            }
            1 => {
                if let (Some(src), Some(dst)) = (
                    CARD_TABLE.get(0).map(|c| c.slug),
                    t.get_mut(idx),
                ) {
                    dst.slug = src;
                }
            }
            2 => {
                if let Some(slot) = t.get_mut(idx) {
                    slot.quota = 0;
                }
            }
            3 => {
                if let Some(slot) = t.get_mut(idx) {
                    slot.keywords = ["未知令牌", "quirk", "配置"];
                }
            }
            _ => {
                if let Some(slot) = t.get_mut(idx) {
                    slot.batch = BATCHES as u8;
                }
            }
        }
        if card_integrity(&t).err() == Some(want) {
            caught = caught.saturating_add(1);
        }
        last = route;
        r = r.saturating_add(1);
    }
    if caught != rounds {
        return Err(SealCode::KeywordBad.val());
    }
    Ok(InjectLog {
        rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：六条结构性坏卡册各按指定码拒绝。
pub fn tamper_rejected() -> bool {
    let mut t = CARD_TABLE.to_vec();
    t[5].id = CARD_TABLE[4].id;
    let d1 = card_integrity(&t).err() == Some(SealCode::DupBad.val());

    let mut t = CARD_TABLE.to_vec();
    t[6].slug = CARD_TABLE[5].slug;
    let d2 = card_integrity(&t).err() == Some(SealCode::SlugBad.val());

    let mut t = CARD_TABLE.to_vec();
    t[7].quota = 0;
    let d3 = card_integrity(&t).err() == Some(SealCode::QuotaBad.val());

    let mut t = CARD_TABLE.to_vec();
    t[8].keywords = ["quirk", "配置", "档案"];
    let d4 = card_integrity(&t).err() == Some(SealCode::CollisionBad.val());

    let mut t = CARD_TABLE.to_vec();
    t[9].batch = BATCHES as u8;
    let d5 = card_integrity(&t).err() == Some(SealCode::BatchBad.val());

    let t = &CARD_TABLE[..CARDS_TOTAL - 1];
    let d6 = card_integrity(t).err() == Some(SealCode::CountBad.val());

    let mut t = CARD_TABLE.to_vec();
    t[10].slug = "Bad_Slug";
    let d7 = card_integrity(&t).err() == Some(SealCode::SlugBad.val());

    d1 && d2 && d3 && d4 && d5 && d6 && d7
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
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
/// params=冻结条数 / schedule=关键词族数 / fails=撞号数 / keys=域账行数。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
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

    // F49019 ①：关键词族自洽（3 族 × 6 令牌，18 个两两不同）。
    let fam = family_integrity();
    let fam_ok = matches!(&fam, Ok(v) if *v != 0)
        && token_count() == TOKENS_TOTAL as u32
        && token_owner("quirk") == Some(0)
        && token_owner("配置") == Some(1)
        && token_owner("档案") == Some(2)
        && token_owner("无此令牌").is_none();
    out.push((
        "F49019/关键词族自洽",
        fam_ok,
        format!(
            "families={} tokens={} fp={:016X}",
            FAMILIES,
            token_count(),
            fam.as_ref().copied().unwrap_or(0)
        ),
    ));

    // F49019 ②：20 卡 grep 零遗漏（每卡至少 1 命中、合计 60）。
    let grep = grep_all();
    let mut missed = 0u32;
    let mut i = 0usize;
    while i < CARD_TABLE.len() {
        if let Some(c) = CARD_TABLE.get(i) {
            if grep_card(c).is_err() {
                missed = missed.saturating_add(1);
            }
        }
        i += 1;
    }
    let grep_ok = match &grep {
        Ok(g) => g.cards == CARDS_TOTAL as u32 && g.hits == 60 && missed == 0,
        Err(_) => false,
    };
    out.push((
        "F49019/20卡零遗漏",
        grep_ok,
        format!(
            "cards={:?} hits={:?} missed={}",
            grep.as_ref().map(|g| g.cards),
            grep.as_ref().map(|g| g.hits),
            missed
        ),
    ));

    // F49019 ③：零撞号（编号/slug 唯一，主族 10/6/4，批次 5×4）。
    let stats = card_integrity(&CARD_TABLE);
    let zero_ok = match &stats {
        Ok(s) => {
            s.frozen == CARDS_TOTAL as u32
                && s.batch_even()
                && s.family_sum() == CARDS_TOTAL as u32
                && s.per_family == [10, 6, 4]
        }
        Err(_) => false,
    };
    out.push((
        "F49019/零撞号",
        zero_ok,
        format!(
            "frozen={:?} per_family={:?} per_batch={:?}",
            stats.as_ref().map(|s| s.frozen),
            stats.as_ref().map(|s| s.per_family),
            stats.as_ref().map(|s| s.per_batch)
        ),
    ));

    // F49019 ④：五路注入全捕获 + 反例必拒。
    let inj = inject_rounds(1_000);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught()) && tamper_rejected();
    out.push((
        "F49019/注入全捕获",
        inj_ok,
        format!(
            "rounds={:?} caught={:?} tamper={}",
            inj.as_ref().map(|l| l.rounds),
            inj.as_ref().map(|l| l.caught),
            tamper_rejected()
        ),
    ));

    // F49020 ①：20 卡冻结（批次 5×4，条数 20）。
    let freeze_ok = match &stats {
        Ok(s) => s.frozen == 20 && s.batch_even() && s.family_sum() == 20,
        Err(_) => false,
    };
    out.push((
        "F49020/20卡冻结",
        freeze_ok,
        format!(
            "frozen={:?} batches={} per_batch={:?}",
            stats.as_ref().map(|s| s.frozen),
            BATCHES,
            stats.as_ref().map(|s| s.per_batch)
        ),
    ));

    // F49020 ②：域账 72 000（各批之和 = 配额 × 12）。
    let lg = line_ledger();
    let profile = batch_profile();
    let ledger_ok = match (&lg, &profile) {
        (Ok(l), Ok((counts, lines))) => {
            l.conserved()
                && l.total == DOMAIN_TOTAL_LINES
                && l.batch_sum() == DOMAIN_TOTAL_LINES
                && l.quota_total == QUOTA_TOTAL
                && *lines == l.per_batch
                && *counts == [4, 4, 4, 4, 4]
        }
        _ => false,
    };
    out.push((
        "F49020/域账72000",
        ledger_ok,
        format!(
            "total={:?} quota={:?} per_batch={:?}",
            lg.as_ref().map(|l| l.total),
            lg.as_ref().map(|l| l.quota_total),
            lg.as_ref().map(|l| l.per_batch)
        ),
    ));

    // F49020 ③：收官印复跑（三遍摘要逐位一致）。
    let seal_a = replay_seal();
    let seal_b = replay_seal();
    let seal_ok = match (&seal_a, &seal_b) {
        (Ok(a), Ok(b)) => a == b && *a != 0,
        _ => false,
    };
    out.push((
        "F49020/收官印复跑",
        seal_ok,
        format!(
            "passes={} seal_a={:016X} seal_b={:016X}",
            REPLAY_PASSES,
            seal_a.as_ref().copied().unwrap_or(0),
            seal_b.as_ref().copied().unwrap_or(0)
        ),
    ));

    // F49020 ④：渲染回读 + 段位守卫 + 卡面原文逐字保留。
    let digest = seal_a.as_ref().copied().unwrap_or(0);
    let row = render_row(
        "S49019",
        CARDS_TOTAL as u32,
        FAMILIES as u32,
        0,
        DOMAIN_TOTAL_LINES,
        digest,
    );
    let seal_text_ok = CARD_SEAL_TEXT
        == "UNX-F49020-J1 批印入册：66000/240,000"
        && CARD_SEAL_TEXT.contains("66000/240,000");
    let render_ok = audit_render(&row)
        && !audit_render("")
        && !audit_render("S49019|params=20|schedule=3|fails=0|keys=72000")
        && !audit_render(
            "S49019|params=20|schedule=3|fails=0|keys=72000|digest=0000000000000000|z=1",
        );
    out.push((
        "F49020/渲染回读",
        render_ok && segment_guard() && seal_text_ok,
        format!(
            "row={} audit={} seg={} text={}",
            row,
            render_ok,
            segment_guard(),
            seal_text_ok
        ),
    ));

    out
}

pub fn run_unx49019_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49020 B11 批防重与收官印");
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
        assert_eq!(SealCode::CountBad.val(), SEG_BASE + 0x01);
        assert_eq!(SealCode::SealBad.val(), SEG_BASE + 14);
        assert_eq!(SealCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < SealCode::ALL.len() {
            if let Some(c) = SealCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn keyword_families_are_disjoint() {
        assert!(family_integrity().unwrap() != 0);
        assert_eq!(token_count(), TOKENS_TOTAL as u32);
        let mut f = 0usize;
        while f < KEYWORD_FAMILIES.len() {
            let fam = KEYWORD_FAMILIES[f];
            let mut t = 0usize;
            while t < fam.tokens.len() {
                assert_eq!(token_owner(fam.tokens[t]), Some(f as u8));
                t += 1;
            }
            f += 1;
        }
        assert_eq!(token_owner("quirk"), Some(0));
        assert_eq!(token_owner("config"), Some(1));
        assert_eq!(token_owner("profile"), Some(2));
        assert_eq!(token_owner("未知"), None);
    }

    #[test]
    fn every_token_is_globally_unique() {
        let mut seen: Vec<&str> = Vec::new();
        let mut f = 0usize;
        while f < KEYWORD_FAMILIES.len() {
            let mut t = 0usize;
            while t < KEYWORD_FAMILIES[f].tokens.len() {
                let tok = KEYWORD_FAMILIES[f].tokens[t];
                assert!(!seen.contains(&tok), "dup token {}", tok);
                seen.push(tok);
                t += 1;
            }
            f += 1;
        }
        assert_eq!(seen.len(), TOKENS_TOTAL);
    }

    #[test]
    fn card_table_is_self_consistent() {
        let s = card_integrity(&CARD_TABLE).unwrap();
        assert_eq!(s.frozen, 20);
        assert!(s.batch_even());
        assert_eq!(s.per_batch, [4, 4, 4, 4, 4]);
        assert_eq!(s.per_family, [10, 6, 4]);
        assert_eq!(s.family_sum(), 20);
        assert_eq!(s.quota_total, QUOTA_TOTAL);
        assert_eq!(s.hits, 60);
        assert_ne!(s.digest, 0);
    }

    #[test]
    fn every_card_has_a_unique_primary_family() {
        let mut i = 0usize;
        let mut primaries = [0u32; FAMILIES];
        while i < CARD_TABLE.len() {
            let g = grep_card(&CARD_TABLE[i]).unwrap();
            assert_eq!(g.card, CARD_TABLE[i].id);
            assert_eq!(g.hits, 3);
            assert!(g.primary < FAMILIES as u8);
            primaries[g.primary as usize] += 1;
            i += 1;
        }
        assert_eq!(primaries, [10, 6, 4]);
    }

    #[test]
    fn card_ids_and_slugs_are_unique() {
        let mut i = 0usize;
        while i < CARD_TABLE.len() {
            assert_eq!(CARD_TABLE[i].id, CARD_ID_FIRST + i as u16);
            assert!(slug_shape_ok(CARD_TABLE[i].slug));
            let mut j = i + 1;
            while j < CARD_TABLE.len() {
                assert_ne!(CARD_TABLE[i].id, CARD_TABLE[j].id);
                assert_ne!(CARD_TABLE[i].slug, CARD_TABLE[j].slug);
                j += 1;
            }
            i += 1;
        }
        assert!(!slug_shape_ok("Bad"));
        assert!(!slug_shape_ok("bad__slug"));
        assert!(!slug_shape_ok("bad_"));
        assert!(slug_shape_ok("bad2_slug"));
    }

    #[test]
    fn batches_are_four_cards_each() {
        let (counts, lines) = batch_profile().unwrap();
        assert_eq!(counts, [4, 4, 4, 4, 4]);
        assert_eq!(lines, [17_760, 15_840, 13_200, 11_760, 13_440]);
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < lines.len() {
            sum = sum.saturating_add(lines[i]);
            i += 1;
        }
        assert_eq!(sum, DOMAIN_TOTAL_LINES);
        assert_eq!(BATCH_NAMES.len(), BATCHES);
    }

    #[test]
    fn line_ledger_conserves_at_72000() {
        let l = line_ledger().unwrap();
        assert_eq!(l.frozen, 20);
        assert_eq!(l.quota_total, 6_000);
        assert_eq!(l.total, 72_000);
        assert_eq!(l.total, l.batch_sum());
        assert_eq!(l.total, l.quota_total * LINE_MULTIPLIER);
        assert!(l.conserved());
        assert_ne!(l.digest(), 0);
    }

    #[test]
    fn per_card_lines_are_quota_times_twelve() {
        let mut i = 0usize;
        while i < CARD_TABLE.len() {
            let want = CARD_TABLE[i].quota * LINE_MULTIPLIER;
            assert_eq!(card_lines(CARD_TABLE[i].quota), Some(want));
            i += 1;
        }
        assert_eq!(card_lines(u32::MAX), None);
    }

    #[test]
    fn bad_card_tables_are_rejected() {
        assert!(tamper_rejected());
        let mut t = CARD_TABLE.to_vec();
        t[2].keywords = ["无主令牌", "quirk", "配置"];
        assert_eq!(
            card_integrity(&t).err(),
            Some(SealCode::KeywordBad.val())
        );
        let mut t = CARD_TABLE.to_vec();
        t[3].quota = CARD_QUOTA_MAX + 1;
        assert_eq!(card_integrity(&t).err(), Some(SealCode::QuotaBad.val()));
        let mut t = CARD_TABLE.to_vec();
        t[4].keywords = ["quirk", "配置", "档案"];
        assert_eq!(
            card_integrity(&t).err(),
            Some(SealCode::CollisionBad.val())
        );
    }

    #[test]
    fn tie_between_families_is_a_collision() {
        let c = Card {
            id: 49999,
            slug: "tie_card",
            quota: 200,
            batch: 0,
            keywords: ["quirk", "配置", "档案"],
        };
        assert_eq!(
            grep_card(&c).err(),
            Some(SealCode::CollisionBad.val())
        );
        let c2 = Card {
            id: 49998,
            slug: "zero_card",
            quota: 200,
            batch: 0,
            keywords: ["无主令牌", "另一个", "第三个"],
        };
        assert_eq!(grep_card(&c2).err(), Some(SealCode::KeywordBad.val()));
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
    fn seal_digest_replays_three_times() {
        let a = seal_digest().unwrap();
        let b = replay_seal().unwrap();
        let c = replay_seal().unwrap();
        assert_ne!(a, 0);
        assert_eq!(b, c);
        assert_ne!(b, 0);
    }

    #[test]
    fn card_seal_text_keeps_verbatim_anchor() {
        assert!(CARD_SEAL_TEXT.starts_with("UNX-F49020-J1"));
        assert!(CARD_SEAL_TEXT.contains("66000/240,000"));
        assert_eq!(DOMAIN_TOTAL_LINES, 72_000);
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let row = render_row("S49019", 20, 3, 0, 72_000, 0xFEDC_BA98_7654_3210);
        assert!(audit_render(&row));
        assert!(!audit_render(""));
        assert!(!audit_render("S49019|params=20|schedule=3|fails=0|keys=72000"));
        assert!(!audit_render(
            "S49019|params=20|schedule=3|fails=0|keys=72000|digest=000000000000000"
        ));
        assert!(!audit_render(
            "S49019|params=20|schedule=3|fails=0|key=72000|digest=0000000000000001"
        ));
        assert_eq!(parse_u32("72000"), Some(72_000));
        assert_eq!(parse_u32("7_2000"), None);
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_hex16("FEDCBA9876543210"), Some(0xFEDC_BA98_7654_3210));
        assert_eq!(parse_hex16("xEDCBA9876543210"), None);
        assert_eq!(row_fields(&row).map(|f| f.len()), Some(6));
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49019_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(extra_checks().len(), 8);
    }
}
