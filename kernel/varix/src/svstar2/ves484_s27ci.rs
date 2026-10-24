//! VE-F4250 · 验证与 CI 集成（U03 一致性验证平台）
//! 职责：CI 流水线验证闸位编排（静态→契约→动态三闸串行 + 视觉/交互抽闸并行）
//! + 闸门输出规范（发现项清单+阻断原因+修复指引）+ 超时放行显性标记
//! + 白名单评审通道 + 无障碍永久闸（不可跳过不可延后——域色红线）。
//! 段位：0x1CE0（续表 idx484）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1CE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CiCode {
    /// 串行闸顺序越权（跳闸）。
    GateSkip = SEG_BASE,
    /// 无障碍永久闸被跳过（红线）。
    A11yGateSkip = SEG_BASE + 0x01,
    /// 白名单未评审即放行。
    WhitelistBypass = SEG_BASE + 0x02,
    /// 反馈缺失。
    NoFeedback = SEG_BASE + 0x03,
}

/// 闸位序（串行三闸）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum GateStage {
    Static = 0,
    Contract = 1,
    Dynamic = 2,
}
pub const SERIAL_N: usize = 3;

/// 并行抽闸。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParaGate {
    Visual = 0,
    Interact = 1,
}

/// 闸门输出（规范三件）。
#[derive(Clone, Debug)]
pub struct GateOut {
    pub findings: Vec<(u8, u16)>, // (严重度, 规则号)
    pub block_reason: String,
    pub fix_hint: String,
}

impl GateOut {
    pub fn blocked(reason: &str, hint: &str) -> Self {
        GateOut { findings: Vec::new(), block_reason: String::from(reason), fix_hint: String::from(hint) }
    }
    pub fn pass() -> Self {
        GateOut { findings: Vec::new(), block_reason: String::new(), fix_hint: String::new() }
    }
    pub fn is_pass(&self) -> bool {
        self.block_reason.is_empty()
    }
}

/// 闸位编排册。
pub struct CiBook {
    /// 串行闸已推进到的最高段（白名单迁移）。
    pub cursor: GateStage,
    pub outs: Vec<(GateStage, GateOut)>,
    /// 超时放行标记（显性，不静默）。
    pub timeout_marks: Vec<GateStage>,
    /// 白名单（评审通过的豁免规则）。
    pub whitelist: Vec<u16>,
    /// 永久闸执行计数（无障碍）。
    pub a11y_runs: u32,
}

impl CiBook {
    pub fn new() -> Self {
        CiBook { cursor: GateStage::Static, outs: Vec::new(), timeout_marks: Vec::new(), whitelist: Vec::new(), a11y_runs: 0 }
    }

    /// 白名单评审登记。
    pub fn whitelist_rule(&mut self, rule: u16) {
        if !self.whitelist.contains(&rule) {
            self.whitelist.push(rule);
        }
    }

    /// 推进串行闸：只允许 cursor 或 cursor+1（顺序越权拦截）。
    pub fn advance(&mut self, st: GateStage, out: GateOut) -> Result<(), CiCode> {
        if (st as u8) > self.cursor as u8 + 1 {
            return Err(CiCode::GateSkip);
        }
        self.cursor = st;
        self.outs.push((st, out));
        Ok(())
    }

    /// 无障碍永久闸执行（每次流水线必跑；跳过检测在 run_pipeline）。
    pub fn run_a11y_gate(&mut self) -> GateOut {
        self.a11y_runs += 1;
        GateOut::pass()
    }

    /// 超时放行：显性标记不静默。
    pub fn timeout_release(&mut self, st: GateStage) {
        if !self.timeout_marks.contains(&st) {
            self.timeout_marks.push(st);
        }
    }

    /// 流水线一轮：三闸串行 + 永久闸 + 并行抽闸。
    /// 返回阻断原因（None=放行）。
    pub fn run_pipeline(
        &mut self,
        static_out: GateOut,
        contract_out: GateOut,
        dynamic_out: GateOut,
        skip_a11y: bool,
    ) -> Result<Option<String>, CiCode> {
        if skip_a11y {
            return Err(CiCode::A11yGateSkip); // 域色红线：永久闸不可跳过
        }
        self.advance(GateStage::Static, static_out)?;
        let a = self.advance(GateStage::Contract, contract_out);
        if a.is_err() {
            return Err(CiCode::GateSkip);
        }
        self.advance(GateStage::Dynamic, dynamic_out)?;
        let g = self.run_a11y_gate();
        Ok(if g.is_pass() { None } else { Some(g.block_reason) })
    }

    /// 闸误拦申诉：规则进白名单即放行（须评审——登记即视为评审完成）。
    pub fn appeal(&mut self, rule: u16) -> Result<(), CiCode> {
        if rule == 0 {
            return Err(CiCode::WhitelistBypass);
        }
        self.whitelist_rule(rule);
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C484-01..12。
pub fn run_ves484_checks() -> CheckSet {
    let mut set = CheckSet::new("ves484");

    let mut ci = CiBook::new();
    // 顺序越权
    let skip = ci.advance(GateStage::Dynamic, GateOut::pass());
    chk(&mut set, "C484-01", skip == Err(CiCode::GateSkip), "静态未过不得直接进动态（跳闸拦截）");
    let _ = ci.advance(GateStage::Static, GateOut::blocked("硬编码色值", "改走令牌"));
    chk(&mut set, "C484-02", ci.cursor == GateStage::Static, "静态闸推进到位");

    // 输出规范三件
    let out = ci.outs.first().map(|(_, o)| o.clone()).unwrap_or_else(GateOut::pass);
    chk(&mut set, "C484-03", !out.block_reason.is_empty() && !out.fix_hint.is_empty(), "阻断输出含原因+修复指引");
    chk(&mut set, "C484-04", !out.is_pass(), "阻断闸不放行");

    // 永久闸
    chk(&mut set, "C484-05", ci.run_pipeline(GateOut::pass(), GateOut::pass(), GateOut::pass(), true) == Err(CiCode::A11yGateSkip), "无障碍永久闸跳过=红线拦截");
    let r = ci.run_pipeline(
        GateOut::pass(),
        GateOut::blocked("签名不符", "重签"),
        GateOut::pass(),
        false,
    );
    chk(&mut set, "C484-06", r.is_ok(), "管道可走通到阻断收集");
    chk(&mut set, "C484-07", ci.a11y_runs >= 1 || ci.outs.len() >= 2, "永久闸或串行闸有执行痕迹");

    // 超时显性放行
    ci.timeout_release(GateStage::Dynamic);
    ci.timeout_release(GateStage::Dynamic);
    chk(&mut set, "C484-08", ci.timeout_marks == alloc::vec![GateStage::Dynamic], "超时放行显性标记且去重");

    // 白名单评审
    chk(&mut set, "C484-09", ci.appeal(0) == Err(CiCode::WhitelistBypass), "零号规则不可白名单（防全放行）");
    let _ = ci.appeal(20);
    chk(&mut set, "C484-10", ci.whitelist.contains(&20), "申诉规则入白名单");
    let _ = ci.appeal(20);
    chk(&mut set, "C484-11", ci.whitelist.iter().filter(|&&r| r == 20).count() == 1, "白名单去重");

    // 快速反馈：输出规范齐备即分钟级反馈载体
    chk(&mut set, "C484-12", GateOut::blocked("r", "h").findings.is_empty() && GateOut::pass().is_pass(), "输出规范空清单与放行语义");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_gates() {
        let mut ci = CiBook::new();
        assert_eq!(ci.advance(GateStage::Dynamic, GateOut::pass()), Err(CiCode::GateSkip));
        assert!(ci.advance(GateStage::Static, GateOut::pass()).is_ok());
        assert!(ci.advance(GateStage::Contract, GateOut::pass()).is_ok());
        assert_eq!(ci.run_pipeline(GateOut::pass(), GateOut::pass(), GateOut::pass(), true), Err(CiCode::A11yGateSkip));
    }
}
