//! UNX-F50142 mDNS 记录解析（TXT 键值族逐键落账）（0x43de0 段）
//!
//! 锚点：UNX-F50142-J1 TXT 解析 ×20 服务逐键一致，坏包记账；判据锚定 §AI-63 §2 专题五。
//!
//! 服务发现的第一道关不是「收到包」，而是「把包读成一张不会骗人的表」。
//! DNS 报文允许名字压缩（指针回引），TXT 记录允许单个字段里塞任意字节，
//! 两者叠加足以让一个「宽容」解析器把 A 服务的属性挂到 B 服务头上。因此
//! 本模块的纪律是：
//!
//! * 名字压缩只接受**严格回退**的指针（target < 指针自身偏移）。自指与前向
//!   指针会让解析进入环，`NameLoop` 是终止性的唯一硬保证；跳数上限作为纵深
//!   防御保留，不承担主要语义。
//! * 每个 TXT 字段必须含 `=`，键非空、长度 ≤64、且为可打印 ASCII；同一键
//!   第二次出现**只记账不覆盖**（首值优先），避免上游结果依赖包内顺序。
//! * 空字段（长度前缀 0）是合法噪声：计数但不出键。静默丢弃与静默接受同样
//!   有害——账本必须能回答「这个包到底有几条有效属性、几条是废料」。
//!
//! 数据结构（四类里占三类）：
//! * `TxtPair` / `TxtRecord` —— 单条 TXT 的解析视图（键值 + 重复键/空字段计数）；
//! * `ParsedResponse` —— 报文级视图（问题数/回答数/跳过数/消费字节数）；
//! * `TxtLedger` —— 定容逐服务账本（按名字反查、错误码逐类计数、首错留痕）。
//!
//! 错误路径：段内闭集 15 码（0x43de1..0x43def），全部在注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43de0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43de0;

/// 空输入：零字节即零信息，直接拒绝而不是当成「空报文」。
pub const E_EMPTY: u16 = 1;
/// 头部不足 12 字节。
pub const E_SHORT: u16 = 2;
/// 问题段计数超上限或问题体截断。
pub const E_QDCOUNT: u16 = 3;
/// 回答计数为 0/超上限，或回答的 class 不是 IN。
pub const E_ANCOUNT: u16 = 4;
/// 名字总长超 255（编码长度按 len+1 累计）。
pub const E_NAME_LEN: u16 = 5;
/// 压缩指针不严格回退（自指/前向/跳数超限）。
pub const E_NAME_LOOP: u16 = 6;
/// 标签字节非法（保留位 0x40/0x80、非可打印、点号）。
pub const E_NAME_CHAR: u16 = 7;
/// 问题类型不是 PTR(12)：本解析器只服务服务发现查询。
pub const E_QTYPE: u16 = 8;
/// 回答 class 非 IN。
pub const E_CLASS: u16 = 9;
/// rdlength 越过报文残余。
pub const E_RDLEN: u16 = 10;
/// TXT 字段长度前缀越过 rdata 残余。
pub const E_TXT_LEN: u16 = 11;
/// TXT 字段不含 `=`。
pub const E_TXT_FIELD: u16 = 12;
/// 键为空或含非法字节（含空格/`=`/非可打印）。
pub const E_TXT_KEY: u16 = 13;
/// 整个回答集合里没有 TXT。
pub const E_NO_TXT: u16 = 14;
/// 账本容量满：新条目必须显式失败，不能顶掉旧服务。
pub const E_CAPACITY: u16 = 15;

/// 闭集大小：错误码 1..=15 连续，账本按 `code - SEG_BASE - 1` 落槽。
pub const ERR_SLOTS: usize = 15;

/// 单条 TXT 的属性上限。超过即视为恶意/失控包：32 条足够覆盖真实打印机
/// 服务（IPP 侧常见 8~15 条），留 2 倍余量。
pub const MAX_PAIRS: usize = 32;

/// 单报文回答数上限。服务发现响应通常 1~4 条，16 是保守上限。
pub const MAX_ANSWERS: u16 = 16;

/// 问题段上限。mDNS 查询可能批量问，但超过 8 条已不属于打印机发现范畴。
pub const MAX_QUESTIONS: u16 = 8;

/// 名字解析跳数上限（纵深防御；正常压缩链远小于此值）。
const MAX_HOPS: u8 = 16;

/// mDNS TXT 解析错误。`code()` 返回 `SEG_BASE + n`，落在 0x43de0 段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdnsError {
    Empty,
    Short,
    QdCount,
    AnCount,
    NameLen,
    NameLoop,
    NameChar,
    Qtype,
    Class,
    RdLen,
    TxtLen,
    TxtField,
    TxtKey,
    NoTxt,
    Capacity,
}

impl MdnsError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32 + 1
    }

    /// 账本槽位（0 基）。
    pub fn slot(self) -> usize {
        let n = match self {
            MdnsError::Empty => E_EMPTY,
            MdnsError::Short => E_SHORT,
            MdnsError::QdCount => E_QDCOUNT,
            MdnsError::AnCount => E_ANCOUNT,
            MdnsError::NameLen => E_NAME_LEN,
            MdnsError::NameLoop => E_NAME_LOOP,
            MdnsError::NameChar => E_NAME_CHAR,
            MdnsError::Qtype => E_QTYPE,
            MdnsError::Class => E_CLASS,
            MdnsError::RdLen => E_RDLEN,
            MdnsError::TxtLen => E_TXT_LEN,
            MdnsError::TxtField => E_TXT_FIELD,
            MdnsError::TxtKey => E_TXT_KEY,
            MdnsError::NoTxt => E_NO_TXT,
            MdnsError::Capacity => E_CAPACITY,
        };
        (n - 1) as usize
    }

    /// 一行诊断文本（日志/判据明细共用）。
    pub fn message(self) -> &'static str {
        match self {
            MdnsError::Empty => "empty mdns packet",
            MdnsError::Short => "packet shorter than dns header",
            MdnsError::QdCount => "question count invalid",
            MdnsError::AnCount => "answer count invalid or class not IN",
            MdnsError::NameLen => "name exceeds 255 octets",
            MdnsError::NameLoop => "compression pointer not strictly backward",
            MdnsError::NameChar => "illegal label byte",
            MdnsError::Qtype => "question type is not PTR",
            MdnsError::Class => "answer class is not IN",
            MdnsError::RdLen => "rdlength exceeds packet tail",
            MdnsError::TxtLen => "txt field length exceeds rdata tail",
            MdnsError::TxtField => "txt field has no '='",
            MdnsError::TxtKey => "txt key empty or illegal",
            MdnsError::NoTxt => "no txt answer in response",
            MdnsError::Capacity => "ledger capacity exhausted",
        }
    }
}

/// 单条 TXT 键值。键已校验为可打印 ASCII，值保留原字节（仅非可打印替换为 ?）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxtPair {
    pub key: String,
    pub value: String,
}

impl TxtPair {
    /// 构造一条键值（供模型/测试使用，不做校验）。
    pub fn new(k: &str, v: &str) -> TxtPair {
        TxtPair {
            key: k.to_string(),
            value: v.to_string(),
        }
    }

    /// 键值拼接的字段长度（`key=value` 字节数）。
    pub fn field_len(&self) -> usize {
        self.key.len() + 1 + self.value.len()
    }
}

/// 单条 TXT 记录的解析视图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxtRecord {
    /// 回答的 owner name（点分文本，已去尾点）。
    pub service: String,
    /// 有效键值，保持包内首次出现顺序。
    pub pairs: Vec<TxtPair>,
    /// 重复键出现次数（首值优先，后续丢弃但记账）。
    pub dup_keys: u16,
    /// 长度前缀为 0 的字段个数。
    pub empty_fields: u16,
    /// TXT rdata 的声明长度。
    pub rdata_len: u16,
}

impl TxtRecord {
    /// 建空记录（解析器内部构造用）。
    pub fn new(service: String) -> TxtRecord {
        TxtRecord {
            service,
            pairs: Vec::new(),
            dup_keys: 0,
            empty_fields: 0,
            rdata_len: 0,
        }
    }

    /// 按键取值；首值优先语义在此体现。
    pub fn get(&self, key: &str) -> Option<&TxtPair> {
        let mut i = 0usize;
        while i < self.pairs.len() {
            if let Some(p) = self.pairs.get(i) {
                if p.key == key {
                    return Some(p);
                }
            }
            i += 1;
        }
        None
    }

    /// 键值表的 FNV-1a 32 位摘要：顺序敏感，用于「两次解析逐位一致」。
    pub fn digest(&self) -> u32 {
        let mut h = 0x811c_9dc5u32;
        let n = self.service.as_bytes();
        h = fnv_step(h, n);
        let mut i = 0usize;
        while i < self.pairs.len() {
            if let Some(p) = self.pairs.get(i) {
                h = fnv_step(h, p.key.as_bytes());
                h = fnv_step(h, b"=");
                h = fnv_step(h, p.value.as_bytes());
                h = fnv_step(h, b"\n");
            }
            i += 1;
        }
        h
    }

    /// 有效属性条数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
}

fn fnv_step(mut h: u32, bytes: &[u8]) -> u32 {
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 报文级解析视图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedResponse {
    pub record: TxtRecord,
    pub questions: u16,
    pub answers: u16,
    /// 被跳过的回答数（非 TXT，或第二条及以后的 TXT）。
    pub skipped: u16,
    /// 实际消费到的字节偏移（正常包应等于报文长度）。
    pub consumed: usize,
}

fn be_u16(data: &[u8], at: usize) -> Result<u16, MdnsError> {
    let a = match data.get(at) {
        Some(v) => *v,
        None => return Err(MdnsError::Short),
    };
    let b = match data.get(at + 1) {
        Some(v) => *v,
        None => return Err(MdnsError::Short),
    };
    Ok(((a as u16) << 8) | b as u16)
}

/// 读取一个可能含压缩指针的名字，返回（点分文本，名字之后的偏移）。
///
/// 关键契约：指针目标必须 `< 指针自身偏移`。这条规则同时保证了解析必然
/// 终止（每次跳转都严格向报文头部推进）与「名字只可能引用更早出现的数据」，
/// 后者是防伪造的语义基础。
pub fn read_name(data: &[u8], at: usize) -> Result<(String, usize), MdnsError> {
    let mut out = String::new();
    let mut pos = at;
    let mut after = 0usize;
    let mut jumped = false;
    let mut hops = 0u8;
    let mut total = 0usize;
    loop {
        let b = match data.get(pos) {
            Some(v) => *v,
            None => return Err(MdnsError::Short),
        };
        if b == 0 {
            pos += 1;
            break;
        }
        if b & 0xC0 == 0xC0 {
            let b2 = match data.get(pos + 1) {
                Some(v) => *v,
                None => return Err(MdnsError::Short),
            };
            let target = (((b & 0x3F) as usize) << 8) | b2 as usize;
            if target >= pos {
                return Err(MdnsError::NameLoop);
            }
            if !jumped {
                after = pos + 2;
                jumped = true;
            }
            hops += 1;
            if hops > MAX_HOPS {
                return Err(MdnsError::NameLoop);
            }
            pos = target;
            continue;
        }
        if b & 0xC0 != 0 {
            // 0x40/0x80 是 DNS 保留标签类型，出现即说明编码损坏。
            return Err(MdnsError::NameChar);
        }
        let len = b as usize;
        if len > 63 {
            return Err(MdnsError::NameChar);
        }
        total += len + 1;
        if total > 255 {
            return Err(MdnsError::NameLen);
        }
        if pos + 1 + len > data.len() {
            return Err(MdnsError::Short);
        }
        let mut i = 0usize;
        while i < len {
            let c = match data.get(pos + 1 + i) {
                Some(v) => *v,
                None => return Err(MdnsError::Short),
            };
            // 点号是文本分隔符，标签里出现会让「点分文本」产生歧义，直接拒绝。
            if c < 0x21 || c > 0x7e || c == b'.' {
                return Err(MdnsError::NameChar);
            }
            out.push(c as char);
            i += 1;
        }
        out.push('.');
        pos += 1 + len;
    }
    if out.is_empty() {
        return Err(MdnsError::NameLen);
    }
    while out.ends_with('.') {
        out.pop();
    }
    let consumed = if jumped { after } else { pos };
    Ok((out, consumed))
}

/// 解析 TXT rdata：一串「长度前缀 + 字段」。字段必须形如 `key=value`。
fn parse_txt(rec: &mut TxtRecord, rd: &[u8]) -> Result<(), MdnsError> {
    let mut i = 0usize;
    while i < rd.len() {
        let n = match rd.get(i) {
            Some(v) => *v as usize,
            None => break,
        };
        i += 1;
        if i + n > rd.len() {
            return Err(MdnsError::TxtLen);
        }
        if n == 0 {
            // 合法噪声：计数，不出键。
            rec.empty_fields = rec.empty_fields.saturating_add(1);
            continue;
        }
        let field = match rd.get(i..i + n) {
            Some(f) => f,
            None => return Err(MdnsError::TxtLen),
        };
        i += n;
        let mut eq: Option<usize> = None;
        let mut k = 0usize;
        while k < field.len() {
            let c = match field.get(k) {
                Some(v) => *v,
                None => break,
            };
            if c == b'=' {
                eq = Some(k);
                break;
            }
            k += 1;
        }
        let eq = match eq {
            Some(v) => v,
            None => return Err(MdnsError::TxtField),
        };
        if eq == 0 || eq > 64 {
            return Err(MdnsError::TxtKey);
        }
        let mut key = String::new();
        let mut j = 0usize;
        while j < eq {
            let c = match field.get(j) {
                Some(v) => *v,
                None => return Err(MdnsError::TxtKey),
            };
            // 键只允许非空格的可打印 ASCII：空格会在 `key=value` 文本化时
            // 造成歧义，非 ASCII 则让不同编码的同名键互相冒名。
            if c < 0x21 || c > 0x7e {
                return Err(MdnsError::TxtKey);
            }
            key.push(c as char);
            j += 1;
        }
        let mut value = String::new();
        let mut m = eq + 1;
        while m < field.len() {
            let c = match field.get(m) {
                Some(v) => *v,
                None => break,
            };
            if c < 0x20 || c == 0x7f {
                value.push('?');
            } else {
                value.push(c as char);
            }
            m += 1;
        }
        if rec.get(key.as_str()).is_some() {
            rec.dup_keys = rec.dup_keys.saturating_add(1);
            continue;
        }
        rec.pairs.push(TxtPair { key, value });
        if rec.pairs.len() > MAX_PAIRS {
            return Err(MdnsError::Capacity);
        }
    }
    Ok(())
}

/// 解析一条 mDNS 响应，取第一条 TXT 回答。
///
/// 逐段校验：问题段只接受 PTR；回答段逐个计算 rdlength 边界；第一条 TXT
/// 之后的其他回答（含第二条 TXT）只计数跳过，不参与建表。
pub fn parse_response(data: &[u8]) -> Result<ParsedResponse, MdnsError> {
    if data.is_empty() {
        return Err(MdnsError::Empty);
    }
    if data.len() < 12 {
        return Err(MdnsError::Short);
    }
    let qd = be_u16(data, 4)?;
    let an = be_u16(data, 6)?;
    if qd > MAX_QUESTIONS {
        return Err(MdnsError::QdCount);
    }
    if an == 0 || an > MAX_ANSWERS {
        return Err(MdnsError::AnCount);
    }
    let mut pos = 12usize;
    let mut q = 0u16;
    while q < qd {
        let (_qn, next) = read_name(data, pos)?;
        pos = next;
        if pos + 4 > data.len() {
            return Err(MdnsError::QdCount);
        }
        let qtype = be_u16(data, pos)?;
        if qtype != 12 {
            return Err(MdnsError::Qtype);
        }
        pos += 4;
        q += 1;
    }
    let mut answers = 0u16;
    let mut skipped = 0u16;
    let mut record: Option<TxtRecord> = None;
    while answers < an {
        let (owner, next) = read_name(data, pos)?;
        pos = next;
        if pos + 10 > data.len() {
            return Err(MdnsError::Short);
        }
        let rtype = be_u16(data, pos)?;
        let rclass = be_u16(data, pos + 2)?;
        let _ttl = {
            // ttl 只做存在性校验：mDNS 的 120s 约定不影响属性内容。
            let hi = be_u16(data, pos + 4)? as u32;
            let lo = be_u16(data, pos + 6)? as u32;
            (hi << 16) | lo
        };
        let rdlen = be_u16(data, pos + 8)? as usize;
        pos += 10;
        if rclass & 0x7fff != 1 {
            return Err(MdnsError::Class);
        }
        if pos + rdlen > data.len() {
            return Err(MdnsError::RdLen);
        }
        if rtype == 16 && record.is_none() {
            let mut rec = TxtRecord::new(owner);
            rec.rdata_len = rdlen as u16;
            let rd = match data.get(pos..pos + rdlen) {
                Some(s) => s,
                None => return Err(MdnsError::RdLen),
            };
            parse_txt(&mut rec, rd)?;
            record = Some(rec);
        } else {
            skipped = skipped.saturating_add(1);
        }
        pos += rdlen;
        answers += 1;
    }
    let rec = match record {
        Some(r) => r,
        None => return Err(MdnsError::NoTxt),
    };
    Ok(ParsedResponse {
        record: rec,
        questions: qd,
        answers: an,
        skipped,
        consumed: pos,
    })
}

// ---------------------------------------------------------------------------
// 20 服务模型与线格式编码（模型即「模拟器」，与解析结果逐键对照）
// ---------------------------------------------------------------------------

/// 第 idx 个服务的期望内容（服务名 + 键值表）。idx ≥ 20 返回 None。
///
/// 模型刻意包含三类边角：值里带 `=`（`path`）、值里带路径分隔符（`rp`）、
/// 以及编码时附加的空字段与重复键——后两者不进模型，只能被计数捕获。
pub fn service_model(idx: usize) -> Option<(String, Vec<TxtPair>)> {
    if idx >= 20 {
        return None;
    }
    let name = format!("_pdl{}-{}._{}.local", idx, idx * 7 + 1, "tcp");
    let mut pairs: Vec<TxtPair> = Vec::new();
    pairs.push(TxtPair::new("typ", &format!("printer-{}", idx)));
    pairs.push(TxtPair::new("note", &format!("unit/{}", idx * 3 + 1)));
    pairs.push(TxtPair::new("path", &format!("a=b{};c", idx)));
    pairs.push(TxtPair::new("uid", &format!("{:04x}-{:02x}", idx * 0x11, idx)));
    pairs.push(TxtPair::new("rp", &format!("ipp://p{}.local/print", idx)));
    Some((name, pairs))
}

fn put_u16(v: &mut Vec<u8>, val: u16) {
    v.push((val >> 8) as u8);
    v.push(val as u8);
}

fn put_u32(v: &mut Vec<u8>, val: u32) {
    v.push((val >> 24) as u8);
    v.push((val >> 16) as u8);
    v.push((val >> 8) as u8);
    v.push(val as u8);
}

/// 把一个点分名字压成标签序列（点号不进入标签）。
fn push_name(v: &mut Vec<u8>, name: &str) {
    let b = name.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= b.len() {
        let boundary = i == b.len() || b.get(i) == Some(&b'.');
        if boundary {
            let n = i - start;
            if n > 0 && n <= 63 {
                v.push(n as u8);
                if let Some(s) = b.get(start..i) {
                    v.extend_from_slice(s);
                }
            }
            start = i + 1;
        }
        i += 1;
    }
    v.push(0);
}

fn push_field(v: &mut Vec<u8>, f: &[u8]) {
    let n = if f.len() > 255 { 255 } else { f.len() };
    v.push(n as u8);
    if let Some(s) = f.get(..n) {
        v.extend_from_slice(s);
    }
}

/// 编码第 idx 个服务的 mDNS 响应：1 问题（PTR）+ 1 回答（TXT，名字指针回引）。
///
/// 额外注入一个空字段与一个重复键，使「解析器计数正确」成为可检验的事实。
pub fn encode_service(idx: usize) -> Option<Vec<u8>> {
    let (name, pairs) = service_model(idx)?;
    let mut v: Vec<u8> = Vec::new();
    put_u16(&mut v, 0); // id
    put_u16(&mut v, 0x8400); // QR=1 AA=1
    put_u16(&mut v, 1); // qdcount
    put_u16(&mut v, 1); // ancount
    put_u16(&mut v, 0); // nscount
    put_u16(&mut v, 0); // arcount
    push_name(&mut v, &name);
    put_u16(&mut v, 12); // PTR
    put_u16(&mut v, 1); // IN
    // 回答名用指向问题名（偏移 12）的压缩指针，走真实压缩路径。
    v.push(0xC0);
    v.push(12);
    put_u16(&mut v, 16); // TXT
    put_u16(&mut v, 1); // IN
    put_u32(&mut v, 120); // ttl
    let len_at = v.len();
    put_u16(&mut v, 0); // rdlength 占位
    let rd_start = v.len();
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some(p) = pairs.get(i) {
            let mut field = String::new();
            field.push_str(p.key.as_str());
            field.push('=');
            field.push_str(p.value.as_str());
            push_field(&mut v, field.as_bytes());
        }
        i += 1;
    }
    v.push(0); // 空字段（合法噪声）
    push_field(&mut v, b"typ=shadow"); // 重复键（只记账）
    let rd_len = v.len() - rd_start;
    if rd_len > 0xffff {
        return None;
    }
    let hi = (rd_len >> 8) as u8;
    let lo = rd_len as u8;
    if let Some(cell) = v.get_mut(len_at) {
        *cell = hi;
    }
    if let Some(cell) = v.get_mut(len_at + 1) {
        *cell = lo;
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// 逐服务账本
// ---------------------------------------------------------------------------

/// 账本条目：一个服务一行，保存可复算摘要而非整包原文（定容结构不上堆外）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceEntry {
    pub service: String,
    pub pairs: u16,
    pub digest: u32,
    pub dup_keys: u16,
    pub rdata_len: u16,
}

/// 服务账本。容量固定（构造时给定），满即显式失败——不许「静默淘汰旧服务」，
/// 因为被淘汰的那一台打印机恰恰可能是用户刚插上的那台。
#[derive(Clone, Debug)]
pub struct TxtLedger {
    entries: Vec<ServiceEntry>,
    cap: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub err_counts: [u16; ERR_SLOTS],
    pub dup_keys: u32,
    /// 首个错误的段内码（0 表示无错）。保留首错比保留末错更能定位根因。
    pub first_err: u32,
}

impl TxtLedger {
    /// 建账本，`cap` 为服务条数上限。
    pub fn new(cap: usize) -> TxtLedger {
        TxtLedger {
            entries: Vec::new(),
            cap,
            accepted: 0,
            rejected: 0,
            err_counts: [0u16; ERR_SLOTS],
            dup_keys: 0,
            first_err: 0,
        }
    }

    /// 解析一包并落账；成功返回条目下标。
    pub fn record(&mut self, bytes: &[u8]) -> Result<usize, MdnsError> {
        match parse_response(bytes) {
            Ok(p) => {
                if self.entries.len() >= self.cap {
                    let e = MdnsError::Capacity;
                    self.note_err(e);
                    return Err(e);
                }
                self.entries.push(ServiceEntry {
                    service: p.record.service.clone(),
                    pairs: p.record.pairs.len() as u16,
                    digest: p.record.digest(),
                    dup_keys: p.record.dup_keys,
                    rdata_len: p.record.rdata_len,
                });
                self.accepted += 1;
                self.dup_keys += p.record.dup_keys as u32;
                Ok(self.entries.len() - 1)
            }
            Err(err) => {
                self.note_err(err);
                Err(err)
            }
        }
    }

    /// 记一次拒绝（坏包计数 + 逐类计数 + 首错留痕）。
    pub fn note_err(&mut self, e: MdnsError) {
        self.rejected += 1;
        let slot = e.slot();
        if let Some(c) = self.err_counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if self.first_err == 0 {
            self.first_err = e.code();
        }
    }

    /// 按服务名反查条目下标。
    pub fn find(&self, service: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.service == service {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 已收服务条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 有效属性总数。
    pub fn total_pairs(&self) -> usize {
        let mut sum = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum += e.pairs as usize;
            }
            i += 1;
        }
        sum
    }

    /// 某类错误的累计次数。
    pub fn err_count(&self, e: MdnsError) -> u16 {
        match self.err_counts.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 条目只读访问。
    pub fn entry(&self, i: usize) -> Option<&ServiceEntry> {
        self.entries.get(i)
    }
}

// ---------------------------------------------------------------------------
// 注入核：15 类坏样本，逐类断言具体诊断码
// ---------------------------------------------------------------------------

fn bad_header(qd: u16, an: u16) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    put_u16(&mut v, 0);
    put_u16(&mut v, 0x8400);
    put_u16(&mut v, qd);
    put_u16(&mut v, an);
    put_u16(&mut v, 0);
    put_u16(&mut v, 0);
    v
}

/// 构造一条只有 A 记录（type=1）的回答，服务名 3 标签，rdata 4 字节。
fn no_txt_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 1);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 4);
    v.extend_from_slice(&[10, 0, 0, 1]);
    v
}

/// 构造「回答 rdata 越界」的包：rdlength 声明 500 但报文已到尾部。
fn rdlen_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 500);
    v
}

/// 构造「TXT 字段长度前缀越界」的包：rdlen=4，字段声明 200 字节。
fn txt_len_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 4);
    v.push(200);
    v.extend_from_slice(&[b'a', b'=', b'1']);
    v
}

/// 构造「字段无等号」的包。
fn txt_field_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 4);
    v.push(3);
    v.extend_from_slice(b"abc");
    v
}

/// 构造「空键」的包：字段为 `=v`。
fn txt_key_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 3);
    v.push(2);
    v.extend_from_slice(b"=v");
    v
}

/// 构造「问题类型非 PTR」的包。
fn qtype_packet() -> Vec<u8> {
    let mut v = bad_header(1, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 1); // A，而非 PTR
    put_u16(&mut v, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 0);
    v
}

/// 构造「class 非 IN」的包。
fn class_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    push_name(&mut v, "prn.local");
    put_u16(&mut v, 16);
    put_u16(&mut v, 3); // CH
    put_u32(&mut v, 120);
    put_u16(&mut v, 0);
    v
}

/// 构造「标签保留位」的包。
fn label_bits_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    v.push(0x40);
    v.extend_from_slice(b"prn");
    v.push(0);
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 0);
    v
}

/// 构造「前向压缩指针」的包：指针目标 0x40 大于指针自身偏移。
fn loop_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    v.push(0xC0);
    v.push(0x40);
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 0);
    v
}

/// 构造「名字超 255 字节」的包：5 个 60 字节标签（61×5 = 305）。
fn long_name_packet() -> Vec<u8> {
    let mut v = bad_header(0, 1);
    let mut n = 0usize;
    while n < 5 {
        v.push(60u8);
        let mut k = 0usize;
        while k < 60 {
            v.push(b'a');
            k += 1;
        }
        n += 1;
    }
    v.push(0);
    put_u16(&mut v, 16);
    put_u16(&mut v, 1);
    put_u32(&mut v, 120);
    put_u16(&mut v, 0);
    v
}

/// 注入核返回（命中数，样本数，码位覆盖掩码）。
///
/// 15 个样本分别命中 15 个码，掩码应为 `(1<<15)-1`——判别依据是每个样本
/// 都断言**具体码**，不是「只要报错就算过」。
pub fn injection_core() -> (usize, usize, u32) {
    let mut caught = 0usize;
    let total = 15usize;
    let mut mask = 0u32;

    // 用拥有所有权的 Vec 承载样本：避免对临时数组取引用带来的生命周期问题。
    let mut cases: Vec<(Vec<u8>, MdnsError)> = Vec::new();
    cases.push((Vec::new(), MdnsError::Empty));
    cases.push((short_packet(), MdnsError::Short));
    cases.push((bad_header(99, 1), MdnsError::QdCount));
    cases.push((bad_header(0, 0), MdnsError::AnCount));
    cases.push((long_name_packet(), MdnsError::NameLen));
    cases.push((loop_packet(), MdnsError::NameLoop));
    cases.push((label_bits_packet(), MdnsError::NameChar));
    cases.push((qtype_packet(), MdnsError::Qtype));
    cases.push((class_packet(), MdnsError::Class));
    cases.push((rdlen_packet(), MdnsError::RdLen));
    cases.push((txt_len_packet(), MdnsError::TxtLen));
    cases.push((txt_field_packet(), MdnsError::TxtField));
    cases.push((txt_key_packet(), MdnsError::TxtKey));
    cases.push((no_txt_packet(), MdnsError::NoTxt));
    let mut i = 0usize;
    while i < cases.len() {
        if let Some((bytes, want)) = cases.get(i) {
            if parse_response(bytes).err() == Some(*want) {
                caught += 1;
                mask |= 1u32 << want.slot();
            }
        }
        i += 1;
    }
    // 账本容量：cap=1 的第二条记录
    let mut led = TxtLedger::new(1);
    if let Some(p) = encode_service(0) {
        let _ = led.record(&p);
    }
    if let Some(p) = encode_service(1) {
        if led.record(&p).err() == Some(MdnsError::Capacity) {
            caught += 1;
            mask |= 1u32 << MdnsError::Capacity.slot();
        }
    }
    (caught, total, mask)
}

/// 6 字节头（不足 12 字节 DNS 头）。
fn short_packet() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    put_u16(&mut v, 0);
    put_u16(&mut v, 0);
    put_u16(&mut v, 0);
    v
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50142 自检：20 服务逐键一致 + 坏包记账 + 15 类注入。
pub fn run_unx50142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50142");
    let mut led = TxtLedger::new(24);
    let mut parsed_ok = 0usize;
    let mut idx = 0usize;
    while idx < 20 {
        if let Some(bytes) = encode_service(idx) {
            if led.record(&bytes).is_ok() {
                parsed_ok += 1;
            }
        }
        idx += 1;
    }

    // J1-1 20 个服务全部可解析并入账
    let ok = parsed_ok == 20 && led.len() == 20 && led.rejected == 0;
    let det = format!("ok={} len={} rej={}", parsed_ok, led.len(), led.rejected);
    s.add_dyn("50142-J1-二十服务入账", ok, &det);

    // J1-2 逐键一致：20 服务 × 5 键 = 100 项
    let mut matched = 0usize;
    let mut compared = 0usize;
    let mut i = 0usize;
    while i < 20 {
        if let (Some(bytes), Some((_, model))) = (encode_service(i), service_model(i)) {
            match parse_response(&bytes) {
                Ok(p) => {
                    let mut k = 0usize;
                    while k < model.len() {
                        if let (Some(a), Some(b)) = (p.record.pairs.get(k), model.get(k)) {
                            if a == b {
                                matched += 1;
                            }
                            compared += 1;
                        }
                        k += 1;
                    }
                }
                Err(_) => {
                    compared += model.len();
                }
            }
        }
        i += 1;
    }
    let ok = compared == 100 && matched == compared;
    let det = format!("matched={}/{}", matched, compared);
    s.add_dyn("50142-J1-逐键一致", ok, &det);

    // J1-3 键序保持：首值优先 + 包内原始顺序
    let mut order_ok = true;
    let mut j = 0usize;
    while j < 20 {
        if let (Some(bytes), Some((_, model))) = (encode_service(j), service_model(j)) {
            if let Ok(p) = parse_response(&bytes) {
                if p.record.pairs != model {
                    order_ok = false;
                }
            } else {
                order_ok = false;
            }
        } else {
            order_ok = false;
        }
        j += 1;
    }
    s.add("50142-J1-键序保持", order_ok, if order_ok { "" } else { "键序或首值被改写" });

    // J1-4 重复键：每包 1 次，值未被覆盖
    let mut dup_ok = true;
    let mut rp = 0usize;
    while rp < 20 {
        if let Some(bytes) = encode_service(rp) {
            match parse_response(&bytes) {
                Ok(p) => {
                    let typ_ok = match p.record.get("typ") {
                        Some(v) => v.value != "shadow",
                        None => false,
                    };
                    if p.record.dup_keys != 1 || !typ_ok {
                        dup_ok = false;
                    }
                }
                Err(_) => dup_ok = false,
            }
        }
        rp += 1;
    }
    s.add("50142-J1-重复键记账", dup_ok, if dup_ok { "" } else { "重复键覆盖首值或未记账" });

    // J1-5 空字段：每包 1 个，不计入属性
    let mut empty_ok = true;
    let mut ef = 0usize;
    while ef < 20 {
        if let Some(bytes) = encode_service(ef) {
            match parse_response(&bytes) {
                Ok(p) => {
                    if p.record.empty_fields != 1 || p.record.pair_count() != 5 {
                        empty_ok = false;
                    }
                }
                Err(_) => empty_ok = false,
            }
        }
        ef += 1;
    }
    s.add("50142-J1-空字段计数", empty_ok, if empty_ok { "" } else { "空字段被吞或计数错" });

    // J1-6 值内含 '='：只按首个 '=' 切分
    let mut eq_ok = 0usize;
    let mut eq = 0usize;
    while eq < 20 {
        if let Some(bytes) = encode_service(eq) {
            if let Ok(p) = parse_response(&bytes) {
                if let Some(v) = p.record.get("path") {
                    if v.value.starts_with("a=b") && v.value.ends_with(";c") {
                        eq_ok += 1;
                    }
                }
            }
        }
        eq += 1;
    }
    let ok = eq_ok == 20;
    let det = format!("split_ok={}/20", eq_ok);
    s.add_dyn("50142-J1-首等号切分", ok, &det);

    // J1-7 压缩名往返：owner 名字与模型完全一致
    let mut name_ok = 0usize;
    let mut ni = 0usize;
    while ni < 20 {
        if let (Some(bytes), Some((name, _))) = (encode_service(ni), service_model(ni)) {
            if let Ok(p) = parse_response(&bytes) {
                if p.record.service == name {
                    name_ok += 1;
                }
            }
        }
        ni += 1;
    }
    let ok = name_ok == 20;
    let det = format!("name_ok={}/20", name_ok);
    s.add_dyn("50142-J1-压缩名往返", ok, &det);

    // J1-8 消费守恒：解析偏移 == 报文长度，且问题/回答计数为 1
    let mut cons_ok = 0usize;
    let mut ci = 0usize;
    while ci < 20 {
        if let Some(bytes) = encode_service(ci) {
            if let Ok(p) = parse_response(&bytes) {
                if p.consumed == bytes.len() && p.questions == 1 && p.answers == 1 && p.skipped == 0 {
                    cons_ok += 1;
                }
            }
        }
        ci += 1;
    }
    let ok = cons_ok == 20;
    let det = format!("consumed_ok={}/20", cons_ok);
    s.add_dyn("50142-J1-消费守恒", ok, &det);

    // J1-9 解析确定性 + 账本摘要一致
    let mut det_ok = true;
    if let (Some(bytes), Some(entry)) = (encode_service(9), led.entry(9)) {
        match (parse_response(&bytes), parse_response(&bytes)) {
            (Ok(a), Ok(b)) => {
                if a.record.digest() != b.record.digest() || a.record.digest() != entry.digest {
                    det_ok = false;
                }
                if a.record.rdata_len != entry.rdata_len {
                    det_ok = false;
                }
            }
            _ => det_ok = false,
        }
    } else {
        det_ok = false;
    }
    s.add("50142-J1-解析确定", det_ok, if det_ok { "" } else { "两次解析/账本摘要不一致" });

    // J1-10 账本守恒：accepted 与逐条属性总数对得上
    let ok = led.accepted == 20 && led.total_pairs() == 100 && led.dup_keys == 20;
    let det = format!("acc={} pairs={} dup={}", led.accepted, led.total_pairs(), led.dup_keys);
    s.add_dyn("50142-J1-账本守恒", ok, &det);

    // J1-11 按名反查：第 7 个服务的条目就是第 7 行
    let mut find_ok = false;
    if let Some((name, _)) = service_model(7) {
        if let Some(pos) = led.find(name.as_str()) {
            if let Some(e) = led.entry(pos) {
                find_ok = pos == 7 && e.service == name;
            }
        }
    }
    s.add("50142-J1-按名反查", find_ok, if find_ok { "" } else { "反查错位" });

    // J1-12 账本容量：cap=1 时第二条必须显式 Capacity
    let mut small = TxtLedger::new(1);
    let mut cap_ok = false;
    if let Some(p0) = encode_service(0) {
        if small.record(&p0).is_ok() {
            if let Some(p1) = encode_service(1) {
                cap_ok = small.record(&p1).err() == Some(MdnsError::Capacity)
                    && small.accepted == 1
                    && small.rejected == 1
                    && small.first_err == MdnsError::Capacity.code();
            }
        }
    }
    s.add("50142-J1-账本容量", cap_ok, if cap_ok { "" } else { "容量未拦截或计数错" });

    // J1-13 坏包记账：6 类坏包逐类计数，首错留痕
    let mut bad = TxtLedger::new(8);
    let mut bads: Vec<(Vec<u8>, MdnsError)> = Vec::new();
    bads.push((Vec::new(), MdnsError::Empty));
    bads.push((short_packet(), MdnsError::Short));
    bads.push((loop_packet(), MdnsError::NameLoop));
    bads.push((rdlen_packet(), MdnsError::RdLen));
    bads.push((txt_field_packet(), MdnsError::TxtField));
    bads.push((txt_key_packet(), MdnsError::TxtKey));
    let mut b = 0usize;
    while b < bads.len() {
        if let Some((bytes, _)) = bads.get(b) {
            let _ = bad.record(bytes);
        }
        b += 1;
    }
    let mut per_class = true;
    let mut c = 0usize;
    while c < bads.len() {
        if let Some((_, e)) = bads.get(c) {
            if bad.err_count(*e) != 1 {
                per_class = false;
            }
        }
        c += 1;
    }
    let ok = bad.rejected == 6 && bad.accepted == 0 && per_class && bad.first_err == MdnsError::Empty.code();
    let det = format!("rej={} first={:x}", bad.rejected, bad.first_err);
    s.add_dyn("50142-J1-坏包记账", ok, &det);

    // J1-14 前向/自指指针必拒（终止性）
    let self_ptr = {
        let mut v = bad_header(0, 1);
        v.push(0xC0);
        v.push(12); // 指向自身（pos=12）
        put_u16(&mut v, 16);
        put_u16(&mut v, 1);
        put_u32(&mut v, 120);
        put_u16(&mut v, 0);
        v
    };
    let ok = parse_response(&loop_packet()).err() == Some(MdnsError::NameLoop)
        && parse_response(&self_ptr).err() == Some(MdnsError::NameLoop);
    s.add("50142-J1-指针必回退", ok, if ok { "" } else { "自指/前向指针被接受" });

    // J1-15 截断族必拒：rdlen / txt 前缀 / 无等号 / 空键
    let ok = parse_response(&rdlen_packet()).err() == Some(MdnsError::RdLen)
        && parse_response(&txt_len_packet()).err() == Some(MdnsError::TxtLen)
        && parse_response(&txt_field_packet()).err() == Some(MdnsError::TxtField)
        && parse_response(&txt_key_packet()).err() == Some(MdnsError::TxtKey)
        && parse_response(&no_txt_packet()).err() == Some(MdnsError::NoTxt);
    s.add("50142-J1-截断族必拒", ok, if ok { "" } else { "截断/无TXT被接受" });

    // J1-16 注入核：15/15 且码位全覆盖
    let (caught, total, mask) = injection_core();
    let full = (1u32 << ERR_SLOTS) - 1;
    let ok = caught == total && mask == full;
    let det = format!("注入 {}/{} mask={:x}", caught, total, mask);
    s.add_dyn("50142-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50142_checks_green() {
        let set = run_unx50142_checks();
        assert!(set.all_passed(), "UNX-F50142 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50142_service_roundtrip() {
        let mut i = 0usize;
        while i < 20 {
            let bytes = match encode_service(i) {
                Some(v) => v,
                None => {
                    assert!(false, "encode failed at {}", i);
                    return;
                }
            };
            let p = match parse_response(&bytes) {
                Ok(v) => v,
                Err(e) => {
                    assert!(false, "parse failed at {}: {:?}", i, e);
                    return;
                }
            };
            let model = match service_model(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(p.record.service, model.0);
            assert_eq!(p.record.pairs, model.1);
            assert_eq!(p.consumed, bytes.len());
            i += 1;
        }
    }

    #[test]
    fn unx50142_pointer_must_backtrack() {
        let mut v = bad_header(0, 1);
        v.push(0xC0);
        v.push(12);
        v.extend_from_slice(&[0, 16, 0, 1, 0, 0, 0, 120, 0, 0]);
        assert_eq!(parse_response(&v).err(), Some(MdnsError::NameLoop));
    }

    #[test]
    fn unx50142_injection_covers_all_codes() {
        let (caught, total, mask) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(mask, (1u32 << ERR_SLOTS) - 1);
    }
}
