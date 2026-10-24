//! unx0411_monojump —— UNX-F0411 时钟跳变检出（单调性违例）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0411 · 时钟跳变检出 · 目标 200 行
//! 判据原文：UNX-F0411-J1 时钟读值流逐点核单调：负跳（回退）
//!           立案带跳量；正跳超阈（跳变幅度 > 阈值，疑似校时
//!           或频率突变）立案带跳量；平稳段（阈值内正跳）放
//!           行；跳变账（立案序列）按流序可回放；阈值可配
//!           （零阈 = 任何正跳立案，用于静止时钟验收）。
//! 组件：读值流 + 跳变分析（差分 + 双向立案）+ 跳变账 + 回放
//!       核 + 阈值闸。
//! 职责定位：跳变检出是「时钟的测谎仪」——单调流里任何异常
//!   跳动都有两种身份：负跳是铁案（时钟不回退是硬承诺），
//!   正跳要看幅度：小跳是正常滴答，大跳是校时/改频事件，必
//!   须显式立案不许静默混过。阈值零是最严模式：任何正跳都
//!   要交代，用于验收纯计数器（无校时通道）。
//! 性能逐项分解：逐点 O(1)；立案 O(1)/点；回放 O(立案数)。
//! 诊断码段：0x21040（0x21041~0x21049 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MJ_VERSION: &str = "unx0411_monojump-v1";

/// 诊断段基址（0x21040 段 · 本模块独占）。
pub const MJ_SEG_BASE: u32 = 0x21040;
pub const XC_NEG_JUMP: u32 = 0x21041; // 负跳立案
pub const XC_POS_JUMP: u32 = 0x21042; // 正跳超阈立案
pub const XC_THRESH_OOR: u32 = 0x21043; // 阈值越域
pub const XC_FEW: u32 = 0x21044; // 流太短（<2）
pub const XC_REPLAY: u32 = 0x21045; // 回放不一致
pub const XC_CASE_LOST: u32 = 0x21046; // 立案丢失核破
pub const XC_MAGIC: u32 = 0x21047; // 阈值魔数破

/// 跳变案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JumpCase {
    /// 流位（1 起）。
    pub at: u32,
    /// 种别：1 = 负跳，2 = 正跳超阈。
    pub kind: u8,
    /// 跳量（绝对值）。
    pub amount: u64,
}

/// 跳变分析器。
pub struct JumpAnalyzer {
    seq: Vec<u64>,
    thresh: u64,
    cases: Vec<JumpCase>,
}

/// 阈值上限。
pub const THRESH_MAX: u64 = 1 << 40;

impl JumpAnalyzer {
    /// 建：正跳阈值。
    pub fn new(thresh: u64) -> Result<JumpAnalyzer, u32> {
        if thresh > THRESH_MAX {
            return Err(XC_THRESH_OOR);
        }
        Ok(JumpAnalyzer { seq: Vec::new(), thresh, cases: Vec::new() })
    }

    /// 追加读值。
    pub fn feed(&mut self, t: u64) {
        self.seq.push(t);
    }

    /// 全流分析：负跳与超阈正跳立案。
    pub fn analyze(&mut self) -> Result<u32, u32> {
        self.cases.clear();
        let n = self.seq.len();
        if n < 2 {
            return Err(XC_FEW);
        }
        for i in 1..n {
            let prev = self.seq[i - 1];
            let cur = self.seq[i];
            if cur < prev {
                self.cases.push(JumpCase { at: i as u32, kind: 1, amount: prev - cur });
            } else {
                let d = cur - prev;
                if d > self.thresh {
                    self.cases.push(JumpCase { at: i as u32, kind: 2, amount: d });
                }
            }
        }
        Ok(self.cases.len() as u32)
    }

    /// 案表读取。
    pub fn cases(&self) -> &[JumpCase] {
        &self.cases
    }

    /// 回放核：案表重算一致。
    pub fn replay_ok(&self) -> Result<(), u32> {
        if self.seq.len() < 2 {
            return Err(XC_FEW);
        }
        let mut ref_cases: Vec<JumpCase> = Vec::new();
        for i in 1..self.seq.len() {
            let prev = self.seq[i - 1];
            let cur = self.seq[i];
            if cur < prev {
                ref_cases.push(JumpCase { at: i as u32, kind: 1, amount: prev - cur });
            } else if cur - prev > self.thresh {
                ref_cases.push(JumpCase { at: i as u32, kind: 2, amount: cur - prev });
            }
        }
        if ref_cases != self.cases {
            return Err(XC_REPLAY);
        }
        Ok(())
    }

    /// 立案丢失核：analyze 后案数与账一致（自洽）。
    pub fn cases_consistent(&self, n: u32) -> Result<(), u32> {
        if self.cases.len() as u32 != n {
            return Err(XC_CASE_LOST);
        }
        Ok(())
    }

    /// 流长。
    pub fn len(&self) -> usize {
        self.seq.len()
    }

    /// 阈值魔数核。
    pub fn magic_ok(&self) -> Result<(), u32> {
        if self.thresh > THRESH_MAX {
            return Err(XC_MAGIC);
        }
        Ok(())
    }
}

pub fn run_unx0411_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0411_monojump");
    // 族一：平稳流（阈值 1000，滴答 5）。
    let mut a = match JumpAnalyzer::new(1000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建分析器", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 5, 10, 15, 20].iter() {
        a.feed(*t);
    }
    cs.add("流长 5", a.len() == 5, "四差分");
    cs.add("平稳无案", a.analyze() == Ok(0), "滴答内");
    cs.add("回放核过", a.replay_ok().is_ok(), "案表空自洽");
    // 族二：负跳检出。
    let mut b = match JumpAnalyzer::new(1000) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建2", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 50, 30, 90].iter() {
        b.feed(*t);
    }
    cs.add("负跳立案 1", b.analyze() == Ok(1), "位 2 回退 20");
    cs.add("案详情", b.cases() == &[JumpCase { at: 2, kind: 1, amount: 20 }], "负跳 20");
    // 族三：正跳超阈检出。
    let mut c = match JumpAnalyzer::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建3", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 50, 500, 560].iter() {
        c.feed(*t);
    }
    cs.add("正跳立案 1", c.analyze() == Ok(1), "位 2 跳 450 > 100");
    cs.add("正跳案详情", c.cases() == &[JumpCase { at: 2, kind: 2, amount: 450 }], "正跳 450");
    // 族四：混合流（负 + 正超 + 平稳）。
    let mut d = match JumpAnalyzer::new(100) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建4", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 10, 5, 500, 505, 700].iter() {
        d.feed(*t);
    }
    // 差分：+10, -5(案1), +495(案2), +5, +195(案3)。
    cs.add("混合立案 3", d.analyze() == Ok(3), "一负两正超");
    let exp = [
        JumpCase { at: 2, kind: 1, amount: 5 },
        JumpCase { at: 3, kind: 2, amount: 495 },
        JumpCase { at: 5, kind: 2, amount: 195 },
    ];
    cs.add("混合案序", d.cases() == &exp[..], "按流序");
    cs.add("案数自洽", d.cases_consistent(3).is_ok(), "3 = 3");
    cs.add("案数失配检出", d.cases_consistent(9) == Err(XC_CASE_LOST), "9 ≠ 3");
    cs.add("混合回放核", d.replay_ok().is_ok(), "重算一致");
    // 族五：零阈（最强模式）。
    let mut e = match JumpAnalyzer::new(0) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建5", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 1, 2, 3].iter() {
        e.feed(*t);
    }
    cs.add("零阈全立案", e.analyze() == Ok(3), "每个正跳都案");
    let mut f = match JumpAnalyzer::new(0) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建6", false, "拒");
            return cs;
        }
    };
    for t in [7u64, 7, 7].iter() {
        f.feed(*t);
    }
    cs.add("静止零跳无案", f.analyze() == Ok(0), "d=0 非正跳");
    // 族六：参数拒绝。
    cs.add("阈值超域拒", matches!(JumpAnalyzer::new(THRESH_MAX + 1), Err(XC_THRESH_OOR)), ">2^40");
    cs.add("阈值上限过", JumpAnalyzer::new(THRESH_MAX).is_ok(), "=2^40");
    let mut g = match JumpAnalyzer::new(10) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建7", false, "拒");
            return cs;
        }
    };
    g.feed(1);
    cs.add("单点拒", g.analyze() == Err(XC_FEW), "n=1");
    cs.add("魔数核", g.magic_ok().is_ok(), "阈值域内");
    cs
}
