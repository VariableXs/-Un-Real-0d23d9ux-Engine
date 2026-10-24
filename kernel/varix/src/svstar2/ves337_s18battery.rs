//! ves337_s18battery — VE-F4097 · T05 语言资源与 J04 续航联动
//! 资产后台下载/热更新重排的功耗面：后台下载限速档 + 重排批处理 + 热更新避开高负载窗口三策略 + 功耗预算登记 + 用户前台操作冲突让路延迟 + 读屏会话优先豁免（域本色：无障碍让路不延迟读屏会话）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9C40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtCode {
    Ok = 0,
    BudgetOver = SEG_BASE + 0x01,
    Yielded = SEG_BASE + 0x02,
    StrategyDead = SEG_BASE + 0x03,
}

/// 后台下载限速档（KB/s）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedLimit { Eco = 0, Balanced = 1, Unthrottled = 2 }

impl SpeedLimit {
    pub const ALL: [SpeedLimit; 3] = [
        SpeedLimit::Eco, SpeedLimit::Balanced, SpeedLimit::Unthrottled];
    pub fn kb_per_s(self) -> u32 {
        match self { SpeedLimit::Eco => 64, SpeedLimit::Balanced => 512, SpeedLimit::Unthrottled => 4096 }
    }
}

/// 功耗预算表：每策略档位 → 预算 mAh。
pub struct PowerBudget { pub download_mah: u32, pub relayout_mah: u32, pub hotswap_mah: u32 }

impl PowerBudget {
    pub fn new() -> Self { PowerBudget { download_mah: 30, relayout_mah: 12, hotswap_mah: 8 } }
    pub fn total(&self) -> u32 { self.download_mah + self.relayout_mah + self.hotswap_mah }
}

/// 联动策略册：三策略登记 + 失效登记升级。
pub struct StrategyBook { pub active: Vec<&'static str>, pub dead: Vec<&'static str> }

pub const THREE_STRATEGIES: [&'static str; 3] = ["dl-speed-limit", "relayout-batch", "hotswap-lowload-window"];

impl StrategyBook {
    pub fn new() -> Self { StrategyBook { active: Vec::new(), dead: Vec::new() } }

    pub fn enable(&mut self, s: &'static str) {
        if !self.active.contains(&s) { self.active.push(s); }
    }

    /// 三策略齐备。
    pub fn all_active(&self) -> bool {
        THREE_STRATEGIES.iter().all(|s| self.active.contains(s))
    }

    /// 策略失效 → 挪入死册 + 登记升级。
    pub fn fail(&mut self, s: &'static str) -> Result<(), BtCode> {
        if !self.active.contains(&s) { return Err(BtCode::StrategyDead); }
        self.active.retain(|x| *x != s);
        self.dead.push(s);
        Ok(())
    }

    pub fn upgraded_needed(&self) -> bool { !self.dead.is_empty() }
}

/// 批处理：重排任务合批执行（批大小与间隔）。
pub struct BatchRunner { pub batch_size: usize, pub gap_ms: u32, pub pending: Vec<&'static str> }

impl BatchRunner {
    pub fn new() -> Self { BatchRunner { batch_size: 8, gap_ms: 100, pending: Vec::new() } }

    pub fn push(&mut self, vp: &'static str) { self.pending.push(vp); }

    /// 执行：按批切分 → 返回批数（功耗面：批间隔省电）。
    pub fn run_batches(&mut self) -> usize {
        let n = self.pending.len();
        self.pending.clear();
        (n + self.batch_size - 1) / self.batch_size
    }
}

/// 高负载窗口：热更新避开（窗口内禁止热换）。
pub struct LoadWindow { pub busy: bool }

impl LoadWindow {
    pub fn hotswap_allowed(&self) -> Result<(), BtCode> {
        if self.busy { Err(BtCode::Yielded) } else { Ok(()) }
    }
}

/// 前台冲突让路：用户前台操作时后台任务延迟；读屏会话豁免（域本色）。
pub struct YieldGate { pub foreground_busy: bool, pub screen_reader_session: bool }

impl YieldGate {
    /// 后台任务准入：前台忙 → 让路延迟（除非读屏会话豁免）。
    pub fn admit(&self) -> Result<(), BtCode> {
        if self.foreground_busy {
            if self.screen_reader_session { return Ok(()); } // 无障碍优先豁免
            return Err(BtCode::Yielded);
        }
        Ok(())
    }
}

/// 预算超支处置：策略加严（降档）。
pub fn tighten_on_over(budget: &PowerBudget, actual: &PowerBudget, cur: SpeedLimit) -> SpeedLimit {
    if actual.total() <= budget.total() { return cur; }
    match cur {
        SpeedLimit::Unthrottled => SpeedLimit::Balanced,
        _ => SpeedLimit::Eco,
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves337_checks() -> CheckSet {
    let mut set = CheckSet::new("ves337");
    let mut sb = StrategyBook::new();

    // C337-01 三策略登记齐备。
    for s in THREE_STRATEGIES.iter() { sb.enable(s); }
    chk(&mut set, "C337-01", sb.all_active() && sb.active.len() == 3, "三策略齐");

    // C337-02 限速档位单源。
    chk(&mut set, "C337-02",
        SpeedLimit::Eco.kb_per_s() == 64 && SpeedLimit::Balanced.kb_per_s() == 512
        && SpeedLimit::Unthrottled.kb_per_s() == 4096, "三档限速 64/512/4096");

    // C337-03 后台下载预算内不限档。
    let pb = PowerBudget::new();
    let actual_ok = PowerBudget { download_mah: 20, relayout_mah: 10, hotswap_mah: 5 };
    chk(&mut set, "C337-03", tighten_on_over(&pb, &actual_ok, SpeedLimit::Balanced) == SpeedLimit::Balanced, "预算内保档");

    // C337-04 预算超 → 策略加严降档。
    let actual_over = PowerBudget { download_mah: 60, relayout_mah: 10, hotswap_mah: 5 };
    chk(&mut set, "C337-04",
        tighten_on_over(&pb, &actual_over, SpeedLimit::Unthrottled) == SpeedLimit::Balanced
        && tighten_on_over(&pb, &actual_over, SpeedLimit::Balanced) == SpeedLimit::Eco, "超预算逐级降档");

    // C337-05 重排批处理：13 视口 8/批 → 2 批。
    let mut br = BatchRunner::new();
    for _ in 0..13 { br.push("vp"); }
    chk(&mut set, "C337-05", br.run_batches() == 2 && br.pending.is_empty(), "13 视口 → 2 批");

    // C337-06 高负载窗口热更新让路。
    let w = LoadWindow { busy: true };
    chk(&mut set, "C337-06", w.hotswap_allowed() == Err(BtCode::Yielded), "高负载窗口 => 让路");

    // C337-07 空闲窗口放行。
    let w2 = LoadWindow { busy: false };
    chk(&mut set, "C337-07", w2.hotswap_allowed().is_ok(), "空闲窗口放行");

    // C337-08 前台冲突：读屏会话豁免（域本色）。
    let g = YieldGate { foreground_busy: true, screen_reader_session: true };
    chk(&mut set, "C337-08", g.admit().is_ok(), "读屏会话不延迟");

    // C337-09 前台冲突：普通后台任务延迟。
    let g2 = YieldGate { foreground_busy: true, screen_reader_session: false };
    chk(&mut set, "C337-09", g2.admit() == Err(BtCode::Yielded), "普通后台任务让路");

    // C337-10 前台空闲全放行。
    let g3 = YieldGate { foreground_busy: false, screen_reader_session: false };
    chk(&mut set, "C337-10", g3.admit().is_ok(), "前台空闲放行");

    // C337-11 策略失效 → 死册 + 升级登记。
    let mut sb2 = StrategyBook::new();
    sb2.enable("relayout-batch");
    let _ = sb2.fail("relayout-batch");
    chk(&mut set, "C337-11", sb2.dead.contains(&"relayout-batch") && sb2.upgraded_needed() && !sb2.all_active(), "失效挪死册待升级");

    // C337-12 功耗预算表三分项单源。
    chk(&mut set, "C337-12", pb.total() == 50, "预算三分项合计 50mAh");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves337_smoke() {
        let set = run_ves337_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves337 red: {:?}", red);
    }

    #[test]
    fn ves337_tighten() {
        let pb = PowerBudget::new();
        let over = PowerBudget { download_mah: 99, relayout_mah: 0, hotswap_mah: 0 };
        assert_eq!(tighten_on_over(&pb, &over, SpeedLimit::Eco), SpeedLimit::Eco);
    }
}
