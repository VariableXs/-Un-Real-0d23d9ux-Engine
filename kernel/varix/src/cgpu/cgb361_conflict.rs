//! CGPU-F0361 · 多场景冲突消解（判据段 0x08F0）
//! 冲突消解：多配方冲突矩阵（两配方争夺同一池的消解规则——用户
//! 可见性优先序）；冲突显性（冲突发生即记录——归因到配方对）；
//! 消解策略可配（默认序+自定义序）；案例库（典型冲突场景与消解
//! 示例——持续沉淀）。判据：冲突矩阵、显性、可配、案例库、判据。
//!
//! 锚点原文摘录（施工书 1607 行）：
//! 「多配方冲突矩阵（两配方争夺同一池的消解规则——用户可见性优
//! 先序）；冲突显性（冲突发生即记录——归因到配方对）；消解策略
//! 可配（默认序+自定义序）；案例库（典型冲突场景与消解示例）。
//! 判据：冲突矩阵、显性、可配、案例库、判据。」
//!
//! 诊断码段 0x08F0（本模块独占：0x08F0~0x08FF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x08F0;

/// 配方族闭集（判据「冲突矩阵」的行/列维——三族配方 + 仲裁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecipeKind {
    /// 视频配方（F0357）。
    Video,
    /// 网页配方（F0358）。
    Web,
    /// 全屏配方（F0359）。
    Fullscreen,
}

impl RecipeKind {
    /// 闭集遍历。
    pub fn all() -> [RecipeKind; 3] {
        [RecipeKind::Video, RecipeKind::Web, RecipeKind::Fullscreen]
    }
    /// 族号（矩阵下标）。
    pub fn ordinal(&self) -> usize {
        match self {
            RecipeKind::Video => 0,
            RecipeKind::Web => 1,
            RecipeKind::Fullscreen => 2,
        }
    }
    /// 名称。
    pub fn name(&self) -> &'static str {
        match self {
            RecipeKind::Video => "video",
            RecipeKind::Web => "web",
            RecipeKind::Fullscreen => "fullscreen",
        }
    }
}

/// 消解裁决（判据「冲突矩阵」的格值）：谁优先、谁让路。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 前者（行配方）优先。
    FirstWins,
    /// 后者（列配方）优先。
    SecondWins,
    /// 对半分（无天然优先序时）。
    SplitHalf,
}

/// 冲突矩阵（判据「冲突矩阵」主判据）：3×3 对称半表——用户可见性
/// 优先序：全屏 > 视频 > 网页（用户正盯着的东西最优先）。
/// 对角线 = 同族冲突，恒 SplitHalf（同族内部走各自配额表）。
pub fn conflict_verdict(a: RecipeKind, b: RecipeKind) -> Result<Verdict, u16> {
    if a == b {
        return Ok(Verdict::SplitHalf);
    }
    // 可见性优先序号：全屏 3 > 视频 2 > 网页 1。
    let pri = |k: RecipeKind| match k {
        RecipeKind::Fullscreen => 3u8,
        RecipeKind::Video => 2,
        RecipeKind::Web => 1,
    };
    match pri(a).cmp(&pri(b)) {
        core::cmp::Ordering::Greater => Ok(Verdict::FirstWins),
        core::cmp::Ordering::Less => Ok(Verdict::SecondWins),
        core::cmp::Ordering::Equal => Err(SEG_BASE),
    }
}

/// 冲突记录（判据「显性」一等公民）：归因到配方对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictRecord {
    /// 行配方。
    pub a: RecipeKind,
    /// 列配方。
    pub b: RecipeKind,
    /// 争夺的池号。
    pub pool: u32,
    /// A 诉求（千分比）。
    pub want_a: u32,
    /// B 诉求（千分比）。
    pub want_b: u32,
}

impl ConflictRecord {
    /// 是否真冲突：诉求和 > 池容量 1000。
    pub fn is_conflict(&self) -> bool {
        self.want_a + self.want_b > 1000
    }
    /// 归因键（配方对规范化——小号在前，同一冲突唯一归因）。
    pub fn pair_key(&self) -> (u8, u8) {
        let (x, y) = (self.a.ordinal() as u8, self.b.ordinal() as u8);
        if x <= y {
            (x, y)
        } else {
            (y, x)
        }
    }
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.pool == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.want_a == 0 || self.want_b == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 消解策略序（判据「可配」的配置对象）：默认序 = 可见性优先；
/// 自定义序 = 调用方给定的优先序（必须是三族的一个排列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolveOrder {
    /// 优先序（从高到低），必须是三族排列。
    pub order: [RecipeKind; 3],
    /// 是否自定义。
    pub custom: bool,
}

impl ResolveOrder {
    /// 默认序：全屏 > 视频 > 网页。
    pub fn default_order() -> Self {
        ResolveOrder { order: [RecipeKind::Fullscreen, RecipeKind::Video, RecipeKind::Web], custom: false }
    }
    /// 自定义序合法性核（判据「可配」守卫）：必须恰好覆盖三族。
    pub fn validate(&self) -> Result<(), u16> {
        let mut seen = [false; 3];
        for k in self.order.iter() {
            if seen[k.ordinal()] {
                return Err(SEG_BASE + 3);
            }
            seen[k.ordinal()] = true;
        }
        if !seen.iter().all(|&s| s) {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }
    /// 查某族优先位（0 最高）。
    pub fn rank(&self, k: RecipeKind) -> Result<usize, u16> {
        self.validate()?;
        self.order.iter().position(|&x| x == k).ok_or(SEG_BASE + 5)
    }
    /// 按序消解一对冲突：秩小者胜。
    pub fn resolve(&self, rec: &ConflictRecord) -> Result<(Verdict, u32, u32), u16> {
        rec.validate()?;
        if !rec.is_conflict() {
            // 非冲突：诉求原样放行。
            return Ok((Verdict::SplitHalf, rec.want_a, rec.want_b));
        }
        let ra = self.rank(rec.a)?;
        let rb = self.rank(rec.b)?;
        // 胜者拿满自身诉求，败者吃剩余——总和恒 1000（守恒消解）。
        let (verdict, a_share, b_share) = match ra.cmp(&rb) {
            core::cmp::Ordering::Less => {
                let a = rec.want_a.min(1000);
                (Verdict::FirstWins, a, 1000 - a)
            }
            core::cmp::Ordering::Greater => {
                let b = rec.want_b.min(1000);
                (Verdict::SecondWins, 1000 - b, b)
            }
            core::cmp::Ordering::Equal => (Verdict::SplitHalf, 500, 500),
        };
        Ok((verdict, a_share, b_share))
    }
}

/// 冲突账（判据「显性」主判据）：冲突发生即入账，配对归因唯一。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConflictLedger {
    /// 账行（封顶 64——账满显性拒绝不静默丢弃）。
    pub rows: Vec<ConflictRecord>,
}

impl ConflictLedger {
    /// 空账。
    pub fn new() -> Self {
        ConflictLedger { rows: Vec::new() }
    }
    /// 入账（同对同池幂等——同一冲突重复上报不重复记）。
    pub fn record(&mut self, rec: ConflictRecord) -> Result<bool, u16> {
        rec.validate()?;
        if !rec.is_conflict() {
            return Ok(false);
        }
        let key = rec.pair_key();
        if self.rows.iter().any(|r| r.pool == rec.pool && r.pair_key() == key) {
            return Ok(false);
        }
        if self.rows.len() >= 64 {
            return Err(SEG_BASE + 6);
        }
        self.rows.push(rec);
        Ok(true)
    }
    /// 按配方对统计（判据「显性」输出面）。
    pub fn count_by_pair(&self, a: RecipeKind, b: RecipeKind) -> usize {
        let ka = a.ordinal() as u8;
        let kb = b.ordinal() as u8;
        let key = if ka <= kb { (ka, kb) } else { (kb, ka) };
        self.rows.iter().filter(|r| r.pair_key() == key).count()
    }
}

impl Default for ConflictLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 案例行（判据「案例库」一等公民）：场景描述 + 消解结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseRow {
    /// 案例号。
    pub case: u32,
    /// 冲突对（规范化键）。
    pub pair: (u8, u8),
    /// 案例描述（静态串——案例库是沉淀不是日志）。
    pub note: &'static str,
}

/// 案例库（判据「案例库」主判据）：典型冲突场景与消解示例，静态
/// 沉淀 + 运行时案例追加（去重按 case 号）。
pub struct CaseBook {
    /// 案例行。
    pub rows: Vec<CaseRow>,
}

impl CaseBook {
    /// 出厂案例库（三条典型：视频 vs 网页画中画、全屏 vs 通知、
    /// 双视频多路）。
    pub fn factory() -> Self {
        CaseBook {
            rows: alloc::vec![
                CaseRow { case: 1, pair: (0, 1), note: "画中画视频+前台网页滚动：视频优先，网页让出 200" },
                CaseRow { case: 2, pair: (1, 2), note: "全屏游戏+后台网页：全屏拿满，网页冻结渲染" },
                CaseRow { case: 3, pair: (0, 0), note: "双视频多路：同族 SplitHalf，走配额表细化" },
            ],
        }
    }
    /// 追加案例（case 号重复即拒——案例库不覆盖历史）。
    pub fn append(&mut self, row: CaseRow) -> Result<(), u16> {
        if row.case == 0 {
            return Err(SEG_BASE + 7);
        }
        if self.rows.iter().any(|r| r.case == row.case) {
            return Err(SEG_BASE + 8);
        }
        if row.note.is_empty() {
            return Err(SEG_BASE + 9);
        }
        self.rows.push(row);
        Ok(())
    }
    /// 查案例（按对）。
    pub fn by_pair(&self, pair: (u8, u8)) -> Vec<&CaseRow> {
        self.rows.iter().filter(|r| r.pair == pair).collect()
    }
}

/// 矩阵自洽核（判据「冲突矩阵」守卫）：反对称性——verdict(a,b)
/// 与 verdict(b,a) 必须互为反转；对角线恒 SplitHalf。
pub fn matrix_self_consistent() -> Result<(), u16> {
    for a in RecipeKind::all() {
        if !matches!(conflict_verdict(a, a)?, Verdict::SplitHalf) {
            return Err(SEG_BASE + 10);
        }
        for b in RecipeKind::all() {
            let ab = conflict_verdict(a, b)?;
            let ba = conflict_verdict(b, a)?;
            let ok = match (ab, ba) {
                (Verdict::FirstWins, Verdict::SecondWins) => true,
                (Verdict::SecondWins, Verdict::FirstWins) => true,
                (Verdict::SplitHalf, Verdict::SplitHalf) => a == b,
                _ => false,
            };
            if !ok {
                return Err(SEG_BASE + 11);
            }
        }
    }
    Ok(())
}

/// 冲突诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "同族优先序相等——矩阵构造错",
        1 => "池号为 0 非法",
        2 => "诉求为 0——无争夺不入冲突账",
        3 => "自定义序含重复族——不是排列",
        4 => "自定义序未覆盖三族",
        5 => "秩查询越界——族不在序内",
        6 => "冲突账满（64 行）——先归档再入账",
        7 => "案例号为 0",
        8 => "案例号重复——案例库不覆盖历史",
        9 => "案例描述为空",
        10 => "对角线必须 SplitHalf",
        11 => "矩阵反对称性被破坏",
        _ => "未知诊断码——查本模块 SEG_BASE 0x08F0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_and_antisymmetry() {
        assert!(matrix_self_consistent().is_ok());
        // 可见性优先序：全屏 > 视频 > 网页。
        assert!(matches!(conflict_verdict(RecipeKind::Fullscreen, RecipeKind::Web).unwrap(), Verdict::FirstWins));
        assert!(matches!(conflict_verdict(RecipeKind::Web, RecipeKind::Fullscreen).unwrap(), Verdict::SecondWins));
        assert!(matches!(conflict_verdict(RecipeKind::Video, RecipeKind::Video).unwrap(), Verdict::SplitHalf));
    }

    #[test]
    fn configurable_order() {
        let def = ResolveOrder::default_order();
        assert!(!def.custom);
        assert!(def.validate().is_ok());
        // 自定义序：网页最高（用户自选）。
        let custom = ResolveOrder { order: [RecipeKind::Web, RecipeKind::Video, RecipeKind::Fullscreen], custom: true };
        assert!(custom.validate().is_ok());
        assert_eq!(custom.rank(RecipeKind::Web).unwrap(), 0);
        // 非排列拒绝。
        let dup = ResolveOrder { order: [RecipeKind::Web, RecipeKind::Web, RecipeKind::Video], custom: true };
        assert_eq!(dup.validate().unwrap_err(), SEG_BASE + 3);
        let dup2 = ResolveOrder { order: [RecipeKind::Web, RecipeKind::Web, RecipeKind::Web], custom: true };
        assert_eq!(dup2.validate().unwrap_err(), SEG_BASE + 3);
    }

    #[test]
    fn ledger_idempotent_by_pair() {
        let mut led = ConflictLedger::new();
        let r1 = ConflictRecord { a: RecipeKind::Video, b: RecipeKind::Web, pool: 1, want_a: 700, want_b: 600 };
        assert!(led.record(r1).unwrap());
        // 同对同池重复上报：幂等不重记。
        let r2 = ConflictRecord { a: RecipeKind::Web, b: RecipeKind::Video, pool: 1, want_a: 600, want_b: 700 };
        assert!(!led.record(r2).unwrap());
        assert_eq!(led.count_by_pair(RecipeKind::Video, RecipeKind::Web), 1);
        // 非冲突不入账。
        let calm = ConflictRecord { a: RecipeKind::Video, b: RecipeKind::Web, pool: 2, want_a: 400, want_b: 500 };
        assert!(!led.record(calm).unwrap());
        assert_eq!(led.rows.len(), 1);
    }

    #[test]
    fn case_book_sediment() {
        let mut book = CaseBook::factory();
        assert_eq!(book.rows.len(), 3);
        assert_eq!(book.by_pair((0, 1)).len(), 1);
        // 追加与去重。
        book.append(CaseRow { case: 4, pair: (1, 2), note: "全屏退出瞬间网页回填：先予后夺顺序回填" }).unwrap();
        assert_eq!(book.append(CaseRow { case: 4, pair: (0, 0), note: "重复号" }).unwrap_err(), SEG_BASE + 8);
        assert_eq!(book.append(CaseRow { case: 5, pair: (0, 1), note: "" }).unwrap_err(), SEG_BASE + 9);
        assert_eq!(book.rows.len(), 4);
    }
}
