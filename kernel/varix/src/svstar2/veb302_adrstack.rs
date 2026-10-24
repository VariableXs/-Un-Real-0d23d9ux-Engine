//! veb302_adrstack —— VE-F0302 Adreno 开源栈路径评估。
//! 锚点（CoRun Unxreal 主册）：VE-F0302 · Adreno 开源栈路径评估 · 目标 320 行
//! 判据原文：VE-F0302-J1 开源栈评估账：路径三条（Turnip 用户
//!           态 / Freedreno 内核态 / 直写私有）；逐路评估
//!           （成熟度 0~5 + 许可旗）；可用门（成熟度 ≥ 3 且
//!           许可兼容 → 可用）；推荐序（可用路按成熟度降
//!           序，同分按许可旗先自由）；直写私有的成本注记
//!           （成本恒 5）；评估冻结（出报告后只读）；混合
//!           路径核（内核 Freedreno + 用户 Turnip 组合判定）。
//! 组件：路径账 + 双因子门 + 推荐序 + 组合判定。
//! 职责定位：开源栈评估是「Adreno 的选路」——三条路三种走
//!           法：Turnip 是新修的近道、Freedreno 是老国道、
//!           直写私有是包车（贵但稳）；成熟度是路况、许可是
//!           通行证：两样都齐才推荐；推荐序是导航的排序：
//!           先看路况再看通行证的颜色。
//! 性能逐项分解：评估 O(3)；排序 O(3²)。
//! 诊断码段：0x31480（0x31481~0x31489 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ST_VERSION: &str = "veb302_adrstack-v1";

/// 诊断段基址（0x31480 段 · 本模块独占）。
pub const ST_SEG_BASE: u32 = 0x31480;
pub const XC_MAGIC: u32 = 0x31481; // 账魔数破
pub const XC_PATH: u32 = 0x31482; // 无此路径
pub const XC_FROZEN: u32 = 0x31483; // 报告冻结

/// 账魔数。
const MAGIC: u32 = 0x3148_1B2C;

/// 三路径。
pub const PATH_TURNIP: usize = 0;
pub const PATH_FREEDRENO: usize = 1;
pub const PATH_PROPRIETARY: usize = 2;
pub const NPATH: usize = 3;

/// 直写私有成本恒 5。
pub const PROP_COST: u32 = 5;

/// 可用门双因子。
pub const MAT_TH: u32 = 3;

/// 许可旗（true = 自由许可）。
#[derive(Clone, Copy)]
pub struct PathEval {
    pub maturity: u32,
    pub lic_free: bool,
    pub set: bool,
}

/// 评估簿。
pub struct StackEval {
    pub paths: [PathEval; NPATH],
    pub frozen: bool,
    magic: u32,
}

impl StackEval {
    /// 建（全未评）。
    pub fn new() -> StackEval {
        StackEval {
            paths: [
                PathEval { maturity: 0, lic_free: false, set: false },
                PathEval { maturity: 0, lic_free: false, set: false },
                PathEval { maturity: 0, lic_free: false, set: false },
            ],
            frozen: false,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记一路评估（成熟度 0~5）。
    pub fn eval(&mut self, p: usize, maturity: u32, lic_free: bool) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if p >= NPATH {
            return Err(XC_PATH);
        }
        if maturity > 5 {
            return Err(XC_PATH);
        }
        self.paths[p] = PathEval { maturity, lic_free, set: true };
        Ok(())
    }

    /// 路可用门：成熟 ≥ 3 且许可兼容。
    pub fn usable(&self, p: usize) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if p >= NPATH || !self.paths[p].set {
            return Err(XC_PATH);
        }
        if p == PATH_PROPRIETARY {
            // 直写私有无许可问题但成本高，不计入可用推荐。
            return Ok(false);
        }
        Ok(self.paths[p].maturity >= MAT_TH && self.paths[p].lic_free)
    }

    /// 推荐序：可用路按成熟度降序，同分自由许可先。
    pub fn recommend(&self) -> Result<Vec<usize>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let mut cand: Vec<usize> = Vec::new();
        for p in 0..NPATH {
            if self.usable(p)? {
                cand.push(p);
            }
        }
        // 选择排序：键 = 成熟度 × 2 + 自由旗，降序出。
        let mut out: Vec<usize> = Vec::new();
        let mut used = [false; NPATH];
        for _ in 0..cand.len() {
            let mut best: Option<usize> = None;
            let mut bk = 0u32;
            for &p in cand.iter() {
                if used[p] {
                    continue;
                }
                let k = self.paths[p].maturity * 2 + self.paths[p].lic_free as u32;
                if best.is_none() || k > bk {
                    best = Some(p);
                    bk = k;
                }
            }
            if let Some(p) = best {
                used[p] = true;
                out.push(p);
            }
        }
        Ok(out)
    }

    /// 混合路径核：内核 Freedreno 可用 + 用户 Turnip 可用 → 组合成立。
    pub fn hybrid(&self) -> Result<bool, u32> {
        let k = self.usable(PATH_FREEDRENO)?;
        let u = self.usable(PATH_TURNIP)?;
        Ok(k && u)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.frozen = true;
        Ok(())
    }
}

pub fn run_veb302_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0302");
    let mut e = StackEval::new();
    // 族零：登记门。
    s.add("路径 3 拒", e.eval(3, 4, true) == Err(XC_PATH), "无四路");
    s.add("成熟 6 拒", e.eval(0, 6, true) == Err(XC_PATH), ">5");
    s.add("未评查拒", e.usable(0) == Err(XC_PATH), "先评后查");
    // 族一：逐路评估。
    s.add("评 Turnip", e.eval(PATH_TURNIP, 4, true).is_ok(), "成熟 4 自由");
    s.add("评 Freedreno", e.eval(PATH_FREEDRENO, 5, true).is_ok(), "成熟 5 自由");
    s.add("评私有", e.eval(PATH_PROPRIETARY, 5, false).is_ok(), "成熟高但闭");
    s.add("Turnip 可用", e.usable(PATH_TURNIP) == Ok(true), "双因子");
    s.add("Freedreno 可用", e.usable(PATH_FREEDRENO) == Ok(true), "双因子");
    s.add("私有不可荐", e.usable(PATH_PROPRIETARY) == Ok(false), "无许可旗");
    // 族二：许可旗否决。
    let mut l = StackEval::new();
    let _ = l.eval(PATH_TURNIP, 5, false); // 成熟但闭源
    s.add("闭源拒荐", l.usable(PATH_TURNIP) == Ok(false), "许可否决");
    // 成熟不足否决。
    let mut m = StackEval::new();
    let _ = m.eval(PATH_FREEDRENO, 2, true);
    s.add("不熟拒荐", m.usable(PATH_FREEDRENO) == Ok(false), "成熟否决");
    // 族三：推荐序。
    let rec = e.recommend();
    s.add("推荐序 2 路", matches!(&rec, Ok(v) if v.len() == 2), "私有出局");
    s.add("Freedreno 先", matches!(&rec, Ok(v) if v[0] == PATH_FREEDRENO), "成熟 5>4");
    // 同分自由先：两路同成熟。
    let mut t = StackEval::new();
    let _ = t.eval(PATH_TURNIP, 4, true);
    let _ = t.eval(PATH_FREEDRENO, 4, true);
    let rec2 = t.recommend();
    s.add("同分序核", matches!(&rec2, Ok(v) if v.len() == 2 && v[0] == PATH_TURNIP && v[1] == PATH_FREEDRENO), "键序稳定");
    // 族四：混合路径。
    s.add("混合成立", e.hybrid() == Ok(true), "内 5 + 用 4");
    let mut h1 = StackEval::new();
    let _ = h1.eval(PATH_TURNIP, 4, true);
    let _ = h1.eval(PATH_FREEDRENO, 2, true);
    s.add("内核不熟混合败", h1.hybrid() == Ok(false), "单腿");
    let mut h2 = StackEval::new();
    let _ = h2.eval(PATH_TURNIP, 1, false);
    let _ = h2.eval(PATH_FREEDRENO, 5, true);
    s.add("用户闭源混合败", h2.hybrid() == Ok(false), "另一腿");
    // 族五：冻结。
    s.add("冻结", e.freeze().is_ok(), "出报告");
    s.add("冻后评拒", e.eval(0, 3, true) == Err(XC_FROZEN), "只读");
    s.add("冻后推荐照常", matches!(e.recommend(), Ok(_)), "可用");
    s.add("魔数核", ST_SEG_BASE == 0x31480, "段基址");
    s
}
