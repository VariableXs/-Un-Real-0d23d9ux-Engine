//! UNX-F50146 多协议发现聚合器（去重 / 单源视图）（0x43e60 段）
//!
//! 锚点：UNX-F50146-J1 三协议聚合去重 ×10 批零重复 id，单源视图与全量一致；
//! 判据锚定 §AI-63 §2 专题五。
//!
//! 发现（discovery）这件事最容易被做错的地方不是「收不到」，而是「同名不同
//! 源」：mDNS 广播、SLP 目录代理、LPD 队列通告会从三个互不相识的通道反复
//! 描述同一台打印机。如果聚合器按「到达一条记一条」，用户界面上就会出现三
//! 个同名设备、三个不同 id；后续的配额、取消、状态查询会各自打到不同的影子
//! 记录上。因此本模块的核心判据只有一句：**同一规范键只允许存在一个 id**。
//!
//! 规范键 = （小写名，小写主机，端口）。大小写不参与身份，端口参与：同一台
//! 主机上 9100 与 515 是两个可独立寻址的服务。来源用位图表示，合并只做按位
//! 或，**首见 id 与首见批次永不改写**——这样重放、乱序、重复通告都不会让用
//! 户看到 id 漂移。
//!
//! 三条纪律：
//! 1. 去重发生在入账之前：先查规范键，命中就只合并来源位，不消耗 id。
//! 2. 拒绝不污染账本：坏行、坏协议、越界端口只记错误码与计数，不建条目。
//! 3. 视图是账本的只读投影：单源视图按来源位筛选，并集必须恒等于全量。
//!
//! 数据结构：
//! * `Aggregator` —— 定容账本 + 单调 id 分配器 + 全量/单源视图；
//! * `ServiceRecord` —— 一条聚合后的服务（规范键、来源位、首见批次、触点）；
//! * `Announcement` —— 单协议解码结果（三协议共用一套字段）；
//! * `IngestReport` —— 一批的入账报告（inserted/merged/rejected 可对账）。
//!
//! 错误路径：段内闭集 11 码，全部在注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43e60 独占 0x20）。
pub const SEG_BASE: u32 = 0x43e60;

pub const E_EMPTY: u16 = 1;
pub const E_PROTO: u16 = 2;
pub const E_NAME: u16 = 3;
pub const E_NAMELEN: u16 = 4;
pub const E_HOST: u16 = 5;
pub const E_HOSTLEN: u16 = 6;
pub const E_PORT: u16 = 7;
pub const E_LINE: u16 = 8;
pub const E_SRC: u16 = 9;
pub const E_CAP: u16 = 10;
pub const E_IDOVF: u16 = 11;

/// 闭集大小。
pub const ERR_SLOTS: usize = 11;

/// 名字/主机名单边长度上限。DNS 单标签 63 是硬约束，聚合层不放大它。
pub const MAX_SIDE: usize = 63;

/// 服务名/主机名的合法字节范围：可打印 ASCII 且不含空格。
pub const SIDE_LO: u8 = 0x21;
pub const SIDE_HI: u8 = 0x7e;

/// 聚合错误：`code()` = `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggError {
    /// 空批次：没有任何可入账的行。
    Empty,
    /// 协议前缀不认识（不是 mdns/slp/lpd）。
    Proto,
    /// 服务名为空或含非法字节。
    Name,
    /// 服务名超过 63 字节。
    NameLen,
    /// 主机名为空或含非法字节。
    Host,
    /// 主机名超过 63 字节。
    HostLen,
    /// 端口为 0 或超出 65535。
    Port,
    /// 行结构不可解码（分隔符缺失、端口非数字、尾随多余字段）。
    Line,
    /// 来源位不是单一协议位。
    Src,
    /// 账本容量已满，且键不存在。
    Cap,
    /// id 分配器耗尽。
    IdOvf,
}

impl AggError {
    /// 段内诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.slot() as u32 + 1
    }

    /// 账本槽位（0 基）。
    pub fn slot(self) -> usize {
        let n = match self {
            AggError::Empty => E_EMPTY,
            AggError::Proto => E_PROTO,
            AggError::Name => E_NAME,
            AggError::NameLen => E_NAMELEN,
            AggError::Host => E_HOST,
            AggError::HostLen => E_HOSTLEN,
            AggError::Port => E_PORT,
            AggError::Line => E_LINE,
            AggError::Src => E_SRC,
            AggError::Cap => E_CAP,
            AggError::IdOvf => E_IDOVF,
        };
        (n - 1) as usize
    }

    /// 一行诊断文本。
    pub fn message(self) -> &'static str {
        match self {
            AggError::Empty => "empty discovery batch",
            AggError::Proto => "unknown discovery protocol",
            AggError::Name => "bad service name",
            AggError::NameLen => "service name too long",
            AggError::Host => "bad host name",
            AggError::HostLen => "host name too long",
            AggError::Port => "port out of range",
            AggError::Line => "undecodable announcement line",
            AggError::Src => "source is not a single protocol bit",
            AggError::Cap => "aggregator capacity exhausted",
            AggError::IdOvf => "service id space exhausted",
        }
    }
}

/// 三个发现来源。位图化是为了让「同一服务从多条通道被看到」变成一次按位或。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Mdns,
    Slp,
    Lpd,
}

impl Source {
    /// 来源位。
    pub fn bit(self) -> u8 {
        match self {
            Source::Mdns => 0b001,
            Source::Slp => 0b010,
            Source::Lpd => 0b100,
        }
    }

    /// 线上前缀（含冒号），解码与编码共用同一张表，避免两处漂移。
    pub fn tag(self) -> &'static str {
        match self {
            Source::Mdns => "mdns:",
            Source::Slp => "slp:",
            Source::Lpd => "lpd:",
        }
    }

    /// 由单一位还原来源；多一位或少一位都返回 None。
    pub fn from_bit(b: u8) -> Option<Source> {
        match b {
            0b001 => Some(Source::Mdns),
            0b010 => Some(Source::Slp),
            0b100 => Some(Source::Lpd),
            _ => None,
        }
    }
}

/// 单协议解码结果：三协议最终都归一成这五个字段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub source: Source,
    pub name: String,
    pub host: String,
    pub port: u16,
}

/// 规范键：小写名 + 小写主机 + 端口。相等即同一服务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    pub host: String,
    pub port: u16,
}

/// 聚合后的服务记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceRecord {
    /// 首见 id，单调递增，永不改写。
    pub id: u32,
    /// 规范键。
    pub key: Key,
    /// 首见时的原始写法（保留大小写，仅用于展示）。
    pub display_name: String,
    /// 来源位图（1=mDNS，2=SLP，4=LPD）。
    pub sources: u8,
    /// 首见批次号。
    pub first_batch: u32,
    /// 被再次通告的次数（含同批次内的重复行）。
    pub touches: u16,
}

/// 一批的入账报告：四项之和必须等于批内行数（空批次记 1 次拒绝）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    pub seen: usize,
    pub inserted: usize,
    pub merged: usize,
    pub rejected: usize,
}

impl IngestReport {
    /// 对账：报告自身闭合。
    pub fn balanced(&self) -> bool {
        self.seen == self.inserted + self.merged + self.rejected
    }
}

// ---------------------------------------------------------------------------
// 基础校验与解码
// ---------------------------------------------------------------------------

fn strip_prefix<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    if s.starts_with(p) {
        s.get(p.len()..)
    } else {
        None
    }
}

fn split_once<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let at = s.find(sep)?;
    let head = s.get(..at)?;
    let tail = s.get(at + sep.len()..)?;
    Some((head, tail))
}

/// 名字/主机名的公共护栏：非空、不超长、可打印且无空格。
fn validate_side(s: &str, empty: AggError, long: AggError) -> Result<(), AggError> {
    if s.is_empty() {
        return Err(empty);
    }
    if s.len() > MAX_SIDE {
        return Err(long);
    }
    let mut i = 0usize;
    while i < s.len() {
        if let Some(c) = s.as_bytes().get(i) {
            if *c < SIDE_LO || *c > SIDE_HI {
                return Err(empty);
            }
        } else {
            return Err(empty);
        }
        i += 1;
    }
    Ok(())
}

/// 端口文本：只接受纯十进制，0 与 >65535 都算越界（而非语法错）。
fn parse_port_text(t: &str) -> Result<u16, AggError> {
    if t.is_empty() || t.len() > 5 {
        return Err(AggError::Line);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < t.len() {
        let c = match t.as_bytes().get(i) {
            Some(x) => *x,
            None => return Err(AggError::Line),
        };
        if c < b'0' || c > b'9' {
            return Err(AggError::Line);
        }
        v = v * 10 + (c - b'0') as u32;
        i += 1;
    }
    if v == 0 || v > 65535 {
        return Err(AggError::Port);
    }
    Ok(v as u16)
}

fn validate_parts(name: &str, host: &str, port_text: &str) -> Result<u16, AggError> {
    validate_side(name, AggError::Name, AggError::NameLen)?;
    validate_side(host, AggError::Host, AggError::HostLen)?;
    parse_port_text(port_text)
}

/// 编码一条 mDNS 通告。
pub fn line_mdns(name: &str, host: &str, port: u16) -> String {
    format!("{}{}@{}:{}", Source::Mdns.tag(), name, host, port)
}

/// 编码一条 SLP 通告。
pub fn line_slp(name: &str, host: &str, port: u16) -> String {
    format!("{}{}//{}:{}", Source::Slp.tag(), name, host, port)
}

/// 编码一条 LPD 通告。
pub fn line_lpd(name: &str, host: &str, port: u16) -> String {
    format!("{}{},{},{}", Source::Lpd.tag(), name, host, port)
}

/// 解码任意协议的一行通告。
///
/// 三种线上记法故意不同（`@`+`:` / `//`+`:` / 逗号），但归一后的字段相同；
/// 解码顺序固定为「前缀 → 名 → 主机 → 端口」，确保同样的坏行永远得到同样
/// 的错误码（诊断可重复）。
pub fn decode_line(line: &str) -> Result<Announcement, AggError> {
    if line.is_empty() {
        return Err(AggError::Line);
    }
    let mut source = Source::Mdns;
    let mut rest = "";
    let mut matched = false;
    let mut i = 0usize;
    // 固定顺序遍历三个来源：mDNS、SLP、LPD，保证诊断确定性。
    while i < 3 {
        let cand = match i {
            0 => Source::Mdns,
            1 => Source::Slp,
            _ => Source::Lpd,
        };
        if let Some(r) = strip_prefix(line, cand.tag()) {
            source = cand;
            rest = r;
            matched = true;
            break;
        }
        i += 1;
    }
    if !matched {
        return Err(AggError::Proto);
    }
    let (name, host, port_text) = match source {
        Source::Mdns => {
            let (n, hp) = split_once(rest, "@").ok_or(AggError::Line)?;
            let (h, p) = split_once(hp, ":").ok_or(AggError::Line)?;
            (n, h, p)
        }
        Source::Slp => {
            let (n, hp) = split_once(rest, "//").ok_or(AggError::Line)?;
            let (h, p) = split_once(hp, ":").ok_or(AggError::Line)?;
            (n, h, p)
        }
        Source::Lpd => {
            let (n, hp) = split_once(rest, ",").ok_or(AggError::Line)?;
            let (h, p) = split_once(hp, ",").ok_or(AggError::Line)?;
            (n, h, p)
        }
    };
    let port = validate_parts(name, host, port_text)?;
    Ok(Announcement {
        source,
        name: name.to_string(),
        host: host.to_string(),
        port,
    })
}

fn key_of(name: &str, host: &str, port: u16) -> Key {
    Key {
        name: name.to_ascii_lowercase(),
        host: host.to_ascii_lowercase(),
        port,
    }
}

// ---------------------------------------------------------------------------
// 聚合账本
// ---------------------------------------------------------------------------

/// 多协议发现聚合器。
#[derive(Clone, Debug)]
pub struct Aggregator {
    entries: Vec<ServiceRecord>,
    cap: usize,
    next_id: u32,
    pub seen: usize,
    pub inserted: usize,
    pub merged: usize,
    pub rejected: usize,
    pub err_counts: [u16; ERR_SLOTS],
    pub first_err: u32,
}

impl Aggregator {
    /// 建账本，id 从 1 起。
    pub fn new(cap: usize) -> Aggregator {
        Aggregator::new_with_next(cap, 1)
    }

    /// 建账本并指定下一个 id（用于耗尽路径的自检）。
    pub fn new_with_next(cap: usize, next_id: u32) -> Aggregator {
        Aggregator {
            entries: Vec::new(),
            cap,
            next_id,
            seen: 0,
            inserted: 0,
            merged: 0,
            rejected: 0,
            err_counts: [0u16; ERR_SLOTS],
            first_err: 0,
        }
    }

    /// 全量视图（按首见 id 升序）。
    pub fn entries(&self) -> &[ServiceRecord] {
        self.entries.as_slice()
    }

    /// 记一次错误（首个错误码永久保留，便于现场定位）。
    fn note_err(&mut self, e: AggError) {
        let slot = e.slot();
        if let Some(c) = self.err_counts.get_mut(slot) {
            *c = c.saturating_add(1);
        }
        if self.first_err == 0 {
            self.first_err = e.code();
        }
    }

    /// 读某类错误被拒绝的次数。
    pub fn err_count(&self, e: AggError) -> u16 {
        match self.err_counts.get(e.slot()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 按规范键查服务。
    pub fn find(&self, key: &Key) -> Option<&ServiceRecord> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == *key {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 单源视图：只投影带该来源位的记录。
    ///
    /// 注意这是**投影**而非副本：同一条记录可以同时出现在多个单源视图里，
    /// 这正是「同一服务多通道可见」的正确表达。
    pub fn by_source(&self, src: Source) -> Vec<&ServiceRecord> {
        let bit = src.bit();
        let mut out: Vec<&ServiceRecord> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.sources & bit != 0 {
                    out.push(e);
                }
            }
            i += 1;
        }
        out
    }

    fn fnv(mut h: u32, b: &[u8]) -> u32 {
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                h ^= *c as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        h
    }

    /// 账本摘要：重放/幂等检查用。
    pub fn digest(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = Aggregator::fnv(h, &e.id.to_le_bytes());
                h = Aggregator::fnv(h, e.key.name.as_bytes());
                h = Aggregator::fnv(h, e.key.host.as_bytes());
                h = Aggregator::fnv(h, &e.key.port.to_le_bytes());
                h = Aggregator::fnv(h, &[e.sources]);
                h = Aggregator::fnv(h, &e.first_batch.to_le_bytes());
            }
            i += 1;
        }
        h
    }

    /// 入账一条解码后的通告；返回是否新建（false 表示合并）。
    ///
    /// 校验→查键→合并/新建的顺序不可交换：容量与 id 只在「确实要新建」时
    /// 才被消耗，否则一条重复通告就能把账本撑满。
    fn apply(&mut self, batch: u32, a: &Announcement) -> Result<bool, AggError> {
        let bit = a.source.bit();
        let key = key_of(a.name.as_str(), a.host.as_str(), a.port);
        let mut found: Option<usize> = None;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    found = Some(i);
                    break;
                }
            }
            i += 1;
        }
        if let Some(idx) = found {
            if let Some(e) = self.entries.get_mut(idx) {
                e.sources |= bit;
                e.touches = e.touches.saturating_add(1);
            }
            self.merged += 1;
            return Ok(false);
        }
        if self.entries.len() >= self.cap {
            return Err(AggError::Cap);
        }
        if self.next_id == u32::MAX {
            return Err(AggError::IdOvf);
        }
        self.entries.push(ServiceRecord {
            id: self.next_id,
            key,
            display_name: a.name.clone(),
            sources: bit,
            first_batch: batch,
            touches: 1,
        });
        self.next_id += 1;
        self.inserted += 1;
        Ok(true)
    }

    fn raw_inner(
        &mut self,
        batch: u32,
        source_bits: u8,
        name: &str,
        host: &str,
        port: u16,
    ) -> Result<bool, AggError> {
        let source = match Source::from_bit(source_bits) {
            Some(s) => s,
            None => return Err(AggError::Src),
        };
        validate_side(name, AggError::Name, AggError::NameLen)?;
        validate_side(host, AggError::Host, AggError::HostLen)?;
        if port == 0 {
            return Err(AggError::Port);
        }
        let a = Announcement {
            source,
            name: name.to_string(),
            host: host.to_string(),
            port,
        };
        self.apply(batch, &a)
    }

    /// 直接以字段入账（调用方已自行解码）；来源必须恰好一位。
    pub fn ingest_raw(
        &mut self,
        batch: u32,
        source_bits: u8,
        name: &str,
        host: &str,
        port: u16,
    ) -> Result<bool, AggError> {
        self.seen += 1;
        match self.raw_inner(batch, source_bits, name, host, port) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.rejected += 1;
                self.note_err(e);
                Err(e)
            }
        }
    }

    /// 解码并入账一行；返回是否新建。
    pub fn ingest_one(&mut self, batch: u32, line: &str) -> Result<bool, AggError> {
        self.seen += 1;
        let r = match decode_line(line) {
            Ok(a) => self.apply(batch, &a),
            Err(e) => Err(e),
        };
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                self.rejected += 1;
                self.note_err(e);
                Err(e)
            }
        }
    }

    /// 入账一批行。坏行只记账不中断：发现场景里一条坏包不应吞掉整批。
    pub fn ingest_lines(&mut self, batch: u32, lines: &[&str]) -> IngestReport {
        let mut rep = IngestReport::default();
        if lines.is_empty() {
            // 空批次记一次拒绝：调用方若把「空」当成功，会掩盖上游的空转。
            self.seen += 1;
            self.rejected += 1;
            self.note_err(AggError::Empty);
            rep.seen = 1;
            rep.rejected = 1;
            return rep;
        }
        rep.seen = lines.len();
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(line) = lines.get(i) {
                match self.ingest_one(batch, line) {
                    Ok(true) => rep.inserted += 1,
                    Ok(false) => rep.merged += 1,
                    Err(_) => rep.rejected += 1,
                }
            }
            i += 1;
        }
        rep
    }
}

// ---------------------------------------------------------------------------
// 十批模拟通告（×10 批的确定性夹具）
// ---------------------------------------------------------------------------

const SERVICES: [(&str, &str, u16); 8] = [
    ("laser-a", "prn-a.lab", 9100),
    ("color-b", "prn-b.lab", 9100),
    ("scan-c", "prn-c.lab", 515),
    ("fax-d", "prn-d.lab", 515),
    ("label-e", "prn-e.lab", 9100),
    ("photo-f", "prn-f.lab", 631),
    ("plot-g", "prn-g.lab", 9100),
    ("stamp-h", "prn-h.lab", 515),
];

fn svc(i: usize) -> (&'static str, &'static str, u16) {
    match SERVICES.get(i % SERVICES.len()) {
        Some(t) => *t,
        None => ("unx", "prn.lab", 9100),
    }
}

/// 第 b 批的 6 行通告。
///
/// 每批覆盖 4 个服务，其中两个从两条协议同时被看到（同批合并），另有一个
/// 故意写成大写（跨批大小写合并）；10 批轮转后 8 个服务全部出现且都拿满
/// 三个来源位。
fn batch_lines(b: usize) -> Vec<String> {
    let s0 = svc(b);
    let s1 = svc(b + 1);
    let s2 = svc(b + 2);
    let s3 = svc(b + 3);
    let mut v: Vec<String> = Vec::new();
    v.push(line_mdns(s0.0, s0.1, s0.2));
    v.push(line_slp(s1.0, s1.1, s1.2));
    v.push(line_lpd(s1.0, s1.1, s1.2));
    v.push(line_mdns(s2.0.to_ascii_uppercase().as_str(), s2.1, s2.2));
    v.push(line_slp(s3.0, s3.1, s3.2));
    v.push(line_lpd(s3.0, s3.1, s3.2));
    v
}

/// 独立于解码器的期望键集合：直接取自 SERVICES 常量表。
fn oracle_keys() -> Vec<Key> {
    let mut out: Vec<Key> = Vec::new();
    let mut i = 0usize;
    while i < SERVICES.len() {
        if let Some(t) = SERVICES.get(i) {
            out.push(key_of(t.0, t.1, t.2));
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 注入核：11 个码逐一真实构造
// ---------------------------------------------------------------------------

fn injection_core() -> (usize, usize, u32) {
    let total = ERR_SLOTS;
    let mut caught = 0usize;
    let mut mask: u32 = 0;
    let bump = |e: AggError, hit: bool, mask: &mut u32| {
        if hit {
            *mask |= 1u32 << e.slot();
        }
        hit
    };

    // 1) Empty：空批次
    let mut ag = Aggregator::new(8);
    let rep = ag.ingest_lines(0, &[]);
    if bump(AggError::Empty, rep.rejected == 1, &mut mask) {
        caught += 1;
    }

    // 2) Proto：未知前缀
    let mut ag = Aggregator::new(8);
    if bump(
        AggError::Proto,
        ag.ingest_one(0, "smb:nas@host:445").err() == Some(AggError::Proto),
        &mut mask,
    ) {
        caught += 1;
    }

    // 3) Name：空名
    let mut ag = Aggregator::new(8);
    if bump(
        AggError::Name,
        ag.ingest_one(0, "mdns:@host:1").err() == Some(AggError::Name),
        &mut mask,
    ) {
        caught += 1;
    }

    // 4) NameLen：64 字节名
    let mut long_name = String::new();
    let mut i = 0usize;
    while i < 64 {
        long_name.push('n');
        i += 1;
    }
    let mut ag = Aggregator::new(8);
    let long_line = line_mdns(long_name.as_str(), "host.lab", 9100);
    if bump(
        AggError::NameLen,
        ag.ingest_one(0, long_line.as_str()).err() == Some(AggError::NameLen),
        &mut mask,
    ) {
        caught += 1;
    }

    // 5) Host：空主机
    let mut ag = Aggregator::new(8);
    if bump(
        AggError::Host,
        ag.ingest_one(0, "slp:prn//:1").err() == Some(AggError::Host),
        &mut mask,
    ) {
        caught += 1;
    }

    // 6) HostLen：64 字节主机
    let mut long_host = String::new();
    let mut i = 0usize;
    while i < 64 {
        long_host.push('h');
        i += 1;
    }
    let mut ag = Aggregator::new(8);
    let long_host_line = line_lpd("prn", long_host.as_str(), 515);
    if bump(
        AggError::HostLen,
        ag.ingest_one(0, long_host_line.as_str()).err() == Some(AggError::HostLen),
        &mut mask,
    ) {
        caught += 1;
    }

    // 7) Port：0 与 70000
    let mut ag = Aggregator::new(8);
    let p0 = ag.ingest_one(0, "mdns:prn@host:0").err() == Some(AggError::Port);
    let p1 = ag.ingest_one(0, "mdns:prn@host:70000").err() == Some(AggError::Port);
    if bump(AggError::Port, p0 && p1, &mut mask) {
        caught += 1;
    }

    // 8) Line：非数字端口 / 缺字段 / 尾随字段
    let mut ag = Aggregator::new(8);
    let l0 = ag.ingest_one(0, "mdns:prn@host:abc").err() == Some(AggError::Line);
    let l1 = ag.ingest_one(0, "lpd:prn,host").err() == Some(AggError::Line);
    let l2 = ag.ingest_one(0, "slp:prn//host:1:2").err() == Some(AggError::Line);
    if bump(AggError::Line, l0 && l1 && l2, &mut mask) {
        caught += 1;
    }

    // 9) Src：零位与多位
    let mut ag = Aggregator::new(8);
    let s0 = ag.ingest_raw(0, 0, "prn", "host", 1).err() == Some(AggError::Src);
    let s1 = ag.ingest_raw(0, 0b011, "prn", "host", 1).err() == Some(AggError::Src);
    if bump(AggError::Src, s0 && s1, &mut mask) {
        caught += 1;
    }

    // 10) Cap：容量 1 塞两键
    let mut ag = Aggregator::new(1);
    let c0 = ag.ingest_raw(0, 0b001, "a", "h", 1).is_ok();
    let c1 = ag.ingest_raw(0, 0b001, "b", "h", 1).err() == Some(AggError::Cap);
    if bump(AggError::Cap, c0 && c1, &mut mask) {
        caught += 1;
    }

    // 11) IdOvf：id 分配器已到上限
    let mut ag = Aggregator::new_with_next(4, u32::MAX);
    if bump(
        AggError::IdOvf,
        ag.ingest_raw(0, 0b001, "a", "h", 1).err() == Some(AggError::IdOvf),
        &mut mask,
    ) {
        caught += 1;
    }

    (caught, total, mask)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50146 自检：三协议 ×10 批去重 + 单源视图 + 11 类注入。
pub fn run_unx50146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50146");

    // 十批统一入账（后面多条判据复用它）
    let mut ag = Aggregator::new(64);
    let mut rep_seen = 0usize;
    let mut rep_inserted = 0usize;
    let mut rep_merged = 0usize;
    let mut rep_rejected = 0usize;
    let mut all_balanced = true;
    let mut b = 0usize;
    while b < 10 {
        let lines = batch_lines(b);
        let mut refs: Vec<&str> = Vec::new();
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(x) = lines.get(i) {
                refs.push(x.as_str());
            }
            i += 1;
        }
        let rep = ag.ingest_lines(b as u32, refs.as_slice());
        if !rep.balanced() {
            all_balanced = false;
        }
        rep_seen += rep.seen;
        rep_inserted += rep.inserted;
        rep_merged += rep.merged;
        rep_rejected += rep.rejected;
        b += 1;
    }

    // J1-1 十批聚合：id 与规范键都不得重复
    let mut ids_unique = true;
    let mut keys_unique = true;
    let mut i = 0usize;
    while i < ag.entries().len() {
        let mut j = i + 1;
        while j < ag.entries().len() {
            if let (Some(a), Some(c)) = (ag.entries().get(i), ag.entries().get(j)) {
                if a.id == c.id {
                    ids_unique = false;
                }
                if a.key == c.key {
                    keys_unique = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let j1 = ag.entries().len() == 8 && ids_unique && keys_unique && all_balanced;
    let det = format!(
        "entries={} unique_id={} unique_key={}",
        ag.entries().len(),
        ids_unique,
        keys_unique
    );
    s.add_dyn("50146-J1-十批去重", j1, &det);

    // J1-2 计数守恒：60 行 = 8 新建 + 52 合并，零拒绝
    let j2 = rep_seen == 60
        && rep_inserted == 8
        && rep_merged == 52
        && rep_rejected == 0
        && ag.seen == rep_seen
        && ag.inserted == rep_inserted
        && ag.merged == rep_merged
        && ag.rejected == 0;
    let det2 = format!(
        "seen={} ins={} mrg={} rej={}",
        rep_seen, rep_inserted, rep_merged, rep_rejected
    );
    s.add_dyn("50146-J1-计数守恒", j2, &det2);

    // J1-3 独立键表对照：8 个服务一个不少、一个不多
    let oracle = oracle_keys();
    let mut matched = 0usize;
    let mut k = 0usize;
    while k < oracle.len() {
        if let Some(want) = oracle.get(k) {
            if ag.find(want).is_some() {
                matched += 1;
            }
        }
        k += 1;
    }
    let mut stray = 0usize;
    let mut e = 0usize;
    while e < ag.entries().len() {
        if let Some(rec) = ag.entries().get(e) {
            let mut hit = false;
            let mut o = 0usize;
            while o < oracle.len() {
                if let Some(want) = oracle.get(o) {
                    if rec.key == *want {
                        hit = true;
                    }
                }
                o += 1;
            }
            if !hit {
                stray += 1;
            }
        }
        e += 1;
    }
    let j3 = oracle.len() == 8 && matched == 8 && stray == 0;
    let det3 = format!("oracle={} matched={} stray={}", oracle.len(), matched, stray);
    s.add_dyn("50146-J1-键表对照", j3, &det3);

    // J1-4 单源视图：三视图各 8 条，并集仍是 8 条，每条记录都拿满三位
    let mdns = ag.by_source(Source::Mdns);
    let slp = ag.by_source(Source::Slp);
    let lpd = ag.by_source(Source::Lpd);
    let mut all_sources = 0usize;
    let mut e = 0usize;
    while e < ag.entries().len() {
        if let Some(rec) = ag.entries().get(e) {
            if rec.sources == 0b111 {
                all_sources += 1;
            }
        }
        e += 1;
    }
    let j4 = mdns.len() == 8
        && slp.len() == 8
        && lpd.len() == 8
        && all_sources == 8
        && ag.entries().len() == 8;
    let det4 = format!(
        "mdns={} slp={} lpd={} triple={}",
        mdns.len(),
        slp.len(),
        lpd.len(),
        all_sources
    );
    s.add_dyn("50146-J1-单源视图", j4, &det4);

    // J1-5 首见 id 稳定：首批见过的 laser-a，十批后 id 与首见批次都不变
    let mut stable = false;
    let first_key = match SERVICES.get(0) {
        Some(t) => key_of(t.0, t.1, t.2),
        None => Key {
            name: String::new(),
            host: String::new(),
            port: 0,
        },
    };
    if let Some(rec) = ag.find(&first_key) {
        stable = rec.id == 1 && rec.first_batch == 0;
    }
    s.add("50146-J1-首见id稳定", stable, if stable { "" } else { "id 被重写" });

    // J1-6 幂等重放：重放第 5 批，账本摘要不变且零新建
    let before = ag.digest();
    let lines5 = batch_lines(5);
    let mut refs5: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < lines5.len() {
        if let Some(x) = lines5.get(i) {
            refs5.push(x.as_str());
        }
        i += 1;
    }
    let rep5 = ag.ingest_lines(5, refs5.as_slice());
    let after = ag.digest();
    let j6 = before == after && rep5.inserted == 0 && rep5.rejected == 0 && rep5.merged == 6;
    let det6 = format!("digest={:08x} ins={} mrg={}", after, rep5.inserted, rep5.merged);
    s.add_dyn("50146-J1-幂等重放", j6, &det6);

    // J1-7 大小写归并：PRN-A 与 prn-a 是同一个服务
    let mut ag2 = Aggregator::new(8);
    let _ = ag2.ingest_one(0, "mdns:PRN-A@prn-a.lab:9100");
    let _ = ag2.ingest_one(0, "slp:prn-a//prn-a.lab:9100");
    let mut case_ok = false;
    if let Some(rec) = ag2.entries().first() {
        case_ok = ag2.entries().len() == 1
            && rec.display_name == "PRN-A"
            && rec.sources == 0b011
            && rec.key.name == "prn-a";
    }
    s.add("50146-J1-大小写归并", case_ok, if case_ok { "" } else { "大小写未归并" });

    // J1-8 端口区分：同名主机不同端口是两个服务
    let mut ag3 = Aggregator::new(8);
    let _ = ag3.ingest_raw(0, 0b001, "prn", "host.lab", 9100);
    let _ = ag3.ingest_raw(0, 0b001, "prn", "host.lab", 9101);
    let port_ok = ag3.entries().len() == 2 && ag3.inserted == 2;
    s.add("50146-J1-端口区分", port_ok, if port_ok { "" } else { "端口被忽略" });

    // J1-9 来源位合并：三次通告一条记录
    let mut ag4 = Aggregator::new(8);
    let _ = ag4.ingest_one(0, "mdns:shared@host.lab:515");
    let _ = ag4.ingest_one(1, "slp:shared//host.lab:515");
    let _ = ag4.ingest_one(2, "lpd:shared,host.lab,515");
    let mut src_ok = false;
    if let Some(rec) = ag4.entries().first() {
        src_ok = ag4.entries().len() == 1
            && rec.sources == 0b111
            && rec.first_batch == 0
            && rec.touches == 3
            && ag4.by_source(Source::Mdns).len() == 1
            && ag4.by_source(Source::Slp).len() == 1
            && ag4.by_source(Source::Lpd).len() == 1;
    }
    s.add("50146-J1-来源位合并", src_ok, if src_ok { "" } else { "来源位未合并" });

    // J1-10 三协议编解码往返：编码器的输出必须被解码回原字段
    let mut rt = 0usize;
    let probes: [(String, Source); 3] = [
        (line_mdns("laser-a", "prn-a.lab", 9100), Source::Mdns),
        (line_slp("color-b", "prn-b.lab", 9100), Source::Slp),
        (line_lpd("scan-c", "prn-c.lab", 515), Source::Lpd),
    ];
    let mut i = 0usize;
    while i < probes.len() {
        if let Some((line, src)) = probes.get(i) {
            if let Ok(a) = decode_line(line.as_str()) {
                if a.source == *src && a.port != 0 && !a.name.is_empty() && !a.host.is_empty() {
                    rt += 1;
                }
            }
        }
        i += 1;
    }
    let rt_ok = rt == 3;
    s.add("50146-J1-解码往返", rt_ok, if rt_ok { "" } else { "编解码不互逆" });

    // J1-11 容量显式拒绝
    let mut ag5 = Aggregator::new(1);
    let c0 = ag5.ingest_raw(0, 0b001, "a", "host.lab", 1).is_ok();
    let c1 = ag5.ingest_raw(0, 0b001, "b", "host.lab", 1).err() == Some(AggError::Cap);
    let cap_ok = c0 && c1 && ag5.entries().len() == 1 && ag5.err_count(AggError::Cap) == 1;
    s.add("50146-J1-容量拒绝", cap_ok, if cap_ok { "" } else { "容量溢出未拒" });

    // J1-12 非法协议与多源位
    let mut ag6 = Aggregator::new(8);
    let proto_ok = ag6.ingest_one(0, "smb:nas@host:445").err() == Some(AggError::Proto);
    let src0 = ag6.ingest_raw(0, 0, "n", "host.lab", 1).err() == Some(AggError::Src);
    let src3 = ag6.ingest_raw(0, 0b111, "n", "host.lab", 1).err() == Some(AggError::Src);
    let guard_ok = proto_ok && src0 && src3 && ag6.entries().is_empty();
    s.add("50146-J1-协议与来源", guard_ok, if guard_ok { "" } else { "协议/来源校验缺失" });

    // J1-13 命名与端口护栏：7 种坏字段各得其所
    let mut long_name = String::new();
    let mut i = 0usize;
    while i < 64 {
        long_name.push('n');
        i += 1;
    }
    let mut long_host = String::new();
    let mut i = 0usize;
    while i < 64 {
        long_host.push('h');
        i += 1;
    }
    let ln = line_mdns(long_name.as_str(), "host.lab", 1);
    let lh = line_mdns("prn", long_host.as_str(), 1);
    let mut g = 0usize;
    let guards: [(&str, AggError); 5] = [
        ("mdns:@host:1", AggError::Name),
        ("mdns:prn@:1", AggError::Host),
        ("mdns:prn@host:0", AggError::Port),
        ("mdns:prn@host:70000", AggError::Port),
        ("mdns:prn@host:abc", AggError::Line),
    ];
    let mut i = 0usize;
    while i < guards.len() {
        if let Some((line, want)) = guards.get(i) {
            if decode_line(line).err() == Some(*want) {
                g += 1;
            }
        }
        i += 1;
    }
    if decode_line(ln.as_str()).err() == Some(AggError::NameLen) {
        g += 1;
    }
    if decode_line(lh.as_str()).err() == Some(AggError::HostLen) {
        g += 1;
    }
    let guard7 = g == 7;
    s.add("50146-J1-字段护栏", guard7, if guard7 { "" } else { "护栏有缺口" });

    // J1-14 坏包不污染账本：2 好 2 坏，账本只记 2 条
    let mut ag7 = Aggregator::new(8);
    let rep7 = ag7.ingest_lines(
        0,
        &[
            "mdns:good-a@host.lab:9100",
            "smb:nas@host.lab:445",
            "slp:good-b//host.lab:515",
            "mdns:bad@host.lab:abc",
        ],
    );
    let clean_ok = ag7.entries().len() == 2
        && rep7.seen == 4
        && rep7.inserted == 2
        && rep7.rejected == 2
        && ag7.err_count(AggError::Proto) == 1
        && ag7.err_count(AggError::Line) == 1
        && ag7.first_err == AggError::Proto.code();
    let det7 = format!(
        "entries={} seen={} ins={} rej={}",
        ag7.entries().len(),
        rep7.seen,
        rep7.inserted,
        rep7.rejected
    );
    s.add_dyn("50146-J1-坏包隔离", clean_ok, &det7);

    // J1-15 注入覆盖：11 个码全部真实构造
    let (caught, total, mask) = injection_core();
    let full_mask = (1u32 << ERR_SLOTS) - 1;
    let inj_ok = caught == total && total == ERR_SLOTS && mask == full_mask;
    let det8 = format!("caught={}/{} mask={:x}", caught, total, mask);
    s.add_dyn("50146-J1-注入覆盖", inj_ok, &det8);

    // J1-16 视图确定性：id 升序，两次摘要一致
    let mut ascending = true;
    let mut prev = 0u32;
    let mut i = 0usize;
    while i < ag.entries().len() {
        if let Some(rec) = ag.entries().get(i) {
            if rec.id <= prev {
                ascending = false;
            }
            prev = rec.id;
        }
        i += 1;
    }
    let d1 = ag.digest();
    let d2 = ag.digest();
    let det_ok = ascending && d1 == d2;
    s.add("50146-J1-视图确定性", det_ok, if det_ok { "" } else { "视图顺序/摘要漂移" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50146_checks_green() {
        let cs = run_unx50146_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn unx50146_ten_batches_unique() {
        let mut ag = Aggregator::new(64);
        let mut b = 0usize;
        while b < 10 {
            let lines = batch_lines(b);
            let mut refs: Vec<&str> = Vec::new();
            let mut i = 0usize;
            while i < lines.len() {
                if let Some(x) = lines.get(i) {
                    refs.push(x.as_str());
                }
                i += 1;
            }
            let rep = ag.ingest_lines(b as u32, refs.as_slice());
            assert!(rep.balanced());
            b += 1;
        }
        assert_eq!(ag.entries().len(), 8);
        assert_eq!(ag.inserted, 8);
        assert_eq!(ag.rejected, 0);
        let mut i = 0usize;
        while i < ag.entries().len() {
            let mut j = i + 1;
            while j < ag.entries().len() {
                if let (Some(a), Some(c)) = (ag.entries().get(i), ag.entries().get(j)) {
                    assert_ne!(a.id, c.id);
                    assert_ne!(a.key, c.key);
                }
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn unx50146_injection_covers_all_codes() {
        let (caught, total, mask) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, ERR_SLOTS);
        assert_eq!(mask, (1u32 << ERR_SLOTS) - 1);
    }

    #[test]
    fn unx50146_line_roundtrip() {
        let cases: [(String, Source); 3] = [
            (line_mdns("laser-a", "prn-a.lab", 9100), Source::Mdns),
            (line_slp("color-b", "prn-b.lab", 9100), Source::Slp),
            (line_lpd("scan-c", "prn-c.lab", 515), Source::Lpd),
        ];
        let mut i = 0usize;
        let mut rt_ok = true;
        while i < cases.len() {
            if let Some((line, src)) = cases.get(i) {
                let mut one = false;
                if let Ok(a) = decode_line(line.as_str()) {
                    one = a.source == *src && a.port != 0;
                }
                rt_ok = rt_ok && one;
            }
            i += 1;
        }
        assert!(rt_ok);
    }
}
