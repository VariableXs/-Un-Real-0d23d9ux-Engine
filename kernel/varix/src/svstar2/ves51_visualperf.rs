//! VE-F3651 · 视觉创作性能（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3651`
//!
//! **职责定位（锚点原文）**：工坊性能（F3610 制度复用）——
//! - **三线复述工坊版**：编辑响应 50ms / 预览 100ms / 导出 5s；
//! - **大画布**：8K 画布编辑（分块渲染 + 代理，复用代理模式）；
//! - **GPU 加速**：画布 GPU 加速 + CPU 回退（回退告知红线：
//!   GPU 不可用静默 CPU 慢速 = 卡顿谜团 → 回退告知）；
//! - **剖析**：工坊剖析（复用制度）。
//!
//! **判据（锚点原文六条）**：三线复述、回退告知、代理复用、
//! 大画布、剖析复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 三线超→立案（复述实测） | [`P51Code::LINE_EXCEEDED`] |
//! | 回退静默→P1（红线实测） | [`P51Code::FALLBACK_SILENT`] |
//! | 代理失真→对拍（复用） | [`P51Code::PROXY_DRIFT`] |
//! | 大画布卡→分块（复用） | [`P51Code::CANVAS_STALL`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3651 诊断码段（独占 0x63xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P51Code(pub u16);

impl P51Code {
    /// 三线之一超限（立案）。
    pub const LINE_EXCEEDED: P51Code = P51Code(0x6301);
    /// GPU 回退静默（P1 红线实测）。
    pub const FALLBACK_SILENT: P51Code = P51Code(0x6302);
    /// 代理失真。
    pub const PROXY_DRIFT: P51Code = P51Code(0x6303);
    /// 大画布卡顿（须分块）。
    pub const CANVAS_STALL: P51Code = P51Code(0x6304);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            P51Code::LINE_EXCEEDED => "性能线超限（立案）。建议：按 50ms/100ms/5s 三线排查。",
            P51Code::FALLBACK_SILENT => "GPU 回退静默（红线实测 P1）。建议：回退必告知。",
            P51Code::PROXY_DRIFT => "代理渲染失真。建议：与全量渲染对拍。",
            P51Code::CANVAS_STALL => "大画布卡顿。建议：分块渲染+代理。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三线（F3610 复用工坊版） ────────────────────────────────────────

/// 性能线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    /// 编辑响应 50ms。
    Edit,
    /// 预览联动 100ms。
    Preview,
    /// 导出 5s。
    Export,
}

impl Line {
    pub fn budget_ms(self) -> u32 {
        match self {
            Line::Edit => 50,
            Line::Preview => 100,
            Line::Export => 5_000,
        }
    }
}

/// 三线执法器。
pub struct ThreeLines {
    violations: Vec<Line>,
}

impl ThreeLines {
    pub fn new() -> Self {
        Self { violations: Vec::new() }
    }

    /// 实测断言：超线即记录并报码。
    pub fn measure(&mut self, line: Line, actual_ms: u32) -> Result<u32, P51Code> {
        if actual_ms > line.budget_ms() {
            self.violations.push(line);
            return Err(P51Code::LINE_EXCEEDED);
        }
        Ok(actual_ms)
    }

    pub fn violations(&self) -> usize {
        self.violations.len()
    }

    pub fn green(&self) -> bool {
        self.violations.is_empty()
    }
}

// ── GPU 回退告知 ────────────────────────────────────────────────────

/// 渲染后端。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    /// GPU 加速。
    Gpu,
    /// CPU 回退。
    Cpu,
}

/// 回退告知协议（红线：不许静默）。
pub struct GpuFallback {
    backend: Backend,
    /// 是否已告知。
    notified: bool,
    silent_hits: u32,
}

impl GpuFallback {
    pub fn new() -> Self {
        Self { backend: Backend::Gpu, notified: true, silent_hits: 0 }
    }

    /// 切后端；落到 CPU 必须告知（notified=true 传入）。
    pub fn switch(&mut self, backend: Backend, notified: bool) -> Result<bool, P51Code> {
        self.backend = backend;
        self.notified = notified;
        if backend == Backend::Cpu && !notified {
            self.silent_hits += 1;
            return Err(P51Code::FALLBACK_SILENT);
        }
        Ok(true)
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn silent_hits(&self) -> u32 {
        self.silent_hits
    }

    /// 回退告知总评。
    pub fn informed(&self) -> bool {
        self.backend == Backend::Gpu || self.notified
    }
}

// ── 分块代理（8K 大画布） ───────────────────────────────────────────

/// 画布分块代理。
pub struct ChunkProxy {
    /// 画布边长（像素）。
    extent: u32,
    /// 分块边长。
    chunk: u32,
    /// 全量渲染指纹（对拍基准）。
    full_fp: u32,
    stalls: u32,
}

impl ChunkProxy {
    /// 新建；8K 画布（extent>=7680）必须分块。
    pub fn new(extent: u32, chunk: u32) -> Result<Self, P51Code> {
        if extent >= 7_680 && chunk >= extent {
            return Err(P51Code::CANVAS_STALL);
        }
        Ok(Self { extent, chunk: chunk.max(1), full_fp: 0, stalls: 0 })
    }

    /// 分块数（O(画布)）。
    pub fn chunk_count(&self) -> u32 {
        let n = (self.extent + self.chunk - 1) / self.chunk;
        n * n
    }

    /// 代理渲染对拍：代理指纹须与全量一致。
    pub fn render_proxy(&mut self, proxy_fp: u32) -> Result<bool, P51Code> {
        if proxy_fp != self.full_fp {
            return Err(P51Code::PROXY_DRIFT);
        }
        Ok(true)
    }

    /// 钉全量基准。
    pub fn set_full_fp(&mut self, fp: u32) {
        self.full_fp = fp;
    }

    /// 卡顿登记（大画布卡 → 分块）。
    pub fn stall(&mut self) -> u32 {
        self.stalls += 1;
        self.stalls
    }

    pub fn stalls(&self) -> u32 {
        self.stalls
    }

    /// 大画布覆盖（8K 支持位）。
    pub fn supports_8k(&self) -> bool {
        self.extent >= 7_680 && self.chunk < self.extent
    }
}

// ── 剖析（复用制度） ────────────────────────────────────────────────

/// 工坊剖析采样。
pub struct Profiler {
    samples: Vec<u32>,
    on: bool,
}

impl Profiler {
    pub fn new() -> Self {
        Self { samples: Vec::new(), on: false }
    }

    pub fn open(&mut self, on: bool) {
        self.on = on;
    }

    /// 采样（打开才采样，零常态）。
    pub fn sample(&mut self, us: u32) -> Option<u32> {
        if !self.on {
            return None;
        }
        self.samples.push(us);
        Some(us)
    }

    pub fn p99_us(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = (sorted.len() * 99) / 100;
        sorted[idx.min(sorted.len() - 1)]
    }
}

// ── 性能面板 ────────────────────────────────────────────────────────

/// 视觉创作性能面板。
pub struct VisualPerf {
    lines: ThreeLines,
    gpu: GpuFallback,
    profiler: Profiler,
    last_code: P51Code,
    has_error: bool,
}

impl VisualPerf {
    pub fn new() -> Self {
        Self {
            lines: ThreeLines::new(),
            gpu: GpuFallback::new(),
            profiler: Profiler::new(),
            last_code: P51Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> P51Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: P51Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 三线实测。
    pub fn measure(&mut self, line: Line, actual_ms: u32) -> Result<u32, P51Code> {
        self.lines.measure(line, actual_ms).map_err(|c| {
            self.fail(c);
            c
        })
    }

    /// GPU 回退（含告知位）。
    pub fn fallback(&mut self, backend: Backend, notified: bool) -> Result<bool, P51Code> {
        self.gpu.switch(backend, notified).map_err(|c| {
            self.fail(c);
            c
        })
    }

    /// 剖析采样。
    pub fn profile(&mut self, us: u32) -> Option<u32> {
        self.profiler.open(true);
        self.profiler.sample(us)
    }

    pub fn violations(&self) -> usize {
        self.lines.violations()
    }

    pub fn silent_fallbacks(&self) -> u32 {
        self.gpu.silent_hits()
    }

    /// 三线复述 + 回退告知总评。
    pub fn perf_green(&self) -> bool {
        self.lines.green() && self.gpu.informed()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves51_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves51_visualperf");
    {
        let p = VisualPerf::new();
        chk(&mut set, "C3651-01", p.violations() == 0 && p.silent_fallbacks() == 0, "err");
    }
    {
        let mut t = ThreeLines::new();
        let r = t.measure(Line::Edit, 30);
        chk(&mut set, "C3651-02", r == Ok(30), "err");
    }
    {
        let mut t = ThreeLines::new();
        let r = t.measure(Line::Edit, 80);
        chk(&mut set, "C3651-03", r.err() == Some(P51Code::LINE_EXCEEDED) && t.violations() == 1, "err");
    }
    {
        let mut t = ThreeLines::new();
        let r = t.measure(Line::Preview, 150);
        chk(&mut set, "C3651-04", r.err() == Some(P51Code::LINE_EXCEEDED), "err");
    }
    {
        let mut t = ThreeLines::new();
        let r = t.measure(Line::Export, 5_000);
        chk(&mut set, "C3651-05", r == Ok(5_000), "err");
    }
    {
        let mut g = GpuFallback::new();
        let r = g.switch(Backend::Cpu, true);
        chk(&mut set, "C3651-06", r == Ok(true) && g.informed(), "err");
    }
    {
        let mut g = GpuFallback::new();
        let r = g.switch(Backend::Cpu, false);
        chk(&mut set, "C3651-07", r.err() == Some(P51Code::FALLBACK_SILENT) && g.silent_hits() == 1, "err");
    }
    {
        let c = ChunkProxy::new(7_680, 256);
        chk(&mut set, "C3651-08", c.is_ok() && c.unwrap().supports_8k(), "err");
    }
    {
        let c = ChunkProxy::new(7_680, 8_192);
        chk(&mut set, "C3651-09", c.err() == Some(P51Code::CANVAS_STALL), "err");
    }
    {
        let mut c = ChunkProxy::new(7_680, 256).unwrap();
        c.set_full_fp(0xAA);
        let r = c.render_proxy(0xAA);
        chk(&mut set, "C3651-10", r == Ok(true), "err");
    }
    {
        let mut c = ChunkProxy::new(7_680, 256).unwrap();
        c.set_full_fp(0xAA);
        let r = c.render_proxy(0xBB);
        chk(&mut set, "C3651-11", r.err() == Some(P51Code::PROXY_DRIFT), "err");
    }
    {
        let mut p = VisualPerf::new();
        p.measure(Line::Edit, 10).unwrap();
        p.measure(Line::Preview, 20).unwrap();
        p.fallback(Backend::Gpu, true).unwrap();
        p.profile(100);
        chk(&mut set, "C3651-12", p.perf_green(), "err");
    }
    set
}
