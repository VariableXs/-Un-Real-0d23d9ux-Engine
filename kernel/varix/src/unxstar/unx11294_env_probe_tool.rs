//! UNX-F11294 环境探测诊断（工具面呈现）（0x1f690 段）
//!
//! 锚点：UNX-F11294-J1 auxv / proc 自省工具面落账，与基准机逐项一致。
//!
//! 纯功能模型：把内核交给动态装载器的 auxiliary vector 按工具视角摊开
//! （AT_PAGESZ / AT_CLKTCK / AT_HWCAP / AT_UID / AT_SECURE / AT_PLATFORM /
//! AT_ENTRY / AT_BASE / AT_RANDOM ...），并以 /proc/self/auxv、
//! /proc/self/status、/proc/self/maps 三份自省面交叉核对。工具面必须
//! 区分「键缺失」与「值为零」：AT_SECURE=0 是有效结论，键不存在才是
//! 探测失败，二者不可混为一谈。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f690;
/// 未知的 auxv 键名
pub const EP_ENAME: u32 = SEG_BASE + 0x01;
/// 已知键但探测面缺失
pub const EP_EMISS: u32 = SEG_BASE + 0x02;
/// 键序回退或键位非法
pub const EP_EORDER: u32 = SEG_BASE + 0x03;
/// 自省面格式不可解析
pub const EP_EFMT: u32 = SEG_BASE + 0x04;
/// 与基准机数值不一致
pub const EP_EBASE: u32 = SEG_BASE + 0x05;
/// 冻结后登记
pub const EP_EFROZEN: u32 = SEG_BASE + 0x06;
/// 键重复登记
pub const EP_EDUP: u32 = SEG_BASE + 0x07;
/// auxv 字数为奇数或与自省面不符
pub const EP_EWORD: u32 = SEG_BASE + 0x08;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

// ---------------------------------------------------------------- auxv 键位
pub const AT_BASE: u32 = 7;
pub const AT_ENTRY: u32 = 9;
pub const AT_UID: u32 = 11;
pub const AT_EUID: u32 = 12;
pub const AT_GID: u32 = 13;
pub const AT_EGID: u32 = 14;
pub const AT_PLATFORM: u32 = 15;
pub const AT_HWCAP: u32 = 16;
pub const AT_CLKTCK: u32 = 17;
pub const AT_SECURE: u32 = 23;
pub const AT_RANDOM: u32 = 25;
pub const AT_EXECFN: u32 = 31;
pub const AT_PAGESZ: u32 = 6;
pub const AT_SYSINFO_EHDR: u32 = 33;

/// 键位到工具名的映射表（按键升序）。
const AUX_NAMES: [(&str, u32); 14] = [
    ("AT_PAGESZ", AT_PAGESZ),
    ("AT_BASE", AT_BASE),
    ("AT_ENTRY", AT_ENTRY),
    ("AT_UID", AT_UID),
    ("AT_EUID", AT_EUID),
    ("AT_GID", AT_GID),
    ("AT_EGID", AT_EGID),
    ("AT_PLATFORM", AT_PLATFORM),
    ("AT_HWCAP", AT_HWCAP),
    ("AT_CLKTCK", AT_CLKTCK),
    ("AT_SECURE", AT_SECURE),
    ("AT_RANDOM", AT_RANDOM),
    ("AT_EXECFN", AT_EXECFN),
    ("AT_SYSINFO_EHDR", AT_SYSINFO_EHDR),
];

/// 基准机的 16 字节随机种子（AT_RANDOM 指向的内存内容）。
static RANDOM16: [u8; 16] = [
    0x9a, 0x17, 0x4c, 0xe2, 0x5b, 0x08, 0xd3, 0x66, 0x21, 0xaf, 0x70, 0x5d, 0xbe, 0x94, 0x3a, 0x0f,
];

/// 单个 auxv 值：字 / 文本 / 字节块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuxValue {
    Word(u64),
    Text(&'static str),
    Bytes(&'static [u8]),
}

/// 单个 auxv 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxEntry {
    pub key: u32,
    pub value: AuxValue,
}

/// 键名反查；未知键返回 None。
pub fn aux_key(name: &str) -> Option<u32> {
    let mut k = 0usize;
    while k < AUX_NAMES.len() {
        if let Some(row) = AUX_NAMES.get(k) {
            if row.0 == name {
                return Some(row.1);
            }
        }
        k = k.wrapping_add(1);
    }
    None
}

/// 键位正查；未知键返回 None。
pub fn aux_name(key: u32) -> Option<&'static str> {
    let mut k = 0usize;
    while k < AUX_NAMES.len() {
        if let Some(row) = AUX_NAMES.get(k) {
            if row.1 == key {
                return Some(row.0);
            }
        }
        k = k.wrapping_add(1);
    }
    None
}

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut s = 0u32;
    while s < 32 {
        x = fold_byte(x, ((v >> s) & 0xff) as u8);
        s = s.wrapping_add(8);
    }
    x
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

/// 工具视角下的 auxv 表：按键升序登记，缺键与零值严格区分。
#[derive(Clone, Debug)]
pub struct AuxVec {
    entries: Vec<AuxEntry>,
    seal: Option<u64>,
    pub lookups: u32,
    pub misses: u32,
    pub rejects: u32,
}

impl AuxVec {
    pub fn empty() -> Self {
        AuxVec {
            entries: Vec::new(),
            seal: None,
            lookups: 0,
            misses: 0,
            rejects: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    /// 按键升序登记；键 0 / 未知键 / 重键 / 回退键 / 冻结后一律拒。
    pub fn install(&mut self, key: u32, value: AuxValue) -> Result<(), u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EP_EFROZEN);
        }
        if key == 0 || aux_name(key).is_none() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(EP_ENAME);
        }
        if let Some(last) = self.entries.last() {
            if key == last.key {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(EP_EDUP);
            }
            if key < last.key {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(EP_EORDER);
            }
        }
        self.entries.push(AuxEntry { key, value });
        Ok(())
    }

    pub fn entry(&self, key: u32) -> Option<&AuxEntry> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.key == key {
                    return Some(e);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 取字值；缺键为 EP_EMISS，类型不符为 EP_EFMT。
    pub fn word_of(&self, key: u32) -> Result<u64, u32> {
        match self.entry(key) {
            Some(e) => match e.value {
                AuxValue::Word(v) => Ok(v),
                _ => Err(EP_EFMT),
            },
            None => Err(EP_EMISS),
        }
    }

    pub fn text_of(&self, key: u32) -> Result<&'static str, u32> {
        match self.entry(key) {
            Some(e) => match e.value {
                AuxValue::Text(t) => Ok(t),
                _ => Err(EP_EFMT),
            },
            None => Err(EP_EMISS),
        }
    }

    pub fn bytes_of(&self, key: u32) -> Result<&'static [u8], u32> {
        match self.entry(key) {
            Some(e) => match e.value {
                AuxValue::Bytes(b) => Ok(b),
                _ => Err(EP_EFMT),
            },
            None => Err(EP_EMISS),
        }
    }

    /// auxv 在内存中的字数（末尾含 AT_NULL 一对）。
    pub fn word_count(&self) -> u32 {
        (self.entries.len() as u32).wrapping_add(1).wrapping_mul(2)
    }

    /// 工具名取值，返回 "NAME=value"；缺键与未知名分别报错。
    pub fn tool_getauxval(&mut self, name: &str) -> Result<String, u32> {
        self.lookups = self.lookups.wrapping_add(1);
        let key = match aux_key(name) {
            Some(k) => k,
            None => {
                self.misses = self.misses.wrapping_add(1);
                return Err(EP_ENAME);
            }
        };
        if self.entry(key).is_none() {
            self.misses = self.misses.wrapping_add(1);
            return Err(EP_EMISS);
        }
        let rendered = match self.entry(key) {
            Some(e) => render_value(key, &e.value),
            None => return Err(EP_EMISS),
        };
        let name = match aux_name(key) {
            Some(n) => n,
            None => name,
        };
        Ok(format!("{}={}", name, rendered))
    }

    /// 整表按工具风格渲染（键升序，值与键同宽展现）。
    pub fn render_table(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                let name = match aux_name(e.key) {
                    Some(n) => n,
                    None => "AT_UNKNOWN",
                };
                out.push(format!("{} = {}", name, render_value(e.key, &e.value)));
            }
            k = k.wrapping_add(1);
        }
        out
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                h = fold_u32(h, e.key);
                h = fold_str(h, &render_value(e.key, &e.value));
            }
            k = k.wrapping_add(1);
        }
        h
    }

    /// 幂等冻结：首次取指纹并落锁，其后原样返回。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

fn render_value(key: u32, value: &AuxValue) -> String {
    match value {
        AuxValue::Word(v) => match key {
            AT_BASE | AT_ENTRY | AT_HWCAP | AT_SYSINFO_EHDR => format!("0x{:x}", v),
            _ => format!("{}", v),
        },
        AuxValue::Text(t) => (*t).to_string(),
        AuxValue::Bytes(b) => format!("{} bytes", b.len()),
    }
}

// ---------------------------------------------------------------- proc 自省面
/// 一份 /proc 自省面：路径 + 正文（正文按行解析）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcFacet {
    pub path: String,
    pub body: String,
}

impl ProcFacet {
    pub fn new(path: &str, body: &str) -> Self {
        ProcFacet {
            path: path.to_string(),
            body: body.to_string(),
        }
    }

    /// 行数（以 \n 计，末行无换行也算一行）。
    pub fn line_count(&self) -> u32 {
        if self.body.is_empty() {
            return 0;
        }
        let mut n = 0u32;
        let mut k = 0usize;
        let bytes = self.body.as_bytes();
        while k < bytes.len() {
            if let Some(b) = bytes.get(k) {
                if *b == b'\n' {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        if bytes.last() == Some(&b'\n') {
            n
        } else {
            n.wrapping_add(1)
        }
    }
}

/// 多份自省面的集合，按路径去重、可冻结。
#[derive(Clone, Debug)]
pub struct ProcView {
    facets: Vec<ProcFacet>,
    seal: Option<u64>,
}

impl ProcView {
    pub fn empty() -> Self {
        ProcView {
            facets: Vec::new(),
            seal: None,
        }
    }

    pub fn len(&self) -> usize {
        self.facets.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    pub fn add(&mut self, path: &str, body: &str) -> Result<(), u32> {
        if self.seal.is_some() {
            return Err(EP_EFROZEN);
        }
        if path.is_empty() || body.is_empty() {
            return Err(EP_EFMT);
        }
        if self.get(path).is_some() {
            return Err(EP_EDUP);
        }
        self.facets.push(ProcFacet::new(path, body));
        Ok(())
    }

    pub fn get(&self, path: &str) -> Option<&ProcFacet> {
        let mut k = 0usize;
        while k < self.facets.len() {
            if let Some(f) = self.facets.get(k) {
                if f.path == path {
                    return Some(f);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    fn body_of(&self, path: &str) -> Result<&str, u32> {
        match self.get(path) {
            Some(f) => Ok(f.body.as_str()),
            None => Err(EP_EMISS),
        }
    }

    /// 交叉核对：auxv 字数、身份、页长、保密位、映射数逐项对表。
    pub fn cross_check(&self, aux: &AuxVec) -> Result<(), u32> {
        let words = match assign_field(self.body_of("/proc/self/auxv")?, "words") {
            Some(v) => v,
            None => return Err(EP_EFMT),
        };
        if words != aux.word_count() {
            return Err(EP_EWORD);
        }
        let status = self.body_of("/proc/self/status")?;
        let uid = match line_field(status, "Uid:") {
            Some(v) => v,
            None => return Err(EP_EFMT),
        };
        let site_uid = match aux.word_of(AT_UID) {
            Ok(v) => v as u32,
            Err(e) => return Err(e),
        };
        if uid != site_uid {
            return Err(EP_EBASE);
        }
        let pagesz = match line_field(status, "Pagesize:") {
            Some(v) => v,
            None => return Err(EP_EFMT),
        };
        let aux_pagesz = match aux.word_of(AT_PAGESZ) {
            Ok(v) => v as u32,
            Err(e) => return Err(e),
        };
        if pagesz != aux_pagesz || pagesz == 0 || (pagesz & pagesz.wrapping_sub(1)) != 0 {
            return Err(EP_EBASE);
        }
        let nnp = match line_field(status, "NoNewPrivs:") {
            Some(v) => v as u64,
            None => return Err(EP_EFMT),
        };
        let secure = match aux.word_of(AT_SECURE) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        if nnp != secure {
            return Err(EP_EBASE);
        }
        let clk = match aux.word_of(AT_CLKTCK) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        if clk != 100 {
            return Err(EP_EBASE);
        }
        let platform = match aux.text_of(AT_PLATFORM) {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        if platform != "x86_64" {
            return Err(EP_EBASE);
        }
        let base = match aux.word_of(AT_BASE) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let entry = match aux.word_of(AT_ENTRY) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        if base == 0 || entry == 0 || base % pagesz as u64 != 0 {
            return Err(EP_EBASE);
        }
        let maps = self.get("/proc/self/maps").ok_or(EP_EMISS)?;
        if maps.line_count() != 3 {
            return Err(EP_EBASE);
        }
        Ok(())
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.facets.len() {
            if let Some(f) = self.facets.get(k) {
                h = fold_str(h, &f.path);
                h = fold_str(h, &f.body);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v = 0u64;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        let b = match bytes.get(k) {
            Some(b) => *b,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add((b - b'0') as u64);
        k = k.wrapping_add(1);
    }
    Some(v)
}

fn parse_u32(s: &str) -> Option<u32> {
    match parse_u64(s) {
        Some(v) if v <= u32::MAX as u64 => Some(v as u32),
        _ => None,
    }
}

/// 解析 "Uid:\t1000 ..." 这类以空白分列的行，取第二个字段。
fn line_field(body: &str, key: &str) -> Option<u32> {
    let mut it = body.split('\n');
    while let Some(line) = it.next() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some(key) {
            return parts.next().and_then(parse_u32);
        }
    }
    None
}

/// 解析 "words=30" 这类赋值行。
fn assign_field(body: &str, key: &str) -> Option<u32> {
    let mut it = body.split('\n');
    while let Some(line) = it.next() {
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                return parse_u32(v.trim());
            }
        }
    }
    None
}

// ---------------------------------------------------------------- 基准机
const AUXV_BODY: &str = "words=30\n";
const STATUS_BODY: &str = "Name:\tprobe\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nPagesize:\t4096\nNoNewPrivs:\t0\n";
const MAPS_BODY: &str = "55f0d2a4c000-55f0d2a4e000 r--p 00000000 08:01 1234 /usr/bin/probe\n7f4a2c000000-7f4a2c021000 r-xp 00001000 08:01 1234 /usr/bin/probe\n7ffd3e4f6000-7ffd3e517000 rw-p 00000000 00:00 0 [stack]\n";
const EXE_BODY: &str = "/usr/bin/probe\n";

/// 基准机探针：14 项 auxv + 4 份自省面，全部已冻结。
pub struct Baseline {
    pub aux: AuxVec,
    pub view: ProcView,
    pub aux_seal: Option<u64>,
    pub proc_seal: Option<u64>,
}

pub fn build_baseline() -> Baseline {
    let mut aux = AuxVec::empty();
    let _ = aux.install(AT_PAGESZ, AuxValue::Word(4096));
    let _ = aux.install(AT_BASE, AuxValue::Word(0x7f4a_2c00_0000));
    let _ = aux.install(AT_ENTRY, AuxValue::Word(0x55f0_d2a4_c1a0));
    let _ = aux.install(AT_UID, AuxValue::Word(1000));
    let _ = aux.install(AT_EUID, AuxValue::Word(1000));
    let _ = aux.install(AT_GID, AuxValue::Word(1000));
    let _ = aux.install(AT_EGID, AuxValue::Word(1000));
    let _ = aux.install(AT_PLATFORM, AuxValue::Text("x86_64"));
    let _ = aux.install(AT_HWCAP, AuxValue::Word(0x178b_fbff));
    let _ = aux.install(AT_CLKTCK, AuxValue::Word(100));
    let _ = aux.install(AT_SECURE, AuxValue::Word(0));
    let _ = aux.install(AT_RANDOM, AuxValue::Bytes(&RANDOM16));
    let _ = aux.install(AT_EXECFN, AuxValue::Text("/usr/bin/probe"));
    let _ = aux.install(AT_SYSINFO_EHDR, AuxValue::Word(0x7f4a_2c00_1000));

    let mut view = ProcView::empty();
    let _ = view.add("/proc/self/auxv", AUXV_BODY);
    let _ = view.add("/proc/self/status", STATUS_BODY);
    let _ = view.add("/proc/self/maps", MAPS_BODY);
    let _ = view.add("/proc/self/exe", EXE_BODY);

    let aux_seal = aux.freeze();
    let proc_seal = view.freeze();
    Baseline {
        aux,
        view,
        aux_seal,
        proc_seal,
    }
}

/// 四路对抗：回退键、重键、未知名、字数篡改，全部必须被拒。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];

    let mut back = AuxVec::empty();
    let _ = back.install(AT_ENTRY, AuxValue::Word(1));
    hits[0] = back.install(AT_PAGESZ, AuxValue::Word(4096)) == Err(EP_EORDER);

    let mut dup = AuxVec::empty();
    let _ = dup.install(AT_UID, AuxValue::Word(1000));
    hits[1] = dup.install(AT_UID, AuxValue::Word(1001)) == Err(EP_EDUP);

    let mut unnamed = AuxVec::empty();
    hits[2] = unnamed.tool_getauxval("AT_BOGUS") == Err(EP_ENAME);

    let base = build_baseline();
    let mut tampered = ProcView::empty();
    let _ = tampered.add("/proc/self/auxv", "words=31\n");
    let _ = tampered.add("/proc/self/status", STATUS_BODY);
    let _ = tampered.add("/proc/self/maps", MAPS_BODY);
    hits[3] = tampered.cross_check(&base.aux) == Err(EP_EWORD);

    hits
}

// ---------------------------------------------------------------- 判据
fn tool_text(aux: &mut AuxVec, name: &str) -> String {
    match aux.tool_getauxval(name) {
        Ok(t) => t,
        Err(e) => format!("#err={}", e),
    }
}

pub fn run_unx11294_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11294");
    let mut base = build_baseline();

    // J1 基准面落账
    s.add(
        "J1-基准面落账",
        base.aux.len() == 14 && base.aux.word_count() == 30 && base.view.len() == 4,
        "14 项 auxv / 30 字 / 4 份自省面",
    );

    // J2 键序呈现
    let table = base.aux.render_table();
    let first = table.first().map(|x| x.as_str());
    let last = table.last().map(|x| x.as_str());
    s.add(
        "J2-键序呈现",
        first == Some("AT_PAGESZ = 4096") && last == Some("AT_SYSINFO_EHDR = 0x7f4a2c001000"),
        "工具表按 key 升序且首末正确",
    );

    // J3 运行参数
    let pagesz = base.aux.word_of(AT_PAGESZ);
    let clk = base.aux.word_of(AT_CLKTCK);
    s.add(
        "J3-运行参数",
        pagesz == Ok(4096) && clk == Ok(100),
        "页长 4096 / 时钟滴答 100",
    );

    // J4 平台与硬件能力
    let platform = base.aux.text_of(AT_PLATFORM);
    let hw = base.aux.word_of(AT_HWCAP);
    let sse2 = match hw {
        Ok(v) => (v & (1u64 << 26)) != 0,
        Err(_) => false,
    };
    let avx = match hw {
        Ok(v) => (v & (1u64 << 28)) != 0,
        Err(_) => false,
    };
    s.add(
        "J4-平台与能力",
        platform == Ok("x86_64") && sse2 && avx,
        "x86_64 且 HWCAP 含 SSE2/AVX 位",
    );

    // J5 身份四元组
    let ids = [
        base.aux.word_of(AT_UID),
        base.aux.word_of(AT_EUID),
        base.aux.word_of(AT_GID),
        base.aux.word_of(AT_EGID),
    ];
    let all_1000 = ids.iter().all(|v| *v == Ok(1000));
    s.add(
        "J5-身份四元组",
        all_1000 && base.aux.word_of(AT_SECURE) == Ok(0),
        "uid/euid/gid/egid 均 1000 且保密位为 0",
    );

    // J6 地址与随机种子
    let rnd_len = match base.aux.bytes_of(AT_RANDOM) {
        Ok(b) => b.len(),
        Err(_) => 0,
    };
    let basev = base.aux.word_of(AT_BASE);
    let entry = base.aux.word_of(AT_ENTRY);
    let sysinfo = base.aux.word_of(AT_SYSINFO_EHDR);
    let aligned = match basev {
        Ok(v) => v % 4096 == 0,
        Err(_) => false,
    };
    s.add(
        "J6-地址与种子",
        rnd_len == 16 && aligned && entry != Ok(0) && sysinfo != Ok(0),
        "AT_RANDOM 16 字节；base 页对齐；entry/vdso 非零",
    );

    // J7 工具读值呈现
    let p = tool_text(&mut base.aux, "AT_PAGESZ");
    let e = tool_text(&mut base.aux, "AT_ENTRY");
    let f = tool_text(&mut base.aux, "AT_PLATFORM");
    let r = tool_text(&mut base.aux, "AT_RANDOM");
    s.add(
        "J7-工具读值",
        p == "AT_PAGESZ=4096"
            && e == "AT_ENTRY=0x55f0d2a4c1a0"
            && f == "AT_PLATFORM=x86_64"
            && r == "AT_RANDOM=16 bytes",
        "四类值按工具风格呈现",
    );

    // J8 缺键与零值区分
    let mut part = AuxVec::empty();
    let _ = part.install(AT_PAGESZ, AuxValue::Word(4096));
    let _ = part.install(AT_SECURE, AuxValue::Word(0));
    let zero_seen = part.tool_getauxval("AT_SECURE") == Ok("AT_SECURE=0".to_string());
    let absent = part.tool_getauxval("AT_ENTRY") == Err(EP_EMISS);
    s.add(
        "J8-缺键区分",
        zero_seen && absent && part.entry(AT_ENTRY).is_none(),
        "AT_SECURE=0 为有效值；缺 AT_ENTRY 报缺失",
    );

    // J9 未知名与计数
    let mut early = AuxVec::empty();
    let unknown = early.tool_getauxval("AT_NOPE") == Err(EP_ENAME);
    let miss = early.tool_getauxval("AT_UID") == Err(EP_EMISS);
    s.add(
        "J9-未知名与计数",
        unknown && miss && early.lookups == 2 && early.misses == 2,
        "未知名/缺键各计一次未命中",
    );

    // J10 键位三拒
    let mut back = AuxVec::empty();
    let _ = back.install(AT_ENTRY, AuxValue::Word(9));
    let order = back.install(AT_PAGESZ, AuxValue::Word(4096)) == Err(EP_EORDER);
    let mut dup = AuxVec::empty();
    let _ = dup.install(AT_UID, AuxValue::Word(1000));
    let dup_rej = dup.install(AT_UID, AuxValue::Word(1000)) == Err(EP_EDUP);
    let mut zero = AuxVec::empty();
    let zero_rej = zero.install(0, AuxValue::Word(0)) == Err(EP_ENAME);
    s.add(
        "J10-键位三拒",
        order && dup_rej && zero_rej && back.len() == 1 && dup.len() == 1,
        "回退/重键/零键均拒且册长不变",
    );

    // J11 冻结拒登记
    let before = base.aux.rejects;
    let late = base.aux.install(AT_CLKTCK, AuxValue::Word(100));
    s.add(
        "J11-冻结拒登记",
        late == Err(EP_EFROZEN)
            && base.aux.rejects == before.wrapping_add(1)
            && base.aux.len() == 14,
        "冻结后登记拒且拒绝计数加一",
    );

    // J12 自省面形态
    let maps_lines = match base.view.get("/proc/self/maps") {
        Some(f) => f.line_count(),
        None => 0,
    };
    let exe_ok = match base.view.get("/proc/self/exe") {
        Some(f) => f.body.trim() == "/usr/bin/probe",
        None => false,
    };
    let words_ok = base
        .view
        .get("/proc/self/auxv")
        .map(|f| f.body.contains("words=30"))
        == Some(true);
    s.add(
        "J12-自省面形态",
        maps_lines == 3 && exe_ok && words_ok,
        "maps 3 段 / exe 指向 / auxv 字数在册",
    );

    // J13 交叉核对
    s.add(
        "J13-交叉核对",
        base.view.cross_check(&base.aux).is_ok(),
        "auxv 与 status/maps 全项一致",
    );

    // J14 字数篡改
    let mut bad_words = ProcView::empty();
    let _ = bad_words.add("/proc/self/auxv", "words=31\n");
    let _ = bad_words.add("/proc/self/status", STATUS_BODY);
    let _ = bad_words.add("/proc/self/maps", MAPS_BODY);
    s.add(
        "J14-字数篡改",
        bad_words.cross_check(&base.aux) == Err(EP_EWORD),
        "自省面字数与 auxv 不符即拒",
    );

    // J15 身份/页长篡改
    const BAD_UID: &str = "Name:\tprobe\nUid:\t1001\t1001\t1001\t1001\nGid:\t1000\t1000\t1000\t1000\nPagesize:\t4096\nNoNewPrivs:\t0\n";
    const BAD_PAGE: &str = "Name:\tprobe\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nPagesize:\t2048\nNoNewPrivs:\t0\n";
    let mut uid_view = ProcView::empty();
    let _ = uid_view.add("/proc/self/auxv", AUXV_BODY);
    let _ = uid_view.add("/proc/self/status", BAD_UID);
    let _ = uid_view.add("/proc/self/maps", MAPS_BODY);
    let mut page_view = ProcView::empty();
    let _ = page_view.add("/proc/self/auxv", AUXV_BODY);
    let _ = page_view.add("/proc/self/status", BAD_PAGE);
    let _ = page_view.add("/proc/self/maps", MAPS_BODY);
    s.add(
        "J15-身份页长篡改",
        uid_view.cross_check(&base.aux) == Err(EP_EBASE)
            && page_view.cross_check(&base.aux) == Err(EP_EBASE),
        "身份或页长偏离基准即拒",
    );

    // J16 冻结幂等
    let af = base.aux.freeze();
    let pf = base.view.freeze();
    s.add(
        "J16-冻结幂等",
        af.is_some()
            && af == base.aux_seal
            && pf == base.proc_seal
            && base.aux.is_frozen()
            && base.view.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J17 注入四拒
    let hits = inject_four();
    s.add(
        "J17-注入四拒",
        hits.iter().all(|h| *h),
        "回退/重键/未知名/字数四路全拒",
    );

    // J18 探测明细
    s.add_dyn(
        "J18-探测明细",
        base.aux.lookups >= 4,
        &format!(
            "aux={} words={} lookups={} misses={} rej={} proc={}",
            base.aux.len(),
            base.aux.word_count(),
            base.aux.lookups,
            base.aux.misses,
            base.aux.rejects,
            base.view.len()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11294_env_probe_flow() {
        let mut b = build_baseline();
        assert_eq!(b.aux.len(), 14);
        assert_eq!(b.aux.word_count(), 30);
        assert_eq!(b.aux.text_of(AT_PLATFORM), Ok("x86_64"));
        assert_eq!(b.aux.word_of(AT_SECURE), Ok(0));
        assert!(b.aux.bytes_of(AT_RANDOM).map(|x| x.len()) == Ok(16));
        assert!(b.aux.freeze() == b.aux_seal);
        assert!(b.view.freeze() == b.proc_seal);
        assert!(b.view.cross_check(&b.aux).is_ok());
        assert_eq!(
            b.aux.install(AT_PAGESZ, AuxValue::Word(4096)).err(),
            Some(EP_EFROZEN)
        );

        let mut part = AuxVec::empty();
        assert!(part.install(AT_PAGESZ, AuxValue::Word(4096)).is_ok());
        assert_eq!(part.install(999, AuxValue::Word(1)).err(), Some(EP_ENAME));
        assert_eq!(
            part.install(AT_PAGESZ, AuxValue::Word(2)).err(),
            Some(EP_EDUP)
        );

        assert_eq!(part.tool_getauxval("AT_NOPE"), Err(EP_ENAME));
        assert_eq!(part.tool_getauxval("AT_ENTRY"), Err(EP_EMISS));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11294_checks_green() {
        let s = run_unx11294_checks();
        assert!(s.all_passed(), "UNX-F11294 判据未全绿");
    }
}
