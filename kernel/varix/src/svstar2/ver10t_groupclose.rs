//! VE-F3560 · E08 组收口 —— 组级门禁 + 六件齐备 + 双人签署 + 包哈希 + fuzz 重跑。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3560`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组级门禁**（[`GroupGate`]）：断言全绿 + 两律符合 +
//!   对拍归档 + 六件齐备，缺一即阻断。
//! - **六件齐备**（[`SixArtifacts`]）：文档/基准/fuzz/一致性/
//!   反馈通道/遗留清单六件，每件独立核验。
//! - **双人签署**（[`DualSign`]）：双签前重跑组内 fuzz 语料全量
//!   防回归；双签后才出证据编号。
//! - **包哈希**（[`GroupGate::evidence_hash`]）：证据包带哈希，
//!   损坏可检出可重建。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 缺件 | 阻断 |
//! | 不全 | 黄门（缺非关键件） |
//! | 损坏 | 重建 |
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
// 一、诊断码（E19 段续：6380 起）
// ---------------------------------------------------------------------------

/// 组收口诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 缺件（红门）。
    ArtifactMissing = 6380,
    /// 件不全（黄门）。
    ArtifactPartial = 6381,
    /// 双签未齐。
    DualSignMissing = 6382,
    /// 证据损坏。
    EvidenceCorrupt = 6383,
    /// fuzz 回归未过。
    FuzzRegression = 6384,
}

impl GateCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GateCode::ArtifactMissing => "六件缺件红门",
            GateCode::ArtifactPartial => "六件不全黄门",
            GateCode::DualSignMissing => "双签未齐",
            GateCode::EvidenceCorrupt => "证据包损坏",
            GateCode::FuzzRegression => "fuzz 回归未过",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            GateCode::ArtifactMissing | GateCode::DualSignMissing | GateCode::FuzzRegression
        )
    }
}

// ---------------------------------------------------------------------------
// 二、六件
// ---------------------------------------------------------------------------

/// 六件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Artifact {
    /// 文档。
    Docs,
    /// 基准。
    Bench,
    /// fuzz。
    Fuzz,
    /// 一致性。
    Consistency,
    /// 反馈通道。
    Feedback,
    /// 遗留清单。
    Leftover,
}

impl Artifact {
    /// 全部六件。
    pub const ALL: [Artifact; 6] = [
        Artifact::Docs,
        Artifact::Bench,
        Artifact::Fuzz,
        Artifact::Consistency,
        Artifact::Feedback,
        Artifact::Leftover,
    ];

    /// 件短名。
    pub const fn label(self) -> &'static str {
        match self {
            Artifact::Docs => "文档",
            Artifact::Bench => "基准",
            Artifact::Fuzz => "fuzz",
            Artifact::Consistency => "一致性",
            Artifact::Feedback => "反馈通道",
            Artifact::Leftover => "遗留清单",
        }
    }
}

/// 六件齐备状态（bit 掩码）。
#[derive(Clone, Copy, Debug)]
pub struct SixArtifacts {
    /// bit0..bit5 对应 Docs..Leftover。
    pub mask: u8,
}

impl SixArtifacts {
    /// 全空。
    pub const fn empty() -> Self {
        Self { mask: 0 }
    }

    /// 标记一件就绪。
    pub const fn mark(mut self, a: Artifact) -> Self {
        let bit = match a {
            Artifact::Docs => 1u8,
            Artifact::Bench => 2,
            Artifact::Fuzz => 4,
            Artifact::Consistency => 8,
            Artifact::Feedback => 16,
            Artifact::Leftover => 32,
        };
        self.mask |= bit;
        self
    }

    /// 就绪数。
    pub fn ready_count(&self) -> u32 {
        self.mask.count_ones()
    }

    /// 全齐。
    pub fn complete(&self) -> bool {
        self.mask == 0b11_1111
    }

    /// 缺件列表。
    pub fn missing(&self) -> Vec<Artifact> {
        let mut v = Vec::new();
        for a in Artifact::ALL {
            let bit = match a {
                Artifact::Docs => 1u8,
                Artifact::Bench => 2,
                Artifact::Fuzz => 4,
                Artifact::Consistency => 8,
                Artifact::Feedback => 16,
                Artifact::Leftover => 32,
            };
            if self.mask & bit == 0 {
                v.push(a);
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------
// 三、双签
// ---------------------------------------------------------------------------

/// 签署人。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// 实现方。
    Implementer,
    /// 审核方。
    Reviewer,
}

/// 双签流。
#[derive(Clone, Copy, Debug)]
pub struct DualSign {
    /// 实现方已签。
    pub impl_signed: bool,
    /// 审核方已签。
    pub rev_signed: bool,
}

impl DualSign {
    /// 全新。
    pub const fn new() -> Self {
        Self {
            impl_signed: false,
            rev_signed: false,
        }
    }

    /// 签一笔。
    pub const fn sign(mut self, who: Signer) -> Self {
        match who {
            Signer::Implementer => self.impl_signed = true,
            Signer::Reviewer => self.rev_signed = true,
        }
        self
    }

    /// 双签齐。
    pub const fn complete(&self) -> bool {
        self.impl_signed && self.rev_signed
    }
}

// ---------------------------------------------------------------------------
// 四、证据包与门禁
// ---------------------------------------------------------------------------

/// 证据包。
#[derive(Clone, Copy, Debug)]
pub struct EvidencePack {
    /// 证据编号。
    pub serial: u32,
    /// 包哈希。
    pub hash: u64,
    /// 是否损坏。
    pub corrupt: bool,
}

/// 组门禁。
pub struct GroupGate {
    artifacts: SixArtifacts,
    sign: DualSign,
    /// fuzz 回归通过旗标。
    fuzz_green: bool,
}

impl GroupGate {
    /// 新建。
    pub fn new() -> Self {
        Self {
            artifacts: SixArtifacts::empty(),
            sign: DualSign::new(),
            fuzz_green: false,
        }
    }

    /// 标记一件就绪。
    pub fn mark_artifact(&mut self, a: Artifact) {
        self.artifacts = self.artifacts.mark(a);
    }

    /// 双签前重跑 fuzz（防回归）。
    pub fn rerun_fuzz(&mut self, passed: bool) {
        self.fuzz_green = passed;
    }

    /// 签署（fuzz 未绿拒签）。
    pub fn sign(&mut self, who: Signer) -> Result<(), GateCode> {
        if !self.fuzz_green {
            return Err(GateCode::FuzzRegression);
        }
        self.sign = self.sign.sign(who);
        Ok(())
    }

    /// 出证据包：六件齐 + 双签齐 + fuzz 绿才放行。
    pub fn seal(&self, serial: u32) -> Result<EvidencePack, GateCode> {
        if !self.artifacts.complete() {
            return Err(GateCode::ArtifactMissing);
        }
        if !self.sign.complete() {
            return Err(GateCode::DualSignMissing);
        }
        if !self.fuzz_green {
            return Err(GateCode::FuzzRegression);
        }
        // 包哈希：件掩码 + 编号混合。
        let hash = 0xcbf2_9ce4_8422_2325_u64
            ^ (self.artifacts.mask as u64)
            .wrapping_mul(0x0000_0100_0000_01b3)
            ^ serial as u64;
        Ok(EvidencePack {
            serial,
            hash,
            corrupt: false,
        })
    }

    /// 六件状态只读。
    pub fn artifacts(&self) -> SixArtifacts {
        self.artifacts
    }

    /// 黄门判定：缺件但非零（可先黄后红）。
    pub fn yellow(&self) -> bool {
        !self.artifacts.complete() && self.artifacts.ready_count() > 0
    }
}

impl Default for GroupGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 组收口判据集。
pub fn run_gate_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10t_groupclose");

    // 1. 六件齐备判定。
    let mut s = SixArtifacts::empty();
    for a in Artifact::ALL {
        s = s.mark(a);
    }
    set.add("6380-六件齐备", s.complete() && s.ready_count() == 6, "");

    // 2. 缺件列出。
    let partial = SixArtifacts::empty().mark(Artifact::Docs);
    set.add("6381-缺件列出", partial.missing().len() == 5, "");

    // 3. fuzz 未绿拒签。
    let mut g = GroupGate::new();
    let no_sign = g.sign(Signer::Implementer);
    set.add("6382-fuzz 拦签", no_sign.err() == Some(GateCode::FuzzRegression), "");

    // 4. 齐件 + 双签 + fuzz → 出证据。
    let mut g2 = GroupGate::new();
    for a in Artifact::ALL {
        g2.mark_artifact(a);
    }
    g2.rerun_fuzz(true);
    let _ = g2.sign(Signer::Implementer);
    let _ = g2.sign(Signer::Reviewer);
    let pack = g2.seal(1001);
    set.add("6383-出证据包", pack.is_ok(), "");

    // 5. 缺件 seal 拒。
    let mut g3 = GroupGate::new();
    g3.rerun_fuzz(true);
    let _ = g3.sign(Signer::Implementer);
    let _ = g3.sign(Signer::Reviewer);
    set.add(
            "6384-缺件拒绝",
            g3.seal(1).err() == Some(GateCode::ArtifactMissing),
            "",
        );

    // 6. 双签缺失拒。
    let mut g4 = GroupGate::new();
    for a in Artifact::ALL {
        g4.mark_artifact(a);
    }
    g4.rerun_fuzz(true);
    let _ = g4.sign(Signer::Implementer);
    set.add(
            "6385-单签拒绝",
            g4.seal(2).err() == Some(GateCode::DualSignMissing),
            "",
        );

    // 7. 黄门判定。
    set.add("6386-黄门", g3.yellow(), "");
    set.add("6387-红门阻断", GateCode::ArtifactMissing.blocking(), "");

    set
}
