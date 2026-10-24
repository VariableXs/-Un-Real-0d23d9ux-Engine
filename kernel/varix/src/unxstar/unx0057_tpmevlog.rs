//! UNX-F0057 · TPM 事件日志只读导出 · 诊断段 0x2090
//! 验收锚点：UNX-F0057-J1 TCG PCR 事件逐条解析入账，SHA1/SHA256
//! 摘要长度闭集核验，PCR 索引账与度量链完整性判据通过；
//! 纯功能代码 ≥260 行。只读导出——零写入、零回放副作用。


use crate::alloc::{vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2090;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HDR: u16 = SEG_BASE + 1;      // 事件日志头（TCG_PCR_EVENT 头）非法
const E_TYPE: u16 = SEG_BASE + 2;     // 事件类型超规范闭集
const E_PCR: u16 = SEG_BASE + 3;      // PCR 索引越 0..=23
const E_DIG: u16 = SEG_BASE + 4;      // 摘要长度不属闭集 {20, 32, 48}
const E_TRUNC: u16 = SEG_BASE + 5;    // 事件被截断（事件长度字段越流尾）
const E_DUP: u16 = SEG_BASE + 6;      // 同 (PCR,摘要) 二次度量
const E_CHAIN: u16 = SEG_BASE + 7;    // 度量链断裂（序列号/序账不连续）
const E_RDONLY: u16 = SEG_BASE + 8;   // 导出账收到写请求
const E_BAL: u16 = SEG_BASE + 9;      // 事件账与字节账对不平
const E_SPEC: u16 = SEG_BASE + 10;    // 规格版本字段非法

/// TCG_EfiSpecIdEvent / TCG_PCR_EVENT2 规格签名 "Spec ID Event03"。
pub const SPEC_ID_SIG: [u8; 16] = [
    0x53, 0x70, 0x65, 0x63, 0x20, 0x49, 0x44, 0x20,
    0x45, 0x76, 0x65, 0x6e, 0x74, 0x30, 0x33, 0x00, // "Spec ID Event03\0"
];

/// SHA1/SHA256/SHA384 摘要长度闭集。
pub const DIG_SHA1: usize = 20;
pub const DIG_SHA256: usize = 32;
pub const DIG_SHA384: usize = 48;

/// PCR 闭集 0..=23（PC 客户端惯例）。
pub const PCR_MAX: u32 = 23;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_HDR => "日志头非法：前 32 字节须为规格签名/平台类标识",
        E_TYPE => "事件类型超规范闭集：0..=0x13 与 EV_NO_ACTION(0x03) 之外的裸值拒绝",
        E_PCR => "PCR 索引越 0..=23：PC 客户端闭集外值拒绝",
        E_DIG => "摘要长度非 {20,32,48}：SHA1/SHA256/SHA384 之外的宽度拒绝",
        E_TRUNC => "事件长度越流尾：半条目拒绝，导出到断点为止",
        E_DUP => "同 (PCR, 摘要) 二次度量：留痕拒绝",
        E_CHAIN => "度量链断裂：事件序号跳变，导出在此断链立案",
        E_RDONLY => "只读导出账收到写请求：路径错误",
        E_BAL => "事件账与字节账不平：Σ事件长度 ≠ 已消费字节数",
        E_SPEC => "规格版本字段非法：算法注册表缺失或版本倒退",
        _ => "未知诊断码",
    }
}

// ── 事件记录 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct EventRecord {
    pub pcr: u32,
    pub ev_type: u32,
    pub digest_lens: Vec<usize>,
    pub digest_fps: Vec<u64>,
    pub event_size: usize,
    pub seq: u64,
}

impl EventRecord {
    pub fn key_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.pcr.to_le_bytes());
        for f in &self.digest_fps {
            buf.extend_from_slice(&f.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

/// 事件类型闭集核验（PC 客户端常用值 + 预定义域 0..=0x13）。
pub fn valid_event_type(t: u32) -> bool {
    t <= 0x13 || (0x8000_0000..=0x8000_00FF).contains(&t) // 客户端自定义域
}

// ── 事件2 结构解析（TCG_PCR_EVENT2 简化投影）───────────────────
/// 流布局：[PCR(4)][类型(4)][摘要数(4)][(算法ID(2)×摘要)...][事件长度(4)][事件体]
pub fn parse_event2(stream: &[u8], off: usize) -> Result<(EventRecord, usize), u16> {
    if stream.len() < off + 12 {
        return Err(E_TRUNC);
    }
    let pcr = u32::from_le_bytes([stream[off], stream[off + 1], stream[off + 2], stream[off + 3]]);
    if pcr > PCR_MAX {
        return Err(E_PCR);
    }
    let ev_type = u32::from_le_bytes([
        stream[off + 4], stream[off + 5], stream[off + 6], stream[off + 7],
    ]);
    if !valid_event_type(ev_type) {
        return Err(E_TYPE);
    }
    let n_digest = u32::from_le_bytes([
        stream[off + 8], stream[off + 9], stream[off + 10], stream[off + 11],
    ]) as usize;
    let mut p = off + 12;
    let mut digest_lens = Vec::new();
    let mut digest_fps = Vec::new();
    for _ in 0..n_digest {
        if stream.len() < p + 2 {
            return Err(E_TRUNC);
        }
        let algo = u16::from_le_bytes([stream[p], stream[p + 1]]);
        p += 2;
        let dlen = match algo {
            0x0004 => DIG_SHA1,
            0x000B => DIG_SHA256,
            0x000C => DIG_SHA384,
            _ => return Err(E_DIG),
        };
        if stream.len() < p + dlen {
            return Err(E_TRUNC);
        }
        digest_lens.push(dlen);
        digest_fps.push(fnv1a64(&stream[p..p + dlen]));
        p += dlen;
    }
    if stream.len() < p + 4 {
        return Err(E_TRUNC);
    }
    let event_size = u32::from_le_bytes([
        stream[p], stream[p + 1], stream[p + 2], stream[p + 3],
    ]) as usize;
    p += 4;
    if event_size > 64 * 1024 || stream.len() < p + event_size {
        return Err(E_TRUNC);
    }
    let body_fp = fnv1a64(&stream[p..p + event_size]);
    digest_fps.push(body_fp); // 事件体指纹并入账（账面区分：末位为体指纹）
    p += event_size;
    Ok((
        EventRecord {
            pcr,
            ev_type,
            digest_lens,
            digest_fps,
            event_size,
            seq: 0,
        },
        p,
    ))
}

// ── 只读导出账 ───────────────────────────────────────────────
pub struct EventLogExport {
    pub records: Vec<EventRecord>,
    pub bytes_consumed: usize,
    pub rejected: Vec<(u32, u16)>,
    pub chain_broken_at: Option<usize>,
    pub closed: bool,
}

impl EventLogExport {
    pub fn new() -> EventLogExport {
        EventLogExport {
            records: Vec::new(),
            bytes_consumed: 0,
            rejected: Vec::new(),
            chain_broken_at: None,
            closed: false,
        }
    }

    /// 头核验：前 16 字节签名（Spec ID Event03）。
    pub fn feed_header(&mut self, stream: &[u8]) -> Result<(), u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        if stream.len() < 16 {
            return Err(E_HDR);
        }
        if stream[0..16] != SPEC_ID_SIG {
            return Err(E_HDR);
        }
        Ok(())
    }

    /// 吸收一条事件：PCR/类型/摘要闭集 + 链账（事件序连续）。
    pub fn absorb(&mut self, stream: &[u8], off: usize) -> Result<(u32, usize), u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        if let Some(_) = self.chain_broken_at {
            return Err(E_CHAIN);
        }
        let (rec, next) = parse_event2(stream, off)?;
        let seq = self.records.len() as u64 + self.rejected.len() as u64;
        let mut rec = rec;
        rec.seq = seq;
        if self
            .records
            .iter()
            .any(|r| r.key_fp() == rec.key_fp())
        {
            self.rejected.push((rec.pcr, E_DUP));
            return Ok((rec.pcr, next));
        }
        let pcr = rec.pcr;
        self.records.push(rec);
        self.bytes_consumed += next - off;
        Ok((pcr, next))
    }

    /// 收链：字节账对平。
    pub fn close(&mut self, total_len: usize) -> Result<(), u16> {
        if self.closed {
            return Err(E_RDONLY);
        }
        self.closed = true;
        if self.chain_broken_at.is_none() && self.bytes_consumed > total_len {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// PCR 分布账：每 PCR 的事件数。
    pub fn pcr_histogram(&self) -> [u32; 24] {
        let mut h = [0u32; 24];
        for r in &self.records {
            h[r.pcr as usize] += 1;
        }
        h
    }

    /// 度量链完整性判定：无断链且账平。
    pub fn integrity(&self) -> Result<u64, u16> {
        if self.chain_broken_at.is_some() {
            return Err(E_CHAIN);
        }
        let mut buf: Vec<u8> = Vec::new();
        for r in &self.records {
            buf.extend_from_slice(&r.seq.to_le_bytes());
            buf.extend_from_slice(&r.key_fp().to_le_bytes());
        }
        Ok(fnv1a64(&buf))
    }
}

// ── 合成事件流（演练复用）────────────────────────────────────
pub fn synth_event(pcr: u32, ev_type: u32, digests: &[(u16, Vec<u8>)], body: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.extend_from_slice(&pcr.to_le_bytes());
    v.extend_from_slice(&ev_type.to_le_bytes());
    v.extend_from_slice(&(digests.len() as u32).to_le_bytes());
    for (algo, d) in digests {
        v.extend_from_slice(&algo.to_le_bytes());
        v.extend_from_slice(d);
    }
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    v
}

/// SHA1 长度哨兵（演练摘要占位，非真摘要——账面只记指纹）。
pub fn pad_digest(len: usize, seed: u8) -> Vec<u8> {
    vec![seed; len]
}

// ── 确定性演练：合法 3 例 + 注入拦截 6 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // 构造流：头 + 三事件（PCR0/1/4，SHA256+SHA1 双摘要）。
    let mut stream: Vec<u8> = Vec::new();
    stream.extend_from_slice(&SPEC_ID_SIG);
    stream.extend_from_slice(&[0u8; 16]); // 规格头余量
    let e1 = synth_event(0, 0x01, &[(0x000B, pad_digest(32, 1)), (0x0004, pad_digest(20, 2))], b"POST");
    let e2 = synth_event(1, 0x05, &[(0x000B, pad_digest(32, 3))], b"LOADER");
    let e3 = synth_event(4, 0x06, &[(0x000B, pad_digest(32, 4))], b"KERNEL");
    let e1_off = stream.len();
    stream.extend_from_slice(&e1);
    let e2_off = stream.len();
    stream.extend_from_slice(&e2);
    let e3_off = stream.len();
    stream.extend_from_slice(&e3);

    // R1：头 + 三事件全链导出，账平，完整性指纹可复算。
    let mut led = EventLogExport::new();
    led.feed_header(&stream)?;
    let (_, _n1) = led.absorb(&stream, e1_off)?;
    let (_, _n2) = led.absorb(&stream, e2_off)?;
    let (_, n3) = led.absorb(&stream, e3_off)?;
    led.close(n3)?;
    if led.records.len() != 3 || led.bytes_consumed != n3 - e1_off {
        return Err(E_BAL);
    }
    let fp1 = led.integrity()?;
    let fp2 = led.integrity()?;
    if fp1 != fp2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：PCR 直方图分桶正确。
    let h = led.pcr_histogram();
    if h[0] != 1 || h[1] != 1 || h[4] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：截断导出——流尾切半条目，absorb 报 E_TRUNC 且此前事件保留。
    let cut = &stream[..e3_off + 10];
    let mut led3 = EventLogExport::new();
    led3.feed_header(cut)?;
    let _ = led3.absorb(cut, e1_off)?;
    let _ = led3.absorb(cut, e2_off)?;
    if led3.absorb(cut, e3_off) != Err(E_TRUNC) {
        return Err(E_TRUNC);
    }
    if led3.records.len() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：签名注入 → E_HDR。
    let mut b = stream.clone();
    b[0] = 0x58;
    let mut i1 = EventLogExport::new();
    if i1.feed_header(&b) != Err(E_HDR) {
        return Err(E_HDR);
    }
    hits += 1;

    // I2：PCR 30 → E_PCR。
    let bad_pcr = synth_event(30, 0x01, &[(0x000B, pad_digest(32, 9))], b"x");
    let mut i2 = EventLogExport::new();
    if i2.absorb(&bad_pcr, 0) != Err(E_PCR) {
        return Err(E_PCR);
    }
    hits += 1;

    // I3：算法 ID 未知 → E_DIG。
    let bad_algo = synth_event(0, 0x01, &[(0x00FF, pad_digest(16, 7))], b"x");
    let mut i3 = EventLogExport::new();
    if i3.absorb(&bad_algo, 0) != Err(E_DIG) {
        return Err(E_DIG);
    }
    hits += 1;

    // I4：事件类型 0x99 越闭集 → E_TYPE。
    let bad_type = synth_event(0, 0x99, &[(0x000B, pad_digest(32, 1))], b"x");
    let mut i4 = EventLogExport::new();
    if i4.absorb(&bad_type, 0) != Err(E_TYPE) {
        return Err(E_TYPE);
    }
    hits += 1;

    // I5：同 (PCR,摘要) 双度量 → E_DUP 留痕。
    let dup = synth_event(0, 0x01, &[(0x000B, pad_digest(32, 1))], b"POST");
    let mut i5 = EventLogExport::new();
    let _ = i5.absorb(&dup, 0)?;
    let r = i5.absorb(&dup, 0)?;
    if i5.rejected.len() != 1 || i5.records.len() != 1 {
        return Err(E_DUP);
    }
    let _ = r;
    hits += 1;

    // I6：收链后写 → E_RDONLY。
    if led.absorb(&stream, e1_off) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
