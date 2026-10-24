//! UNX-F0119 · 早期失败显性化探针 · 诊断段 0x2410
//! 验收锚点：UNX-F0119-J1 早年失败三要素（阶段/症状/取证号）
//! 强制落账，静默失败全数检出。零浮点、零 unwrap、零 expect。


use alloc::format;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2410;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STAGE: u16 = SEG_BASE + 1; // 阶段越纲（六阶段闭集外）
const E_SILENT: u16 = SEG_BASE + 2;// 静默失败（有跳阶段无账）
const E_TWICE: u16 = SEG_BASE + 3; // 同阶段重复落账（非重试语义）
const E_PROBE: u16 = SEG_BASE + 4; // 取证号越纲（0 非法）
const E_RDONLY: u16 = SEG_BASE + 5;// 账已封，再写被拒
const E_BAL: u16 = SEG_BASE + 6;   // 账不平（过+败 ≠ 已探）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_STAGE => "阶段越纲：六阶段闭集 [页表,堆,帧簿,调度,中断,交接] 外",
        E_SILENT => "静默失败：跳阶段无账即探针失明",
        E_TWICE => "重复落账：同阶段二次终账（重试须走子账）",
        E_PROBE => "取证号非法：须为正整数",
        E_RDONLY => "账已封：交接后禁止再写",
        E_BAL => "账不平：通过数 + 失败数 ≠ 已探阶段数",
        _ => "未知诊断码",
    }
}

/// 六阶段闭集。
pub const S_PAGING: usize = 0;
pub const S_HEAP: usize = 1;
pub const S_FRAMEBUFFER: usize = 2;
pub const S_SCHED: usize = 3;
pub const S_INTR: usize = 4;
pub const S_HANDOFF: usize = 5;
pub const STAGE_N: usize = 6;

pub fn stage_name(s: usize) -> Result<&'static str, u16> {
    match s {
        S_PAGING => Ok("页表"),
        S_HEAP => Ok("堆"),
        S_FRAMEBUFFER => Ok("帧簿"),
        S_SCHED => Ok("调度"),
        S_INTR => Ok("中断"),
        S_HANDOFF => Ok("交接"),
        _ => Err(E_STAGE),
    }
}

/// 阶段账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StageState {
    Untouched, // 未探
    Passed,    // 过
    Failed { probe: u64 }, // 败（带取证号）
}

/// 早账探针：逐阶段必须显性落账（过/败二选一），跳阶段即静默失败。
pub struct EarlyProbe {
    pub stages: [StageState; STAGE_N],
    pub sealed: bool,
    pub order: Vec<usize>, // 实际探测到达序
}

impl EarlyProbe {
    pub fn new() -> EarlyProbe {
        EarlyProbe {
            stages: [
                StageState::Untouched,
                StageState::Untouched,
                StageState::Untouched,
                StageState::Untouched,
                StageState::Untouched,
                StageState::Untouched,
            ],
            sealed: false,
            order: Vec::new(),
        }
    }

    /// 过账。
    pub fn pass(&mut self, stage: usize) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if stage >= STAGE_N {
            return Err(E_STAGE);
        }
        if self.stages[stage] != StageState::Untouched {
            return Err(E_TWICE);
        }
        self.stages[stage] = StageState::Passed;
        self.order.push(stage);
        Ok(())
    }

    /// 败账：取证号必须为正。
    pub fn fail(&mut self, stage: usize, probe: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if stage >= STAGE_N {
            return Err(E_STAGE);
        }
        if probe == 0 {
            return Err(E_PROBE);
        }
        if self.stages[stage] != StageState::Untouched {
            return Err(E_TWICE);
        }
        self.stages[stage] = StageState::Failed { probe };
        self.order.push(stage);
        Ok(())
    }

    /// 封账：显性化核——不许存在 Untouched 阶段（静默失败检出）。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        let silent: Vec<usize> = self
            .stages
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == StageState::Untouched)
            .map(|(i, _)| i)
            .collect();
        if !silent.is_empty() {
            return Err(E_SILENT);
        }
        self.sealed = true;
        Ok(self.order.len())
    }

    /// 静默失败点名（阶段名清单）。
    pub fn silent_items(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, s) in self.stages.iter().enumerate() {
            if *s == StageState::Untouched {
                if let Ok(name) = stage_name(i) {
                    out.push(format!("{}段未探（静默）", name));
                }
            }
        }
        out
    }

    /// 账守恒：过数 + 败数 == 已探数（= 全量，封账后）。
    pub fn audit(&self) -> Result<(), u16> {
        let pass = self
            .stages
            .iter()
            .filter(|s| **s == StageState::Passed)
            .count();
        let fail = self
            .stages
            .iter()
            .filter(|s| matches!(s, StageState::Failed { .. }))
            .count();
        if pass + fail != self.order.len() {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.order.is_empty() {
            return Err(E_BAL);
        }
        let mut s = String::new();
        for (i, st) in self.order.iter().enumerate() {
            if i > 0 {
                s.push_str("；");
            }
            let name = stage_name(*st).unwrap_or("?");
            match self.stages[*st] {
                StageState::Passed => s.push_str(&format!("{}✓", name)),
                StageState::Failed { probe } => {
                    s.push_str(&format!("{}✗(取证{})", name, probe))
                }
                StageState::Untouched => s.push_str(&format!("{}—", name)),
            }
        }
        Ok(s)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：六阶段全过 → 封账 6 探，渲染全 ✓。
    let mut p = EarlyProbe::new();
    for s in 0..STAGE_N {
        p.pass(s)?;
    }
    let n = p.seal()?;
    if n != 6 {
        return Err(E_BAL);
    }
    p.audit()?;
    let rep = p.render()?;
    if !rep.contains("页表✓") || !rep.contains("交接✓") {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：第 3 阶段败（取证 2024）→ 封账可，渲染显性 ✗。
    let mut p2 = EarlyProbe::new();
    p2.pass(S_PAGING)?;
    p2.pass(S_HEAP)?;
    p2.fail(S_FRAMEBUFFER, 2024)?;
    p2.pass(S_SCHED)?;
    p2.pass(S_INTR)?;
    p2.pass(S_HANDOFF)?;
    p2.seal()?;
    let rep2 = p2.render()?;
    if !rep2.contains("帧簿✗(取证2024)") {
        return Err(E_BAL);
    }
    p2.audit()?;
    hits += 1;

    // R3：静默点名——只探两阶段 → 四阶段进名单。
    let mut p3 = EarlyProbe::new();
    p3.pass(S_PAGING)?;
    p3.fail(S_HEAP, 7)?;
    let items = p3.silent_items();
    if items.len() != 4 || !items[0].contains("帧簿") {
        return Err(E_SILENT);
    }
    if p3.seal() != Err(E_SILENT) {
        return Err(E_SILENT);
    }
    hits += 1;

    // I1：阶段越纲 → E_STAGE。
    let mut i1 = EarlyProbe::new();
    if i1.pass(STAGE_N) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    if i1.fail(STAGE_N + 5, 1) != Err(E_STAGE) {
        return Err(E_STAGE);
    }
    hits += 1;

    // I2：取证号零 → E_PROBE。
    if i1.fail(S_PAGING, 0) != Err(E_PROBE) {
        return Err(E_PROBE);
    }
    hits += 1;

    // I3：同阶段重复终账 → E_TWICE。
    i1.fail(S_PAGING, 9)?;
    if i1.pass(S_PAGING) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    if i1.fail(S_PAGING, 10) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：封账后再写 → E_RDONLY；静默失败封账 → E_SILENT（已验 R3）。
    i1.seal()?;
    if i1.pass(S_HEAP) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i1.fail(S_HEAP, 3) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i1.seal() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：账破注入（order 缺账）→ E_BAL；空账渲染 → E_BAL。
    let mut i5 = EarlyProbe::new();
    i5.pass(S_PAGING)?;
    i5.order.pop(); // 人为抹到达账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    let i5b = EarlyProbe::new();
    if i5b.render() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
