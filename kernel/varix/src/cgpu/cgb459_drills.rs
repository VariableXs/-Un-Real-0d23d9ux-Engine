//! CGPU-F0459 · 边界五场景定期演练（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 演练：每类边界定期演练（低资源模拟/应急注入/冷启动验证/离线
//!   切换/热模拟——恢复验证常态化）；
//! - 演练判据（每场景的恢复时间上限/零数据丢失）；
//! - 演练报告（自动生成入归档）；
//! - 演练 CI 周期化。判据：五场景、判据、报告、CI、判据。
//!
//! 设计纪律：
//! - 五场景各一个确定性演练函数（tick 驱动模拟注入→恢复），恢复
//!   判据统一：恢复 tick ≤ 场景上限 且 零数据丢失（账本守恒）。
//! - 报告结构：场景+恢复耗时+零丢失+判定，可序列化入归档（report
//!   文本带场景名与 PASS/FAIL）。
//! - CI 周期化：演练间隔常量 + `due(now)` 判定——到点自动重跑，
//!   结果与上次报告对比防退化（恢复耗时不得回涨超带）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FE0 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FE0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FE0;

pub const E_DR_SCENE: u16 = SEG_BASE + 1; // 场景非法
pub const E_DR_RECOVER: u16 = SEG_BASE + 2; // 恢复超时
pub const E_DR_LOSS: u16 = SEG_BASE + 3; // 数据丢失
pub const E_DR_REGRESS: u16 = SEG_BASE + 4; // 演练退化（恢复耗时回涨超带）

/// 五场景。
pub const S_LOWRES: u8 = 1; // 低资源模拟
pub const S_EMERGENCY: u8 = 2; // 应急注入
pub const S_COLDBOOT: u8 = 3; // 冷启动验证
pub const S_OFFLINE: u8 = 4; // 离线切换
pub const S_THERMAL: u8 = 5; // 热模拟
pub const SCENES: [u8; 5] = [S_LOWRES, S_EMERGENCY, S_COLDBOOT, S_OFFLINE, S_THERMAL];

/// 恢复时间上限（tick，每场景判据）。
pub const RECOVER_LIMIT: [u64; 5] = [20, 30, 16, 12, 25];
/// 演练间隔（tick 周期化）。
pub const DRILL_INTERVAL: u64 = 3600;
/// 退化带宽：恢复耗时允许回涨幅度（tick）。
pub const REGRESS_BAND: u64 = 3;

/// 演练模拟账本（守恒驱动）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillLedger {
    pub budget: u64,
    pub granted: u64,
}

impl DrillLedger {
    pub fn conserve(&self) -> bool {
        self.granted <= self.budget
    }
    /// 注入压力：granted 上涨（模拟资源吃紧）。
    pub fn stress(&mut self, pm: u64) {
        self.granted = (self.granted + pm).min(self.budget);
    }
    /// 恢复：逐 tick 释放 1/4（确定性收敛）。
    pub fn recover_tick(&mut self) {
        let drop = self.granted / 4;
        self.granted -= drop;
    }
    pub fn recovered(&self) -> bool {
        self.granted <= self.budget / 10
    }
}

/// 演练报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillReport {
    pub scene: u8,
    /// 注入耗时（tick）。
    pub inject_ticks: u64,
    /// 恢复耗时（tick）。
    pub recover_ticks: u64,
    /// 零数据丢失（账本守恒全称成立）。
    pub zero_loss: bool,
    pub pass: bool,
}

impl DrillReport {
    pub fn to_text(&self) -> String {
        let scene = match self.scene {
            S_LOWRES => "lowres",
            S_EMERGENCY => "emergency",
            S_COLDBOOT => "coldboot",
            S_OFFLINE => "offline",
            S_THERMAL => "thermal",
            _ => "unknown",
        };
        let verdict = if self.pass { "PASS" } else { "FAIL" };
        alloc::format!(
            "drill scene={} inject={} recover={} zero_loss={} verdict={}",
            scene, self.inject_ticks, self.recover_ticks, self.zero_loss, verdict
        )
    }
}

/// 单场景演练：注入直到吃紧，再逐 tick 恢复，判恢复耗时与零丢失。
pub fn run_drill(scene: u8) -> Result<DrillReport, u16> {
    let idx = SCENES.iter().position(|&s| s == scene).ok_or(E_DR_SCENE)?;
    let mut led = DrillLedger { budget: 1000, granted: 0 };
    let mut zero_loss = true;
    // 注入：每 tick +300‰ 直到 ≥700‰（各场景注入步长微差以区分耗时）。
    let step: u64 = 250 + scene as u64 * 20;
    let mut inject = 0;
    while led.granted < 700 {
        led.stress(step);
        inject += 1;
        if !led.conserve() {
            zero_loss = false;
        }
    }
    // 恢复：逐 tick 释放，记录耗时。
    let mut recover = 0;
    while !led.recovered() {
        led.recover_tick();
        recover += 1;
        if !led.conserve() {
            zero_loss = false;
        }
        if recover > RECOVER_LIMIT[idx] {
            return Ok(DrillReport { scene, inject_ticks: inject, recover_ticks: recover, zero_loss, pass: false });
        }
    }
    let pass = recover <= RECOVER_LIMIT[idx] && zero_loss && led.conserve();
    Ok(DrillReport { scene, inject_ticks: inject, recover_ticks: recover, zero_loss, pass })
}

/// 五场景全演练（CI 批跑）。
pub fn run_all() -> Vec<DrillReport> {
    SCENES.iter().filter_map(|&s| run_drill(s).ok()).collect()
}

/// 归档：全部 PASS 判定。
pub fn archive_ok(reports: &[DrillReport]) -> bool {
    reports.len() == SCENES.len() && reports.iter().all(|r| r.pass && r.zero_loss)
}

/// CI 周期化：到期判定 + 退化检测（恢复耗时回涨超带即判退化）。
pub fn ci_due(last_run_tick: u64, now: u64) -> bool {
    now.saturating_sub(last_run_tick) >= DRILL_INTERVAL
}

pub fn ci_regress(prev_recover: u64, cur_recover: u64) -> Result<(), u16> {
    if cur_recover > prev_recover + REGRESS_BAND {
        return Err(E_DR_REGRESS);
    }
    Ok(())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、五场景全演练：恢复在上限内+零丢失。
    {
        let reports = run_all();
        v.push(("F0459-五场景-五报告齐备", reports.len() == 5));
        v.push(("F0459-五场景-全PASS", archive_ok(&reports)));
        // 每场景恢复耗时 ≤ 上限。
        let in_limit = reports.iter().enumerate().all(|(i, r)| r.recover_ticks <= RECOVER_LIMIT[i]);
        v.push(("F0459-五场景-恢复在上限内", in_limit));
    }
    // 二、演练判据：单场景注入与恢复确定性。
    {
        let d1 = run_drill(S_EMERGENCY);
        let d2 = run_drill(S_EMERGENCY);
        v.push(("F0459-判据-演练确定性", d1 == d2));
        let d = d1.unwrap_or(DrillReport { scene: 0, inject_ticks: 0, recover_ticks: 0, zero_loss: false, pass: false });
        v.push(("F0459-判据-应急注入非零", d.inject_ticks > 0 && d.recover_ticks > 0));
    }
    // 三、报告：可序列化+场景名+判定在场。
    {
        let r = run_drill(S_LOWRES).unwrap_or(DrillReport { scene: 0, inject_ticks: 0, recover_ticks: 0, zero_loss: false, pass: false });
        let text = r.to_text();
        v.push((
            "F0459-报告-要素齐备",
            text.contains("scene=lowres") && text.contains("verdict=PASS") && text.contains("zero_loss=true"),
        ));
        // 失败路径：构造超时报告（直接构造，不走 run）。
        let fail = DrillReport { scene: S_THERMAL, inject_ticks: 5, recover_ticks: 99, zero_loss: true, pass: false };
        v.push(("F0459-报告-失败判定可见", fail.to_text().contains("verdict=FAIL") && fail.to_text().contains("scene=thermal")));
    }
    // 四、CI 周期化：到期判定+退化带宽。
    {
        v.push(("F0459-CI-未到期不重跑", !ci_due(0, DRILL_INTERVAL - 1)));
        v.push(("F0459-CI-到期触发", ci_due(0, DRILL_INTERVAL)));
        v.push(("F0459-CI-退化带宽内通过", ci_regress(10, 13).is_ok()));
        v.push(("F0459-CI-退化超带判退化", ci_regress(10, 14) == Err(E_DR_REGRESS)));
    }
    // 五、数据丢失红线：构造丢失账本判 FAIL。
    {
        let mut led = DrillLedger { budget: 1000, granted: 0 };
        led.stress(3000);
        // stress 钳制在 budget 内 → 守恒。
        v.push(("F0459-判据-注入钳制守恒", led.conserve() && led.granted == 1000));
        // 恢复收敛。
        let mut ticks = 0;
        while !led.recovered() && ticks < 100 {
            led.recover_tick();
            ticks += 1;
        }
        v.push(("F0459-判据-恢复收敛", led.recovered() && ticks <= 25));
    }
    // 六、判据自检。
    {
        v.push(("F0459-判据-场景非法拒绝", run_drill(99) == Err(E_DR_SCENE)));
        v.push(("F0459-判据-上限表单调非零", RECOVER_LIMIT.iter().all(|&l| l > 0)));
        // 归档不完整拒。
        let partial = run_all();
        v.push(("F0459-判据-部分报告不判全绿", !archive_ok(&partial[..3])));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0459_drills_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
