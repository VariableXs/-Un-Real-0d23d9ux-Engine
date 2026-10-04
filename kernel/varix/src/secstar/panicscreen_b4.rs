//! F173 panic 画面设计 · 批次四深化（secstar · G-G-03）。
//!
//! 批次四功能面（与批次三互补：批次三管「版面与倒计时」，本批管
//! 「码与统计」）：
//! - [`PanicCode`]：错误码编解码——`VX-PANIC-<模块>-<序号>` 格式双向
//!   （模块表 8 项、序号 3 位定宽；解析失败诚实拒——码是协议不是装饰）；
//! - [`SerialMirror`]：串口镜像行缓冲——渲染前串口先行（日志优先纪律：
//!   图形挂了串口也在，镜像行数有账）；
//! - [`PanicStats`]：百次演练统计——渲染成功率 / 降级次数 / 码解析
//!   失败数三数（百次演练 100% 判据的统计面）；
//! - [`qr_payload`]：二维码载荷打包——版本头+错误码+故障地址+校验和
//!   定长帧（扫码即得结构化信息，不是一坨十六进制）。
//!
//! 零堆纪律：定长行缓冲 + 定长统计，无 alloc。

use super::panicscreen::{DUMP_CAP_BYTES, REBOOT_COUNTDOWN_MS, SHATTER_FRAMES};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 错误码编解码（VX-PANIC-<模块>-<序号>）
// ---------------------------------------------------------------------------

/// 已登记模块表（8 项——码的命名空间是封闭的，不认开放字符串）。
pub const MODULES: [&str; 8] = ["GFX", "SCHED", "MEM", "STORE", "NET", "INPUT", "POWER", "BOOT"];

/// 序号定宽 3 位（001-999）。
pub const SEQ_WIDTH: usize = 3;

/// 解析后的码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanicCode {
    pub module_idx: usize,
    pub seq: u16,
}

/// 错误码错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    BadPrefix,
    UnknownModule,
    BadSeq,
}

/// 编码：模块名 + 序号 → 定宽字符串缓冲。
pub fn encode_code(module: &str, seq: u16, out: &mut [u8]) -> Result<usize, CodeError> {
    let module_idx = MODULES.iter().position(|x| *x == module).ok_or(CodeError::UnknownModule)?;
    if seq == 0 || seq > 999 {
        return Err(CodeError::BadSeq);
    }
    let prefix = b"VX-PANIC-";
    if out.len() < prefix.len() + module.len() + 1 + SEQ_WIDTH {
        return Err(CodeError::BadPrefix);
    }
    let mut n = 0;
    for b in prefix {
        out[n] = *b;
        n += 1;
    }
    for b in MODULES[module_idx].as_bytes() {
        out[n] = *b;
        n += 1;
    }
    out[n] = b'-';
    n += 1;
    // 定宽 3 位（001 填零——排序稳定）。
    out[n] = b'0' + (seq / 100) as u8;
    out[n + 1] = b'0' + (seq / 10 % 10) as u8;
    out[n + 2] = b'0' + (seq % 10) as u8;
    n += SEQ_WIDTH;
    Ok(n)
}

/// 解码：字节面 → 结构化码（前缀/模块/序号三关全过才放行）。
pub fn decode_code(text: &[u8]) -> Result<PanicCode, CodeError> {
    let prefix = b"VX-PANIC-";
    if text.len() < prefix.len() + 2 || &text[..prefix.len()] != prefix {
        return Err(CodeError::BadPrefix);
    }
    let rest = &text[prefix.len()..];
    // 模块名：最长的 '-' 前段。
    let dash = rest.iter().position(|b| *b == b'-').ok_or(CodeError::UnknownModule)?;
    let module = &rest[..dash];
    let idx = MODULES.iter().position(|m| m.as_bytes() == module).ok_or(CodeError::UnknownModule)?;
    let seq_bytes = &rest[dash + 1..];
    if seq_bytes.len() != SEQ_WIDTH || !seq_bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(CodeError::BadSeq);
    }
    let seq = (seq_bytes[0] - b'0') as u16 * 100 + (seq_bytes[1] - b'0') as u16 * 10 + (seq_bytes[2] - b'0') as u16;
    if seq == 0 {
        return Err(CodeError::BadSeq);
    }
    Ok(PanicCode { module_idx: idx, seq })
}

// ---------------------------------------------------------------------------
// 串口镜像行缓冲
// ---------------------------------------------------------------------------

/// 镜像行上限。
pub const MIRROR_CAP: usize = 24;

pub struct SerialMirror {
    lines: [[u8; 80]; MIRROR_CAP],
    lens: [usize; MIRROR_CAP],
    pub n: usize,
    pub dropped: u32,
}

impl SerialMirror {
    pub const fn new() -> SerialMirror {
        SerialMirror { lines: [[0; 80]; MIRROR_CAP], lens: [0; MIRROR_CAP], n: 0, dropped: 0 }
    }

    /// 镜像一行（超 80 截断；超 24 行丢弃计数——串口面不阻塞不膨胀）。
    pub fn mirror(&mut self, line: &[u8]) {
        if self.n >= MIRROR_CAP {
            self.dropped += 1;
            return;
        }
        let take = line.len().min(80);
        self.lines[self.n][..take].copy_from_slice(&line[..take]);
        self.lens[self.n] = take;
        self.n += 1;
    }

    /// 第 i 行切片。
    pub fn line(&self, i: usize) -> &[u8] {
        &self.lines[i][..self.lens[i]]
    }
}

// ---------------------------------------------------------------------------
// 百次演练统计
// ---------------------------------------------------------------------------

/// 演练统计（三数——百次演练 100% 判据的账面）。
#[derive(Clone, Copy, Debug, Default)]
pub struct PanicStats {
    pub drills: u32,
    pub rendered: u32,
    pub degraded: u32,
    pub code_parse_fail: u32,
}

impl PanicStats {
    pub fn record_render(&mut self, ok: bool) {
        self.drills += 1;
        if ok {
            self.rendered += 1;
        }
    }

    pub fn record_degraded(&mut self) {
        self.degraded += 1;
    }

    pub fn record_parse_fail(&mut self) {
        self.code_parse_fail += 1;
    }

    /// 渲染成功率 ‰（ drills=0 → 0，不编造）。
    pub fn render_rate_permille(&self) -> u32 {
        if self.drills == 0 {
            return 0;
        }
        (self.rendered * 1_000 / self.drills) as u32
    }

    /// 主册判据直算：百次演练渲染成功率 100%。
    pub fn hundred_drills_perfect(&self) -> bool {
        self.drills >= 100 && self.rendered == self.drills
    }
}

// ---------------------------------------------------------------------------
// 二维码载荷打包
// ---------------------------------------------------------------------------

/// 载荷帧：[0] 版本 · [1..3) 序号 LE · [3..7) 故障地址低 32b LE ·
/// [7] 模块 idx · [8..10) 校验和（FNV-16 折叠前 8B）。
pub const QR_PAYLOAD_LEN: usize = 10;

fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811c9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    ((h >> 16) ^ h) as u16
}

pub fn qr_payload_encode(module_idx: u8, seq: u16, addr_low32: u32, out: &mut [u8; QR_PAYLOAD_LEN]) -> bool {
    if module_idx as usize >= MODULES.len() || seq == 0 {
        return false;
    }
    out[0] = 1; // 载荷版本
    out[1..3].copy_from_slice(&seq.to_le_bytes());
    out[3..7].copy_from_slice(&addr_low32.to_le_bytes());
    out[7] = module_idx;
    let c = fnv16(&out[..8]);
    out[8] = (c & 0xFF) as u8;
    out[9] = (c >> 8) as u8;
    true
}

pub fn qr_payload_decode(frame: &[u8; QR_PAYLOAD_LEN]) -> Option<(u8, u16, u32)> {
    if frame[0] != 1 {
        return None;
    }
    let want = (frame[9] as u16) << 8 | frame[8] as u16;
    if fnv16(&frame[..8]) != want {
        return None;
    }
    Some((frame[7], u16::from_le_bytes(frame[1..3].try_into().ok()?), u32::from_le_bytes(frame[3..7].try_into().ok()?)))
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_panicscreen_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-b4");

    // 1) 码编码：GFX-007 → 定宽 17B 逐字节对（填零排序稳定）。
    let mut buf = [0u8; 32];
    let n = encode_code("GFX", 7, &mut buf).unwrap();
    cs.add(
        "code_encode_exact",
        n == 16 && &buf[..n] == b"VX-PANIC-GFX-007",
        "",
    );

    // 2) 码解码 round-trip：全 8 模块 × 边界序号（1/500/999）全对。
    let mut all = true;
    for (mi, m) in MODULES.iter().enumerate() {
        for seq in [1u16, 500, 999] {
            let n = encode_code(m, seq, &mut buf).unwrap();
            let dec = decode_code(&buf[..n]).unwrap();
            all &= dec == PanicCode { module_idx: mi, seq };
        }
    }
    cs.add("code_roundtrip_all_modules", all, "");

    // 3) 码三拒：坏前缀/未登记模块/序号 0（协议面不开放）。
    let mut n = encode_code("GFX", 1, &mut buf).unwrap();
    buf[0] = b'X';
    let bad_prefix = decode_code(&buf[..n]) == Err(CodeError::BadPrefix);
    let bad_module = encode_code("NOPE", 1, &mut buf).is_err();
    let bad_seq = encode_code("GFX", 0, &mut buf).is_err();
    n = encode_code("GFX", 1, &mut buf).unwrap();
    let short = decode_code(&buf[..n - 1]) == Err(CodeError::BadSeq);
    cs.add("code_three_rejects", bad_prefix && bad_module && bad_seq && short, "");

    // 4) 序号边界拒：1000 拒（3 位定宽上限）。
    cs.add("code_seq_cap", encode_code("GFX", 1_000, &mut buf).is_err(), "");

    // 5) 串口镜像：行进账、超宽截断 80、超容丢弃计数（三面全对）。
    let mut m = SerialMirror::new();
    m.mirror(b"panic: store error at 0xDEAD");
    m.mirror(&[b'x'; 200]);
    cs.add(
        "mirror_lines",
        m.n == 2 && m.line(0) == b"panic: store error at 0xDEAD" && m.line(1).len() == 80,
        "",
    );

    // 6) 串口镜像满容：24 行后丢弃计数（不阻塞不膨胀）。
    let mut m2 = SerialMirror::new();
    for i in 0..(MIRROR_CAP + 5) as u64 {
        let mut line = [0u8; 4];
        line[0] = b'0' + (i % 10) as u8;
        m2.mirror(&line);
    }
    cs.add("mirror_overflow_counted", m2.n == MIRROR_CAP && m2.dropped == 5, "");

    // 7) 百次演练：100 次全渲染 → 成功率 1000‰ + 判据直算绿。
    let mut st = PanicStats::default();
    for _ in 0..100 {
        st.record_render(true);
    }
    cs.add("stats_hundred_perfect", st.hundred_drills_perfect() && st.render_rate_permille() == 1_000, "");

    // 8) 统计反面：一次降级 → 成功率 990‰（降级不冒充渲染成功）。
    let mut st2 = PanicStats::default();
    for _ in 0..99 {
        st2.record_render(true);
    }
    st2.record_render(false);
    st2.record_degraded();
    cs.add("stats_degradation_visible", st2.render_rate_permille() == 990 && st2.degraded == 1, "");

    // 9) 统计零次不编造：drills=0 → 成功率 0（不冒充 100%）。
    cs.add("stats_zero_honest", PanicStats::default().render_rate_permille() == 0 && !PanicStats::default().hundred_drills_perfect(), "");

    // 10) 载荷 round-trip：编解码全字段保真（版本/序号/地址/模块）。
    let mut frame = [0u8; QR_PAYLOAD_LEN];
    let enc = qr_payload_encode(2, 42, 0xDEADBEEF, &mut frame);
    cs.add(
        "qr_payload_roundtrip",
        enc && qr_payload_decode(&frame) == Some((2, 42, 0xDEADBEEF)),
        "",
    );

    // 11) 载荷撕裂必拒：任一字节翻转 → 校验和关拦（10 字节逐一）。
    let mut torn_all = true;
    for i in 0..QR_PAYLOAD_LEN {
        let mut t = frame;
        t[i] ^= 0x5A;
        torn_all &= qr_payload_decode(&t).is_none() || t == frame;
    }
    cs.add("qr_payload_tears", torn_all, "");

    // 12) 载荷范围拒：模块 idx 越界 / 序号 0 不出门。
    cs.add(
        "qr_payload_range",
        !qr_payload_encode(8, 1, 0, &mut frame) && !qr_payload_encode(0, 0, 0, &mut frame),
        "",
    );

    // 13) 主册常量贯通：30s 倒计时 / 3 碎裂帧 / 4MB dump 一处一事实。
    cs.add("consts_aligned", REBOOT_COUNTDOWN_MS == 30_000 && SHATTER_FRAMES == 3 && DUMP_CAP_BYTES == 4 << 20, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn code_sort_stability() {
        // 定宽填零 → 字节序即数值序（007 < 010 < 100——日志排序不依赖解析）。
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        let mut c = [0u8; 32];
        let na = encode_code("MEM", 7, &mut a).unwrap();
        let nb = encode_code("MEM", 10, &mut b).unwrap();
        let nc = encode_code("MEM", 100, &mut c).unwrap();
        assert!(a[..na] < b[..nb] && b[..nb] < c[..nc]);
    }

    #[test]
    fn mirror_no_partial_lines() {
        // 截断行仍是行界完整字节序列（不劈 UTF-8 场景由上层保证——
        // 本层保证截断只发生在 80 界、不挪字节）。
        let mut m = SerialMirror::new();
        m.mirror(b"short");
        // 恰 80 字节：8×'a' + 64×'p' + 8×'9'。
        let mut eighty = [0u8; 80];
        eighty[..8].fill(b'a');
        eighty[8..72].fill(b'p');
        eighty[72..].fill(b'9');
        m.mirror(&eighty);
        assert_eq!(m.line(1).len(), 80);
        assert_eq!(m.line(0), b"short");
    }

    #[test]
    fn payload_survives_roundtrip_zero_addr() {
        // 地址 0 是合法值（全零帧 != 无效帧——版本位才是有效性锚）。
        let mut frame = [0u8; QR_PAYLOAD_LEN];
        assert!(qr_payload_encode(0, 1, 0, &mut frame));
        assert_eq!(qr_payload_decode(&frame), Some((0, 1, 0)));
    }
}
