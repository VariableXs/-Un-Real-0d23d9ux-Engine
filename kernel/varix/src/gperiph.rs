//! GALAXY-1800 AI-06 外设全栈 USB·GPU·音频域（G301~G360）。
//!
//! Three merged sub-domains: USB stack (G301~G320), graphics pipeline
//! (G321~G340) and audio stack (G341~G360). Pure logic over fixed
//! arrays; MMIO access stays at the edges, logic is host-testable.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G301~G320 — USB
// ---------------------------------------------------------------------------

/// G301 设备描述符解析（18 字节标准设备描述符）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbDevDesc {
    pub usb_version: u16, // BCD，如 0x0200
    pub class: u8,
    pub vendor: u16,
    pub product: u16,
    pub num_configs: u8,
}

pub fn parse_dev_desc(d: &[u8]) -> Option<UsbDevDesc> {
    if d.len() < 18 || d[0] != 18 || d[1] != 0x01 {
        return None;
    }
    Some(UsbDevDesc {
        usb_version: u16::from_le_bytes([d[2], d[3]]),
        class: d[4],
        vendor: u16::from_le_bytes([d[8], d[9]]),
        product: u16::from_le_bytes([d[10], d[11]]),
        num_configs: d[17],
    })
}

/// G301 端点描述符：地址方向类型与包大小。
pub fn parse_ep_desc(d: &[u8]) -> Option<(u8, bool, u8, u16)> {
    if d.len() < 7 || d[1] != 0x05 {
        return None;
    }
    let addr = d[2] & 0x0F;
    let dir_in = d[2] & 0x80 != 0;
    let transfer = d[3] & 0x3;
    let mps = u16::from_le_bytes([d[5], d[6]]);
    Some((addr, dir_in, transfer, mps))
}

/// G302 xHCI TRB：参数/状态/控制位编码（cycle 位在 bit0）。
pub fn trb_encode(trb_type: u8, cycle: bool, param: u64) -> [u32; 4] {
    [
        param as u32,
        (param >> 32) as u32,
        0,
        (trb_type as u32) << 10 | cycle as u32,
    ]
}

pub fn trb_decode(w: &[u32; 4]) -> (u8, bool) {
    ((w[3] >> 10) as u8 & 0x3F, w[3] & 1 != 0)
}

/// G302 环推进：到链尾回绕并翻转 cycle。
pub fn trb_ring_advance(idx: usize, len: usize, cycle: bool) -> (usize, bool) {
    if idx + 1 == len {
        (0, !cycle)
    } else {
        (idx + 1, cycle)
    }
}

/// G304 传输调度：批量/中断/等时三队列。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum XferType {
    Bulk,
    Interrupt,
    Isoch,
}

pub fn schedule_xfer(bandwidth_used: u16, isoch_budget: u16, t: XferType) -> bool {
    match t {
        XferType::Isoch => bandwidth_used + 1 <= isoch_budget,
        _ => true, // 批量/中断可排队
    }
}

/// G305 HID 键盘报告：修饰键 + 6 键位。
pub fn hid_keyboard_mods(report: &[u8]) -> (u8, [u8; 6]) {
    let mut keys = [0u8; 6];
    for (i, k) in keys.iter_mut().enumerate() {
        *k = report.get(i + 2).copied().unwrap_or(0);
    }
    (report.first().copied().unwrap_or(0), keys)
}

pub const HID_MOD_CTRL: u8 = 1 << 0;
pub const HID_MOD_SHIFT: u8 = 1 << 1;

/// G306 USB 存储类：LBA 读写封包（CBW 简化）。
pub fn msc_cbw_read10(lba: u32, blocks: u16, tag: u32) -> [u8; 16] {
    let mut cbw = [0u8; 16];
    cbw[0..4].copy_from_slice(&tag.to_le_bytes());
    cbw[4..8].copy_from_slice(&(blocks as u32 * 512).to_le_bytes());
    cbw[8] = 0x80; // data-in
    cbw[12] = 0x28; // READ(10) op
    cbw[13..17.min(16)].copy_from_slice(&lba.to_le_bytes()[..3]);
    cbw
}

/// G308 USB 网络类：以太网帧封装校验（dst/src/type）。
pub fn usbnet_frame_ok(frame: &[u8]) -> bool {
    frame.len() >= 14 && frame[12] == 0x08 && (frame[13] == 0x00 || frame[13] == 0x06)
}

/// G309 热插拔事件流：attach/detach 序列化。
pub fn usb_hotplug_event(port: u8, attached: bool) -> u16 {
    ((port as u16) << 8) | attached as u16
}

/// G310 集线器端口状态位。
pub fn hub_port_status(raw: u16) -> (bool, bool, bool, bool) {
    (raw & 1 != 0, raw & 2 != 0, raw & 4 != 0, raw & 8 != 0) // connect/en/suspend/overcurrent
}

/// G312 设备权限：类设备访问白名单。
pub fn usb_permission_ok(class: u8, whitelist: &[u8]) -> bool {
    whitelist.contains(&class)
}

/// G316 速度协商：低速/全速/高速/超速。
pub fn usb_speed_name(port_speed: u8) -> &'static str {
    match port_speed {
        0 => "low",
        1 => "full",
        2 => "high",
        3 => "super",
        _ => "unknown",
    }
}

/// G317 错误恢复：三次重试后停止。
pub fn usb_retry(attempts: u8, max: u8) -> u8 {
    attempts.min(max)
}

/// G315 复合设备：多接口类列表。
pub fn composite_interfaces(cfg: &[u8]) -> usize {
    cfg.iter().filter(|&&b| b == 0x04).count() // 接口描述符标记
}

// ---------------------------------------------------------------------------
// G321~G340 — GPU
// ---------------------------------------------------------------------------

pub const FB_W: usize = 64;
pub const FB_H: usize = 64;

/// G321 像素格式：XRGB8888 打包。
pub fn pixel_rgb(r: u8, g: u8, b: u8) -> u32 {
    (0xFFu32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub fn pixel_unpack(p: u32) -> (u8, u8, u8) {
    ((p >> 16) as u8, (p >> 8) as u8, p as u8)
}

/// G322 软件 GPU：三角形光栅化（包围盒 + 重心边函数）。
pub struct Fb {
    pub px: [u32; FB_W * FB_H],
}

impl Fb {
    pub fn clear(&mut self, c: u32) {
        self.px.fill(c);
    }
    pub fn put(&mut self, x: usize, y: usize, c: u32) {
        if x < FB_W && y < FB_H {
            self.px[y * FB_W + x] = c;
        }
    }
    pub fn get(&self, x: usize, y: usize) -> u32 {
        self.px[y * FB_W + x]
    }
}

fn edge(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

pub fn raster_triangle(fb: &mut Fb, v: [(i32, i32); 3], c: u32) -> usize {
    let minx = v[0].0.min(v[1].0).min(v[2].0).max(0) as usize;
    let maxx = v[0].0.max(v[1].0).max(v[2].0).min(FB_W as i32 - 1) as usize;
    let miny = v[0].1.min(v[1].1).min(v[2].1).max(0) as usize;
    let maxy = v[0].1.max(v[1].1).max(v[2].1).min(FB_H as i32 - 1) as usize;
    let mut n = 0usize;
    for y in miny..=maxy {
        for x in minx..=maxx {
            let (px, py) = (x as i32, y as i32);
            let e0 = edge(v[0].0, v[0].1, v[1].0, v[1].1, px, py);
            let e1 = edge(v[1].0, v[1].1, v[2].0, v[2].1, px, py);
            let e2 = edge(v[2].0, v[2].1, v[0].0, v[0].1, px, py);
            let inside = (e0 >= 0 && e1 >= 0 && e2 >= 0) || (e0 <= 0 && e1 <= 0 && e2 <= 0);
            if inside {
                fb.put(x, y, c);
                n += 1;
            }
        }
    }
    n
}

/// G323 2D 加速：矩形填充与位块拷贝。
pub fn fill_rect(fb: &mut Fb, x0: usize, y0: usize, w: usize, h: usize, c: u32) {
    for y in y0..(y0 + h).min(FB_H) {
        for x in x0..(x0 + w).min(FB_W) {
            fb.put(x, y, c);
        }
    }
}

pub fn blit(dst: &mut Fb, src: &Fb, dx: usize, dy: usize) {
    for y in 0..FB_H {
        for x in 0..FB_W {
            dst.put(dx + x, dy + y, src.get(x, y));
        }
    }
}

/// G324 命令提交框架：命令缓冲解码（op, args…）。
pub const CMD_FILL: u8 = 1;
pub const CMD_TRI: u8 = 2;
pub const CMD_FLIP: u8 = 3;

pub fn cmd_decode(buf: &[u8]) -> ([u8; 8], usize) {
    let mut ops = [0u8; 8];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < buf.len() && n < 8 {
        let op = buf[i];
        ops[n] = op;
        n += 1;
        i += match op {
            CMD_FILL => 6,  // op x y w h c
            CMD_TRI => 7,   // op x0 y0 x1 y1 x2 y2
            CMD_FLIP => 1,
            _ => 1,
        };
    }
    (ops, n)
}

/// G326/G329 降级链：真 GPU → 虚拟 GPU → 软件。
pub fn gpu_degrade(real: bool, virt: bool) -> &'static str {
    if real {
        "real"
    } else if virt {
        "virtio"
    } else {
        "softpipe"
    }
}

/// G327 显存管理：固定块分配器。
pub struct Vram {
    free: [bool; 16], // true = 空闲
    pub used: usize,
}

impl Vram {
    pub fn new() -> Vram {
        Vram { free: [true; 16], used: 0 }
    }
    pub fn alloc(&mut self) -> Option<usize> {
        let i = (0..16).find(|&i| self.free[i])?;
        self.free[i] = false;
        self.used += 1;
        Some(i)
    }
    pub fn release(&mut self, i: usize) {
        if i < 16 && !self.free[i] {
            self.free[i] = true;
            self.used -= 1;
        }
    }
}

/// G335 合成器加速：脏矩形合并。
pub fn dirty_rects_merge(a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let x0 = a.0.min(b.0);
    let y0 = a.1.min(b.1);
    let x1 = (a.0 + a.2).max(b.0 + b.2);
    let y1 = (a.1 + a.3).max(b.1 + b.3);
    (x0, y0, x1 - x0, y1 - y0)
}

/// G339 GPU 功耗：空闲降频档位。
pub fn gpu_idle_clock(idle_ms: u64, base_mhz: u32) -> u32 {
    if idle_ms > 1000 {
        base_mhz / 8
    } else if idle_ms > 100 {
        base_mhz / 2
    } else {
        base_mhz
    }
}

// ---------------------------------------------------------------------------
// G341~G360 — 音频
// ---------------------------------------------------------------------------

/// G341 HDA verb 打包：cad/nid/verb/payload。
pub fn hda_verb(cad: u8, nid: u8, verb: u16, payload: u16) -> u32 {
    ((cad as u32) << 28) | ((nid as u32) << 20) | ((verb as u32) << 8) | payload as u32
}

pub fn hda_verb_unpack(w: u32) -> (u8, u8, u16, u16) {
    (
        (w >> 28) as u8,
        ((w >> 20) & 0xFF) as u8,
        ((w >> 8) & 0xFFF) as u16,
        (w & 0xFF) as u16,
    )
}

pub const HDA_VERB_GET_PARAM: u16 = 0xF00;
pub const HDA_VERB_SET_AMP: u16 = 0x300;

/// G343 DMA 环形缓冲：读/写指针与可用帧数。
pub struct AudioRing {
    pub buf: [i16; 32],
    pub write_pos: usize,
    pub read_pos: usize,
}

impl AudioRing {
    pub const fn new() -> AudioRing {
        AudioRing { buf: [0; 32], write_pos: 0, read_pos: 0 }
    }
    pub fn frames_available(&self) -> usize {
        (self.write_pos + 32 - self.read_pos) % 32
    }
    pub fn push(&mut self, s: i16) -> bool {
        let next = (self.write_pos + 1) % 32;
        if next == self.read_pos {
            return false;
        }
        self.buf[self.write_pos] = s;
        self.write_pos = next;
        true
    }
    pub fn pop(&mut self) -> Option<i16> {
        if self.read_pos == self.write_pos {
            return None;
        }
        let v = self.buf[self.read_pos];
        self.read_pos = (self.read_pos + 1) % 32;
        Some(v)
    }
}

/// G344 混音器：多流求和 + 饱和裁剪。
pub fn mixer_sum(streams: &[[i16; 4]]) -> [i16; 4] {
    let mut out = [0i16; 4];
    for ch in 0..4 {
        let mut acc = 0i32;
        for st in streams {
            acc += st[ch] as i32;
        }
        out[ch] = acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }
    out
}

/// G346 软件合成器：方波/锯齿波/噪声。
pub fn synth_square(phase: u16, period: u16) -> i16 {
    if phase % period < period / 2 {
        8000
    } else {
        -8000
    }
}

pub fn synth_saw(phase: u16, period: u16) -> i16 {
    let t = (phase % period) as i32;
    ((t * 16000 / period as i32) - 8000) as i16
}

pub fn synth_noise(seed: &mut u32) -> i16 {
    // LCG
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    ((*seed >> 16) as i16 & 0x1FFF) - 4096
}

/// G347 MIDI 消息解析：status + data。
pub fn midi_parse(b: &[u8]) -> Option<(u8, u8, u8)> {
    let st = *b.first()?;
    if st & 0x80 == 0 {
        return None;
    }
    let kind = st & 0xF0;
    match kind {
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => {
            if b.len() < 3 {
                return None;
            }
            Some((kind, b[1], b[2]))
        }
        0xC0 | 0xD0 => Some((kind, *b.get(1)?, 0)),
        _ => None,
    }
}

pub fn midi_note_on(ch: u8, note: u8, vel: u8) -> [u8; 3] {
    [0x90 | (ch & 0x0F), note & 0x7F, vel & 0x7F]
}

/// G348 程序化声景：开机声序列（音符时间线）。
pub fn boot_chime() -> [(u8, u16); 4] {
    [(60, 0), (64, 120), (67, 240), (72, 360)] // (note, ms)
}

/// G349 音量与静音：0..100 音量，静音旁路。
pub fn apply_volume(sample: i16, volume_pct: u8, muted: bool) -> i16 {
    if muted {
        0
    } else {
        ((sample as i32 * volume_pct as i32) / 100).clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }
}

/// G351 低延迟预算：DMA 周期帧数换算延迟。
pub fn audio_latency_us(period_frames: u32, rate_hz: u32) -> u32 {
    if rate_hz == 0 {
        return u32::MAX;
    }
    period_frames * 1_000_000 / rate_hz
}

/// G355 音频设备热插拔：到达/移除事件。
pub fn audio_hotplug(card: u8, arrived: bool) -> u16 {
    ((card as u16) << 8) | arrived as u16
}

/// G356 降级：无声卡时静默（不崩溃）。
pub fn audio_degrade(device_present: bool) -> &'static str {
    if device_present {
        "hda"
    } else {
        "silent"
    }
}

// ---------------------------------------------------------------------------
// 自检与收口
// ---------------------------------------------------------------------------

/// AI-06 域自检：≤32 项覆盖三段。
pub fn run_periph_checks() -> CheckSet {
    let mut s = CheckSet::new("gperiph");
    let desc = [
        18, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x34, 0x12, 0x78, 0x56, 0x00, 0x01, 0x02, 0x03, 0x01, 0x01,
    ];
    let dd = parse_dev_desc(&desc);
    s.add("G301 dev desc", dd.is_some() && dd.unwrap().vendor == 0x1234 && dd.unwrap().usb_version == 0x0200, "18B");
    s.add("G301 bad desc", parse_dev_desc(&desc[..10]).is_none(), "short");
    let ep = [7u8, 0x05, 0x81, 0x03, 0x08, 0x08, 0x00];
    let epp = parse_ep_desc(&ep);
    s.add("G301 ep desc", epp == Some((1, true, 3, 8)), "in/int/8B");
    let trb = trb_encode(1, true, 0x1234_5678_9ABC_DEF0);
    s.add("G302 trb", trb_decode(&trb) == (1, true) && trb[0] == 0x9ABC_DEF0, "encode");
    let (idx, cy) = trb_ring_advance(7, 8, true);
    s.add("G302 ring wrap", idx == 0 && !cy, "cycle flip");
    s.add("G304 xfer sched", schedule_xfer(10, 12, XferType::Isoch) && !schedule_xfer(12, 12, XferType::Isoch) && schedule_xfer(12, 12, XferType::Bulk), "isoch budget");
    let rep = [HID_MOD_CTRL | HID_MOD_SHIFT, 0, 4, 5, 0, 0, 0, 0];
    let (mods, keys) = hid_keyboard_mods(&rep);
    s.add("G305 hid", mods == 0b11 && keys[0] == 4 && keys[1] == 5, "report");
    let cbw = msc_cbw_read10(1000, 2, 0x77);
    s.add("G306 msc", cbw[12] == 0x28 && cbw[0] == 0x77, "cbw");
    s.add("G308 usbnet", !usbnet_frame_ok(&[0u8; 12]) && usbnet_frame_ok(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x08, 0x00]), "ethertype");
    s.add("G309 hotplug", usb_hotplug_event(3, true) == 0x0301, "port event");
    let (conn, en, _susp, ovc) = hub_port_status(0b1011);
    s.add("G310 hub", conn && en && ovc, "status bits");
    s.add("G312 permission", usb_permission_ok(3, &[3, 8]) && !usb_permission_ok(9, &[3, 8]), "whitelist");
    s.add("G316 speed", usb_speed_name(2) == "high" && usb_speed_name(3) == "super" && usb_speed_name(9) == "unknown", "speed");
    s.add("G317 retry", usb_retry(5, 3) == 3 && usb_retry(2, 3) == 2, "bounded");
    let cfg = [0x04u8, 0x09, 0x04, 0x04];
    s.add("G315 composite", composite_interfaces(&cfg) == 3, "interfaces");
    // GPU 段
    s.add("G321 pixel", pixel_unpack(pixel_rgb(0x12, 0x34, 0x56)) == (0x12, 0x34, 0x56), "xrgb");
    let mut fb = Fb { px: [0; FB_W * FB_H] };
    let painted = raster_triangle(&mut fb, [(0, 0), (10, 0), (0, 10)], 0xFF0000FF);
    s.add("G322 raster", painted == 66 && fb.get(0, 0) == 0xFF0000FF && fb.get(10, 10) == 0, "triangle area≈66");
    fill_rect(&mut fb, 0, 0, 64, 64, 0);
    fill_rect(&mut fb, 2, 2, 4, 4, 0x00FF00FF);
    s.add("G323 fill", fb.get(3, 3) == 0x00FF00FF && fb.get(1, 1) == 0, "rect");
    let cmds = [CMD_FILL, 0, 0, 4, 4, 9, CMD_FLIP, CMD_TRI, 0, 0, 5, 5, 9, 9];
    let (ops, cn) = cmd_decode(&cmds);
    s.add("G324 cmd buffer", cn == 3 && ops[0] == CMD_FILL && ops[1] == CMD_FLIP && ops[2] == CMD_TRI, "decode");
    s.add("G329 degrade chain", gpu_degrade(false, true) == "virtio" && gpu_degrade(false, false) == "softpipe" && gpu_degrade(true, false) == "real", "3 tiers");
    let mut vram = Vram::new();
    let b0 = vram.alloc();
    let b1 = vram.alloc();
    vram.release(b0.unwrap_or(0));
    let b2 = vram.alloc();
    s.add("G327 vram", b1 == Some(1) && b2 == Some(0) && vram.used == 2, "alloc/reuse");
    let m = dirty_rects_merge((0, 0, 10, 10), (5, 5, 10, 10));
    s.add("G335 dirty merge", m == (0, 0, 15, 15), "bounding");
    s.add("G339 idle clock", gpu_idle_clock(2000, 800) == 100 && gpu_idle_clock(50, 800) == 800, "dvfs");
    // 音频段
    let v = hda_verb(0, 2, HDA_VERB_GET_PARAM, 0x0A);
    s.add("G341 hda verb", hda_verb_unpack(v) == (0, 2, HDA_VERB_GET_PARAM, 0x0A), "pack/unpack");
    let mut ar = AudioRing::new();
    ar.push(100);
    ar.push(-100);
    s.add("G343 audio ring", ar.frames_available() == 2 && ar.pop() == Some(100) && ar.pop() == Some(-100) && ar.pop().is_none(), "fifo");
    let mix = mixer_sum(&[[1000, -1000, 0, 0], [500, 500, 0, 0]]);
    s.add("G344 mixer", mix[0] == 1500 && mix[1] == -500 && mix[2] == 0, "sum");
    s.add("G346 square", synth_square(0, 4) == 8000 && synth_square(2, 4) == -8000, "waveform");
    s.add("G346 saw", synth_saw(0, 8) == -8000 && synth_saw(4, 8) == 0, "ramp");
    let mut seed = 42u32;
    let n1 = synth_noise(&mut seed);
    let n2 = synth_noise(&mut seed);
    s.add("G346 noise", n1 != n2, "lcg");
    let msg = midi_note_on(0, 60, 100);
    s.add("G347 midi", midi_parse(&msg) == Some((0x90, 60, 100)) && midi_parse(&[0x30]).is_none(), "note on");
    s.add("G348 chime", boot_chime()[3] == (72, 360), "timeline");
    s.add("G349 volume", apply_volume(100, 50, false) == 50 && apply_volume(100, 100, true) == 0, "vol/mute");
    s.add("G351 latency", audio_latency_us(48, 48000) == 1000 && audio_latency_us(48, 0) == u32::MAX, "period");
    s.add("G356 degrade", audio_degrade(false) == "silent" && audio_degrade(true) == "hda", "silent fallback");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g322_triangle_area_matches_pixels() {
        let mut fb = Fb { px: [0; FB_W * FB_H] };
        let n = raster_triangle(&mut fb, [(0, 0), (20, 0), (0, 20)], 1);
        assert_eq!(n, 231, "半开栅格下 (0,0)~(20,20) 三角形像素数");
        assert_eq!(fb.get(19, 0), 1);
        assert_eq!(fb.get(0, 19), 1);
        assert_eq!(fb.get(11, 11), 0);
    }

    #[test]
    fn g341_verb_roundtrip_all_fields() {
        for cad in [0u8, 3, 15] {
            for nid in [0u8, 42, 255] {
                let w = hda_verb(cad, nid, HDA_VERB_SET_AMP, 0x7F);
                assert_eq!(hda_verb_unpack(w), (cad, nid, HDA_VERB_SET_AMP, 0x7F));
            }
        }
    }

    #[test]
    fn g343_ring_wraparound() {
        let mut ar = AudioRing::new();
        for i in 0..31 {
            assert!(ar.push(i as i16));
        }
        assert!(!ar.push(99), "full");
        for i in 0..31 {
            assert_eq!(ar.pop(), Some(i as i16));
        }
        assert_eq!(ar.pop(), None);
    }

    #[test]
    fn g344_mixer_saturates() {
        let big = [i16::MAX; 4];
        let out = mixer_sum(&[big, big]);
        assert_eq!(out[0], i16::MAX, "clamped");
    }

    #[test]
    fn g347_midi_parsing() {
        assert!(midi_parse(&[0xF0]).is_none(), "system message rejected by 2-arg paths");
        assert_eq!(midi_parse(&[0xC0, 5]), Some((0xC0, 5, 0)), "program change 1 data byte");
        assert_eq!(midi_parse(&[0x80, 60, 0]), Some((0x80, 60, 0)), "note off");
    }

    #[test]
    fn g360_domain_selftest_all_green() {
        let s = run_periph_checks();
        if !s.all_passed() {
            let mut buf = [0u8; 2048];
            let n = s.render(&mut buf);
            panic!("domain self-test must pass://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(s.len() >= 25);
    }
}
