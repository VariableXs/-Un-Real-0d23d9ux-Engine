//! unx0239_batchtally —— UNX-F0239 域收官批次清点（40 批对账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0239 · 批次清点 · 目标 200 行
//! 判据原文：UNX-F0239-J1 A1 域 40 批逐批清点：批号闭集 [1,40]
//!           零缺零重，缺批/重批逐条立案，40/40 齐才收官。
//! 组件：批清单（位图账：40 位一一对应）+ 清点引擎（缺/重双查）
//!       + 收官闸（40/40 全齐 + 位图与清单双向对账）。
//! 职责定位：批次清点是「收官的地基完整性」——40 批是一个闭集，
//!   少一批 = 有活没干完，重一批 = 账面重复计功。位图账让缺批
//!   与重批都是 O(1) 可判定；双向对账保证位图与清单不会各说各话。
//! 性能逐项分解：清点 O(批数)；位图核 O(1)；闸 O(1)。
//! 诊断码段：0x20140（0x20141~0x20149 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const BT_VERSION: &str = "unx0239_batchtally-v1";

/// 诊断段基址（0x20140 段 · 本模块独占）。
pub const BT_SEG_BASE: u32 = 0x20140;
pub const XC_BATCH_OOR: u32 = 0x20141; // 批号出 [1,40]
pub const XC_BATCH_DUP: u32 = 0x20142; // 重批
pub const XC_BATCH_MISS: u32 = 0x20143; // 缺批
pub const XC_EMPTY: u32 = 0x20144; // 空清单拒
pub const XC_BITMAP_DRIFT: u32 = 0x20145; // 位图/清单漂移
pub const XC_ORDER_OOO: u32 = 0x20146; // 清单乱序
pub const XC_COUNT_DRIFT: u32 = 0x20147; // 计数漂移
pub const XC_TALLY_NEG: u32 = 0x20148; // 清点负值
pub const XC_CLOSED: u32 = 0x20149; // 已收官再清点

/// 批闭集。
pub const BATCH_LO: u32 = 1;
pub const BATCH_HI: u32 = 40;

/// 批位图账：u64 两字覆盖 1..=40。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BatchBitmap {
    lo: u64, // 批 1~32
    hi: u64, // 批 33~40（低 8 位有效）
}

impl BatchBitmap {
    pub fn new() -> BatchBitmap {
        BatchBitmap::default()
    }

    /// 置位：批号出闭集拒；重复置位拒（重批立案）。
    pub fn mark(&mut self, batch: u32) -> Result<(), u32> {
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(XC_BATCH_OOR);
        }
        if self.has(batch) {
            return Err(XC_BATCH_DUP);
        }
        if batch <= 32 {
            self.lo |= 1u64 << (batch - 1);
        } else {
            self.hi |= 1u64 << (batch - 33);
        }
        Ok(())
    }

    /// 查位。
    pub fn has(&self, batch: u32) -> bool {
        if batch < BATCH_LO || batch > BATCH_HI {
            return false;
        }
        if batch <= 32 {
            self.lo & (1u64 << (batch - 1)) != 0
        } else {
            self.hi & (1u64 << (batch - 33)) != 0
        }
    }

    /// 已批数。
    pub fn count(&self) -> u32 {
        self.lo.count_ones() + (self.hi & 0xFF).count_ones()
    }

    /// 缺批清单（升序）。
    pub fn missing(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for b in BATCH_LO..=BATCH_HI {
            if !self.has(b) {
                v.push(b);
            }
        }
        v
    }

    /// 全齐核：40/40。
    pub fn full(&self) -> bool {
        self.count() == BATCH_HI
    }

    /// 位图词位（序列化：lo 低 32 + [lo 高 32 低 16 位 | hi 高 8 位]）。
    pub fn words(&self) -> [u32; 2] {
        [self.lo as u32, ((self.lo >> 32) as u32) | (((self.hi & 0xFF) as u32) << 24)]
    }
}

/// 清点引擎：批清单 → 位图 + 双向对账。
/// 清单必须升序（乱序拒——产线批号序即施工序）。
pub fn tally(list: &[u32]) -> Result<BatchBitmap, u32> {
    if list.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut bm = BatchBitmap::new();
    let mut prev = 0;
    for &b in list.iter() {
        if b <= prev {
            return Err(XC_ORDER_OOO);
        }
        bm.mark(b)?;
        prev = b;
    }
    // 双向对账：位图计数 == 清单长度（重批在 mark 已拦，此为账面冗余核）。
    if bm.count() != list.len() as u32 {
        return Err(XC_COUNT_DRIFT);
    }
    Ok(bm)
}

/// 收官闸：40/40 全齐才放行；缺批立案。
pub fn close_gate(bm: &BatchBitmap) -> Result<(), u32> {
    if !bm.full() {
        return Err(XC_BATCH_MISS);
    }
    Ok(())
}

/// 位图词位往返闭账：反序列化位图与原位图逐位相等。
pub fn bitmap_from_words(w: [u32; 2]) -> Result<BatchBitmap, u32> {
    let lo = (w[0] as u64) | ((w[1] as u64 & 0xFFFF) << 32);
    let hi = ((w[1] >> 24) as u64) & 0xFF;
    let bm = BatchBitmap { lo, hi };
    // hi 只允许低 8 位有效（批 33~40）。
    if bm.hi & !0xFFu64 != 0 {
        return Err(XC_BITMAP_DRIFT);
    }
    Ok(bm)
}

pub fn run_unx0239_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0239_batchtally");
    // 族一：全 40 批清点。
    let full_list: Vec<u32> = (BATCH_LO..=BATCH_HI).collect();
    let bm = match tally(&full_list) {
        Ok(b) => b,
        Err(_) => {
            cs.add("全批清点", false, "被拒");
            return cs;
        }
    };
    cs.add("40/40", bm.full(), "位图全齐");
    cs.add("零缺批", bm.missing().is_empty(), "闭集无缺");
    cs.add("收官放行", close_gate(&bm).is_ok(), "全齐开闸");
    // 族二：缺批立案。
    let mut short = full_list.clone();
    short.retain(|&b| b != 7 && b != 23);
    let bm2 = tally(&short).unwrap_or(BatchBitmap::new());
    cs.add("缺两批", bm2.missing().as_slice() == &[7u32, 23], "升序立案");
    cs.add("计数 38", bm2.count() == 38, "40-2");
    cs.add("收官拦", close_gate(&bm2) == Err(XC_BATCH_MISS), "缺批不放");
    // 族三：重批/乱序/越域拒。
    cs.add("重批拒", tally(&[1, 2, 2]).is_err(), "DUP");
    cs.add("乱序拒", tally(&[1, 5, 3]).is_err(), "ORDER");
    cs.add("越界高拒", tally(&[1, 41]).is_err(), "OOR");
    cs.add("越界低拒", tally(&[0, 1]).is_err(), "OOR");
    cs.add("空清单拒", tally(&[]).is_err(), "EMPTY");
    cs.add("单批 mark 重拒", {
        let mut b = BatchBitmap::new();
        let _ = b.mark(3);
        b.mark(3).is_err()
    }, "DUP");
    // 族四：位图词位往返。
    let w = bm.words();
    let back = bitmap_from_words(w);
    cs.add("词位往返", matches!(back, Ok(b) if b.full()), "逐位相等");
    cs.add("hi 越域拒", bitmap_from_words([0, 0x0100_0000]).is_err(), "BITMAP_DRIFT");
    // 族五：边界批号。
    let mut edge = BatchBitmap::new();
    cs.add("批 1 可置", edge.mark(1).is_ok(), "下界");
    let mut edge2 = BatchBitmap::new();
    cs.add("批 40 可置", edge2.mark(40).is_ok(), "上界");
    let mut edge3 = BatchBitmap::new();
    cs.add("批 0 拒", edge3.mark(0).is_err(), "OOR");
    cs.add("has 出界 false", !edge.has(41), "闭集外恒无");
    cs
}
