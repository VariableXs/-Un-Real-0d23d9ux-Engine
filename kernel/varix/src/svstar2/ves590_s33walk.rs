//! VE-F4356 · 一致性走查汇总（U08 测试与资产组）
//! 职责：一致性真机走查汇总——走查设计（按二十维验收清单抽取一致性相关
//! 维——交互正确性 / 一致性 / 无障碍三维主查）+ 走查执行（真机多环境 +
//! 乱点路径）+ 发现处置（阻断级即修 / 细节入账）+ 环境缺位补齐后走查 +
//! 发现争议复核分级 + 走查与自动化冲突人工优先 + 走查主维即无障碍（域色
//! 核心）+ 记录脱敏。
//! 段位：0x44E0（续表 idx590）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x44E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WkCode {
    /// 环境缺位（补齐后走查）。
    EnvMissing = SEG_BASE,
    /// 发现争议（复核分级）。
    FindingDispute = SEG_BASE + 0x01,
    /// 阻断级未即修（放行拦截）。
    BlockerOpen = SEG_BASE + 0x02,
}

/// 三维主查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    Interaction,
    Consistency,
    /// 无障碍——主维之核心（域色）。
    A11y,
}

/// 走查环境。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    pub id: u32,
    /// 环境就位（设备 + 版本 + 读屏器——缺一不就位）。
    pub ready: bool,
}

/// 一条发现。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finding {
    pub dim: Dim,
    pub env: u32,
    /// 级别：0 阻断级 1 细节级。
    pub level: u8,
    /// 阻断级即修完成。
    pub fixed: bool,
    /// 争议复核中。
    pub disputed: bool,
    /// 乱点路径触发标记。
    pub via_random_path: bool,
}

/// 走查台。
#[derive(Default)]
pub struct Walkthrough {
    pub envs: Vec<Env>,
    pub findings: Vec<Finding>,
    /// 自动化与人工冲突标记（人工优先断言）。
    pub human_first: bool,
    pub walks: u32,
}

impl Walkthrough {
    pub fn new() -> Self {
        Walkthrough { human_first: true, ..Default::default() }
    }

    /// 环境登记。
    pub fn add_env(&mut self, e: Env) {
        if !self.envs.iter().any(|x| x.id == e.id) {
            self.envs.push(e);
        }
    }

    /// 走查准入：全部环境就位才可走查（缺位补齐后走查）。
    pub fn ready_to_walk(&self) -> Result<(), WkCode> {
        if self.envs.is_empty() || self.envs.iter().any(|e| !e.ready) {
            return Err(WkCode::EnvMissing);
        }
        Ok(())
    }

    /// 发现登记（阻断级即修义务在案）。
    pub fn record(&mut self, f: Finding) -> Result<(), WkCode> {
        if !self.envs.iter().any(|e| e.id == f.env && e.ready) {
            return Err(WkCode::EnvMissing);
        }
        self.walks += 1;
        self.findings.push(f);
        Ok(())
    }

    /// 阻断级即修核验：存在未修阻断级 = 放行拦截。
    pub fn blockers_open(&self) -> bool {
        self.findings.iter().any(|f| f.level == 0 && !f.fixed && !f.disputed)
    }

    /// 细节入账断言：细节级不阻断但必须留台账。
    pub fn minor_logged(&self) -> usize {
        self.findings.iter().filter(|f| f.level == 1).count()
    }

    /// 发现争议复核分级：复核后可降级/维持（争议期不计数为阻断）。
    pub fn dispute(&mut self, idx: usize) -> Result<(), WkCode> {
        let f = self.findings.get_mut(idx).ok_or(WkCode::FindingDispute)?;
        f.disputed = true;
        Ok(())
    }

    /// 复核裁决：降级（0→1）或维持。
    pub fn adjudicate(&mut self, idx: usize, downgrade: bool) -> Result<(), WkCode> {
        let f = self.findings.get_mut(idx).ok_or(WkCode::FindingDispute)?;
        if !f.disputed {
            return Err(WkCode::FindingDispute);
        }
        if downgrade && f.level == 0 {
            f.level = 1;
        }
        f.disputed = false;
        Ok(())
    }

    /// 三维主查覆盖断言（域色核心）：三维都有发现或三维都走过。
    pub fn dims_covered(&self) -> bool {
        [Dim::Interaction, Dim::Consistency, Dim::A11y]
            .iter()
            .all(|d| self.findings.iter().any(|f| f.dim == *d))
            || self.walks >= 3
    }

    /// 乱点路径发现占比 bp（乱点执行证据）。
    pub fn random_path_bp(&self) -> u32 {
        if self.findings.is_empty() {
            return 0;
        }
        let r = self.findings.iter().filter(|f| f.via_random_path).count() as u64;
        (r * 10000 / self.findings.len() as u64) as u32
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves590_checks() -> CheckSet {
    let mut set = CheckSet::new("ves590");
    let mut wk = Walkthrough::new();

    // C590-01 环境登记与缺位检测。
    wk.add_env(Env { id: 1, ready: true });
    wk.add_env(Env { id: 2, ready: false });
    chk(&mut set, "C590-01", wk.ready_to_walk() == Err(WkCode::EnvMissing), "环境缺位检出");

    // C590-02 补齐后走查准入。
    wk.envs[1].ready = true;
    chk(&mut set, "C590-02", wk.ready_to_walk().is_ok(), "补齐准入");

    // C590-03 发现登记（阻断级 + 细节级）。
    let _ = wk.record(Finding { dim: Dim::A11y, env: 1, level: 0, fixed: false, disputed: false, via_random_path: true });
    let _ = wk.record(Finding { dim: Dim::Interaction, env: 2, level: 1, fixed: false, disputed: false, via_random_path: false });
    chk(&mut set, "C590-03", wk.findings.len() == 2 && wk.minor_logged() == 1, "阻断+细节入账");

    // C590-04 阻断级未即修放行拦截。
    chk(&mut set, "C590-04", wk.blockers_open(), "未修阻断拦截");

    // C590-05 阻断即修后放行。
    wk.findings[0].fixed = true;
    chk(&mut set, "C590-05", !wk.blockers_open(), "即修后放行");

    // C590-06 环境外发现拒绝。
    chk(&mut set, "C590-06", wk.record(Finding { dim: Dim::A11y, env: 9, level: 1, fixed: false, disputed: false, via_random_path: false })
        == Err(WkCode::EnvMissing), "环境外拒");

    // C590-07 发现争议复核降级。
    let _ = wk.dispute(0);
    let _ = wk.adjudicate(0, true);
    chk(&mut set, "C590-07", wk.findings[0].level == 1 && !wk.findings[0].disputed, "争议降级");

    // C590-08 复核维持不降级。
    let _ = wk.record(Finding { dim: Dim::Consistency, env: 1, level: 0, fixed: true, disputed: false, via_random_path: true });
    let _ = wk.dispute(2);
    let _ = wk.adjudicate(2, false);
    chk(&mut set, "C590-08", wk.findings[2].level == 0, "复核维持");

    // C590-09 未争议直接裁决拒。
    chk(&mut set, "C590-09", wk.adjudicate(1, true).is_err(), "未争议裁决拒");

    // C590-10 三维主查覆盖断言（域色核心）。
    chk(&mut set, "C590-10", wk.dims_covered(), "三维覆盖");

    // C590-11 乱点路径证据占比。
    chk(&mut set, "C590-11", wk.random_path_bp() == 6666, "乱点占比 66.7%");

    // C590-12 空台三维恒假（不臆测）。
    let wk2 = Walkthrough::new();
    chk(&mut set, "C590-12", !wk2.dims_covered() && wk2.random_path_bp() == 0, "空台不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_gate() {
        let mut wk = Walkthrough::new();
        assert_eq!(wk.ready_to_walk(), Err(WkCode::EnvMissing));
        wk.add_env(Env { id: 1, ready: true });
        assert!(wk.ready_to_walk().is_ok());
    }

    #[test]
    fn blocker_gate() {
        let mut wk = Walkthrough::new();
        wk.add_env(Env { id: 1, ready: true });
        let _ = wk.record(Finding { dim: Dim::A11y, env: 1, level: 0, fixed: false, disputed: false, via_random_path: false });
        assert!(wk.blockers_open());
        wk.findings[0].fixed = true;
        assert!(!wk.blockers_open());
    }
}
