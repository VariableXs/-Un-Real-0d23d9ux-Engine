//! VE-F3580 · E09 组收口 —— 组级门禁 + 六件齐备 + 双人签署 + 包哈希 + fuzz 重跑。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3580`
//!
//! # 职责（锚点原文拆解）
//!
//! 与 E08 组收口同构（六件 + 双签 + fuzz 重跑 + 证据包哈希），
//! 承接 E 域台账。
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6780 起）
// ---------------------------------------------------------------------------

/// E09 收口诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Close9Code {
    /// 缺件。
    ArtifactMissing = 6780,
    /// 双签未齐。
    DualSignMissing = 6781,
    /// fuzz 未过。
    FuzzRegression = 6782,
}

impl Close9Code {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            Close9Code::ArtifactMissing => "六件缺件",
            Close9Code::DualSignMissing => "双签未齐",
            Close9Code::FuzzRegression => "fuzz 回归未过",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、六件与双签
// ---------------------------------------------------------------------------

/// 六件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Art9 {
    /// 文档。
    Docs,
    /// 基准。
    Bench,
    /// fuzz。
    Fuzz,
    /// 一致性。
    Consistency,
    /// 反馈。
    Feedback,
    /// 遗留。
    Leftover,
}

impl Art9 {
    /// 全部六件。
    pub const ALL: [Art9; 6] = [
        Art9::Docs,
        Art9::Bench,
        Art9::Fuzz,
        Art9::Consistency,
        Art9::Feedback,
        Art9::Leftover,
    ];
}

/// 双签。
#[derive(Clone, Copy, Debug)]
pub struct DualSign9 {
    /// 实现方。
    pub impl_signed: bool,
    /// 审核方。
    pub rev_signed: bool,
}

impl DualSign9 {
    /// 全新。
    pub const fn new() -> Self {
        Self {
            impl_signed: false,
            rev_signed: false,
        }
    }

    /// 签一笔。
    pub const fn sign(mut self, impl_side: bool) -> Self {
        if impl_side {
            self.impl_signed = true;
        } else {
            self.rev_signed = true;
        }
        self
    }

    /// 双签齐。
    pub const fn complete(&self) -> bool {
        self.impl_signed && self.rev_signed
    }
}

// ---------------------------------------------------------------------------
// 三、组门禁
// ---------------------------------------------------------------------------

/// E09 组门禁。
pub struct Gate9 {
    artifacts: u8,
    sign: DualSign9,
    fuzz_green: bool,
}

impl Gate9 {
    /// 新建。
    pub fn new() -> Self {
        Self {
            artifacts: 0,
            sign: DualSign9::new(),
            fuzz_green: false,
        }
    }

    /// 标记一件。
    pub fn mark(&mut self, a: Art9) {
        let bit = match a {
            Art9::Docs => 1u8,
            Art9::Bench => 2,
            Art9::Fuzz => 4,
            Art9::Consistency => 8,
            Art9::Feedback => 16,
            Art9::Leftover => 32,
        };
        self.artifacts |= bit;
    }

    /// 双签前 fuzz 重跑。
    pub fn rerun_fuzz(&mut self, passed: bool) {
        self.fuzz_green = passed;
    }

    /// 签署。
    pub fn sign(&mut self, impl_side: bool) -> Result<(), Close9Code> {
        if !self.fuzz_green {
            return Err(Close9Code::FuzzRegression);
        }
        self.sign = self.sign.sign(impl_side);
        Ok(())
    }

    /// 出证据包哈希。
    pub fn seal(&self, serial: u32) -> Result<u64, Close9Code> {
        if self.artifacts != 0b11_1111 {
            return Err(Close9Code::ArtifactMissing);
        }
        if !self.sign.complete() {
            return Err(Close9Code::DualSignMissing);
        }
        if !self.fuzz_green {
            return Err(Close9Code::FuzzRegression);
        }
        Ok(0xcbf2_9ce4_8422_2325_u64
            ^ (self.artifacts as u64).wrapping_mul(0x0000_0100_0000_01b3)
            ^ serial as u64)
    }

    /// 六件齐备。
    pub fn complete(&self) -> bool {
        self.artifacts == 0b11_1111
    }
}

impl Default for Gate9 {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// E09 收口判据集。
pub fn run_close9_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11t");

    // 1. 六件齐备。
    let mut g = Gate9::new();
    for a in Art9::ALL {
        g.mark(a);
    }
    chk.add("6780-六件齐", g.complete(), "");

    // 2. fuzz 未绿拒签。
    let mut g2 = Gate9::new();
    chk.add(
        "6781-fuzz拦签",
        g2.sign(true).err() == Some(Close9Code::FuzzRegression),
        "",
    );

    // 3. 齐件+双签+fuzz → 出哈希。
    g.rerun_fuzz(true);
    let _ = g.sign(true);
    let _ = g.sign(false);
    chk.add("6782-出哈希", g.seal(1).is_ok(), "");

    // 4. 缺件拒。
    let mut g3 = Gate9::new();
    g3.rerun_fuzz(true);
    let _ = g3.sign(true);
    let _ = g3.sign(false);
    chk.add(
        "6783-缺件拒",
        g3.seal(2).err() == Some(Close9Code::ArtifactMissing),
        "",
    );

    // 5. 单签拒。
    let mut g4 = Gate9::new();
    for a in Art9::ALL {
        g4.mark(a);
    }
    g4.rerun_fuzz(true);
    let _ = g4.sign(true);
    chk.add(
        "6784-单签拒",
        g4.seal(3).err() == Some(Close9Code::DualSignMissing),
        "",
    );

    // 6. 阻断级。
    chk.add("6785-阻断级", Close9Code::ArtifactMissing.blocking(), "");

    chk
}
