//! veb336_riskreg —— VE-F0336 风险登记。
//! 锚点：VE-F0336 · 目标 300 行。
//! 判据原文：四类五要素（硬件供给 / 上游变动 / 人力时间 /
//!           信息不足 × 现象·影响·概率·缓解·触发信号）、
//!           触发升级（触发信号出现即升级处置）、显性关闭
//!           （风险解除不静默）、周期复核、判据。
//! 组件：风险账 + 触发升级器 + 复核轮。
//! 职责定位：风险登记是「值班记录本」——每条风险五要素写全，
//!           触发信号一响就得升级处理；解除风险要打烊声明，
//!           不许悄悄抹掉。
//! 性能逐项分解：登记 O(1)/条；复核 O(账目) 周期；升级 O(1)。
//! 诊断码段：0x31D00（0x31D01~0x31D09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const RR_VERSION: &str = "veb336_riskreg-v1";

/// 诊断段基址（0x31D00 段 · 本模块独占）。
pub const RR_SEG_BASE: u32 = 0x31D00;
pub const XC_BADCAT: u32 = 0x31D01; // 类别非法
pub const XC_BADPROB: u32 = 0x31D02; // 概率不合法
pub const XC_NOSIG: u32 = 0x31D03; // 缺触发信号
pub const XC_NORISK: u32 = 0x31D04; // 风险不存在
pub const XC_CLOSED: u32 = 0x31D05; // 已关闭再操作
pub const XC_OPENP: u32 = 0x31D06; // 缓解失效重开

/// 四类风险。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    /// 硬件供给（开发板可得性）。
    Supply,
    /// 上游变动（驱动接口破坏性变更）。
    Upstream,
    /// 人力与时间。
    Manpower,
    /// 信息不足。
    Info,
}

/// 概率三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prob {
    Low,
    Mid,
    High,
}

/// 概率代号 0~2。
pub fn prob_no(p: Prob) -> u32 {
    match p {
        Prob::Low => 0,
        Prob::Mid => 1,
        Prob::High => 2,
    }
}

/// 风险状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 在册。
    Open,
    /// 已升级（触发信号出现）。
    Escalated,
    /// 显性关闭。
    Closed,
}

/// 风险条目（五要素）。
pub struct Risk {
    pub id: u32,
    pub cat: Cat,
    /// 现象代号。
    pub phen: u32,
    /// 影响等级 0~9。
    pub impact: u32,
    pub prob: Prob,
    /// 缓解措施代号（0 = 无缓解 → 登记拒）。
    pub ease: u32,
    /// 触发信号阈值（登记后周期触发复核用）。
    pub sig: u32,
    pub state: State,
    /// 升级次数。
    pub escs: u32,
}

/// 风险登记簿。
pub struct RiskReg {
    risks: Vec<Risk>,
    next: u32,
    /// 复核轮次。
    pub rounds: u32,
}

impl RiskReg {
    /// 空簿。
    pub fn new() -> RiskReg {
        RiskReg {
            risks: Vec::new(),
            next: 1,
            rounds: 0,
        }
    }

    /// 类别核（防御式：枚举恒合法，供外部文本解析后的收口）。
    pub fn cat_ok(_c: Cat) -> bool {
        true
    }

    /// 登记：影响 0~9、概率任意、缓解代号非零、触发信号非零。
    pub fn register(
        &mut self,
        cat: Cat,
        phen: u32,
        impact: u32,
        prob: Prob,
        ease: u32,
        sig: u32,
    ) -> Result<u32, u32> {
        let _ = Self::cat_ok(cat);
        if impact > 9 {
            return Err(XC_BADPROB);
        }
        if ease == 0 {
            return Err(XC_NORISK);
        }
        if sig == 0 {
            return Err(XC_NOSIG);
        }
        let id = self.next;
        self.next += 1;
        self.risks.push(Risk {
            id,
            cat,
            phen,
            impact,
            prob,
            ease,
            sig,
            state: State::Open,
            escs: 0,
        });
        Ok(id)
    }

    /// 触发升级：当前风险值 ≥ 触发信号即升级。风险值 = 影响 × (概率+1)。
    pub fn value(&self, id: u32) -> Option<u32> {
        let r = self.risks.iter().find(|r| r.id == id)?;
        Some(r.impact * (prob_no(r.prob) + 1))
    }

    pub fn trigger(&mut self, id: u32, observed: u32) -> Result<bool, u32> {
        let r = self.risks.iter_mut().find(|r| r.id == id).ok_or(XC_NORISK)?;
        if r.state == State::Closed {
            return Err(XC_CLOSED);
        }
        if observed >= r.sig {
            r.state = State::Escalated;
            r.escs += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 显性关闭：须先处置（升级过或缓解在案）才许关闭。
    pub fn close(&mut self, id: u32) -> Result<(), u32> {
        let r = self.risks.iter_mut().find(|r| r.id == id).ok_or(XC_NORISK)?;
        if r.state == State::Closed {
            return Err(XC_CLOSED);
        }
        r.state = State::Closed;
        Ok(())
    }

    /// 缓解失效重开：已关闭风险重新评估入册（新条目引用旧现象）。
    pub fn reopen(&mut self, id: u32) -> Result<u32, u32> {
        let old = self.risks.iter().find(|r| r.id == id).ok_or(XC_NORISK)?;
        if old.state != State::Closed {
            return Err(XC_OPENP);
        }
        let nid = self.next;
        self.next += 1;
        self.risks.push(Risk {
            id: nid,
            cat: old.cat,
            phen: old.phen,
            impact: old.impact,
            prob: old.prob,
            ease: old.ease,
            sig: old.sig,
            state: State::Open,
            escs: 0,
        });
        Ok(nid)
    }

    /// 周期复核：清点在册（Open+Escalated）与升级数。
    pub fn review(&mut self) -> (usize, usize) {
        self.rounds += 1;
        let open = self
            .risks
            .iter()
            .filter(|r| r.state != State::Closed)
            .count();
        let esc = self.risks.iter().filter(|r| r.state == State::Escalated).count();
        (open, esc)
    }

    /// 状态查询。
    pub fn state_of(&self, id: u32) -> Option<State> {
        self.risks.iter().find(|r| r.id == id).map(|r| r.state)
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.risks.len()
    }
}

pub fn run_veb336_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0336");
    // 族零：登记门。
    let mut g = RiskReg::new();
    s.add("影响越界拒", g.register(Cat::Supply, 1, 10, Prob::Low, 3, 5) == Err(XC_BADPROB), ">9");
    s.add("无缓解拒", g.register(Cat::Supply, 1, 5, Prob::Low, 0, 5) == Err(XC_NORISK), "裸奔");
    s.add("无信号拒", g.register(Cat::Supply, 1, 5, Prob::Low, 3, 0) == Err(XC_NOSIG), "不设触发");
    s.add("登记过", g.register(Cat::Supply, 11, 5, Prob::Mid, 3, 12).is_ok(), "一号");
    s.add("号自增", g.register(Cat::Upstream, 21, 6, Prob::High, 4, 20) == Ok(2), "二号");
    // 族一：风险值与触发。
    s.add("值 15", g.value(1) == Some(15), "5×3");
    s.add("值 24", g.value(2) == Some(24), "8×3");
    s.add("未触发", g.trigger(1, 11) == Ok(false), "11<12");
    s.add("触发升", g.trigger(1, 12) == Ok(true), "12≥12");
    s.add("态在升", g.state_of(1) == Some(State::Escalated), "升级");
    s.add("查无触发拒", g.trigger(99, 1) == Err(XC_NORISK), "无此号");
    // 族二：显性关闭。
    s.add("关闭过", g.close(2).is_ok(), "打烊声明");
    s.add("态在闭", g.state_of(2) == Some(State::Closed), "不静默");
    s.add("闭后再触发拒", g.trigger(2, 99) == Err(XC_CLOSED), "关了不接单");
    s.add("闭后再关拒", g.close(2) == Err(XC_CLOSED), "无二闭");
    // 族三：缓解失效重开。
    s.add("重开过", g.reopen(2) == Ok(3), "新三号");
    s.add("重开态开", g.state_of(3) == Some(State::Open), "重新在册");
    s.add("未闭重开拒", g.reopen(1) == Err(XC_OPENP), "在册不重开");
    // 族四：周期复核。
    let (open, esc) = g.review();
    s.add("在册二", open == 2, "1,3");
    s.add("升级一", esc == 1, "一号升");
    s.add("轮计一", g.rounds == 1, "一轮");
    let _ = g.review();
    s.add("轮计二", g.rounds == 2, "两轮");
    // 族五：四类齐全登记。
    let _ = g.register(Cat::Manpower, 31, 4, Prob::Low, 2, 9);
    let _ = g.register(Cat::Info, 41, 3, Prob::Mid, 2, 8);
    s.add("账四", g.len() == 4, "四类齐");
    s.add("魔数核", RR_SEG_BASE == 0x31D00, "段基址");
    s
}
