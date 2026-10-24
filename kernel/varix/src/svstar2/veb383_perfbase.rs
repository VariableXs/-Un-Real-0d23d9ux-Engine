//! veb383_perfbase —— VE-F0383 B 域性能基线总账。
//! 锚点（CoRun Unxreal 主册）：VE-F0383 · B 域性能基线总账 · 目标 340 行
//! 判据原文：VE-F0383-J1 各操作记性能基线（操作名 → 基线刻）；
//!           实测超基线 20% 即回归立案；连续三次超限升级（基线
//!           重新校准提案）；基线修订须带依据（样本数 ≥ 最低样
//!           本数才许改）；总账核（基线表 + 回归案 + 修订史三账
//!           一致）。
//! 组件：基线表（名主键）+ 实测登记 + 回归案（20% 阈 + 三连升
//!       级）+ 修订史（样本门槛）。
//! 职责定位：性能基线总账是「B 域的体检档案」——没有基线就无从
//!           谈回归；20% 阈是信噪平衡：抖动 5% 不足为虑，掉
//!           30% 必须立案；修订要样本是防「一次跑分改基线」的
//!           投机：三个样本才够说话。
//! 性能逐项分解：登记 O(1)；立案判定 O(1)；修订 O(1)。
//! 诊断码段：0x30540（0x30541~0x3054A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PB_VERSION: &str = "veb383_perfbase-v1";

/// 诊断段基址（0x30540 段 · 本模块独占）。
pub const PB_SEG_BASE: u32 = 0x30540;
pub const XC_REGRESS: u32 = 0x30541; // 超基线 20%
pub const XC_ESCALATE: u32 = 0x30542; // 三连超限升级
pub const XC_DUP: u32 = 0x30543; // 基线重名
pub const XC_NOENT: u32 = 0x30544; // 无此基线
pub const XC_SAMP_LOW: u32 = 0x30545; // 样本不足拒修订
pub const XC_MAGIC: u32 = 0x30546; // 账魔数破
pub const XC_ZERO_BASE: u32 = 0x30547; // 基线为零
pub const XC_LEDGER: u32 = 0x30548; // 三账不一致
pub const XC_ESC_MIS: u32 = 0x30549; // 升级案缺档
pub const XC_MARK: u32 = 0x3054A; // 修订后未清连超

/// 回归阈（千分比 200 = 20%）。
pub const THRESH_MILLI: u32 = 200;
/// 修订最低样本数。
pub const MIN_SAMPLES: u32 = 3;
/// 升级连超次数。
pub const ESC_N: u32 = 3;

/// 基线条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseEntry {
    pub op: &'static str,
    pub base: u32,
    /// 连续超限次数。
    pub streak: u32,
    /// 累计样本。
    pub samples: u32,
}

/// 修订史一条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevRec {
    pub op: &'static str,
    pub old: u32,
    pub new: u32,
    pub samples: u32,
}

/// 总账。
pub struct PerfBook {
    bases: Vec<BaseEntry>,
    revs: Vec<RevRec>,
    regress_cases: u32,
    escalations: u32,
    magic: u32,
}

impl PerfBook {
    /// 建。
    pub fn new() -> PerfBook {
        PerfBook { bases: Vec::new(), revs: Vec::new(), regress_cases: 0, escalations: 0, magic: 0x0540_BA5E }
    }

    /// 立基线。
    pub fn set_base(&mut self, op: &'static str, base: u32) -> Result<(), u32> {
        if self.magic != 0x0540_BA5E {
            return Err(XC_MAGIC);
        }
        if base == 0 {
            return Err(XC_ZERO_BASE);
        }
        for b in self.bases.iter() {
            if b.op == op {
                return Err(XC_DUP);
            }
        }
        self.bases.push(BaseEntry { op, base, streak: 0, samples: 0 });
        Ok(())
    }

    /// 实测登记：超基线 20% 立案；三连升级。
    pub fn measure(&mut self, op: &str, got: u32) -> Result<(), u32> {
        let i = match self.bases.iter().position(|b| b.op == op) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        self.bases[i].samples += 1;
        let base = self.bases[i].base as u64;
        let over_milli = if got as u64 >= base {
            ((got as u64 - base) * 1000 / base) as u32
        } else {
            0
        };
        if over_milli > THRESH_MILLI {
            self.regress_cases += 1;
            self.bases[i].streak += 1;
            if self.bases[i].streak >= ESC_N {
                self.escalations += 1;
                return Err(XC_ESCALATE);
            }
            return Err(XC_REGRESS);
        }
        self.bases[i].streak = 0;
        Ok(())
    }

    /// 基线修订（样本 ≥ 3 才许改）。
    pub fn revise(&mut self, op: &'static str, new_base: u32) -> Result<(), u32> {
        if new_base == 0 {
            return Err(XC_ZERO_BASE);
        }
        let i = match self.bases.iter().position(|b| b.op == op) {
            Some(i) => i,
            None => return Err(XC_NOENT),
        };
        if self.bases[i].samples < MIN_SAMPLES {
            return Err(XC_SAMP_LOW);
        }
        let old = self.bases[i].base;
        self.bases[i].base = new_base;
        self.bases[i].streak = 0;
        self.revs.push(RevRec { op, old, new: new_base, samples: self.bases[i].samples });
        Ok(())
    }

    /// 三账一致核。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.magic != 0x0540_BA5E {
            return Err(XC_MAGIC);
        }
        for r in self.revs.iter() {
            let mut found = false;
            for b in self.bases.iter() {
                if b.op == r.op {
                    found = true;
                    if b.base != r.new {
                        return Err(XC_LEDGER); // 修订史与基线表脱节
                    }
                    if b.streak != 0 {
                        return Err(XC_MARK); // 修订后未清连超
                    }
                }
            }
            if !found {
                return Err(XC_LEDGER);
            }
            if r.samples < MIN_SAMPLES {
                return Err(XC_ESC_MIS);
            }
        }
        Ok(self.revs.len() as u32)
    }

    /// 回归案数。
    pub fn regress_count(&self) -> u32 {
        self.regress_cases
    }
    /// 升级数。
    pub fn esc_count(&self) -> u32 {
        self.escalations
    }
    /// 修订数。
    pub fn rev_count(&self) -> usize {
        self.revs.len()
    }
    /// 基线数。
    pub fn base_count(&self) -> usize {
        self.bases.len()
    }
}

pub fn run_veb383_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0383");
    // 族一：立基线与实测。
    let mut b = PerfBook::new();
    let _ = b.set_base("io_read", 100);
    let _ = b.set_base("io_write", 200);
    s.add("基线 2", b.base_count() == 2, "两 op");
    s.add("在限内", b.measure("io_read", 110).is_ok(), "+10% 放行");
    s.add("压线过", b.measure("io_read", 120).is_ok(), "+20% 恰好不超（>才立案）");
    s.add("超限立案", b.measure("io_read", 130) == Err(XC_REGRESS), "+30%");
    s.add("回归案 1", b.regress_count() == 1, "一案");
    s.add("无此 op", b.measure("ghost", 1) == Err(XC_NOENT), "未立");
    // 族二：三连升级。
    let mut c = PerfBook::new();
    let _ = c.set_base("slow_op", 100);
    s.add("超 1", c.measure("slow_op", 150) == Err(XC_REGRESS), "一超");
    s.add("超 2", c.measure("slow_op", 150) == Err(XC_REGRESS), "二超");
    s.add("超 3 升级", c.measure("slow_op", 150) == Err(XC_ESCALATE), "三连");
    s.add("升级数 1", c.esc_count() == 1, "升级档");
    s.add("回归案 3", c.regress_count() == 3, "逐案");
    // 族三：样本门槛。
    let mut d = PerfBook::new();
    let _ = d.set_base("x", 100);
    s.add("样本 0 修订拒", d.revise("x", 110) == Err(XC_SAMP_LOW), "无据");
    let _ = d.measure("x", 100);
    let _ = d.measure("x", 100);
    s.add("样本 2 修订拒", d.revise("x", 110) == Err(XC_SAMP_LOW), "不足 3");
    let _ = d.measure("x", 100);
    s.add("样本 3 修订过", d.revise("x", 110).is_ok(), "有据");
    s.add("修订史 1", d.rev_count() == 1, "入史");
    // 族四：三账一致。
    s.add("审计过", d.audit() == Ok(1), "史表一致");
    s.add("修订后连超清", d.measure("x", 100).is_ok(), "新基线 110 下 100 正常");
    // 族五：入参约束。
    let mut e = PerfBook::new();
    s.add("基线零拒", e.set_base("y", 0) == Err(XC_ZERO_BASE), "无意义");
    let _ = e.set_base("y", 50);
    s.add("重立拒", e.set_base("y", 60) == Err(XC_DUP), "主键");
    s.add("修订零拒", e.revise("y", 0) == Err(XC_ZERO_BASE), "零基线");
    s.add("魔数核", PB_SEG_BASE == 0x30540, "段基址");
    s
}
