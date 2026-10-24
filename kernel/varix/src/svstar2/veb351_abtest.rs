//! veb351_abtest —— VE-F0351 软渲与硬件路径总对拍。
//! 锚点：VE-F0351 · 目标 400 行。
//! 判据原文：子集对拍（对拍子集选自 2D/3D 清单交集）、差异
//!           分类（舍入级可接受 / 语义级不可接受）、语义级
//!           阻断（语义级差异未处理阻断对拍通过）、版本化
//!           （对拍结果版本化随回归门引用）、判据。
//! 组件：对拍器 + 差异分类账 + 版本闸。
//! 职责定位：对拍是「双路复核的验钞机」——同一笔输入两条路
//!           各出一版，逐像素对光验真；色差在容差内是印钞
//!           瑕疵（舍入级），图案错了是假钞（语义级）——假
//!           钞不过机，账本封版。
//! 性能逐项分解：对拍 O(像素数×子集) 限场景；分类 O(1) 判
//!           定；版本 O(1)。
//! 诊断码段：0x320C0（0x320C1~0x320C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AB_VERSION: &str = "veb351_abtest-v1";

/// 诊断段基址（0x320C0 段 · 本模块独占）。
pub const AB_SEG_BASE: u32 = 0x320C0;
pub const XC_SEM: u32 = 0x320C1; // 语义级差异（阻断）
pub const XC_NA: u32 = 0x320C2; // 单路径缺席（对拍 N/A）
pub const XC_LEN: u32 = 0x320C3; // 像素数不等
pub const XC_UNHAND: u32 = 0x320C4; // 语义级差异未处理
pub const XC_TOL: u32 = 0x320C5; // 容差口径非法

/// 舍入级容差上限（1/255 单位）。
pub const TOL_MAX: u32 = 8;

/// 差异分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diff {
    /// 无差异。
    Same,
    /// 舍入级（容差内）。
    Round,
    /// 语义级（超容差）。
    Sem,
}

/// 分类。
pub fn classify(a: u32, b: u32, tol: u32) -> Diff {
    if a == b {
        return Diff::Same;
    }
    let d = if a > b { a - b } else { b - a };
    if d <= tol {
        Diff::Round
    } else {
        Diff::Sem
    }
}

/// 对拍结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Abof {
    /// 语义级差异位置账（像素号 → 分类）。
    pub sems: Vec<u32>,
    pub rounds: u32,
    pub same: u32,
    /// 已处理语义级差异数（登记修复或标注）。
    pub handled: u32,
}

/// 对拍器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbTest {
    tol: u32,
}

impl AbTest {
    /// 建：容差夹界。
    pub fn new(tol: u32) -> Result<AbTest, u32> {
        if tol > TOL_MAX {
            return Err(XC_TOL);
        }
        Ok(AbTest { tol })
    }

    /// 双路径对拍：单路径缺席 N/A；长度不等拒。
    pub fn run(&self, sim: &[u32], hw: &[u32]) -> Result<Abof, u32> {
        if sim.is_empty() || hw.is_empty() {
            return Err(XC_NA);
        }
        if sim.len() != hw.len() {
            return Err(XC_LEN);
        }
        let mut o = Abof {
            sems: Vec::new(),
            rounds: 0,
            same: 0,
            handled: 0,
        };
        for (i, (a, b)) in sim.iter().zip(hw.iter()).enumerate() {
            match classify(*a, *b, self.tol) {
                Diff::Same => o.same += 1,
                Diff::Round => o.rounds += 1,
                Diff::Sem => o.sems.push(i as u32),
            }
        }
        Ok(o)
    }

    /// 裁决：语义级差异全处理才许通过。
    pub fn verdict(o: &Abof) -> Result<(), u32> {
        if o.sems.len() > o.handled as usize {
            return Err(XC_UNHAND);
        }
        Ok(())
    }

    /// 处理一条语义级差异（登记修复或显性标注）。
    pub fn handle(o: &mut Abof, px: u32) -> Result<(), u32> {
        if !o.sems.contains(&px) {
            return Err(XC_SEM);
        }
        o.handled += 1;
        Ok(())
    }
}

/// 对拍版本账（随回归门引用）。
pub struct VerLedger {
    pub vers: Vec<(u32, u32)>, // (版本, 通过像素对数)
}

impl VerLedger {
    pub fn new() -> VerLedger {
        VerLedger { vers: Vec::new() }
    }

    /// 封版：裁决过才许入账。
    pub fn seal(&mut self, ver: u32, o: &Abof) -> Result<(), u32> {
        AbTest::verdict(o)?;
        self.vers.push((ver, o.same));
        Ok(())
    }

    /// 版本数。
    pub fn len(&self) -> usize {
        self.vers.len()
    }
}

pub fn run_veb351_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0351");
    // 族零：分类器。
    s.add("同值同", classify(100, 100, 4) == Diff::Same, "无差");
    s.add("差 3 舍入", classify(100, 103, 4) == Diff::Round, "容差内");
    s.add("差 5 舍入", classify(103, 100, 5) == Diff::Round, "恰容差");
    s.add("差 6 语义", classify(100, 106, 5) == Diff::Sem, "超容差");
    s.add("反向语义", classify(106, 100, 5) == Diff::Sem, "对称");
    // 族一：容差门。
    s.add("容差爆界拒", matches!(AbTest::new(9), Err(XC_TOL)), ">8");
    let ab = match AbTest::new(4) {
        Ok(x) => x,
        Err(_) => {
            s.add("建败", false, "不应到");
            return s;
        }
    };
    // 族二：对拍。
    s.add("空路径 N/A", matches!(ab.run(&[], &[]), Err(XC_NA)), "单路缺席");
    s.add("长不等拒", matches!(ab.run(&[1, 2], &[1]), Err(XC_LEN)), "不齐");
    let o1 = match ab.run(&[100, 103, 250], &[100, 104, 0]) {
        Ok(x) => x,
        Err(_) => {
            s.add("拍败", false, "不应到");
            return s;
        }
    };
    s.add("同一路", o1.same == 1, "px0");
    s.add("舍入一路", o1.rounds == 1, "px1 差 1");
    s.add("语义一路", o1.sems == vec![2u32], "px2 250vs0");
    // 族三：语义级阻断。
    s.add("未处理阻断", AbTest::verdict(&o1) == Err(XC_UNHAND), "拦");
    // 重新构造处理。
    let mut o2 = match ab.run(&[100, 103, 250], &[100, 104, 0]) {
        Ok(x) => x,
        Err(_) => {
            s.add("拍败 2", false, "不应到");
            return s;
        }
    };
    s.add("登记处理", AbTest::handle(&mut o2, 2).is_ok(), "标注豁免");
    s.add("处理账一", o2.handled == 1, "一笔");
    s.add("处理后过", AbTest::verdict(&o2).is_ok(), "放行");
    s.add("处理不存在的拒", AbTest::handle(&mut o2, 9) == Err(XC_SEM), "无此差异");
    // 族四：全同直过。
    let o3 = match ab.run(&[7, 7, 7], &[7, 7, 7]) {
        Ok(x) => x,
        Err(_) => {
            s.add("拍败 3", false, "不应到");
            return s;
        }
    };
    s.add("全同", o3.same == 3 && o3.sems.is_empty(), "零差异");
    s.add("直过", AbTest::verdict(&o3).is_ok(), "无阻断");
    // 族五：版本账。
    let mut v = VerLedger::new();
    s.add("未清封版拒", v.seal(1, &o1).is_err(), "o1 语义未处理");
    s.add("封版过", v.seal(1, &o3).is_ok(), "全同可封");
    s.add("封版二", v.seal(2, &o2).is_ok(), "处理后可封");
    s.add("账二", v.len() == 2, "两版");
    // 全语义差异大样本。
    let sim: Vec<u32> = (0..64u32).map(|x| x * 4).collect();
    let hw: Vec<u32> = (0..64u32).map(|x| x * 4 + 2).collect();
    let o4 = match ab.run(&sim, &hw) {
        Ok(x) => x,
        Err(_) => {
            s.add("拍败 4", false, "不应到");
            return s;
        }
    };
    s.add("全舍入", o4.rounds == 64, "差 2 容差内");
    s.add("零语义", o4.sems.is_empty(), "无阻断");
    s.add("魔数核", AB_SEG_BASE == 0x320C0, "段基址");
    s
}
