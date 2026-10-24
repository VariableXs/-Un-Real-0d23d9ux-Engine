//! VE-F3572 · 个性化与一致性边界机检 —— 边界机检 + 判分界线 + 案例库 + 复审。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3572`
//!
//! # 职责（锚点原文拆解）
//!
//! - **边界机检**（[`BoundaryCheck`]）：个性化允许的空间与
//!   一致性底线的机器可判分界。
//! - **案例库**（[`CaseBook`]）：边界案例库维护（案例含期望裁决）。
//! - **复审机制**：案例失效复审。
//!
//! # 复杂度
//!
//! O(案例)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6620 起）
// ---------------------------------------------------------------------------

/// 边界机检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundCode {
    /// 边界模糊（裁决不确定）。
    BoundaryBlur = 6620,
    /// 案例失效。
    CaseExpired = 6621,
    /// 机检失效。
    CheckDown = 6622,
}

impl BoundCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BoundCode::BoundaryBlur => "个性化一致性边界模糊",
            BoundCode::CaseExpired => "边界案例失效",
            BoundCode::CheckDown => "边界机检失效",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, BoundCode::BoundaryBlur)
    }
}

// ---------------------------------------------------------------------------
// 二、裁决
// ---------------------------------------------------------------------------

/// 裁决：个性化空间 vs 一致性底线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 允许个性化。
    AllowPersonalize,
    /// 必须一致。
    MustConform,
    /// 模糊需评审。
    Ambiguous,
}

impl Verdict {
    /// 裁决短名。
    pub const fn label(self) -> &'static str {
        match self {
            Verdict::AllowPersonalize => "允许个性化",
            Verdict::MustConform => "必须一致",
            Verdict::Ambiguous => "模糊需评审",
        }
    }
}

/// 边界案例。
#[derive(Clone, Copy, Debug)]
pub struct BoundaryCase {
    /// 案例 id。
    pub id: u32,
    /// 期望裁决。
    pub expected: Verdict,
    /// 是否有效（过期案例需复审）。
    pub valid: bool,
}

// ---------------------------------------------------------------------------
// 三、案例库与机检
// ---------------------------------------------------------------------------

/// 案例库。
#[derive(Default)]
pub struct CaseBook {
    cases: Vec<BoundaryCase>,
}

impl CaseBook {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加案例。
    pub fn add(&mut self, c: BoundaryCase) {
        self.cases.push(c);
    }

    /// 有效案例数。
    pub fn valid_count(&self) -> usize {
        self.cases.iter().filter(|c| c.valid).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 过期案例数（复审队列）。
    pub fn expired_count(&self) -> usize {
        self.cases.iter().filter(|c| !c.valid).count()
    }
}

/// 边界机检器。
pub struct BoundaryCheck {
    book: CaseBook,
}

impl BoundaryCheck {
    /// 新建。
    pub fn new(book: CaseBook) -> Self {
        Self { book }
    }

    /// 机检一处边界（个性值 + 一致值偏离千分比）。
    /// 偏离 < 50 → 允许个性化；< 200 → 必须一致；≥ 200 → 模糊。
    pub fn judge(&self, deviation: u32) -> Verdict {
        if deviation < 50 {
            Verdict::AllowPersonalize
        } else if deviation < 200 {
            Verdict::MustConform
        } else {
            Verdict::Ambiguous
        }
    }

    /// 案例库只读。
    pub fn book(&self) -> &CaseBook {
        &self.book
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 边界机检判据集。
pub fn run_bound_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11l");

    let mut book = CaseBook::new();
    book.add(BoundaryCase {
        id: 1,
        expected: Verdict::AllowPersonalize,
        valid: true,
    });
    book.add(BoundaryCase {
        id: 2,
        expected: Verdict::MustConform,
        valid: false,
    });

    // 1. 案例库统计。
    chk.add("6620-案例库", book.valid_count() == 1 && book.expired_count() == 1, "");

    let checker = BoundaryCheck::new(book);

    // 2. 三档裁决。
    chk.add(
        "6621-三档裁决",
        checker.judge(10) == Verdict::AllowPersonalize
            && checker.judge(100) == Verdict::MustConform
            && checker.judge(300) == Verdict::Ambiguous,
        "",
    );

    // 3. 模糊阻断。
    chk.add("6622-模糊阻断", BoundCode::BoundaryBlur.blocking(), "");

    // 4. 裁决名可达。
    chk.add("6623-裁决名", Verdict::MustConform.label() == "必须一致", "");

    // 5. 空库判定。
    let empty = CaseBook::new();
    chk.add("6624-空库", empty.is_empty() && empty.valid_count() == 0, "");

    chk
}
