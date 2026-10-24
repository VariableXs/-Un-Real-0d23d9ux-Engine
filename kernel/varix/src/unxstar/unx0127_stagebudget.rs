//! UNX-F0127 · 阶段耗时预算与超标告警 · 诊断段 0x2450
//! 验收锚点：UNX-F0127-J1 六阶段各设微秒预算，实测超支告警落账，
//! 预算可查可核。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2450;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAGE: u16 = SEG_BASE + 1; // 阶段越纲（六段闭集外）
const E_TWICE: u16 = SEG_BASE + 2; // 同阶段重复记账
const E_RDONLY: u16 = SEG_BASE + 3;// 账已封，再记被拒
const E_EMPTY: u16 = SEG_BASE + 4; // 零记账出报告
const E_FREQ: u16 = SEG_BASE + 5;  // 频标非法
const E_BAL: u16 = SEG_BASE + 6;   // 账不平（Σ实测 ≠ 汇总）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STAGE => "阶段越纲：六段闭集外",
        E_TWICE => "同阶段重复记账：一段一实测",
        E_RDONLY => "账已封：封账后禁止再记",
        E_EMPTY => "零记账：未记任何实测禁止出报告",
        E_FREQ => "频标非法：TSC 频率须为正且不超 32GHz",
        E_BAL => "账不平：各段实测之和与汇总不符",
        _ => "未知诊断码",
    }
}

pub const FREQ_CAP_MHZ: u64 = 32_000;
/// 默认预算（微秒）：页表 5ms / 堆 3ms / 帧簿 8ms / 调度 4ms / 中断 6ms / 交接 10ms。
pub const DEFAULT_BUDGET_US: [u64; 6] = [5_000, 3_000, 8_000, 4_000, 6_000, 10_000];

pub fn stage_name(s: usize) -> Result<&'static str, u16> {
    match s {
        0 => Ok("页表"),
        1 => Ok("堆"),
        2 => Ok("帧簿"),
        3 => Ok("调度"),
        4 => Ok("中断"),
        5 => Ok("交接"),
        _ => Err(E_STAGE),
    }
}

/// 周期折微秒。
pub fn cycles_to_us(cycles: u64, freq_mhz: u64) -> Result<u64, u16> {
    if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
        return Err(E_FREQ);
    }
    Ok(cycles / freq_mhz)
}

/// 单段账。
#[derive(Clone, Copy, Debug)]
pub struct StageActual {
    pub cycles: u64,
    pub us: u64,
    pub over_budget: bool,
}

/// 预算账本。
pub struct BudgetBook {
    pub freq_mhz: u64,
    pub budgets: [u64; 6],
    pub actuals: [Option<StageActual>; 6],
    pub sealed: bool,
    pub total_us: u64, // Σ已记段微秒（随记随更）
}

impl BudgetBook {
    pub fn new(freq_mhz: u64) -> Result<BudgetBook, u16> {
        if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
            return Err(E_FREQ);
        }
        Ok(BudgetBook {
            freq_mhz,
            budgets: DEFAULT_BUDGET_US,
            actuals: [None, None, None, None, None, None],
            sealed: false,
            total_us: 0,
        })
    }

    /// 记实测（周期数）：折微秒、对预算、超标落告警位。
    pub fn record(&mut self, stage: usize, cycles: u64) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if stage >= 6 {
            return Err(E_STAGE);
        }
        if self.actuals[stage].is_some() {
            return Err(E_TWICE);
        }
        let us = cycles_to_us(cycles, self.freq_mhz)?;
        let over = us > self.budgets[stage];
        self.actuals[stage] = Some(StageActual {
            cycles,
            us,
            over_budget: over,
        });
        self.total_us += us;
        Ok(over)
    }

    /// 改预算（封账前）。
    pub fn set_budget(&mut self, stage: usize, us: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if stage >= 6 {
            return Err(E_STAGE);
        }
        self.budgets[stage] = us;
        Ok(())
    }

    /// 封账：至少一段有实测。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.actuals.iter().all(|a| a.is_none()) {
            return Err(E_EMPTY);
        }
        self.sealed = true;
        Ok(self.total_us)
    }

    /// 汇总微秒重算（审计基准）。
    fn sum_recompute(&self) -> u64 {
        self.actuals
            .iter()
            .filter_map(|a| a.as_ref())
            .map(|a| a.us)
            .sum()
    }

    /// 告警名单：超标阶段（名+超支微秒）。
    pub fn alerts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, a) in self.actuals.iter().enumerate() {
            if let Some(act) = a {
                if act.over_budget {
                    if let Ok(name) = stage_name(i) {
                        out.push(format!(
                            "{}超支 {}us（预算 {}）",
                            name,
                            act.us - self.budgets[i],
                            self.budgets[i]
                        ));
                    }
                }
            }
        }
        out
    }

    /// 账守恒：Σ已记段微秒 == total。
    pub fn audit(&self) -> Result<(), u16> {
        if self.sum_recompute() != self.total_us {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.actuals.iter().all(|a| a.is_none()) {
            return Err(E_EMPTY);
        }
        let alerts = self.alerts();
        Ok(format!(
            "频标 {}MHz，Σ实测 {}us，告警 {} 条",
            self.freq_mhz,
            self.total_us,
            alerts.len()
        ))
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：@2000MHz 六段全记且全在预算内 → 零告警封账。
    let mut b = BudgetBook::new(2000)?;
    // 预算 us × 2000 = 周期：5000us→10M；恰等不超标（us > budget 才算超）。
    for (i, &bud) in DEFAULT_BUDGET_US.iter().enumerate() {
        let over = b.record(i, bud * 2000)?;
        if over {
            return Err(E_BAL);
        }
    }
    b.audit()?;
    b.seal()?;
    if !b.alerts().is_empty() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：帧簿超支（9ms > 8ms 预算）→ 告警 1 条含超支 1000us。
    let mut b2 = BudgetBook::new(2000)?;
    b2.record(S2_PAGING, 4_000_000)?; // 2000us < 5000
    b2.record(S2_FB, 18_000_000)?;    // 9000us > 8000
    let al = b2.alerts();
    if al.len() != 1 || !al[0].contains("帧簿超支 1000us") {
        return Err(E_BAL);
    }
    b2.seal()?;
    let rep = b2.render()?;
    if !rep.contains("告警 1 条") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：改预算后同实测不再超（预算放宽到 10ms）。
    let mut b3 = BudgetBook::new(2000)?;
    b3.set_budget(S2_FB, 10_000)?;
    b3.record(S2_FB, 18_000_000)?;
    if !b3.alerts().is_empty() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：周期折算边界（0 周期 → 0us；单周期向下取整）。
    if cycles_to_us(0, 2000)? != 0 || cycles_to_us(1999, 2000)? != 0 {
        return Err(E_BAL);
    }
    if cycles_to_us(2000, 2000)? != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：频标非法 → E_FREQ。
    if BudgetBook::new(0).is_ok() {
        return Err(E_FREQ);
    }
    if BudgetBook::new(FREQ_CAP_MHZ + 1).is_ok() {
        return Err(E_FREQ);
    }
    hits += 1;

    // I2：阶段越纲 → E_STAGE。
    let mut i2 = BudgetBook::new(2000)?;
    if i2.record(6, 100) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    if i2.set_budget(9, 100) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I3：同段重复记账 → E_TWICE。
    i2.record(S2_PAGING, 1_000_000)?;
    if i2.record(S2_PAGING, 2_000_000) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：封账后再记/再封 → E_RDONLY；零记账封账 → E_EMPTY。
    i2.seal()?;
    if i2.record(S2_HEAP, 1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i2.seal() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    let mut i4 = BudgetBook::new(2000)?;
    if i4.seal() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if i4.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I5：账破注入 → E_BAL。
    let mut i5 = BudgetBook::new(2000)?;
    i5.record(S2_PAGING, 2_000_000)?;
    i5.total_us = 99; // 人为破账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

const S2_PAGING: usize = 0;
const S2_HEAP: usize = 1;
const S2_FB: usize = 2;
