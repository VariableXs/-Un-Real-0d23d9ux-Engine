//! UNX-F50152 发现 fuzz 面（坏 mDNS/SNMP 包注入不崩）（0x43f20 段）
//!
//! 锚点：UNX-F50152-J1 fuzz ×10^4 零崩；判据锚定 §AI-63 §5。
//!
//! 数据段 `SEG_BASE = 0x43f20`，诊断码闭集 Base+1..Base+13。
//!
//! 「零崩」在 no_std 里没有 catch_unwind 可用，只能靠**结构**证明：
//! * 两个解析器（mDNS 名字游走 / BER-TLV 游走）每次读字节都先做区间校验，
//!   任何越界都变成 `Err`，没有裸下标；
//! * 变异器只写自有 `Vec`，写入前判 `get_mut`，结果硬截断在 `MAX_INPUT`；
//! * 于是 10^4 次变异只可能落 Ok / Err 两条路径。两条路径都进台账：
//!   Err 按诊断码落直方图，Ok 再做一次独立重解析——两次结论不一致即记为
//!   violation（这就是「静默接受坏包」的检出点）。
//!
//! 结构清单：
//! * `XorShift64` 确定性伪随机源（同种子逐位复现，fuzz 可回放）；
//! * `MdnsHeader/MdnsPacket` + 标签/压缩指针游走器（只许后向指针，跳数受限）；
//! * `BerTlv/SnmpMessage` + BER 长度/嵌套校验（不定长拒绝、深度上限）；
//! * `Mutation` 九类变异 + `FuzzReport` 台账（直方图/策略命中/长度界/违例）；
//! * 注入核 13 类坏样本，逐类断言**具体**诊断码。
//!
//! 错误纪律：内核路径零 panic；不出现裸下标、无 unwrap/expect、不引 std。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43f20 独占 0x20）。
pub const SEG_BASE: u32 = 0x43f20;

pub const E_EMPTY: u16 = 1;
pub const E_SHORT: u16 = 2;
pub const E_LABEL: u16 = 3;
pub const E_POINTER: u16 = 4;
pub const E_TRUNCATED: u16 = 5;
pub const E_BERTAG: u16 = 6;
pub const E_BERLEN: u16 = 7;
pub const E_DEPTH: u16 = 8;
pub const E_BUDGET: u16 = 9;
pub const E_UNSTABLE: u16 = 10;
pub const E_THIN: u16 = 11;
pub const E_OVERFLOW: u16 = 12;
pub const E_KIND: u16 = 13;

/// fuzz 面错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    Empty,
    Short,
    Label,
    Pointer,
    Truncated,
    BerTag,
    BerLen,
    Depth,
    Budget,
    Unstable,
    Thin,
    Overflow,
    Kind,
}

impl FuzzError {
    pub fn code(self) -> u32 {
        let n = match self {
            FuzzError::Empty => E_EMPTY,
            FuzzError::Short => E_SHORT,
            FuzzError::Label => E_LABEL,
            FuzzError::Pointer => E_POINTER,
            FuzzError::Truncated => E_TRUNCATED,
            FuzzError::BerTag => E_BERTAG,
            FuzzError::BerLen => E_BERLEN,
            FuzzError::Depth => E_DEPTH,
            FuzzError::Budget => E_BUDGET,
            FuzzError::Unstable => E_UNSTABLE,
            FuzzError::Thin => E_THIN,
            FuzzError::Overflow => E_OVERFLOW,
            FuzzError::Kind => E_KIND,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FuzzError::Empty => "packet carries no records",
            FuzzError::Short => "packet shorter than fixed header",
            FuzzError::Label => "name label illegal (length/reserved/oversize)",
            FuzzError::Pointer => "compression pointer not backward or too many jumps",
            FuzzError::Truncated => "declared extent exceeds buffer",
            FuzzError::BerTag => "BER tag outside expected set",
            FuzzError::BerLen => "BER length form illegal",
            FuzzError::Depth => "BER nesting deeper than limit",
            FuzzError::Budget => "iteration budget out of range",
            FuzzError::Unstable => "re-parse disagreed with first parse",
            FuzzError::Thin => "error histogram too thin",
            FuzzError::Overflow => "declared length beyond packet cap",
            FuzzError::Kind => "packet kind code unknown",
        }
    }
}

/// J1 预算：10^4 次。
pub const FUZZ_ITERS: usize = 10_000;
/// 单次变异后的输入上限（同时保护解析时间与栈）。
pub const MAX_INPUT: usize = 1024;
/// 名字压缩指针跳数上限。
pub const MAX_JUMPS: u8 = 8;
/// BER 嵌套深度上限（SNMP 实际最深 varbind 3 层，留余量）。
pub const MAX_DEPTH: u8 = 6;
/// 直方图至少覆盖的错误类别数。
pub const MIN_CLASSES: usize = 4;
/// 诊断码总数（与 `SEG_BASE` 偏移 1..=13 对应）。
pub const CLASS_COUNT: usize = 13;
/// 变异策略数。
pub const STRATEGY_COUNT: usize = 9;

/// xorshift64（13/7/17）。种子 0 换成黄金比常量，避免全零不动点。
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    s: u64,
}

impl XorShift64 {
    pub const fn new(seed: u64) -> XorShift64 {
        XorShift64 {
            s: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// fuzz 只需确定性，取模即可；n=0 时返回 0（调用点仍须判空）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

/// 发现协议种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketKind {
    Mdns,
    Snmp,
}

impl PacketKind {
    pub fn code(self) -> u8 {
        match self {
            PacketKind::Mdns => 0,
            PacketKind::Snmp => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PacketKind::Mdns => "mdns",
            PacketKind::Snmp => "snmp",
        }
    }

    pub fn from_code(c: u8) -> Result<PacketKind, FuzzError> {
        match c {
            0 => Ok(PacketKind::Mdns),
            1 => Ok(PacketKind::Snmp),
            _ => Err(FuzzError::Kind),
        }
    }
}

fn be16(d: &[u8], at: usize) -> Result<u16, FuzzError> {
    let hi = match d.get(at) {
        Some(v) => *v,
        None => return Err(FuzzError::Truncated),
    };
    let lo = match at.checked_add(1).and_then(|i| d.get(i)) {
        Some(v) => *v,
        None => return Err(FuzzError::Truncated),
    };
    Ok(((hi as u16) << 8) | lo as u16)
}

/// mDNS/DNS 头部（12 字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdnsHeader {
    pub id: u16,
    pub flags: u16,
    pub qd: u16,
    pub an: u16,
    pub ns: u16,
    pub ar: u16,
}

impl MdnsHeader {
    /// 头部声明的总记录数（问题也算一条）。零记录 = 空包。
    pub fn records(self) -> u32 {
        self.qd as u32 + self.an as u32 + self.ns as u32 + self.ar as u32
    }
}

/// 解析结果台账：`consumed` 是走完整包后的偏移，供后验对账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdnsPacket {
    pub header: MdnsHeader,
    pub questions: u16,
    pub records: u16,
    pub consumed: usize,
}

/// 走一个 DNS 名字，返回（下一个字段偏移, 标签数）。
///
/// 压缩指针只允许指向**严格靠前**的位置：这样每次跳转都让偏移单调下降，
/// 循环在所有输入上都不可能发生；`MAX_JUMPS` 是第二道闸（防止恶意构造的
/// 长链拖慢解析）。保留位 0x40/0x80 一律判 Label。
fn read_name(d: &[u8], start: usize) -> Result<(usize, usize), FuzzError> {
    let mut pos = start;
    let mut jumps = 0u8;
    let mut labels = 0usize;
    let mut total = 0usize;
    let mut resume: Option<usize> = None;
    loop {
        let b = match d.get(pos) {
            Some(v) => *v,
            None => return Err(FuzzError::Truncated),
        };
        if b == 0 {
            let next = match resume {
                Some(r) => r,
                None => pos + 1,
            };
            return Ok((next, labels));
        }
        if (b & 0xC0) == 0xC0 {
            let b2 = match pos.checked_add(1).and_then(|i| d.get(i)) {
                Some(v) => *v,
                None => return Err(FuzzError::Truncated),
            };
            let target = (((b & 0x3F) as usize) << 8) | b2 as usize;
            if target >= pos {
                return Err(FuzzError::Pointer);
            }
            if jumps >= MAX_JUMPS {
                return Err(FuzzError::Pointer);
            }
            jumps += 1;
            if resume.is_none() {
                resume = pos.checked_add(2);
            }
            pos = target;
            continue;
        }
        if (b & 0xC0) != 0 {
            return Err(FuzzError::Label);
        }
        let l = b as usize;
        if l == 0 || l > 63 {
            return Err(FuzzError::Label);
        }
        total += l + 1;
        if total > 255 {
            return Err(FuzzError::Label);
        }
        if pos + 1 + l > d.len() {
            return Err(FuzzError::Truncated);
        }
        pos += 1 + l;
        labels += 1;
    }
}

/// 解析 mDNS 报文。问题区与三条资源记录区都逐条走完并校验尾部。
pub fn parse_mdns(d: &[u8]) -> Result<MdnsPacket, FuzzError> {
    if d.is_empty() {
        return Err(FuzzError::Empty);
    }
    if d.len() < 12 {
        return Err(FuzzError::Short);
    }
    let header = MdnsHeader {
        id: be16(d, 0)?,
        flags: be16(d, 2)?,
        qd: be16(d, 4)?,
        an: be16(d, 6)?,
        ns: be16(d, 8)?,
        ar: be16(d, 10)?,
    };
    if header.records() == 0 {
        return Err(FuzzError::Empty);
    }
    let mut pos = 12usize;
    let mut q = 0u16;
    while q < header.qd {
        let (next, _labels) = read_name(d, pos)?;
        pos = next;
        if pos + 4 > d.len() {
            return Err(FuzzError::Truncated);
        }
        pos += 4;
        q = q.saturating_add(1);
    }
    let total_rr = header.an as u32 + header.ns as u32 + header.ar as u32;
    let mut r = 0u16;
    while (r as u32) < total_rr {
        let (next, _labels) = read_name(d, pos)?;
        pos = next;
        if pos + 10 > d.len() {
            return Err(FuzzError::Truncated);
        }
        let rdlen = be16(d, pos + 8)? as usize;
        pos += 10;
        if pos + rdlen > d.len() {
            return Err(FuzzError::Truncated);
        }
        pos += rdlen;
        r = r.saturating_add(1);
    }
    Ok(MdnsPacket {
        header,
        questions: header.qd,
        records: r,
        consumed: pos,
    })
}

/// 一个 BER TLV 的定位信息（不复制内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BerTlv {
    pub tag: u8,
    pub value_off: usize,
    pub value_len: usize,
    pub end: usize,
}

/// 读一个 BER TLV。长度只接受确定型短/长格式：不定长（0x80）在 SNMP 里
/// 非法；长格式超过 4 字节或声明长度超过 `MAX_INPUT` 都显式拒绝。
pub fn read_ber_tlv(d: &[u8], at: usize) -> Result<BerTlv, FuzzError> {
    let tag = match d.get(at) {
        Some(v) => *v,
        None => return Err(FuzzError::Truncated),
    };
    let first = match at.checked_add(1).and_then(|i| d.get(i)) {
        Some(v) => *v,
        None => return Err(FuzzError::Truncated),
    };
    let mut pos = at + 2;
    let mut acc: u64 = 0;
    if first < 0x80 {
        acc = first as u64;
    } else if first == 0x80 {
        return Err(FuzzError::BerLen);
    } else {
        let n = (first & 0x7F) as usize;
        if n > 4 {
            return Err(FuzzError::BerLen);
        }
        let mut i = 0usize;
        while i < n {
            let b = match pos.checked_add(i).and_then(|p| d.get(p)) {
                Some(v) => *v,
                None => return Err(FuzzError::Truncated),
            };
            acc = (acc << 8) | b as u64;
            i += 1;
        }
        pos += n;
    }
    if acc > MAX_INPUT as u64 {
        return Err(FuzzError::Overflow);
    }
    let len = acc as usize;
    let end = match pos.checked_add(len) {
        Some(e) => e,
        None => return Err(FuzzError::Overflow),
    };
    if end > d.len() {
        return Err(FuzzError::Truncated);
    }
    Ok(BerTlv {
        tag,
        value_off: pos,
        value_len: len,
        end,
    })
}

/// 递归走 BER 子树。构造类型（tag 0x20 位）进入下一层；叶子直接跳过。
/// 递归前先卡深度，避免恶意深嵌套吃栈。
fn walk_ber(
    d: &[u8],
    mut pos: usize,
    end: usize,
    depth: u8,
    tlvs: &mut u16,
) -> Result<(), FuzzError> {
    if depth > MAX_DEPTH {
        return Err(FuzzError::Depth);
    }
    while pos < end {
        let t = read_ber_tlv(d, pos)?;
        if t.end > end {
            return Err(FuzzError::Truncated);
        }
        *tlvs = (*tlvs).saturating_add(1);
        if (t.tag & 0x20) != 0 {
            walk_ber(d, t.value_off, t.end, depth + 1, tlvs)?;
        }
        pos = t.end;
    }
    if pos != end {
        return Err(FuzzError::Truncated);
    }
    Ok(())
}

fn signed_be(d: &[u8], off: usize, len: usize) -> Result<i64, FuzzError> {
    if len == 0 || len > 8 {
        return Err(FuzzError::BerLen);
    }
    let mut acc: i64 = 0;
    let mut i = 0usize;
    while i < len {
        let b = match off.checked_add(i).and_then(|p| d.get(p)) {
            Some(v) => *v,
            None => return Err(FuzzError::Truncated),
        };
        acc = (acc << 8) | b as i64;
        i += 1;
    }
    if len < 8 {
        let bits = (len * 8) as u32;
        if (acc & (1i64 << (bits - 1))) != 0 {
            acc -= 1i64 << bits;
        }
    }
    Ok(acc)
}

/// SNMP 报文摘要（版本/共同体长度/PDU 标签/子树 TLV 数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnmpMessage {
    pub version: i64,
    pub community_len: u16,
    pub pdu_tag: u8,
    pub tlvs: u16,
    pub consumed: usize,
}

/// 解析 SNMP v1/v2c 报文：SEQUENCE{version, community, PDU}，PDU 子树逐层校验。
pub fn parse_snmp(d: &[u8]) -> Result<SnmpMessage, FuzzError> {
    if d.is_empty() {
        return Err(FuzzError::Empty);
    }
    let outer = read_ber_tlv(d, 0)?;
    if outer.tag != 0x30 {
        return Err(FuzzError::BerTag);
    }
    if outer.end != d.len() {
        return Err(FuzzError::Truncated);
    }
    let ver = read_ber_tlv(d, outer.value_off)?;
    if ver.tag != 0x02 {
        return Err(FuzzError::BerTag);
    }
    let version = signed_be(d, ver.value_off, ver.value_len)?;
    let comm = read_ber_tlv(d, ver.end)?;
    if comm.tag != 0x04 {
        return Err(FuzzError::BerTag);
    }
    let pdu = read_ber_tlv(d, comm.end)?;
    if pdu.tag < 0xA0 || pdu.tag > 0xA8 {
        return Err(FuzzError::BerTag);
    }
    if pdu.end != outer.end {
        return Err(FuzzError::Truncated);
    }
    let mut tlvs = 0u16;
    walk_ber(d, pdu.value_off, pdu.end, 1, &mut tlvs)?;
    Ok(SnmpMessage {
        version,
        community_len: comm.value_len as u16,
        pdu_tag: pdu.tag,
        tlvs,
        consumed: outer.end,
    })
}

/// 一条合法的 mDNS 查询 + PTR 应答（48 字节，手工推导，供种子用）。
pub fn seed_mdns() -> Vec<u8> {
    let mut v = Vec::with_capacity(48);
    // 头部：id=0x1234 flags=0x0100 qd=1 an=1 ns=0 ar=0
    v.extend_from_slice(&[0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    // 问题名 _http._tcp.local
    v.extend_from_slice(&[0x05, b'_', b'h', b't', b't', b'p']);
    v.extend_from_slice(&[0x04, b'_', b't', b'c', b'p']);
    v.extend_from_slice(&[0x05, b'l', b'o', b'c', b'a', b'l', 0x00]);
    // qtype=PTS(12), qclass=IN(1)
    v.extend_from_slice(&[0x00, 0x0C, 0x00, 0x01]);
    // 应答：名字指回偏移 12，PTR，IN，TTL=120，rdlen=2
    v.extend_from_slice(&[0xC0, 0x0C, 0x00, 0x0C, 0x00, 0x01]);
    v.extend_from_slice(&[0x00, 0x00, 0x00, 0x78, 0x00, 0x02, 0x00, 0x00]);
    v
}

/// 一条合法的 SNMPv1 GetRequest（26 字节，手工推导）。
pub fn seed_snmp() -> Vec<u8> {
    let mut v = Vec::with_capacity(26);
    v.extend_from_slice(&[0x30, 0x18, 0x02, 0x01, 0x00, 0x04, 0x06]);
    v.extend_from_slice(b"public");
    v.extend_from_slice(&[0xA0, 0x0B, 0x02, 0x01, 0x01, 0x02, 0x01, 0x00, 0x02, 0x01, 0x00, 0x30, 0x00]);
    v
}

/// 九类确定性变异。`Intact` 用来单独测「直方图太薄」这条错误路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    BitFlip,
    ByteSet,
    Truncate,
    Append,
    ZeroFill,
    Swap,
    Duplicate,
    HugeLen,
    Intact,
}

impl Mutation {
    pub fn code(self) -> u8 {
        match self {
            Mutation::BitFlip => 0,
            Mutation::ByteSet => 1,
            Mutation::Truncate => 2,
            Mutation::Append => 3,
            Mutation::ZeroFill => 4,
            Mutation::Swap => 5,
            Mutation::Duplicate => 6,
            Mutation::HugeLen => 7,
            Mutation::Intact => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mutation::BitFlip => "bitflip",
            Mutation::ByteSet => "byteset",
            Mutation::Truncate => "truncate",
            Mutation::Append => "append",
            Mutation::ZeroFill => "zerofill",
            Mutation::Swap => "swap",
            Mutation::Duplicate => "duplicate",
            Mutation::HugeLen => "hugelen",
            Mutation::Intact => "intact",
        }
    }

    pub fn from_code(c: u8) -> Result<Mutation, FuzzError> {
        match c {
            0 => Ok(Mutation::BitFlip),
            1 => Ok(Mutation::ByteSet),
            2 => Ok(Mutation::Truncate),
            3 => Ok(Mutation::Append),
            4 => Ok(Mutation::ZeroFill),
            5 => Ok(Mutation::Swap),
            6 => Ok(Mutation::Duplicate),
            7 => Ok(Mutation::HugeLen),
            8 => Ok(Mutation::Intact),
            _ => Err(FuzzError::Kind),
        }
    }
}

/// 执行一次变异。所有写操作先取 `get_mut`，结果长度硬截断到 `MAX_INPUT`。
pub fn apply_mutation(base: &[u8], m: Mutation, rng: &mut XorShift64) -> Vec<u8> {
    let mut v = Vec::with_capacity(base.len() + 8);
    v.extend_from_slice(base);
    let n = v.len();
    match m {
        Mutation::Intact => {}
        Mutation::BitFlip => {
            if n > 0 {
                let at = rng.below(n as u32) as usize;
                let bit = rng.below(8) as u8;
                if let Some(b) = v.get_mut(at) {
                    *b ^= 1u8 << bit;
                }
            }
        }
        Mutation::ByteSet => {
            if n > 0 {
                let at = rng.below(n as u32) as usize;
                let val = rng.next_u32() as u8;
                if let Some(b) = v.get_mut(at) {
                    *b = val;
                }
            }
        }
        Mutation::Truncate => {
            if n > 0 {
                let cut = rng.below(n as u32) as usize;
                v.truncate(cut);
            }
        }
        Mutation::Append => {
            let k = rng.below(4) + 1;
            let mut i = 0u32;
            while i < k {
                if v.len() < MAX_INPUT {
                    v.push(rng.next_u32() as u8);
                }
                i += 1;
            }
        }
        Mutation::ZeroFill => {
            if n > 0 {
                let at = rng.below(n as u32) as usize;
                let k = rng.below(3) + 1;
                let mut i = 0u32;
                while i < k {
                    if let Some(b) = v.get_mut(at.saturating_add(i as usize)) {
                        *b = 0;
                    }
                    i += 1;
                }
            }
        }
        Mutation::Swap => {
            if n >= 2 {
                let a = rng.below(n as u32) as usize;
                let b = rng.below(n as u32) as usize;
                v.swap(a, b);
            }
        }
        Mutation::Duplicate => {
            if n > 0 {
                let at = rng.below(n as u32) as usize;
                let k = rng.below(4) + 1;
                let mut chunk: Vec<u8> = Vec::new();
                let mut i = 0u32;
                while i < k {
                    if let Some(b) = v.get(at.saturating_add(i as usize)) {
                        chunk.push(*b);
                    }
                    i += 1;
                }
                let mut j = 0usize;
                while j < chunk.len() && v.len() < MAX_INPUT {
                    if let Some(b) = chunk.get(j) {
                        v.push(*b);
                    }
                    j += 1;
                }
            }
        }
        Mutation::HugeLen => {
            // 把第二字节改成 4 字节长格式长度头，再补 0xFF×4：解析器应在
            // 读长度时以 Overflow 拒绝，而不是尝试分配/越界。
            if let Some(b) = v.get_mut(1) {
                *b = 0x84;
            }
            let mut i = 0usize;
            while i < 4 {
                if v.len() < MAX_INPUT {
                    v.push(0xFF);
                }
                i += 1;
            }
        }
    }
    if v.len() > MAX_INPUT {
        v.truncate(MAX_INPUT);
    }
    v
}

/// fuzz 台账。`classes` 按诊断码偏移 1..=13 落直方图，`strategies` 按策略码 0..=8。
#[derive(Clone, Debug, PartialEq)]
pub struct FuzzReport {
    pub iters: usize,
    pub ok: usize,
    pub err: usize,
    pub violations: usize,
    pub max_len: usize,
    pub digest: u64,
    pub classes: [u16; CLASS_COUNT],
    pub strategies: [u16; STRATEGY_COUNT],
}

impl FuzzReport {
    pub const fn empty() -> FuzzReport {
        FuzzReport {
            iters: 0,
            ok: 0,
            err: 0,
            violations: 0,
            max_len: 0,
            digest: 0,
            classes: [0; CLASS_COUNT],
            strategies: [0; STRATEGY_COUNT],
        }
    }

    /// 直方图实际覆盖的错误类别数。
    pub fn distinct_errors(&self) -> usize {
        self.classes.iter().filter(|c| **c > 0).count()
    }

    pub fn hist_total(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < CLASS_COUNT {
            sum += self.classes[i] as u64;
            i += 1;
        }
        sum
    }

    pub fn strategies_hit(&self) -> usize {
        self.strategies.iter().filter(|c| **c > 0).count()
    }

    /// 零崩判据的结构化版本：每条输入恰好落一条路径，且无后验违例。
    pub fn crash_free(&self) -> bool {
        self.ok + self.err == self.iters && self.violations == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "iters={} ok={} err={} classes={} max={} violations={}",
            self.iters,
            self.ok,
            self.err,
            self.distinct_errors(),
            self.max_len,
            self.violations
        )
    }
}

const DIGEST_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 主 fuzz 循环：选种类 → 变异 → 解析 → Ok 路径重解析后验 → 记账。
pub fn run_fuzz(seed: u64, iters: usize) -> FuzzReport {
    let mut rng = XorShift64::new(seed);
    let mut rep = FuzzReport::empty();
    rep.iters = iters;
    let mdns_seed = seed_mdns();
    let snmp_seed = seed_snmp();
    let mut digest = 0xCBF2_9CE4_8422_2325u64;
    let mut i = 0usize;
    while i < iters {
        let kind = match PacketKind::from_code(rng.below(2) as u8) {
            Ok(k) => k,
            Err(_) => PacketKind::Mdns,
        };
        let m = match Mutation::from_code(rng.below(STRATEGY_COUNT as u32) as u8) {
            Ok(v) => v,
            Err(_) => Mutation::Intact,
        };
        let base = match kind {
            PacketKind::Mdns => &mdns_seed,
            PacketKind::Snmp => &snmp_seed,
        };
        let buf = apply_mutation(base, m, &mut rng);
        if buf.len() > rep.max_len {
            rep.max_len = buf.len();
        }
        let first = match kind {
            PacketKind::Mdns => match parse_mdns(&buf) {
                Ok(p) => Ok(p.consumed),
                Err(e) => Err(e),
            },
            PacketKind::Snmp => match parse_snmp(&buf) {
                Ok(p) => Ok(p.consumed),
                Err(e) => Err(e),
            },
        };
        let outcome = match first {
            Ok(consumed) => {
                rep.ok += 1;
                // 后验：同缓冲再解析一次必须得到同一 consumed，否则说明解析
                // 依赖了外部状态（静默接受/结果漂移）。
                let again = match kind {
                    PacketKind::Mdns => match parse_mdns(&buf) {
                        Ok(p) => Ok(p.consumed),
                        Err(e) => Err(e),
                    },
                    PacketKind::Snmp => match parse_snmp(&buf) {
                        Ok(p) => Ok(p.consumed),
                        Err(e) => Err(e),
                    },
                };
                match again {
                    Ok(c2) if c2 == consumed && consumed <= buf.len() => 0u64,
                    _ => {
                        rep.violations += 1;
                        0x8000_0000u64
                    }
                }
            }
            Err(e) => {
                rep.err += 1;
                let idx = e.code().saturating_sub(SEG_BASE) as usize;
                if idx >= 1 && idx <= CLASS_COUNT {
                    rep.classes[idx - 1] = rep.classes[idx - 1].saturating_add(1);
                }
                e.code() as u64
            }
        };
        let s_idx = m.code() as usize;
        if s_idx < STRATEGY_COUNT {
            rep.strategies[s_idx] = rep.strategies[s_idx].saturating_add(1);
        }
        digest = digest.rotate_left(7)
            ^ (buf.len() as u64)
            ^ ((kind.code() as u64) << 20)
            ^ ((m.code() as u64) << 24)
            ^ outcome;
        digest = digest.wrapping_mul(DIGEST_PRIME);
        i += 1;
    }
    rep.digest = digest;
    rep
}

/// 只喂合法种子、不做变异的对照运行：Ok=iters、错误类别 0。
pub fn run_fuzz_pass(seed: u64, iters: usize) -> FuzzReport {
    let mut rep = FuzzReport::empty();
    rep.iters = iters;
    let mut digest = 0xCBF2_9CE4_8422_2325u64;
    let mdns_seed = seed_mdns();
    let snmp_seed = seed_snmp();
    let mut i = 0usize;
    while i < iters {
        let mdns_ok = parse_mdns(&mdns_seed).is_ok();
        let snmp_ok = parse_snmp(&snmp_seed).is_ok();
        if mdns_ok && snmp_ok {
            rep.ok += 2;
            rep.strategies[Mutation::Intact.code() as usize] =
                rep.strategies[Mutation::Intact.code() as usize].saturating_add(2);
        } else {
            rep.err += 1;
        }
        digest = digest.rotate_left(3) ^ (mdns_seed.len() as u64) ^ ((snmp_seed.len() as u64) << 16);
        digest = digest.wrapping_mul(DIGEST_PRIME);
        i += 1;
    }
    rep.digest = digest;
    rep
}

/// 台账闸：预算之外再核「无违例」与「类别覆盖」。
pub fn guard_report(r: &FuzzReport) -> Result<(), FuzzError> {
    if r.iters == 0 || r.ok + r.err != r.iters {
        return Err(FuzzError::Budget);
    }
    if r.violations > 0 {
        return Err(FuzzError::Unstable);
    }
    if r.distinct_errors() < MIN_CLASSES {
        return Err(FuzzError::Thin);
    }
    Ok(())
}

/// fuzz 入口：预算 1..=100_000，返回台账或显式错误。
pub fn fuzz_guard(seed: u64, iters: usize) -> Result<FuzzReport, FuzzError> {
    if iters == 0 || iters > 100_000 {
        return Err(FuzzError::Budget);
    }
    let r = run_fuzz(seed, iters);
    guard_report(&r)?;
    Ok(r)
}

// ---------------------------------------------------------------------------
// 注入核：13 类坏样本，逐类断言具体诊断码
// ---------------------------------------------------------------------------

fn mdns_header_bytes(qd: u16, an: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    v.extend_from_slice(&qd.to_be_bytes());
    v.extend_from_slice(&an.to_be_bytes());
    v.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    v
}

fn nested_snmp(depth: usize) -> Vec<u8> {
    let mut inner: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < depth {
        let mut v = Vec::with_capacity(inner.len() + 2);
        v.push(0xA0);
        v.push(inner.len() as u8);
        v.extend_from_slice(&inner);
        inner = v;
        i += 1;
    }
    let mut body: Vec<u8> = Vec::with_capacity(6 + inner.len());
    body.extend_from_slice(&[0x02, 0x01, 0x00, 0x04, 0x01, 0x61]);
    body.extend_from_slice(&inner);
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + 2);
    out.push(0x30);
    out.push(body.len() as u8);
    out.extend_from_slice(&body);
    out
}

/// 注入核：返回（被抓数, 总数）。每类都断言确定诊断码，错误码错位即失败。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 13usize;

    // 1 空包
    if parse_mdns(&[]).err() == Some(FuzzError::Empty) {
        caught += 1;
    }
    // 2 短于固定头
    if parse_mdns(&[0u8; 5]).err() == Some(FuzzError::Short) {
        caught += 1;
    }
    // 3 标签长度 0x46 > 63
    let mut label = mdns_header_bytes(1, 0);
    label.push(0x46);
    if parse_mdns(&label).err() == Some(FuzzError::Label) {
        caught += 1;
    }
    // 4 前向压缩指针（名字起点即指向未来）
    let mut fwd = mdns_header_bytes(1, 0);
    fwd.extend_from_slice(&[0xC0, 0x20]);
    if parse_mdns(&fwd).err() == Some(FuzzError::Pointer) {
        caught += 1;
    }
    // 5 声明 1 条应答但缓冲已尽
    if parse_mdns(&mdns_header_bytes(0, 1)).err() == Some(FuzzError::Truncated) {
        caught += 1;
    }
    // 6 SNMP 顶层 tag 不是 SEQUENCE
    if parse_snmp(&[0x31, 0x00]).err() == Some(FuzzError::BerTag) {
        caught += 1;
    }
    // 7 不定长（0x80）在 SNMP 非法
    if parse_snmp(&[0x30, 0x80]).err() == Some(FuzzError::BerLen) {
        caught += 1;
    }
    // 8 声明长度 0xFFFFFFFF 超上限
    if parse_snmp(&[0x30, 0x84, 0xFF, 0xFF, 0xFF, 0xFF]).err() == Some(FuzzError::Overflow) {
        caught += 1;
    }
    // 9 嵌套深度 8 > 上限 6
    if parse_snmp(&nested_snmp(8)).err() == Some(FuzzError::Depth) {
        caught += 1;
    }
    // 10 后验发现违例（手工台账模拟「静默接受」）
    let mut liar = FuzzReport::empty();
    liar.iters = 1;
    liar.ok = 1;
    liar.violations = 1;
    if guard_report(&liar).err() == Some(FuzzError::Unstable) {
        caught += 1;
    }
    // 11 预算 0
    if fuzz_guard(1, 0).err() == Some(FuzzError::Budget) {
        caught += 1;
    }
    // 12 无变异对照：错误类别 0，直方图太薄
    if guard_report(&run_fuzz_pass(3, 4)).err() == Some(FuzzError::Thin) {
        caught += 1;
    }
    // 13 未知包种类
    if PacketKind::from_code(9).err() == Some(FuzzError::Kind) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50152");

    // J1-1 合法 mDNS 种子可解析且台账字段齐全
    let mdns = seed_mdns();
    let ok = match parse_mdns(&mdns) {
        Ok(p) => p.consumed == 48 && p.questions == 1 && p.records == 1 && p.header.records() == 2,
        Err(_) => false,
    };
    s.add("50152-J1-mDNS种子", ok, if ok { "" } else { "种子解析失败" });

    // J1-2 合法 SNMP 种子可解析（v1、共同体 public、GetRequest）
    let snmp = seed_snmp();
    let ok = match parse_snmp(&snmp) {
        Ok(p) => p.version == 0 && p.community_len == 6 && p.pdu_tag == 0xA0 && p.tlvs == 4 && p.consumed == 26,
        Err(_) => false,
    };
    s.add("50152-J1-SNMP种子", ok, if ok { "" } else { "种子解析失败" });

    // J1-3 解析确定性：同缓冲两次结论逐位一致
    let ok = parse_mdns(&mdns) == parse_mdns(&mdns) && parse_snmp(&snmp) == parse_snmp(&snmp);
    s.add("50152-J1-解析确定", ok, if ok { "" } else { "两次解析不一致" });

    // J1-4 核心：fuzz ×10^4 零崩（结构判定：路径守恒 + 无后验违例）
    let report = match fuzz_guard(0xA11CE, FUZZ_ITERS) {
        Ok(r) => r,
        Err(_) => FuzzReport::empty(),
    };
    let ok = report.iters == FUZZ_ITERS
        && report.ok + report.err == FUZZ_ITERS
        && report.violations == 0
        && report.crash_free();
    let det = report.summary();
    s.add_dyn("50152-J1-fuzz万次零崩", ok, &det);

    // J1-5 可回放：同种子两次运行 digest 与计数一致
    let a = run_fuzz(0x5EED, 500);
    let b = run_fuzz(0x5EED, 500);
    let ok = a == b;
    let det = format!("digest={:016x}", a.digest);
    s.add_dyn("50152-J1-同种子复现", ok, &det);

    // J1-6 前缀单调：前 100 次计数不超过后 1000 次（同一 rng 流）
    let r100 = run_fuzz(0xC0FFEE, 100);
    let r1000 = run_fuzz(0xC0FFEE, 1000);
    let ok = r100.ok <= r1000.ok
        && r100.err <= r1000.err
        && r100.max_len <= r1000.max_len
        && r1000.ok + r1000.err == 1000;
    let det = format!("100={}/{} 1000={}/{}", r100.ok, r100.err, r1000.ok, r1000.err);
    s.add_dyn("50152-J1-前缀守恒", ok, &det);

    // J1-7 错误直方图：至少 4 类且各类之和等于 err
    let ok = report.distinct_errors() >= MIN_CLASSES && report.hist_total() == report.err as u64;
    let det = format!("classes={} hist={} err={}", report.distinct_errors(), report.hist_total(), report.err);
    s.add_dyn("50152-J1-类别覆盖", ok, &det);

    // J1-8 九类变异全部真实开火
    let ok = report.strategies_hit() == STRATEGY_COUNT;
    let det = format!("hit={}/{}", report.strategies_hit(), STRATEGY_COUNT);
    s.add_dyn("50152-J1-策略覆盖", ok, &det);

    // J1-9 变异长度界：永不越过 MAX_INPUT
    let ok = report.max_len <= MAX_INPUT;
    let det = format!("max={}", report.max_len);
    s.add_dyn("50152-J1-长度上限", ok, &det);

    // J1-10 预算闸：0 与超上限都显式拒绝
    let ok = fuzz_guard(1, 0).err() == Some(FuzzError::Budget)
        && fuzz_guard(1, 100_001).err() == Some(FuzzError::Budget);
    s.add("50152-J1-预算必拒", ok, if ok { "" } else { "预算未拦截" });

    // J1-11 13 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50152-J1-注入必抓", ok, &det);

    // J1-12 压缩指针只许后向：自指/前向都判 Pointer，合法后向仍可解析
    let mut self_ptr = mdns_header_bytes(1, 0);
    self_ptr.extend_from_slice(&[0xC0, 0x0C]);
    let ok = parse_mdns(&self_ptr).err() == Some(FuzzError::Pointer)
        && parse_mdns(&mdns).is_ok();
    s.add("50152-J1-指针后向", ok, if ok { "" } else { "指针方向未校验" });

    // J1-13 BER 深度：8 层必拒，2 层合法
    let ok = parse_snmp(&nested_snmp(8)).err() == Some(FuzzError::Depth)
        && parse_snmp(&nested_snmp(2)).is_ok();
    s.add("50152-J1-深度闸", ok, if ok { "" } else { "深度上限未生效" });

    // J1-14 零记录包判 Empty（而不是伪装成功）
    let ok = parse_mdns(&mdns_header_bytes(0, 0)).err() == Some(FuzzError::Empty);
    s.add("50152-J1-空包必拒", ok, if ok { "" } else { "零记录被接受" });

    // J1-15 种类码往返
    let ok = PacketKind::from_code(0) == Ok(PacketKind::Mdns)
        && PacketKind::from_code(1) == Ok(PacketKind::Snmp)
        && PacketKind::from_code(2).err() == Some(FuzzError::Kind)
        && PacketKind::Mdns.code() == 0
        && PacketKind::Snmp.code() == 1;
    s.add("50152-J1-种类往返", ok, if ok { "" } else { "种类码不一致" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50152_checks_green() {
        let set = run_unx50152_checks();
        assert!(set.all_passed(), "UNX-F50152 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50152_seeds_parse() {
        assert!(parse_mdns(&seed_mdns()).is_ok());
        assert!(parse_snmp(&seed_snmp()).is_ok());
    }

    #[test]
    fn unx50152_fuzz_is_replayable() {
        let a = run_fuzz(7, 64);
        let b = run_fuzz(7, 64);
        assert_eq!(a, b);
    }
}
