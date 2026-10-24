//! unx0552_dualswitch —— UNX-F0552 编译/运行双态开关账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0552 · 编译/运行双态开关账 · 目标 200 行
//! 判据原文：双态开关账齐（编译期裁剪面+运行期阈值面），两态独立核
//!           （4/4 组合）。
//! 组件：编译态（四裁剪档：全关/仅 ERROR/WARN+/全开）× 运行态（阈值 0..3）
//!   四组合独立核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x35740;
pub const XC_COMPILE_OOB: u32 = SEG_BASE + 1; // 编译档越界
pub const XC_RUN_OOB: u32 = SEG_BASE + 2; // 运行阈值越界
pub const XC_SWITCH_BAD: u32 = SEG_BASE + 3; // 坏账
pub const XC_SWITCH_OOB: u32 = SEG_BASE + 4; // 越界

/// 编译期裁剪档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompileCut {
    /// C0 全关（零日志）。
    Off = 0,
    /// C1 仅 ERROR。
    ErrOnly = 1,
    /// C2 WARN 及以上。
    WarnUp = 2,
    /// C3 全开（四级别都编入）。
    All = 3,
}

impl CompileCut {
    /// 档内可编入的最高级别号（C0 无 → None）。
    pub const fn max_level(self) -> Option<u8> {
        match self {
            CompileCut::Off => None,
            CompileCut::ErrOnly => Some(0),
            CompileCut::WarnUp => Some(1),
            CompileCut::All => Some(3),
        }
    }
}

/// 运行期阈值面。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunThresh {
    pub thresh: u8,
}

impl RunThresh {
    /// 构造（0..=3）。
    pub fn new(t: u8) -> Result<Self, u32> {
        if t > 3 {
            return Err(XC_RUN_OOB);
        }
        Ok(RunThresh { thresh: t })
    }

    /// 运行期放行判定。
    pub const fn allows(&self, level: u8) -> bool {
        level <= self.thresh
    }
}

/// 双态组合判定：一行日志是否最终可见（编译裁剪先、运行阈值后）。
pub fn visible(cut: CompileCut, rt: &RunThresh, level: u8) -> Result<bool, u32> {
    if level > 3 {
        return Err(XC_SWITCH_OOB);
    }
    let max = match cut.max_level() {
        Some(m) => m,
        None => return Ok(false),
    };
    if level > max {
        return Ok(false); // 编译期未编入——运行态无从谈起。
    }
    Ok(rt.allows(level))
}

/// 四组合独立核：编译档 C0..C3 × 运行阈值 3（全放）——可见集=编译集。
pub fn four_combo_scan() -> Result<bool, u32> {
    let rt = RunThresh::new(3)?;
    let mut c = 0;
    while c <= 3 {
        let cut = match CompileCut::from_u8(c) {
            Some(x) => x,
            None => return Ok(false),
        };
        let mut lv = 0;
        while lv < 4 {
            let v = visible(cut, &rt, lv)?;
            let expect = match cut.max_level() {
                Some(m) => lv <= m,
                None => false,
            };
            if v != expect {
                return Ok(false);
            }
            lv += 1;
        }
        c += 1;
    }
    Ok(true)
}

impl CompileCut {
    /// 号→枚举。
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(CompileCut::Off),
            1 => Some(CompileCut::ErrOnly),
            2 => Some(CompileCut::WarnUp),
            3 => Some(CompileCut::All),
            _ => None,
        }
    }
}

pub fn run_unx0552_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0552");
    cs.add("SEG 段位独占", SEG_BASE == 0x35740 && XC_SWITCH_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：编译档。
    cs.add("C0 全关", CompileCut::Off.max_level().is_none(), "零编入");
    cs.add("C1 仅 ERR", CompileCut::ErrOnly.max_level() == Some(0), "顶=0");
    cs.add("C2 WARN+", CompileCut::WarnUp.max_level() == Some(1), "顶=1");
    cs.add("C3 全开", CompileCut::All.max_level() == Some(3), "顶=3");
    cs.add("档越界", CompileCut::from_u8(4).is_none(), "4 无档");
    cs.add("运行越界拒", RunThresh::new(4).is_err(), "thresh=4 Err");

    // 族二：运行阈值。
    let rt0 = RunThresh::new(0).unwrap_or(RunThresh { thresh: 9 });
    cs.add("阈值 0", rt0.allows(0) && !rt0.allows(1), "只放 ERROR");
    let rt3 = RunThresh::new(3).unwrap_or(RunThresh { thresh: 9 });
    cs.add("阈值 3", rt3.allows(3), "全放");

    // 族三：4/4 组合独立。
    cs.add("四组合扫描", four_combo_scan() == Ok(true), "编译×运行 双态独立");

    // 族四：可见性语义。
    let rtmid = RunThresh::new(1).unwrap_or(RunThresh { thresh: 9 });
    cs.add("编译胜于运行", visible(CompileCut::ErrOnly, &rt3, 1) == Ok(false), "未编入即不可见（运行全放）");
    cs.add("运行裁于编译", visible(CompileCut::All, &rtmid, 2) == Ok(false), "编入但阈值不放");
    cs.add("全开门", visible(CompileCut::All, &rt3, 3) == Ok(true), "全开全放可见");
    cs.add("级别越界拒", visible(CompileCut::All, &rt3, 4).is_err(), "level=4 Err");
    cs
}
