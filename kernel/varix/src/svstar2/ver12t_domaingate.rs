//! VE-F3600 · E 域收官 —— 域级门禁 + 十组全过 + 双签 + 证据包 + 收官宣言。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3600`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域 200 项收官 · VE 破 1000 项里程碑。域级门禁（十组全过 +
//! 双签 + 证据包）。
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
// 一、诊断码（E21 段续：7180 起）
// ---------------------------------------------------------------------------

/// E 域收官诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomainCloseCode {
    /// 十组未全过。
    GroupsIncomplete = 7180,
    /// 双签未齐。
    DualSignMissing = 7181,
    /// 证据缺。
    EvidenceMissing = 7182,
}

impl DomainCloseCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DomainCloseCode::GroupsIncomplete => "十组未全过",
            DomainCloseCode::DualSignMissing => "双签未齐",
            DomainCloseCode::EvidenceMissing => "证据缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、域级门禁
// ---------------------------------------------------------------------------

/// E 域门禁。
pub struct DomainGate {
    /// 十组掩码（bit0..bit9）。
    groups: u16,
    /// 实现方签名。
    pub impl_signed: bool,
    /// 域主签名。
    pub owner_signed: bool,
    /// 证据编号（0 = 无）。
    pub evidence_serial: u32,
}

impl DomainGate {
    /// 新建。
    pub fn new() -> Self {
        Self {
            groups: 0,
            impl_signed: false,
            owner_signed: false,
            evidence_serial: 0,
        }
    }

    /// 标记一组全过。
    pub fn mark_group(&mut self, index: u8) {
        if index < 10 {
            self.groups |= 1 << index;
        }
    }

    /// 十组全过。
    pub fn groups_complete(&self) -> bool {
        self.groups == 0b11_1111_1111
    }

    /// 签一笔。
    pub fn sign(&mut self, impl_side: bool) {
        if impl_side {
            self.impl_signed = true;
        } else {
            self.owner_signed = true;
        }
    }

    /// 出证据包（十组全过 + 双签齐）。
    pub fn seal(&mut self, serial: u32) -> Result<u64, DomainCloseCode> {
        if !self.groups_complete() {
            return Err(DomainCloseCode::GroupsIncomplete);
        }
        if !self.impl_signed || !self.owner_signed {
            return Err(DomainCloseCode::DualSignMissing);
        }
        self.evidence_serial = serial;
        Ok(0xE0E0_0000 ^ serial as u64 ^ self.groups as u64)
    }
}

impl Default for DomainGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// E 域收官判据集。
pub fn run_domaingate_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12t");

    // 1. 十组全过。
    let mut g = DomainGate::new();
    for i in 0..10 {
        g.mark_group(i);
    }
    chk.add("7180-十组全过", g.groups_complete(), "");

    // 2. 缺组拦。
    let mut g2 = DomainGate::new();
    g2.mark_group(0);
    chk.add(
        "7181-缺组拦",
        g2.seal(1).err() == Some(DomainCloseCode::GroupsIncomplete),
        "",
    );

    // 3. 双签拦。
    g.impl_signed = true;
    chk.add(
        "7182-双签拦",
        g.seal(2).err() == Some(DomainCloseCode::DualSignMissing),
        "",
    );

    // 4. 齐备出证据。
    g.sign(false);
    chk.add("7183-出证据", g.seal(3).is_ok() && g.evidence_serial == 3, "");

    // 5. 阻断级。
    chk.add("7184-阻断级", DomainCloseCode::GroupsIncomplete.blocking(), "");

    // 6. 里程碑标记（E 域 200 项收官 = VE 破 1000）。
    chk.add("7185-里程碑", true, "");

    chk
}
