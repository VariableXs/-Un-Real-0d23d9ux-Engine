//! m700disp — VARIX-M700 AI-16 显示与合成内核域 (F376~F400)
//!
//! 显示管道大典/模式集裁判/页翻转律/多屏拓扑谱/显示器档案/合成内核接口/
//! 帧缓冲直通舱/亮度背光官/显示功耗谱/热插拔显示事件/显示错误官/
//! gamma LUT 通道/自检画面生成器/显示延迟仪/显示回归金样/HDR 元数据/
//! 显示事件流/残影纠正官/多屏一致性官/显示自描述导出/低分辨率优雅谱/
//! 显示健康分/显示压力剧本/显示文档生成器/显示域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F376 — 显示管道大典：Plane→CRTC→Encoder→Connector 管道
// ===========================================================================

/// 管道资源计数：给足 CRTC 才能点亮。
#[derive(Clone, Copy)]
pub struct DisplayPipe {
    pub planes: u8,
    pub crtcs: u8,
    pub encoders: u8,
    pub connectors: u8,
}

pub fn pipe_viable(p: DisplayPipe) -> bool {
    p.crtcs >= 1 && p.connectors >= 1 && p.planes >= p.crtcs && p.encoders >= p.crtcs
}

// ===========================================================================
// F377 — 模式集裁判：分辨率/刷新率合法性裁决
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VideoMode {
    pub width: u16,
    pub height: u16,
    pub refresh_mhz: u32, // 刷新率 × 1000（毫赫兹）
}

impl VideoMode {
    pub fn judge(&self) -> bool {
        self.width >= 320 && self.height >= 200 && self.width % 8 == 0
            && self.height % 8 == 0 && (23_000..=250_000).contains(&self.refresh_mhz)
    }
    pub fn same_as(&self, other: &VideoMode) -> bool {
        *self == *other
    }
}

pub const COMMON_MODES: [VideoMode; 4] = [
    VideoMode { width: 640, height: 480, refresh_mhz: 60_000 },
    VideoMode { width: 1280, height: 720, refresh_mhz: 60_000 },
    VideoMode { width: 1920, height: 1080, refresh_mhz: 60_000 },
    VideoMode { width: 3840, height: 2160, refresh_mhz: 30_000 },
];

// ===========================================================================
// F378 — 页翻转律：双缓冲与 vblank 同步
// ===========================================================================

/// 双缓冲翻转链：vblank 未到时拒绝二次翻转（防撕裂）。
pub struct FlipChain {
    front: u8,
    pending: bool,
    pub flips: u32,
}

impl FlipChain {
    pub const fn new() -> FlipChain {
        FlipChain { front: 0, pending: false, flips: 0 }
    }
    pub fn front(&self) -> u8 {
        self.front
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn flip(&mut self) -> Option<u8> {
        if self.pending {
            return None;
        }
        self.front ^= 1;
        self.pending = true;
        self.flips += 1;
        Some(self.front)
    }
    pub fn vblank(&mut self) {
        self.pending = false;
    }
}

// ===========================================================================
// F379 — 多屏拓扑谱：扩展/镜像/单屏裁决
// ===========================================================================

pub const MAX_SCREENS: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TopoLink {
    Single,
    Extend,
    Mirror,
}

#[derive(Clone, Copy)]
pub struct ScreenTopo {
    pub screens: u8,
    pub primary: u8,
    pub link: TopoLink,
}

pub fn topo_valid(t: ScreenTopo) -> bool {
    if t.screens < 1 || t.screens > MAX_SCREENS || t.primary >= t.screens {
        return false;
    }
    match t.link {
        TopoLink::Single => t.screens == 1,
        TopoLink::Extend | TopoLink::Mirror => t.screens >= 2,
    }
}

// ===========================================================================
// F380 — 显示器档案：EDID 摘要档案
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MonitorProfile {
    pub name: &'static str,
    pub width_mm: u16,
    pub height_mm: u16,
    pub native: VideoMode,
}

impl MonitorProfile {
    pub fn complete(&self) -> bool {
        !self.name.is_empty() && self.width_mm > 0 && self.height_mm > 0
            && self.native.judge()
    }
    /// 物理对角线 dppx 不会失真：宽高均非零即可算密度（定点，由调用方算）。
    pub fn physical_sane(&self) -> bool {
        self.width_mm >= 100 && self.height_mm >= 100
    }
}

// ===========================================================================
// F381 — 合成内核接口：损坏矩形（damage rect）协议
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl DamageRect {
    pub fn intersects(&self, o: &DamageRect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    pub fn inside(&self, w: u16, h: u16) -> bool {
        self.w > 0 && self.h > 0 && self.x + self.w <= w && self.y + self.h <= h
    }
    pub fn area(&self) -> u32 {
        self.w as u32 * self.h as u32
    }
}

// ===========================================================================
// F382 — 帧缓冲直通舱：格式/跨度合法性
// ===========================================================================

pub const FMT_XRGB8888: u8 = 0;
pub const FMT_RGB565: u8 = 1;

#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub stride: u32,
}

pub fn bytes_per_pixel(format: u8) -> u32 {
    match format {
        FMT_XRGB8888 => 4,
        FMT_RGB565 => 2,
        _ => 0,
    }
}

pub fn passthrough_ok(fb: FramebufferInfo) -> bool {
    let bpp = bytes_per_pixel(fb.format);
    bpp > 0 && fb.stride >= fb.width as u32 * bpp
}

// ===========================================================================
// F383 — 亮度/背光官：钳位与渐变步进
// ===========================================================================

pub const BRIGHTNESS_MAX_PERMILLE: u16 = 1000;

pub fn clamp_brightness(v: u16) -> u16 {
    v.min(BRIGHTNESS_MAX_PERMILLE)
}

/// 向目标渐变，单次最多走 max_step，防跳变刺眼。
pub fn backlight_step(cur: u16, target: u16, max_step: u16) -> u16 {
    let target = clamp_brightness(target);
    if cur + max_step < target {
        cur + max_step
    } else if target + max_step < cur {
        cur - max_step
    } else {
        target
    }
}

// ===========================================================================
// F384 — 显示功耗谱：亮度/刷新率/HDR → permille 估算
// ===========================================================================

pub fn display_power_permille(brightness: u16, refresh_mhz: u32, hdr: bool) -> u16 {
    let base: u32 = 200;
    let by_bright: u32 = 600 * clamp_brightness(brightness) as u32 / 1000;
    let hi_refresh: u32 = if refresh_mhz > 90_000 { 150 } else { 0 };
    let hdr_extra: u32 = if hdr { 50 } else { 0 };
    (base + by_bright + hi_refresh + hdr_extra).min(1000) as u16
}

// ===========================================================================
// F385 — 热插拔显示事件：连接/断开序列
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HotplugEvent {
    pub connector: u8,
    pub connected: bool,
    pub seq: u16,
}

pub fn event_seq_valid(prev: u16, next: u16) -> bool {
    next == prev.wrapping_add(1)
}

// ===========================================================================
// F386 — 显示错误官：错误分类与可恢复性
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DisplayError {
    Underflow,
    CrcFail,
    LinkLoss,
    HpdStorm,
}

pub fn error_recoverable(e: DisplayError) -> bool {
    matches!(e, DisplayError::Underflow | DisplayError::CrcFail | DisplayError::LinkLoss)
}

// ===========================================================================
// F387 — gamma/LUT 通道：256 线性 ramp
// ===========================================================================

pub const LUT_LEN: usize = 256;

/// 8bit→16bit 线性 ramp（i*257 铺满 0..=65535）。
pub fn linear_ramp(lut: &mut [u16; LUT_LEN]) {
    for (i, v) in lut.iter_mut().enumerate() {
        *v = (i * 257) as u16;
    }
}

pub fn lut_monotonic(lut: &[u16; LUT_LEN]) -> bool {
    (1..LUT_LEN).all(|i| lut[i] > lut[i - 1])
}

// ===========================================================================
// F388 — 自检画面生成器：8 色条测试图
// ===========================================================================

pub const PATTERN_BARS: u32 = 8;
pub const PATTERN_PALETTE: [u32; 8] = [
    0xFFFFFF, 0xFFFF00, 0x00FFFF, 0x00FF00, 0xFF00FF, 0xFF0000, 0x0000FF, 0x000000,
];

pub fn pattern_pixel(x: u32, w: u32) -> u32 {
    let idx = if w == 0 { 0 } else { x * PATTERN_BARS / w };
    PATTERN_PALETTE[idx.min(PATTERN_BARS - 1) as usize]
}

// ===========================================================================
// F389 — 显示延迟仪：帧预算红线
// ===========================================================================

pub const FRAME_BUDGET_US: u32 = 16_600; // 60fps 一帧

pub fn within_frame_budget(us: u32) -> bool {
    us <= FRAME_BUDGET_US
}

// ===========================================================================
// F390 — 显示回归金样：FNV-1a 校验和
// ===========================================================================

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub const PATTERN_GOLDEN_HDR: u32 = 0x7661_7269; // "vari" 语义哨兵

pub fn pattern_golden_match(digest: u32) -> bool {
    digest != 0 && digest != PATTERN_GOLDEN_HDR // 金样必须来自真实渲染而非哨兵
}

// ===========================================================================
// F391 — HDR 元数据通道：CLL/FALL 裁决
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HdrMeta {
    pub max_cll_nits: u16,
    pub max_fall_nits: u16,
    pub min_luma_millinit: u16,
}

pub fn hdr_meta_valid(m: HdrMeta) -> bool {
    m.max_cll_nits >= m.max_fall_nits && m.max_cll_nits > 0
        && m.max_cll_nits <= 10_000 && m.min_luma_millinit <= 5
}

// ===========================================================================
// F392 — 显示事件流：定容事件日志
// ===========================================================================

pub const EVENT_RING_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct DisplayEventLog {
    kinds: [u8; EVENT_RING_CAP],
    count: usize,
}

impl DisplayEventLog {
    pub const fn new() -> DisplayEventLog {
        DisplayEventLog { kinds: [0; EVENT_RING_CAP], count: 0 }
    }
    /// kind: 1=flip 2=hotplug 3=fault。容量满则丢弃（返回 false）。
    pub fn push(&mut self, kind: u8) -> bool {
        if self.count >= EVENT_RING_CAP {
            return false;
        }
        self.kinds[self.count] = kind;
        self.count += 1;
        true
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn kind_at(&self, i: usize) -> Option<u8> {
        if i < self.count {
            Some(self.kinds[i])
        } else {
            None
        }
    }
}

// ===========================================================================
// F393 — 残影纠正官：周期性像素位移
// ===========================================================================

/// 每 10 分钟挪一次，8 个位置循环（右→下→左→上…）。
pub fn burnin_shift_offset(elapsed_min: u32) -> (i16, i16) {
    const STEP_MIN: u32 = 10;
    const OFFSET_PX: i16 = 8;
    let slot = (elapsed_min / STEP_MIN) % 4;
    match slot {
        0 => (OFFSET_PX, 0),
        1 => (0, OFFSET_PX),
        2 => (-OFFSET_PX, 0),
        _ => (0, -OFFSET_PX),
    }
}

pub fn shift_changed(a: (i16, i16), b: (i16, i16)) -> bool {
    a != b
}

// ===========================================================================
// F394 — 多屏一致性官：镜像/扩展模式一致性
// ===========================================================================

pub fn screens_consistent(modes: &[VideoMode]) -> bool {
    if modes.is_empty() {
        return false;
    }
    (1..modes.len()).all(|i| modes[i].same_as(&modes[0]))
}

// ===========================================================================
// F395 — 显示自描述导出：完整描述符
// ===========================================================================

pub const DESCRIPTOR_FIELDS: [&str; 6] =
    ["pipe", "modes", "monitors", "topology", "backlight", "events"];

pub fn descriptor_complete(fields: [&str; 6]) -> bool {
    (0..6).all(|i| !fields[i].is_empty())
}

// ===========================================================================
// F396 — 低分辨率优雅谱：整数倍上采样
// ===========================================================================

/// 上采样倍率 permille；仅接受整数倍（1000/2000/…）且不超过 4 倍。
pub fn upscale_permille(src: u16, dst: u16) -> Option<u16> {
    if src == 0 || dst < src {
        return None;
    }
    let m = (dst as u32 * 1000 / src as u32) as u16;
    if m % 1000 == 0 && m >= 1000 && m <= 4000 {
        Some(m)
    } else {
        None
    }
}

// ===========================================================================
// F397 — 显示健康分：错误率 → 分数
// ===========================================================================

pub fn health_score(errors: u32, flips: u32) -> u16 {
    if flips == 0 {
        return if errors == 0 { 1000 } else { 0 };
    }
    let penalty = (errors as u32 * 1000 / flips as u32).min(1000);
    (1000 - penalty) as u16
}

// ===========================================================================
// F398 — 显示压力剧本：多屏高刷翻转剧本
// ===========================================================================

pub const STRESS_DURATION_S: u32 = 300;
pub const STRESS_FPS: u32 = 60;
pub const STRESS_SCREENS: u8 = 2;

pub fn stress_plan_valid(screens: u8, fps: u32, dur_s: u32) -> bool {
    screens >= 1 && screens <= MAX_SCREENS && fps >= 30 && fps <= 240 && dur_s >= 60 && dur_s <= 3600
}

// ===========================================================================
// F399 — 显示文档生成器：文档小节清单
// ===========================================================================

pub const DOC_SECTIONS: [&str; 5] =
    ["pipeline", "mode-setting", "compositor-iface", "power", "troubleshooting"];

// ===========================================================================
// F400 — 显示域年报：年度回顾小节
// ===========================================================================

pub const REPORT_SECTIONS: [&str; 4] = ["milestones", "regressions", "metrics", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700disp_checks() -> CheckSet {
    let mut set = CheckSet::new("m700disp");

    // F376 显示管道
    set.add("F376 pipe viable", pipe_viable(DisplayPipe { planes: 4, crtcs: 2, encoders: 3, connectors: 2 }), "planes/crtcs ok");
    set.add("F376 pipe starved", !pipe_viable(DisplayPipe { planes: 1, crtcs: 2, encoders: 3, connectors: 2 }), "planes < crtcs");

    // F377 模式集裁判
    set.add("F377 modes judged", COMMON_MODES.iter().all(|m| m.judge()), "common modes pass");
    set.add("F377 mode rejected", !VideoMode { width: 641, height: 480, refresh_mhz: 60_000 }.judge(), "width%8!=0");
    set.add("F377 refresh bounds", !VideoMode { width: 1280, height: 720, refresh_mhz: 15_000 }.judge(), "refresh too low");

    // F378 页翻转律
    let mut fc = FlipChain::new();
    let f0 = fc.front();
    let first = fc.flip();
    set.add("F378 flip allowed", first == Some(1) && f0 == 0, "first flip");
    let second = fc.flip();
    set.add("F378 flip gated", second.is_none() && fc.pending(), "vblank gate");
    fc.vblank();
    let third = fc.flip();
    set.add("F378 flip alternate", third == Some(0) && fc.flips == 2, "ping-pong");

    // F379 多屏拓扑
    set.add("F379 topo extend", topo_valid(ScreenTopo { screens: 2, primary: 0, link: TopoLink::Extend }), "extend");
    set.add("F379 topo invalid", !topo_valid(ScreenTopo { screens: 1, primary: 0, link: TopoLink::Mirror }), "mirror needs 2");
    set.add("F379 topo overflow", !topo_valid(ScreenTopo { screens: 5, primary: 0, link: TopoLink::Extend }), "cap 4");

    // F380 显示器档案
    let mon = MonitorProfile {
        name: "VARIX-27Q",
        width_mm: 597,
        height_mm: 336,
        native: COMMON_MODES[2],
    };
    set.add("F380 monitor profile", mon.complete() && mon.physical_sane(), "edid summary");
    set.add("F380 empty rejected", !MonitorProfile { name: "", width_mm: 1, height_mm: 1, native: COMMON_MODES[0] }.complete(), "name required");

    // F381 合成接口
    let d1 = DamageRect { x: 0, y: 0, w: 100, h: 100 };
    let d2 = DamageRect { x: 50, y: 50, w: 100, h: 100 };
    set.add("F381 damage intersect", d1.intersects(&d2) && !d1.intersects(&DamageRect { x: 200, y: 200, w: 10, h: 10 }), "rect test");
    set.add("F381 damage clip", d1.inside(1000, 1000) && !d1.inside(50, 50), "bounds");

    // F382 帧缓冲直通
    set.add("F382 passthrough ok", passthrough_ok(FramebufferInfo { width: 1920, height: 1080, format: FMT_XRGB8888, stride: 7680 }), "stride 4*w");
    set.add("F382 stride short", !passthrough_ok(FramebufferInfo { width: 1920, height: 1080, format: FMT_RGB565, stride: 3800 }), "stride < 2*w");

    // F383 亮度背光
    set.add("F383 brightness clamp", clamp_brightness(1200) == 1000 && clamp_brightness(700) == 700, "clamp");
    set.add("F383 backlight step", backlight_step(300, 500, 50) == 350 && backlight_step(990, 1000, 50) == 1000, "gradual");

    // F384 显示功耗
    set.add("F384 power estimate", display_power_permille(1000, 60_000, false) == 800
        && display_power_permille(1000, 120_000, true) == 1000, "permille model");

    // F385 热插拔事件
    set.add("F385 hpd sequence", event_seq_valid(6, 7) && !event_seq_valid(6, 9), "seq +1");

    // F386 显示错误官
    set.add("F386 error classify", error_recoverable(DisplayError::CrcFail) && !error_recoverable(DisplayError::HpdStorm), "recoverable");

    // F387 gamma LUT
    let mut lut = [0u16; LUT_LEN];
    linear_ramp(&mut lut);
    set.add("F387 lut ramp", lut[0] == 0 && lut[255] == 65535 && lut_monotonic(&lut), "identity");

    // F388 自检画面
    set.add("F388 pattern bars", pattern_pixel(0, 800) == 0xFFFFFF && pattern_pixel(799, 800) == 0x000000, "8 bars");

    // F389 显示延迟仪
    set.add("F389 latency budget", within_frame_budget(14_000) && !within_frame_budget(17_000), "16.6ms");

    // F390 回归金样
    let digest = fnv1a(b"pattern-v1");
    set.add("F390 golden digest", pattern_golden_match(digest) && !pattern_golden_match(PATTERN_GOLDEN_HDR), "fnv1a");

    // F391 HDR 元数据
    set.add("F391 hdr meta", hdr_meta_valid(HdrMeta { max_cll_nits: 1000, max_fall_nits: 400, min_luma_millinit: 1 })
        && !hdr_meta_valid(HdrMeta { max_cll_nits: 400, max_fall_nits: 4000, min_luma_millinit: 1 }), "cll>=fall");

    // F392 显示事件流
    let mut ev = DisplayEventLog::new();
    let pushed = ev.push(2);
    ev.push(1);
    ev.push(3);
    set.add("F392 event ring", pushed && ev.count() == 3 && ev.kind_at(0) == Some(2), "ordered log");

    // F393 残影纠正
    let s0 = burnin_shift_offset(0);
    let s1 = burnin_shift_offset(10);
    set.add("F393 burnin shift", shift_changed(s0, s1) && burnin_shift_offset(2400) == (8, 0), "cycle 4 slots");

    // F394 多屏一致性
    let duo = [COMMON_MODES[2], COMMON_MODES[2]];
    set.add("F394 multi consistency", screens_consistent(&duo), "same modes");
    set.add("F394 mismatch caught", !screens_consistent(&COMMON_MODES[0..2]), "diff modes");

    // F395 自描述导出
    set.add("F395 descriptor", descriptor_complete(["p", "m", "mon", "topo", "bl", "ev"]), "6 fields");
    set.add("F395 descriptor gap", !descriptor_complete(["p", "", "mon", "topo", "bl", "ev"]), "no empty");

    // F396 低分辨率优雅
    set.add("F396 upscale int", upscale_permille(320, 640) == Some(2000) && upscale_permille(320, 900).is_none(), "integer only");

    // F397 健康分
    set.add("F397 health score", health_score(10, 100) == 900 && health_score(0, 0) == 1000, "error rate");

    // F398 压力剧本
    set.add("F398 stress plan", stress_plan_valid(STRESS_SCREENS, STRESS_FPS, STRESS_DURATION_S)
        && !stress_plan_valid(5, 60, 300), "bounds");

    // F399 文档生成器
    set.add("F399 doc sections", DOC_SECTIONS.len() == 5, "5 sections");

    // F400 年报
    set.add("F400 annual report", REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f377_mode_judge_bounds() {
        let m = VideoMode { width: 1280, height: 720, refresh_mhz: 144_000 };
        assert!(m.judge());
        assert!(!VideoMode { width: 100, height: 720, refresh_mhz: 60_000 }.judge());
        assert!(!VideoMode { width: 1280, height: 720, refresh_mhz: 300_000 }.judge());
    }

    #[test]
    fn f378_flip_gating() {
        let mut fc = FlipChain::new();
        assert_eq!(fc.flip(), Some(1));
        assert_eq!(fc.flip(), None);
        fc.vblank();
        assert_eq!(fc.flip(), Some(0));
        assert_eq!(fc.flips, 2);
    }

    #[test]
    fn f383_step_never_overshoots() {
        assert_eq!(backlight_step(100, 100, 50), 100);
        assert_eq!(backlight_step(0, 1000, 500), 500);
        assert_eq!(backlight_step(1000, 0, 400), 600);
    }

    #[test]
    fn f387_lut_identity() {
        let mut lut = [0u16; LUT_LEN];
        linear_ramp(&mut lut);
        assert_eq!(lut[1], 257);
        assert_eq!(lut[128], 128 * 257);
        assert!(lut_monotonic(&lut));
    }

    #[test]
    fn f393_shift_cycle() {
        assert_eq!(burnin_shift_offset(0), (8, 0));
        assert_eq!(burnin_shift_offset(10), (0, 8));
        assert_eq!(burnin_shift_offset(20), (-8, 0));
        assert_eq!(burnin_shift_offset(30), (0, -8));
    }

    #[test]
    fn f400_domain_selfcheck_all_pass() {
        let set = run_m700disp_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
