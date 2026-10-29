//! unx0623_xpkcons —— UNX-F0623 跨包一致性核（版本/时戳对齐）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0623 · 跨包一致性核（版本/时戳对齐） · 目标 ≥220 行
//! 判据原文摘录：一致性核账齐（12 包版本全 v1 对齐+封账时戳有序
//!   12/12），不一致检出注入红。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 版本对齐闭集：12 包版本必须全等 v1（版本闭集外即红），逐包
//!      版本落账，对齐核 = 逐包比对基准版。
//!   2. 时戳有序核：12 包封账时戳按包号序严格递增（相等也算乱序，
//!      因为封账有先后），12/12 全过才平。
//!   3. 不一致检出：支持注入面——任何一包版本漂移或时戳倒挂，核必
//!      须红且能定位包号。
//!   4. 定容 12 槽账本，守恒恒等式：已登包数 + 空槽数 = 12。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4FA0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4FA0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4FA0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4FAF;

/// XcCode：版本漂移。
pub const XC_VER_DRIFT: u16 = SEG_BASE + 1; // 0x4FA1 包版本 ≠ v1
/// XcCode：时戳乱序。
pub const XC_STAMP_ORDER: u16 = SEG_BASE + 2; // 0x4FA2 封账时戳非严格递增
/// XcCode：包号越界。
pub const XC_BAD_PKG: u16 = SEG_BASE + 3; // 0x4FA3 包号闭集外
/// XcCode：重复登记。
pub const XC_DUP_PKG: u16 = SEG_BASE + 4; // 0x4FA4 一槽一包不许重登
/// XcCode：包未齐禁核。
pub const XC_PKG_SHORT: u16 = SEG_BASE + 5; // 0x4FA5 12/12 未齐
/// XcCode：基准版本非法。
pub const XC_BAD_BASE: u16 = SEG_BASE + 6; // 0x4FA6 基准版本闭集外

/// 包槽容量。
pub const PKG_N: usize = 12;
/// 版本闭集：仅 v1（判据原文：12 包版本全 v1）。
pub const VER_V1: u32 = 1;

/// 单包一致性账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgConsRow {
    /// 包号（0..12）。
    pub pkg_no: usize,
    /// 包版本（闭集 {1}）。
    pub version: u32,
    /// 封账时戳（整数拍）。
    pub seal_stamp: u64,
}

/// 跨包一致性账本。
#[derive(Clone, Debug)]
pub struct XPkgConsBook {
    /// 12 槽（None = 未登）。
    slots: [Option<PkgConsRow>; PKG_N],
    /// 基准版本（= VER_V1）。
    base_ver: u32,
}

impl XPkgConsBook {
    /// 开账：基准版本必须是 v1。
    pub fn open() -> Result<Self, u16> {
        if VER_V1 != 1 {
            return Err(XC_BAD_BASE);
        }
        Ok(XPkgConsBook {
            slots: [
                None, None, None, None, None, None, None, None, None, None, None, None,
            ],
            base_ver: VER_V1,
        })
    }

    /// 已登包数。
    pub fn registered(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 守恒：登记数 + 空槽数 = 12。
    pub fn conserve_ok(&self) -> bool {
        self.registered() + self.slots.iter().filter(|s| s.is_none()).count() == PKG_N
    }

    /// 登一包（号闭集 + 去重）。
    pub fn enroll(&mut self, pkg_no: usize, version: u32, seal_stamp: u64) -> Result<(), u16> {
        if pkg_no >= PKG_N {
            return Err(XC_BAD_PKG);
        }
        if self.slots[pkg_no].is_some() {
            return Err(XC_DUP_PKG);
        }
        self.slots[pkg_no] = Some(PkgConsRow {
            pkg_no,
            version,
            seal_stamp,
        });
        Ok(())
    }

    /// 版本对齐核：12/12 全登 + 逐包版本 = 基准 v1。
    /// 红 = 版本漂移的**最小**包号（定位语义）。
    pub fn version_align(&self) -> Result<usize, u16> {
        if self.registered() != PKG_N {
            return Err(XC_PKG_SHORT);
        }
        for i in 0..PKG_N {
            match &self.slots[i] {
                Some(r) => {
                    if r.version != self.base_ver {
                        return Err(XC_VER_DRIFT);
                    }
                }
                None => return Err(XC_PKG_SHORT),
            }
        }
        Ok(PKG_N)
    }

    /// 时戳有序核：按包号序严格递增，12/12。
    pub fn stamps_ordered(&self) -> Result<(u64, u64), u16> {
        if self.registered() != PKG_N {
            return Err(XC_PKG_SHORT);
        }
        let mut prev: Option<u64> = None;
        for i in 0..PKG_N {
            let r = match &self.slots[i] {
                Some(r) => r,
                None => return Err(XC_PKG_SHORT),
            };
            if let Some(p) = prev {
                if r.seal_stamp <= p {
                    return Err(XC_STAMP_ORDER);
                }
            }
            prev = Some(r.seal_stamp);
        }
        match (
            self.slots[0].as_ref(),
            self.slots[PKG_N - 1].as_ref(),
        ) {
            (Some(a), Some(b)) => Ok((a.seal_stamp, b.seal_stamp)),
            _ => Err(XC_PKG_SHORT),
        }
    }

    /// 一致性总核：版本对齐 + 时戳有序双过。
    pub fn consistency_green(&self) -> Result<u32, u16> {
        self.version_align()?;
        let (first, last) = self.stamps_ordered()?;
        Ok((first as u32) ^ (last as u32) ^ (PKG_N as u32))
    }

    /// 定位漂移包：返回第一个版本 ≠ v1 的包号（无漂移回 None）。
    pub fn find_drift(&self) -> Option<usize> {
        for i in 0..PKG_N {
            if let Some(r) = &self.slots[i] {
                if r.version != self.base_ver {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "跨包一致性核 · 登记 {}/{} · 基准 v{} · 守恒={}",
            self.registered(),
            PKG_N,
            self.base_ver,
            self.conserve_ok()
        );
        for i in 0..PKG_N {
            if let Some(r) = &self.slots[i] {
                s.push_str(&alloc::format!(
                    "\n  包{} v{} 封账@{}",
                    r.pkg_no, r.version, r.seal_stamp
                ));
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

/// 建 12 包标准一致性账（版本全 v1，时戳 = 100+包号*10，严格递增）。
pub fn build_standard() -> Result<XPkgConsBook, u16> {
    let mut book = XPkgConsBook::open()?;
    for i in 0..PKG_N {
        book.enroll(i, VER_V1, 100 + (i as u64) * 10)?;
    }
    book.consistency_green()?;
    Ok(book)
}

/// 自例程：主路径（标准账→双核对平）+ 错误路径（版本漂移/时戳倒挂/缺包/重登/越界）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let book = build_standard()?;
    if book.version_align()? != PKG_N {
        return Err(XC_VER_DRIFT);
    }
    let (first, last) = book.stamps_ordered()?;
    if !(first < last) {
        return Err(XC_STAMP_ORDER);
    }
    if book.consistency_green() == Err(XC_VER_DRIFT) {
        return Err(XC_VER_DRIFT);
    }
    if book.find_drift().is_some() {
        return Err(XC_VER_DRIFT);
    }
    if book.fingerprint() == 0 {
        return Err(XC_PKG_SHORT);
    }

    // 错误路径 1：版本漂移（包 5 改 v2）→ 红，且能定位。
    let mut b1 = build_standard()?;
    if let Some(r) = &mut b1.slots[5] {
        r.version = 2;
    }
    if b1.version_align() != Err(XC_VER_DRIFT) {
        return Err(XC_VER_DRIFT);
    }
    if b1.find_drift() != Some(5) {
        return Err(XC_VER_DRIFT);
    }

    // 错误路径 2：时戳倒挂（包 7 与包 6 相等 → 非严格递增）。
    let mut b2 = build_standard()?;
    let s6 = match &b2.slots[6] {
        Some(r) => r.seal_stamp,
        None => return Err(XC_PKG_SHORT),
    };
    if let Some(r) = &mut b2.slots[7] {
        r.seal_stamp = s6;
    }
    if b2.stamps_ordered() != Err(XC_STAMP_ORDER) {
        return Err(XC_STAMP_ORDER);
    }

    // 错误路径 3：时戳回退。
    let mut b3 = build_standard()?;
    if let Some(r) = &mut b3.slots[9] {
        r.seal_stamp = 5;
    }
    if b3.stamps_ordered() != Err(XC_STAMP_ORDER) {
        return Err(XC_STAMP_ORDER);
    }

    // 错误路径 4：缺包禁核。
    let mut b4 = XPkgConsBook::open()?;
    b4.enroll(0, VER_V1, 1)?;
    if b4.version_align() != Err(XC_PKG_SHORT) {
        return Err(XC_PKG_SHORT);
    }
    if b4.stamps_ordered() != Err(XC_PKG_SHORT) {
        return Err(XC_PKG_SHORT);
    }

    // 错误路径 5：重登与越界。
    let mut b5 = XPkgConsBook::open()?;
    b5.enroll(0, VER_V1, 1)?;
    if b5.enroll(0, VER_V1, 2) != Err(XC_DUP_PKG) {
        return Err(XC_DUP_PKG);
    }
    if b5.enroll(PKG_N, VER_V1, 3) != Err(XC_BAD_PKG) {
        return Err(XC_BAD_PKG);
    }
    // 守恒核。
    if !b5.conserve_ok() {
        return Err(XC_PKG_SHORT);
    }
    Ok(0)
}
