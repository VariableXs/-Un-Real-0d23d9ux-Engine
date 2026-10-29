//! UNX-F48470 · E·半包/粘包边界（正：流式解析重组正确）
//!
//! 判据原文：UNX-F48470-J1 —— 流式字节流按任意切块边界喂入后，帧重组结果与
//! 原帧序列逐帧一致（长度+载荷指纹）；半包、粘包、混合切块、空喂四种边界
//! 均不丢帧、不重帧、不错位；超长帧与零长帧走独立错误码且进入终态后拒绝
//! 后续喂入；多解析器实例状态互不串扰。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；帧头 3 字节
//! 大端长度；超长判定以 MAX_FRAME 常量为准；fuzz 切块长度由确定性 LCG
//! 驱动（wrapping_mul/wrapping_add），同种子同序列，可复现。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE360;

pub const E_LEN_ZERO: u16 = SEG_BASE + 1;
pub const E_LEN_OVER: u16 = SEG_BASE + 2;
pub const E_DEAD: u16 = SEG_BASE + 3;
pub const E_STUCK: u16 = SEG_BASE + 4;
pub const E_MISMATCH: u16 = SEG_BASE + 5;
pub const E_DUP: u16 = SEG_BASE + 6;
pub const E_LOST: u16 = SEG_BASE + 7;
pub const E_CROSS: u16 = SEG_BASE + 8;

/// 单帧长度头上限（3 字节可表达 0xFFFFFF，但协议面收紧到 4096）。
pub const MAX_FRAME: usize = 4096;
/// 帧头字节数。
pub const HEAD_LEN: usize = 3;
/// 单解析器内部缓冲上限，防御性收口。
pub const BUF_CAP: usize = MAX_FRAME + HEAD_LEN;

/// 重组出的完整帧。
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(payload: Vec<u8>) -> Frame {
        Frame { payload }
    }

    /// FNV-1a 64 位载荷指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.payload.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 编码为线格式：3 字节大端长度 + 载荷。
    pub fn encode(&self) -> Vec<u8> {
        let n = self.payload.len();
        let mut out = Vec::with_capacity(n + HEAD_LEN);
        out.push(((n >> 16) & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push((n & 0xFF) as u8);
        for b in self.payload.iter() {
            out.push(*b);
        }
        out
    }
}

/// 解析器状态机相位。
#[derive(Clone, PartialEq, Eq)]
pub enum Phase {
    /// 正在攒帧头，need 为还差几字节。
    Head(usize),
    /// 正在攒载荷，need 为还差几字节。
    Body(usize),
    /// 终态：遇到不可恢复错误，拒绝一切后续喂入。
    Dead,
}

/// 流式帧解析器：半包/粘包无关。
pub struct StreamSplitter {
    phase: Phase,
    buf: Vec<u8>,
    body_need: usize,
    pub frames_out: u32,
    pub bytes_in: u64,
    pub err_zero: u32,
    pub err_over: u32,
}

impl StreamSplitter {
    pub fn new() -> StreamSplitter {
        StreamSplitter {
            phase: Phase::Head(HEAD_LEN),
            buf: Vec::new(),
            body_need: 0,
            frames_out: 0,
            bytes_in: 0,
            err_zero: 0,
            err_over: 0,
        }
    }

    pub fn is_dead(&self) -> bool {
        self.phase == Phase::Dead
    }

    /// 喂入一段字节；吐出的帧追加到 out。返回本次吐出的帧数。
    /// 半包/粘包/混合切块在此统一消化：先攒头、读长度、再攒体。
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<Frame>) -> Result<usize, u16> {
        if self.is_dead() {
            return Err(E_DEAD);
        }
        let mut produced: usize = 0;
        let mut idx: usize = 0;
        while idx < bytes.len() {
            if self.is_dead() {
                break;
            }
            match self.phase {
                Phase::Dead => break,
                Phase::Head(need) => {
                    let take = if need < bytes.len() - idx { need } else { bytes.len() - idx };
                    for k in 0..take {
                        if self.buf.len() >= BUF_CAP {
                            self.phase = Phase::Dead;
                            return Err(E_STUCK);
                        }
                        self.buf.push(bytes[idx + k]);
                    }
                    idx += take;
                    self.bytes_in += take as u64;
                    let left = need - take;
                    if left == 0 {
                        // 头已齐：解出 24 位大端长度。
                        let n = ((self.buf[0] as usize) << 16)
                            | ((self.buf[1] as usize) << 8)
                            | (self.buf[2] as usize);
                        self.buf.clear();
                        if n == 0 {
                            self.err_zero += 1;
                            self.phase = Phase::Dead;
                            return Err(E_LEN_ZERO);
                        }
                        if n > MAX_FRAME {
                            self.err_over += 1;
                            self.phase = Phase::Dead;
                            return Err(E_LEN_OVER);
                        }
                        self.body_need = n;
                        self.phase = Phase::Body(n);
                    } else {
                        self.phase = Phase::Head(left);
                    }
                }
                Phase::Body(need) => {
                    let take = if need < bytes.len() - idx { need } else { bytes.len() - idx };
                    for k in 0..take {
                        if self.buf.len() >= BUF_CAP {
                            self.phase = Phase::Dead;
                            return Err(E_STUCK);
                        }
                        self.buf.push(bytes[idx + k]);
                    }
                    idx += take;
                    self.bytes_in += take as u64;
                    let left = need - take;
                    if left == 0 {
                        let payload = self.buf.clone();
                        self.buf.clear();
                        out.push(Frame::new(payload));
                        produced += 1;
                        self.frames_out += 1;
                        self.phase = Phase::Head(HEAD_LEN);
                    } else {
                        self.phase = Phase::Body(left);
                    }
                }
            }
        }
        Ok(produced)
    }

    /// 待决字节数（诊断面：头残差 + 体残差）。
    pub fn pending(&self) -> usize {
        match self.phase {
            Phase::Head(n) => n,
            Phase::Body(n) => n + self.buf.len(),
            Phase::Dead => 0,
        }
    }
}

/// 确定性 LCG（与全批 fuzz 惯例一致）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

/// 构造 n 帧确定性载荷（长度 1..=64，内容由种子派生）。
pub fn build_frames(n: usize, seed: u64) -> Vec<Frame> {
    let mut g = Lcg::new(seed);
    let mut out = Vec::new();
    for _ in 0..n {
        let len = 1 + g.below(64) as usize;
        let mut p = Vec::with_capacity(len);
        for _ in 0..len {
            p.push((g.next() & 0xFF) as u8);
        }
        out.push(Frame::new(p));
    }
    out
}

/// 帧序列整流编码：全部帧 encode 后顺序拼接。
pub fn wire_of(frames: &[Frame]) -> Vec<u8> {
    let mut w = Vec::new();
    for f in frames.iter() {
        let enc = f.encode();
        for b in enc.iter() {
            w.push(*b);
        }
    }
    w
}

/// 重组结果与原序列比对：数量、逐帧指纹。
pub fn compare(origin: &[Frame], got: &[Frame]) -> Result<(), u16> {
    if got.len() != origin.len() {
        return Err(E_LOST);
    }
    for i in 0..origin.len() {
        if origin[i].fp() != got[i].fp() {
            return Err(E_MISMATCH);
        }
        if i > 0 && got[i] == got[i - 1] && origin[i].fp() != origin[i - 1].fp() {
            return Err(E_DUP);
        }
    }
    Ok(())
}

/// 序列指纹：帧数与逐帧 FNV 链式混合。
pub fn seq_fp(frames: &[Frame]) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= frames.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for f in frames.iter() {
        h ^= f.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// R1 整帧一次喂入：单帧 encode 后整段喂，一次吐出。
pub fn r1_whole_single() -> Result<(), u16> {
    let f = Frame::new({
        let mut p = Vec::new();
        for k in 0..16u32 {
            p.push((0xA0 + k) as u8);
        }
        p
    });
    let wire = f.encode();
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    sp.feed(&wire, &mut out)?;
    if out.len() != 1 {
        return Err(E_LOST);
    }
    if out[0].fp() != f.fp() {
        return Err(E_MISMATCH);
    }
    if sp.pending() != 0 {
        return Err(E_STUCK);
    }
    Ok(())
}

/// R2 半包：逐字节喂入，重组与原序一致。
pub fn r2_byte_by_byte() -> Result<(), u16> {
    let origin = build_frames(6, 0x48470_0001);
    let wire = wire_of(&origin);
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    for b in wire.iter() {
        let one = [*b];
        sp.feed(&one, &mut out)?;
    }
    compare(&origin, &out)
}

/// R3 粘包：三帧拼接一次喂，三帧全出且内容不错位。
pub fn r3_sticky_three() -> Result<(), u16> {
    let origin = build_frames(3, 0x48470_0002);
    let wire = wire_of(&origin);
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    sp.feed(&wire, &mut out)?;
    compare(&origin, &out)?;
    if sp.frames_out != 3 {
        return Err(E_DUP);
    }
    Ok(())
}

/// R4 混合切块：LCG 驱动 1..=7 字节随机块，重组一致。
pub fn r4_mixed_chunks() -> Result<(), u16> {
    let origin = build_frames(12, 0x48470_0003);
    let wire = wire_of(&origin);
    let mut g = Lcg::new(0x48470_0004);
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    let mut pos: usize = 0;
    while pos < wire.len() {
        let step = 1 + g.below(7) as usize;
        let end = if pos + step > wire.len() { wire.len() } else { pos + step };
        let mut chunk = Vec::new();
        for k in pos..end {
            chunk.push(wire[k]);
        }
        sp.feed(&chunk, &mut out)?;
        pos = end;
    }
    compare(&origin, &out)
}

/// R5 空喂：零字节喂入状态保持、无输出、不报错。
pub fn r5_empty_feed() -> Result<(), u16> {
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    for _ in 0..5 {
        sp.feed(&[], &mut out)?;
    }
    if out.len() != 0 {
        return Err(E_DUP);
    }
    if sp.frames_out != 0 || sp.bytes_in != 0 {
        return Err(E_CROSS);
    }
    if sp.pending() != HEAD_LEN {
        return Err(E_STUCK);
    }
    Ok(())
}

/// R6 超长帧：长度头越过 MAX_FRAME → E_LEN_OVER，且终态后拒收。
pub fn r6_overlong_reject() -> Result<(), u16> {
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    // 构造长度 0x00FF01（远超 MAX_FRAME=4096）。
    let head = [0x00u8, 0xFF, 0x01, 0x11, 0x22];
    let r = sp.feed(&head, &mut out);
    match r {
        Err(E_LEN_OVER) => {}
        _ => return Err(E_LEN_OVER),
    }
    if !sp.is_dead() {
        return Err(E_STUCK);
    }
    // 终态后任何喂入都必须拒绝。
    let r2 = sp.feed(&[0x01u8], &mut out);
    match r2 {
        Err(E_DEAD) => {}
        _ => return Err(E_DEAD),
    }
    if sp.err_over != 1 {
        return Err(E_LEN_OVER);
    }
    Ok(())
}

/// R7 零长帧：长度头为 0 → E_LEN_ZERO，终态拒收。
pub fn r7_zero_len_reject() -> Result<(), u16> {
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    let r = sp.feed(&[0x00u8, 0x00, 0x00], &mut out);
    match r {
        Err(E_LEN_ZERO) => {}
        _ => return Err(E_LEN_ZERO),
    }
    if !sp.is_dead() {
        return Err(E_STUCK);
    }
    let r2 = sp.feed(&[0x01u8, 0x02], &mut out);
    match r2 {
        Err(E_DEAD) => {}
        _ => return Err(E_DEAD),
    }
    if sp.err_zero != 1 {
        return Err(E_LEN_ZERO);
    }
    Ok(())
}

/// R8 序列指纹守恒：随机切块重组后的整序指纹与直算指纹一致。
pub fn r8_seq_fp_conserved() -> Result<(), u16> {
    let origin = build_frames(20, 0x48470_0005);
    let direct = seq_fp(&origin);
    let wire = wire_of(&origin);
    let mut g = Lcg::new(0x48470_0006);
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    let mut pos: usize = 0;
    while pos < wire.len() {
        let step = 1 + g.below(5) as usize;
        let end = if pos + step > wire.len() { wire.len() } else { pos + step };
        let mut chunk = Vec::new();
        for k in pos..end {
            chunk.push(wire[k]);
        }
        sp.feed(&chunk, &mut out)?;
        pos = end;
    }
    if seq_fp(&out) != direct {
        return Err(E_MISMATCH);
    }
    Ok(())
}

/// R9 多实例独立：两个解析器并行喂不同流，互不串扰。
pub fn r9_dual_instance() -> Result<(), u16> {
    let a = build_frames(4, 0x48470_0007);
    let b = build_frames(4, 0x48470_0008);
    let wa = wire_of(&a);
    let wb = wire_of(&b);
    let mut sa = StreamSplitter::new();
    let mut sb = StreamSplitter::new();
    let mut oa: Vec<Frame> = Vec::new();
    let mut ob: Vec<Frame> = Vec::new();
    // 交错喂入：每轮各喂 3 字节。
    let (mut pa, mut pb) = (0usize, 0usize);
    while pa < wa.len() || pb < wb.len() {
        if pa < wa.len() {
            let end = if pa + 3 > wa.len() { wa.len() } else { pa + 3 };
            let mut c = Vec::new();
            for k in pa..end {
                c.push(wa[k]);
            }
            sa.feed(&c, &mut oa)?;
            pa = end;
        }
        if pb < wb.len() {
            let end = if pb + 3 > wb.len() { wb.len() } else { pb + 3 };
            let mut c = Vec::new();
            for k in pb..end {
                c.push(wb[k]);
            }
            sb.feed(&c, &mut ob)?;
            pb = end;
        }
    }
    compare(&a, &oa)?;
    compare(&b, &ob)?;
    if sa.frames_out != 4 || sb.frames_out != 4 {
        return Err(E_CROSS);
    }
    Ok(())
}

/// R10 吞吐回归：200 帧混合切块流式吞吐，帧计数与字节计数守恒。
pub fn r10_throughput() -> Result<(), u16> {
    let origin = build_frames(200, 0x48470_0009);
    let wire = wire_of(&origin);
    let mut g = Lcg::new(0x48470_000A);
    let mut sp = StreamSplitter::new();
    let mut out: Vec<Frame> = Vec::new();
    let mut pos: usize = 0;
    while pos < wire.len() {
        let step = 1 + g.below(9) as usize;
        let end = if pos + step > wire.len() { wire.len() } else { pos + step };
        let mut chunk = Vec::new();
        for k in pos..end {
            chunk.push(wire[k]);
        }
        sp.feed(&chunk, &mut out)?;
        pos = end;
    }
    compare(&origin, &out)?;
    if sp.frames_out != 200 {
        return Err(E_LOST);
    }
    if sp.bytes_in != wire.len() as u64 {
        return Err(E_CROSS);
    }
    Ok(())
}

/// 判据主入口：R1..R10 全场景，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_whole_single()?;
    hits += 1;
    r2_byte_by_byte()?;
    hits += 1;
    r3_sticky_three()?;
    hits += 1;
    r4_mixed_chunks()?;
    hits += 1;
    r5_empty_feed()?;
    hits += 1;
    r6_overlong_reject()?;
    hits += 1;
    r7_zero_len_reject()?;
    hits += 1;
    r8_seq_fp_conserved()?;
    hits += 1;
    r9_dual_instance()?;
    hits += 1;
    r10_throughput()?;
    hits += 1;
    Ok(hits)
}
