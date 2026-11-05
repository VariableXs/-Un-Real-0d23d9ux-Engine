//! UNX-F49059 B13 批防重与批收官印（20 卡 / 78 000 行）· 0x73240
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B13 族第十二槽；J1 引用按任务板原文保留）：
//!   UNX-F49059 B13 批防重 grep focus/vxwm/constraint（三族令牌 30 条）:
//!          ① 纯功能代码 ≥200 行；
//!          ② 判据原文：UNX-F49060-J1 批印入册：78000/240,000；判据锚定 §AI-62 §7；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49060 B13 批收官印 20 卡/78 000 行（四段分布 × 10 日落账）:
//!          ① 纯功能代码 ≥420 行；
//!          ② 判据原文：UNX-F49061-J1 四段分布 ×10 日落账一致；判据锚定 §AI-62 §5 J-1；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（批防重）：词法 → 分族 → 唯一 → grep → 入册。
//! 词法先于分族：一个连族规词法都不合的令牌，谈不上属于哪一族；
//!   先分族会把 `FOCUS_OWNER_` 这种残缺串硬塞进 focus 族，掩盖书写事故。
//! 分族先于唯一：唯一性必须**跨三族**成立——三族共用同一张主账，
//!   族内不重但跨族重名（focus 与 vxwm 都叫 owner_01）一样会污染 grep。
//! 唯一先于 grep：grep 的语义是「按族名数出现次数」，重名会让一条账行
//!   同时被两族数到；唯一性不成立时，grep 的数字本身就是假的。
//! grep 先于入册：入册（批印）是对「三族 × 10 主账」的冻结；
//!   只有 grep 证明 30 条各归其族、零跨族泄漏，冻结才有意义。
//! 闸门次序（批收官印）：卡 → 段 → 日 → 印 → 幂等。
//! 卡先于段：20 张卡是原子单位，先确认卡号连续（F49041..F49060）、槽位不撞，
//!   再谈它们怎么分到四个额度段；卡不齐时谈分布是空话。
//! 段先于日：四段分布（4200/3600/3400/3200，各 5 卡）是「按卡」的横切；
//!   卡行数合计 72 000 必须先对上，才有资格与「按日」的竖切对账。
//! 日先于印：十日账（card_lines + 600 aux）是「按日」的竖切，合计 78 000；
//!   横切与竖切两侧必须落在同一个数上——这是「×10 日落账一致」的实质。
//! 印先于幂等：收官印 = 对 (20 卡, 78 000 行, 3250 bp, 10 日) 的一次冻结；
//!   冻结必须可重复：同一账本冻两次摘要必须相同（否则印会自己漂）。
//! 幂等先于交付：冻不出一致的印，就不许写「批印入册」；
//!   交付账只在印幂等成立后才允许生成。
//! 关键差异：F49059 只回答「三族 30 条令牌是否防重、grep 是否零泄漏」，
//!   不计算行数；F49060 只回答「20 卡 78 000 行在四段 × 十日的双重切法下
//!   是否对得上、印是否幂等」，不重写令牌词法。
//!   两卡的账彼此独立：令牌账折「族+序号+词法」，批账折「卡号+额度+日」。
//!
//! 行数口径（78 000 的来历，两处独立计算必须相等）：
//!   横切（按卡的四段）：[4200, 3600, 3400, 3200] × 5 组 = 72 000 卡行；
//!   竖切（按日的十日）：[7800, 6600] 交替 5 轮 = 39 000 + 33 000 = 72 000 卡行；
//!   另加辅记 600 行/日 × 10 日 = 6 000 行；
//!   合计 72 000 + 6 000 = 78 000 行；域账 240 000 行；
//!   占比 78 000 / 240 000 = 3250 bp（万分比，整除，无余数）。
//!   四段各 5 卡：5 × (4200+3600+3400+3200) = 5 × 14 400 = 72 000。
//!   十日合计：5 × 7800 + 5 × 6600 = 72 000；72 000 + 6 000 = 78 000。
//!
//! 错误码口径（两卡共用，×15 的来源）：段内 14 条闭集（SEG_BASE+0x01..+0x0E）
//!   + 1 条越界哨（detail() 兜底返回「未知批收官诊断码」，不占段内槽位）。
//!   本模块的 15 条冻结快照见 `CODE_SNAPSHOT`，逐行与枚举 `detail()` 比对。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73240，B13 族第十二槽：F49059/F49060 两卡共用）。
pub const SEG_BASE: u32 = 0x73240;

/// 码族目录行数：14 段内 + 1 越界哨。
pub const CODE_ROWS: usize = 15;
/// 三族令牌总数。
pub const TOKEN_TOTAL: usize = 30;
/// 每族令牌数（每族 10 个 owner 主账）。
pub const PER_FAMILY: usize = 10;
/// 族数。
pub const FAMILIES: usize = 3;
/// 批卡总数（F49041..F49060）。
pub const CARD_TOTAL: usize = 20;
/// 批卡编号起点：卡号文本为 F49041..F49060，数值取十进制序号 49041..49060
/// （F 是批号前缀，不参与进位；若按十六进制解释，0xF49041..0xF49060 会多出 12 个空号）。
pub const CARD_ID_BASE: u32 = 49_041;
/// 卡槽基址（B13 段：0x73000 起，每卡 0x20）。
pub const CARD_SLOT_BASE: u32 = 0x73000;
/// 卡槽步长。
pub const CARD_SLOT_STEP: u32 = 0x20;
/// 四段额度（卡行数）。
pub const TIER_QUOTAS: [u32; 4] = [4200, 3600, 3400, 3200];
/// 每段的卡数（四段各 5 卡）。
pub const TIER_CARDS: [u32; 4] = [5, 5, 5, 5];
/// 批卡行数合计（四段 × 5 组）。
pub const CARD_ROWS: u32 = 72_000;
/// 每卡行数之和的另一条算法结果（编译期常量，供断言对照）。
pub const CARD_ROWS_SUM: u32 = 14_400 * 5;
/// 十日卡行分布（7800/6600 交替）。
pub const DAY_LINES: [u32; 10] = [7800, 6600, 7800, 6600, 7800, 6600, 7800, 6600, 7800, 6600];
/// 每日辅记行数。
pub const AUX_PER_DAY: u32 = 600;
/// 批总行数：72 000 卡行 + 6 000 辅记。
pub const BATCH_ROWS: u32 = 78_000;
/// 域账行数。
pub const DOMAIN_ROWS: u32 = 240_000;
/// 批占比（万分比）：3250 bp = 32.50%。
pub const RATIO_BP: u32 = 3250;
/// 账期天数。
pub const DAYS: usize = 10;
/// 六字段账行的字段数。
pub const RENDER_FIELDS: usize = 6;
/// 任务板给本模块的跨槽编号总数。
pub const SLOT_CASE_TOTAL: usize = 16;
/// 跨槽编号基址（0x0734_1000 起，实际用 +0x01..+0x10）。
pub const CASE_BASE: u32 = 0x0734_1000;

/// 令牌账标签。
pub const TOKEN_TAG: &str = "TOKN";
/// 收官印账标签。
pub const SEAL_TAG: &str = "SEAL";
/// 日账标签。
pub const DAY_TAG: &str = "DAYS";
/// 卡账标签。
pub const CARD_TAG: &str = "CARD";
/// 码族账标签。
pub const CODE_TAG: &str = "CODE";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知批收官诊断码";

// ── 诊断码闭集（段内 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 令牌缺失：三族 30 条不全
    TokenMissing,
    /// 令牌重名：族内或跨族撞车
    TokenDup,
    /// 令牌词法不合族规
    TokenSyntax,
    /// 某族条数不足 10
    FamilyShort,
    /// grep 跨族泄漏
    GrepLeak,
    /// 批印漂移：两次冻结摘要不同
    StampDrift,
    /// 行数不足 78 000
    LineShort,
    /// 卡数不足 20
    CardShort,
    /// 占比不符 3250 bp
    RatioBad,
    /// 日账缺口：某日合计对不上
    DayGap,
    /// 冻结失败：印章约束不成立
    FreezeBad,
    /// 交付失败：印与预算不符
    DeliveryBad,
    /// 卡号重复或越出 F49041..F49060
    IdDup,
    /// 收官印破损：摘要为零或账行不可回读
    SealBroken,
}

impl SealCode {
    /// 码值 = 段基址 + 0x01 + 枚举序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 码的中文说明（逐条不同）。
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::TokenMissing => "批防重：三族三十条令牌不全",
            SealCode::TokenDup => "批防重：令牌族内或跨族重名",
            SealCode::TokenSyntax => "批防重：令牌词法不合族规",
            SealCode::FamilyShort => "批防重：某族 owner 主账不足十条",
            SealCode::GrepLeak => "批防重：grep 出现跨族泄漏",
            SealCode::StampDrift => "批印漂移：两次冻结摘要不同",
            SealCode::LineShort => "批收官：行数未达七万八千",
            SealCode::CardShort => "批收官：卡数未满二十",
            SealCode::RatioBad => "批收官：占域账比例不是三千二百五十基点",
            SealCode::DayGap => "批收官：某日卡行加辅记对不上",
            SealCode::FreezeBad => "批收官：冻结约束不成立",
            SealCode::DeliveryBad => "批收官：交付账与预算不符",
            SealCode::IdDup => "批收官：卡号重复或越出编号窗口",
            SealCode::SealBroken => "批收官：收官印破损不可回读",
        }
    }

    /// 全部 14 条段内码。
    pub const ALL: [SealCode; 14] = [
        SealCode::TokenMissing,
        SealCode::TokenDup,
        SealCode::TokenSyntax,
        SealCode::FamilyShort,
        SealCode::GrepLeak,
        SealCode::StampDrift,
        SealCode::LineShort,
        SealCode::CardShort,
        SealCode::RatioBad,
        SealCode::DayGap,
        SealCode::FreezeBad,
        SealCode::DeliveryBad,
        SealCode::IdDup,
        SealCode::SealBroken,
    ];
}

/// 自由函数版 detail：越界码兜底到 UNKNOWN。
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
    UNKNOWN
}

/// 段内码判定。
pub fn known_code(code: u32) -> bool {
    code >= SEG_BASE + 0x01 && code <= SEG_BASE + 0x0E
}

/// 段位自检。
pub fn segment_guard() -> bool {
    if SealCode::ALL.len() != 14 {
        return false;
    }
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        let c = SealCode::ALL[i];
        if c.val() != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if detail(c.val()) != c.detail() {
            return false;
        }
        i += 1;
    }
    !known_code(SEG_BASE)
        && !known_code(SEG_BASE + 0x0F)
        && detail(SEG_BASE + 0x0F) == UNKNOWN
}

// ── 码族冻结快照 ──────────────────────────────────────────────────
/// 快照行：码值 + 中文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeRow {
    pub code: u32,
    pub cn: &'static str,
}

/// 手写 15 条冻结快照（14 条段内 + 1 条越界哨）。
pub const CODE_SNAPSHOT: [CodeRow; CODE_ROWS] = [
    CodeRow { code: 0x73241, cn: "批防重：三族三十条令牌不全" },
    CodeRow { code: 0x73242, cn: "批防重：令牌族内或跨族重名" },
    CodeRow { code: 0x73243, cn: "批防重：令牌词法不合族规" },
    CodeRow { code: 0x73244, cn: "批防重：某族 owner 主账不足十条" },
    CodeRow { code: 0x73245, cn: "批防重：grep 出现跨族泄漏" },
    CodeRow { code: 0x73246, cn: "批印漂移：两次冻结摘要不同" },
    CodeRow { code: 0x73247, cn: "批收官：行数未达七万八千" },
    CodeRow { code: 0x73248, cn: "批收官：卡数未满二十" },
    CodeRow { code: 0x73249, cn: "批收官：占域账比例不是三千二百五十基点" },
    CodeRow { code: 0x7324A, cn: "批收官：某日卡行加辅记对不上" },
    CodeRow { code: 0x7324B, cn: "批收官：冻结约束不成立" },
    CodeRow { code: 0x7324C, cn: "批收官：交付账与预算不符" },
    CodeRow { code: 0x7324D, cn: "批收官：卡号重复或越出编号窗口" },
    CodeRow { code: 0x7324E, cn: "批收官：收官印破损不可回读" },
    CodeRow { code: 0x7324F, cn: UNKNOWN },
];

/// 快照行取值。
pub fn snapshot_row(i: usize) -> Option<CodeRow> {
    CODE_SNAPSHOT.get(i).copied()
}

/// 码族快照审计：码值序、中文、唯一性、哨位。
pub fn audit_code_snapshot() -> Result<u64, u32> {
    if CODE_SNAPSHOT.len() != CODE_ROWS {
        return Err(SealCode::TokenMissing.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < CODE_ROWS {
        let r = match CODE_SNAPSHOT.get(i) {
            Some(v) => *v,
            None => return Err(SealCode::TokenMissing.val()),
        };
        if i + 1 < CODE_ROWS {
            let want = match SealCode::ALL.get(i) {
                Some(c) => c.val(),
                None => return Err(SealCode::TokenMissing.val()),
            };
            if r.code != want || r.cn != detail(want) {
                return Err(SealCode::StampDrift.val());
            }
        } else {
            if r.code != SEG_BASE + 0x0F || known_code(r.code) || r.cn != UNKNOWN {
                return Err(SealCode::SealBroken.val());
            }
        }
        let mut j = i + 1;
        while j < CODE_ROWS {
            if let Some(o) = CODE_SNAPSHOT.get(j) {
                if o.cn == r.cn || o.code == r.code {
                    return Err(SealCode::TokenDup.val());
                }
            }
            j += 1;
        }
        h = p_core::fold_u32(h, r.code);
        h = p_core::fold_u32(h, r.cn.len() as u32);
        i += 1;
    }
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(h)
}

// ── 三族令牌 ──────────────────────────────────────────────────────
/// 令牌族：focus / vxwm / constraint。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    Focus,
    Vxwm,
    Constraint,
}

impl Family {
    pub fn index(self) -> u8 {
        match self {
            Family::Focus => 0,
            Family::Vxwm => 1,
            Family::Constraint => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Family::Focus => "focus",
            Family::Vxwm => "vxwm",
            Family::Constraint => "constraint",
        }
    }

    pub fn from_index(i: u8) -> Result<Family, u32> {
        match i {
            0 => Ok(Family::Focus),
            1 => Ok(Family::Vxwm),
            2 => Ok(Family::Constraint),
            _ => Err(SealCode::GrepLeak.val()),
        }
    }

    /// 族规词法：focus/constraint 用 UPPER_SNAKE，vxwm 用 lower_snake。
    pub fn upper(self) -> bool {
        self != Family::Vxwm
    }
}

/// 令牌：族 + owner 序号 + 名字 + 是否主账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token {
    pub name: &'static str,
    pub family: Family,
    pub owner: u8,
    pub main_ledger: bool,
}

impl Token {
    /// 令牌的账行：`name=<family>-<owner>` 形式，供 grep 数族。
    pub fn ledger_line(self) -> String {
        format!("{}={}-{:02}", self.name, self.family.name(), self.owner)
    }
}

/// 手写 30 条令牌（三族 × 10 owner 主账）。
pub const TOKENS: [Token; TOKEN_TOTAL] = [
    Token { name: "FOCUS_OWNER_01", family: Family::Focus, owner: 1, main_ledger: true },
    Token { name: "FOCUS_OWNER_02", family: Family::Focus, owner: 2, main_ledger: true },
    Token { name: "FOCUS_OWNER_03", family: Family::Focus, owner: 3, main_ledger: true },
    Token { name: "FOCUS_OWNER_04", family: Family::Focus, owner: 4, main_ledger: true },
    Token { name: "FOCUS_OWNER_05", family: Family::Focus, owner: 5, main_ledger: true },
    Token { name: "FOCUS_OWNER_06", family: Family::Focus, owner: 6, main_ledger: true },
    Token { name: "FOCUS_OWNER_07", family: Family::Focus, owner: 7, main_ledger: true },
    Token { name: "FOCUS_OWNER_08", family: Family::Focus, owner: 8, main_ledger: true },
    Token { name: "FOCUS_OWNER_09", family: Family::Focus, owner: 9, main_ledger: true },
    Token { name: "FOCUS_OWNER_10", family: Family::Focus, owner: 10, main_ledger: true },
    Token { name: "vxwm_owner_01", family: Family::Vxwm, owner: 1, main_ledger: true },
    Token { name: "vxwm_owner_02", family: Family::Vxwm, owner: 2, main_ledger: true },
    Token { name: "vxwm_owner_03", family: Family::Vxwm, owner: 3, main_ledger: true },
    Token { name: "vxwm_owner_04", family: Family::Vxwm, owner: 4, main_ledger: true },
    Token { name: "vxwm_owner_05", family: Family::Vxwm, owner: 5, main_ledger: true },
    Token { name: "vxwm_owner_06", family: Family::Vxwm, owner: 6, main_ledger: true },
    Token { name: "vxwm_owner_07", family: Family::Vxwm, owner: 7, main_ledger: true },
    Token { name: "vxwm_owner_08", family: Family::Vxwm, owner: 8, main_ledger: true },
    Token { name: "vxwm_owner_09", family: Family::Vxwm, owner: 9, main_ledger: true },
    Token { name: "vxwm_owner_10", family: Family::Vxwm, owner: 10, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_01", family: Family::Constraint, owner: 1, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_02", family: Family::Constraint, owner: 2, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_03", family: Family::Constraint, owner: 3, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_04", family: Family::Constraint, owner: 4, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_05", family: Family::Constraint, owner: 5, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_06", family: Family::Constraint, owner: 6, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_07", family: Family::Constraint, owner: 7, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_08", family: Family::Constraint, owner: 8, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_09", family: Family::Constraint, owner: 9, main_ledger: true },
    Token { name: "CONSTRAINT_OWNER_10", family: Family::Constraint, owner: 10, main_ledger: true },
];

/// 按名字取令牌。
pub fn token_by_name(name: &str) -> Option<Token> {
    let mut i = 0usize;
    while i < TOKENS.len() {
        if TOKENS[i].name == name {
            return Some(TOKENS[i]);
        }
        i += 1;
    }
    None
}

/// 按 (族, owner) 取令牌。
pub fn token_at(family: Family, owner: u8) -> Option<Token> {
    let mut i = 0usize;
    while i < TOKENS.len() {
        if TOKENS[i].family == family && TOKENS[i].owner == owner {
            return Some(TOKENS[i]);
        }
        i += 1;
    }
    None
}

/// 词法检查：UPPER_SNAKE / lower_snake，首字符必须是字母，无首尾下划线、无连续下划线。
pub fn syntax_ok(name: &str, upper: bool) -> bool {
    if name.len() < 6 || name.len() > 40 {
        return false;
    }
    let bytes = name.as_bytes();
    let first = match bytes.first() {
        Some(v) => *v,
        None => return false,
    };
    let first_ok = if upper {
        (b'A'..=b'Z').contains(&first)
    } else {
        (b'a'..=b'z').contains(&first)
    };
    if !first_ok {
        return false;
    }
    let last = match bytes.last() {
        Some(v) => *v,
        None => return false,
    };
    if last == b'_' {
        return false;
    }
    let mut i = 0usize;
    let mut prev_us = false;
    while i < bytes.len() {
        let b = bytes[i];
        let ok = if upper {
            (b'A'..=b'Z').contains(&b) || (b'0'..=b'9').contains(&b) || b == b'_'
        } else {
            (b'a'..=b'z').contains(&b) || (b'0'..=b'9').contains(&b) || b == b'_'
        };
        if !ok {
            return false;
        }
        if b == b'_' && prev_us {
            return false;
        }
        prev_us = b == b'_';
        i += 1;
    }
    true
}

/// 令牌账：条数、每族条数、重名数、词法错数、主账数、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenLedger {
    pub rows: u32,
    pub per_family: [u32; FAMILIES],
    pub dup: u32,
    pub syntax_bad: u32,
    pub main_ledger: u32,
    pub digest: u64,
}

/// 审计令牌目录：条数 → 分族 → 唯一 → 词法 → 主账。
pub fn audit_tokens() -> Result<TokenLedger, u32> {
    if TOKENS.len() != TOKEN_TOTAL {
        return Err(SealCode::TokenMissing.val());
    }
    let mut per = [0u32; FAMILIES];
    let mut dup = 0u32;
    let mut syntax_bad = 0u32;
    let mut main_ledger = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < TOKENS.len() {
        let t = TOKENS[i];
        let fi = t.family.index() as usize;
        if fi >= FAMILIES {
            return Err(SealCode::GrepLeak.val());
        }
        if t.owner == 0 || t.owner as usize > PER_FAMILY {
            return Err(SealCode::FamilyShort.val());
        }
        if !syntax_ok(t.name, t.family.upper()) {
            syntax_bad = syntax_bad.saturating_add(1);
        }
        if t.main_ledger {
            main_ledger = main_ledger.saturating_add(1);
        }
        // 跨三族的唯一性：名字、族+owner 两个维度都不许撞。
        let mut j = i + 1;
        while j < TOKENS.len() {
            let o = TOKENS[j];
            if o.name == t.name || (o.family == t.family && o.owner == t.owner) {
                dup = dup.saturating_add(1);
            }
            j += 1;
        }
        per[fi] = per[fi].saturating_add(1);
        h = p_core::fold_byte(h, t.family.index());
        h = p_core::fold_byte(h, t.owner);
        h = p_core::fold_u32(h, t.name.len() as u32);
        i += 1;
    }
    let l = TokenLedger {
        rows: TOKENS.len() as u32,
        per_family: per,
        dup,
        syntax_bad,
        main_ledger,
        digest: h,
    };
    if l.rows != TOKEN_TOTAL as u32 {
        return Err(SealCode::TokenMissing.val());
    }
    let mut k = 0usize;
    while k < FAMILIES {
        if l.per_family[k] != PER_FAMILY as u32 {
            return Err(SealCode::FamilyShort.val());
        }
        k += 1;
    }
    if l.dup != 0 {
        return Err(SealCode::TokenDup.val());
    }
    if l.syntax_bad != 0 {
        return Err(SealCode::TokenSyntax.val());
    }
    if l.main_ledger != TOKEN_TOTAL as u32 {
        return Err(SealCode::FamilyShort.val());
    }
    if l.digest == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(l)
}

/// 账行语料：30 条令牌各写成一行（同族成对，便于 grep 计数与泄漏检查）。
pub fn ledger_corpus() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < TOKENS.len() {
        let t = TOKENS[i];
        // 每行带两个同族令牌：一条主账 + 它的下一号同伴（首尾相接）。
        let next_owner = if t.owner as usize == PER_FAMILY {
            1
        } else {
            t.owner + 1
        };
        let mate = match token_at(t.family, next_owner) {
            Some(v) => v,
            None => t,
        };
        out.push(format!(
            "{}|{}={};{}={}",
            t.family.name(),
            t.name,
            t.owner,
            mate.name,
            mate.owner
        ));
        i += 1;
    }
    out
}

/// grep 一族：数语料里出现本族令牌名字的行数。
pub fn grep_family(corpus: &[String], family: Family) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < corpus.len() {
        if line_has_family(&corpus[i], family) {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    n
}

/// 一行里是否出现该族的任一令牌名字。
pub fn line_has_family(line: &str, family: Family) -> bool {
    let mut i = 0usize;
    while i < TOKENS.len() {
        let t = TOKENS[i];
        if t.family == family && line.contains(t.name) {
            return true;
        }
        i += 1;
    }
    false
}

/// 一行里出现了哪几族（bit0 focus / bit1 vxwm / bit2 constraint）。
pub fn families_in(line: &str) -> u8 {
    let mut m = 0u8;
    let mut i = 0usize;
    while i < TOKENS.len() {
        let t = TOKENS[i];
        if line.contains(t.name) {
            m |= 1u8 << t.family.index();
        }
        i += 1;
    }
    m
}

/// grep 审计：三族各 10 行、每行恰好一族、零跨族泄漏。
pub fn audit_grep() -> Result<u64, u32> {
    let corpus = ledger_corpus();
    if corpus.len() != TOKEN_TOTAL {
        return Err(SealCode::TokenMissing.val());
    }
    let mut h = p_core::FNV_OFF;
    let mut f = 0u8;
    while f < FAMILIES as u8 {
        let fam = Family::from_index(f)?;
        let n = grep_family(&corpus, fam);
        if n != PER_FAMILY as u32 {
            return Err(SealCode::GrepLeak.val());
        }
        h = p_core::fold_u32(h, n);
        f += 1;
    }
    let mut i = 0usize;
    while i < corpus.len() {
        let m = families_in(&corpus[i]);
        // 恰好一族：m 必须是 1/2/4 之一，绝不能是多族 bit 或零。
        if m != 1 && m != 2 && m != 4 {
            return Err(SealCode::GrepLeak.val());
        }
        h = p_core::fold_byte(h, m);
        i += 1;
    }
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(h)
}

// ── 20 卡与四段分布 ───────────────────────────────────────────────
/// 一张卡的印：序号、卡号、槽位、额度段、行配额、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardStamp {
    pub index: u8,
    pub id: u32,
    pub slot: u32,
    pub tier: u8,
    pub quota: u32,
    pub digest: u64,
}

/// 造一张卡印（纯函数，输入相同则输出相同）。
pub fn stamp_of(index: u8) -> Result<CardStamp, u32> {
    if index as usize >= CARD_TOTAL {
        return Err(SealCode::CardShort.val());
    }
    // 轮转分档：index % 4 → 0..3，保证四段各 5 卡。
    let tier = (index as usize % TIER_QUOTAS.len()) as u8;
    let quota = match TIER_QUOTAS.get(tier as usize) {
        Some(v) => *v,
        None => return Err(SealCode::FreezeBad.val()),
    };
    let id = CARD_ID_BASE
        .checked_add(index as u32)
        .ok_or(SealCode::IdDup.val())?;
    let slot = CARD_SLOT_BASE
        .checked_add((index as u32).saturating_mul(CARD_SLOT_STEP))
        .ok_or(SealCode::IdDup.val())?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, index);
    h = p_core::fold_u32(h, id);
    h = p_core::fold_u32(h, slot);
    h = p_core::fold_byte(h, tier);
    h = p_core::fold_u32(h, quota);
    Ok(CardStamp {
        index,
        id,
        slot,
        tier,
        quota,
        digest: h,
    })
}

/// 卡印全表（20 张，顺序即卡号顺序）。
pub fn all_stamps() -> Result<Vec<CardStamp>, u32> {
    let mut out: Vec<CardStamp> = Vec::new();
    let mut i = 0u8;
    while (i as usize) < CARD_TOTAL {
        out.push(stamp_of(i)?);
        i += 1;
    }
    Ok(out)
}

/// 卡账审计：20 张、卡号连续、槽位不撞、四段各 5 卡、额度合计 72 000。
pub fn audit_cards() -> Result<u64, u32> {
    let cards = all_stamps()?;
    if cards.len() != CARD_TOTAL {
        return Err(SealCode::CardShort.val());
    }
    let mut tier_count = [0u32; 4];
    let mut sum = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < cards.len() {
        let c = cards[i];
        let want_id = match CARD_ID_BASE.checked_add(i as u32) {
            Some(v) => v,
            None => return Err(SealCode::IdDup.val()),
        };
        if c.id != want_id || c.id < CARD_ID_BASE || c.id >= CARD_ID_BASE + 20 {
            return Err(SealCode::IdDup.val());
        }
        let want_slot = match CARD_SLOT_BASE.checked_add((i as u32).saturating_mul(CARD_SLOT_STEP)) {
            Some(v) => v,
            None => return Err(SealCode::IdDup.val()),
        };
        if c.slot != want_slot {
            return Err(SealCode::IdDup.val());
        }
        let mut j = i + 1;
        while j < cards.len() {
            if cards[j].id == c.id || cards[j].slot == c.slot {
                return Err(SealCode::IdDup.val());
            }
            j += 1;
        }
        if (c.tier as usize) >= TIER_QUOTAS.len() {
            return Err(SealCode::FreezeBad.val());
        }
        tier_count[c.tier as usize] = tier_count[c.tier as usize].saturating_add(1);
        sum = sum.saturating_add(c.quota);
        h = p_core::fold_byte(h, c.index);
        h = p_core::fold_u32(h, c.quota);
        h = p_core::fold_u32(h, c.digest as u32);
        i += 1;
    }
    let mut k = 0usize;
    while k < TIER_QUOTAS.len() {
        if tier_count[k] != TIER_CARDS[k] {
            return Err(SealCode::FreezeBad.val());
        }
        k += 1;
    }
    if sum != CARD_ROWS || CARD_ROWS != CARD_ROWS_SUM {
        return Err(SealCode::LineShort.val());
    }
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(h)
}

// ── 十日账 ────────────────────────────────────────────────────────
/// 一日账：日序号、卡行、辅记、合计、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayLedger {
    pub day: u8,
    pub card_lines: u32,
    pub aux_lines: u32,
    pub total: u32,
    pub digest: u64,
}

/// 造一日账（纯函数）。
pub fn day_of(day: u8) -> Result<DayLedger, u32> {
    let idx = day as usize;
    if idx >= DAYS {
        return Err(SealCode::DayGap.val());
    }
    let card_lines = match DAY_LINES.get(idx) {
        Some(v) => *v,
        None => return Err(SealCode::DayGap.val()),
    };
    let total = card_lines
        .checked_add(AUX_PER_DAY)
        .ok_or(SealCode::DayGap.val())?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, day);
    h = p_core::fold_u32(h, card_lines);
    h = p_core::fold_u32(h, AUX_PER_DAY);
    h = p_core::fold_u32(h, total);
    Ok(DayLedger {
        day,
        card_lines,
        aux_lines: AUX_PER_DAY,
        total,
        digest: h,
    })
}

/// 十日全账。
pub fn all_days() -> Result<Vec<DayLedger>, u32> {
    let mut out: Vec<DayLedger> = Vec::new();
    let mut d = 0u8;
    while (d as usize) < DAYS {
        out.push(day_of(d)?);
        d += 1;
    }
    Ok(out)
}

/// 十日审计：日序连续、每日合计对、两侧合计都是 72 000 + 6 000。
pub fn audit_days() -> Result<u64, u32> {
    let days = all_days()?;
    if days.len() != DAYS {
        return Err(SealCode::DayGap.val());
    }
    let mut card_sum = 0u32;
    let mut total_sum = 0u32;
    let mut h = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < days.len() {
        let d = days[i];
        if d.day as usize != i {
            return Err(SealCode::DayGap.val());
        }
        if d.card_lines != DAY_LINES[i] {
            return Err(SealCode::DayGap.val());
        }
        if d.total != d.card_lines.saturating_add(d.aux_lines) {
            return Err(SealCode::DayGap.val());
        }
        card_sum = card_sum.saturating_add(d.card_lines);
        total_sum = total_sum.saturating_add(d.total);
        h = p_core::fold_byte(h, d.day);
        h = p_core::fold_u32(h, d.total);
        h = p_core::fold_u32(h, d.digest as u32);
        i += 1;
    }
    if card_sum != CARD_ROWS {
        return Err(SealCode::DayGap.val());
    }
    if total_sum != BATCH_ROWS {
        return Err(SealCode::LineShort.val());
    }
    if AUX_PER_DAY.saturating_mul(DAYS as u32) != BATCH_ROWS - CARD_ROWS {
        return Err(SealCode::DayGap.val());
    }
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(h)
}

/// 四段分布审计：每段 5 卡 × 段额度，合计 72 000；与日账卡行合计一致。
pub fn audit_tiers() -> Result<u64, u32> {
    let mut h = p_core::FNV_OFF;
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < TIER_QUOTAS.len() {
        let q = TIER_QUOTAS[i];
        let n = TIER_CARDS[i];
        let part = q.checked_mul(n).ok_or(SealCode::FreezeBad.val())?;
        sum = sum.saturating_add(part);
        h = p_core::fold_u32(h, q);
        h = p_core::fold_u32(h, n);
        h = p_core::fold_u32(h, part);
        i += 1;
    }
    if sum != CARD_ROWS {
        return Err(SealCode::LineShort.val());
    }
    if sum != audit_days_card_part()? {
        return Err(SealCode::DayGap.val());
    }
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(h)
}

/// 日账卡行合计（四段审计的对照物）。
fn audit_days_card_part() -> Result<u32, u32> {
    let days = all_days()?;
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < days.len() {
        sum = sum.saturating_add(days[i].card_lines);
        i += 1;
    }
    Ok(sum)
}

// ── 收官印 ────────────────────────────────────────────────────────
/// 预算：卡数 / 行数 / 域账 / 天数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Budget {
    pub cards: u32,
    pub rows: u32,
    pub domain: u32,
    pub days: u32,
}

impl Budget {
    /// 本批预算（写死口径，任何改动都会让印对不上）。
    pub fn b13() -> Budget {
        Budget {
            cards: CARD_TOTAL as u32,
            rows: BATCH_ROWS,
            domain: DOMAIN_ROWS,
            days: DAYS as u32,
        }
    }

    /// 占比基点 = rows × 10^4 / domain（本批整除）。
    pub fn ratio_bp(&self) -> Result<u32, u32> {
        if self.domain == 0 {
            return Err(SealCode::RatioBad.val());
        }
        let scaled = self.rows.checked_mul(10_000).ok_or(SealCode::RatioBad.val())?;
        let bp = scaled / self.domain;
        if scaled % self.domain != 0 {
            return Err(SealCode::RatioBad.val());
        }
        Ok(bp)
    }
}

/// 收官印：卡数 / 行数（卡行 + 辅记）/ 占比 / 天数 / 三线摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seal {
    pub cards: u32,
    pub card_rows: u32,
    pub aux_rows: u32,
    pub rows: u32,
    pub ratio_bp: u32,
    pub days: u32,
    pub token_digest: u64,
    pub card_digest: u64,
    pub day_digest: u64,
    pub digest: u64,
}

/// 冻结：把令牌账、卡账、日账三线折成一方印。
pub fn freeze() -> Result<Seal, u32> {
    let tk = audit_tokens()?;
    let tp = audit_grep()?;
    let cd = audit_cards()?;
    let dy = audit_days()?;
    let tier = audit_tiers()?;
    let b = Budget::b13();
    let ratio = b.ratio_bp()?;
    if ratio != RATIO_BP {
        return Err(SealCode::RatioBad.val());
    }
    if tk.rows != TOKEN_TOTAL as u32 {
        return Err(SealCode::TokenMissing.val());
    }
    if cd == 0 || dy == 0 || tp == 0 || tier == 0 {
        return Err(SealCode::SealBroken.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, CARD_TOTAL as u32);
    h = p_core::fold_u32(h, BATCH_ROWS);
    h = p_core::fold_u32(h, ratio);
    h = p_core::fold_u32(h, tk.digest as u32);
    h = p_core::fold_u32(h, (tk.digest >> 32) as u32);
    h = p_core::fold_u32(h, tp as u32);
    h = p_core::fold_u32(h, cd as u32);
    h = p_core::fold_u32(h, dy as u32);
    h = p_core::fold_u32(h, tier as u32);
    if h == 0 {
        return Err(SealCode::SealBroken.val());
    }
    Ok(Seal {
        cards: CARD_TOTAL as u32,
        card_rows: CARD_ROWS,
        aux_rows: AUX_PER_DAY.saturating_mul(DAYS as u32),
        rows: BATCH_ROWS,
        ratio_bp: ratio,
        days: DAYS as u32,
        token_digest: tk.digest,
        card_digest: cd,
        day_digest: dy,
        digest: h,
    })
}

/// 冻结幂等：冻两次，两方印必须逐字段相等。
pub fn freeze_twice() -> Result<Seal, u32> {
    let a = freeze()?;
    let b = freeze()?;
    if a != b {
        return Err(SealCode::StampDrift.val());
    }
    Ok(a)
}

/// 交付账：印 + 预算核对结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Delivery {
    pub seal: Seal,
    pub budget: Budget,
    pub lines_ok: bool,
    pub ratio_ok: bool,
    pub days_ok: bool,
    pub digest: u64,
}

/// 交付：印与预算逐项对账，任何一项不符都拒交。
pub fn deliver() -> Result<Delivery, u32> {
    let seal = freeze_twice()?;
    let b = Budget::b13();
    let lines_ok = seal.rows == b.rows
        && seal.card_rows.saturating_add(seal.aux_rows) == b.rows
        && seal.card_rows == CARD_ROWS;
    let ratio_ok = seal.ratio_bp == b.ratio_bp()? && seal.ratio_bp == RATIO_BP;
    let days_ok = seal.days == b.days && seal.days == DAYS as u32;
    if !lines_ok {
        return Err(SealCode::LineShort.val());
    }
    if !ratio_ok {
        return Err(SealCode::RatioBad.val());
    }
    if !days_ok {
        return Err(SealCode::DayGap.val());
    }
    if seal.cards != b.cards {
        return Err(SealCode::CardShort.val());
    }
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, seal.digest as u32);
    h = p_core::fold_u32(h, (seal.digest >> 32) as u32);
    h = p_core::fold_u32(h, b.rows);
    h = p_core::fold_u32(h, seal.ratio_bp);
    if h == 0 {
        return Err(SealCode::DeliveryBad.val());
    }
    Ok(Delivery {
        seal,
        budget: b,
        lines_ok,
        ratio_ok,
        days_ok,
        digest: h,
    })
}

// ── 六字段账行：渲染与回读 ────────────────────────────────────────
/// 六字段账行。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|k1={}|k2={}|k3={}|k4={}|digest={:016X}",
        tag, a, b, c, d, digest
    )
}

/// 解析十进制 u32。
pub fn parse_u32(s: &str) -> Option<u32> {
    s.parse::<u32>().ok()
}

/// 解析 16 位大写十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = *s.as_bytes().get(i)?;
        if !((b'0'..=b'9').contains(&b) || (b'A'..=b'F').contains(&b)) {
            return None;
        }
        i += 1;
    }
    u64::from_str_radix(s, 16).ok()
}

/// 回读账行。
pub fn row_fields(row: &str) -> Option<(&str, [u32; 4], u64)> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != RENDER_FIELDS {
        return None;
    }
    let tag = *parts.first()?;
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < 4 {
        let seg = *parts.get(i + 1)?;
        let body = seg.strip_prefix('k')?;
        let eq = body.find('=')?;
        let (kn, vn) = body.split_at(eq);
        if parse_u32(kn)? != i as u32 + 1 {
            return None;
        }
        vals[i] = parse_u32(vn.strip_prefix('=')?)?;
        i += 1;
    }
    let last = *parts.get(5)?;
    let dg = parse_hex16(last.strip_prefix("digest=")?)?;
    Some((tag, vals, dg))
}

/// 账行自审。
pub fn audit_render(row: &str) -> bool {
    match row_fields(row) {
        Some((tag, _, dg)) => {
            let known = tag == TOKEN_TAG
                || tag == SEAL_TAG
                || tag == DAY_TAG
                || tag == CARD_TAG
                || tag == CODE_TAG;
            known && dg != 0
        }
        None => false,
    }
}

/// 令牌账：族 / 序号 / 名字长度 / 主账 / 摘要。
pub fn token_row(t: &Token) -> String {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_byte(h, t.family.index());
    h = p_core::fold_byte(h, t.owner);
    h = p_core::fold_u32(h, t.name.len() as u32);
    h = p_core::fold_byte(h, if t.main_ledger { 1 } else { 0 });
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    render_row(
        TOKEN_TAG,
        t.family.index() as u32,
        t.owner as u32,
        t.name.len() as u32,
        if t.main_ledger { 1 } else { 0 },
        h,
    )
}

/// 卡账：卡号 / 槽位 / 段 / 额度 / 摘要。
pub fn card_row(c: &CardStamp) -> String {
    render_row(CARD_TAG, c.id, c.slot, c.tier as u32, c.quota, c.digest)
}

/// 日账：日 / 卡行 / 辅记 / 合计 / 摘要。
pub fn day_row(d: &DayLedger) -> String {
    render_row(DAY_TAG, d.day as u32, d.card_lines, d.aux_lines, d.total, d.digest)
}

/// 收官印账：卡数 / 卡行 / 总行 / 占比 / 摘要。
pub fn seal_row(s: &Seal) -> String {
    render_row(SEAL_TAG, s.cards, s.card_rows, s.rows, s.ratio_bp, s.digest)
}

/// 码族账。
pub fn code_row(idx: usize) -> Option<String> {
    let r = snapshot_row(idx)?;
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, r.code);
    h = p_core::fold_u32(h, r.cn.len() as u32);
    if h == 0 {
        h = p_core::FNV_OFF;
    }
    Some(render_row(CODE_TAG, r.code, r.cn.len() as u32, idx as u32, 0, h))
}

// ── 编号纪律 ──────────────────────────────────────────────────────
/// B12 已占用的优先槽位（本模块 16 个编号必须与之零交集）。
pub const RESERVED_IDS: [u32; 6] = [
    0x071C_8001,
    0x071C_8002,
    0x071C_8014,
    0x071D_0001,
    0x071D_2001,
    0x071D_2014,
];

/// 本模块 16 个跨槽编号（0x0734_1001..0x0734_1010）。
pub const CASE_IDS: [u32; SLOT_CASE_TOTAL] = [
    0x0734_1001,
    0x0734_1002,
    0x0734_1003,
    0x0734_1004,
    0x0734_1005,
    0x0734_1006,
    0x0734_1007,
    0x0734_1008,
    0x0734_1009,
    0x0734_100A,
    0x0734_100B,
    0x0734_100C,
    0x0734_100D,
    0x0734_100E,
    0x0734_100F,
    0x0734_1010,
];

/// 按序号取跨槽编号。
pub fn case_id(i: usize) -> Option<u32> {
    CASE_IDS.get(i).copied()
}

/// 跨槽编号严格铺满 CASE_BASE+0x01..+0x10。
pub fn case_ids_tile() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let want = match CASE_BASE.checked_add(1 + i as u32) {
            Some(v) => v,
            None => return false,
        };
        if CASE_IDS[i] != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 十六个编号与六个预留槽位零交集，且自身无重复。
pub fn no_cross_collision() -> bool {
    let mut i = 0usize;
    while i < CASE_IDS.len() {
        let a = CASE_IDS[i];
        let mut j = 0usize;
        while j < RESERVED_IDS.len() {
            if a == RESERVED_IDS[j] {
                return false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < CASE_IDS.len() {
            if a == CASE_IDS[k] {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针甲：码族快照 + 段位 + 令牌账 + grep 零泄漏。
pub fn probe_seq_a() -> Result<(), u32> {
    let _d = audit_code_snapshot()?;
    if !segment_guard() {
        return Err(SealCode::SealBroken.val());
    }
    let t = audit_tokens()?;
    if t.rows != TOKEN_TOTAL as u32 || t.main_ledger != TOKEN_TOTAL as u32 {
        return Err(SealCode::TokenMissing.val());
    }
    let _g = audit_grep()?;
    if grep_family(&ledger_corpus(), Family::Focus) != PER_FAMILY as u32 {
        return Err(SealCode::GrepLeak.val());
    }
    Ok(())
}

/// 探针乙：卡账 + 四段分布 + 十日账，两条切法必须落在同一个 72 000。
pub fn probe_seq_b() -> Result<(), u32> {
    let _c = audit_cards()?;
    let _t = audit_tiers()?;
    let _d = audit_days()?;
    let days_card = audit_days_card_part()?;
    let mut tier_sum = 0u32;
    let mut i = 0usize;
    while i < TIER_QUOTAS.len() {
        tier_sum = tier_sum
            .saturating_add(TIER_QUOTAS[i].saturating_mul(TIER_CARDS[i]));
        i += 1;
    }
    if tier_sum != days_card || tier_sum != CARD_ROWS {
        return Err(SealCode::DayGap.val());
    }
    Ok(())
}

/// 探针丙：印幂等 + 交付对账 + 编号纪律。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = freeze()?;
    let b = freeze_twice()?;
    if a != b || a.digest == 0 {
        return Err(SealCode::StampDrift.val());
    }
    if a.rows != BATCH_ROWS || a.ratio_bp != RATIO_BP {
        return Err(SealCode::RatioBad.val());
    }
    let d = deliver()?;
    if !d.lines_ok || !d.ratio_ok || !d.days_ok {
        return Err(SealCode::DeliveryBad.val());
    }
    if !case_ids_tile() || !no_cross_collision() {
        return Err(SealCode::IdDup.val());
    }
    Ok(())
}

/// 复跑自证。
pub fn repro_check() -> Result<(), u32> {
    probe_seq_a()?;
    probe_seq_b()?;
    probe_seq_c()?;
    Ok(())
}

// ── 八项检查 ──────────────────────────────────────────────────────
/// 八项检查：四条防重（F49059）+ 四条收官（F49060）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    let tk = audit_tokens();
    out.push((
        "F49059/三族30令牌防重",
        tk.is_ok(),
        match tk {
            Ok(l) => format!("rows={} dup={} main={}", l.rows, l.dup, l.main_ledger),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let syn = TOKENS.iter().all(|t| syntax_ok(t.name, t.family.upper()));
    out.push((
        "F49059/词法分族合规",
        syn,
        format!(
            "focus_upper={} vxwm_lower={} constraint_upper={}",
            syntax_ok("FOCUS_OWNER_01", true),
            syntax_ok("vxwm_owner_01", false),
            syntax_ok("CONSTRAINT_OWNER_01", true)
        ),
    ));

    let gp = audit_grep();
    let corpus = ledger_corpus();
    let counts = [
        grep_family(&corpus, Family::Focus),
        grep_family(&corpus, Family::Vxwm),
        grep_family(&corpus, Family::Constraint),
    ];
    out.push((
        "F49059/grep零跨族泄漏",
        gp.is_ok() && counts == [10, 10, 10],
        format!("focus={} vxwm={} constraint={} die={:?}", counts[0], counts[1], counts[2], gp.is_ok()),
    ));

    let b = Budget::b13();
    out.push((
        "F49059/批印入册78000",
        b.rows == BATCH_ROWS && b.domain == DOMAIN_ROWS && b.ratio_bp().ok() == Some(RATIO_BP),
        format!(
            "rows={} domain={} bp={:?}",
            b.rows,
            b.domain,
            b.ratio_bp().ok()
        ),
    ));

    let cd = audit_cards();
    let tier_ok = {
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < TIER_QUOTAS.len() {
            sum = sum.saturating_add(TIER_QUOTAS[k].saturating_mul(TIER_CARDS[k]));
            k += 1;
        }
        sum == CARD_ROWS
    };
    out.push((
        "F49060/20卡配额守恒",
        cd.is_ok() && tier_ok && CARD_ROWS == CARD_ROWS_SUM,
        format!(
            "cards={} card_rows={} sum={}",
            CARD_TOTAL, CARD_ROWS, CARD_ROWS_SUM
        ),
    ));

    let tr = audit_tiers();
    out.push((
        "F49060/四段分布可复算",
        tr.is_ok() && TIER_QUOTAS == [4200, 3600, 3400, 3200] && TIER_CARDS == [5, 5, 5, 5],
        format!(
            "tiers={:?} cards={:?} ok={}",
            TIER_QUOTAS,
            TIER_CARDS,
            tr.is_ok()
        ),
    ));

    let dy = audit_days();
    let days = match all_days() {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut day_sum = 0u32;
    let mut card_part = 0u32;
    let mut i = 0usize;
    while i < days.len() {
        day_sum = day_sum.saturating_add(days[i].total);
        card_part = card_part.saturating_add(days[i].card_lines);
        i += 1;
    }
    out.push((
        "F49060/十日账两侧一致",
        dy.is_ok()
            && day_sum == BATCH_ROWS
            && card_part == CARD_ROWS
            && days.len() == DAYS,
        format!(
            "days={} card_part={} total={}",
            days.len(),
            card_part,
            day_sum
        ),
    ));

    let fz = freeze_twice();
    out.push((
        "F49060/收官印幂等",
        fz.is_ok() && deliver().is_ok(),
        match fz {
            Ok(s) => format!(
                "rows={} bp={} digest={:016X}",
                s.rows, s.ratio_bp, s.digest
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

/// 汇总检查集（八项动态检查）。
pub fn run_unx49059_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49059 B13 批防重与批收官印");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        set.add_dyn(items[i].0, items[i].1, &items[i].2);
        i += 1;
    }
    set
}

// ── 测试 ──────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_closed_and_contiguous() {
        assert_eq!(SealCode::ALL.len(), 14);
        assert!(segment_guard());
        let mut i = 0usize;
        while i < SealCode::ALL.len() {
            let c = SealCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(known_code(c.val()));
            assert_eq!(detail(c.val()), c.detail());
            i += 1;
        }
        assert!(!known_code(SEG_BASE + 0x0F));
    }

    #[test]
    fn unknown_code_falls_back() {
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0xFFFF), UNKNOWN);
    }

    #[test]
    fn snapshot_audits_clean() {
        let d = audit_code_snapshot().unwrap();
        assert_ne!(d, 0);
        assert_eq!(CODE_SNAPSHOT.len(), 15);
        assert_eq!(CODE_SNAPSHOT[14].code, SEG_BASE + 0x0F);
        assert_eq!(CODE_SNAPSHOT[14].cn, UNKNOWN);
    }

    #[test]
    fn token_catalog_is_thirty() {
        assert_eq!(TOKENS.len(), 30);
        let l = audit_tokens().unwrap();
        assert_eq!(l.rows, 30);
        assert_eq!(l.per_family, [10, 10, 10]);
        assert_eq!(l.dup, 0);
        assert_eq!(l.syntax_bad, 0);
        assert_eq!(l.main_ledger, 30);
        assert_ne!(l.digest, 0);
    }

    #[test]
    fn token_lookup_by_name_and_slot() {
        assert_eq!(token_by_name("FOCUS_OWNER_01").unwrap().family, Family::Focus);
        assert_eq!(token_by_name("vxwm_owner_10").unwrap().owner, 10);
        assert_eq!(token_by_name("CONSTRAINT_OWNER_05").unwrap().family, Family::Constraint);
        assert!(token_by_name("nope").is_none());
        assert!(token_at(Family::Focus, 11).is_none());
        assert!(token_at(Family::Vxwm, 0).is_none());
    }

    #[test]
    fn syntax_family_rules() {
        assert!(syntax_ok("FOCUS_OWNER_01", true));
        assert!(syntax_ok("vxwm_owner_01", false));
        assert!(!syntax_ok("FOCUS_OWNER_01", false));
        assert!(!syntax_ok("vxwm_owner_01", true));
        assert!(!syntax_ok("FOCUS__X", true));
        assert!(!syntax_ok("_FOCUS", true));
        assert!(!syntax_ok("FOCUS_", true));
        assert!(!syntax_ok("FOC", true));
        assert!(!syntax_ok("focus_owner_01", true));
    }

    #[test]
    fn family_index_roundtrip() {
        let mut i = 0u8;
        while i < 3 {
            let f = Family::from_index(i).unwrap();
            assert_eq!(f.index(), i);
            assert!(!f.name().is_empty());
            i += 1;
        }
        assert!(Family::from_index(3).is_err());
        assert!(Family::from_index(255).is_err());
    }

    #[test]
    fn corpus_is_thirty_lines_one_family_each() {
        let c = ledger_corpus();
        assert_eq!(c.len(), 30);
        let mut i = 0usize;
        while i < c.len() {
            let m = families_in(&c[i]);
            assert!(m == 1 || m == 2 || m == 4, "line {} mask={}", i, m);
            i += 1;
        }
    }

    #[test]
    fn grep_counts_ten_per_family() {
        let c = ledger_corpus();
        assert_eq!(grep_family(&c, Family::Focus), 10);
        assert_eq!(grep_family(&c, Family::Vxwm), 10);
        assert_eq!(grep_family(&c, Family::Constraint), 10);
        let h = audit_grep().unwrap();
        assert_ne!(h, 0);
    }

    #[test]
    fn cross_family_line_is_detected() {
        let line = "FOCUS_OWNER_01=vxwm_owner_02".to_string();
        assert!(line_has_family(&line, Family::Focus));
        assert!(line_has_family(&line, Family::Vxwm));
        assert_eq!(families_in(&line), 3);
        // 混合行的掩码不是单族，正是 grep 审计要拒的形状。
        assert!(families_in(&line) != 1 && families_in(&line) != 2 && families_in(&line) != 4);
    }

    #[test]
    fn cards_are_twenty_and_tile() {
        let cards = all_stamps().unwrap();
        assert_eq!(cards.len(), 20);
        let mut i = 0usize;
        while i < cards.len() {
            assert_eq!(cards[i].id, CARD_ID_BASE + i as u32);
            assert_eq!(cards[i].slot, CARD_SLOT_BASE + (i as u32) * CARD_SLOT_STEP);
            assert_eq!(cards[i].tier as usize, i % 4);
            assert_eq!(cards[i].quota, TIER_QUOTAS[i % 4]);
            assert_ne!(cards[i].digest, 0);
            i += 1;
        }
        assert!(stamp_of(20).is_err());
        assert!(stamp_of(255).is_err());
    }

    #[test]
    fn card_ids_cover_f49041_to_f49060() {
        let cards = all_stamps().unwrap();
        assert_eq!(cards[0].id, 49_041);
        assert_eq!(cards[19].id, 49_060);
        assert_eq!(49_060 - 49_041 + 1, 20);
    }

    #[test]
    fn four_tiers_five_cards_each() {
        let mut per = [0u32; 4];
        let cards = all_stamps().unwrap();
        let mut i = 0usize;
        while i < cards.len() {
            per[cards[i].tier as usize] += 1;
            i += 1;
        }
        assert_eq!(per, [5, 5, 5, 5]);
    }

    #[test]
    fn card_quotas_sum_to_72000() {
        let cards = all_stamps().unwrap();
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < cards.len() {
            sum += cards[i].quota;
            i += 1;
        }
        assert_eq!(sum, 72_000);
        assert_eq!(sum, CARD_ROWS);
        assert_eq!(CARD_ROWS, CARD_ROWS_SUM);
    }

    #[test]
    fn card_audit_is_green() {
        let h = audit_cards().unwrap();
        assert_ne!(h, 0);
        assert_eq!(TIER_CARDS, [5, 5, 5, 5]);
    }

    #[test]
    fn day_ledger_is_ten_days() {
        let days = all_days().unwrap();
        assert_eq!(days.len(), 10);
        let mut i = 0usize;
        while i < days.len() {
            assert_eq!(days[i].day as usize, i);
            assert_eq!(days[i].card_lines, DAY_LINES[i]);
            assert_eq!(days[i].aux_lines, AUX_PER_DAY);
            assert_eq!(days[i].total, DAY_LINES[i] + AUX_PER_DAY);
            assert_ne!(days[i].digest, 0);
            i += 1;
        }
        assert!(day_of(10).is_err());
        assert!(day_of(255).is_err());
    }

    #[test]
    fn ten_day_sums_are_78000() {
        let days = all_days().unwrap();
        let mut card = 0u32;
        let mut total = 0u32;
        let mut i = 0usize;
        while i < days.len() {
            card += days[i].card_lines;
            total += days[i].total;
            i += 1;
        }
        assert_eq!(card, 72_000);
        assert_eq!(total, 78_000);
        assert_eq!(total, BATCH_ROWS);
        assert_eq!(AUX_PER_DAY * 10, 6_000);
    }

    #[test]
    fn day_audit_is_green() {
        let h = audit_days().unwrap();
        assert_ne!(h, 0);
    }

    #[test]
    fn tier_audit_cross_checks_days() {
        let h = audit_tiers().unwrap();
        assert_ne!(h, 0);
        assert_eq!(audit_days_card_part().unwrap(), CARD_ROWS);
    }

    #[test]
    fn budget_ratio_is_3250_bp() {
        let b = Budget::b13();
        assert_eq!(b.cards, 20);
        assert_eq!(b.rows, 78_000);
        assert_eq!(b.domain, 240_000);
        assert_eq!(b.days, 10);
        assert_eq!(b.ratio_bp().unwrap(), 3250);
        assert_eq!(RATIO_BP, 3250);
    }

    #[test]
    fn ratio_rejects_zero_domain() {
        let b = Budget {
            cards: 1,
            rows: 1,
            domain: 0,
            days: 1,
        };
        assert!(b.ratio_bp().is_err());
    }

    #[test]
    fn seal_freezes_and_is_idempotent() {
        let a = freeze().unwrap();
        let b = freeze_twice().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.cards, 20);
        assert_eq!(a.card_rows, 72_000);
        assert_eq!(a.aux_rows, 6_000);
        assert_eq!(a.rows, 78_000);
        assert_eq!(a.ratio_bp, 3250);
        assert_eq!(a.days, 10);
        assert_ne!(a.token_digest, 0);
        assert_ne!(a.card_digest, 0);
        assert_ne!(a.day_digest, 0);
        assert_ne!(a.digest, 0);
    }

    #[test]
    fn delivery_reconciles_budget() {
        let d = deliver().unwrap();
        assert!(d.lines_ok);
        assert!(d.ratio_ok);
        assert!(d.days_ok);
        assert_eq!(d.seal.rows, d.budget.rows);
        assert_eq!(d.seal.ratio_bp, 3250);
        assert_ne!(d.digest, 0);
    }

    #[test]
    fn seal_row_roundtrip() {
        let s = freeze().unwrap();
        let row = seal_row(&s);
        assert!(audit_render(&row));
        let (tag, vals, dg) = row_fields(&row).unwrap();
        assert_eq!(tag, SEAL_TAG);
        assert_eq!(vals, [20, 72_000, 78_000, 3250]);
        assert_eq!(dg, s.digest);
    }

    #[test]
    fn card_row_roundtrip() {
        let cards = all_stamps().unwrap();
        let row = card_row(&cards[0]);
        let (tag, vals, _) = row_fields(&row).unwrap();
        assert_eq!(tag, CARD_TAG);
        assert_eq!(vals[0], 49_041);
        assert_eq!(vals[1], CARD_SLOT_BASE);
        assert_eq!(vals[2], 0);
        assert_eq!(vals[3], 4200);
    }

    #[test]
    fn day_row_roundtrip() {
        let d = day_of(0).unwrap();
        let row = day_row(&d);
        let (tag, vals, _) = row_fields(&row).unwrap();
        assert_eq!(tag, DAY_TAG);
        assert_eq!(vals, [0, 7800, 600, 8400]);
    }

    #[test]
    fn token_row_roundtrip() {
        let t = token_by_name("vxwm_owner_03").unwrap();
        let row = token_row(&t);
        let (tag, vals, _) = row_fields(&row).unwrap();
        assert_eq!(tag, TOKEN_TAG);
        assert_eq!(vals[0], 1);
        assert_eq!(vals[1], 3);
        assert_eq!(vals[3], 1);
    }

    #[test]
    fn code_rows_all_parse() {
        let mut i = 0usize;
        while i < CODE_ROWS {
            let row = code_row(i).unwrap();
            let (tag, vals, _) = row_fields(&row).unwrap();
            assert_eq!(tag, CODE_TAG);
            assert_eq!(vals[0], CODE_SNAPSHOT[i].code);
            i += 1;
        }
        assert!(code_row(CODE_ROWS).is_none());
    }

    #[test]
    fn render_rejects_malformed() {
        assert!(!audit_render(""));
        assert!(!audit_render("SEAL|k1=1"));
        assert!(parse_u32("00012").is_some());
        assert!(parse_u32("-3").is_none());
        assert!(parse_hex16("00000000000000FF").is_some());
        assert!(parse_hex16("000000000000000f").is_none());
        let good = render_row(SEAL_TAG, 1, 2, 3, 4, 0x1);
        assert!(!audit_render(&good.replace("k3=", "k7=")));
        assert!(!audit_render(&render_row(SEAL_TAG, 1, 2, 3, 4, 0)));
        assert!(!audit_render(&render_row("NOPE", 1, 2, 3, 4, 0x1)));
    }

    #[test]
    fn case_ids_tile_and_do_not_collide() {
        assert!(case_ids_tile());
        assert!(no_cross_collision());
        assert_eq!(CASE_IDS.len(), 16);
        assert_eq!(CASE_IDS[0], 0x0734_1001);
        assert_eq!(CASE_IDS[15], 0x0734_1010);
        assert_eq!(case_id(0), Some(0x0734_1001));
        assert_eq!(case_id(16), None);
    }

    #[test]
    fn reserved_ids_are_six_distinct() {
        assert_eq!(RESERVED_IDS.len(), 6);
        let mut i = 0usize;
        while i < RESERVED_IDS.len() {
            let mut j = i + 1;
            while j < RESERVED_IDS.len() {
                assert_ne!(RESERVED_IDS[i], RESERVED_IDS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn repro_is_deterministic() {
        assert!(repro_check().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn checks_are_eight_and_green() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            assert!(items[i].1, "red check: {} {}", items[i].0, items[i].2);
            i += 1;
        }
        let s = run_unx49059_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn arithmetic_reconciles_both_ways() {
        // 横切：四段 × 5 卡。
        let mut tier = 0u32;
        let mut i = 0usize;
        while i < TIER_QUOTAS.len() {
            tier += TIER_QUOTAS[i] * TIER_CARDS[i];
            i += 1;
        }
        // 竖切：十日卡行。
        let mut day = 0u32;
        let mut k = 0usize;
        while k < DAY_LINES.len() {
            day += DAY_LINES[k];
            k += 1;
        }
        assert_eq!(tier, day);
        assert_eq!(tier, 72_000);
        assert_eq!(tier + 6000, 78_000);
        assert_eq!(78_000u32 * 10_000 / 240_000, 3250);
        assert_eq!(78_000u32 * 10_000 % 240_000, 0);
    }

    #[test]
    fn token_digest_changes_if_name_shape_changes() {
        // 只读旁证：词法检查对大小写与下划线敏感，任何一次手误都能被抓到。
        assert_ne!(
            syntax_ok("FOCUS_OWNER_01", true),
            syntax_ok("FOCUS_OWNER_01_", true)
        );
        assert_ne!(syntax_ok("FOCUS_OWNER_01", true), syntax_ok("FOCUS_OWNER_", true));
        assert_ne!(syntax_ok("vxwm_owner_01", false), syntax_ok("VXWM_OWNER_01", false));
    }

    #[test]
    fn seal_fields_sum_consistently() {
        let s = freeze().unwrap();
        assert_eq!(s.rows, s.card_rows + s.aux_rows);
        assert_eq!(s.aux_rows, AUX_PER_DAY * DAYS as u32);
        assert_eq!(s.rows * 10_000 / DOMAIN_ROWS, s.ratio_bp);
    }

    #[test]
    fn token_names_are_family_prefixed() {
        let mut i = 0usize;
        while i < TOKENS.len() {
            let t = TOKENS[i];
            match t.family {
                Family::Focus => assert!(t.name.starts_with("FOCUS_")),
                Family::Vxwm => assert!(t.name.starts_with("vxwm_")),
                Family::Constraint => assert!(t.name.starts_with("CONSTRAINT_")),
            }
            // 跨族前缀不互串：focus 的账行里不许出现 vxwm/constraint 前缀。
            let line = t.ledger_line();
            match t.family {
                Family::Focus => {
                    assert!(!line.contains("vxwm_"));
                    assert!(!line.contains("CONSTRAINT_"));
                }
                Family::Vxwm => {
                    assert!(!line.contains("FOCUS_"));
                    assert!(!line.contains("CONSTRAINT_"));
                }
                Family::Constraint => {
                    assert!(!line.contains("FOCUS_"));
                    assert!(!line.contains("vxwm_"));
                }
            }
            i += 1;
        }
    }

    #[test]
    fn token_owner_ordinals_tile_per_family() {
        let mut f = 0u8;
        while f < FAMILIES as u8 {
            let fam = Family::from_index(f).unwrap();
            let mut owner = 1u8;
            while owner as usize <= PER_FAMILY {
                let t = token_at(fam, owner).unwrap();
                assert_eq!(t.family, fam);
                assert_eq!(t.owner, owner);
                assert!(t.main_ledger);
                owner += 1;
            }
            assert!(token_at(fam, PER_FAMILY as u8 + 1).is_none());
            f += 1;
        }
    }

    #[test]
    fn day_lines_alternate_and_sum() {
        let mut hi = 0u32;
        let mut lo = 0u32;
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < DAY_LINES.len() {
            if DAY_LINES[i] == 7800 {
                hi += 1;
            }
            if DAY_LINES[i] == 6600 {
                lo += 1;
            }
            sum += DAY_LINES[i];
            i += 1;
        }
        assert_eq!(hi, 5);
        assert_eq!(lo, 5);
        assert_eq!(sum, CARD_ROWS);
        assert_eq!(sum + AUX_PER_DAY * DAYS as u32, BATCH_ROWS);
    }

    #[test]
    fn delivery_is_idempotent() {
        let a = deliver().unwrap();
        let b = deliver().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.seal.digest, b.seal.digest);
        assert_ne!(a.digest, 0);
    }
}
