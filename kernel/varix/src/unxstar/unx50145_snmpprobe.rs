//! UNX-F50145 SNMPv1 打印 MIB 探针（可选档缺省关闭）（0x43e40 段）
//!
//! 锚点：UNX-F50145-J1 MIB 探测 ×2 模型一致，关闭档零流量断言；
//! 判据锚定 §AI-63 §2 专题五。
//!
//! SNMP 是「能不开就不开」的能力：它不是一个打印必需的通道，而是一扇对
//! 网络敞开的只读窗口。因此本模块的第一条纪律不是解析正确，而是**关闭档
//! 一字不发**：`ProbeAgent::default_off()` 的 `build_request()` 返回 `None`，
//! `poll()` 直接 `Disabled`，`bytes_out/bytes_in/polls` 三个计数器恒为 0。
//! 这条性质用判据钉死，任何未来改动把「关闭」做成「仍然发包只是不显示」
//! 都会立刻变红。
//!
//! 第二条纪律是 BER 长度的**唯一解释**：SNMPv1 的 BER 允许短形式、长形式、
//! 理论上的不定长（0x80）。本解析器只接受短形式与最小化长形式——不定长
//! 直接 `Indefinite`，长形式带前导零（非最小）直接 `LenLong`。宽容解析是
//! 协议歧义与内存越界的温床，这里不做。
//!
//! 数据结构（四类占三类）：
//! * `Tlv` + `read_tlv` —— 带父子边界校验的 BER-TLV 解析器；
//! * `Oid` —— 点分 OID 的编解码（首两段合并、后续 base-128）；
//! * `MibLedger` —— 定容 MIB 结果账本（同 OID 更新不新增条目，单源视图）；
//! * `ProbeAgent` —— 五相状态机（Disabled/Idle/Sent/Parsed/Failed）+ 流量计数。
//!
//! 两个模拟器模型（mdl-a / mdl-b）覆盖不同 community、不同设备描述与状态，
//! J1 的「×2 模型一致」即逐字段对照这两份快照。
//!
//! 错误路径：段内闭集 17 码，全部在注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43e40 独占 0x20）。
pub const SEG_BASE: u32 = 0x43e40;

pub const E_EMPTY: u16 = 1;
pub const E_SHORT: u16 = 2;
pub const E_TAG: u16 = 3;
pub const E_LEN: u16 = 4;
pub const E_LENLONG: u16 = 5;
pub const E_INDEFINITE: u16 = 6;
pub const E_VERSION: u16 = 7;
pub const E_COMMUNITY: u16 = 8;
pub const E_PDU: u16 = 9;
pub const E_REQID: u16 = 10;
pub const E_ERRSTATUS: u16 = 11;
pub const E_ERRINDEX: u16 = 12;
pub const E_VARCOUNT: u16 = 13;
pub const E_OID: u16 = 14;
pub const E_VALUE: u16 = 15;
pub const E_DISABLED: u16 = 16;
pub const E_CAPACITY: u16 = 17;

/// 闭集大小。
pub const ERR_SLOTS: usize = 17;

/// 探测请求 id（固定值即可：本模块是主动探针，不做并发复用）。
pub const REQ_ID: i32 = 0x4321;

/// OID 最大段数。真实 MIB 最深也就二十来段，128 足够且能挡住放大攻击。
pub const MAX_ARCS: usize = 128;

/// SNMPv1 探针错误：`code()` = `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnmpError {
    Empty,
    Short,
    Tag,
    Len,
    LenLong,
    Indefinite,
    Version,
    Community,
    Pdu,
    ReqId,
    ErrStatus,
    ErrIndex,
    VarCount,
    Oid,
    Value,
    Disabled,
    Capacity,
}

impl SnmpError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32 + 1
    }

    /// 账本槽位（0 基）。
    pub fn slot(self) -> usize {
        let n = match self {
            SnmpError::Empty => E_EMPTY,
            SnmpError::Short => E_SHORT,
            SnmpError::Tag => E_TAG,
            SnmpError::Len => E_LEN,
            SnmpError::LenLong => E_LENLONG,
            SnmpError::Indefinite => E_INDEFINITE,
            SnmpError::Version => E_VERSION,
            SnmpError::Community => E_COMMUNITY,
            SnmpError::Pdu => E_PDU,
            SnmpError::ReqId => E_REQID,
            SnmpError::ErrStatus => E_ERRSTATUS,
            SnmpError::ErrIndex => E_ERRINDEX,
            SnmpError::VarCount => E_VARCOUNT,
            SnmpError::Oid => E_OID,
            SnmpError::Value => E_VALUE,
            SnmpError::Disabled => E_DISABLED,
            SnmpError::Capacity => E_CAPACITY,
        };
        (n - 1) as usize
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            SnmpError::Empty => "empty snmp message",
            SnmpError::Short => "ber header truncated",
            SnmpError::Tag => "unexpected ber tag",
            SnmpError::Len => "ber length exceeds buffer",
            SnmpError::LenLong => "ber long-form length not minimal",
            SnmpError::Indefinite => "indefinite length not allowed",
            SnmpError::Version => "snmp version is not v1",
            SnmpError::Community => "community string mismatch",
            SnmpError::Pdu => "pdu tag is not get-response",
            SnmpError::ReqId => "request id mismatch",
            SnmpError::ErrStatus => "error-status is non-zero",
            SnmpError::ErrIndex => "error-index is non-zero",
            SnmpError::VarCount => "varbind count mismatch",
            SnmpError::Oid => "object identifier malformed",
            SnmpError::Value => "value type unsupported",
            SnmpError::Disabled => "snmp probe disabled: zero traffic",
            SnmpError::Capacity => "mib ledger full",
        }
    }
}

/// 打印机 MIB 值类型（只保留打印管理真正会读的四种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    OctetString,
    Integer,
    Counter,
    Null,
}

impl ValueKind {
    /// 文本标签。
    pub fn label(self) -> &'static str {
        match self {
            ValueKind::OctetString => "string",
            ValueKind::Integer => "integer",
            ValueKind::Counter => "counter",
            ValueKind::Null => "null",
        }
    }
}

/// 点分 OID。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Oid {
    pub arcs: Vec<u32>,
}

impl Oid {
    /// 构造 OID：至少两段，首段 ≤2，次段在首段 <2 时 <40。
    pub fn new(arcs: &[u32]) -> Option<Oid> {
        if arcs.len() < 2 || arcs.len() > MAX_ARCS {
            return None;
        }
        let a0 = arcs.first().copied()?;
        let a1 = arcs.get(1).copied()?;
        if a0 > 2 {
            return None;
        }
        if a0 < 2 && a1 >= 40 {
            return None;
        }
        let mut v: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < arcs.len() {
            if let Some(a) = arcs.get(i) {
                v.push(*a);
            }
            i += 1;
        }
        Some(Oid { arcs: v })
    }

    /// 点分文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.arcs.len() {
            if i > 0 {
                s.push('.');
            }
            if let Some(a) = self.arcs.get(i) {
                s.push_str(format!("{}", a).as_str());
            }
            i += 1;
        }
        s
    }

    /// 编码为 OID 内容字节（首两段合并为 40*x+y，其余 base-128）。
    pub fn encode_body(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let a0 = self.arcs.first().copied().unwrap_or(0);
        let a1 = self.arcs.get(1).copied().unwrap_or(0);
        push_base128(&mut out, a0.saturating_mul(40).saturating_add(a1));
        let mut i = 2usize;
        while i < self.arcs.len() {
            if let Some(a) = self.arcs.get(i) {
                push_base128(&mut out, *a);
            }
            i += 1;
        }
        out
    }

    /// 解析 OID 内容字节。
    pub fn parse_body(b: &[u8]) -> Result<Oid, SnmpError> {
        if b.is_empty() {
            return Err(SnmpError::Oid);
        }
        let mut arcs: Vec<u32> = Vec::new();
        let mut i = 0usize;
        let mut first = true;
        while i < b.len() {
            let mut val: u32 = 0;
            let mut used = 0usize;
            loop {
                let c = match b.get(i) {
                    Some(v) => *v,
                    None => return Err(SnmpError::Oid),
                };
                i += 1;
                used += 1;
                if used > 5 {
                    return Err(SnmpError::Oid);
                }
                // 移位前先挡住溢出：5 字节 base-128 已超过 u32 表达能力。
                if val > 0x01ff_ffff {
                    return Err(SnmpError::Oid);
                }
                val = (val << 7) | (c & 0x7f) as u32;
                if c & 0x80 == 0 {
                    break;
                }
            }
            if first {
                first = false;
                let x = if val < 40 {
                    0
                } else if val < 80 {
                    1
                } else {
                    2
                };
                let y = val - 40 * x;
                arcs.push(x);
                arcs.push(y);
            } else {
                arcs.push(val);
            }
            if arcs.len() > MAX_ARCS {
                return Err(SnmpError::Oid);
            }
        }
        if arcs.len() < 2 {
            return Err(SnmpError::Oid);
        }
        Ok(Oid { arcs })
    }
}

fn push_base128(v: &mut Vec<u8>, mut n: u32) {
    let mut buf: [u8; 5] = [0u8; 5];
    let mut i = 0usize;
    loop {
        buf[i] = (n & 0x7f) as u8;
        n >>= 7;
        i += 1;
        if n == 0 || i >= 5 {
            break;
        }
    }
    while i > 0 {
        i -= 1;
        if let Some(b) = buf.get(i) {
            let out = if i == 0 { *b } else { *b | 0x80 };
            v.push(out);
        }
    }
}

/// 一个已解析的 BER TLV 视图：`start` 指向内容起点，`len` 为内容长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tlv {
    pub tag: u8,
    pub start: usize,
    pub len: usize,
}

fn read_tlv(d: &[u8], pos: &mut usize) -> Result<Tlv, SnmpError> {
    let tag = match d.get(*pos) {
        Some(v) => *v,
        None => return Err(SnmpError::Short),
    };
    *pos += 1;
    let b = match d.get(*pos) {
        Some(v) => *v,
        None => return Err(SnmpError::Short),
    };
    *pos += 1;
    let len;
    if b < 0x80 {
        len = b as usize;
    } else if b == 0x80 {
        return Err(SnmpError::Indefinite);
    } else {
        let n = (b & 0x7f) as usize;
        if n > 4 {
            return Err(SnmpError::LenLong);
        }
        let mut v = 0usize;
        let mut i = 0usize;
        let mut first = 0u8;
        while i < n {
            let c = match d.get(*pos) {
                Some(x) => *x,
                None => return Err(SnmpError::Short),
            };
            if i == 0 {
                first = c;
            }
            *pos += 1;
            v = (v << 8) | c as usize;
            i += 1;
        }
        // 长度字段的最小化表示：首字节为 0 说明数值本可以用更短形式。
        if n > 1 && first == 0 {
            return Err(SnmpError::LenLong);
        }
        len = v;
    }
    let start = *pos;
    if start + len > d.len() {
        return Err(SnmpError::Len);
    }
    // 读头即越过内容：顺序读下一个 TLV 时指针自然落在其起点，
    // 调用方若要进内容用 `start/len` 切片，不必再手工推进。
    *pos = start + len;
    Ok(Tlv { tag, start, len })
}

fn slice_of<'a>(d: &'a [u8], t: &Tlv) -> Result<&'a [u8], SnmpError> {
    let end = t.start + t.len;
    match d.get(t.start..end) {
        Some(s) => Ok(s),
        None => Err(SnmpError::Len),
    }
}

fn ascii_bytes(b: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            if *c >= 0x20 && *c < 0x7f {
                s.push(*c as char);
            } else {
                s.push('?');
            }
        }
        i += 1;
    }
    s
}

fn be_int_slice(body: &[u8]) -> Result<i64, SnmpError> {
    if body.is_empty() || body.len() > 4 {
        return Err(SnmpError::Len);
    }
    let mut v: i64 = if body.first().copied().unwrap_or(0) & 0x80 != 0 {
        -1
    } else {
        0
    };
    let mut i = 0usize;
    while i < body.len() {
        if let Some(b) = body.get(i) {
            v = (v << 8) | (*b as i64);
        }
        i += 1;
    }
    Ok(v)
}

fn be_int(d: &[u8], t: &Tlv) -> Result<i64, SnmpError> {
    let body = slice_of(d, t)?;
    be_int_slice(body)
}

/// 一个 MIB 值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MibValue {
    pub oid: Oid,
    pub kind: ValueKind,
    pub text: String,
    pub number: i64,
}

/// 探针请求。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRequest {
    pub community: String,
    pub request_id: i32,
    pub oids: Vec<Oid>,
}

/// 打印管理关心的四个 OID：描述、名字、位置、总体状态。
pub fn probe_oids() -> Vec<Oid> {
    let mut v: Vec<Oid> = Vec::new();
    let specs: [[u32; 10]; 4] = [
        [1, 3, 6, 1, 2, 1, 1, 1, 0, 0],
        [1, 3, 6, 1, 2, 1, 1, 5, 0, 0],
        [1, 3, 6, 1, 2, 1, 1, 6, 0, 0],
        [1, 3, 6, 1, 2, 1, 43, 5, 1, 0],
    ];
    // 上面每个规格的最后一位是占位 0，按实际段数裁剪：前三个 9 段，状态 8 段。
    let lens: [usize; 4] = [9, 9, 9, 8];
    let mut i = 0usize;
    while i < specs.len() {
        if let (Some(spec), Some(n)) = (specs.get(i), lens.get(i)) {
            let mut arcs: Vec<u32> = Vec::new();
            let mut k = 0usize;
            while k < *n {
                if let Some(a) = spec.get(k) {
                    arcs.push(*a);
                }
                k += 1;
            }
            if let Some(o) = Oid::new(arcs.as_slice()) {
                v.push(o);
            }
        }
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// BER 编码（请求与模拟器响应共用）
// ---------------------------------------------------------------------------

fn enc_len(v: &mut Vec<u8>, n: usize) {
    if n < 0x80 {
        v.push(n as u8);
        return;
    }
    let mut bytes: [u8; 4] = [0u8; 4];
    let mut i = 0usize;
    let mut x = n;
    while x > 0 && i < 4 {
        bytes[i] = (x & 0xff) as u8;
        x >>= 8;
        i += 1;
    }
    v.push(0x80 | i as u8);
    while i > 0 {
        i -= 1;
        if let Some(b) = bytes.get(i) {
            v.push(*b);
        }
    }
}

fn wrap(tag: u8, body: Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.push(tag);
    enc_len(&mut out, body.len());
    out.extend_from_slice(body.as_slice());
    out
}

fn int_body(n: i32) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let be = (n as u32).to_be_bytes();
    let neg = n < 0;
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < 4 {
        if let Some(b) = be.get(i) {
            bytes.push(*b);
        }
        i += 1;
    }
    let mut start = 0usize;
    if !neg {
        while start < 3 {
            let cur = match bytes.get(start) {
                Some(v) => *v,
                None => break,
            };
            let nxt = match bytes.get(start + 1) {
                Some(v) => *v,
                None => break,
            };
            if cur == 0 && nxt < 0x80 {
                start += 1;
            } else {
                break;
            }
        }
    } else {
        while start < 3 {
            let cur = match bytes.get(start) {
                Some(v) => *v,
                None => break,
            };
            let nxt = match bytes.get(start + 1) {
                Some(v) => *v,
                None => break,
            };
            if cur == 0xff && nxt >= 0x80 {
                start += 1;
            } else {
                break;
            }
        }
    }
    while start < bytes.len() {
        if let Some(b) = bytes.get(start) {
            out.push(*b);
        }
        start += 1;
    }
    out
}

fn varbind_raw(oid_body: &[u8], tag: u8, body: &[u8]) -> Vec<u8> {
    let mut inner: Vec<u8> = Vec::new();
    inner.extend_from_slice(wrap(0x06, oid_body.to_vec()).as_slice());
    inner.extend_from_slice(wrap(tag, body.to_vec()).as_slice());
    wrap(0x30, inner)
}

fn varbind(oid: &Oid, tag: u8, body: &[u8]) -> Vec<u8> {
    varbind_raw(oid.encode_body().as_slice(), tag, body)
}

fn build_pdu(reqid: i32, err_status: i32, err_index: i32, vbs: &[Vec<u8>]) -> Vec<u8> {
    let mut inner: Vec<u8> = Vec::new();
    inner.extend_from_slice(wrap(0x02, int_body(reqid)).as_slice());
    inner.extend_from_slice(wrap(0x02, int_body(err_status)).as_slice());
    inner.extend_from_slice(wrap(0x02, int_body(err_index)).as_slice());
    let mut list_body: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < vbs.len() {
        if let Some(vb) = vbs.get(i) {
            list_body.extend_from_slice(vb.as_slice());
        }
        i += 1;
    }
    inner.extend_from_slice(wrap(0x30, list_body).as_slice());
    inner
}

fn build_message(version: i32, community: &str, pdu_tag: u8, pdu_body: Vec<u8>) -> Vec<u8> {
    let mut inner: Vec<u8> = Vec::new();
    inner.extend_from_slice(wrap(0x02, int_body(version)).as_slice());
    inner.extend_from_slice(wrap(0x04, community.as_bytes().to_vec()).as_slice());
    inner.extend_from_slice(wrap(pdu_tag, pdu_body).as_slice());
    wrap(0x30, inner)
}

/// 编码 GetRequest（PDU tag 0xA0，varbind 值为 NULL）。
pub fn encode_request(req: &ProbeRequest) -> Vec<u8> {
    let mut vbs: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i < req.oids.len() {
        if let Some(o) = req.oids.get(i) {
            vbs.push(varbind(o, 0x05, b""));
        }
        i += 1;
    }
    let pdu = build_pdu(req.request_id, 0, 0, vbs.as_slice());
    build_message(0, req.community.as_str(), 0xA0, pdu)
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

fn parse_header_common(d: &[u8]) -> Result<(Tlv, usize), SnmpError> {
    let mut pos = 0usize;
    let top = read_tlv(d, &mut pos)?;
    if top.tag != 0x30 {
        return Err(SnmpError::Tag);
    }
    Ok((top, top.start))
}

/// 解析 GetResponse，返回按 varbind 顺序的值表。
pub fn parse_response(
    d: &[u8],
    community: &str,
    reqid: i32,
    want: usize,
) -> Result<Vec<MibValue>, SnmpError> {
    if d.is_empty() {
        return Err(SnmpError::Empty);
    }
    let (top, top_start) = parse_header_common(d)?;
    let end = top.start + top.len;
    let mut p = top_start;
    let vt = read_tlv(d, &mut p)?;
    if vt.tag != 0x02 || vt.start + vt.len > end {
        return Err(SnmpError::Tag);
    }
    if be_int(d, &vt)? != 0 {
        return Err(SnmpError::Version);
    }
    let ct = read_tlv(d, &mut p)?;
    if ct.tag != 0x04 || ct.start + ct.len > end {
        return Err(SnmpError::Tag);
    }
    let comm = ascii_bytes(slice_of(d, &ct)?);
    if comm != community {
        return Err(SnmpError::Community);
    }
    let pdut = read_tlv(d, &mut p)?;
    if pdut.tag != 0xA2 {
        return Err(SnmpError::Pdu);
    }
    let pend = pdut.start + pdut.len;
    if pend > end {
        return Err(SnmpError::Len);
    }
    let mut q = pdut.start;
    let rt = read_tlv(d, &mut q)?;
    if rt.tag != 0x02 {
        return Err(SnmpError::Tag);
    }
    if be_int(d, &rt)? != reqid as i64 {
        return Err(SnmpError::ReqId);
    }
    let est = read_tlv(d, &mut q)?;
    if est.tag != 0x02 {
        return Err(SnmpError::Tag);
    }
    if be_int(d, &est)? != 0 {
        return Err(SnmpError::ErrStatus);
    }
    let eit = read_tlv(d, &mut q)?;
    if eit.tag != 0x02 {
        return Err(SnmpError::Tag);
    }
    if be_int(d, &eit)? != 0 {
        return Err(SnmpError::ErrIndex);
    }
    let lt = read_tlv(d, &mut q)?;
    if lt.tag != 0x30 || lt.start + lt.len > pend {
        return Err(SnmpError::Tag);
    }
    let lend = lt.start + lt.len;
    let mut r = lt.start;
    let mut out: Vec<MibValue> = Vec::new();
    while r < lend {
        let vbt = read_tlv(d, &mut r)?;
        if vbt.tag != 0x30 || vbt.start + vbt.len > lend {
            return Err(SnmpError::Tag);
        }
        let mut s = vbt.start;
        let obt = read_tlv(d, &mut s)?;
        if obt.tag != 0x06 {
            return Err(SnmpError::Tag);
        }
        let oid = Oid::parse_body(slice_of(d, &obt)?)?;
        let valt = read_tlv(d, &mut s)?;
        let body = slice_of(d, &valt)?;
        let (kind, text, number) = match valt.tag {
            0x04 => (ValueKind::OctetString, ascii_bytes(body), 0i64),
            0x02 | 0x41 => {
                let n = be_int_slice(body)?;
                let kind = if valt.tag == 0x02 {
                    ValueKind::Integer
                } else {
                    ValueKind::Counter
                };
                (kind, format!("{}", n), n)
            }
            0x05 => (ValueKind::Null, String::new(), 0i64),
            _ => return Err(SnmpError::Value),
        };
        out.push(MibValue {
            oid,
            kind,
            text,
            number,
        });
    }
    if out.len() != want {
        return Err(SnmpError::VarCount);
    }
    Ok(out)
}

/// 解析 GetRequest（用于编码往返自检）。
pub fn decode_request(d: &[u8]) -> Result<ProbeRequest, SnmpError> {
    if d.is_empty() {
        return Err(SnmpError::Empty);
    }
    let (top, top_start) = parse_header_common(d)?;
    let end = top.start + top.len;
    let mut p = top_start;
    let vt = read_tlv(d, &mut p)?;
    if vt.tag != 0x02 || vt.start + vt.len > end {
        return Err(SnmpError::Tag);
    }
    if be_int(d, &vt)? != 0 {
        return Err(SnmpError::Version);
    }
    let ct = read_tlv(d, &mut p)?;
    if ct.tag != 0x04 {
        return Err(SnmpError::Tag);
    }
    let community = ascii_bytes(slice_of(d, &ct)?);
    let pdut = read_tlv(d, &mut p)?;
    if pdut.tag != 0xA0 {
        return Err(SnmpError::Pdu);
    }
    let mut q = pdut.start;
    let rt = read_tlv(d, &mut q)?;
    if rt.tag != 0x02 {
        return Err(SnmpError::Tag);
    }
    let reqid = be_int(d, &rt)? as i32;
    let _es = read_tlv(d, &mut q)?;
    let _ei = read_tlv(d, &mut q)?;
    let lt = read_tlv(d, &mut q)?;
    if lt.tag != 0x30 {
        return Err(SnmpError::Tag);
    }
    let lend = lt.start + lt.len;
    let mut r = lt.start;
    let mut oids: Vec<Oid> = Vec::new();
    while r < lend {
        let vbt = read_tlv(d, &mut r)?;
        if vbt.tag != 0x30 {
            return Err(SnmpError::Tag);
        }
        let mut s = vbt.start;
        let obt = read_tlv(d, &mut s)?;
        if obt.tag != 0x06 {
            return Err(SnmpError::Tag);
        }
        oids.push(Oid::parse_body(slice_of(d, &obt)?)?);
        let valt = read_tlv(d, &mut s)?;
        if valt.tag != 0x05 {
            return Err(SnmpError::Value);
        }
    }
    Ok(ProbeRequest {
        community,
        request_id: reqid,
        oids,
    })
}

// ---------------------------------------------------------------------------
// 两个模拟器模型
// ---------------------------------------------------------------------------

/// 一台模拟打印机的 MIB 快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentModel {
    pub name: &'static str,
    pub community: &'static str,
    pub sys_descr: &'static str,
    pub sys_name: &'static str,
    pub sys_location: &'static str,
    pub status: i32,
}

/// 两份模型：community、描述、位置、状态都不同，用来验证探针不是背答案。
pub fn models() -> Vec<AgentModel> {
    let mut v: Vec<AgentModel> = Vec::new();
    v.push(AgentModel {
        name: "mdl-a",
        community: "public",
        sys_descr: "UNX LaserJet 5000",
        sys_name: "prn-a",
        sys_location: "lab-1",
        status: 3,
    });
    v.push(AgentModel {
        name: "mdl-b",
        community: "printer-ro",
        sys_descr: "UNX InkJet 220",
        sys_name: "prn-b",
        sys_location: "floor-2",
        status: 4,
    });
    v
}

fn model_varbinds(m: &AgentModel) -> Vec<Vec<u8>> {
    let oids = probe_oids();
    let mut vbs: Vec<Vec<u8>> = Vec::new();
    if let Some(o) = oids.get(0) {
        vbs.push(varbind(o, 0x04, m.sys_descr.as_bytes()));
    }
    if let Some(o) = oids.get(1) {
        vbs.push(varbind(o, 0x04, m.sys_name.as_bytes()));
    }
    if let Some(o) = oids.get(2) {
        vbs.push(varbind(o, 0x04, m.sys_location.as_bytes()));
    }
    if let Some(o) = oids.get(3) {
        vbs.push(varbind(o, 0x02, int_body(m.status).as_slice()));
    }
    vbs
}

/// 生成模型对 `reqid` 的 GetResponse 报文。
pub fn response_bytes(m: &AgentModel, reqid: i32) -> Vec<u8> {
    let vbs = model_varbinds(m);
    let pdu = build_pdu(reqid, 0, 0, vbs.as_slice());
    build_message(0, m.community, 0xA2, pdu)
}

// ---------------------------------------------------------------------------
// MIB 账本 + 探针状态机
// ---------------------------------------------------------------------------

/// MIB 账本条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MibEntry {
    pub oid: Oid,
    pub kind: ValueKind,
    pub text: String,
    pub number: i64,
}

/// 定容 MIB 账本：同一 OID 再写入只更新值（单源视图），不新增条目。
#[derive(Clone, Debug)]
pub struct MibLedger {
    entries: Vec<MibEntry>,
    cap: usize,
    pub accepted: usize,
    pub updates: usize,
    pub rejected: usize,
    pub err_counts: [u16; ERR_SLOTS],
    pub first_err: u32,
}

impl MibLedger {
    /// 建账本。
    pub fn new(cap: usize) -> MibLedger {
        MibLedger {
            entries: Vec::new(),
            cap,
            accepted: 0,
            updates: 0,
            rejected: 0,
            err_counts: [0u16; ERR_SLOTS],
            first_err: 0,
        }
    }

    /// 写入一个 MIB 值。
    pub fn insert(&mut self, v: MibValue) -> Result<usize, SnmpError> {
        if let Some(idx) = self.find(&v.oid) {
            if let Some(e) = self.entries.get_mut(idx) {
                *e = MibEntry {
                    oid: v.oid,
                    kind: v.kind,
                    text: v.text,
                    number: v.number,
                };
            }
            self.updates += 1;
            return Ok(idx);
        }
        if self.entries.len() >= self.cap {
            self.note_err(SnmpError::Capacity);
            return Err(SnmpError::Capacity);
        }
        self.entries.push(MibEntry {
            oid: v.oid,
            kind: v.kind,
            text: v.text,
            number: v.number,
        });
        self.accepted += 1;
        Ok(self.entries.len() - 1)
    }

    /// 按 OID 查下标。
    pub fn find(&self, oid: &Oid) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.oid == *oid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 条目访问。
    pub fn entry(&self, i: usize) -> Option<&MibEntry> {
        self.entries.get(i)
    }

    /// 记一次拒绝。
    pub fn note_err(&mut self, e: SnmpError) {
        self.rejected += 1;
        let slot = e.slot();
        if let Some(c) = self.err_counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if self.first_err == 0 {
            self.first_err = e.code();
        }
    }

    /// 某类错误计数。
    pub fn err_count(&self, e: SnmpError) -> u16 {
        match self.err_counts.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 探针相位五态。`Disabled` 是**初始态**，不是失败态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Disabled,
    Idle,
    Sent,
    Parsed,
    Failed,
}

impl Phase {
    /// 相位文本。
    pub fn label(self) -> &'static str {
        match self {
            Phase::Disabled => "disabled",
            Phase::Idle => "idle",
            Phase::Sent => "sent",
            Phase::Parsed => "parsed",
            Phase::Failed => "failed",
        }
    }
}

/// SNMP 探针：可选档，构造即决定开关。
#[derive(Clone, Debug)]
pub struct ProbeAgent {
    enabled: bool,
    phase: Phase,
    community: String,
    request_id: i32,
    pub bytes_out: usize,
    pub bytes_in: usize,
    pub polls: usize,
    pub ledger: MibLedger,
}

impl ProbeAgent {
    /// 构造探针；`enabled=false` 即默认关闭档。
    pub fn new(enabled: bool, community: &str) -> ProbeAgent {
        ProbeAgent {
            enabled,
            phase: if enabled {
                Phase::Idle
            } else {
                Phase::Disabled
            },
            community: community.to_string(),
            request_id: REQ_ID,
            bytes_out: 0,
            bytes_in: 0,
            polls: 0,
            ledger: MibLedger::new(8),
        }
    }

    /// 默认关闭档（J1「关闭档零流量」的直接对象）。
    pub fn default_off() -> ProbeAgent {
        ProbeAgent::new(false, "public")
    }

    /// 是否启用。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 当前相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 组请求：关闭档返回 None（零流量纪律的落点）。
    pub fn build_request(&self) -> Option<Vec<u8>> {
        if !self.enabled {
            return None;
        }
        let req = ProbeRequest {
            community: self.community.clone(),
            request_id: self.request_id,
            oids: probe_oids(),
        };
        Some(encode_request(&req))
    }

    /// 完整往返：关档不动任何计数器；开档发一次请求、解析一次响应。
    pub fn poll(&mut self, response: &[u8]) -> Result<usize, SnmpError> {
        if !self.enabled {
            self.phase = Phase::Disabled;
            return Err(SnmpError::Disabled);
        }
        let req = match self.build_request() {
            Some(r) => r,
            None => {
                self.phase = Phase::Disabled;
                return Err(SnmpError::Disabled);
            }
        };
        self.bytes_out += req.len();
        self.polls += 1;
        self.phase = Phase::Sent;
        let want = probe_oids().len();
        match parse_response(response, self.community.as_str(), self.request_id, want) {
            Ok(vals) => {
                self.bytes_in += response.len();
                let n = vals.len();
                let mut i = 0usize;
                while i < vals.len() {
                    if let Some(v) = vals.get(i) {
                        let _ = self.ledger.insert(v.clone());
                    }
                    i += 1;
                }
                self.phase = Phase::Parsed;
                Ok(n)
            }
            Err(e) => {
                self.phase = Phase::Failed;
                self.ledger.note_err(e);
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：17 类坏样本
// ---------------------------------------------------------------------------

fn bytes(b: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            v.push(*c);
        }
        i += 1;
    }
    v
}

/// 注入核返回（命中，总数，码位掩码）。
pub fn injection_core() -> (usize, usize, u32) {
    let total = 17usize;
    let mut caught = 0usize;
    let mut mask = 0u32;
    let m = match models().first().copied() {
        Some(v) => v,
        None => return (0, total, 0),
    };
    let good = response_bytes(&m, REQ_ID);
    let comm = m.community;
    let vbs = model_varbinds(&m);

    let mut cases: Vec<(Vec<u8>, SnmpError)> = Vec::new();
    cases.push((Vec::new(), SnmpError::Empty));
    cases.push((bytes(&[0x30]), SnmpError::Short));
    cases.push((wrap(0x31, bytes(&[0x02, 0x01, 0x00])), SnmpError::Tag));
    cases.push((bytes(&[0x30, 0x7f, 0x00, 0x00, 0x00]), SnmpError::Len));
    cases.push((
        bytes(&[0x30, 0x82, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00]),
        SnmpError::LenLong,
    ));
    cases.push((bytes(&[0x30, 0x80, 0x00]), SnmpError::Indefinite));
    cases.push((
        build_message(1, comm, 0xA2, build_pdu(REQ_ID, 0, 0, vbs.as_slice())),
        SnmpError::Version,
    ));
    cases.push((
        build_message(0, "bogus", 0xA2, build_pdu(REQ_ID, 0, 0, vbs.as_slice())),
        SnmpError::Community,
    ));
    cases.push((
        build_message(0, comm, 0xA1, build_pdu(REQ_ID, 0, 0, vbs.as_slice())),
        SnmpError::Pdu,
    ));
    cases.push((response_bytes(&m, REQ_ID + 1), SnmpError::ReqId));
    cases.push((
        build_message(0, comm, 0xA2, build_pdu(REQ_ID, 2, 0, vbs.as_slice())),
        SnmpError::ErrStatus,
    ));
    cases.push((
        build_message(0, comm, 0xA2, build_pdu(REQ_ID, 0, 1, vbs.as_slice())),
        SnmpError::ErrIndex,
    ));
    {
        // 少一个 varbind：期望 4 条却只给 3 条。
        let mut short_vbs: Vec<Vec<u8>> = Vec::new();
        let mut i = 0usize;
        while i < 3 {
            if let Some(vb) = vbs.get(i) {
                short_vbs.push(vb.clone());
            }
            i += 1;
        }
        cases.push((
            build_message(0, comm, 0xA2, build_pdu(REQ_ID, 0, 0, short_vbs.as_slice())),
            SnmpError::VarCount,
        ));
    }
    {
        // OID 内容以续延位结尾：解码必然缺尾，必须拒绝。
        let mut bad_vbs: Vec<Vec<u8>> = Vec::new();
        bad_vbs.push(varbind_raw(&[0x2b, 0x80], 0x04, b"x"));
        let mut i = 1usize;
        while i < 4 {
            if let Some(vb) = vbs.get(i) {
                bad_vbs.push(vb.clone());
            }
            i += 1;
        }
        cases.push((
            build_message(0, comm, 0xA2, build_pdu(REQ_ID, 0, 0, bad_vbs.as_slice())),
            SnmpError::Oid,
        ));
    }
    {
        // 值类型 0x06（OID 类型）不在支持集合内。
        let mut bad_vbs: Vec<Vec<u8>> = Vec::new();
        let oid_body = match probe_oids().first() {
            Some(o) => o.encode_body(),
            None => Vec::new(),
        };
        bad_vbs.push(varbind_raw(oid_body.as_slice(), 0x06, &[0x2b]));
        let mut i = 1usize;
        while i < 4 {
            if let Some(vb) = vbs.get(i) {
                bad_vbs.push(vb.clone());
            }
            i += 1;
        }
        cases.push((
            build_message(0, comm, 0xA2, build_pdu(REQ_ID, 0, 0, bad_vbs.as_slice())),
            SnmpError::Value,
        ));
    }

    let mut i = 0usize;
    while i < cases.len() {
        if let Some((b, want)) = cases.get(i) {
            if parse_response(b.as_slice(), comm, REQ_ID, 4).err() == Some(*want) {
                caught += 1;
                mask |= 1u32 << want.slot();
            }
        }
        i += 1;
    }

    // 关闭档
    let mut off = ProbeAgent::default_off();
    if off.poll(good.as_slice()).err() == Some(SnmpError::Disabled)
        && off.bytes_out == 0
        && off.bytes_in == 0
        && off.polls == 0
    {
        caught += 1;
        mask |= 1u32 << SnmpError::Disabled.slot();
    }
    // 账本容量
    let mut led = MibLedger::new(1);
    let mut first_ok = false;
    if let (Some(o0), Some(o1)) = (probe_oids().get(0), probe_oids().get(1)) {
        let v0 = MibValue {
            oid: o0.clone(),
            kind: ValueKind::OctetString,
            text: "a".to_string(),
            number: 0,
        };
        let v1 = MibValue {
            oid: o1.clone(),
            kind: ValueKind::OctetString,
            text: "b".to_string(),
            number: 0,
        };
        first_ok = led.insert(v0).is_ok() && led.insert(v1).err() == Some(SnmpError::Capacity);
    }
    if first_ok {
        caught += 1;
        mask |= 1u32 << SnmpError::Capacity.slot();
    }
    (caught, total, mask)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50145 自检：2 模型逐字段 + 关闭档零流量 + 17 类注入。
pub fn run_unx50145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50145");
    let all = models();

    // J1-1 两模型响应都可解析
    let mut parsed: Vec<Vec<MibValue>> = Vec::new();
    let mut ok_count = 0usize;
    let mut i = 0usize;
    while i < all.len() {
        if let Some(m) = all.get(i) {
            let resp = response_bytes(m, REQ_ID);
            match parse_response(resp.as_slice(), m.community, REQ_ID, 4) {
                Ok(v) => {
                    ok_count += 1;
                    parsed.push(v);
                }
                Err(_) => {}
            }
        }
        i += 1;
    }
    let ok = ok_count == 2 && parsed.len() == 2;
    let det = format!("parsed={}/2", ok_count);
    s.add_dyn("50145-J1-双模型可解", ok, &det);

    // J1-2 逐字段一致：2 模型 × 5 字段 = 10 项
    let mut matched = 0usize;
    let mut compared = 0usize;
    let mut k = 0usize;
    while k < all.len() {
        if let (Some(m), Some(v)) = (all.get(k), parsed.get(k)) {
            let f0 = v
                .get(0)
                .map(|x| x.text == m.sys_descr && x.kind == ValueKind::OctetString)
                .unwrap_or(false);
            let f1 = v.get(1).map(|x| x.text == m.sys_name).unwrap_or(false);
            let f2 = v.get(2).map(|x| x.text == m.sys_location).unwrap_or(false);
            let f3 = v
                .get(3)
                .map(|x| x.number == m.status as i64 && x.kind == ValueKind::Integer)
                .unwrap_or(false);
            let f4 = v
                .get(0)
                .map(|x| x.oid.text() == "1.3.6.1.2.1.1.1.0")
                .unwrap_or(false);
            let fields: [bool; 5] = [f0, f1, f2, f3, f4];
            let mut f = 0usize;
            while f < fields.len() {
                if let Some(p) = fields.get(f) {
                    if *p {
                        matched += 1;
                    }
                    compared += 1;
                }
                f += 1;
            }
        }
        k += 1;
    }
    let ok = compared == 10 && matched == 10;
    let det = format!("matched={}/{}", matched, compared);
    s.add_dyn("50145-J1-逐字段一致", ok, &det);

    // J1-3 关闭档零流量：任何计数器非零都算红
    let mut off = ProbeAgent::default_off();
    let sample = match all.first() {
        Some(m) => response_bytes(m, REQ_ID),
        None => Vec::new(),
    };
    let err = off.poll(sample.as_slice());
    let ok = err.err() == Some(SnmpError::Disabled)
        && off.bytes_out == 0
        && off.bytes_in == 0
        && off.polls == 0
        && off.phase() == Phase::Disabled
        && off.ledger.len() == 0
        && off.build_request().is_none()
        && !off.enabled();
    let det = format!(
        "out={} in={} polls={} phase={}",
        off.bytes_out,
        off.bytes_in,
        off.polls,
        off.phase().label()
    );
    s.add_dyn("50145-J1-关档零流量", ok, &det);

    // J1-4 开档计数守恒：发多少记多少
    let mut on = match all.first() {
        Some(m) => ProbeAgent::new(true, m.community),
        None => ProbeAgent::new(true, "public"),
    };
    let expect_req = match on.build_request() {
        Some(r) => r.len(),
        None => 0,
    };
    let resp_len = sample.len();
    let polled = on.poll(sample.as_slice());
    let ok = match polled {
        Ok(n) => {
            n == 4
                && on.bytes_out == expect_req
                && expect_req > 0
                && on.bytes_in == resp_len
                && on.polls == 1
                && on.phase() == Phase::Parsed
                && on.ledger.len() == 4
        }
        Err(_) => false,
    };
    let det = format!(
        "out={}/{} in={}/{} polls={}",
        on.bytes_out, expect_req, on.bytes_in, resp_len, on.polls
    );
    s.add_dyn("50145-J1-开档计数", ok, &det);

    // J1-5 请求编解码往返：community / reqid / OID 列表逐项一致
    let req = ProbeRequest {
        community: "public".to_string(),
        request_id: REQ_ID,
        oids: probe_oids(),
    };
    let back = decode_request(encode_request(&req).as_slice());
    let ok = match back {
        Ok(r) => r.community == req.community && r.request_id == req.request_id && r.oids == req.oids,
        Err(_) => false,
    };
    s.add("50145-J1-请求往返", ok, if ok { "" } else { "请求编解码不互逆" });

    // J1-6 varbind 顺序保持：账本 OID 序 == 请求 OID 序
    let mut order_ok = on.ledger.len() == 4;
    let oids = probe_oids();
    let mut oi = 0usize;
    while oi < oids.len() {
        if let (Some(want), Some(e)) = (oids.get(oi), on.ledger.entry(oi)) {
            if e.oid != *want {
                order_ok = false;
            }
        } else {
            order_ok = false;
        }
        oi += 1;
    }
    s.add("50145-J1-顺序保持", order_ok, if order_ok { "" } else { "varbind 顺序被打乱" });

    // J1-7 同 OID 更新不新增条目（单源视图）
    let mut led = MibLedger::new(4);
    let mut upd_ok = false;
    if let Some(v) = parsed.first().and_then(|v| v.get(0)) {
        if led.insert(v.clone()).is_ok() && led.insert(v.clone()).is_ok() {
            upd_ok = led.len() == 1 && led.updates == 1 && led.accepted == 1;
        }
    }
    let det = format!("len={} upd={}", led.len(), led.updates);
    s.add_dyn("50145-J1-单源更新", upd_ok, &det);

    // J1-8 community 不匹配
    let ok = match all.first() {
        Some(m) => {
            let resp = response_bytes(m, REQ_ID);
            parse_response(resp.as_slice(), "wrong", REQ_ID, 4).err() == Some(SnmpError::Community)
        }
        None => false,
    };
    s.add("50145-J1-community", ok, if ok { "" } else { "community 未校验" });

    // J1-9 reqid 不匹配
    let ok = match all.first() {
        Some(m) => {
            let resp = response_bytes(m, REQ_ID + 7);
            parse_response(resp.as_slice(), m.community, REQ_ID, 4).err() == Some(SnmpError::ReqId)
        }
        None => false,
    };
    s.add("50145-J1-reqid", ok, if ok { "" } else { "reqid 未校验" });

    // J1-10 error-status / error-index 非零必拒
    let mut err_ok = false;
    if let Some(m) = all.first() {
        let vbs = model_varbinds(m);
        let es = build_message(0, m.community, 0xA2, build_pdu(REQ_ID, 2, 0, vbs.as_slice()));
        let ei = build_message(0, m.community, 0xA2, build_pdu(REQ_ID, 0, 1, vbs.as_slice()));
        err_ok = parse_response(es.as_slice(), m.community, REQ_ID, 4).err()
            == Some(SnmpError::ErrStatus)
            && parse_response(ei.as_slice(), m.community, REQ_ID, 4).err()
                == Some(SnmpError::ErrIndex);
    }
    s.add("50145-J1-错误状态", err_ok, if err_ok { "" } else { "错误状态被忽略" });

    // J1-11 长度族：不定长 / 非最小长形式 / 越界
    let ok = parse_response(bytes(&[0x30, 0x80, 0x00]).as_slice(), "public", REQ_ID, 4).err()
        == Some(SnmpError::Indefinite)
        && parse_response(
            bytes(&[0x30, 0x82, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00]).as_slice(),
            "public",
            REQ_ID,
            4,
        )
        .err()
            == Some(SnmpError::LenLong)
        && parse_response(bytes(&[0x30, 0x7f, 0, 0, 0]).as_slice(), "public", REQ_ID, 4).err()
            == Some(SnmpError::Len);
    s.add("50145-J1-长度族", ok, if ok { "" } else { "长度解析过宽" });

    // J1-12 OID 编解码往返（大子标识走多字节 base-128）
    let mut oid_ok = false;
    if let Some(o) = Oid::new(&[1, 3, 6, 1, 4, 1, 12345, 678]) {
        let body = o.encode_body();
        let back = Oid::parse_body(body.as_slice());
        oid_ok = back.ok() == Some(o.clone()) && o.text() == "1.3.6.1.4.1.12345.678";
    }
    s.add("50145-J1-OID往返", oid_ok, if oid_ok { "" } else { "OID 编解码不互逆" });

    // J1-13 值类型白名单：不支持的类型必拒
    let mut val_ok = false;
    if let Some(first) = probe_oids().first() {
        let vb = varbind_raw(first.encode_body().as_slice(), 0x06, &[0x2b]);
        let mut vbs: Vec<Vec<u8>> = Vec::new();
        vbs.push(vb);
        let msg = build_message(0, "public", 0xA2, build_pdu(REQ_ID, 0, 0, vbs.as_slice()));
        val_ok = parse_response(msg.as_slice(), "public", REQ_ID, 1).err() == Some(SnmpError::Value);
    }
    s.add("50145-J1-值类型", val_ok, if val_ok { "" } else { "未知值类型被接受" });

    // J1-14 账本容量显式拒绝
    let mut small = MibLedger::new(1);
    let mut cap_ok = false;
    if let (Some(o0), Some(o1)) = (probe_oids().get(0), probe_oids().get(1)) {
        let v0 = MibValue {
            oid: o0.clone(),
            kind: ValueKind::OctetString,
            text: "a".to_string(),
            number: 0,
        };
        let v1 = MibValue {
            oid: o1.clone(),
            kind: ValueKind::OctetString,
            text: "b".to_string(),
            number: 0,
        };
        cap_ok = small.insert(v0).is_ok()
            && small.insert(v1).err() == Some(SnmpError::Capacity)
            && small.len() == 1
            && small.accepted == 1;
    }
    s.add("50145-J1-账本容量", cap_ok, if cap_ok { "" } else { "容量未拦截" });

    // J1-15 坏包记账：5 类逐类计数，首错留痕
    let mut bad = MibLedger::new(4);
    let mut bads: Vec<(Vec<u8>, SnmpError)> = Vec::new();
    bads.push((Vec::new(), SnmpError::Empty));
    bads.push((bytes(&[0x30]), SnmpError::Short));
    bads.push((bytes(&[0x30, 0x80, 0x00]), SnmpError::Indefinite));
    bads.push((bytes(&[0x30, 0x7f, 0, 0, 0]), SnmpError::Len));
    bads.push((bytes(&[0x30, 0x82, 0, 5, 0, 0, 0, 0, 0]), SnmpError::LenLong));
    let mut b = 0usize;
    while b < bads.len() {
        if let Some((msg, want)) = bads.get(b) {
            match parse_response(msg.as_slice(), "public", REQ_ID, 4) {
                Err(e) if e == *want => bad.note_err(e),
                _ => {}
            }
        }
        b += 1;
    }
    let mut per_class = true;
    let mut b2 = 0usize;
    while b2 < bads.len() {
        if let Some((_, e)) = bads.get(b2) {
            if bad.err_count(*e) != 1 {
                per_class = false;
            }
        }
        b2 += 1;
    }
    let ok = bad.rejected == 5 && per_class && bad.first_err == SnmpError::Empty.code();
    let det = format!("rej={} first={:x}", bad.rejected, bad.first_err);
    s.add_dyn("50145-J1-坏包记账", ok, &det);

    // J1-16 注入核：17/17 且码位全覆盖
    let (caught, total, mask) = injection_core();
    let full = (1u32 << ERR_SLOTS) - 1;
    let ok = caught == total && mask == full;
    let det = format!("注入 {}/{} mask={:x}", caught, total, mask);
    s.add_dyn("50145-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50145_checks_green() {
        let set = run_unx50145_checks();
        assert!(set.all_passed(), "UNX-F50145 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50145_disabled_emits_nothing() {
        let mut a = ProbeAgent::default_off();
        let resp = match models().first().copied() {
            Some(m) => response_bytes(&m, REQ_ID),
            None => Vec::new(),
        };
        assert_eq!(a.poll(resp.as_slice()).err(), Some(SnmpError::Disabled));
        assert_eq!(a.bytes_out, 0);
        assert_eq!(a.bytes_in, 0);
        assert_eq!(a.polls, 0);
        assert!(a.build_request().is_none());
    }

    #[test]
    fn unx50145_models_match_snapshot() {
        let all = models();
        let mut i = 0usize;
        while i < all.len() {
            let m = match all.get(i) {
                Some(v) => *v,
                None => return,
            };
            let resp = response_bytes(&m, REQ_ID);
            let vals = match parse_response(resp.as_slice(), m.community, REQ_ID, 4) {
                Ok(v) => v,
                Err(e) => {
                    assert!(false, "model {} parse: {:?}", m.name, e);
                    return;
                }
            };
            assert_eq!(vals.len(), 4);
            assert_eq!(vals.get(0).map(|v| v.text.as_str()), Some(m.sys_descr));
            assert_eq!(vals.get(1).map(|v| v.text.as_str()), Some(m.sys_name));
            assert_eq!(vals.get(2).map(|v| v.text.as_str()), Some(m.sys_location));
            assert_eq!(vals.get(3).map(|v| v.number), Some(m.status as i64));
            i += 1;
        }
    }

    #[test]
    fn unx50145_injection_covers_all_codes() {
        let (caught, total, mask) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(mask, (1u32 << ERR_SLOTS) - 1);
    }
}
