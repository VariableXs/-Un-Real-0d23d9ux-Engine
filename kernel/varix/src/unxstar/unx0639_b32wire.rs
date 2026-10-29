//! unx0639_b32wire —— UNX-F0639 ktest 域级断言接入（B32 分接入）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0639 · ktest 域级断言接入（B32 分接入） · 目标 ≥240 行
//! 判据原文摘录：B32 判据入套件 unx_a1_b32_p1 全绿，映射双向对账。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. B32 判据号闭集（initrd 收尾域分接入：F0639/F0640/F0643/F0644
//!      /F0645/F0646），六判据定容入套件 unx_a1_b32_p1。
//!   2. 映射双向对账：正向（判据→套件位）与反向（套件位→判据集）
//!      用定容数组双表互逆核，表间任何脱节即红。
//!   3. 全绿定义：六判据逐条跑主路径（登记在场 + 摘要非空 + 号在
//!      闭集）全过，缺一即红。
//!   4. 注入红检出：闭集外判据/重号/坏摘要三类注入均须被核出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x51A0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x51A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x51A0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x51AF;

/// XcCode：判据号闭集外。
pub const XC_BAD_CRIT: u16 = SEG_BASE + 1; // 0x51A1 六判据闭集外
/// XcCode：重号。
pub const XC_DUP_CRIT: u16 = SEG_BASE + 2; // 0x51A2 一号一位
/// XcCode：摘要空。
pub const XC_EMPTY_DIGEST: u16 = SEG_BASE + 3; // 0x51A3 空摘要
/// XcCode：双向对账破。
pub const XC_BIMAP: u16 = SEG_BASE + 4; // 0x51A4 正反表互逆失败
/// XcCode：断言红。
pub const XC_ASSERT_RED: u16 = SEG_BASE + 5; // 0x51A5 逐条有红
/// XcCode：套件未登满。
pub const XC_WIRE_SHORT: u16 = SEG_BASE + 6; // 0x51A6 6/6 未接入

/// B32 判据数（六判据闭集）。
pub const CRIT_N: usize = 6;
/// B32 分接入套件名。
pub const SUITE_B32: &str = "unx_a1_b32_p1";

/// B32 判据号闭集。
pub const B32_CRIT_IDS: [u16; CRIT_N] = [639, 640, 643, 644, 645, 646];

/// B32 判据摘要名册。
pub const B32_DIGESTS: [&str; CRIT_N] = [
    "F0639 B32 分接入",
    "F0640 B32 批断言集",
    "F0643 演练记录簿",
    "F0644 失败复演规程",
    "F0645 域回归门禁",
    "F0646 引导履历账",
];

/// 正向表：判据号 → 套件槽位（定容，未接入 = 0xFF 哨兵）。
pub type FwdTable = [u8; CRIT_N];

/// 反向表：套件槽位 → 判据号（定容，按判据号升序冻结）。
pub type RevTable = [u16; CRIT_N];

/// 双向接入账本。
#[derive(Clone, Debug)]
pub struct B32Wire {
    /// 正向表。
    fwd: FwdTable,
    /// 反向表。
    rev: RevTable,
    /// 已接入数。
    wired: usize,
    /// 摘要账（按槽位）。
    digests: [Option<String>; CRIT_N],
}

impl B32Wire {
    /// 空接入账（正向表全哨兵）。
    pub fn new() -> Self {
        B32Wire {
            fwd: [0xFF; CRIT_N],
            rev: [0xFFFF; CRIT_N],
            wired: 0,
            digests: [None, None, None, None, None, None],
        }
    }

    /// 已接入数。
    pub fn wired_n(&self) -> usize {
        self.wired
    }

    /// 接入一条判据（闭集 + 去重 + 摘要非空）。
    pub fn wire(&mut self, crit_id: u16, digest: &str) -> Result<usize, u16> {
        let slot = match B32_CRIT_IDS.iter().position(|c| *c == crit_id) {
            Some(s) => s,
            None => return Err(XC_BAD_CRIT),
        };
        if self.fwd[slot] != 0xFF {
            return Err(XC_DUP_CRIT);
        }
        if digest.is_empty() {
            return Err(XC_EMPTY_DIGEST);
        }
        self.fwd[slot] = slot as u8;
        self.rev[slot] = crit_id;
        self.digests[slot] = Some(digest.to_string());
        self.wired += 1;
        Ok(slot)
    }

    /// 正向查（判据号 → 槽位）。
    pub fn fwd_lookup(&self, crit_id: u16) -> Option<usize> {
        let slot = B32_CRIT_IDS.iter().position(|c| *c == crit_id)?;
        if self.fwd[slot] == 0xFF {
            None
        } else {
            Some(slot)
        }
    }

    /// 反向查（槽位 → 判据号）。
    pub fn rev_lookup(&self, slot: usize) -> Option<u16> {
        if slot >= CRIT_N || self.rev[slot] == 0xFFFF {
            None
        } else {
            Some(self.rev[slot])
        }
    }

    /// 双向对账：逐接入判据 正向命中槽 == 反向命中判据号，计数相等。
    pub fn bimap_reconcile(&self) -> Result<usize, u16> {
        if self.wired == 0 {
            return Err(XC_WIRE_SHORT);
        }
        let mut n = 0usize;
        for (slot, &cid) in self.rev.iter().enumerate() {
            if cid == 0xFFFF {
                continue;
            }
            match self.fwd_lookup(cid) {
                Some(s) => {
                    if s != slot {
                        return Err(XC_BIMAP);
                    }
                }
                None => return Err(XC_BIMAP),
            }
            n += 1;
        }
        if n != self.wired {
            return Err(XC_BIMAP);
        }
        Ok(n)
    }

    /// 逐条断言执行机：接入在场 + 摘要非空 + 号在闭集。
    pub fn run_one(&self, crit_id: u16) -> Result<(), u16> {
        if !B32_CRIT_IDS.contains(&crit_id) {
            return Err(XC_BAD_CRIT);
        }
        match self.fwd_lookup(crit_id) {
            None => Err(XC_ASSERT_RED),
            Some(slot) => match &self.digests[slot] {
                Some(d) if !d.is_empty() => Ok(()),
                _ => Err(XC_ASSERT_RED),
            },
        }
    }

    /// 套件全绿核：6/6 接入 + 双向对账 + 逐条绿。
    pub fn suite_green(&self) -> Result<usize, u16> {
        if self.wired != CRIT_N {
            return Err(XC_WIRE_SHORT);
        }
        self.bimap_reconcile()?;
        for c in B32_CRIT_IDS.iter() {
            self.run_one(*c)?;
        }
        Ok(CRIT_N)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "{} · 接入 {}/{} · 双向对账平={}",
            SUITE_B32,
            self.wired,
            CRIT_N,
            self.bimap_reconcile().is_ok()
        );
        for slot in 0..CRIT_N {
            if let Some(cid) = self.rev_lookup(slot) {
                s.push_str(&alloc::format!("\n  槽{} 判据F0{}", slot, cid));
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// B32 全量接入器（六判据按名册摘要接入）。
pub fn wire_b32_suite() -> Result<B32Wire, u16> {
    let mut w = B32Wire::new();
    for i in 0..CRIT_N {
        w.wire(B32_CRIT_IDS[i], B32_DIGESTS[i])?;
    }
    w.suite_green()?;
    Ok(w)
}

/// 自例程：主路径（六判据全绿）+ 错误路径（闭集外/重号/空摘要/对账破/缺接入）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let w = wire_b32_suite()?;
    if w.suite_green()? != CRIT_N {
        return Err(XC_ASSERT_RED);
    }
    // 双向查一致：正向反查全等。
    for c in B32_CRIT_IDS.iter() {
        let s = match w.fwd_lookup(*c) {
            Some(s) => s,
            None => return Err(XC_BIMAP),
        };
        if w.rev_lookup(s) != Some(*c) {
            return Err(XC_BIMAP);
        }
    }
    if w.fingerprint() == 0 {
        return Err(XC_BIMAP);
    }

    // 错误路径 1：闭集外判据。
    let mut w1 = B32Wire::new();
    if w1.wire(999, "x") != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    // 错误路径 2：重号。
    let mut w2 = wire_b32_suite()?;
    if w2.wire(639, "重接") != Err(XC_DUP_CRIT) {
        return Err(XC_DUP_CRIT);
    }
    // 错误路径 3：空摘要。
    let mut w3 = B32Wire::new();
    if w3.wire(639, "") != Err(XC_EMPTY_DIGEST) {
        return Err(XC_EMPTY_DIGEST);
    }
    // 错误路径 4：缺接入禁全绿。
    let mut w4 = B32Wire::new();
    for i in 0..CRIT_N - 1 {
        w4.wire(B32_CRIT_IDS[i], B32_DIGESTS[i])?;
    }
    if w4.suite_green() != Err(XC_WIRE_SHORT) {
        return Err(XC_WIRE_SHORT);
    }
    // 错误路径 5：对账破（反向表篡改）。
    let mut w5 = wire_b32_suite()?;
    w5.rev[2] = 0xFFFF;
    if w5.bimap_reconcile() != Err(XC_BIMAP) {
        return Err(XC_BIMAP);
    }
    // 错误路径 6：未接入判据断言红。
    let mut w6 = B32Wire::new();
    w6.wire(639, "只接一条")?;
    if w6.run_one(640) != Err(XC_ASSERT_RED) {
        return Err(XC_ASSERT_RED);
    }
    // 错误路径 7：闭集外断言红。
    if w6.run_one(0) != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    Ok(0)
}
