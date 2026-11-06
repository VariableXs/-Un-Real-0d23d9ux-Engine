//! UNX-F50130 USB fuzz 面（坏描述符注入不崩）（0x43c60 段）
//!
//! 锚点：UNX-F50130-J1 fuzz ×10^4 零崩；判据锚定 §AI-63 §5。
//!
//! 为什么 fuzz 面要自带「判定」：USB 描述符是设备提供的不可信字节流，任何一次
//! 越界读、任何一次「bLength=0 导致游标不前进」都足以让枚举器挂死。因此本模块
//! 不是「随便喂随机数看崩不崩」，而是一个可复算的实验装置：
//!
//! * `XorShift64` —— 确定性伪随机（xorshift64*），同种子同序列，10^4 次实验可逐位
//!   复现；种子 0 显式拒绝（全零状态是吸收态）；
//! * `Mutation` —— 七类结构化变异（翻位/改字节/截断/加长/拼接/清零长度/伪造长度），
//!   每类都做边界校验，变异失败不等于崩溃，只计入「不适用」；
//! * `FuzzCorpus` —— 三个自造种子（合法打印机 / 无端点 / 复合含类专用描述符），
//!   定容、去空；
//! * `scan` —— 加固描述符扫描器：每次读取先做区间校验，`bLength < 2` 一律报
//!   `ZeroLen`（这是 fuzz 里最常见的死循环面），游标步数上限 64（防止 2 字节
//!   描述符串拖死枚举），返回的 `consumed` 永不超过输入长度；
//! * `fuzz` —— 主实验：逐轮变异后扫描，统计接受/拒绝/越界违规数，产出可复算摘要。
//!   判据 J1 的「零崩」由三件事共同保证：扫描器零 panic、`violations == 0`
//!   （摘要从不超过实际缓冲区）、实验跑满 10^4 轮并正常返回。
//!
//! 错误纪律：段内闭集 `SEG_BASE + n`；内核路径零 panic，无裸下标读写。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43c60 独占 0x20）。
pub const SEG_BASE: u32 = 0x43c60;

pub const E_EMPTY: u16 = 1;
pub const E_SEED: u16 = 2;
pub const E_TRUNC: u16 = 3;
pub const E_BOUND: u16 = 4;
pub const E_STEP: u16 = 5;
pub const E_DEVLEN: u16 = 6;
pub const E_CFG: u16 = 7;
pub const E_CFGLEN: u16 = 8;
pub const E_CFGTYPE: u16 = 9;
pub const E_ZEROLEN: u16 = 10;
pub const E_TOTAL: u16 = 11;
pub const E_CAPACITY: u16 = 12;
pub const E_OP: u16 = 13;

/// 描述符扫描/变异错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    Empty,
    Seed,
    Trunc,
    Bound,
    Step,
    DevLen,
    Cfg,
    CfgLen,
    CfgType,
    ZeroLen,
    Total,
    Capacity,
    Op,
}

impl FuzzError {
    pub fn code(self) -> u32 {
        let n = match self {
            FuzzError::Empty => E_EMPTY,
            FuzzError::Seed => E_SEED,
            FuzzError::Trunc => E_TRUNC,
            FuzzError::Bound => E_BOUND,
            FuzzError::Step => E_STEP,
            FuzzError::DevLen => E_DEVLEN,
            FuzzError::Cfg => E_CFG,
            FuzzError::CfgLen => E_CFGLEN,
            FuzzError::CfgType => E_CFGTYPE,
            FuzzError::ZeroLen => E_ZEROLEN,
            FuzzError::Total => E_TOTAL,
            FuzzError::Capacity => E_CAPACITY,
            FuzzError::Op => E_OP,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FuzzError::Empty => "empty buffer or corpus",
            FuzzError::Seed => "prng seed zero (absorbing state)",
            FuzzError::Trunc => "cannot truncate below one byte",
            FuzzError::Bound => "mutation would exceed the 4 KiB fuzz window",
            FuzzError::Step => "descriptor walk exceeded its step budget",
            FuzzError::DevLen => "device descriptor shorter than 18 or wrong bLength",
            FuzzError::Cfg => "configuration descriptor missing",
            FuzzError::CfgLen => "descriptor bLength shorter than its type requires",
            FuzzError::CfgType => "configuration descriptor has wrong type",
            FuzzError::ZeroLen => "descriptor bLength below 2 (cursor stall)",
            FuzzError::Total => "wTotalLength outside the supplied buffer",
            FuzzError::Capacity => "corpus capacity exhausted",
            FuzzError::Op => "unknown mutation opcode",
        }
    }

    /// 直方图槽位（0..13）。每条错误都能落到唯一槽，避免统计时合并吞掉类别。
    pub fn index(self) -> usize {
        self.code() as usize - SEG_BASE as usize - 1
    }
}

/// 扫描器最多走多少个描述符。64 个足够覆盖复合设备（含 alt setting），
/// 超过就说明输入在构造「无穷小描述符」，直接判死。
pub const MAX_STEPS: u32 = 64;
/// fuzz 缓冲区上限（4 KiB）。超过这个量级已不是描述符，是内存攻击面。
pub const MAX_BUF: usize = 4096;
/// 直方图槽数（= 错误枚举条数）。
pub const HIST_SLOTS: usize = 13;

// ---------------------------------------------------------------------------
// 确定性伪随机
// ---------------------------------------------------------------------------

/// xorshift64*：状态 64 位、周期 2^64-1、乘法扰频改善低位分布。
/// 选它而不是 LCG 的原因：fuzz 需要「同种子逐位复现」，且不允许周期过短导致
/// 变异退化到少数几种组合。
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// 种子 0 是全零吸收态（永远输出 0），显式拒绝而不是静默替换。
    pub fn new(seed: u64) -> Result<XorShift64, FuzzError> {
        if seed == 0 {
            return Err(FuzzError::Seed);
        }
        Ok(XorShift64 { state: seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// [0, n) 均匀取（n=0 返回 0，调用方必须自行处理 0 宽区间）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

// ---------------------------------------------------------------------------
// 变异算子
// ---------------------------------------------------------------------------

/// 七类结构化变异。全部按「读-校验-写」顺序做，任何一步越界都返回错误，
/// 绝不让一条坏变异把实验本身搞崩。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    FlipBit,
    SetByte,
    Truncate,
    Extend,
    Splice,
    ZeroLen,
    SetLen,
}

impl Mutation {
    pub fn wire(self) -> u8 {
        match self {
            Mutation::FlipBit => 0,
            Mutation::SetByte => 1,
            Mutation::Truncate => 2,
            Mutation::Extend => 3,
            Mutation::Splice => 4,
            Mutation::ZeroLen => 5,
            Mutation::SetLen => 6,
        }
    }

    pub fn from_wire(c: u8) -> Result<Mutation, FuzzError> {
        match c {
            0 => Ok(Mutation::FlipBit),
            1 => Ok(Mutation::SetByte),
            2 => Ok(Mutation::Truncate),
            3 => Ok(Mutation::Extend),
            4 => Ok(Mutation::Splice),
            5 => Ok(Mutation::ZeroLen),
            6 => Ok(Mutation::SetLen),
            _ => Err(FuzzError::Op),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mutation::FlipBit => "flip-bit",
            Mutation::SetByte => "set-byte",
            Mutation::Truncate => "truncate",
            Mutation::Extend => "extend",
            Mutation::Splice => "splice",
            Mutation::ZeroLen => "zero-len",
            Mutation::SetLen => "set-len",
        }
    }

    /// 就地施加变异，返回受影响的位置/字节数（仅供统计，不参与正确性判定）。
    pub fn apply(
        self,
        buf: &mut Vec<u8>,
        rng: &mut XorShift64,
        other: &[u8],
    ) -> Result<usize, FuzzError> {
        let len = buf.len();
        match self {
            Mutation::FlipBit => {
                if len == 0 {
                    return Err(FuzzError::Empty);
                }
                let pos = rng.below(len as u32) as usize;
                let bit = rng.below(8);
                if let Some(b) = buf.get_mut(pos) {
                    *b ^= 1u8 << bit;
                    return Ok(pos);
                }
                Err(FuzzError::Bound)
            }
            Mutation::SetByte => {
                if len == 0 {
                    return Err(FuzzError::Empty);
                }
                let pos = rng.below(len as u32) as usize;
                let val = (rng.next_u32() & 0xff) as u8;
                if let Some(b) = buf.get_mut(pos) {
                    *b = val;
                    return Ok(pos);
                }
                Err(FuzzError::Bound)
            }
            Mutation::Truncate => {
                if len < 2 {
                    return Err(FuzzError::Trunc);
                }
                let keep = 1 + rng.below(len as u32 - 1) as usize;
                buf.truncate(keep);
                Ok(keep)
            }
            Mutation::Extend => {
                let extra = 1 + rng.below(16) as usize;
                if len + extra > MAX_BUF {
                    return Err(FuzzError::Bound);
                }
                let mut k = 0usize;
                while k < extra {
                    buf.push((rng.next_u32() & 0xff) as u8);
                    k += 1;
                }
                Ok(extra)
            }
            Mutation::Splice => {
                if len == 0 || other.is_empty() {
                    return Err(FuzzError::Empty);
                }
                let pos = rng.below(len as u32) as usize;
                let room = len - pos;
                let take = if other.len() < room { other.len() } else { room };
                if take == 0 {
                    return Err(FuzzError::Bound);
                }
                let mut k = 0usize;
                while k < take {
                    let src = match other.get(k) {
                        Some(v) => *v,
                        None => break,
                    };
                    if let Some(dst) = buf.get_mut(pos + k) {
                        *dst = src;
                    }
                    k += 1;
                }
                Ok(take)
            }
            Mutation::ZeroLen => {
                if len < 2 {
                    return Err(FuzzError::Bound);
                }
                let pos = rng.below(len as u32) as usize;
                if let Some(b) = buf.get_mut(pos) {
                    *b = 0;
                    return Ok(pos);
                }
                Err(FuzzError::Bound)
            }
            Mutation::SetLen => {
                if len == 0 {
                    return Err(FuzzError::Empty);
                }
                let pos = rng.below(len as u32) as usize;
                // 2..=255：既覆盖「虚报长描述符」，也避开 0/1 这种必被判 ZeroLen 的平凡值，
                // 让 10^4 轮里真正跑到「长度字段自相矛盾」的路径。
                let val = 2 + rng.below(254) as u8;
                if let Some(b) = buf.get_mut(pos) {
                    *b = val;
                    return Ok(pos);
                }
                Err(FuzzError::Bound)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 种子语料
// ---------------------------------------------------------------------------

fn push_u16(v: &mut Vec<u8>, x: u16) {
    v.push((x & 0xff) as u8);
    v.push((x >> 8) as u8);
}

fn device_desc(vid: u16, pid: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(18);
    v.push(18);
    v.push(1);
    push_u16(&mut v, 0x0200);
    v.push(0);
    v.push(0);
    v.push(0);
    v.push(64);
    push_u16(&mut v, vid);
    push_u16(&mut v, pid);
    push_u16(&mut v, 0x0100);
    v.push(0);
    v.push(0);
    v.push(0);
    v.push(1);
    v
}

fn config_desc(total: u16, nif: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(9);
    v.push(9);
    v.push(2);
    push_u16(&mut v, total);
    v.push(nif);
    v.push(1);
    v.push(0);
    v.push(0xc0);
    v.push(50);
    v
}

fn iface_desc(num: u8, alt: u8, neps: u8, class: u8, proto: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(9);
    v.push(9);
    v.push(4);
    v.push(num);
    v.push(alt);
    v.push(neps);
    v.push(class);
    v.push(1);
    v.push(proto);
    v.push(0);
    v
}

fn ep_desc(addr: u8, attr: u8, mps: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(7);
    v.push(7);
    v.push(5);
    v.push(addr);
    v.push(attr);
    push_u16(&mut v, mps);
    v.push(0);
    v
}

/// 三个自造种子。
/// ① 合法双向打印机（50 字节，wTotalLength=32）；
/// ② 无端点打印机（36 字节，wTotalLength=18）——绑定阶段必然失败，但扫描合法；
/// ③ 复合设备（74 字节，wTotalLength=56）：厂商接口 + 类专用描述符 + 打印接口。
pub fn build_seed(variant: u8) -> Result<Vec<u8>, FuzzError> {
    match variant {
        0 => {
            let mut v = device_desc(0x04b8, 0x0005);
            let mut cfg = config_desc(32, 1);
            v.append(&mut cfg);
            let mut ifc = iface_desc(0, 0, 2, 7, 2);
            v.append(&mut ifc);
            let mut e1 = ep_desc(0x01, 0x02, 512);
            v.append(&mut e1);
            let mut e2 = ep_desc(0x81, 0x02, 512);
            v.append(&mut e2);
            Ok(v)
        }
        1 => {
            let mut v = device_desc(0x0403, 0x6001);
            let mut cfg = config_desc(18, 1);
            v.append(&mut cfg);
            let mut ifc = iface_desc(0, 0, 0, 7, 1);
            v.append(&mut ifc);
            Ok(v)
        }
        2 => {
            let mut v = device_desc(0x03f0, 0x0004);
            let mut cfg = config_desc(56, 2);
            v.append(&mut cfg);
            let mut i0 = iface_desc(0, 0, 1, 0xff, 0);
            v.append(&mut i0);
            let mut e0 = ep_desc(0x84, 0x03, 8);
            v.append(&mut e0);
            // 类专用描述符（0x24）：扫描器只计数不解读。
            v.push(8);
            v.push(0x24);
            v.push(0);
            v.push(0x10);
            v.push(0);
            v.push(1);
            v.push(0);
            v.push(0);
            let mut i1 = iface_desc(1, 0, 2, 7, 2);
            v.append(&mut i1);
            let mut e1 = ep_desc(0x02, 0x02, 64);
            v.append(&mut e1);
            let mut e2 = ep_desc(0x82, 0x02, 64);
            v.append(&mut e2);
            Ok(v)
        }
        _ => Err(FuzzError::Op),
    }
}

/// 种子语料：定容、去空、可枚举。
pub struct FuzzCorpus {
    seeds: Vec<Vec<u8>>,
    cap: usize,
}

impl FuzzCorpus {
    pub fn new(cap: usize) -> Result<FuzzCorpus, FuzzError> {
        if cap == 0 || cap > 8 {
            return Err(FuzzError::Capacity);
        }
        Ok(FuzzCorpus {
            seeds: Vec::with_capacity(cap),
            cap,
        })
    }

    /// 三个内置种子一次装好。任一种子构造失败都整体失败，不静默少装。
    pub fn builtin() -> Result<FuzzCorpus, FuzzError> {
        let mut c = FuzzCorpus::new(3)?;
        let mut i = 0u8;
        while i < 3 {
            let seed = build_seed(i)?;
            c.push(seed)?;
            i += 1;
        }
        Ok(c)
    }

    pub fn push(&mut self, bytes: Vec<u8>) -> Result<usize, FuzzError> {
        if bytes.is_empty() {
            return Err(FuzzError::Empty);
        }
        if bytes.len() > MAX_BUF {
            return Err(FuzzError::Bound);
        }
        if self.seeds.len() >= self.cap {
            return Err(FuzzError::Capacity);
        }
        self.seeds.push(bytes);
        Ok(self.seeds.len() - 1)
    }

    pub fn len(&self) -> usize {
        self.seeds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seeds.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&[u8]> {
        self.seeds.get(i).map(|v| v.as_slice())
    }
}

// ---------------------------------------------------------------------------
// 加固扫描器
// ---------------------------------------------------------------------------

fn byte(d: &[u8], at: usize) -> Result<u8, FuzzError> {
    match d.get(at) {
        Some(b) => Ok(*b),
        None => Err(FuzzError::Bound),
    }
}

fn le16(d: &[u8], at: usize) -> Result<u16, FuzzError> {
    let lo = byte(d, at)? as u16;
    let hi = byte(d, at + 1)? as u16;
    Ok(lo | (hi << 8))
}

fn fnv_u8(mut h: u32, b: u8) -> u32 {
    h ^= b as u32;
    h.wrapping_mul(0x0100_0193)
}

/// 前 `n` 字节的 FNV-1a 摘要（n 会被裁到缓冲区长度，不越界）。
pub fn digest_prefix(buf: &[u8], n: usize) -> u32 {
    let cap = if n < buf.len() { n } else { buf.len() };
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < cap {
        if let Some(b) = buf.get(i) {
            h = fnv_u8(h, *b);
        }
        i += 1;
    }
    h
}

/// 一次扫描的摘要。`consumed` 是扫描器真正读过的字节数，恒 ≤ 输入长度，
/// 这就是「不越界」的可判定证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanSummary {
    pub consumed: u32,
    pub total_len: u16,
    pub ifaces: u16,
    pub eps: u16,
    pub skipped: u16,
    pub hash: u32,
}

/// 扫描平坦描述符流（device + 单 configuration）。
///
/// 加固点：
/// * 所有字节读取走 `get()`（越界 = `Bound`）；
/// * `bLength < 2` 直接 `ZeroLen`（否则游标不动 = 死循环）；
/// * 步数上限 `MAX_STEPS`，2 字节描述符串无法拖死循环；
/// * `wTotalLength` 必须落在缓冲区内，扫描域被它封住。
pub fn scan(buf: &[u8]) -> Result<ScanSummary, FuzzError> {
    if buf.is_empty() {
        return Err(FuzzError::Empty);
    }
    if buf.len() < 18 {
        return Err(FuzzError::DevLen);
    }
    if byte(buf, 0)? != 18 || byte(buf, 1)? != 1 {
        return Err(FuzzError::DevLen);
    }
    if buf.len() < 27 {
        return Err(FuzzError::Cfg);
    }
    if byte(buf, 18)? != 9 {
        return Err(FuzzError::CfgLen);
    }
    if byte(buf, 19)? != 2 {
        return Err(FuzzError::CfgType);
    }
    let total = le16(buf, 20)?;
    if total < 9 || 18usize.saturating_add(total as usize) > buf.len() {
        return Err(FuzzError::Total);
    }
    let end = 18usize + total as usize;
    let mut pos = 27usize;
    let mut steps: u32 = 0;
    let mut ifaces: u16 = 0;
    let mut eps: u16 = 0;
    let mut skipped: u16 = 0;
    while pos < end {
        steps += 1;
        if steps > MAX_STEPS {
            return Err(FuzzError::Step);
        }
        let dl = byte(buf, pos)? as usize;
        if dl < 2 {
            return Err(FuzzError::ZeroLen);
        }
        if pos.saturating_add(dl) > end {
            return Err(FuzzError::Total);
        }
        let dt = byte(buf, pos + 1)?;
        match dt {
            4 => {
                // 接口描述符最短 9 字节；短于 9 的「接口」没有 bNumEndpoints 字段。
                if dl < 9 {
                    return Err(FuzzError::CfgLen);
                }
                ifaces = ifaces.saturating_add(1);
            }
            5 => {
                // 端点描述符最短 7 字节（含 wMaxPacketSize 与 bInterval）。
                if dl < 7 {
                    return Err(FuzzError::CfgLen);
                }
                eps = eps.saturating_add(1);
            }
            _ => {
                skipped = skipped.saturating_add(1);
            }
        }
        pos += dl;
    }
    let consumed = end as u32;
    Ok(ScanSummary {
        consumed,
        total_len: total,
        ifaces,
        eps,
        skipped,
        hash: digest_prefix(buf, end),
    })
}

// ---------------------------------------------------------------------------
// 主实验
// ---------------------------------------------------------------------------

/// 一次 fuzz 实验的落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub iterations: u32,
    pub accepted: u32,
    pub rejected: u32,
    /// 摘要越界违规数（`consumed > len` 或 hash 覆盖范围不符）。J1 要求恒 0。
    pub violations: u32,
    /// 成功施加的变异次数（证明实验真的在变异，而不是空转）。
    pub mutated: u32,
    pub hist: [u32; HIST_SLOTS],
    pub digest: u64,
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0u32;
    while i < 4 {
        h ^= ((v >> (i * 8)) & 0xff) as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x ^= ((v >> (i * 8)) & 0xff) as u64;
        x = x.wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    x
}

/// 主实验：每个迭代从语料随机取底、施加 1~4 个变异、扫描、落账。
///
/// 「零崩」的判定不看进程是否活着，而看三条硬账：`iterations` 跑满、
/// `violations == 0`、`accepted + rejected == iterations`。
pub fn fuzz(seed: u64, iterations: u32, corpus: &FuzzCorpus) -> Result<FuzzReport, FuzzError> {
    if iterations == 0 || corpus.is_empty() {
        return Err(FuzzError::Empty);
    }
    let mut rng = XorShift64::new(seed)?;
    let mut accepted: u32 = 0;
    let mut rejected: u32 = 0;
    let mut violations: u32 = 0;
    let mut mutated: u32 = 0;
    let mut hist = [0u32; HIST_SLOTS];
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i: u32 = 0;
    while i < iterations {
        let pick = rng.below(corpus.len() as u32) as usize;
        let base = match corpus.get(pick) {
            Some(b) => b,
            None => return Err(FuzzError::Capacity),
        };
        let mut buf: Vec<u8> = Vec::with_capacity(base.len() + 32);
        buf.extend_from_slice(base);
        let rounds = 1 + rng.below(4);
        let mut r: u32 = 0;
        while r < rounds {
            let op = Mutation::from_wire(rng.below(7) as u8)?;
            let oi = rng.below(corpus.len() as u32) as usize;
            let other = match corpus.get(oi) {
                Some(o) => o,
                None => &[],
            };
            // 变异不适用（例如对 1 字节缓冲做拼接）不是崩溃，只跳过本轮。
            if op.apply(&mut buf, &mut rng, other).is_ok() {
                mutated = mutated.saturating_add(1);
            }
            r += 1;
        }
        match scan(&buf) {
            Ok(sum) => {
                accepted = accepted.saturating_add(1);
                if sum.consumed as usize > buf.len() {
                    violations = violations.saturating_add(1);
                }
                if sum.hash != digest_prefix(&buf, sum.consumed as usize) {
                    violations = violations.saturating_add(1);
                }
                digest = fnv_u64(digest, sum.hash as u64);
                digest = fnv_u32(digest, sum.consumed);
                digest = fnv_u32(digest, sum.eps as u32);
            }
            Err(e) => {
                rejected = rejected.saturating_add(1);
                if let Some(slot) = hist.get_mut(e.index()) {
                    *slot = slot.saturating_add(1);
                }
                digest = fnv_u32(digest, 0x8000_0000 | e.code());
            }
        }
        digest = fnv_u32(digest, buf.len() as u32);
        digest = fnv_u32(digest, i);
        i += 1;
    }
    Ok(FuzzReport {
        iterations,
        accepted,
        rejected,
        violations,
        mutated,
        hist,
        digest,
    })
}

// ---------------------------------------------------------------------------
// 注入核：十三类坏样本逐类断言具体码
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 13usize;

    // 1 空缓冲
    if scan(&[]).err() == Some(FuzzError::Empty) {
        caught += 1;
    }
    // 2 种子为 0
    if XorShift64::new(0).err() == Some(FuzzError::Seed) {
        caught += 1;
    }
    // 3 截断到 1 字节以下
    let mut one = Vec::new();
    one.push(1u8);
    let mut rng = match XorShift64::new(0x1234_5678_9abc_def1) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if Mutation::Truncate.apply(&mut one, &mut rng, &[]).err() == Some(FuzzError::Trunc) {
        caught += 1;
    }
    // 4 加长越过 4 KiB / 清零长度对 1 字节缓冲
    let mut big = Vec::with_capacity(MAX_BUF);
    let mut k = 0usize;
    while k < MAX_BUF {
        big.push(0u8);
        k += 1;
    }
    let mut tiny = Vec::new();
    tiny.push(1u8);
    if Mutation::Extend.apply(&mut big, &mut rng, &[]).err() == Some(FuzzError::Bound)
        && Mutation::ZeroLen.apply(&mut tiny, &mut rng, &[]).err() == Some(FuzzError::Bound)
    {
        caught += 1;
    }
    // 5 不足 18 字节
    let stub = [18u8, 1, 0, 2, 0, 0, 0, 64, 1, 2];
    if scan(&stub).err() == Some(FuzzError::DevLen) {
        caught += 1;
    }
    // 6 有设备描述符但没有配置
    let dev_only = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut twenty = Vec::new();
    let mut k = 0usize;
    while k < 20 {
        if let Some(b) = dev_only.get(k) {
            twenty.push(*b);
        }
        k += 1;
    }
    if scan(&twenty).err() == Some(FuzzError::Cfg) {
        caught += 1;
    }
    // 7 配置 bLength 错
    let mut cfg_len = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let Some(b) = cfg_len.get_mut(18) {
        *b = 8;
    }
    if scan(&cfg_len).err() == Some(FuzzError::CfgLen) {
        caught += 1;
    }
    // 8 配置类型错
    let mut cfg_type = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let Some(b) = cfg_type.get_mut(19) {
        *b = 3;
    }
    if scan(&cfg_type).err() == Some(FuzzError::CfgType) {
        caught += 1;
    }
    // 9 长度字段为零（游标不动）
    let mut zero_len = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let Some(b) = zero_len.get_mut(27) {
        *b = 0;
    }
    if scan(&zero_len).err() == Some(FuzzError::ZeroLen) {
        caught += 1;
    }
    // 10 wTotalLength 越界
    let mut total_bad = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if let Some(b) = total_bad.get_mut(20) {
        *b = 0xff;
    }
    if let Some(b) = total_bad.get_mut(21) {
        *b = 0x7f;
    }
    if scan(&total_bad).err() == Some(FuzzError::Total) {
        caught += 1;
    }
    // 11 描述符步数撑爆（70 个 2 字节描述符）
    let many = match build_seed(1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    // 重新拼一份：设备(18) + 配置(9) + 70×2
    let mut piled: Vec<u8> = Vec::with_capacity(200);
    let mut k = 0usize;
    while k < 27 {
        if let Some(b) = many.get(k) {
            piled.push(*b);
        }
        k += 1;
    }
    let total_len: u16 = 9 + 140;
    if let Some(b) = piled.get_mut(20) {
        *b = (total_len & 0xff) as u8;
    }
    if let Some(b) = piled.get_mut(21) {
        *b = (total_len >> 8) as u8;
    }
    let mut k = 0usize;
    while k < 70 {
        piled.push(2);
        piled.push(0xff);
        k += 1;
    }
    if scan(&piled).err() == Some(FuzzError::Step) {
        caught += 1;
    }
    // 12 语料容量/空样本
    let cap_bad = FuzzCorpus::new(0).err() == Some(FuzzError::Capacity);
    let mut cap2 = match FuzzCorpus::new(1) {
        Ok(c) => c,
        Err(_) => return (caught, total),
    };
    let first = cap2.push(match build_seed(0) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    });
    let second = cap2.push(match build_seed(1) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    });
    let empty_push = cap2.push(Vec::new());
    if cap_bad
        && first.is_ok()
        && second.err() == Some(FuzzError::Capacity)
        && empty_push.err() == Some(FuzzError::Empty)
    {
        caught += 1;
    }
    // 13 变异操作码越界
    if Mutation::from_wire(9).err() == Some(FuzzError::Op) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50130_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50130");

    // J1-1 PRNG 确定性 + 非退化：同种子同序列，异种子异序列，below 有界
    let mut a = match XorShift64::new(0xdead_beef_0bad_f00d) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut b = match XorShift64::new(0xdead_beef_0bad_f00d) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut c = match XorShift64::new(0x0bad_f00d_dead_beef) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut same = true;
    let mut differ = false;
    let mut bounded = true;
    let mut i = 0usize;
    while i < 16 {
        let x = a.next_u64();
        let y = b.next_u64();
        let z = c.next_u64();
        if x != y {
            same = false;
        }
        if x != z {
            differ = true;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < 64 {
        let n = 1 + (i as u32) * 7;
        if a.below(n) >= n {
            bounded = false;
        }
        i += 1;
    }
    let ok = same && differ && bounded && a.next_u64() != 0;
    s.add("50130-J1-伪随机", ok, if ok { "" } else { "同种子不可复现或退化" });

    // J1-2 三个内置种子全部合法，且彼此不同
    let corpus = match FuzzCorpus::builtin() {
        Ok(c) => c,
        Err(_) => return s,
    };
    let mut seeds_ok = corpus.len() == 3;
    let mut sums: [Option<ScanSummary>; 3] = [None, None, None];
    let mut i = 0usize;
    while i < 3 {
        match corpus.get(i) {
            Some(bytes) => match scan(bytes) {
                Ok(sum) => {
                    if let Some(cell) = sums.get_mut(i) {
                        *cell = Some(sum);
                    }
                }
                Err(_) => seeds_ok = false,
            },
            None => seeds_ok = false,
        }
        i += 1;
    }
    let ok = seeds_ok
        && sums.get(0).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0) == 50
        && sums.get(1).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0) == 36
        && sums.get(2).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0) == 74;
    let det = format!(
        "consumed={}/{}/{}",
        sums.get(0).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0),
        sums.get(1).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0),
        sums.get(2).and_then(|x| *x).map(|x| x.consumed).unwrap_or(0)
    );
    s.add_dyn("50130-J1-种子可扫", ok, &det);

    // J1-3 合法种子的结构计数正确：接口/端点/跳过各自对表
    let s0 = sums.get(0).and_then(|x| *x);
    let s2 = sums.get(2).and_then(|x| *x);
    let ok = match (s0, s2) {
        (Some(a0), Some(a2)) => {
            a0.ifaces == 1
                && a0.eps == 2
                && a0.skipped == 0
                && a0.total_len == 32
                && a2.ifaces == 2
                && a2.eps == 3
                && a2.skipped == 1
                && a2.total_len == 56
        }
        _ => false,
    };
    let det = format!(
        "seed0=({},{},{}) seed2=({},{},{})",
        s0.map(|x| x.ifaces).unwrap_or(0),
        s0.map(|x| x.eps).unwrap_or(0),
        s0.map(|x| x.skipped).unwrap_or(0),
        s2.map(|x| x.ifaces).unwrap_or(0),
        s2.map(|x| x.eps).unwrap_or(0),
        s2.map(|x| x.skipped).unwrap_or(0)
    );
    s.add_dyn("50130-J1-结构计数", ok, &det);

    // J1-4 扫描域封口：consumed 恒 ≤ 输入长度，hash 只覆盖 consumed 字节
    let mut bounds_ok = true;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(bytes) = corpus.get(i) {
            if let Ok(sum) = scan(bytes) {
                if sum.consumed as usize > bytes.len() {
                    bounds_ok = false;
                }
                if sum.hash != digest_prefix(bytes, sum.consumed as usize) {
                    bounds_ok = false;
                }
                // 截掉尾巴不影响已 consumed 部分（扫描域由 wTotalLength 封住）
                let mut short: Vec<u8> = Vec::with_capacity(bytes.len());
                short.extend_from_slice(bytes);
                short.pop();
                if let Ok(again) = scan(&short) {
                    if again.consumed != sum.consumed || again.hash != sum.hash {
                        bounds_ok = false;
                    }
                }
            } else {
                bounds_ok = false;
            }
        }
        i += 1;
    }
    s.add("50130-J1-扫描封口", bounds_ok, if bounds_ok { "" } else { "扫描域越界或摘要漂移" });

    // J1-5 主实验 10^4：跑满、零违规、接受+拒绝守恒（J1 核心）
    let report = match fuzz(0x5eed_1234_9876_abcd, 10_000, &corpus) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = report.iterations == 10_000
        && report.violations == 0
        && report.accepted.saturating_add(report.rejected) == 10_000
        && report.mutated > 0;
    let det = format!(
        "iter={} acc={} rej={} viol={} mut={}",
        report.iterations, report.accepted, report.rejected, report.violations, report.mutated
    );
    s.add_dyn("50130-J1-万轮零崩", ok, &det);

    // J1-6 实验确定性：同种子两次逐位一致（摘要/直方图/接受数）
    let again = match fuzz(0x5eed_1234_9876_abcd, 10_000, &corpus) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = again.digest == report.digest
        && again.accepted == report.accepted
        && again.rejected == report.rejected
        && again.hist == report.hist;
    s.add("50130-J1-实验可复现", ok, if ok { "" } else { "同种子摘要漂移" });

    // J1-7 实验敏感性：换种子结果必须不同（否则等于没在测随机面）
    let other = match fuzz(0x0f0f_0f0f_1234_5678, 2_000, &corpus) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = other.digest != report.digest;
    s.add("50130-J1-实验敏感", ok, if ok { "" } else { "换种子结果不变" });

    // J1-8 变异真的制造了拒绝面：拒绝数 > 0 且直方图至少 4 类非零
    let mut kinds = 0usize;
    let mut i = 0usize;
    while i < HIST_SLOTS {
        if report.hist.get(i).copied().unwrap_or(0) > 0 {
            kinds += 1;
        }
        i += 1;
    }
    let ok = report.rejected > 0 && kinds >= 4;
    let det = format!("rej={} kinds={}", report.rejected, kinds);
    s.add_dyn("50130-J1-拒绝面", ok, &det);

    // J1-9 七类变异各自可施加，且都能改变缓冲区（或明确报不适用）
    let mut rng = match XorShift64::new(0x0123_4567_89ab_cdef) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let base = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let other_seed = match build_seed(2) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut applied = 0usize;
    let mut changed = 0usize;
    let mut op = 0u8;
    while op < 7 {
        let m = match Mutation::from_wire(op) {
            Ok(m) => m,
            Err(_) => return s,
        };
        let mut buf: Vec<u8> = Vec::with_capacity(base.len() + 32);
        buf.extend_from_slice(&base);
        let before_len = buf.len();
        let before_hash = digest_prefix(&buf, buf.len());
        if m.apply(&mut buf, &mut rng, &other_seed).is_ok() {
            applied += 1;
            if buf.len() != before_len || digest_prefix(&buf, buf.len()) != before_hash {
                changed += 1;
            }
        }
        op += 1;
    }
    let ok = applied == 7 && changed >= 5 && Mutation::from_wire(7).err() == Some(FuzzError::Op);
    let det = format!("applied={}/7 changed={}", applied, changed);
    s.add_dyn("50130-J1-七类变异", ok, &det);

    // J1-10 边界：对 1 字节缓冲，截断/清零/拼接各自返回具体码
    let mut one = Vec::new();
    one.push(0u8);
    let mut one2 = Vec::new();
    one2.push(0u8);
    let ok = Mutation::Truncate.apply(&mut one, &mut rng, &other_seed).err() == Some(FuzzError::Trunc)
        && Mutation::ZeroLen.apply(&mut one2, &mut rng, &other_seed).err() == Some(FuzzError::Bound)
        && Mutation::Splice
            .apply(&mut one2, &mut rng, &[])
            .err()
            == Some(FuzzError::Empty)
        && Mutation::FlipBit
            .apply(&mut Vec::new(), &mut rng, &other_seed)
            .err()
            == Some(FuzzError::Empty);
    s.add("50130-J1-变异边界", ok, if ok { "" } else { "边界未按码拒绝" });

    // J1-11 扫描错误族：每个结构错误都返回它自己的码，不合并成泛错
    let e = match build_seed(0) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut dev_bad = e.clone();
    if let Some(x) = dev_bad.get_mut(0) {
        *x = 17;
    }
    let mut cfg_len = e.clone();
    if let Some(x) = cfg_len.get_mut(18) {
        *x = 7;
    }
    let mut cfg_type = e.clone();
    if let Some(x) = cfg_type.get_mut(19) {
        *x = 4;
    }
    let mut zero = e.clone();
    if let Some(x) = zero.get_mut(27) {
        *x = 1;
    }
    let mut total = e.clone();
    if let Some(x) = total.get_mut(20) {
        *x = 0x40;
    }
    if let Some(x) = total.get_mut(21) {
        *x = 0x40;
    }
    let ok = scan(&[]).err() == Some(FuzzError::Empty)
        && scan(&dev_bad).err() == Some(FuzzError::DevLen)
        && scan(&cfg_len).err() == Some(FuzzError::CfgLen)
        && scan(&cfg_type).err() == Some(FuzzError::CfgType)
        && scan(&zero).err() == Some(FuzzError::ZeroLen)
        && scan(&total).err() == Some(FuzzError::Total);
    s.add("50130-J1-错误分族", ok, if ok { "" } else { "结构错误被合并" });

    // J1-12 语料闸：容量为 0/超容/空样本都显式拒绝，不静默少装
    let mut cap1 = match FuzzCorpus::new(1) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let first = cap1.push(base.clone());
    let second = cap1.push(other_seed.clone());
    let empty_push = cap1.push(Vec::new());
    let ok = FuzzCorpus::new(0).err() == Some(FuzzError::Capacity)
        && FuzzCorpus::new(9).err() == Some(FuzzError::Capacity)
        && first == Ok(0)
        && second.err() == Some(FuzzError::Capacity)
        && empty_push.err() == Some(FuzzError::Empty)
        && cap1.len() == 1;
    s.add("50130-J1-语料闸", ok, if ok { "" } else { "语料闸失守" });

    // J1-13 实验参数闸：0 轮/空语料/零种子都拒绝
    let empty_corpus = match FuzzCorpus::new(1) {
        Ok(c) => c,
        Err(_) => return s,
    };
    let ok = fuzz(1, 0, &corpus).err() == Some(FuzzError::Empty)
        && fuzz(0, 10, &corpus).err() == Some(FuzzError::Seed)
        && fuzz(1, 10, &empty_corpus).err() == Some(FuzzError::Empty);
    s.add("50130-J1-参数闸", ok, if ok { "" } else { "实验参数闸失守" });

    // J1-14 注入核：十三类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50130-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50130_checks_green() {
        let set = run_unx50130_checks();
        assert!(set.all_passed(), "UNX-F50130 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50130_fuzz_ten_thousand_no_violation() {
        let corpus = FuzzCorpus::builtin();
        assert!(corpus.is_ok());
        if let Ok(c) = corpus {
            let r = fuzz(0xabcd_ef01_2345_6789, 10_000, &c);
            assert!(r.is_ok());
            if let Ok(r) = r {
                assert_eq!(r.iterations, 10_000);
                assert_eq!(r.violations, 0);
                assert_eq!(r.accepted + r.rejected, 10_000);
            }
        }
    }

    #[test]
    fn unx50130_scan_never_exceeds_buffer() {
        let corpus = FuzzCorpus::builtin();
        assert!(corpus.is_ok());
        if let Ok(c) = corpus {
            let mut i = 0usize;
            while i < c.len() {
                if let Some(b) = c.get(i) {
                    if let Ok(sum) = scan(b) {
                        assert!(sum.consumed as usize <= b.len());
                    }
                }
                i += 1;
            }
        }
    }

    #[test]
    fn unx50130_error_codes_in_segment() {
        assert_eq!(FuzzError::Empty.code(), SEG_BASE + 1);
        assert_eq!(FuzzError::Op.code(), SEG_BASE + 13);
    }
}
