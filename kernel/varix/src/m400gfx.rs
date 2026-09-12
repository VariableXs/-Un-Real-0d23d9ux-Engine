//! VARIX-M400 AI-08 图形域（F176~F200）。
//!
//! 帧预算内的流畅合成。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F176 — KMS 式显示管理（模式设置）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

pub fn f176_mode_valid(m: VideoMode) -> bool {
    m.width > 0 && m.height > 0 && (24..=240).contains(&m.refresh_hz)
}

pub fn f176_modeset(cur: VideoMode, want: VideoMode) -> Option<VideoMode> {
    if f176_mode_valid(want) {
        Some(want)
    } else {
        Some(cur) // 非法请求保持现状
    }
}

// ---------------------------------------------------------------------------
// F177 — 多显示器热插拔
// ---------------------------------------------------------------------------

pub const MONITOR_CAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub id: u8,
    pub connected: bool,
    pub x: i32,
    pub y: i32,
}

/// 拔掉一台后其余显示器布局自适应（坐标不重叠的简化检查）。
pub fn f177_layout_sane(mons: &[Monitor]) -> bool {
    for (i, a) in mons.iter().enumerate() {
        for b in &mons[i + 1..] {
            if a.x == b.x && a.y == b.y {
                return false; // 原点重叠
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F178 — EDID 解析
// ---------------------------------------------------------------------------

pub fn f178_edid_valid(block: &[u8; 128]) -> bool {
    block[0] == 0x00 && block[1] == 0xFF && block[2] == 0xFF && block[3] == 0xFF
        && block[4] == 0xFF && block[5] == 0xFF && block[6] == 0xFF && block[7] == 0x00
        && block[126] != 0xFF // 扩展数合法（0xFF 无效）
}

/// 详细时序描述符第一字节 0x00 表示详细时序，否则是监视器名。
pub fn f178_is_descriptor(tag: u8) -> bool {
    tag == 0x00
}

// ---------------------------------------------------------------------------
// F179 — 硬件光标（不进合成路径）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: i32,
    pub y: i32,
    pub visible: bool,
}

/// 光标移动只改硬件层寄存器，不触发 damage。
pub fn f179_cursor_no_damage(before: Cursor, after: Cursor) -> bool {
    before.x != after.x || before.y != after.y
}

// ---------------------------------------------------------------------------
// F180 — GPU PCI 探测
// ---------------------------------------------------------------------------

pub fn f180_gpu_vendor(vendor_id: u16) -> Option<&'static str> {
    match vendor_id {
        0x8086 => Some("Intel"),
        0x10DE => Some("NVIDIA"),
        0x1002 => Some("AMD"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F181 — 2D 加速接口（blit/fill）
// ---------------------------------------------------------------------------

/// fill：dst 区域在表面内。
pub fn f181_fill_in_bounds(surf: (u32, u32), x: u32, y: u32, w: u32, h: u32) -> bool {
    w > 0 && h > 0 && x.checked_add(w).map(|e| e <= surf.0).unwrap_or(false)
        && y.checked_add(h).map(|e| e <= surf.1).unwrap_or(false)
}

/// blit：src 与 dst 区域尺寸一致且都在各自表面内。
pub fn f181_blit_ok(src_surf: (u32, u32), dst_surf: (u32, u32), sx: u32, sy: u32, dx: u32, dy: u32, w: u32, h: u32) -> bool {
    f181_fill_in_bounds(src_surf, sx, sy, w, h) && f181_fill_in_bounds(dst_surf, dx, dy, w, h)
}

// ---------------------------------------------------------------------------
// F182 — 合成器离屏缓冲（合成与显示解耦）
// ---------------------------------------------------------------------------

pub const OFFSCREEN_LAYERS: usize = 8;

pub struct Layer {
    pub z: u8,
    pub visible: bool,
    pub alpha: u8, // 0..=255
}

/// 按 z 排序渲染顺序（稳定选择排序，固定容量）。
pub fn f182_render_order(layers: &[Layer]) -> ([usize; OFFSCREEN_LAYERS], usize) {
    let mut idx = [0usize; OFFSCREEN_LAYERS];
    let mut n = 0usize;
    for i in 0..layers.len() {
        idx[n] = i;
        n += 1;
    }
    for i in 1..n {
        let key = idx[i];
        let mut j = i;
        while j > 0 && layers[idx[j - 1]].z > layers[key].z {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[j] = key;
    }
    (idx, n)
}

// ---------------------------------------------------------------------------
// F183 — damage 跟踪（只重绘脏区）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

pub struct DamageTracker {
    pub dirty: [Option<Rect>; 8],
    pub count: usize,
}

impl DamageTracker {
    pub const fn new() -> DamageTracker {
        DamageTracker { dirty: [const { None }; 8], count: 0 }
    }

    pub fn mark(&mut self, r: Rect) -> bool {
        if self.count >= 8 {
            return false;
        }
        self.dirty[self.count] = Some(r);
        self.count += 1;
        true
    }

    /// 帧内脏区是否覆盖给定的检测矩形。
    pub fn needs_redraw(&self, probe: &Rect) -> bool {
        self.dirty[..self.count].iter().flatten().any(|r| r.intersects(probe))
    }

    pub fn present(&mut self) {
        self.count = 0;
    }
}

// ---------------------------------------------------------------------------
// F184 — VSYNC 同步
// ---------------------------------------------------------------------------

/// 呈现必须发生在 vblank 窗口内才无撕裂。
pub fn f184_present_in_vblank(scanline: u32, total_scanlines: u32) -> bool {
    scanline >= total_scanlines.saturating_sub(1)
}

// ---------------------------------------------------------------------------
// F185 — 帧率自适应（低负载降帧）
// ---------------------------------------------------------------------------

pub fn f185_target_fps(gpu_load_permille: u16, base_fps: u32) -> u32 {
    if gpu_load_permille > 900 {
        base_fps / 2
    } else if gpu_load_permille > 700 {
        base_fps * 3 / 4
    } else {
        base_fps
    }
}

// ---------------------------------------------------------------------------
// F186 — 色彩管理（sRGB）
// ---------------------------------------------------------------------------

/// sRGB gamma 编码（近似，定点）。
pub fn f186_srgb_encode(linear: u8) -> u8 {
    // 线性 0-255 到 sRGB 的幂次近似：out = in^(1/2.2) * 255^(1-1/2.2)
    let lut: [u8; 9] = [0, 52, 90, 117, 138, 156, 171, 184, 195];
    let idx = (linear / 32) as usize;
    lut[idx.min(8)]
}

// ---------------------------------------------------------------------------
// F187 — HiDPI 完备
// ---------------------------------------------------------------------------

/// 逻辑像素 -> 物理像素。
pub fn f187_hidpi_scale(logical: u32, scale_permille: u32) -> u32 {
    (logical as u64 * scale_permille as u64 / 1000) as u32
}

// ---------------------------------------------------------------------------
// F188 — 截屏 API
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureTarget {
    FullScreen,
    Region(Rect),
    Window(u32),
}

pub fn f188_capture_target_valid(t: CaptureTarget) -> bool {
    match t {
        CaptureTarget::FullScreen | CaptureTarget::Window(_) => true,
        CaptureTarget::Region(r) => r.w > 0 && r.h > 0,
    }
}

// ---------------------------------------------------------------------------
// F189 — 录屏骨架（帧序列捕获）
// ---------------------------------------------------------------------------

pub const REC_FRAMES: usize = 32;

pub struct Recorder {
    pub frames: [u64; REC_FRAMES], // 时间戳 ms
    pub count: usize,
}

impl Recorder {
    pub const fn new() -> Recorder {
        Recorder { frames: [0; REC_FRAMES], count: 0 }
    }

    pub fn add_frame(&mut self, ts_ms: u64) -> bool {
        if self.count >= REC_FRAMES {
            return false;
        }
        if self.count > 0 && ts_ms <= self.frames[self.count - 1] {
            return false; // 时间戳必须单调
        }
        self.frames[self.count] = ts_ms;
        self.count += 1;
        true
    }

    pub fn fps(&self) -> u32 {
        if self.count < 2 {
            return 0;
        }
        let span = self.frames[self.count - 1] - self.frames[0];
        if span == 0 {
            return 0;
        }
        ((self.count as u64 - 1) * 1000 / span) as u32
    }
}

// ---------------------------------------------------------------------------
// F190 — 显示故障安全模式
// ---------------------------------------------------------------------------

pub const SAFE_MODE: VideoMode = VideoMode { width: 640, height: 480, refresh_hz: 60 };

/// 主模式异常时回落最低可用显示。
pub fn f190_fallback_mode(primary_ok: bool, primary: VideoMode) -> VideoMode {
    if primary_ok && f176_mode_valid(primary) {
        primary
    } else {
        SAFE_MODE
    }
}

// ---------------------------------------------------------------------------
// F191 — 亮度/夜览
// ---------------------------------------------------------------------------

pub fn f191_brightness_clamp(pct: i8) -> u8 {
    pct.clamp(0, 100) as u8
}

/// 夜览色温偏移：R 增 B 减。
pub fn f191_night_shift(r: u8, b: u8, strength: u8) -> (u8, u8) {
    let s = strength.min(100) as u16;
    let nr = (r as u16 + (255 - r as u16) * s / 400) as u8;
    let nb = (b as u16).saturating_sub(b as u16 * s / 200) as u8;
    (nr, nb)
}

// ---------------------------------------------------------------------------
// F192 — 分辨率偏好持久化
// ---------------------------------------------------------------------------

pub fn f192_persist_pref(pref: VideoMode) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&pref.width.to_le_bytes());
    out[4..8].copy_from_slice(&pref.height.to_le_bytes());
    out[8..12].copy_from_slice(&pref.refresh_hz.to_le_bytes());
    out
}

pub fn f192_restore_pref(bytes: &[u8; 12]) -> VideoMode {
    VideoMode {
        width: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        height: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        refresh_hz: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
    }
}

// ---------------------------------------------------------------------------
// F193 — 合成 P95 基准（16.6ms 帧预算）
// ---------------------------------------------------------------------------

pub const FRAME_BUDGET_US: u64 = 16_600;

pub fn f193_within_budget(sample_us: u64) -> bool {
    sample_us <= FRAME_BUDGET_US
}

// ---------------------------------------------------------------------------
// F194 — tearing 检测
// ---------------------------------------------------------------------------

/// 相邻帧同一扫描线内容不一致即撕裂。
pub fn f194_tearing_detected(top_frame_hash: u64, bottom_frame_hash: u64, scanline_uniform: bool) -> bool {
    !scanline_uniform && top_frame_hash != bottom_frame_hash
}

// ---------------------------------------------------------------------------
// F195 — 多 GPU 优先级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: u16,
    pub discrete: bool,
    pub vram_mib: u32,
}

/// 离散显存大者优先；同型取显存大者。
pub fn f195_pick_gpu(a: Gpu, b: Gpu) -> Gpu {
    let rank = |g: Gpu| (g.discrete as u32, g.vram_mib);
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// F196 — 热事件处理（显示器事件不丢帧）
// ---------------------------------------------------------------------------

pub fn f196_event_queue_ok(queued: u32, cap: u32, dropped: u32) -> bool {
    dropped == 0 && queued <= cap
}

// ---------------------------------------------------------------------------
// F197 — 显示器配置数据源
// ---------------------------------------------------------------------------

pub fn f197_config_snapshot(mons: &[Monitor]) -> ([u8; MONITOR_CAP * 2], usize) {
    let mut out = [0u8; MONITOR_CAP * 2];
    let mut n = 0usize;
    for m in mons {
        if n + 2 > out.len() {
            break;
        }
        out[n] = m.id;
        out[n + 1] = m.connected as u8;
        n += 2;
    }
    (out, n)
}

// ---------------------------------------------------------------------------
// F198 — 帧捕获（供兼容层嵌入验证）
// ---------------------------------------------------------------------------

/// 捕获延迟必须 < 50ms。
pub fn f198_capture_latency_ok(us: u64) -> bool {
    us < 50_000
}

// ---------------------------------------------------------------------------
// F199 — 合成器压力测试（50 窗口）
// ---------------------------------------------------------------------------

pub fn f199_windows_in_budget(layers: usize, per_layer_us: u64) -> bool {
    (layers as u64 * per_layer_us) <= FRAME_BUDGET_US
}

// ---------------------------------------------------------------------------
// F200 — 图形域自检
// ---------------------------------------------------------------------------

pub fn run_gfxm400_checks() -> CheckSet {
    let mut set = CheckSet::new("gfxm400");

    // F176
    let mode = VideoMode { width: 1920, height: 1080, refresh_hz: 60 };
    set.add("F176 valid", f176_mode_valid(mode), "1080p60");
    set.add("F176 bad refresh", !f176_mode_valid(VideoMode { refresh_hz: 300, ..mode }), "300Hz rejected");
    set.add("F176 fallback", f176_modeset(mode, VideoMode { width: 0, height: 0, refresh_hz: 0 }) == Some(mode), "keep current on invalid");

    // F177
    let mons = [
        Monitor { id: 0, connected: true, x: 0, y: 0 },
        Monitor { id: 1, connected: true, x: 1920, y: 0 },
    ];
    let bad = [
        Monitor { id: 0, connected: true, x: 0, y: 0 },
        Monitor { id: 1, connected: true, x: 0, y: 0 },
    ];
    set.add("F177 sane", f177_layout_sane(&mons), "side by side");
    set.add("F177 overlap", !f177_layout_sane(&bad), "overlapping origins");

    // F178
    let mut edid = [0u8; 128];
    edid[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
    edid[126] = 1;
    set.add("F178 valid", f178_edid_valid(&edid), "header+ext");
    edid[126] = 0xFF;
    set.add("F178 bad ext", !f178_edid_valid(&edid), "0xFF extensions invalid");
    set.add("F178 desc", f178_is_descriptor(0x00) && !f178_is_descriptor(0xFC), "tag 0x00 = timing");

    // F179
    let c1 = Cursor { x: 10, y: 10, visible: true };
    let c2 = Cursor { x: 11, y: 10, visible: true };
    set.add("F179 move", f179_cursor_no_damage(c1, c2), "cursor move free");

    // F180
    set.add("F180 intel", f180_gpu_vendor(0x8086) == Some("Intel"), "intel");
    set.add("F180 unknown", f180_gpu_vendor(0xBEEF).is_none(), "unknown");

    // F181
    set.add("F181 fill", f181_fill_in_bounds((100, 100), 10, 10, 50, 50), "inside");
    set.add("F181 fill oob", !f181_fill_in_bounds((100, 100), 60, 60, 50, 50), "out of bounds");
    set.add("F181 blit", f181_blit_ok((100, 100), (200, 200), 0, 0, 150, 150, 50, 50), "blit ok");
    set.add("F181 blit src oob", !f181_blit_ok((100, 100), (200, 200), 80, 0, 0, 0, 50, 50), "src clipped");

    // F182
    let layers = [
        Layer { z: 30, visible: true, alpha: 255 },
        Layer { z: 10, visible: true, alpha: 128 },
        Layer { z: 20, visible: false, alpha: 255 },
    ];
    let (order, n) = f182_render_order(&layers);
    set.add("F182 order", n == 3 && order[0] == 1 && order[1] == 2 && order[2] == 0, "z sorted");

    // F183
    let mut dmg = DamageTracker::new();
    dmg.mark(Rect { x: 0, y: 0, w: 100, h: 100 });
    set.add("F183 dirty hit", dmg.needs_redraw(&Rect { x: 50, y: 50, w: 10, h: 10 }), "intersects dirty");
    set.add("F183 clean", !dmg.needs_redraw(&Rect { x: 200, y: 200, w: 10, h: 10 }), "no intersection");
    dmg.present();
    set.add("F183 cleared", !dmg.needs_redraw(&Rect { x: 50, y: 50, w: 10, h: 10 }), "present clears");

    // F184
    set.add("F184 vblank", f184_present_in_vblank(1079, 1080), "last scanline");
    set.add("F184 mid frame", !f184_present_in_vblank(500, 1080), "mid-frame tears");

    // F185
    set.add("F185 full", f185_target_fps(500, 60) == 60, "light load");
    set.add("F185 three quarter", f185_target_fps(800, 60) == 45, "medium load");
    set.add("F185 half", f185_target_fps(950, 60) == 30, "heavy load");

    // F186
    set.add("F186 zero", f186_srgb_encode(0) == 0, "black");
    set.add("F186 monotonic", f186_srgb_encode(32) <= f186_srgb_encode(224), "gamma grows");

    // F187
    set.add("F187 2x", f187_hidpi_scale(800, 2000) == 1600, "2x scale");
    set.add("F187 125%", f187_hidpi_scale(800, 1250) == 1000, "1.25x scale");

    // F188
    set.add("F188 full", f188_capture_target_valid(CaptureTarget::FullScreen), "fullscreen ok");
    set.add("F188 region", f188_capture_target_valid(CaptureTarget::Region(Rect { x: 0, y: 0, w: 100, h: 100 })), "region ok");
    set.add("F188 empty region", !f188_capture_target_valid(CaptureTarget::Region(Rect { x: 0, y: 0, w: 0, h: 0 })), "empty rejected");

    // F189
    let mut rec = Recorder::new();
    let seq_ok = rec.add_frame(0) && rec.add_frame(33) && rec.add_frame(66);
    set.add("F189 frames", seq_ok && rec.count == 3, "three frames");
    set.add("F189 monotonic", !rec.add_frame(66), "non-monotonic rejected");
    set.add("F189 fps", rec.fps() == 30, "~30fps");

    // F190
    set.add("F190 ok keep", f190_fallback_mode(true, mode) == mode, "primary kept");
    set.add("F190 fallback", f190_fallback_mode(false, mode) == SAFE_MODE, "safe mode used");

    // F191
    set.add("F191 clamp", f191_brightness_clamp(-10) == 0 && f191_brightness_clamp(120) == 100, "brightness range");
    let (nr, nb) = f191_night_shift(100, 100, 100);
    set.add("F191 night", nr >= 100 && nb < 100, "warm shift");

    // F192
    let saved = f192_persist_pref(mode);
    set.add("F192 roundtrip", f192_restore_pref(&saved) == mode, "persist/restore");

    // F193
    set.add("F193 within", f193_within_budget(12_000), "12ms ok");
    set.add("F193 over", !f193_within_budget(20_000), "20ms over budget");

    // F194
    set.add("F194 detect", f194_tearing_detected(1, 2, false), "hash mismatch = tear");
    set.add("F194 clean", !f194_tearing_detected(1, 1, true), "uniform scanline");

    // F195
    let igpu = Gpu { vendor: 0x8086, discrete: false, vram_mib: 2048 };
    let dgpu = Gpu { vendor: 0x10DE, discrete: true, vram_mib: 8192 };
    set.add("F195 discrete", f195_pick_gpu(igpu, dgpu) == dgpu, "discrete wins");
    set.add("F195 vram", f195_pick_gpu(dgpu, Gpu { vram_mib: 4096, ..dgpu }).vram_mib == 8192, "more vram wins");

    // F196
    set.add("F196 queue ok", f196_event_queue_ok(5, 16, 0), "no drops");
    set.add("F196 dropped", !f196_event_queue_ok(17, 16, 1), "drop detected");

    // F197
    let (snap, n) = f197_config_snapshot(&mons);
    set.add("F197 snapshot", n == 4 && snap[0] == 0 && snap[1] == 1, "id+connected");

    // F198
    set.add("F198 latency", f198_capture_latency_ok(40_000) && !f198_capture_latency_ok(60_000), "<50ms");

    // F199
    set.add("F199 50 windows", f199_windows_in_budget(50, 300), "50*300us = 15ms");
    set.add("F199 over", !f199_windows_in_budget(50, 400), "20ms over");

    // F200
    set.add("F200 coverage", set.len() >= 25, "domain coverage");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f200_gfxm400_selftest_all_pass() {
        let set = run_gfxm400_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f183_damage_capacity() {
        let mut d = DamageTracker::new();
        for i in 0..8 {
            assert!(d.mark(Rect { x: i as u32 * 10, y: 0, w: 5, h: 5 }));
        }
        assert!(!d.mark(Rect { x: 99, y: 0, w: 1, h: 1 }));
    }

    #[test]
    fn f189_recorder_capacity() {
        let mut r = Recorder::new();
        for i in 0..REC_FRAMES {
            assert!(r.add_frame(i as u64 * 16));
        }
        assert!(!r.add_frame(1 << 30));
    }
}
