//! veb282_mlidrm —— VE-F0282 Mali DRM 接口对接评估。
//! 锚点（CoRun Unxreal 主册）：VE-F0282 · Mali DRM 接口对接评估 · 目标 420 行
//! 判据原文：VE-F0282-J1 DRM 对接评估账：接口清单（五类：
//!           版本 / GEM / 渲染节点 / 同步 / 权限）；逐项评估
//!           （可用 / 缺失 / 半支持三态）；可用度评分（可用
//!           20 + 半支持 8 + 缺失 0，满分 100）；对接门槛
//!           （评分 ≥ 60 才准入）；关键项一票否决（GEM 或
//!           渲染节点缺失 → 直接拒）；半支持补测（半支持项
//!           补测过 → 升可用重算）；评估报告冻结（出报告后
//!           清单不可改）。
//! 组件：接口清单 + 三态评估 + 评分器 + 否决闸 + 冻结。
//! 职责定位：DRM 对接评估是「Mali 上车前的安检」——五类接
//!           口是五个安全带：缺哪个都系不紧；关键项否决是
//!           底线逻辑：GEM 缺了寸步难行，其余项再好也白搭；
//!           半支持补测给的是机会不是纵容：测得过就升，测
//!           不过照旧。
//! 性能逐项分解：评估 O(5)；评分 O(5)。
//! 诊断码段：0x30F80（0x30F81~0x30F89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DR_VERSION: &str = "veb282_mlidrm-v1";

/// 诊断段基址（0x30F80 段 · 本模块独占）。
pub const DR_SEG_BASE: u32 = 0x30F80;
pub const XC_MAGIC: u32 = 0x30F81; // 账魔数破
pub const XC_NOITEM: u32 = 0x30F82; // 无此接口项
pub const XC_FROZEN: u32 = 0x30F83; // 报告已冻结
pub const XC_VETO: u32 = 0x30F84; // 关键项否决

/// 账魔数。
const MAGIC: u32 = 0x0F80_1D12;

/// 五类接口。
pub const ITEMS: [&str; 5] = ["version", "gem", "render_node", "sync", "perm"];

/// 关键项（一票否决）。
pub const VITAL: [usize; 2] = [1, 2]; // gem / render_node

/// 三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avail {
    /// 可用。
    Ok,
    /// 半支持。
    Half,
    /// 缺失。
    Miss,
}

/// 评估簿。
pub struct DrmEval {
    states: [Avail; 5],
    /// 补测过的项。
    pub retested: Vec<usize>,
    /// 冻结旗。
    pub frozen: bool,
    magic: u32,
}

impl DrmEval {
    /// 建账：默认全缺失。
    pub fn new() -> DrmEval {
        DrmEval {
            states: [Avail::Miss; 5],
            retested: Vec::new(),
            frozen: false,
            magic: MAGIC,
        }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 记某项状态。
    pub fn set(&mut self, item: usize, st: Avail) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if item >= 5 {
            return Err(XC_NOITEM);
        }
        self.states[item] = st;
        Ok(())
    }

    /// 查状态。
    pub fn state(&self, item: usize) -> Result<Avail, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if item >= 5 {
            return Err(XC_NOITEM);
        }
        Ok(self.states[item])
    }

    /// 关键项否决闸。
    pub fn veto(&self) -> Result<(), u32> {
        for v in VITAL.iter() {
            if self.states[*v] == Avail::Miss {
                return Err(XC_VETO);
            }
        }
        Ok(())
    }

    /// 评分：可用 20 + 半支持 8。
    pub fn score(&self) -> u32 {
        let mut sc = 0u32;
        for st in self.states.iter() {
            sc += match st {
                Avail::Ok => 20,
                Avail::Half => 8,
                Avail::Miss => 0,
            };
        }
        sc
    }

    /// 准入门槛：先过否决闸再评 ≥ 60。
    pub fn admit(&self) -> Result<u32, u32> {
        self.veto()?;
        let sc = self.score();
        if sc >= 60 {
            Ok(sc)
        } else {
            Err(sc)
        }
    }

    /// 半支持补测：补测过 → 升可用。
    pub fn retest(&mut self, item: usize, ok: bool) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if item >= 5 {
            return Err(XC_NOITEM);
        }
        if self.states[item] != Avail::Half {
            // 只有半支持项可补测。
            return Ok(());
        }
        self.retested.push(item);
        if ok {
            self.states[item] = Avail::Ok;
        }
        Ok(())
    }

    /// 冻结报告。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.frozen = true;
        Ok(())
    }
}

pub fn run_veb282_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0282");
    // 族零：配置。
    let mut e = DrmEval::new();
    s.add("无项 5 拒", e.set(5, Avail::Ok) == Err(XC_NOITEM), "越界");
    s.add("默认全缺", e.score() == 0, "零分");
    // 族一：逐项评估。
    s.add("版本可用", e.set(0, Avail::Ok).is_ok(), "item0");
    s.add("GEM 可用", e.set(1, Avail::Ok).is_ok(), "item1");
    s.add("节点半支", e.set(2, Avail::Half).is_ok(), "item2");
    s.add("同步可用", e.set(3, Avail::Ok).is_ok(), "item3");
    s.add("权限缺失", e.state(4) == Ok(Avail::Miss), "默认缺");
    s.add("评分 68", e.score() == 68, "20+20+8+20");
    // 族二：否决闸。
    let mut v = DrmEval::new();
    let _ = v.set(0, Avail::Ok);
    let _ = v.set(2, Avail::Ok);
    let _ = v.set(3, Avail::Ok);
    let _ = v.set(4, Avail::Ok);
    s.add("GEM 缺否决", v.admit() == Err(XC_VETO), "关键项");
    let _ = v.set(1, Avail::Ok);
    let _ = v.set(2, Avail::Miss);
    s.add("节点缺否决", v.admit() == Err(XC_VETO), "另一关键");
    // 节点半支持不否决。
    let _ = v.set(2, Avail::Half);
    s.add("半支不否决", v.veto().is_ok(), "门槛看评分");
    s.add("半支 68 过", v.admit() == Ok(68), "≥60");
    // 族三：半支持补测。
    let mut r = DrmEval::new();
    for i in 0..5 {
        let _ = r.set(i, Avail::Half);
    }
    s.add("全半 40", r.score() == 40, "5×8");
    s.add("全半不过", r.admit().is_err(), "40<60");
    // 补测三过两败。
    s.add("补 0 过", r.retest(0, true).is_ok(), "升可用");
    s.add("补 1 过", r.retest(1, true).is_ok(), "升可用");
    s.add("补 2 过", r.retest(2, true).is_ok(), "升可用");
    s.add("补 3 败", r.retest(3, false).is_ok(), "留半");
    s.add("补后 76", r.score() == 76, "3×20+2×8");
    s.add("补后准入", r.admit() == Ok(76), "≥60");
    s.add("补测账 3", r.retested.len() == 3, "三笔");
    // 非半支持项补测空转。
    s.add("可用项补测空", r.retest(0, true).is_ok() && r.score() == 76, "幂等");
    // 族四：冻结。
    s.add("冻结", r.freeze().is_ok(), "出报告");
    s.add("冻结后改拒", r.set(4, Avail::Ok) == Err(XC_FROZEN), "不可改");
    s.add("冻结后补拒", r.retest(3, true) == Err(XC_FROZEN), "不可补");
    s.add("冻结后再冻", r.freeze().is_ok(), "幂等");
    // 族五：评分满分核。
    let mut f = DrmEval::new();
    for i in 0..5 {
        let _ = f.set(i, Avail::Ok);
    }
    s.add("满分 100", f.score() == 100, "5×20");
    s.add("满分准入", f.admit() == Ok(100), "全绿");
    // 半支持给 8 分不是 10。
    let mut h = DrmEval::new();
    let _ = h.set(0, Avail::Half);
    s.add("半支 8 分", h.score() == 8, "口径");
    s.add("魔数核", DR_SEG_BASE == 0x30F80, "段基址");
    s
}
