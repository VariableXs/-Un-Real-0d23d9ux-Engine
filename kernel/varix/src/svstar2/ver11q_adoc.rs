//! VE-F3577 · 审查文档 —— 方法论三件 + 反面案例 + CI 防漂移 + 复审。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3577`
//!
//! # 职责（锚点原文拆解）
//!
//! - **方法论三件**（[`MethodDoc`]）：审查什么 / 怎么审查 /
//!   怎么处置三件。
//! - **反面案例库**（[`CaseLibrary`]）：历史违规案例教学。
//! - **CI 防漂移**：文档哈希对拍基准。
//!
//! # 复杂度
//!
//! O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6720 起）
// ---------------------------------------------------------------------------

/// 审查文档诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditDocCode {
    /// 三件不全。
    Incomplete = 6720,
    /// 漂移。
    Drift = 6721,
    /// 案例过时。
    CaseStale = 6722,
}

impl AuditDocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AuditDocCode::Incomplete => "方法论三件不全",
            AuditDocCode::Drift => "审查文档漂移",
            AuditDocCode::CaseStale => "反面案例过时",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, AuditDocCode::Incomplete | AuditDocCode::Drift)
    }
}

// ---------------------------------------------------------------------------
// 二、方法论三件
// ---------------------------------------------------------------------------

/// 方法论文档。
pub struct MethodDoc {
    /// 审查什么。
    pub what: &'static str,
    /// 怎么审查。
    pub how: &'static str,
    /// 怎么处置。
    pub disposition: &'static str,
    /// 内容指纹。
    pub hash: u64,
}

impl MethodDoc {
    /// 三件齐备。
    pub fn complete(&self) -> bool {
        !self.what.is_empty() && !self.how.is_empty() && !self.disposition.is_empty()
    }
}

/// 反面案例。
#[derive(Clone, Copy, Debug)]
pub struct NegativeCase {
    /// 案例 id。
    pub id: u32,
    /// 是否过时。
    pub stale: bool,
}

/// 案例库。
#[derive(Default)]
pub struct CaseLibrary {
    cases: Vec<NegativeCase>,
}

impl CaseLibrary {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加案例。
    pub fn add(&mut self, c: NegativeCase) {
        self.cases.push(c);
    }

    /// 新鲜案例数。
    pub fn fresh_count(&self) -> usize {
        self.cases.iter().filter(|c| !c.stale).count()
    }

    /// 过时数。
    pub fn stale_count(&self) -> usize {
        self.cases.iter().filter(|c| c.stale).count()
    }

    /// 案例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、CI
// ---------------------------------------------------------------------------

/// 审查文档 CI。
pub struct AuditDocCi;

impl AuditDocCi {
    /// 漂移核验。
    pub fn verify(doc: &MethodDoc, baseline_hash: u64) -> Result<(), AuditDocCode> {
        if !doc.complete() {
            return Err(AuditDocCode::Incomplete);
        }
        if doc.hash != baseline_hash {
            return Err(AuditDocCode::Drift);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 审查文档判据集。
pub fn run_adoc_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11q");

    // 1. 三件齐。
    let doc = MethodDoc {
        what: "令牌/动效/文案/无障碍",
        how: "机检+对拍+人工抽核",
        disposition: "编号登记分级处置",
        hash: 42,
    };
    chk.add("6720-三件齐", doc.complete(), "");

    // 2. 缺件拦截。
    let bad = MethodDoc {
        what: "",
        how: "x",
        disposition: "y",
        hash: 1,
    };
    chk.add(
        "6721-缺件拦",
        AuditDocCi::verify(&bad, 1).err() == Some(AuditDocCode::Incomplete),
        "",
    );

    // 3. 漂移拦。
    chk.add(
        "6722-漂移拦",
        AuditDocCi::verify(&doc, 99).err() == Some(AuditDocCode::Drift),
        "",
    );

    // 4. 基准过。
    chk.add("6723-基准过", AuditDocCi::verify(&doc, 42).is_ok(), "");

    // 5. 案例库。
    let mut lib = CaseLibrary::new();
    lib.add(NegativeCase { id: 1, stale: false });
    lib.add(NegativeCase { id: 2, stale: true });
    chk.add("6724-案例库", lib.fresh_count() == 1 && lib.stale_count() == 1, "");

    chk
}
