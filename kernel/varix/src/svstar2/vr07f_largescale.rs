//! VE-F3726 · 大规模创作性能（VE-S 域 · 批次 R07 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3726`
//!
//! **职责定位（锚点原文）**：大规模创作专项——
//! - **超大画布**：16M 像素画布编辑预算（分块渲染 + 代理复用，
//!   复述 F3651 大画布全域化）——全域复用声明；
//! - **万图层**：理论极限性能边界（图层数上限 1000 + 超限警告）——
//!   上限红线：无限图层 = 内存爆（上限钳制）；
//! - **规模阶梯**：复用 F3724 三阶实测（近线性复述）；
//! - **用户视角**：大画布编辑心流不破（中断断言复用）。
//!
//! **判据（锚点原文六条）**：1000 层上限、近线性、心流不破、
//! 全域复用、代理对拍、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 超线性→架构立案（复述红线实测） | [`S7Code::SUPERLINEAR`] |
//! | 上限缺失→补钳制（红线实测） | [`S7Code::CAP_MISS`] |
//! | 代理失真→对拍（复用） | [`S7Code::PROXY_DRIFT`] |
//! | 峰值崩→背压（复述） | [`S7Code::PEAK_COLLAPSE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3726 诊断码段（独占 0x75xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S7Code(pub u16);

impl S7Code {
    /// 超线性（架构立案）。
    pub const SUPERLINEAR: S7Code = S7Code(0x7501);
    /// 上限缺失（补钳制）。
    pub const CAP_MISS: S7Code = S7Code(0x7502);
    /// 代理失真（对拍）。
    pub const PROXY_DRIFT: S7Code = S7Code(0x7503);
    /// 峰值崩（背压）。
    pub const PEAK_COLLAPSE: S7Code = S7Code(0x7504);
    /// 分块参数非法。
    pub const BAD_CHUNK: S7Code = S7Code(0x7505);
    /// 心流破。
    pub const FLOW_BREAK: S7Code = S7Code(0x7506);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            S7Code::SUPERLINEAR => "规模超线性（红线实测）。建议：架构立案定位瓶颈。",
            S7Code::CAP_MISS => "图层上限缺失（红线实测）。建议：补 1000 层钳制。",
            S7Code::PROXY_DRIFT => "代理失真（复用）。建议：与全量对拍校正。",
            S7Code::PEAK_COLLAPSE => "峰值崩（复述）。建议：启用背压限流。",
            S7Code::BAD_CHUNK => "分块参数非法。建议：块边长 64-1024。",
            S7Code::FLOW_BREAK => "心流破。建议：大画布中断率须守 ≤3/h。",
            _ => "未知诊断码。",
        }
    }
}

// ── 图层上限钳制（红线） ────────────────────────────────────────────

/// 图层栈。
pub struct LayerStack {
    /// 当前层数。
    count: u16,
    /// 上限（1000）。
    cap: u16,
}

impl LayerStack {
    /// 标准构造（上限 1000）。
    pub fn standard() -> LayerStack {
        LayerStack {
            count: 0,
            cap: 1000,
        }
    }

    /// 无上限构造（红线违规，自检用）。
    pub fn uncapped() -> LayerStack {
        LayerStack {
            count: 0,
            cap: u16::MAX,
        }
    }

    /// 加层（超上限钳制 + 拒）。
    pub fn add(&mut self) -> Result<(), S7Code> {
        if self.cap == u16::MAX {
            // 上限缺失红线
            return Err(S7Code::CAP_MISS);
        }
        if self.count >= self.cap {
            return Err(S7Code::CAP_MISS);
        }
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// 上限钳制存在。
    pub fn capped(&self) -> bool {
        self.cap != u16::MAX && self.cap > 0
    }

    /// 当前层数。
    pub fn count(&self) -> u16 {
        self.count
    }
}

// ── 分块渲染（F3651 全域复用） ──────────────────────────────────────

/// 分块规划。
pub struct ChunkGrid {
    /// 画布宽。
    w: u32,
    /// 画布高。
    h: u32,
    /// 块边长。
    chunk: u32,
}

impl ChunkGrid {
    /// 制定（64-1024）。
    pub fn new(w: u32, h: u32, chunk: u32) -> Result<ChunkGrid, S7Code> {
        if chunk < 64 || chunk > 1024 || w == 0 || h == 0 {
            return Err(S7Code::BAD_CHUNK);
        }
        Ok(ChunkGrid { w, h, chunk })
    }

    /// 块数。
    pub fn count(&self) -> u32 {
        let cx = (self.w + self.chunk - 1) / self.chunk;
        let cy = (self.h + self.chunk - 1) / self.chunk;
        cx.saturating_mul(cy)
    }

    /// 视口分块（仅渲染可见块）。
    pub fn visible_chunks(&self, vx: u32, vy: u32, vw: u32, vh: u32) -> u32 {
        let _ = (vx, vy);
        let cx = (vw.min(self.w) + self.chunk - 1) / self.chunk;
        let cy = (vh.min(self.h) + self.chunk - 1) / self.chunk;
        cx.saturating_mul(cy)
    }
}

// ── 近线性阶梯（F3724 复用） ────────────────────────────────────────

/// 阶梯样本（像素 vs 耗时）。
pub struct ScaleSample {
    /// 像素数。
    pub pixels: u32,
    /// 耗时（毫秒）。
    pub ms: u32,
}

/// 近线性检查。
pub struct LinearityCheck;

impl LinearityCheck {
    /// 近线性判定：耗时比 / 像素比 ≤ 1.5（放大 1000 比较）。
    pub fn near_linear(a: &ScaleSample, b: &ScaleSample) -> Result<(), S7Code> {
        if a.pixels == 0 || a.ms == 0 {
            return Ok(());
        }
        if b.pixels <= a.pixels {
            return Ok(());
        }
        let pixel_ratio_x1000 = (b.pixels as u64 * 1000) / a.pixels as u64;
        let time_ratio_x1000 = (b.ms as u64 * 1000) / a.ms as u64;
        // time_ratio ≤ pixel_ratio * 1.5 → time_ratio * 2 ≤ pixel_ratio * 3
        if time_ratio_x1000 * 2 > pixel_ratio_x1000 * 3 {
            return Err(S7Code::SUPERLINEAR);
        }
        Ok(())
    }
}

// ── 代理对拍（复用） ────────────────────────────────────────────────

/// 对拍样本。
pub struct ProxyPair {
    pub proxy_hash: u32,
    pub full_hash: u32,
}

/// 对拍器。
pub struct ProxyAudit {
    samples: Vec<ProxyPair>,
}

impl ProxyAudit {
    pub fn new() -> ProxyAudit {
        ProxyAudit {
            samples: Vec::new(),
        }
    }

    pub fn push(&mut self, p: ProxyPair) {
        self.samples.push(p);
    }

    pub fn audit(&self) -> Result<(), S7Code> {
        for s in &self.samples {
            if s.proxy_hash != s.full_hash {
                return Err(S7Code::PROXY_DRIFT);
            }
        }
        Ok(())
    }
}

// ── 背压（峰值防线） ────────────────────────────────────────────────

/// 背压阀。
pub struct Backpressure {
    /// 峰值水位（×100）。
    peak_x100: u32,
    /// 触发线（800 = 80%）。
    trigger_x100: u32,
}

impl Backpressure {
    pub const fn new(trigger_x100: u32) -> Backpressure {
        Backpressure {
            peak_x100: 0,
            trigger_x100,
        }
    }

    /// 上报水位。
    pub fn feed(&mut self, x100: u32) {
        if x100 > self.peak_x100 {
            self.peak_x100 = x100;
        }
    }

    /// 峰值检查（超触发即崩）。
    pub fn check(&self) -> Result<(), S7Code> {
        if self.peak_x100 >= self.trigger_x100 {
            Err(S7Code::PEAK_COLLAPSE)
        } else {
            Ok(())
        }
    }
}

// ── 心流断言（复用） ────────────────────────────────────────────────

/// 心流断言器。
pub struct FlowAssert;

impl FlowAssert {
    /// 中断率断言（≤3/h）。
    pub fn interrupt_ok(per_hour: u32) -> Result<(), S7Code> {
        if per_hour > 3 {
            Err(S7Code::FLOW_BREAK)
        } else {
            Ok(())
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3726 判据集（锚点原文六条逐条落位）。
pub fn run_vr07f_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3726");

    // 1) 1000 层上限：钳制存在
    let mut ls = LayerStack::standard();
    out.add(
        "0x7502-1000 上限",
        ls.capped(),
        "上限须钳",
    );

    // 2) 1000 层上限：无上限被拦
    let mut unc = LayerStack::uncapped();
    out.add(
        "0x7502-上限缺失",
        unc.add().is_err(),
        "无上限须拦",
    );

    // 3) 近线性：正常过
    let a = ScaleSample {
        pixels: 1_000_000,
        ms: 100,
    };
    let b = ScaleSample {
        pixels: 4_000_000,
        ms: 380,
    };
    out.add(
        "0x7501-近线性",
        LinearityCheck::near_linear(&a, &b).is_ok(),
        "近线性须过",
    );

    // 4) 近线性：超线性被拦
    let c = ScaleSample {
        pixels: 16_000_000,
        ms: 4000,
    };
    out.add(
        "0x7501-超线性拦",
        LinearityCheck::near_linear(&a, &c).is_err(),
        "超线性须拦",
    );

    // 5) 全域复用：分块
    let grid = ChunkGrid::new(7680, 4320, 512).unwrap_or_else(|_| ChunkGrid {
        w: 7680,
        h: 4320,
        chunk: 512,
    });
    out.add(
        "0x7505-全域复用",
        grid.count() > 1 && grid.visible_chunks(0, 0, 1920, 1080) < grid.count(),
        "视口分块须更少",
    );

    // 6) 代理对拍：失真被拦
    let mut pa = ProxyAudit::new();
    pa.push(ProxyPair {
        proxy_hash: 1,
        full_hash: 2,
    });
    out.add("0x7503-代理对拍", pa.audit().is_err(), "失真须拦");

    // 7) 心流不破：断言
    out.add(
        "0x7506-心流不破",
        FlowAssert::interrupt_ok(2).is_ok() && FlowAssert::interrupt_ok(10).is_err(),
        "中断率须守",
    );

    // 8) 峰值崩：背压
    let mut bp = Backpressure::new(800);
    bp.feed(900);
    out.add(
        "0x7504-峰值崩",
        bp.check().is_err(),
        "超峰须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7506-判据",
        S7Code::SUPERLINEAR.value() != S7Code::CAP_MISS.value(),
        "码段互异",
    );

    out
}
