//! UNX-F48452 —— E·fcitx5 流半包注入（反：截断 socket 流 ×10 重同步成功）
//! 判据原文：UNX-F48452-J1 socket 流按 MAGIC+LEN+载荷 组帧；任意切点截断
//!           （切在 MAGIC 中/LEN 中/载荷中/粘包/半包跨界 ×10 形态）注入后
//!           组帧器必须重同步成功且零丢包零崩；
//!           随机切点 ×512 组装结果与原始消息序列逐条等价；
//!           重同步账四型封闭；字节守恒；重演指纹一致；
//!           垃圾段滑动丢弃后合法流恢复。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::String;

pub const SEG_BASE: u16 = 0xDD40; // E23 fcitx5 流半包注入段

const E_RESYNC_DEAD: u16 = SEG_BASE + 1; // 重同步失败
const E_MSG_LOST: u16 = SEG_BASE + 2; // 消息丢失
const E_MSG_CORRUPT: u16 = SEG_BASE + 3; // 消息内容失真
const E_KIND_BAD: u16 = SEG_BASE + 4; // 重同步账形态越界
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机切点崩溃
const E_JUNK_PASS: u16 = SEG_BASE + 6; // 垃圾段被放行
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 字节守恒破

pub const MAGIC: [u8; 2] = [0xA5, 0x5A];
pub const MAX_PAYLOAD: usize = 64;

/// 重同步型（四型封闭）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ResyncKind {
    InMagic = 0,    // 切在 MAGIC 两字节之间（悬置待后续半包）
    InLen = 1,      // 切在 LEN 两字节之间
    InBody = 2,     // 切在载荷中
    CrossFrame = 3, // 粘包跨界：单次投递完成 ≥1 帧且余量残留
}

/// 组帧事件。
pub struct FrameEvent {
    pub kind: ResyncKind,
    pub bytes: usize,
}

/// 组帧步进结果（内部）。
enum Step {
    Complete(usize, Vec<u8>), // 消费字节数 + 载荷
    Suspend(ResyncKind),      // 半包悬置
    Junk,                     // 垃圾头滑动丢弃
    Empty,                    // 缓冲已空
}

/// 流组帧器：容错重同步。
pub struct StreamFramer {
    buf: Vec<u8>,
    pub messages: Vec<Vec<u8>>,
    pub events: Vec<FrameEvent>,
    pub junk_dropped: usize,
    pub fed_total: usize,
}

impl StreamFramer {
    pub fn new() -> Self {
        StreamFramer {
            buf: Vec::new(),
            messages: Vec::new(),
            events: Vec::new(),
            junk_dropped: 0,
            fed_total: 0,
        }
    }

    /// 编码一帧：MAGIC + LEN(u16 LE) + 载荷。
    pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
        let n = payload.len();
        let mut out: Vec<u8> = Vec::new();
        out.push(MAGIC[0]);
        out.push(MAGIC[1]);
        out.push((n & 0xFF) as u8);
        out.push((n >> 8) as u8);
        let mut i = 0usize;
        while i < n {
            out.push(payload[i]);
            i += 1;
        }
        out
    }

    /// 投递任意字节块（半包/粘包/垃圾均可），返回本次重组出的消息数。
    pub fn feed(&mut self, bytes: &[u8]) -> Result<usize, u16> {
        self.fed_total += bytes.len();
        let mut i = 0usize;
        while i < bytes.len() {
            self.buf.push(bytes[i]);
            i += 1;
        }
        let mut produced = 0usize;
        loop {
            match self.try_frame() {
                Step::Complete(consumed, payload) => {
                    self.events.push(FrameEvent { kind: ResyncKind::InBody, bytes: consumed });
                    self.messages.push(payload);
                    produced += 1;
                }
                Step::Suspend(kind) => {
                    self.events.push(FrameEvent { kind, bytes: 0 });
                    break;
                }
                Step::Junk => {
                    // 滑动丢弃后继续尝试
                }
                Step::Empty => break,
            }
        }
        if produced > 0 && !self.buf.is_empty() {
            self.events.push(FrameEvent { kind: ResyncKind::CrossFrame, bytes: 0 });
        }
        Ok(produced)
    }

    /// 从 buf 头部尝试解一帧。
    fn try_frame(&mut self) -> Step {
        if self.buf.is_empty() {
            return Step::Empty;
        }
        if self.buf.len() < 2 {
            return Step::Suspend(ResyncKind::InMagic);
        }
        if self.buf[0] != MAGIC[0] || self.buf[1] != MAGIC[1] {
            let mut rest: Vec<u8> = Vec::new();
            let mut i = 1usize;
            while i < self.buf.len() {
                rest.push(self.buf[i]);
                i += 1;
            }
            self.buf = rest;
            self.junk_dropped += 1;
            return Step::Junk;
        }
        if self.buf.len() < 4 {
            return Step::Suspend(ResyncKind::InLen);
        }
        let n = (self.buf[2] as usize) | ((self.buf[3] as usize) << 8);
        if n > MAX_PAYLOAD {
            let mut rest: Vec<u8> = Vec::new();
            let mut i = 2usize;
            while i < self.buf.len() {
                rest.push(self.buf[i]);
                i += 1;
            }
            self.buf = rest;
            self.junk_dropped += 2;
            return Step::Junk;
        }
        if self.buf.len() < 4 + n {
            return Step::Suspend(ResyncKind::InBody);
        }
        let mut payload: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < n {
            payload.push(self.buf[4 + i]);
            i += 1;
        }
        let mut rest: Vec<u8> = Vec::new();
        let mut i = 4 + n;
        while i < self.buf.len() {
            rest.push(self.buf[i]);
            i += 1;
        }
        self.buf = rest;
        Step::Complete(4 + n, payload)
    }

    /// 事件账四型封闭。
    pub fn audit_kinds(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 字节守恒：已组装 + 悬置 + 丢弃 = 喂入总量。
    pub fn audit_balance(&self) -> Result<(), u16> {
        let mut framed = 0usize;
        let mut i = 0usize;
        while i < self.events.len() {
            framed += self.events[i].bytes;
            i += 1;
        }
        if framed + self.buf.len() + self.junk_dropped != self.fed_total {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    /// 消息序列逐条等价校验。
    pub fn audit_messages(&self, expect: &[Vec<u8>]) -> Result<(), u16> {
        if self.messages.len() != expect.len() {
            return Err(E_MSG_LOST);
        }
        let mut i = 0usize;
        while i < expect.len() {
            if self.messages[i] != expect[i] {
                return Err(E_MSG_CORRUPT);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        let mut s = String::from("framer:");
        s.push_str("resync");
        s
    }

    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    pub fn msg_count(&self) -> usize {
        self.messages.len()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.messages.len() {
            let mut j = 0usize;
            while j < self.messages[i].len() {
                h ^= self.messages[i][j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h ^= self.junk_dropped as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }
}

/// 原始消息序列（三条）。
pub fn raw_messages() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = Vec::new();
    let mut m1: Vec<u8> = Vec::new();
    m1.push(b'h');
    m1.push(b'e');
    m1.push(b'l');
    m1.push(b'l');
    m1.push(b'o');
    v.push(m1);
    let mut m2: Vec<u8> = Vec::new();
    m2.push(b'w');
    m2.push(b'o');
    m2.push(b'r');
    m2.push(b'l');
    m2.push(b'd');
    m2.push(b'!');
    v.push(m2);
    let mut m3: Vec<u8> = Vec::new();
    m3.push(b'f');
    m3.push(b'c');
    m3.push(b'i');
    m3.push(b't');
    m3.push(b'x');
    m3.push(b'5');
    m3.push(b'-');
    m3.push(b'o');
    m3.push(b'k');
    v.push(m3);
    v
}

/// 拼接整流。
pub fn build_stream(msgs: &[Vec<u8>]) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < msgs.len() {
        let f = StreamFramer::encode_frame(&msgs[i]);
        let mut j = 0usize;
        while j < f.len() {
            s.push(f[j]);
            j += 1;
        }
        i += 1;
    }
    s
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：流半包注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let msgs = raw_messages();
    let stream = build_stream(&msgs);

    // R1 十种截断切点（MAGIC 中/LEN 中/载荷中/跨界覆盖）两段喂入重同步零丢包
    let mut seen_kind = [0u32; 4];
    let mut k = 0usize;
    while k < 10 {
        let cut = 1 + (k * (stream.len() - 2)) / 9;
        let mut fr = StreamFramer::new();
        let _ = fr.feed(&stream[0..cut])?;
        let _ = fr.feed(&stream[cut..])?;
        fr.audit_messages(&msgs)?;
        fr.audit_kinds()?;
        let mut i = 0usize;
        while i < fr.events.len() {
            seen_kind[fr.events[i].kind as usize] += 1;
            i += 1;
        }
        hits += 1;
        k += 1;
    }
    if seen_kind[0] == 0 || seen_kind[1] == 0 || seen_kind[2] == 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R2 垃圾段滑动拒收后合法流恢复
    let mut fr2 = StreamFramer::new();
    let junk: Vec<u8> = vec![0x00, 0x11, 0x22, 0x33, 0xA5];
    let _ = fr2.feed(&junk)?;
    let _ = fr2.feed(&stream)?;
    fr2.audit_messages(&msgs)?;
    if fr2.junk_dropped == 0 {
        return Err(E_JUNK_PASS);
    }
    fr2.audit_balance()?;
    hits += 1;

    // R3 粘包整流一次喂入全量重组
    let mut fr3 = StreamFramer::new();
    let _ = fr3.feed(&stream)?;
    fr3.audit_messages(&msgs)?;
    if fr3.msg_count() != 3 {
        return Err(E_MSG_LOST);
    }
    fr3.audit_balance()?;
    hits += 1;

    // R4 逐字节投递：最细粒度半包仍零丢包
    let mut fr4 = StreamFramer::new();
    let mut i = 0usize;
    while i < stream.len() {
        let one = vec![stream[i]];
        let _ = fr4.feed(&one)?;
        i += 1;
    }
    fr4.audit_messages(&msgs)?;
    fr4.audit_balance()?;
    hits += 1;

    // R5 随机切点 ×512：逐条等价零崩
    let mut rng = Lcg::new(0x48452);
    let mut k = 0u32;
    while k < 512 {
        let cut = rng.below((stream.len() + 1) as u64) as usize;
        let mut fr = StreamFramer::new();
        match fr.feed(&stream[0..cut]) {
            Ok(_) => {}
            Err(_) => return Err(E_CRASH_LEAK),
        }
        match fr.feed(&stream[cut..]) {
            Ok(_) => {}
            Err(_) => return Err(E_CRASH_LEAK),
        }
        fr.audit_messages(&msgs)?;
        fr.audit_kinds()?;
        fr.audit_balance()?;
        k += 1;
    }
    hits += 1;

    // R6 跨界事件出现：粘包余量残留场景记 CrossFrame
    let mut fr6 = StreamFramer::new();
    let cut = 11usize; // 第 1 帧完整 + 第 2 帧头部残留
    let _ = fr6.feed(&stream[0..cut])?;
    if fr6.msg_count() != 1 {
        return Err(E_RESYNC_DEAD);
    }
    let mut has_cross = false;
    let mut i = 0usize;
    while i < fr6.events.len() {
        if fr6.events[i].kind == ResyncKind::CrossFrame {
            has_cross = true;
        }
        i += 1;
    }
    if !has_cross {
        return Err(E_KIND_BAD);
    }
    let _ = fr6.feed(&stream[cut..])?;
    fr6.audit_messages(&msgs)?;
    fr6.audit_balance()?;
    hits += 1;

    // R7 重演一致：同种子随机切点指纹相等
    let mut rng2 = Lcg::new(0x48452);
    let mut fr7a = StreamFramer::new();
    let mut k = 0u32;
    while k < 512 {
        let cut = rng2.below((stream.len() + 1) as u64) as usize;
        let _ = fr7a.feed(&stream[0..cut]);
        let _ = fr7a.feed(&stream[cut..]);
        k += 1;
    }
    let fp1 = fr7a.fingerprint();
    let mut rng3 = Lcg::new(0x48452);
    let mut fr7b = StreamFramer::new();
    let mut k = 0u32;
    while k < 512 {
        let cut = rng3.below((stream.len() + 1) as u64) as usize;
        let _ = fr7b.feed(&stream[0..cut]);
        let _ = fr7b.feed(&stream[cut..]);
        k += 1;
    }
    if fp1 != fr7b.fingerprint() || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 四型齐备 + 全场景事件账封闭
    if seen_kind[3] == 0 {
        return Err(E_KIND_BAD);
    }
    fr2.audit_kinds()?;
    fr4.audit_kinds()?;
    fr6.audit_kinds()?;
    hits += 1;

    Ok(hits)
}
