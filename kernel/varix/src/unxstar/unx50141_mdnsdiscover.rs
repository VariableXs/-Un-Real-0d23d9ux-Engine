//! UNX-F50141 mDNS 打印服务发现（_ipp._tcp / _printer._tcp 双型）（0x43dc0 段）
//!
//! 锚点：UNX-F50141-J1 双型发现 ×3 模拟器服务零漏报，离线摘除零残留；
//! 判据锚定 §AI-63 §2 专题五。
//!
//! 这个模块是一条完整的 DNS-SD 链路：发查询 → 收应答（PTR/SRV/TXT/A）→
//! 组装服务记录 → 按 TTL 老化 → 收 goodbye 摘除。它面对的是**别人发的报文**，
//! 所以解析器的每一条读路径都必须先校验区间：名字压缩指针只许向后跳、
//! 跳数封顶 64、标签长度 1..=63、总长 ≤255、rdata 长度越界即拒。任何一条
//! 缺失都会让一个 20 字节的恶意报文把内核拖进死循环或越界读。
//!
//! 「零漏报」的定义：三个模拟器服务在两个服务类型上都完整落地（PTR 给出
//! 实例、SRV 给出主机与端口、TXT 给出属性、A 给出地址），`residue() == 0`。
//! 「零残留」的定义：收到 goodbye（TTL=0）后注册表为空、半成品计数为 0、
//! 摘要回到空表初值——半成品（只有 PTR 没有 SRV 的实例）必须显性计数，
//! 不能悄悄算作「已发现」。
//!
//! 数据结构（解析器 + 编码器 + 注册表 + 老化器）：
//! * `read_name` / `parse_message` —— 带压缩指针的 DNS 名字与报文解析；
//! * `MessageWriter` —— 应答/查询编码器（压缩与不压缩两档，用于验证压缩正确）；
//! * `PrinterService` / `PrinterRegistry` —— 服务注册表（四件套齐才算完整）；
//! * `SimPrinter` / `simulator_fleet` —— 三台模拟器与公告/goodbye 生成器。
//!
//! 错误闭集：`SEG_BASE + n`（0x43dc0 段），十二个码值全部被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43dc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43dc0;

pub const E_SHORT: u16 = 1;
pub const E_LABEL: u16 = 2;
pub const E_POINTER: u16 = 3;
pub const E_TYPE: u16 = 4;
pub const E_CLASS: u16 = 5;
pub const E_RDATA: u16 = 6;
pub const E_COUNT: u16 = 7;
pub const E_PORT: u16 = 8;
pub const E_NAME: u16 = 9;
pub const E_TXT: u16 = 10;
pub const E_KIND: u16 = 11;
pub const E_EMPTY: u16 = 12;

/// mDNS 错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdnsError {
    Short,
    Label,
    Pointer,
    Type,
    Class,
    Rdata,
    Count,
    Port,
    Name,
    Txt,
    Kind,
    Empty,
}

impl MdnsError {
    /// 诊断码 = 段基址 + 序号。
    pub fn code(self) -> u32 {
        let n = match self {
            MdnsError::Short => E_SHORT,
            MdnsError::Label => E_LABEL,
            MdnsError::Pointer => E_POINTER,
            MdnsError::Type => E_TYPE,
            MdnsError::Class => E_CLASS,
            MdnsError::Rdata => E_RDATA,
            MdnsError::Count => E_COUNT,
            MdnsError::Port => E_PORT,
            MdnsError::Name => E_NAME,
            MdnsError::Txt => E_TXT,
            MdnsError::Kind => E_KIND,
            MdnsError::Empty => E_EMPTY,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            MdnsError::Short => "packet shorter than declared field",
            MdnsError::Label => "label tag or length illegal",
            MdnsError::Pointer => "compression pointer forward or too many jumps",
            MdnsError::Type => "record type unsupported",
            MdnsError::Class => "record class is not IN",
            MdnsError::Rdata => "rdata length crosses packet bounds",
            MdnsError::Count => "section record count above 64",
            MdnsError::Port => "SRV port is zero",
            MdnsError::Name => "name empty, label empty or name above 255 bytes",
            MdnsError::Txt => "TXT entry malformed or empty key",
            MdnsError::Kind => "instance advertised under two service types",
            MdnsError::Empty => "message carries no answer records",
        }
    }
}

/// 报文里最多接受的记录数（PTR+SRV+TXT+A 三台的公告是 12 条，留 5 倍余量）。
pub const MAX_RECORDS: u32 = 64;
/// 单条 TXT 属性串上限（DNS 规定单串 ≤255）。
pub const MAX_TXT_STRING: usize = 255;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(FNV_PRIME)
}

fn fnv_u16(mut h: u64, v: u16) -> u64 {
    h = fnv_u8(h, (v & 0xff) as u8);
    fnv_u8(h, (v >> 8) as u8)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut i = 0usize;
    while i < 4 {
        h = fnv_u8(h, (v >> (i * 8)) as u8);
        i += 1;
    }
    h
}

fn ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

fn ascii_ieq(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.len() {
        match (x.get(i), y.get(i)) {
            (Some(p), Some(q)) => {
                if ascii_lower(*p) != ascii_lower(*q) {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// DNS-SD 服务类型。双型发现是 J1 的核心：两种类型各有一套 PTR。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceType {
    Ipp,
    Printer,
}

impl ServiceType {
    /// 类型码（0=ipp，1=printer）。
    pub fn code(self) -> u8 {
        match self {
            ServiceType::Ipp => 0,
            ServiceType::Printer => 1,
        }
    }

    /// 码值反解。
    pub fn from_code(c: u8) -> Result<ServiceType, MdnsError> {
        match c {
            0 => Ok(ServiceType::Ipp),
            1 => Ok(ServiceType::Printer),
            _ => Err(MdnsError::Type),
        }
    }

    /// 服务标签（不含 `.local`）。
    pub fn label(self) -> &'static str {
        match self {
            ServiceType::Ipp => "_ipp._tcp",
            ServiceType::Printer => "_printer._tcp",
        }
    }

    /// 完整服务名（`.local` 域）。
    pub fn service_name(self) -> &'static str {
        match self {
            ServiceType::Ipp => "_ipp._tcp.local",
            ServiceType::Printer => "_printer._tcp.local",
        }
    }

    /// 由完整服务名反查（大小写不敏感；mDNS 名字比较本就忽略大小写）。
    pub fn from_service_name(n: &str) -> Option<ServiceType> {
        if ascii_ieq(n, ServiceType::Ipp.service_name()) {
            Some(ServiceType::Ipp)
        } else if ascii_ieq(n, ServiceType::Printer.service_name()) {
            Some(ServiceType::Printer)
        } else {
            None
        }
    }

    /// 默认端口提示（ipp=631，printer/lpd=515）；仅用于诊断渲染。
    pub fn port_hint(self) -> u16 {
        match self {
            ServiceType::Ipp => 631,
            ServiceType::Printer => 515,
        }
    }
}

/// 记录类型闭集（只收打印发现必需的四种 + AAAA）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RType {
    A,
    Ptr,
    Txt,
    Srv,
    Aaaa,
}

impl RType {
    /// 线上的类型码。
    pub fn code(self) -> u16 {
        match self {
            RType::A => 1,
            RType::Ptr => 12,
            RType::Txt => 16,
            RType::Srv => 33,
            RType::Aaaa => 28,
        }
    }

    /// 类型码反解；未支持的类型显式拒绝（不静默丢弃，否则「零漏报」会造假）。
    pub fn from_u16(v: u16) -> Result<RType, MdnsError> {
        match v {
            1 => Ok(RType::A),
            12 => Ok(RType::Ptr),
            16 => Ok(RType::Txt),
            33 => Ok(RType::Srv),
            28 => Ok(RType::Aaaa),
            _ => Err(MdnsError::Type),
        }
    }

    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            RType::A => "A",
            RType::Ptr => "PTR",
            RType::Txt => "TXT",
            RType::Srv => "SRV",
            RType::Aaaa => "AAAA",
        }
    }
}

/// 记录数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RData {
    Addr([u8; 4]),
    Ptr(String),
    Txt(Vec<(String, String)>),
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },
    Aaaa([u8; 16]),
}

/// 一条资源记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub rtype: RType,
    pub ttl: u32,
    pub data: RData,
}

impl Record {
    /// 一行渲染（诊断用）。
    pub fn render(&self) -> String {
        format!("{} {} ttl={}", self.name, self.rtype.label(), self.ttl)
    }
}

/// 解析后的报文。问题数保留在 `questions` 里，便于查询报文自检。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: u16,
    pub flags: u16,
    pub questions: u16,
    pub records: Vec<Record>,
}

fn be_u16(d: &[u8], at: usize) -> Result<u16, MdnsError> {
    let a = match d.get(at) {
        Some(v) => *v as u16,
        None => return Err(MdnsError::Short),
    };
    let b = match d.get(at + 1) {
        Some(v) => *v as u16,
        None => return Err(MdnsError::Short),
    };
    Ok((a << 8) | b)
}

fn be_u32(d: &[u8], at: usize) -> Result<u32, MdnsError> {
    let a = be_u16(d, at)? as u32;
    let b = be_u16(d, at + 2)? as u32;
    Ok((a << 16) | b)
}

/// 读一条（可能带压缩指针的）DNS 名字，返回 (点名, 名字在主线上占用的下一偏移)。
///
/// 压缩指针只许指向**严格更小**的偏移：这样每跳至少回退一字节，配合 64 跳
/// 上限可以保证终止。正向指针（含自环）直接 `E_POINTER`——现实里正常报文
/// 不会出现，出现了就是构造攻击。
pub fn read_name(buf: &[u8], start: usize) -> Result<(String, usize), MdnsError> {
    let mut out = String::new();
    let mut at = start;
    let mut jumps = 0usize;
    let mut end_at: Option<usize> = None;
    loop {
        let b = match buf.get(at) {
            Some(v) => *v,
            None => return Err(MdnsError::Short),
        };
        if b == 0 {
            if end_at.is_none() {
                end_at = Some(at + 1);
            }
            break;
        }
        let tag = b & 0xc0;
        if tag == 0xc0 {
            let lo = match buf.get(at + 1) {
                Some(v) => *v as usize,
                None => return Err(MdnsError::Short),
            };
            let target = (((b & 0x3f) as usize) << 8) | lo;
            if end_at.is_none() {
                end_at = Some(at + 2);
            }
            if target >= at || jumps >= 64 {
                return Err(MdnsError::Pointer);
            }
            jumps += 1;
            at = target;
            continue;
        }
        if tag != 0 {
            return Err(MdnsError::Label);
        }
        let len = b as usize;
        if len > 63 {
            return Err(MdnsError::Label);
        }
        let end = at + 1 + len;
        if end > buf.len() {
            return Err(MdnsError::Short);
        }
        if !out.is_empty() {
            out.push('.');
        }
        let mut i = at + 1;
        while i < end {
            let c = match buf.get(i) {
                Some(v) => *v,
                None => return Err(MdnsError::Short),
            };
            if c < 0x20 || c >= 0x7f {
                return Err(MdnsError::Label);
            }
            out.push(c as char);
            i += 1;
        }
        if out.len() > 255 {
            return Err(MdnsError::Name);
        }
        at = end;
    }
    Ok((
        out,
        match end_at {
            Some(v) => v,
            None => start,
        },
    ))
}

fn parse_txt(buf: &[u8], start: usize, rend: usize) -> Result<Vec<(String, String)>, MdnsError> {
    let mut out = Vec::new();
    let mut at = start;
    while at < rend {
        let l = match buf.get(at) {
            Some(v) => *v as usize,
            None => return Err(MdnsError::Rdata),
        };
        at += 1;
        if at + l > rend {
            return Err(MdnsError::Rdata);
        }
        if l > 0 {
            let end = at + l;
            let mut key = String::new();
            let mut val = String::new();
            let mut seen_eq = false;
            let mut i = at;
            while i < end {
                let c = match buf.get(i) {
                    Some(v) => *v,
                    None => return Err(MdnsError::Rdata),
                };
                if c < 0x20 || c > 0x7e {
                    return Err(MdnsError::Txt);
                }
                if c == b'=' && !seen_eq {
                    seen_eq = true;
                } else if seen_eq {
                    val.push(c as char);
                } else {
                    key.push(c as char);
                }
                i += 1;
            }
            if key.is_empty() {
                return Err(MdnsError::Txt);
            }
            out.push((key, val));
            at = end;
        }
    }
    if at != rend {
        return Err(MdnsError::Rdata);
    }
    Ok(out)
}

fn parse_rdata(
    buf: &[u8],
    rtype: RType,
    rstart: usize,
    rend: usize,
) -> Result<RData, MdnsError> {
    match rtype {
        RType::Ptr => {
            let (t, end) = read_name(buf, rstart)?;
            if end > rend {
                return Err(MdnsError::Rdata);
            }
            Ok(RData::Ptr(t))
        }
        RType::Srv => {
            if rstart + 6 > rend {
                return Err(MdnsError::Rdata);
            }
            let priority = be_u16(buf, rstart)?;
            let weight = be_u16(buf, rstart + 2)?;
            let port = be_u16(buf, rstart + 4)?;
            let (target, end) = read_name(buf, rstart + 6)?;
            if end > rend {
                return Err(MdnsError::Rdata);
            }
            Ok(RData::Srv {
                priority,
                weight,
                port,
                target,
            })
        }
        RType::Txt => Ok(RData::Txt(parse_txt(buf, rstart, rend)?)),
        RType::A => {
            if rend != rstart + 4 {
                return Err(MdnsError::Rdata);
            }
            let mut ip = [0u8; 4];
            let mut i = 0usize;
            while i < 4 {
                ip[i] = match buf.get(rstart + i) {
                    Some(v) => *v,
                    None => return Err(MdnsError::Rdata),
                };
                i += 1;
            }
            Ok(RData::Addr(ip))
        }
        RType::Aaaa => {
            if rend != rstart + 16 {
                return Err(MdnsError::Rdata);
            }
            let mut ip = [0u8; 16];
            let mut i = 0usize;
            while i < 16 {
                ip[i] = match buf.get(rstart + i) {
                    Some(v) => *v,
                    None => return Err(MdnsError::Rdata),
                };
                i += 1;
            }
            Ok(RData::Aaaa(ip))
        }
    }
}

/// 解析一条 DNS 报文。问题段只做区间跳过（mDNS 查询里问题名不参与发现）。
pub fn parse_message(buf: &[u8]) -> Result<Message, MdnsError> {
    if buf.len() < 12 {
        return Err(MdnsError::Short);
    }
    let id = be_u16(buf, 0)?;
    let flags = be_u16(buf, 2)?;
    let qd = be_u16(buf, 4)?;
    let an = be_u16(buf, 6)?;
    let ns = be_u16(buf, 8)?;
    let ar = be_u16(buf, 10)?;
    let total = qd as u32 + an as u32 + ns as u32 + ar as u32;
    if total > MAX_RECORDS {
        return Err(MdnsError::Count);
    }
    let mut at = 12usize;
    let mut q = 0u16;
    while q < qd {
        let (_n, next) = read_name(buf, at)?;
        if next + 4 > buf.len() {
            return Err(MdnsError::Short);
        }
        let cls = be_u16(buf, next + 2)?;
        if cls & 0x7fff != 1 {
            return Err(MdnsError::Class);
        }
        at = next + 4;
        q += 1;
    }
    let rr = an as u32 + ns as u32 + ar as u32;
    let mut records = Vec::with_capacity(rr as usize);
    let mut r = 0u32;
    while r < rr {
        let (name, next) = read_name(buf, at)?;
        if next + 10 > buf.len() {
            return Err(MdnsError::Short);
        }
        let rt_raw = be_u16(buf, next)?;
        let cls = be_u16(buf, next + 2)?;
        let ttl = be_u32(buf, next + 4)?;
        let rdlen = be_u16(buf, next + 8)? as usize;
        let rstart = next + 10;
        let rend = rstart + rdlen;
        if rend > buf.len() {
            return Err(MdnsError::Rdata);
        }
        if cls & 0x7fff != 1 {
            return Err(MdnsError::Class);
        }
        let rtype = RType::from_u16(rt_raw)?;
        let data = parse_rdata(buf, rtype, rstart, rend)?;
        records.push(Record {
            name,
            rtype,
            ttl,
            data,
        });
        at = rend;
        r += 1;
    }
    Ok(Message {
        id,
        flags,
        questions: qd,
        records,
    })
}

// ---------------------------------------------------------------------------
// 编码器
// ---------------------------------------------------------------------------

fn split_labels(name: &str) -> Result<Vec<&str>, MdnsError> {
    let bytes = name.as_bytes();
    let trimmed = if bytes.last() == Some(&b'.') {
        match name.get(..name.len() - 1) {
            Some(v) => v,
            None => return Err(MdnsError::Name),
        }
    } else {
        name
    };
    if trimmed.is_empty() {
        return Err(MdnsError::Name);
    }
    let raw = trimmed.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= raw.len() {
        if i == raw.len() || raw.get(i) == Some(&b'.') {
            let label = match trimmed.get(start..i) {
                Some(v) => v,
                None => return Err(MdnsError::Name),
            };
            if label.is_empty() || label.len() > 63 {
                return Err(MdnsError::Name);
            }
            let lb = label.as_bytes();
            let mut k = 0usize;
            while k < lb.len() {
                if let Some(b) = lb.get(k) {
                    if *b < 0x20 || *b >= 0x7f {
                        return Err(MdnsError::Name);
                    }
                }
                k += 1;
            }
            out.push(label);
            start = i + 1;
        }
        i += 1;
    }
    Ok(out)
}

fn join_labels(labels: &[&str]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < labels.len() {
        if i > 0 {
            s.push('.');
        }
        if let Some(l) = labels.get(i) {
            s.push_str(l);
        }
        i += 1;
    }
    s
}

/// 应答/查询编码器。`plain` 档关掉名字压缩，用于验证压缩确实省字节且可逆。
pub struct MessageWriter {
    buf: Vec<u8>,
    qd: u16,
    an: u16,
    names: Vec<(String, usize)>,
    compress: bool,
}

impl MessageWriter {
    /// 带压缩的编码器（正常路径）。
    pub fn new(id: u16) -> MessageWriter {
        MessageWriter::with_compression(id, true)
    }

    /// 不带压缩的编码器（对照路径）。
    pub fn plain(id: u16) -> MessageWriter {
        MessageWriter::with_compression(id, false)
    }

    fn with_compression(id: u16, compress: bool) -> MessageWriter {
        let mut buf = Vec::with_capacity(512);
        buf.push((id >> 8) as u8);
        buf.push(id as u8);
        buf.push(0x84);
        buf.push(0x00);
        let mut i = 0usize;
        while i < 8 {
            buf.push(0u8);
            i += 1;
        }
        MessageWriter {
            buf,
            qd: 0,
            an: 0,
            names: Vec::new(),
            compress,
        }
    }

    fn write_u16(&mut self, v: u16) {
        self.buf.push((v >> 8) as u8);
        self.buf.push(v as u8);
    }

    fn write_u32(&mut self, v: u32) {
        self.buf.push((v >> 24) as u8);
        self.buf.push((v >> 16) as u8);
        self.buf.push((v >> 8) as u8);
        self.buf.push(v as u8);
    }

    fn patch_u16(&mut self, at: usize, v: u16) {
        if let Some(s) = self.buf.get_mut(at) {
            *s = (v >> 8) as u8;
        }
        if let Some(s) = self.buf.get_mut(at + 1) {
            *s = v as u8;
        }
    }

    fn find_name(&self, full: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.names.len() {
            if let Some((n, off)) = self.names.get(i) {
                if ascii_ieq(n, full) {
                    return Some(*off);
                }
            }
            i += 1;
        }
        None
    }

    fn write_name(&mut self, name: &str) -> Result<usize, MdnsError> {
        let labels = split_labels(name)?;
        let mut ptr_off: Option<usize> = None;
        let mut upto = labels.len();
        if self.compress {
            let mut k = 0usize;
            while k < labels.len() {
                let suffix = join_labels(&labels[k..]);
                if let Some(off) = self.find_name(&suffix) {
                    ptr_off = Some(off);
                    upto = k;
                    break;
                }
                k += 1;
            }
        }
        let start = self.buf.len();
        let mut i = 0usize;
        while i < upto {
            if let Some(l) = labels.get(i) {
                let lb = l.as_bytes();
                self.buf.push(lb.len() as u8);
                let mut j = 0usize;
                while j < lb.len() {
                    if let Some(b) = lb.get(j) {
                        self.buf.push(*b);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        match ptr_off {
            Some(off) => {
                self.buf.push(0xc0 | ((off >> 8) as u8 & 0x3f));
                self.buf.push(off as u8);
            }
            None => self.buf.push(0u8),
        }
        let full = join_labels(&labels);
        if !full.is_empty() {
            self.names.push((full, start));
        }
        Ok(start)
    }

    fn begin_rr(&mut self, name: &str, rtype: u16, ttl: u32) -> Result<usize, MdnsError> {
        self.write_name(name)?;
        self.write_u16(rtype);
        self.write_u16(1);
        self.write_u32(ttl);
        let at = self.buf.len();
        self.write_u16(0);
        Ok(at)
    }

    fn end_rr(&mut self, len_at: usize) -> Result<(), MdnsError> {
        let end = self.buf.len();
        let start = len_at + 2;
        if end < start {
            return Err(MdnsError::Rdata);
        }
        let len = end - start;
        if len > 0xffff {
            return Err(MdnsError::Rdata);
        }
        self.patch_u16(len_at, len as u16);
        self.an = self.an.saturating_add(1);
        Ok(())
    }

    /// 写一个 PTR 查询问题。
    pub fn question(&mut self, kind: ServiceType) -> Result<(), MdnsError> {
        self.write_name(ServiceType::service_name(kind))?;
        self.write_u16(12);
        self.write_u16(1);
        self.qd = self.qd.saturating_add(1);
        Ok(())
    }

    /// 写一条 PTR 应答（服务类型 → 实例）。
    pub fn answer_ptr(
        &mut self,
        kind: ServiceType,
        instance: &str,
        ttl: u32,
    ) -> Result<(), MdnsError> {
        let at = self.begin_rr(ServiceType::service_name(kind), 12, ttl)?;
        self.write_name(instance)?;
        self.end_rr(at)
    }

    /// 写一条 SRV 应答。端口 0 在编码侧就拒——mDNS 里端口 0 只可能是坏数据。
    pub fn answer_srv(
        &mut self,
        instance: &str,
        host: &str,
        port: u16,
        ttl: u32,
    ) -> Result<(), MdnsError> {
        if port == 0 {
            return Err(MdnsError::Port);
        }
        self.write_srv_unchecked(instance, host, port, ttl)
    }

    /// 不做端口校验的 SRV 写入（注入核造坏包用，正常路径走 `answer_srv`）。
    fn write_srv_unchecked(
        &mut self,
        instance: &str,
        host: &str,
        port: u16,
        ttl: u32,
    ) -> Result<(), MdnsError> {
        let at = self.begin_rr(instance, 33, ttl)?;
        self.write_u16(0);
        self.write_u16(0);
        self.write_u16(port);
        self.write_name(host)?;
        self.end_rr(at)
    }

    /// 写一条 TXT 应答（key=value 串）。单串超过 255 即拒。
    pub fn answer_txt(
        &mut self,
        instance: &str,
        pairs: &[(&str, &str)],
        ttl: u32,
    ) -> Result<(), MdnsError> {
        let at = self.begin_rr(instance, 16, ttl)?;
        let mut i = 0usize;
        while i < pairs.len() {
            if let Some((k, v)) = pairs.get(i) {
                let mut s = String::new();
                s.push_str(k);
                s.push('=');
                s.push_str(v);
                let bytes = s.as_bytes();
                if bytes.is_empty() || bytes.len() > MAX_TXT_STRING {
                    return Err(MdnsError::Txt);
                }
                self.buf.push(bytes.len() as u8);
                let mut j = 0usize;
                while j < bytes.len() {
                    if let Some(b) = bytes.get(j) {
                        self.buf.push(*b);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        self.end_rr(at)
    }

    /// 写一条 A 应答。
    pub fn answer_a(&mut self, host: &str, ip: [u8; 4], ttl: u32) -> Result<(), MdnsError> {
        let at = self.begin_rr(host, 1, ttl)?;
        let mut i = 0usize;
        while i < 4 {
            if let Some(b) = ip.get(i) {
                self.buf.push(*b);
            }
            i += 1;
        }
        self.end_rr(at)
    }

    /// 收尾：回填各段计数，交出字节流。
    pub fn finish(mut self) -> Vec<u8> {
        self.patch_u16(4, self.qd);
        self.patch_u16(6, self.an);
        self.buf
    }
}

// ---------------------------------------------------------------------------
// 注册表
// ---------------------------------------------------------------------------

/// 一台被发现的打印服务。四件套（实例/主机/端口/地址/属性）齐才算完整。
#[derive(Clone, Debug)]
pub struct PrinterService {
    pub instance: String,
    pub kind: ServiceType,
    pub host: String,
    pub port: u16,
    pub addr: Option<[u8; 4]>,
    pub txt: Vec<(String, String)>,
    pub ttl: u32,
    pub seen_ms: u64,
}

impl PrinterService {
    /// 四件套是否齐全（PTR 给了实例、SRV 给了主机端口、A 给了地址、TXT 给了属性）。
    pub fn complete(&self) -> bool {
        !self.host.is_empty() && self.port != 0 && self.addr.is_some() && !self.txt.is_empty()
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        let ip = match self.addr {
            Some(a) => format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]),
            None => "0.0.0.0".to_string(),
        };
        format!(
            "{} {}:{} -> {} ttl={}",
            self.instance,
            ip,
            self.port,
            self.host,
            self.ttl
        )
    }
}

/// 服务注册表：PTR 建实例、SRV/TXT/A 补全，TTL 老化，goodbye 摘除。
#[derive(Clone, Debug)]
pub struct PrinterRegistry {
    services: Vec<PrinterService>,
    applied: u32,
}

impl PrinterRegistry {
    /// 空表。
    pub fn new() -> PrinterRegistry {
        PrinterRegistry {
            services: Vec::new(),
            applied: 0,
        }
    }

    /// 服务条数。
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// 是否为空表。
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// 已处理记录累计（只增不减，用于「报文确实被消费」）。
    pub fn applied(&self) -> u32 {
        self.applied
    }

    /// 只读视图。
    pub fn services(&self) -> &[PrinterService] {
        &self.services
    }

    /// 按实例名取服务（大小写不敏感）。
    pub fn get(&self, instance: &str) -> Option<&PrinterService> {
        match self.index_of(instance) {
            Some(i) => self.services.get(i),
            None => None,
        }
    }

    /// 某类型的全部实例（不做完整性过滤，半成品也在内——完整性看 `complete_count`）。
    pub fn discover(&self, kind: ServiceType) -> Vec<&PrinterService> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.services.len() {
            if let Some(s) = self.services.get(i) {
                if s.kind == kind {
                    out.push(s);
                }
            }
            i += 1;
        }
        out
    }

    /// 某类型四件套齐全的条数。
    pub fn complete_count(&self, kind: ServiceType) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.services.len() {
            if let Some(s) = self.services.get(i) {
                if s.kind == kind && s.complete() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 半成品条数（缺主机/端口/地址/属性任一项）。必须显性可见，不能算作发现。
    pub fn residue(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.services.len() {
            if let Some(s) = self.services.get(i) {
                if !s.complete() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn index_of(&self, instance: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.services.len() {
            if let Some(s) = self.services.get(i) {
                if ascii_ieq(&s.instance, instance) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn upsert(
        &mut self,
        instance: &str,
        kind: ServiceType,
        ttl: u32,
        now_ms: u64,
    ) -> Result<(), MdnsError> {
        if let Some(idx) = self.index_of(instance) {
            let existing = match self.services.get(idx) {
                Some(s) => Some(s.kind),
                None => None,
            };
            if existing != Some(kind) {
                return Err(MdnsError::Kind);
            }
            if let Some(s) = self.services.get_mut(idx) {
                s.ttl = ttl;
                s.seen_ms = now_ms;
            }
            return Ok(());
        }
        self.services.push(PrinterService {
            instance: instance.to_string(),
            kind,
            host: String::new(),
            port: 0,
            addr: None,
            txt: Vec::new(),
            ttl,
            seen_ms: now_ms,
        });
        Ok(())
    }

    fn remove(&mut self, instance: &str) -> usize {
        match self.index_of(instance) {
            Some(idx) => {
                self.services.remove(idx);
                1
            }
            None => 0,
        }
    }

    /// 吃掉一条报文。TTL=0 视为 goodbye（摘除），其余按类型更新。
    pub fn apply(&mut self, msg: &Message, now_ms: u64) -> Result<usize, MdnsError> {
        if msg.records.is_empty() {
            return Err(MdnsError::Empty);
        }
        let mut applied = 0usize;
        let mut i = 0usize;
        while i < msg.records.len() {
            let rec = match msg.records.get(i) {
                Some(r) => r,
                None => break,
            };
            match &rec.data {
                RData::Ptr(target) => {
                    if let Some(kind) = ServiceType::from_service_name(&rec.name) {
                        if rec.ttl == 0 {
                            self.remove(target);
                        } else {
                            self.upsert(target, kind, rec.ttl, now_ms)?;
                        }
                        applied += 1;
                    }
                }
                RData::Srv { port, target, .. } => {
                    if *port == 0 {
                        return Err(MdnsError::Port);
                    }
                    if rec.ttl == 0 {
                        self.remove(&rec.name);
                    } else if let Some(idx) = self.index_of(&rec.name) {
                        if let Some(s) = self.services.get_mut(idx) {
                            s.host = target.clone();
                            s.port = *port;
                            s.ttl = rec.ttl;
                            s.seen_ms = now_ms;
                        }
                    }
                    applied += 1;
                }
                RData::Txt(pairs) => {
                    if rec.ttl == 0 {
                        self.remove(&rec.name);
                    } else if let Some(idx) = self.index_of(&rec.name) {
                        if let Some(s) = self.services.get_mut(idx) {
                            s.txt = pairs.clone();
                            s.ttl = rec.ttl;
                            s.seen_ms = now_ms;
                        }
                    }
                    applied += 1;
                }
                RData::Addr(ip) => {
                    let mut k = 0usize;
                    while k < self.services.len() {
                        let is_host = match self.services.get(k) {
                            Some(s) => ascii_ieq(&s.host, &rec.name),
                            None => false,
                        };
                        if is_host {
                            if let Some(s) = self.services.get_mut(k) {
                                if rec.ttl == 0 {
                                    s.addr = None;
                                } else {
                                    s.addr = Some(*ip);
                                }
                                s.seen_ms = now_ms;
                            }
                        }
                        k += 1;
                    }
                    applied += 1;
                }
                RData::Aaaa(_) => {
                    // 双栈部署里 AAAA 常见；发现逻辑只落 A，AAAA 计数但不进地址槽。
                    applied += 1;
                }
            }
            i += 1;
        }
        self.applied = self.applied.saturating_add(applied as u32);
        Ok(applied)
    }

    /// 老化：删除 `now >= seen + ttl` 的服务，返回摘除条数。
    pub fn expire(&mut self, now_ms: u64) -> usize {
        let mut removed = 0usize;
        let mut i = 0usize;
        while i < self.services.len() {
            let due = match self.services.get(i) {
                Some(s) => now_ms >= s.seen_ms.saturating_add(s.ttl as u64),
                None => false,
            };
            if due {
                self.services.remove(i);
                removed += 1;
            } else {
                i += 1;
            }
        }
        removed
    }

    /// 表内容摘要（空表恒为 FNV 初值——「摘除后回到初值」是可断言的性质）。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.services.len() {
            if let Some(s) = self.services.get(i) {
                let bytes = s.instance.as_bytes();
                let mut k = 0usize;
                while k < bytes.len() {
                    if let Some(b) = bytes.get(k) {
                        h = fnv_u8(h, *b);
                    }
                    k += 1;
                }
                h = fnv_u8(h, s.kind.code());
                h = fnv_u16(h, s.port);
                h = fnv_u32(h, s.ttl);
                h = h.wrapping_mul(FNV_PRIME);
            }
            i += 1;
        }
        h
    }

    /// 一行渲染（诊断用）。
    pub fn render(&self) -> String {
        format!(
            "registry n={} residue={} applied={} digest={:016x}",
            self.services.len(),
            self.residue(),
            self.applied,
            self.digest()
        )
    }
}

// ---------------------------------------------------------------------------
// 模拟器
// ---------------------------------------------------------------------------

/// 一台模拟打印服务。
#[derive(Clone, Debug)]
pub struct SimPrinter {
    pub instance: String,
    pub kind: ServiceType,
    pub host: String,
    pub port: u16,
    pub ip: [u8; 4],
    pub txt: Vec<(String, String)>,
}

fn txt_pair(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

/// 三台模拟器服务：两台 IPP、一台 1284/raw。
pub fn simulator_fleet() -> Vec<SimPrinter> {
    let mut out = Vec::with_capacity(3);
    let mut t0 = Vec::new();
    t0.push(txt_pair("rp", "ipp/print"));
    t0.push(txt_pair("ty", "Lab LaserJet"));
    out.push(SimPrinter {
        instance: "Lab IPP._ipp._tcp.local".to_string(),
        kind: ServiceType::Ipp,
        host: "lab-printer.local".to_string(),
        port: 631,
        ip: [192, 168, 1, 20],
        txt: t0,
    });
    let mut t1 = Vec::new();
    t1.push(txt_pair("rp", "raw"));
    t1.push(txt_pair("note", "ieee 1284"));
    out.push(SimPrinter {
        instance: "Office 1284._printer._tcp.local".to_string(),
        kind: ServiceType::Printer,
        host: "office-1284.local".to_string(),
        port: 515,
        ip: [192, 168, 1, 21],
        txt: t1,
    });
    let mut t2 = Vec::new();
    t2.push(txt_pair("rp", "ipp/print"));
    out.push(SimPrinter {
        instance: "BulkJet._ipp._tcp.local".to_string(),
        kind: ServiceType::Ipp,
        host: "bulkjet.local".to_string(),
        port: 631,
        ip: [192, 168, 1, 22],
        txt: t2,
    });
    out
}

/// 为整批服务编码一份公告（PTR+SRV+TXT+A）。`compress` 控制是否用名字压缩。
pub fn build_fleet(
    fleet: &[SimPrinter],
    ttl: u32,
    compress: bool,
) -> Result<Vec<u8>, MdnsError> {
    let mut w = if compress {
        MessageWriter::new(0x5014)
    } else {
        MessageWriter::plain(0x5014)
    };
    let mut i = 0usize;
    while i < fleet.len() {
        let p = match fleet.get(i) {
            Some(v) => v,
            None => break,
        };
        w.answer_ptr(p.kind, &p.instance, ttl)?;
        w.answer_srv(&p.instance, &p.host, p.port, ttl)?;
        let mut pairs: Vec<(&str, &str)> = Vec::with_capacity(p.txt.len());
        let mut k = 0usize;
        while k < p.txt.len() {
            if let Some((a, b)) = p.txt.get(k) {
                pairs.push((a.as_str(), b.as_str()));
            }
            k += 1;
        }
        w.answer_txt(&p.instance, &pairs, ttl)?;
        w.answer_a(&p.host, p.ip, ttl)?;
        i += 1;
    }
    Ok(w.finish())
}

/// 正常公告（TTL=120）。
pub fn announce_fleet(fleet: &[SimPrinter]) -> Result<Vec<u8>, MdnsError> {
    build_fleet(fleet, 120, true)
}

/// 离线公告（TTL=0，goodbye）。
pub fn goodbye_fleet(fleet: &[SimPrinter]) -> Result<Vec<u8>, MdnsError> {
    build_fleet(fleet, 0, true)
}

/// 编一条 PTR 查询。
pub fn query(kind: ServiceType) -> Result<Vec<u8>, MdnsError> {
    let mut w = MessageWriter::new(0x2701);
    w.question(kind)?;
    Ok(w.finish())
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn raw_header(id: u16, qd: u16, an: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.push((id >> 8) as u8);
    v.push(id as u8);
    v.push(0);
    v.push(0);
    v.push((qd >> 8) as u8);
    v.push(qd as u8);
    v.push((an >> 8) as u8);
    v.push(an as u8);
    let mut i = 0usize;
    while i < 4 {
        v.push(0u8);
        i += 1;
    }
    v
}

fn push_be16(v: &mut Vec<u8>, x: u16) {
    v.push((x >> 8) as u8);
    v.push(x as u8);
}

fn push_be32(v: &mut Vec<u8>, x: u32) {
    v.push((x >> 24) as u8);
    v.push((x >> 16) as u8);
    v.push((x >> 8) as u8);
    v.push(x as u8);
}

fn push_rr(v: &mut Vec<u8>, rtype: u16, class: u16, ttl: u32, rdlen: u16) {
    push_be16(v, rtype);
    push_be16(v, class);
    push_be32(v, ttl);
    push_be16(v, rdlen);
}

/// 注入核：十二类坏报文，逐类断言专属码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 1 空缓冲
    if parse_message(&[]).err() == Some(MdnsError::Short) {
        caught += 1;
    }

    // 2 非法标签类型位（0x80）
    let mut p2 = raw_header(1, 1, 0);
    p2.push(0x80);
    if parse_message(&p2).err() == Some(MdnsError::Label) {
        caught += 1;
    }

    // 3 正向压缩指针（自环）
    let mut p3 = raw_header(1, 1, 0);
    p3.push(0xc0);
    p3.push(12);
    if parse_message(&p3).err() == Some(MdnsError::Pointer) {
        caught += 1;
    }

    // 4 未支持的类型
    let mut p4 = raw_header(1, 0, 1);
    p4.push(0);
    push_rr(&mut p4, 99, 1, 0, 0);
    if parse_message(&p4).err() == Some(MdnsError::Type) {
        caught += 1;
    }

    // 5 非 IN 类
    let mut p5 = raw_header(1, 0, 1);
    p5.push(0);
    push_rr(&mut p5, 12, 2, 0, 0);
    if parse_message(&p5).err() == Some(MdnsError::Class) {
        caught += 1;
    }

    // 6 rdata 越界（声明 100 字节，一个字节都没有）
    let mut p6 = raw_header(1, 0, 1);
    p6.push(0);
    push_rr(&mut p6, 12, 1, 0, 100);
    if parse_message(&p6).err() == Some(MdnsError::Rdata) {
        caught += 1;
    }

    // 7 计数超限
    let p7 = raw_header(1, 0, 200);
    if parse_message(&p7).err() == Some(MdnsError::Count) {
        caught += 1;
    }

    // 8 SRV 端口 0
    let mut w8 = MessageWriter::new(0x1);
    if w8
        .write_srv_unchecked("x._ipp._tcp.local", "h.local", 0, 120)
        .is_ok()
    {
        let bytes = w8.finish();
        if let Ok(msg) = parse_message(&bytes) {
            let mut reg = PrinterRegistry::new();
            if reg.apply(&msg, 0).err() == Some(MdnsError::Port) {
                caught += 1;
            }
        }
    }

    // 9 名字超 255 字节（5 × 60 字符标签）
    let mut p9 = raw_header(1, 1, 0);
    let mut k = 0usize;
    while k < 5 {
        p9.push(60u8);
        let mut j = 0usize;
        while j < 60 {
            p9.push(b'a' + (k as u8));
            j += 1;
        }
        k += 1;
    }
    p9.push(0);
    if parse_message(&p9).err() == Some(MdnsError::Name) {
        caught += 1;
    }

    // 10 TXT 空键（"=xy"）
    let mut p10 = raw_header(1, 0, 1);
    p10.push(0);
    push_rr(&mut p10, 16, 1, 120, 4);
    p10.push(3);
    p10.push(b'=');
    p10.push(b'x');
    p10.push(b'y');
    if parse_message(&p10).err() == Some(MdnsError::Txt) {
        caught += 1;
    }

    // 11 同一实例挂在两个服务类型下
    let mut w11 = MessageWriter::new(0x2);
    let ok11 = w11.answer_ptr(ServiceType::Ipp, "Dual._ipp._tcp.local", 120).is_ok()
        && w11
            .answer_ptr(ServiceType::Printer, "Dual._ipp._tcp.local", 120)
            .is_ok();
    if ok11 {
        let bytes = w11.finish();
        if let Ok(msg) = parse_message(&bytes) {
            let mut reg = PrinterRegistry::new();
            if reg.apply(&msg, 0).err() == Some(MdnsError::Kind) {
                caught += 1;
            }
        }
    }

    // 12 无记录报文
    let p12 = raw_header(1, 0, 0);
    if let Ok(msg) = parse_message(&p12) {
        let mut reg = PrinterRegistry::new();
        if reg.apply(&msg, 0).err() == Some(MdnsError::Empty) {
            caught += 1;
        }
    }

    (caught, total)
}

/// 判据集（14 条）。
pub fn run_unx50141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50141");

    // J1-1 服务类型表：双型、大小写不敏感、码值闭集
    let ok = ServiceType::Ipp.service_name() == "_ipp._tcp.local"
        && ServiceType::Printer.service_name() == "_printer._tcp.local"
        && ServiceType::from_service_name("_IPP._TCP.LOCAL") == Some(ServiceType::Ipp)
        && ServiceType::from_service_name("_printer._tcp.local") == Some(ServiceType::Printer)
        && ServiceType::from_service_name("_http._tcp.local").is_none()
        && ServiceType::from_code(0) == Ok(ServiceType::Ipp)
        && ServiceType::from_code(1) == Ok(ServiceType::Printer)
        && ServiceType::from_code(2).err() == Some(MdnsError::Type)
        && ServiceType::Ipp.port_hint() == 631
        && ServiceType::Printer.port_hint() == 515;
    s.add("50141-J1-类型表", ok, if ok { "" } else { "服务类型表漂移" });

    // J1-2 查询往返：一条问题、零记录、id 保留
    let ok = match query(ServiceType::Printer) {
        Ok(bytes) => match parse_message(&bytes) {
            Ok(msg) => msg.id == 0x2701 && msg.questions == 1 && msg.records.is_empty(),
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50141-J1-查询往返", ok, if ok { "" } else { "查询报文不自洽" });

    // J1-3 公告解析：12 条记录，四类各 3 条，实例名逐一对上
    let fleet = simulator_fleet();
    let mut ok = false;
    let mut det = String::new();
    if let Ok(bytes) = announce_fleet(&fleet) {
        if let Ok(msg) = parse_message(&bytes) {
            let mut ptr = 0usize;
            let mut srv = 0usize;
            let mut txt = 0usize;
            let mut a = 0usize;
            let mut names_ok = true;
            let mut i = 0usize;
            while i < msg.records.len() {
                if let Some(r) = msg.records.get(i) {
                    match &r.data {
                        RData::Ptr(t) => {
                            ptr += 1;
                            let mut found = false;
                            let mut k = 0usize;
                            while k < fleet.len() {
                                if let Some(p) = fleet.get(k) {
                                    if ascii_ieq(&p.instance, t) {
                                        found = true;
                                    }
                                }
                                k += 1;
                            }
                            if !found {
                                names_ok = false;
                            }
                        }
                        RData::Srv { port, .. } => {
                            srv += 1;
                            if *port == 0 {
                                names_ok = false;
                            }
                        }
                        RData::Txt(_) => txt += 1,
                        RData::Addr(_) => a += 1,
                        RData::Aaaa(_) => {}
                    }
                }
                i += 1;
            }
            ok = msg.records.len() == 12 && ptr == 3 && srv == 3 && txt == 3 && a == 3 && names_ok;
            det = format!("rec={} ptr={} srv={} txt={} a={}", msg.records.len(), ptr, srv, txt, a);
        }
    }
    s.add_dyn("50141-J1-公告解析", ok, &det);

    // J1-4 名字压缩：压缩后更短、解析出的实例与原名逐一对上
    let mut ok = false;
    let mut det = String::new();
    if let (Ok(c), Ok(p)) = (build_fleet(&fleet, 120, true), build_fleet(&fleet, 120, false)) {
        let cp = parse_message(&c);
        let pp = parse_message(&p);
        if let (Ok(cm), Ok(pm)) = (cp, pp) {
            let same_records = cm.records.len() == pm.records.len() && cm.records.len() == 12;
            let first_ptr_c = match (cm.records.first(), fleet.first()) {
                (Some(r), Some(f)) => match &r.data {
                    RData::Ptr(t) => ascii_ieq(t, &f.instance),
                    _ => false,
                },
                _ => false,
            };
            ok = c.len() < p.len() && same_records && first_ptr_c;
            det = format!("compressed={} plain={}", c.len(), p.len());
        }
    }
    s.add_dyn("50141-J1-名字压缩", ok, &det);

    // J1-5 双型发现零漏报：2 IPP + 1 Printer，四件套齐、无半成品
    let mut ok = false;
    let mut det = String::new();
    let mut reg = PrinterRegistry::new();
    if let Ok(bytes) = announce_fleet(&fleet) {
        if let Ok(msg) = parse_message(&bytes) {
            if let Ok(n) = reg.apply(&msg, 0) {
                ok = n == 12
                    && reg.len() == 3
                    && reg.complete_count(ServiceType::Ipp) == 2
                    && reg.complete_count(ServiceType::Printer) == 1
                    && reg.residue() == 0
                    && reg.discover(ServiceType::Ipp).len() == 2
                    && reg.discover(ServiceType::Printer).len() == 1;
                det = reg.render();
            }
        }
    }
    s.add_dyn("50141-J1-双型零漏报", ok, &det);

    // J1-6 半成品显性：只有 PTR 时不冒充「已发现」
    let mut ok = false;
    let mut det = String::new();
    if let Ok(mut w) = Ok::<MessageWriter, MdnsError>(MessageWriter::new(0x3)) {
        let mut built = true;
        let mut i = 0usize;
        while i < fleet.len() {
            if let Some(p) = fleet.get(i) {
                if w.answer_ptr(p.kind, &p.instance, 120).is_err() {
                    built = false;
                }
            }
            i += 1;
        }
        if built {
            let bytes = w.finish();
            if let Ok(msg) = parse_message(&bytes) {
                let mut r2 = PrinterRegistry::new();
                if let Ok(_n) = r2.apply(&msg, 0) {
                    ok = r2.len() == 3
                        && r2.residue() == 3
                        && r2.complete_count(ServiceType::Ipp) == 0
                        && r2.complete_count(ServiceType::Printer) == 0;
                    det = r2.render();
                }
            }
        }
    }
    s.add_dyn("50141-J1-半成品显性", ok, &det);

    // J1-7 重复公告幂等：两次公告不增条、不改端口、不改摘要
    let mut ok = false;
    if let Ok(bytes) = announce_fleet(&fleet) {
        if let Ok(msg) = parse_message(&bytes) {
            let mut r3 = PrinterRegistry::new();
            let first = r3.apply(&msg, 0);
            let d1 = r3.digest();
            let second = r3.apply(&msg, 0);
            let d2 = r3.digest();
            let port_ok = match fleet.first() {
                Some(f) => match r3.get(&f.instance) {
                    Some(p) => p.port == 631,
                    None => false,
                },
                None => false,
            };
            ok = first.is_ok() && second.is_ok() && r3.len() == 3 && d1 == d2 && port_ok;
        }
    }
    s.add("50141-J1-公告幂等", ok, if ok { "" } else { "重复公告产生副本" });

    // J1-8 离线摘除零残留：goodbye 后空表、摘要回初值、二次 goodbye 不炸
    let mut ok = false;
    let mut det = String::new();
    let empty_digest = PrinterRegistry::new().digest();
    if let (Ok(ann), Ok(bye)) = (announce_fleet(&fleet), goodbye_fleet(&fleet)) {
        let am = parse_message(&ann);
        let bm = parse_message(&bye);
        if let (Ok(a), Ok(b)) = (am, bm) {
            let mut r4 = PrinterRegistry::new();
            if r4.apply(&a, 0).is_ok() {
                let before = r4.len();
                let removed = r4.apply(&b, 0);
                let again = r4.apply(&b, 0);
                ok = before == 3
                    && removed.is_ok()
                    && again.is_ok()
                    && r4.is_empty()
                    && r4.residue() == 0
                    && r4.complete_count(ServiceType::Ipp) == 0
                    && r4.complete_count(ServiceType::Printer) == 0
                    && r4.digest() == empty_digest;
                det = format!("before={} after={} residue={}", before, r4.len(), r4.residue());
            }
        }
    }
    s.add_dyn("50141-J1-摘除零残留", ok, &det);

    // J1-9 TTL 老化：119 未到期、120 到期全清
    let mut ok = false;
    let mut det = String::new();
    if let Ok(ann) = announce_fleet(&fleet) {
        if let Ok(msg) = parse_message(&ann) {
            let mut r5 = PrinterRegistry::new();
            if r5.apply(&msg, 0).is_ok() {
                let at119 = r5.expire(119);
                let at120 = r5.expire(120);
                ok = at119 == 0 && at120 == 3 && r5.is_empty();
                det = format!("t119={} t120={}", at119, at120);
            }
        }
    }
    s.add_dyn("50141-J1-TTL老化", ok, &det);

    // J1-10 单机摘除不误伤
    let mut ok = false;
    let mut one: Vec<SimPrinter> = Vec::new();
    if let Some(p) = fleet.get(1) {
        one.push(p.clone());
    }
    if let (Ok(ann), Ok(bye)) = (announce_fleet(&fleet), goodbye_fleet(&one)) {
        let am = parse_message(&ann);
        let bm = parse_message(&bye);
        if let (Ok(a), Ok(b)) = (am, bm) {
            let mut r6 = PrinterRegistry::new();
            if r6.apply(&a, 0).is_ok() && r6.apply(&b, 0).is_ok() {
                let survivor_a = match fleet.first() {
                    Some(f) => r6.get(&f.instance).is_some(),
                    None => false,
                };
                let survivor_b = match fleet.get(2) {
                    Some(f) => r6.get(&f.instance).is_some(),
                    None => false,
                };
                ok = r6.len() == 2
                    && survivor_a
                    && survivor_b
                    && r6.complete_count(ServiceType::Printer) == 0
                    && r6.complete_count(ServiceType::Ipp) == 2
                    && r6.residue() == 0;
            }
        }
    }
    s.add("50141-J1-单机摘除", ok, if ok { "" } else { "摘除误伤邻机" });

    // J1-11 端口 0：编码侧拒、解码侧拒
    let mut w = MessageWriter::new(0x4);
    let enc = w.answer_srv("x._ipp._tcp.local", "h.local", 0, 120);
    let mut raw = raw_header(1, 0, 1);
    raw.push(0);
    push_rr(&mut raw, 33, 1, 120, 7);
    push_be16(&mut raw, 0);
    push_be16(&mut raw, 0);
    push_be16(&mut raw, 0);
    raw.push(0);
    let dec = match parse_message(&raw) {
        Ok(msg) => {
            let mut r7 = PrinterRegistry::new();
            r7.apply(&msg, 0).err() == Some(MdnsError::Port)
        }
        Err(_) => false,
    };
    let ok = enc.err() == Some(MdnsError::Port) && dec;
    s.add("50141-J1-端口零拒", ok, if ok { "" } else { "端口 0 被收下" });

    // J1-12 TXT 编码边界：单串 255 可写、256 拒（前缀 "k=" 占 2 字节）
    let long255 = {
        let mut v = String::new();
        let mut i = 0usize;
        while i < 253 {
            v.push('a');
            i += 1;
        }
        v
    };
    let mut w12 = MessageWriter::new(0x5);
    let ok255 = w12.answer_txt("x._ipp._tcp.local", &[("k", &long255)], 120).is_ok();
    let mut long256 = long255.clone();
    long256.push('b');
    let mut w12b = MessageWriter::new(0x6);
    let too_long = w12b.answer_txt("x._ipp._tcp.local", &[("k", &long256)], 120);
    let ok = ok255 && too_long.err() == Some(MdnsError::Txt);
    s.add("50141-J1-TXT边界", ok, if ok { "" } else { "TXT 长串边界未守" });

    // J1-13 老化与摘除后摘要恒等：两条路径都回到空表初值
    let mut ok = false;
    if let Ok(ann) = announce_fleet(&fleet) {
        if let Ok(msg) = parse_message(&ann) {
            let mut r8 = PrinterRegistry::new();
            let mut r9 = PrinterRegistry::new();
            let _ = r8.apply(&msg, 0);
            let _ = r9.apply(&msg, 0);
            let _ = r8.expire(1_000);
            if let Ok(bye) = goodbye_fleet(&fleet) {
                if let Ok(bm) = parse_message(&bye) {
                    let _ = r9.apply(&bm, 0);
                }
            }
            ok = r8.digest() == r9.digest() && r8.is_empty() && r9.is_empty();
        }
    }
    s.add("50141-J1-摘要归位", ok, if ok { "" } else { "两条摘除路径摘要不一致" });

    // J1-14 注入核
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50141-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50141_checks_green() {
        let set = run_unx50141_checks();
        assert!(set.all_passed(), "UNX-F50141 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50141_discovery_is_complete() {
        let fleet = simulator_fleet();
        let ann = announce_fleet(&fleet);
        assert!(ann.is_ok());
        let bytes = match ann {
            Ok(v) => v,
            Err(_) => return,
        };
        let msg = parse_message(&bytes);
        assert!(msg.is_ok());
        if let Ok(m) = msg {
            let mut reg = PrinterRegistry::new();
            assert!(reg.apply(&m, 0).is_ok());
            assert_eq!(reg.complete_count(ServiceType::Ipp), 2);
            assert_eq!(reg.complete_count(ServiceType::Printer), 1);
            assert_eq!(reg.residue(), 0);
        }
    }

    #[test]
    fn unx50141_goodbye_leaves_no_residue() {
        let fleet = simulator_fleet();
        let mut reg = PrinterRegistry::new();
        if let Ok(ann) = announce_fleet(&fleet) {
            if let Ok(m) = parse_message(&ann) {
                let _ = reg.apply(&m, 0);
            }
        }
        if let Ok(bye) = goodbye_fleet(&fleet) {
            if let Ok(m) = parse_message(&bye) {
                let _ = reg.apply(&m, 0);
            }
        }
        assert!(reg.is_empty());
        assert_eq!(reg.residue(), 0);
        assert_eq!(reg.digest(), PrinterRegistry::new().digest());
    }
}
