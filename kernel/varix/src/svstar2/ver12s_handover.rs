//! VE-F3599 · E 域移交包 —— 移交清单 + 对接点 + 哈希校验 + 拒收条件。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3599`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域 → F 域移交包（清单/对接点/哈希/拒收）。
//!
//! # 复杂度
//!
//! O(件)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7160 起）
// ---------------------------------------------------------------------------

/// 移交包诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverCode {
    /// 清单缺件。
    ItemMissing = 7160,
    /// 哈希不符。
    HashMismatch = 7161,
    /// 拒收。
    Rejected = 7162,
}

impl HandoverCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            HandoverCode::ItemMissing => "移交清单缺件",
            HandoverCode::HashMismatch => "移交哈希不符",
            HandoverCode::Rejected => "移交被拒收",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, HandoverCode::ItemMissing | HandoverCode::HashMismatch)
    }
}

// ---------------------------------------------------------------------------
// 二、移交包
// ---------------------------------------------------------------------------

/// 移交件。
#[derive(Clone, Copy, Debug)]
pub struct HandoverItem {
    /// 件 id。
    pub id: u32,
    /// 内容哈希。
    pub hash: u64,
}

/// 移交包。
pub struct HandoverPack {
    items: Vec<HandoverItem>,
    /// 包哈希。
    pub pack_hash: u64,
}

impl HandoverPack {
    /// 从移交件建包（包哈希 = 件哈希异或累乘）。
    pub fn build(items: Vec<HandoverItem>) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for it in &items {
            h ^= it.hash;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self {
            items,
            pack_hash: h,
        }
    }

    /// 件数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 校验（期望件数 + 期望包哈希）。
    pub fn verify(&self, expected_count: usize, expected_hash: u64) -> Result<(), HandoverCode> {
        if self.items.len() != expected_count {
            return Err(HandoverCode::ItemMissing);
        }
        if self.pack_hash != expected_hash {
            return Err(HandoverCode::HashMismatch);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 移交包判据集。
pub fn run_handover_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12s");

    // 1. 建包。
    let items = alloc::vec![
        HandoverItem { id: 1, hash: 100 },
        HandoverItem { id: 2, hash: 200 },
    ];
    let pack = HandoverPack::build(items);
    chk.add("7160-建包", pack.item_count() == 2, "");

    // 2. 哈希正确。
    let h = pack.pack_hash;
    chk.add("7161-哈希", pack.verify(2, h).is_ok(), "");

    // 3. 件数不符。
    chk.add(
        "7162-件数拦",
        pack.verify(3, h).err() == Some(HandoverCode::ItemMissing),
        "",
    );

    // 4. 哈希不符。
    chk.add(
        "7163-哈希拦",
        pack.verify(2, 999).err() == Some(HandoverCode::HashMismatch),
        "",
    );

    // 5. 阻断级。
    chk.add("7164-阻断级", HandoverCode::HashMismatch.blocking(), "");

    chk
}
