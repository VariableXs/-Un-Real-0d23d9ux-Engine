//! TRINITY-500 · AI-05 2D 合成渲染栈域（F101~F125，W2）与 AI-06 字体文本域载体
//!
//! * [`surface`] — 帧缓冲/模式表/双缓冲/光栅化/色彩/图层/帧预算/多显示器
//! * [`blur`] — F105 模糊与玻璃材质
//! * [`capture`] — F120 截图/录屏采集通道
//! * [`text`] — AI-06 字体与文本（F126~F150 主体）
//! * [`ime`] — AI-06 IME 候选窗与拼音输入
//!
//! 本文件承载 F116（渲染自检）、F121（渲染诊断器）、F124（崩溃隔离）与 F125（收口）。

pub mod blur;
pub mod capture;
pub mod ime;
pub mod surface;
pub mod text;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F116 渲染自检
// ---------------------------------------------------------------------------

/// 一次渲染管线的自检结果：所有子项都必须 PASS 才允许进入桌面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderProbe {
    Ok,
    NoFramebuffer,
    NoMode,
    OutOfMemory,
}

impl RenderProbe {
    pub fn text(self) -> &'static str {
        match self {
            RenderProbe::Ok => "ok",
            RenderProbe::NoFramebuffer => "no linear framebuffer",
            RenderProbe::NoMode => "no usable video mode",
            RenderProbe::OutOfMemory => "render buffers exceed budget",
        }
    }
}

/// 进入桌面前的渲染自检：模式、帧缓冲、内存预算三关。
pub fn probe_render(width: u32, height: u32, has_fb: bool, buffers: u32) -> RenderProbe {
    if !has_fb {
        return RenderProbe::NoFramebuffer;
    }
    if surface::pick_mode(width, height).width == 0 {
        return RenderProbe::NoMode;
    }
    if !surface::render_memory_within_budget(width, height, buffers) {
        return RenderProbe::OutOfMemory;
    }
    RenderProbe::Ok
}

// ---------------------------------------------------------------------------
// F121 渲染诊断器
// ---------------------------------------------------------------------------

pub const MAX_RENDER_ISSUES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderIssue {
    FrameOverBudget,
    DirtyOverflow,
    LayerOverflow,
    MonitorOverlap,
    MemoryOverBudget,
    SoftwareFallback,
}

impl RenderIssue {
    pub fn text(self) -> &'static str {
        match self {
            RenderIssue::FrameOverBudget => "frame time over 16.6ms budget",
            RenderIssue::DirtyOverflow => "dirty rect list overflowed, full redraw",
            RenderIssue::LayerOverflow => "layer count exceeds compositor capacity",
            RenderIssue::MonitorOverlap => "monitor rectangles overlap",
            RenderIssue::MemoryOverBudget => "surface memory over budget",
            RenderIssue::SoftwareFallback => "running on software compositing",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderDiag {
    issues: [Option<RenderIssue>; MAX_RENDER_ISSUES],
    count: usize,
}

impl RenderDiag {
    pub const fn new() -> RenderDiag {
        RenderDiag { issues: [None; MAX_RENDER_ISSUES], count: 0 }
    }

    fn push(&mut self, i: RenderIssue) {
        if self.count >= MAX_RENDER_ISSUES || (0..self.count).any(|k| self.issues[k] == Some(i)) {
            return;
        }
        self.issues[self.count] = Some(i);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn has(&self, i: RenderIssue) -> bool {
        (0..self.count).any(|k| self.issues[k] == Some(i))
    }

    pub fn inspect(
        &mut self,
        meter: &surface::FrameMeter,
        dirty: &surface::DirtyList,
        layers: usize,
        monitors: &surface::MonitorTable,
        width: u32,
        height: u32,
        gpu: surface::GpuPath,
    ) {
        if meter.over_budget() {
            self.push(RenderIssue::FrameOverBudget);
        }
        if dirty.len() == 1 && dirty.get(0).map(|r| r.area() > 1 << 24).unwrap_or(false) {
            self.push(RenderIssue::DirtyOverflow);
        }
        if layers > surface::MAX_LAYERS {
            self.push(RenderIssue::LayerOverflow);
        }
        if overlaps(monitors) {
            self.push(RenderIssue::MonitorOverlap);
        }
        if !surface::render_memory_within_budget(width, height, 3) {
            self.push(RenderIssue::MemoryOverBudget);
        }
        if gpu == surface::GpuPath::Software {
            self.push(RenderIssue::SoftwareFallback);
        }
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.issues[i] {
                for &b in s.text().as_bytes() {
                    if n < out.len() {
                        out[n] = b;
                        n += 1;
                    }
                }
                if n < out.len() {
                    out[n] = b'\n';
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for RenderDiag {
    fn default() -> Self {
        RenderDiag::new()
    }
}

fn overlaps(t: &surface::MonitorTable) -> bool {
    for i in 0..t.len() {
        for j in (i + 1)..t.len() {
            if let (Some(a), Some(b)) = (t.get(i), t.get(j)) {
                let ra = surface::Rect::new(a.x, a.y, a.mode.width as i32, a.mode.height as i32);
                let rb = surface::Rect::new(b.x, b.y, b.mode.width as i32, b.mode.height as i32);
                if !ra.intersect(&rb).is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// F124 渲染崩溃隔离 — 不拖垮内核
// ---------------------------------------------------------------------------

/// 渲染段守卫：渲染代码在 guard 内执行，出错只杀渲染、不杀内核。
#[derive(Clone, Copy, Debug)]
pub struct RenderGuard {
    pub faulted: bool,
    pub faults: u32,
    pub last_reason: &'static str,
    depth: u8,
}

impl RenderGuard {
    pub const fn new() -> RenderGuard {
        RenderGuard { faulted: false, faults: 0, last_reason: "", depth: 0 }
    }

    pub fn enter(&mut self) -> bool {
        if self.faulted {
            return false;
        }
        self.depth = self.depth.saturating_add(1);
        true
    }

    /// 渲染段内出错时调用：记录原因并立即终止本段渲染。
    pub fn fault(&mut self, reason: &'static str) {
        self.faulted = true;
        self.last_reason = reason;
        self.faults = self.faults.saturating_add(1);
        self.depth = 0;
    }

    pub fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// 恢复到「可再次尝试渲染」的状态（例如重建 surface 之后）。
    pub fn recover(&mut self) {
        self.faulted = false;
        self.depth = 0;
    }

    /// 连续多次故障后不再重试，直接常驻软降级（避免死循环重启渲染）。
    pub fn should_retry(&self) -> bool {
        self.faults < 3
    }
}

impl Default for RenderGuard {
    fn default() -> Self {
        RenderGuard::new()
    }
}

// ---------------------------------------------------------------------------
// F116 / F125 渲染域自检与收口
// ---------------------------------------------------------------------------

/// AI-05 域自检：F101~F125 逐项登记。
pub fn run_gfx_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx");

    set.add(
        "F101 framebuffer modes",
        surface::MODE_TABLE.len() == 6 && surface::pick_mode(1920, 1080).width == 1920,
        "mode table",
    );

    let mut sc = surface::SwapChain::new(surface::SwapKind::Triple);
    sc.present();
    set.add("F102 double/triple buffer", sc.kind.buffer_count() == 3 && sc.frames == 1, "swap chain");

    let mut dst = [0u32; 16];
    let src = [0x22u32; 16];
    set.add(
        "F103 software rasteriser",
        surface::blit(&mut dst, 4, &src, 4, &surface::Rect::new(0, 0, 4, 4), &surface::Rect::new(0, 0, 4, 4)) == 16,
        "blit",
    );

    set.add(
        "F104 rounded rect + gradient",
        surface::clamp_radius(99, 20, 20) == 10
            && surface::inside_round_rect(10, 10, 20, 20, 10)
            && surface::gradient_at(0, 0x0000_00FF, 1000) == 0xFF00_00FF,
        "geometry",
    );

    let k = blur::BlurKernel::new(2, 1);
    set.add("F105 blur kernel", k.window() == 5 && blur::blur_worth_it(256, &k), "separable box blur");

    set.add(
        "F106 alpha compositing",
        surface::blend(0x0000_00FF, 0x0000_0000, 255) == 0xFF00_00FF
            && surface::blend(0x0000_00FF, 0x0000_0000, 0) == 0xFF00_0000,
        "src-over",
    );

    let mut d = surface::DirtyList::new();
    d.add(surface::Rect::new(0, 0, 4, 4));
    d.add(surface::Rect::new(2, 2, 4, 4));
    set.add("F107 dirty rects", d.len() == 1, "merge");

    let white = surface::oklch_to_argb(surface::Oklch { l: 1.0, c: 0.0, h: 0.0 });
    set.add(
        "F108 oklch tokens",
        (white >> 16) & 0xFF > 250 && surface::contrast_ratio(0xFFFF_FFFF, 0xFF00_0000) > 15.0,
        "colour pipeline",
    );

    let spring = surface::Spring::default_spring();
    let (p, v) = spring.step(0.0, 0.0, 1.0, 16.0);
    set.add("F109 spring integrator", p > 0.0 && v > 0.0, "semi-implicit euler");

    let layers = [surface::Layer::new(3, surface::Rect::new(0, 0, 1, 1)), surface::Layer::new(1, surface::Rect::new(0, 0, 1, 1))];
    let mut order = [0usize; 4];
    set.add(
        "F110 z-order",
        surface::compose_order(&layers, &mut order) == 2 && order[0] == 1,
        "bottom to top",
    );

    set.add(
        "F111 shadow and highlight",
        surface::shadow_alpha(0, 4) == 160 && surface::shadow_alpha(4, 4) == 0,
        "falloff",
    );

    let mut pf = surface::ParticleField::new();
    pf.spawn(surface::Particle { x: 0, y: 0, vx: 1, vy: 1, life: 2, color: 0xFF });
    pf.step(&surface::Rect::new(0, 0, 4, 4));
    set.add("F112 particle pipeline", pf.len() == 1, "fixed capacity field");

    let mut meter = surface::FrameMeter::new();
    meter.push(16_000);
    set.add(
        "F113 vsync + 60fps budget",
        surface::FRAME_BUDGET_US == 16_666 && !meter.over_budget() && meter.fps() > 55,
        "frame budget",
    );

    meter.push(20_000);
    set.add("F114 frame timing monitor", meter.over_budget(), "over-budget detection");

    set.add(
        "F115 reduce-motion",
        surface::MotionPref::Reduced.duration_ms(200) == 0
            && !surface::MotionPref::Reduced.particles_allowed(),
        "motion honoured",
    );

    set.add(
        "F116 render self-check",
        probe_render(1920, 1080, true, 3) == RenderProbe::Ok
            && probe_render(1920, 1080, false, 3) == RenderProbe::NoFramebuffer,
        "pre-desktop probe",
    );

    set.add(
        "F117 gpu detection",
        surface::probe_gpu(true, true) == surface::GpuPath::Direct
            && surface::probe_gpu(false, true) == surface::GpuPath::Software,
        "honest fallback",
    );

    set.add(
        "F118 integer permille scaling",
        surface::scale_permille(200, 1500) == 300 && surface::dpi_step(1600) == 1500,
        "no half pixels",
    );

    let mut mt = surface::MonitorTable::new();
    mt.add(surface::Monitor { id: 0, x: 0, y: 0, mode: surface::MODE_TABLE[3], primary: true });
    set.add(
        "F119 multi-monitor",
        mt.len() == 1 && mt.primary().is_some() && mt.hit(100, 100).is_some(),
        "hit routing",
    );

    let shot = capture::CaptureRequest {
        rect: surface::Rect::new(0, 0, 64, 64),
        format: capture::CaptureFormat::RawPixels,
        fps: 0,
        seconds: 0,
    };
    set.add(
        "F120 capture channel",
        shot.is_screenshot() && capture::within_budget(&shot) && shot.total_frames() == 1,
        "screenshot",
    );

    let mut diag = RenderDiag::new();
    let clean_meter = surface::FrameMeter::new();
    diag.inspect(&clean_meter, &d, 2, &mt, 1920, 1080, surface::GpuPath::Direct);
    set.add("F121 render diagnostics", diag.len() == 0, "clean pipeline");

    set.add(
        "F122 pixel format abstraction",
        surface::pack(surface::PixelFormat::Bgr32, 0xFF11_2233) == 0xFF33_2211,
        "rgb/bgr swizzle",
    );

    set.add(
        "F123 render memory budget",
        surface::render_memory_within_budget(1920, 1080, 3)
            && surface::buffer_bytes(64, 64) >= 64 * 64 * 4,
        "64MiB cap",
    );

    let mut guard = RenderGuard::new();
    guard.enter();
    guard.fault("bad surface");
    set.add(
        "F124 crash isolation",
        guard.faulted && !guard.enter() && guard.should_retry() && {
            guard.recover();
            guard.enter()
        },
        "renderer dies, kernel lives",
    );

    set.add("F125 render domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f116_probe_paths() {
        assert_eq!(probe_render(1920, 1080, false, 3), RenderProbe::NoFramebuffer);
        assert_eq!(probe_render(7680, 4320, true, 8), RenderProbe::OutOfMemory);
    }

    #[test]
    fn f121_diag_detects_software_fallback() {
        let mut d = RenderDiag::new();
        let mut m = surface::MonitorTable::new();
        m.add(surface::Monitor { id: 0, x: 0, y: 0, mode: surface::MODE_TABLE[0], primary: true });
        let meter = surface::FrameMeter::new();
        let dirty = surface::DirtyList::new();
        d.inspect(&meter, &dirty, 1, &m, 1024, 768, surface::GpuPath::Software);
        assert!(d.has(RenderIssue::SoftwareFallback));
        let mut out = [0u8; 128];
        assert!(d.render(&mut out) > 0);
    }

    #[test]
    fn f124_guard_stops_after_three_faults() {
        let mut g = RenderGuard::new();
        for _ in 0..3 {
            g.recover();
            g.enter();
            g.fault("x");
        }
        assert!(!g.should_retry());
        assert_eq!(g.faults, 3);
    }

    #[test]
    fn f125_domain_self_test_is_green() {
        let set = run_gfx_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed(), "gfx domain self-test must pass");
    }
}
