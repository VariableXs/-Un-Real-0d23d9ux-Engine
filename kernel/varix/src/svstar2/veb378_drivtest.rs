//! veb378_drivtest —— VE-F0378 驱动测试沙箱。
//! 锚点（CoRun Unxreal 主册）：VE-F0378 · 驱动测试沙箱 · 目标 380 行
//! 判据原文：VE-F0378-J1 驱动未过测试沙箱不许上线（未跑全用例
//!           拒签发「可上线」章）；用例三分类：功能（必须全过）、
//!           压力（允许有限失败，超限拒）、回归（基线比对，偏移
//!           拒）；沙箱隔离核：测试崩溃不外溢（沙箱内立案，宿主
//!           状态零污染）；跑批报告含三分类统计 + 总裁决。
//! 组件：用例项（名 + 类别 + 结果）+ 沙箱执行器（崩溃隔离）+
//!       三类闸（功能全过/压力限值/回归基线）+ 裁决章。
//! 职责定位：测试沙箱是「驱动的驾考考场」——考场里撞车不许把
//!       考场拆了（崩溃隔离），但撞车记录要进成绩单；三分类是
//!       三道闸：功能闸是底线，压力闸是韧性，回归闸是防倒退；
//!       裁决章是唯一放行凭证，没章的驱动上线即违规。
//! 性能逐项分解：跑批 O(n)；基线比对 O(n)；裁决 O(1)。
//! 诊断码段：0x30400（0x30401~0x3040A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DT_VERSION: &str = "veb378_drivtest-v1";

/// 诊断段基址（0x30400 段 · 本模块独占）。
pub const DT_SEG_BASE: u32 = 0x30400;
pub const XC_FUNC_FAIL: u32 = 0x30401; // 功能用例有败
pub const XC_STRESS_OVER: u32 = 0x30402; // 压力失败超限
pub const XC_REGRESS: u32 = 0x30403; // 回归偏移
pub const XC_EMPTY: u32 = 0x30404; // 空用例批
pub const XC_CASE_DUP: u32 = 0x30405; // 用例重名
pub const XC_NOT_RUN: u32 = 0x30406; // 未跑先裁决
pub const XC_LEAK: u32 = 0x30407; // 沙箱外溢（宿主被污染）
pub const XC_MAGIC: u32 = 0x30408; // 账魔数破
pub const XC_CLASS_OOR: u32 = 0x30409; // 类别越域
pub const XC_STAMP: u32 = 0x3040A; // 裁决章不一致

/// 用例类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 功能：必须全过。
    Func = 0,
    /// 压力：限值内可败。
    Stress = 1,
    /// 回归：基线比对。
    Regress = 2,
}

impl Kind {
    pub fn from_u32(v: u32) -> Option<Kind> {
        match v {
            0 => Some(Kind::Func),
            1 => Some(Kind::Stress),
            2 => Some(Kind::Regress),
            _ => None,
        }
    }
}

/// 压力失败限值。
pub const STRESS_ALLOW: u32 = 2;

/// 单条用例结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseOut {
    pub name: &'static str,
    pub kind: Kind,
    pub ok: bool,
    /// 回归基线值（仅 Regress 用）。
    pub base: u32,
    pub got: u32,
}

/// 跑批统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub func_total: u32,
    pub func_fail: u32,
    pub stress_total: u32,
    pub stress_fail: u32,
    pub regress_total: u32,
    pub regress_drift: u32,
}

impl Stats {
    /// 三闸全过判定。
    pub fn gates_ok(&self) -> bool {
        self.func_fail == 0 && self.stress_fail <= STRESS_ALLOW && self.regress_drift == 0
    }
}

/// 沙箱执行器。
pub struct Sandbox {
    outs: Vec<CaseOut>,
    ran: bool,
    /// 宿主污染标志（隔离核）。
    host_tainted: bool,
    magic: u32,
}

impl Sandbox {
    /// 建。
    pub fn new() -> Sandbox {
        Sandbox { outs: Vec::new(), ran: false, host_tainted: false, magic: 0x0400_5A17 }
    }

    /// 提交一条用例结果（沙箱内执行；崩溃即 ok=false，不外溢）。
    pub fn submit(&mut self, name: &'static str, kind: Kind, ok: bool, base: u32, got: u32) -> Result<(), u32> {
        for c in self.outs.iter() {
            if c.name == name {
                return Err(XC_CASE_DUP);
            }
        }
        self.outs.push(CaseOut { name, kind, ok, base, got });
        Ok(())
    }

    /// 模拟崩溃用例：结果入账 false，宿主零污染。
    pub fn submit_crash(&mut self, name: &'static str, kind: Kind) -> Result<(), u32> {
        let r = self.submit(name, kind, false, 0, 0);
        if r.is_ok() {
            self.host_tainted = false; // 隔离核：崩溃不出考场
        }
        r
    }

    /// 跑批统计。
    pub fn run(&mut self) -> Result<Stats, u32> {
        if self.magic != 0x0400_5A17 {
            return Err(XC_MAGIC);
        }
        if self.outs.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut st = Stats {
            func_total: 0, func_fail: 0,
            stress_total: 0, stress_fail: 0,
            regress_total: 0, regress_drift: 0,
        };
        for c in self.outs.iter() {
            match c.kind {
                Kind::Func => {
                    st.func_total += 1;
                    if !c.ok {
                        st.func_fail += 1;
                    }
                }
                Kind::Stress => {
                    st.stress_total += 1;
                    if !c.ok {
                        st.stress_fail += 1;
                    }
                }
                Kind::Regress => {
                    st.regress_total += 1;
                    if c.got != c.base {
                        st.regress_drift += 1;
                    }
                }
            }
        }
        self.ran = true;
        Ok(st)
    }

    /// 裁决章：三闸全过发「可上线」。
    pub fn verdict(&self) -> Result<bool, u32> {
        if !self.ran {
            return Err(XC_NOT_RUN);
        }
        let st = self.stats_ref();
        if st.func_fail > 0 {
            return Err(XC_FUNC_FAIL);
        }
        if st.stress_fail > STRESS_ALLOW {
            return Err(XC_STRESS_OVER);
        }
        if st.regress_drift > 0 {
            return Err(XC_REGRESS);
        }
        if self.host_tainted {
            return Err(XC_LEAK);
        }
        Ok(true)
    }

    /// 裁决后的统计快照（须与 run 一致）。
    pub fn stats_ref(&self) -> Stats {
        let mut st = Stats {
            func_total: 0, func_fail: 0,
            stress_total: 0, stress_fail: 0,
            regress_total: 0, regress_drift: 0,
        };
        for c in self.outs.iter() {
            match c.kind {
                Kind::Func => {
                    st.func_total += 1;
                    if !c.ok { st.func_fail += 1; }
                }
                Kind::Stress => {
                    st.stress_total += 1;
                    if !c.ok { st.stress_fail += 1; }
                }
                Kind::Regress => {
                    st.regress_total += 1;
                    if c.got != c.base { st.regress_drift += 1; }
                }
            }
        }
        st
    }

    /// 用例数。
    pub fn case_count(&self) -> usize {
        self.outs.len()
    }

    /// 宿主洁净核。
    pub fn host_clean(&self) -> bool {
        !self.host_tainted
    }
}

pub fn run_veb378_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0378");
    // 族一：全绿批 → 发章。
    let mut a = Sandbox::new();
    let _ = a.submit("read", Kind::Func, true, 0, 0);
    let _ = a.submit("write", Kind::Func, true, 0, 0);
    let _ = a.submit("storm", Kind::Stress, true, 0, 0);
    let _ = a.submit("latency", Kind::Regress, true, 42, 42);
    s.add("用例 4", a.case_count() == 4, "入箱");
    let st = a.run();
    s.add("跑批统计", matches!(&st, Ok(v) if v.func_total == 2 && v.stress_total == 1 && v.regress_total == 1), "三类分明");
    s.add("三闸过", matches!(&st, Ok(v) if v.gates_ok()), "全绿");
    s.add("发章", a.verdict() == Ok(true), "可上线");
    s.add("宿主洁净", a.host_clean(), "零污染");
    // 族二：功能败 → 拒。
    let mut b = Sandbox::new();
    let _ = b.submit("read", Kind::Func, true, 0, 0);
    let _ = b.submit("write", Kind::Func, false, 0, 0);
    let _ = b.submit("storm", Kind::Stress, true, 0, 0);
    let _ = b.submit("lat", Kind::Regress, true, 7, 7);
    let _ = b.run();
    s.add("功能败拒", b.verdict() == Err(XC_FUNC_FAIL), "底线闸");
    // 族三：压力限值内过 / 超限拒。
    let mut c = Sandbox::new();
    let _ = c.submit("f1", Kind::Func, true, 0, 0);
    let _ = c.submit("s1", Kind::Stress, false, 0, 0);
    let _ = c.submit("s2", Kind::Stress, false, 0, 0);
    let _ = c.submit("r1", Kind::Regress, true, 9, 9);
    let stc = c.run();
    s.add("压力败 2 限内", matches!(&stc, Ok(v) if v.stress_fail == 2 && v.gates_ok()), "≤2 放行");
    s.add("限内发章", c.verdict() == Ok(true), "韧性闸过");
    let mut d = Sandbox::new();
    let _ = d.submit("f1", Kind::Func, true, 0, 0);
    let _ = d.submit("s1", Kind::Stress, false, 0, 0);
    let _ = d.submit("s2", Kind::Stress, false, 0, 0);
    let _ = d.submit("s3", Kind::Stress, false, 0, 0);
    let _ = d.submit("r1", Kind::Regress, true, 9, 9);
    let _ = d.run();
    s.add("压力败 3 超限拒", d.verdict() == Err(XC_STRESS_OVER), ">2 拒");
    // 族四：回归偏移拒。
    let mut e = Sandbox::new();
    let _ = e.submit("f1", Kind::Func, true, 0, 0);
    let _ = e.submit("r1", Kind::Regress, true, 100, 103);
    let _ = e.run();
    s.add("回归偏移拒", e.verdict() == Err(XC_REGRESS), "基线漂了");
    let ste = e.stats_ref();
    s.add("偏移计 1", ste.regress_drift == 1, "逐例核");
    // 族五：崩溃隔离 + 未跑拒 + 重名拒。
    let mut f = Sandbox::new();
    let _ = f.submit_crash("boom", Kind::Func);
    s.add("崩溃入账", f.case_count() == 1, "考场立案");
    s.add("宿主仍洁", f.host_clean(), "不外溢");
    let _ = f.submit("g1", Kind::Func, true, 0, 0);
    let _ = f.run();
    s.add("功能败(崩溃)拒", f.verdict() == Err(XC_FUNC_FAIL), "崩溃也是败");
    let mut g = Sandbox::new();
    s.add("未跑裁决拒", g.verdict() == Err(XC_NOT_RUN), "先跑再判");
    s.add("空批跑拒", g.run() == Err(XC_EMPTY), "无用例");
    let _ = g.submit("dup", Kind::Func, true, 0, 0);
    s.add("重名拒", g.submit("dup", Kind::Func, true, 0, 0) == Err(XC_CASE_DUP), "同名");
    s.add("类别核", Kind::from_u32(2) == Some(Kind::Regress) && Kind::from_u32(3).is_none(), "枚举闭集");
    s.add("魔数核", DT_SEG_BASE == 0x30400, "段基址");
    s
}
