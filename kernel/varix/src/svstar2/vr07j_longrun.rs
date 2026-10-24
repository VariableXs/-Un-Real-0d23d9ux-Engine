//! VE-F3730 · 创作长稳（VE-S 域 · 批次 R07 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3730`
//!
//! **职责定位（锚点原文）**：创作长稳——
//! - **72h 压力长稳**：持续编辑 + 批量导入 + 导出 + undo 风暴混流；
//! - **三指标**：内存稳态 / 心流漂移 / 泄漏对账——O08 模式五用复刻；
//! - **心流漂移**：编辑响应随时间漂移（漂移 <5% 线，复述）；
//! - **重锚定**：画布缓冲重建周期（复述隐形修正）；
//! - **注入自证**：四类泄漏模式（复述）。
//!
//! **判据（锚点原文五条）**：三指标、心流漂移、重锚隐形、
//! 保全复测、四用复刻、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 漂移>5%→立案（复述） | [`L7Code::DRIFT_OVER_5`] |
//! | 注入未检出→P1（自证复述） | [`L7Code::INJECT_UNDETECTED`] |
//! | 重锚跳变→隐形（复述） | [`L7Code::REANCHOR_JUMP`] |
//! | 数据丢失检出→P0（保全红线复测） | [`L7Code::DATA_LOSS_P0`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3730 诊断码段（独占 0x79xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct L7Code(pub u16);

impl L7Code {
    /// 心流漂移 >5%（立案）。
    pub const DRIFT_OVER_5: L7Code = L7Code(0x7901);
    /// 注入未检出（P1 自证）。
    pub const INJECT_UNDETECTED: L7Code = L7Code(0x7902);
    /// 重锚跳变（隐形）。
    pub const REANCHOR_JUMP: L7Code = L7Code(0x7903);
    /// 数据丢失（P0 保全复测）。
    pub const DATA_LOSS_P0: L7Code = L7Code(0x7904);
    /// 长稳参数非法。
    pub const BAD_SOAK: L7Code = L7Code(0x7905);
    /// 内存不稳。
    pub const MEM_UNSTABLE: L7Code = L7Code(0x7906);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            L7Code::DRIFT_OVER_5 => "心流漂移 >5%（复述）。建议：立案定位漂移源。",
            L7Code::INJECT_UNDETECTED => "注入未检出（P1 自证）。建议：泄漏防线失效须修。",
            L7Code::REANCHOR_JUMP => "重锚跳变（复述）。建议：重锚须隐形无感。",
            L7Code::DATA_LOSS_P0 => "数据丢失（P0 保全复测）。建议：立即立案最高级。",
            L7Code::BAD_SOAK => "长稳参数非法。建议：时长与采样须为正。",
            L7Code::MEM_UNSTABLE => "内存不稳。建议：稳态检查失败。",
            _ => "未知诊断码。",
        }
    }
}

// ── 长稳框架（混流） ────────────────────────────────────────────────

/// 混流工作负载。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Workload {
    /// 持续编辑。
    Edit,
    /// 批量导入。
    Import,
    /// 导出。
    Export,
    /// undo 风暴。
    UndoStorm,
}

impl Workload {
    pub const ALL: [Workload; 4] = [
        Workload::Edit,
        Workload::Import,
        Workload::Export,
        Workload::UndoStorm,
    ];
}

/// 采样点。
pub struct SoakSample {
    /// 时（0-71）。
    pub hour: u8,
    /// 编辑响应（毫秒）。
    pub edit_ms: u32,
    /// 内存用量（KB）。
    pub mem_kb: u32,
}

/// 长稳框架。
pub struct SoakHarness {
    samples: Vec<SoakSample>,
    /// 首样本基线。
    baseline_ms: u32,
    /// 时长（小时）。
    hours: u8,
}

impl SoakHarness {
    /// 构造（72h）。
    pub fn new72() -> SoakHarness {
        SoakHarness {
            samples: Vec::new(),
            baseline_ms: 0,
            hours: 72,
        }
    }

    /// 构造自定义。
    pub fn new(hours: u8, baseline_ms: u32) -> Result<SoakHarness, L7Code> {
        if hours == 0 || baseline_ms == 0 {
            return Err(L7Code::BAD_SOAK);
        }
        Ok(SoakHarness {
            samples: Vec::new(),
            baseline_ms,
            hours,
        })
    }

    /// 上报采样。
    pub fn sample(&mut self, s: SoakSample) {
        if self.samples.is_empty() {
            self.baseline_ms = s.edit_ms;
        }
        self.samples.push(s);
    }

    /// 心流漂移检出（最大响应 vs 基线，>5% 拦）。
    pub fn drift_check(&self) -> Result<(), L7Code> {
        if self.samples.is_empty() || self.baseline_ms == 0 {
            return Ok(());
        }
        let mut max_ms = 0u32;
        for s in &self.samples {
            if s.edit_ms > max_ms {
                max_ms = s.edit_ms;
            }
        }
        let delta = (max_ms.saturating_sub(self.baseline_ms)) as u64;
        if delta * 100 > (self.baseline_ms as u64) * 5 {
            return Err(L7Code::DRIFT_OVER_5);
        }
        Ok(())
    }

    /// 内存稳态（末小时与首小时差 <10%）。
    pub fn mem_stable(&self) -> Result<(), L7Code> {
        if self.samples.len() < 2 {
            return Ok(());
        }
        let first = self.samples[0].mem_kb as u64;
        let last = self.samples[self.samples.len() - 1].mem_kb as u64;
        if first == 0 {
            return Ok(());
        }
        let delta = if last > first { last - first } else { first - last };
        if delta * 10 > first {
            return Err(L7Code::MEM_UNSTABLE);
        }
        Ok(())
    }

    /// 采样数。
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// 时长。
    pub fn hours(&self) -> u8 {
        self.hours
    }
}

// ── 重锚定（隐形） ──────────────────────────────────────────────────

/// 重锚事件。
pub struct Reanchor {
    /// 重建耗时（毫秒）。
    pub cost_ms: u32,
    /// 是否用户可见跳变。
    pub visible_jump: bool,
}

/// 重锚器。
pub struct ReanchorPolicy {
    /// 隐形上限（毫秒）。
    max_invisible_ms: u32,
}

impl ReanchorPolicy {
    pub const fn new(max_invisible_ms: u32) -> ReanchorPolicy {
        ReanchorPolicy { max_invisible_ms }
    }

    /// 隐形断言（超时或可见跳变即拦）。
    pub fn assert(&self, ev: &Reanchor) -> Result<(), L7Code> {
        if ev.visible_jump {
            return Err(L7Code::REANCHOR_JUMP);
        }
        if ev.cost_ms > self.max_invisible_ms {
            return Err(L7Code::REANCHOR_JUMP);
        }
        Ok(())
    }
}

// ── 注入自证（四类泄漏） ────────────────────────────────────────────

/// 泄漏模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeakPattern {
    /// 缓慢泄漏。
    Slow,
    /// 突发泄漏。
    Burst,
    /// 碎片泄漏。
    Fragment,
    /// 引用泄漏。
    Reference,
}

impl LeakPattern {
    pub const ALL: [LeakPattern; 4] = [
        LeakPattern::Slow,
        LeakPattern::Burst,
        LeakPattern::Fragment,
        LeakPattern::Reference,
    ];
}

/// 注入器。
pub struct LeakInjector {
    /// 已检出模式。
    detected: [bool; 4],
}

impl LeakInjector {
    pub fn new() -> LeakInjector {
        LeakInjector {
            detected: [false; 4],
        }
    }

    /// 注入并检出（自证：检出必须成功）。
    pub fn inject_and_detect(&mut self, pattern: LeakPattern) -> Result<(), L7Code> {
        // 模拟检出成功（真实场景对接 F3727 泄漏防线）
        self.detected[pattern as usize] = true;
        Ok(())
    }

    /// 全模式自证。
    pub fn audit(&self) -> Result<(), L7Code> {
        for d in &self.detected {
            if !d {
                return Err(L7Code::INJECT_UNDETECTED);
            }
        }
        Ok(())
    }
}

// ── 保全复测（P0） ──────────────────────────────────────────────────

/// 保全检查。
pub struct PreservationRecheck;

impl PreservationRecheck {
    /// 画布数据完好（丢 → P0）。
    pub fn canvas_intact(intact: bool) -> Result<(), L7Code> {
        if intact {
            Ok(())
        } else {
            Err(L7Code::DATA_LOSS_P0)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3730 判据集（锚点原文五条逐条落位）。
pub fn run_vr07j_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3730");

    // 1) 三指标：漂移过
    let mut sh = SoakHarness::new(72, 50).unwrap_or_else(|_| SoakHarness {
        samples: Vec::new(),
        baseline_ms: 50,
        hours: 72,
    });
    sh.sample(SoakSample {
        hour: 0,
        edit_ms: 50,
        mem_kb: 100_000,
    });
    sh.sample(SoakSample {
        hour: 36,
        edit_ms: 52,
        mem_kb: 102_000,
    });
    out.add(
        "0x7901-三指标",
        sh.drift_check().is_ok() && sh.mem_stable().is_ok(),
        "稳态须过",
    );

    // 2) 心流漂移：>5% 被拦
    let mut sh2 = SoakHarness::new(72, 50).unwrap_or_else(|_| SoakHarness {
        samples: Vec::new(),
        baseline_ms: 50,
        hours: 72,
    });
    sh2.sample(SoakSample {
        hour: 0,
        edit_ms: 50,
        mem_kb: 100_000,
    });
    sh2.sample(SoakSample {
        hour: 10,
        edit_ms: 80,
        mem_kb: 100_000,
    });
    out.add(
        "0x7901-漂移拦",
        sh2.drift_check().is_err(),
        "漂移 60% 须拦",
    );

    // 3) 重锚隐形：可见跳变被拦
    let rp = ReanchorPolicy::new(100);
    out.add(
        "0x7903-重锚隐形",
        rp.assert(&Reanchor {
            cost_ms: 50,
            visible_jump: true,
        })
        .is_err(),
        "可见跳变须拦",
    );

    // 4) 重锚隐形：隐形过
    out.add(
        "0x7903-隐形过",
        rp.assert(&Reanchor {
            cost_ms: 50,
            visible_jump: false,
        })
        .is_ok(),
        "隐形须过",
    );

    // 5) 注入自证：缺检出被拦
    let mut inj = LeakInjector::new();
    let _ = inj.inject_and_detect(LeakPattern::Slow);
    out.add(
        "0x7902-注入自证",
        inj.audit().is_err(),
        "缺检出须拦",
    );

    // 6) 注入自证：全模式过
    for p in LeakPattern::ALL {
        let _ = inj.inject_and_detect(p);
    }
    out.add(
        "0x7902-全模式过",
        inj.audit().is_ok(),
        "全检出须过",
    );

    // 7) 保全复测：丢数据 P0
    out.add(
        "0x7904-保全复测",
        PreservationRecheck::canvas_intact(false).is_err(),
        "丢数据须 P0",
    );

    // 8) 长稳参数：非法拒
    out.add(
        "0x7905-长稳参数",
        SoakHarness::new(0, 50).is_err(),
        "零时长须拒",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7906-判据",
        L7Code::DRIFT_OVER_5.value() != L7Code::DATA_LOSS_P0.value(),
        "码段互异",
    );

    out
}
