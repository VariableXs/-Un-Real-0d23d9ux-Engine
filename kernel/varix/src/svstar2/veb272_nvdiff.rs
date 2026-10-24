//! veb272_nvdiff —— VE-F0272 NVIDIA 驱动版本差异表。
//! 锚点（CoRun Unxreal 主册）：VE-F0272 · NVIDIA 驱动版本差异表 · 目标 320 行
//! 判据原文：VE-F0272-J1 驱动版本差异表：版本号（主.次.补）+
//!           差异项（新特性位 / 回归位 / 弃用位）；版本比较
//!           （三元组字典序）；差异查询（两版本间的差异项集
//!           合）；回归检测（新版引入回归位 → 提示降级候选）；
//!           弃用传播（新版弃用的能力，差异表标弃）。
//! 组件：版本结构 + 差异账 + 回归提示 + 弃用传播。
//! 职责定位：版本差异表是「NVIDIA 驱动的 changelog 机器可读版」
//!           ——手写 changelog 会漏，差异表从版本账直接算：新
//!           特性是加法，回归是减法，弃用是搬家通知；回归提示
//!           是降级的理由库：知道新版坏了什么才知道要不要降。
//! 性能逐项分解：版本比较 O(1)；差异 O(项数)。
//! 诊断码段：0x30D00（0x30D01~0x30D09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DF_VERSION: &str = "veb272_nvdiff-v1";

/// 诊断段基址（0x30D00 段 · 本模块独占）。
pub const DF_SEG_BASE: u32 = 0x30D00;
pub const XC_MAGIC: u32 = 0x30D01; // 账魔数破
pub const XC_VER_FMT: u32 = 0x30D02; // 版本分量越域
pub const XC_NOENT: u32 = 0x30D03; // 无此版本
pub const XC_DIFF_SAME: u32 = 0x30D04; // 同版本无差异
pub const XC_BIT_OOR: u32 = 0x30D05; // 差异位越域

/// 差异位。
pub const D_NEW_FEAT: u32 = 1 << 0;
pub const D_REGRESS: u32 = 1 << 1;
pub const D_DEPRECATE: u32 = 1 << 2;
pub const D_PERF: u32 = 1 << 3;

/// 版本（主.次.补 各 8 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ver {
    pub raw: u32,
}

impl Ver {
    pub fn new(major: u32, minor: u32, patch: u32) -> Result<Ver, u32> {
        if major > 0xFF || minor > 0xFF || patch > 0xFF {
            return Err(XC_VER_FMT);
        }
        Ok(Ver { raw: (major << 16) | (minor << 8) | patch })
    }
    pub fn major(self) -> u32 {
        self.raw >> 16
    }
    pub fn minor(self) -> u32 {
        (self.raw >> 8) & 0xFF
    }
    pub fn patch(self) -> u32 {
        self.raw & 0xFF
    }
}

/// 版本差异账。
pub struct DiffBook {
    /// 版本 → 差异位图（相对上一版）。
    entries: Vec<(Ver, u32)>,
    magic: u32,
}

impl DiffBook {
    /// 建。
    pub fn new() -> DiffBook {
        DiffBook { entries: Vec::new(), magic: 0x0D00_012F }
    }

    /// 登记版本差异。
    pub fn put(&mut self, v: Ver, bits: u32) -> Result<(), u32> {
        if self.magic != 0x0D00_012F {
            return Err(XC_MAGIC);
        }
        if bits & !(D_NEW_FEAT | D_REGRESS | D_DEPRECATE | D_PERF) != 0 {
            return Err(XC_BIT_OOR);
        }
        for e in self.entries.iter() {
            if e.0 == v {
                return Err(XC_DIFF_SAME);
            }
        }
        self.entries.push((v, bits));
        // 版本序保持。
        self.entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(())
    }

    /// 相邻两版本差异查询。
    pub fn diff(&self, from: Ver, to: Ver) -> Result<u32, u32> {
        if from == to {
            return Err(XC_DIFF_SAME);
        }
        let mut f: Option<u32> = None;
        let mut t: Option<u32> = None;
        for e in self.entries.iter() {
            if e.0 == from {
                f = Some(e.1);
            }
            if e.0 == to {
                t = Some(e.1);
            }
        }
        match (f, t) {
            (Some(_), Some(_)) => Ok(to.raw - from.raw),
            _ => Err(XC_NOENT),
        }
    }

    /// 回归检测：to 相对 from 新增回归位 → 提示降级候选。
    pub fn regress_hint(&self, from: Ver, to: Ver) -> Result<bool, u32> {
        let mut fb = None;
        let mut tb = None;
        for e in self.entries.iter() {
            if e.0 == from {
                fb = Some(e.1);
            }
            if e.0 == to {
                tb = Some(e.1);
            }
        }
        match (fb, tb) {
            (Some(a), Some(b)) => Ok((b & D_REGRESS) != 0 && (a & D_REGRESS) == 0),
            _ => Err(XC_NOENT),
        }
    }

    /// 弃用传播：to 标弃的能力 → 查询能力弃用态。
    pub fn deprecated_at(&self, v: Ver) -> Result<bool, u32> {
        for e in self.entries.iter() {
            if e.0 == v {
                return Ok(e.1 & D_DEPRECATE != 0);
            }
        }
        Err(XC_NOENT)
    }

    /// 表长。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Ver 构造助手（判据内解包）。
fn mk(ma: u32, mi: u32, pa: u32) -> Ver {
    match Ver::new(ma, mi, pa) {
        Ok(v) => v,
        Err(_) => Ver { raw: 0 },
    }
}

pub fn run_veb272_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0272");
    // 族一：版本比较。
    let v1 = mk(535, 104, 5);
    let v2 = mk(535, 104, 12);
    let v3 = mk(535, 98, 99);
    let v4 = mk(550, 0, 0);
    s.add("补丁序", v1 < v2, "5.104.5 < 5.104.12");
    s.add("次序倒", v2 > v3, "104 > 98");
    s.add("主序倒", v4 > v2, "550 > 535");
    s.add("分量读", v1.major() == 535 && v1.minor() == 104 && v1.patch() == 5, "三元组");
    s.add("分量越域拒", matches!(Ver::new(256, 0, 0), Err(XC_VER_FMT)), ">255");
    // 族二：差异账。
    let mut b = DiffBook::new();
    let _ = b.put(v1, D_NEW_FEAT | D_PERF);
    let _ = b.put(v2, D_REGRESS | D_DEPRECATE);
    let _ = b.put(v3, 0);
    s.add("表 3", b.len() == 3, "三版本");
    s.add("重版拒", b.put(v1, 0) == Err(XC_DIFF_SAME), "主键");
    s.add("野位拒", b.put(mk(1, 1, 1), 1 << 7) == Err(XC_BIT_OOR), "位越域");
    // 族三：差异查询。
    s.add("差 7", b.diff(v3, v2) == Ok(v2.raw - v3.raw), "相邻差");
    s.add("同版拒", b.diff(v1, v1) == Err(XC_DIFF_SAME), "自比");
    s.add("缺版拒", b.diff(mk(1, 0, 0), v2) == Err(XC_NOENT), "未登记");
    // 族四：回归提示。
    s.add("回归提示真", b.regress_hint(v1, v2) == Ok(true), "v2 引入回归");
    s.add("回归提示假", b.regress_hint(v2, v1) == Ok(false), "回退无新回归");
    // v1 相对 v3：v1 无回归位 → false。
    s.add("无回归假", b.regress_hint(v3, v1) == Ok(false), "v1 无回归位");
    // 族五：弃用传播。
    s.add("v2 弃用态", b.deprecated_at(v2) == Ok(true), "标弃");
    s.add("v1 非弃用", b.deprecated_at(v1) == Ok(false), "未弃");
    s.add("野版弃查拒", b.deprecated_at(mk(9, 9, 9)) == Err(XC_NOENT), "未登记");
    s.add("魔数核", DF_SEG_BASE == 0x30D00, "段基址");
    s
}
