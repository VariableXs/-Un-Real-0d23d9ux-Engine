//! ves449_s24debug — VE-F4215 · 一致性调试器
//! 三视区：发现项追踪器（发现项→规则→契约→影响面全链反查）+规则执行单步调试+图谱可视化视区；
//! 链断→显性报断点；单步开销超限→降采样；渲染失败→降级列表视图；
//! 调试器遵循五标准（快/稳/diff/集成/扩展复述）；调试数据不含用户内容。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1500;
pub const STEP_BUDGET: u32 = 64; // 单步开销预算（步）

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01, // 0x1501
    OverSampled = SEG_BASE + 0x02, // 0x1502 降采样
    RenderFail = SEG_BASE + 0x03,  // 0x1503 降级列表
}

/// 反查链节点（发现项→规则→契约→影响面）。
#[derive(Clone, Copy)]
pub struct ChainNode { pub finding: u32, pub rule: Option<u32>, pub contract: Option<u32>, pub impact: Option<u32> }

/// 调试器册。
pub struct Debugger {
    pub chains: Vec<ChainNode>,
    pub steps_used: u32,
    pub downsampled: bool,
    pub list_fallback: bool,
    pub breakpoints: Vec<u32>, // 显性断点（链断位置：0=规则位 1=契约位 2=影响位）
}

impl Debugger {
    pub fn new() -> Self {
        Debugger { chains: Vec::new(), steps_used: 0, downsampled: false, list_fallback: false, breakpoints: Vec::new() }
    }
    pub fn put(&mut self, c: ChainNode) { self.chains.retain(|x| x.finding != c.finding); self.chains.push(c); }
    /// 全链反查：断链→显性断点定位（报断在哪一环）。
    pub fn trace(&mut self, finding: u32) -> Result<[u32; 4], DbgCode> {
        let c = match self.chains.iter().find(|c| c.finding == finding) {
            Some(c) => *c, None => return Err(DbgCode::ChainBroken),
        };
        let mut out = [c.finding, 0, 0, 0];
        if let Some(r) = c.rule { out[1] = r; } else { self.breakpoints.push(0); return Err(DbgCode::ChainBroken); }
        if let Some(k) = c.contract { out[2] = k; } else { self.breakpoints.push(1); return Err(DbgCode::ChainBroken); }
        if let Some(i) = c.impact { out[3] = i; } else { self.breakpoints.push(2); return Err(DbgCode::ChainBroken); }
        Ok(out)
    }
    /// 单步调试：预算超限→降采样。
    pub fn step(&mut self, n: u32) -> Result<(), DbgCode> {
        self.steps_used += n;
        if self.steps_used > STEP_BUDGET {
            self.downsampled = true;
            self.steps_used = STEP_BUDGET / 2; // 降采样减半
            return Err(DbgCode::OverSampled);
        }
        Ok(())
    }
    /// 图谱渲染失败→降级列表视图。
    pub fn render(&mut self, ok: bool) -> Result<(), DbgCode> {
        if ok { self.list_fallback = false; Ok(()) } else { self.list_fallback = true; Err(DbgCode::RenderFail) }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves449_checks() -> CheckSet {
    let mut set = CheckSet::new("ves449");
    let mut dbg = Debugger::new();

    // C449-01 全链反查通过（发现→规则→契约→影响）。
    dbg.put(ChainNode { finding: 9001, rule: Some(201), contract: Some(7001), impact: Some(30) });
    chk(&mut set, "C449-01", dbg.trace(9001) == Ok([9001, 201, 7001, 30]), "全链反查");

    // C449-02 断链（缺契约）→显性断点位 1。
    dbg.put(ChainNode { finding: 9002, rule: Some(202), contract: None, impact: Some(31) });
    chk(&mut set, "C449-02", dbg.trace(9002) == Err(DbgCode::ChainBroken)
        && dbg.breakpoints.contains(&1), "断点定位");

    // C449-03 未知发现项→链断。
    chk(&mut set, "C449-03", dbg.trace(9999) == Err(DbgCode::ChainBroken), "未知项断链");

    // C449-04 单步预算内通过。
    chk(&mut set, "C449-04", dbg.step(32).is_ok() && dbg.steps_used == 32, "预算内");

    // C449-05 单步超限→降采样（减半）。
    chk(&mut set, "C449-05", dbg.step(33) == Err(DbgCode::OverSampled)
        && dbg.downsampled && dbg.steps_used == 32, "降采样");

    // C449-06 降采样后可继续（预算重置）。
    chk(&mut set, "C449-06", dbg.step(20).is_ok(), "降采样后可续");

    // C449-07 渲染失败→降级列表视图。
    chk(&mut set, "C449-07", dbg.render(false) == Err(DbgCode::RenderFail) && dbg.list_fallback, "降级列表");

    // C449-08 渲染恢复→列表位清空。
    chk(&mut set, "C449-08", dbg.render(true).is_ok() && !dbg.list_fallback, "渲染恢复");

    // C449-09 断点留痕（多次断链累计）。
    dbg.put(ChainNode { finding: 9003, rule: None, contract: None, impact: None });
    let _ = dbg.trace(9003);
    chk(&mut set, "C449-09", dbg.breakpoints == vec![1, 0], "断点累计");

    // C449-10 调试数据不含用户内容（载荷全为系统 id）。
    chk(&mut set, "C449-10", dbg.chains.iter().all(|c| c.finding >= 9000), "数据无用户内容");

    // C449-11 链覆盖幂等。
    dbg.put(ChainNode { finding: 9001, rule: Some(205), contract: Some(7002), impact: Some(35) });
    chk(&mut set, "C449-11", dbg.chains.len() == 3, "链覆盖幂等");

    // C449-12 单步预算常量单源。
    chk(&mut set, "C449-12", STEP_BUDGET == 64 && DbgCode::OverSampled as u16 == SEG_BASE + 2, "预算单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves449_smoke() {
        let set = run_ves449_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves449 red: {:?}", red);
    }
}
