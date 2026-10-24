//! VE-F3556 · persona 一致性 —— 词典一致 + 同款交互 + 随组验证 + 兜底扫描。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3556`
//!
//! # 职责（锚点原文拆解）
//!
//! - **词典一致**（[`LexiconCheck`]）：设置/预览/回退三处交互
//!   用词与 U 域词典对照，偏离即违例。
//! - **同款交互**（[`InteractionPattern`]）：三处交互形状同款
//!   （确认/取消/返回行为一致），不许一处一个样。
//! - **随组验证**：一致性检查随组收口跑，不是可选项。
//! - **兜底扫描**（[`ConsistencySweep`]）：词典更新后复检 +
//!   漏检兜底。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 违例 | 修正 |
//! | 词典更新 | 复检 |
//! | 漏检 | 兜底 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6300 起）
// ---------------------------------------------------------------------------

/// persona 一致性诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsistCode {
    /// 词典偏离。
    LexiconDrift = 6300,
    /// 交互不同款。
    PatternMismatch = 6301,
    /// 兜底扫描命中。
    FallbackHit = 6302,
    /// 词典版本漂移。
    DictVersionDrift = 6303,
}

impl ConsistCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ConsistCode::LexiconDrift => "词典偏离",
            ConsistCode::PatternMismatch => "交互不同款",
            ConsistCode::FallbackHit => "兜底扫描命中",
            ConsistCode::DictVersionDrift => "词典版本漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ConsistCode::PatternMismatch)
    }
}

// ---------------------------------------------------------------------------
// 二、词典
// ---------------------------------------------------------------------------

/// 词典版本。
pub const DICT_VERSION: u32 = 1;

/// 词典条目（词形 id ↔ 规范用语 id）。
#[derive(Clone, Copy, Debug)]
pub struct LexEntry {
    /// 场景（0=设置 1=预览 2=回退）。
    pub scene: u8,
    /// 规范用语 id。
    pub canonical: u32,
}

/// 词典核验。
pub struct LexiconCheck {
    entries: Vec<LexEntry>,
    version: u32,
}

impl LexiconCheck {
    /// 标准词典（三场景各一条）。
    pub fn standard() -> Self {
        let entries = alloc::vec![
            LexEntry { scene: 0, canonical: 100 },
            LexEntry { scene: 1, canonical: 100 },
            LexEntry { scene: 2, canonical: 100 },
        ];
        Self {
            entries,
            version: DICT_VERSION,
        }
    }

    /// 版本核验。
    pub fn version_ok(&self) -> bool {
        self.version == DICT_VERSION
    }

    /// 三场景用语一致性（同词形 = 一致）。
    pub fn consistent(&self, a: u32, b: u32, c: u32) -> bool {
        a == b && b == c
    }

    /// 查场景规范。
    pub fn canonical_of(&self, scene: u8) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.scene == scene)
            .map(|e| e.canonical)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、同款交互
// ---------------------------------------------------------------------------

/// 交互形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InteractionPattern {
    /// 确认键位 id。
    pub confirm_key: u32,
    /// 取消键位 id。
    pub cancel_key: u32,
    /// 返回行为 id。
    pub back_behavior: u32,
}

impl InteractionPattern {
    /// 同款判定。
    pub const fn same_as(&self, o: &Self) -> bool {
        self.confirm_key == o.confirm_key
            && self.cancel_key == o.cancel_key
            && self.back_behavior == o.back_behavior
    }
}

// ---------------------------------------------------------------------------
// 四、兜底扫描
// ---------------------------------------------------------------------------

/// 一致性扫描器。
pub struct ConsistencySweep {
    /// 兜底命中计数。
    pub fallback_hits: u32,
    /// 违例计数。
    pub violations: u32,
}

impl ConsistencySweep {
    /// 新建。
    pub fn new() -> Self {
        Self {
            fallback_hits: 0,
            violations: 0,
        }
    }

    /// 扫一处（用语三元组 + 交互三方）。
    pub fn sweep(
        &mut self,
        lex: &LexiconCheck,
        words: (u32, u32, u32),
        patterns: (InteractionPattern, InteractionPattern, InteractionPattern),
    ) -> Result<(), ConsistCode> {
        if !lex.version_ok() {
            return Err(ConsistCode::DictVersionDrift);
        }
        if !lex.consistent(words.0, words.1, words.2) {
            self.violations = self.violations.saturating_add(1);
            return Err(ConsistCode::LexiconDrift);
        }
        if !patterns.0.same_as(&patterns.1) || !patterns.1.same_as(&patterns.2) {
            self.violations = self.violations.saturating_add(1);
            return Err(ConsistCode::PatternMismatch);
        }
        Ok(())
    }

    /// 兜底复扫（词典更新后强制跑）。
    pub fn fallback_resweep(&mut self, prior_violations: u32) -> u32 {
        if prior_violations != self.violations {
            self.fallback_hits = self.fallback_hits.saturating_add(1);
        }
        self.violations
    }
}

impl Default for ConsistencySweep {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// persona 一致性判据集。
pub fn run_consist_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10p_consistency");

    let lex = LexiconCheck::standard();
    let p = InteractionPattern {
        confirm_key: 1,
        cancel_key: 2,
        back_behavior: 3,
    };

    // 1. 词典一致 + 同款交互 → 过。
    let mut sw = ConsistencySweep::new();
    let pass = sw.sweep(&lex, (100, 100, 100), (p, p, p)).is_ok();
    set.add("6300-一致通过", pass, "");

    // 2. 用语偏离检出。
    let mut sw2 = ConsistencySweep::new();
    let drift = sw2.sweep(&lex, (100, 200, 100), (p, p, p)).is_err();
    set.add("6301-词典偏离", drift && sw2.violations == 1, "");

    // 3. 交互不同款检出。
    let p2 = InteractionPattern {
        confirm_key: 9,
        cancel_key: 2,
        back_behavior: 3,
    };
    let mut sw3 = ConsistencySweep::new();
    let mismatch = sw3
        .sweep(&lex, (100, 100, 100), (p, p, p2))
        .err()
        == Some(ConsistCode::PatternMismatch);
    set.add("6302-交互不同款", mismatch, "");

    // 4. 词典版本漂移拦。
    let mut bad_lex = LexiconCheck::standard();
    bad_lex.version = DICT_VERSION + 5;
    let mut sw4 = ConsistencySweep::new();
    let drift_v = sw4
        .sweep(&bad_lex, (1, 1, 1), (p, p, p))
        .err()
        == Some(ConsistCode::DictVersionDrift);
    set.add("6303-版本漂移拦", drift_v, "");

    // 5. 兜底复扫。
    let mut sw5 = ConsistencySweep::new();
    let _ = sw5.sweep(&lex, (1, 2, 3), (p, p, p));
    let hits = sw5.fallback_resweep(0);
    set.add("6304-兜底复扫", hits == 1 && sw5.fallback_hits == 1, "");

    // 6. 词典条目齐。
    set.add("6305-词典完整", lex.len() == 3 && lex.canonical_of(1) == Some(100), "");

    set
}
