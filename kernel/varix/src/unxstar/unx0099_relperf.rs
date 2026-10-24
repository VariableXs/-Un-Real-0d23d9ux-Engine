//! UNX-F0099 · 早年性能记账：重定位耗时 · 诊断段 0x22F0
//! 验收锚点：UNX-F0099-J1 解析/重定位/清零各阶段耗时微秒级落账，
//! 可跨引导对比。零浮点、零 unwrap、零 expect（耗时以周期计数
//! 折算微秒整数，除法在固定频标下进行，全整数路径）。


use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x22F0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PHASE: u16 = SEG_BASE + 1;  // 阶段越纲
const E_OPEN: u16 = SEG_BASE + 2;   // 账未封先读汇总
const E_TWICE: u16 = SEG_BASE + 3;  // 同阶段重复记账
const E_FREQ: u16 = SEG_BASE + 4;   // 频标非法（零或超纲）
const E_RDONLY: u16 = SEG_BASE + 5; // 账已封，再写被拒
const E_BAL: u16 = SEG_BASE + 6;    // 账目守恒破（Σ阶段 ≠ 总计）
const E_EMPTY: u16 = SEG_BASE + 7;  // 空账出报告

pub fn detail(code: u16) -> &'static str {
    match code {
        E_PHASE => "阶段越纲：阶段号超出三段闭集",
        E_OPEN => "账未封先读汇总：封账后才能出报告",
        E_TWICE => "同阶段重复记账：一阶段一账",
        E_FREQ => "频标非法：TSC 频率须为正且不超 32GHz 上限",
        E_RDONLY => "账已封：早账不可再写",
        E_BAL => "账目守恒破：各阶段之和与总计不符",
        E_EMPTY => "空账出报告：至少一段有账",
        _ => "未知诊断码",
    }
}

// ── 耗时折算：周期 → 微秒（固定频标整数除法）────────────────
pub const FREQ_CAP_MHZ: u64 = 32_000;

/// 周期数折微秒：cycles / freq_mhz（freq_mhz 即每微秒周期数），向下取整。
pub fn cycles_to_us(cycles: u64, freq_mhz: u64) -> Result<u64, u16> {
    if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
        return Err(E_FREQ);
    }
    Ok(cycles / freq_mhz)
}

/// 三段闭集：0=头解析、1=重定位、2=清零。
pub const PHASE_PARSE: usize = 0;
pub const PHASE_RELOC: usize = 1;
pub const PHASE_ZERO: usize = 2;
pub const PHASE_N: usize = 3;

pub fn phase_name(p: usize) -> &'static str {
    match p {
        PHASE_PARSE => "解析",
        PHASE_RELOC => "重定位",
        PHASE_ZERO => "清零",
        _ => "越纲",
    }
}

/// 单阶段账。
#[derive(Clone, Copy, Debug)]
pub struct PhaseEntry {
    pub cycles: u64,
    pub items: u64, // 该阶段处理的条目数
}

/// 早账账本：单次引导的三段耗时，封账后可跨引导对比。
pub struct PerfLedger {
    pub freq_mhz: u64,
    pub phases: [Option<PhaseEntry>; PHASE_N],
    pub sealed: bool,
    pub boot_no: u64,
}

impl PerfLedger {
    pub fn new(boot_no: u64, freq_mhz: u64) -> Result<PerfLedger, u16> {
        if freq_mhz == 0 || freq_mhz > FREQ_CAP_MHZ {
            return Err(E_FREQ);
        }
        Ok(PerfLedger {
            freq_mhz,
            phases: [None, None, None],
            sealed: false,
            boot_no,
        })
    }

    /// 记一笔阶段账：cycles 与 items 均须非零。
    pub fn record(&mut self, phase: usize, cycles: u64, items: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if phase >= PHASE_N {
            return Err(E_PHASE);
        }
        if self.phases[phase].is_some() {
            return Err(E_TWICE);
        }
        self.phases[phase] = Some(PhaseEntry { cycles, items });
        Ok(())
    }

    /// 封账：三段全有账才可封（空段视为漏测）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        for p in &self.phases {
            if p.is_none() {
                return Err(E_EMPTY);
            }
        }
        self.sealed = true;
        Ok(self.total_cycles())
    }

    pub fn total_cycles(&self) -> u64 {
        self.phases
            .iter()
            .filter_map(|p| p.as_ref())
            .map(|e| e.cycles)
            .sum()
    }

    /// 汇总微秒账（封账前拒绝）。
    pub fn summary_us(&self) -> Result<[u64; PHASE_N], u16> {
        if !self.sealed {
            return Err(E_OPEN);
        }
        let mut out = [0u64; PHASE_N];
        for (i, p) in self.phases.iter().enumerate() {
            if let Some(e) = p {
                out[i] = cycles_to_us(e.cycles, self.freq_mhz)?;
            }
        }
        Ok(out)
    }

    /// 渲染：跨引导对比行。
    pub fn render(&self) -> Result<String, u16> {
        let us = self.summary_us()?;
        Ok(format!(
            "引导#{} @{}MHz 解析{}us 重定位{}us 清零{}us",
            self.boot_no, self.freq_mhz, us[0], us[1], us[2]
        ))
    }
}

/// 跨引导对比账：两次引导的同阶段微秒差与更慢方。
pub struct BootCompare {
    pub slower_phase: Option<usize>,
    pub deltas_us: [i64; PHASE_N],
}

pub fn compare_boots(a: &PerfLedger, b: &PerfLedger) -> Result<BootCompare, u16> {
    let ua = a.summary_us()?;
    let ub = b.summary_us()?;
    let mut deltas = [0i64; PHASE_N];
    let mut slower = None;
    let mut worst: i64 = 0;
    for i in 0..PHASE_N {
        deltas[i] = ub[i] as i64 - ua[i] as i64;
        if deltas[i] > worst {
            worst = deltas[i];
            slower = Some(i);
        }
    }
    Ok(BootCompare {
        slower_phase: slower,
        deltas_us: deltas,
    })
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三段落账 → 封账 → 微秒折算（@2000MHz：2M 周期 = 1ms = 1000us）。
    let mut a = PerfLedger::new(1, 2000)?;
    a.record(PHASE_PARSE, 2_000_000, 120)?;
    a.record(PHASE_RELOC, 4_000_000, 350)?;
    a.record(PHASE_ZERO, 1_000_000, 64)?;
    if a.total_cycles() != 7_000_000 {
        return Err(E_BAL);
    }
    a.seal()?;
    let us = a.summary_us()?;
    if us != [1000, 2000, 500] {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：渲染行含全部三段。
    let rep = a.render()?;
    if !rep.contains("引导#1") || !rep.contains("解析1000us") || !rep.contains("清零500us") {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：跨引导对比——第二次引导重定位变慢 1500us（5M−4M 周期 @2GHz）。
    let mut b = PerfLedger::new(2, 2000)?;
    b.record(PHASE_PARSE, 2_000_000, 120)?;
    b.record(PHASE_RELOC, 5_000_000, 360)?;
    b.record(PHASE_ZERO, 1_000_000, 64)?;
    b.seal()?;
    let cmp = compare_boots(&a, &b)?;
    if cmp.deltas_us[PHASE_RELOC] != 1500 || cmp.slower_phase != Some(PHASE_RELOC) {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：频标零 / 超 32GHz → E_FREQ（折算与建账两路同拒）。
    if cycles_to_us(1000, 0) != Err(E_FREQ) {
        return Err(E_FREQ);
    }
    if cycles_to_us(1000, FREQ_CAP_MHZ + 1) != Err(E_FREQ) {
        return Err(E_FREQ);
    }
    if PerfLedger::new(3, 0).is_ok() {
        return Err(E_FREQ);
    }
    hits += 1;

    // I2：阶段越纲 → E_PHASE。
    let mut i2 = PerfLedger::new(3, 2000)?;
    if i2.record(PHASE_N, 100, 1) != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    hits += 1;

    // I3：同阶段重复 → E_TWICE；空段封账 → E_EMPTY。
    i2.record(PHASE_PARSE, 100, 1)?;
    if i2.record(PHASE_PARSE, 200, 2) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    if i2.seal() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：未封账读汇总/渲染 → E_OPEN。
    if i2.summary_us() != Err(E_OPEN) {
        return Err(E_OPEN);
    }
    if i2.render() != Err(E_OPEN) {
        return Err(E_OPEN);
    }
    hits += 1;

    // I5：封账后再写 → E_RDONLY（record 与 seal 两路）。
    let mut i5 = PerfLedger::new(4, 800)?;
    i5.record(PHASE_PARSE, 800_000, 10)?;
    i5.record(PHASE_RELOC, 1_600_000, 20)?;
    i5.record(PHASE_ZERO, 400_000, 5)?;
    i5.seal()?;
    if i5.record(PHASE_PARSE, 1, 1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i5.seal() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
