//! UNX-F50114 直通向量账（PDF/PS 用例集+校验和）（0x43a60 段）
//!
//! 锚点：UNX-F50114-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 「可复跑」在本模块有确定含义：向量不是存一段字节，而是存**生成规范**
//! （格式 / 种子 / 页数 / 正文长度）。任何一次回放都用同一规范重新合成载荷，
//! 逐位比较摘要；摘要不一致即判 `Digest`——语料被改过，向量作废。
//! 这样向量账只有几百字节，却能稳定复现 MB 级 PDF/PS 形态的输入。
//!
//! 结构清单：
//! * 生成规范 `CaseSpec` + 确定性合成器 `render`：格式头 + 每页 `%%Page` 标记
//!   + 由种子派生的可打印正文 + 尾标记，无随机源、无时钟；
//! * 向量 `VectorCase`：规范 + 规范校验和（与字段绑定）+ 载荷摘要（回放口径）；
//! * 向量账 `CaseSet`：容量 24，顺序敏感根校验和，逐条可 replay；
//! * 文本编解码 `encode_set` / `parse_set`：导入时重算规范校验和与账根；
//! * 注入核：账文本坏样本 7 类 + 规范坏样本 3 类 + 回放摘要篡改 1 类，共 11 类。
//!
//! 错误纪律：诊断码闭集 `0x43a60 + 1..=11`；内核路径零 panic——解析按 `split`
//! 取字段，生成器按容量预留 push，不对 `Vec` 做未校验下标写入。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43a60 独占 0x20）。
pub const SEG_BASE: u32 = 0x43a60;

pub const E_FIELDS: u16 = 1;
pub const E_HEX: u16 = 2;
pub const E_FORMAT: u16 = 3;
pub const E_DUP: u16 = 4;
pub const E_COUNT: u16 = 5;
pub const E_EMPTY: u16 = 6;
pub const E_ID: u16 = 7;
pub const E_CHECKSUM: u16 = 8;
pub const E_RANGE: u16 = 9;
pub const E_PAGES: u16 = 10;
pub const E_DIGEST: u16 = 11;
pub const E_CAPACITY: u16 = 12;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecErr {
    Fields,
    Hex,
    Format,
    Dup,
    Count,
    Empty,
    Id,
    Checksum,
    Range,
    Pages,
    Digest,
    Capacity,
}

impl VecErr {
    /// 诊断码（段基址 + 闭集内偏移）。
    pub fn code(self) -> u32 {
        let n = match self {
            VecErr::Fields => E_FIELDS,
            VecErr::Hex => E_HEX,
            VecErr::Format => E_FORMAT,
            VecErr::Dup => E_DUP,
            VecErr::Count => E_COUNT,
            VecErr::Empty => E_EMPTY,
            VecErr::Id => E_ID,
            VecErr::Checksum => E_CHECKSUM,
            VecErr::Range => E_RANGE,
            VecErr::Pages => E_PAGES,
            VecErr::Digest => E_DIGEST,
            VecErr::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    /// 稳定文案。
    pub fn message(self) -> &'static str {
        match self {
            VecErr::Fields => "case line does not have 7 fields",
            VecErr::Hex => "numeric field malformed",
            VecErr::Format => "vector format tag unknown",
            VecErr::Dup => "duplicate case id",
            VecErr::Count => "declared case count mismatch",
            VecErr::Empty => "vector set is empty",
            VecErr::Id => "case id is zero",
            VecErr::Checksum => "case checksum or set root mismatch",
            VecErr::Range => "value outside legal range",
            VecErr::Pages => "page count outside 1..=8",
            VecErr::Digest => "replay digest differs from recorded digest",
            VecErr::Capacity => "vector set full",
        }
    }
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

fn fnv_u64(h: u64, x: u64) -> u64 {
    let mut acc = h;
    let mut shift = 0u32;
    while shift < 64 {
        let b = ((x >> shift) & 0xff) as u8;
        acc ^= b as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        shift += 8;
    }
    acc
}

fn parse_hex(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 16 {
        return None;
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < s.len() {
        let c = s.get(i..i + 1)?;
        let ch = c.chars().next()?;
        v = (v << 4) | ch.to_digit(16)? as u64;
        i += 1;
    }
    Some(v)
}

/// 向量格式两档（直通只认 PDF / PostScript）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecFormat {
    Pdf,
    Ps,
}

impl VecFormat {
    /// 稳定文本标签。
    pub fn tag(self) -> &'static str {
        match self {
            VecFormat::Pdf => "pdf",
            VecFormat::Ps => "ps",
        }
    }

    /// 标签反解；未知标签返回 `Format`。
    pub fn from_tag(t: &str) -> Result<VecFormat, VecErr> {
        match t {
            "pdf" => Ok(VecFormat::Pdf),
            "ps" => Ok(VecFormat::Ps),
            _ => Err(VecErr::Format),
        }
    }

    /// 文件头魔数（生成器第一段就写它，便于格式嗅探）。
    pub fn header(self) -> &'static [u8] {
        match self {
            VecFormat::Pdf => b"%PDF-1.7\n",
            VecFormat::Ps => b"%!PS-Adobe-3.0\n",
        }
    }

    /// 尾标记。
    pub fn trailer(self) -> &'static [u8] {
        match self {
            VecFormat::Pdf => b"%%EOF\n",
            VecFormat::Ps => b"%%Trailer\n",
        }
    }

    /// 码值（规范校验和用；与文本标签解耦）。
    pub fn code(self) -> u64 {
        match self {
            VecFormat::Pdf => 1,
            VecFormat::Ps => 2,
        }
    }
}

/// 用例生成规范。正文长度上限 8192 字节：向量账要的是形态覆盖，
/// 不是真实 MB 级语料（后者归样本页账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseSpec {
    pub id: u32,
    pub format: VecFormat,
    pub seed: u32,
    pub pages: u8,
    pub body_len: u16,
}

impl CaseSpec {
    /// 构造并校验。
    pub fn new(
        id: u32,
        format: VecFormat,
        seed: u32,
        pages: u8,
        body_len: u16,
    ) -> Result<CaseSpec, VecErr> {
        let s = CaseSpec {
            id,
            format,
            seed,
            pages,
            body_len,
        };
        s.validate()?;
        Ok(s)
    }

    /// 校验顺序固定为 id → pages → body：坏样本的诊断码因此可稳定断言。
    pub fn validate(&self) -> Result<(), VecErr> {
        if self.id == 0 {
            return Err(VecErr::Id);
        }
        if self.pages < 1 || self.pages > 8 {
            return Err(VecErr::Pages);
        }
        if self.body_len < 16 || self.body_len > 8192 {
            return Err(VecErr::Range);
        }
        // 每页至少 16 字节正文（页标记本身另算），否则页边界没有意义。
        let per = self.body_len as u32 / self.pages as u32;
        if per < 16 {
            return Err(VecErr::Range);
        }
        Ok(())
    }

    /// 规范校验和：与字段绑定，改任一位都变。
    pub fn checksum(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.id as u64);
        h = fnv_u64(h, self.format.code());
        h = fnv_u64(h, self.seed as u64);
        h = fnv_u64(h, self.pages as u64);
        fnv_u64(h, self.body_len as u64)
    }
}

fn body_byte(seed: u32, page: u8, j: u32) -> u8 {
    // 可打印区间 A..=Z：线性同余 + 旋转，三步即可打散相邻 j 的相关性。
    let a = seed.rotate_left(7) ^ (page as u32).wrapping_mul(2_654_435_761);
    let b = a ^ j.wrapping_mul(40_503);
    let m = (b ^ (b >> 11)) % 26;
    b'A' + m as u8
}

/// 按规范确定性合成载荷。
///
/// 布局：格式头 → 每页 [`%%Page: n pages` + 正文 + 换行] → 尾标记。
/// 正文总长严格等于 `body_len`（余数全部给最后一页），这是字节守恒判据。
pub fn render(spec: &CaseSpec) -> Result<Vec<u8>, VecErr> {
    spec.validate()?;
    let pages = spec.pages as usize;
    let total = spec.body_len as usize;
    let per = total / pages;
    let rem = total % pages;
    let mut out = Vec::with_capacity(total + pages * 24 + 32);
    out.extend_from_slice(spec.format.header());
    let mut p = 0usize;
    while p < pages {
        let line = format!("%%Page: {} {}\n", p + 1, pages);
        out.extend_from_slice(line.as_bytes());
        let take = if p + 1 == pages { per + rem } else { per };
        let mut j = 0usize;
        while j < take {
            out.push(body_byte(spec.seed, p as u8, j as u32));
            j += 1;
        }
        out.push(b'\n');
        p += 1;
    }
    out.extend_from_slice(spec.format.trailer());
    Ok(out)
}

/// 载荷摘要（回放口径，FNV-1a 64）。
pub fn payload_digest(bytes: &[u8]) -> u64 {
    fnv1a(bytes, FNV_BASIS)
}

fn count_marker(bytes: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || needle.len() > bytes.len() {
        return 0;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    let last = bytes.len() - needle.len();
    while i <= last {
        let mut k = 0usize;
        let mut hit = true;
        while k < needle.len() {
            let a = bytes.get(i + k);
            let b = needle.get(k);
            if a != b {
                hit = false;
                break;
            }
            k += 1;
        }
        if hit {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 向量条目：规范 + 规范校验和 + 载荷摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VectorCase {
    pub spec: CaseSpec,
    pub checksum: u64,
    pub digest: u64,
}

impl VectorCase {
    /// 由规范生成条目（合成一次载荷取其摘要）。
    pub fn build(spec: CaseSpec) -> Result<VectorCase, VecErr> {
        let bytes = render(&spec)?;
        Ok(VectorCase {
            spec,
            checksum: spec.checksum(),
            digest: payload_digest(&bytes),
        })
    }

    /// 条目摘要（账根折叠单元）。
    pub fn digest_entry(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.checksum);
        fnv_u64(h, self.digest)
    }

    /// 审计文本行：`id,format,seed,pages,body,checksum,digest`（数值十六进制）。
    pub fn to_line(&self) -> String {
        format!(
            "{:x},{},{:x},{:x},{:x},{:016x},{:016x}",
            self.spec.id,
            self.spec.format.tag(),
            self.spec.seed,
            self.spec.pages,
            self.spec.body_len,
            self.checksum,
            self.digest
        )
    }
}

/// 回放单条向量：重合成载荷并与记录摘要比对。摘要不符 `Digest`。
pub fn replay(case: &VectorCase) -> Result<(Vec<u8>, u64), VecErr> {
    let bytes = render(&case.spec)?;
    let d = payload_digest(&bytes);
    if d != case.digest {
        return Err(VecErr::Digest);
    }
    Ok((bytes, d))
}

/// 向量账容量：24 条覆盖 2 格式 × 4 页数档 × 3 种子档。
pub const CASE_CAP: usize = 24;

/// 向量账。
#[derive(Clone, Debug)]
pub struct CaseSet {
    cases: Vec<VectorCase>,
    root: u64,
}

impl CaseSet {
    /// 空账（根为 FNV 基值）。
    pub fn new() -> CaseSet {
        CaseSet {
            cases: Vec::with_capacity(CASE_CAP),
            root: FNV_BASIS,
        }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 账根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 只读条目切片。
    pub fn cases(&self) -> &[VectorCase] {
        &self.cases
    }

    /// 按 id 查条目。
    pub fn find(&self, id: u32) -> Option<&VectorCase> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if let Some(c) = self.cases.get(i) {
                if c.spec.id == id {
                    return Some(c);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条规范：校验 → 容量 → 重复 → 合成 → 折叠根。
    /// 拒绝路径不改账（根与长度均不变）。
    pub fn register(&mut self, spec: CaseSpec) -> Result<u64, VecErr> {
        spec.validate()?;
        if self.cases.len() >= CASE_CAP {
            return Err(VecErr::Capacity);
        }
        if self.find(spec.id).is_some() {
            return Err(VecErr::Dup);
        }
        let c = VectorCase::build(spec)?;
        let d = c.digest_entry();
        self.cases.push(c);
        self.root = fnv_u64(self.root, d);
        Ok(d)
    }

    /// 按 id 回放。
    pub fn replay_id(&self, id: u32) -> Result<(Vec<u8>, u64), VecErr> {
        let c = self.find(id).ok_or(VecErr::Id)?;
        replay(c)
    }
}

/// 导出向量账文本：首行 `root <hex> <count>`，其后每行一条。
pub fn encode_set(s: &CaseSet) -> String {
    let mut out = format!("root {:016x} {}\n", s.root(), s.len());
    let mut i = 0usize;
    while i < s.cases().len() {
        if let Some(c) = s.cases().get(i) {
            out.push_str(&c.to_line());
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// 导入向量账文本：逐行重建条目（复核规范校验和），再核对条数与根。
pub fn parse_set(text: &str) -> Result<CaseSet, VecErr> {
    if text.is_empty() {
        return Err(VecErr::Empty);
    }
    let mut lines = text.split('\n');
    let head = match lines.next() {
        Some(h) => h,
        None => return Err(VecErr::Empty),
    };
    let mut hp = head.split(' ');
    let tag = hp.next().unwrap_or("");
    let root_s = hp.next().unwrap_or("");
    let count_s = hp.next().unwrap_or("");
    if tag != "root" || hp.next().is_some() || root_s.is_empty() || count_s.is_empty() {
        return Err(VecErr::Fields);
    }
    let declared_root = parse_hex(root_s).ok_or(VecErr::Hex)?;
    let declared_count = parse_hex(count_s).ok_or(VecErr::Hex)? as usize;
    let mut set = CaseSet::new();
    let mut seen = 0usize;
    loop {
        let line = match lines.next() {
            Some(x) => x,
            None => break,
        };
        if line.is_empty() {
            continue;
        }
        if seen >= CASE_CAP {
            return Err(VecErr::Capacity);
        }
        let mut f = line.split(',');
        let id_s = f.next().unwrap_or("");
        let fmt_s = f.next().unwrap_or("");
        let seed_s = f.next().unwrap_or("");
        let pages_s = f.next().unwrap_or("");
        let body_s = f.next().unwrap_or("");
        let cks_s = f.next().unwrap_or("");
        let dig_s = f.next().unwrap_or("");
        if f.next().is_some() {
            return Err(VecErr::Fields);
        }
        if id_s.is_empty()
            || fmt_s.is_empty()
            || seed_s.is_empty()
            || pages_s.is_empty()
            || body_s.is_empty()
            || cks_s.is_empty()
            || dig_s.is_empty()
        {
            return Err(VecErr::Fields);
        }
        let id = parse_hex(id_s).ok_or(VecErr::Hex)?;
        let seed = parse_hex(seed_s).ok_or(VecErr::Hex)?;
        let pages = parse_hex(pages_s).ok_or(VecErr::Hex)?;
        let body = parse_hex(body_s).ok_or(VecErr::Hex)?;
        let cks = parse_hex(cks_s).ok_or(VecErr::Hex)?;
        let dig = parse_hex(dig_s).ok_or(VecErr::Hex)?;
        if id > u32::MAX as u64 || seed > u32::MAX as u64 || pages > 255 || body > 65535 {
            return Err(VecErr::Range);
        }
        let fmt = VecFormat::from_tag(fmt_s)?;
        if set.find(id as u32).is_some() {
            return Err(VecErr::Dup);
        }
        let spec = CaseSpec::new(id as u32, fmt, seed as u32, pages as u8, body as u16)?;
        if spec.checksum() != cks {
            return Err(VecErr::Checksum);
        }
        let c = VectorCase {
            spec,
            checksum: cks,
            digest: dig,
        };
        let d = c.digest_entry();
        set.cases.push(c);
        set.root = fnv_u64(set.root, d);
        seen += 1;
    }
    if seen != declared_count {
        return Err(VecErr::Count);
    }
    if set.root != declared_root {
        return Err(VecErr::Checksum);
    }
    Ok(set)
}

fn spec(id: u32, fmt: VecFormat, seed: u32, pages: u8, body: u16) -> CaseSpec {
    match CaseSpec::new(id, fmt, seed, pages, body) {
        Ok(s) => s,
        Err(_) => CaseSpec {
            id: 1,
            format: VecFormat::Pdf,
            seed: 1,
            pages: 1,
            body_len: 64,
        },
    }
}

fn sample_set() -> CaseSet {
    let mut s = CaseSet::new();
    let _ = s.register(spec(1, VecFormat::Pdf, 0x11, 1, 128));
    let _ = s.register(spec(2, VecFormat::Ps, 0x22, 2, 256));
    let _ = s.register(spec(3, VecFormat::Pdf, 0x33, 4, 512));
    s
}

/// 注入核：11 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 11usize;

    // 1 空文本 → Empty
    if parse_set("").err() == Some(VecErr::Empty) {
        caught += 1;
    }
    // 2 字段数错 → Fields
    if parse_set("root 0 1\n1,pdf,1\n").err() == Some(VecErr::Fields) {
        caught += 1;
    }
    // 3 数值非十六进制 → Hex
    if parse_set("root 0 1\nzz,pdf,1,1,20,1,1\n").err() == Some(VecErr::Hex) {
        caught += 1;
    }
    // 4 未知格式 → Format
    if parse_set("root 0 1\n1,tiff,1,1,20,1,1\n").err() == Some(VecErr::Format) {
        caught += 1;
    }
    // 5 重复 id → Dup（首行合法，第二行同 id 在复核前就被挡下）
    if let Ok(c) = VectorCase::build(spec(1, VecFormat::Pdf, 3, 1, 64)) {
        let text = format!("root 0 2\n{}\n{}\n", c.to_line(), c.to_line());
        if parse_set(&text).err() == Some(VecErr::Dup) {
            caught += 1;
        }
    }
    // 6 条数不符 → Count（单条账头改成 2，根保持真实值）
    let mut one = CaseSet::new();
    let _ = one.register(spec(7, VecFormat::Pdf, 5, 1, 64));
    let enc = encode_set(&one);
    let mut text = format!("root {:016x} 2\n", one.root());
    if let Some(pos) = enc.find('\n') {
        if let Some(rest) = enc.get(pos + 1..) {
            text.push_str(rest);
        }
    }
    if parse_set(&text).err() == Some(VecErr::Count) {
        caught += 1;
    }
    // 7 账根不符 → Checksum
    let set3 = sample_set();
    let good = encode_set(&set3);
    let mut bad_root = format!("root 0000000000000000 {}", set3.len());
    if let Some(pos) = good.find('\n') {
        if let Some(rest) = good.get(pos..) {
            bad_root.push_str(rest);
        }
    }
    if parse_set(&bad_root).err() == Some(VecErr::Checksum) {
        caught += 1;
    }
    // 8 id 为 0 → Id
    if CaseSpec::new(0, VecFormat::Pdf, 1, 1, 64).err() == Some(VecErr::Id) {
        caught += 1;
    }
    // 9 页数 0 → Pages
    if CaseSpec::new(1, VecFormat::Pdf, 1, 0, 64).err() == Some(VecErr::Pages) {
        caught += 1;
    }
    // 10 正文过短 → Range
    if CaseSpec::new(1, VecFormat::Pdf, 1, 1, 8).err() == Some(VecErr::Range) {
        caught += 1;
    }
    // 11 回放摘要篡改 → Digest
    let mut c = match VectorCase::build(spec(21, VecFormat::Ps, 9, 3, 192)) {
        Ok(v) => v,
        Err(_) => VectorCase {
            spec: spec(21, VecFormat::Ps, 9, 3, 192),
            checksum: 0,
            digest: 0,
        },
    };
    c.digest ^= 1;
    if replay(&c).err() == Some(VecErr::Digest) {
        caught += 1;
    }
    (caught, total)
}

/// J1 判据：用例可复跑（13 条）。
pub fn run_unx50114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50114");

    // J1-1 回放两次逐位一致，且摘要等于登记值（可复跑的定义）。
    let c = VectorCase::build(spec(11, VecFormat::Pdf, 0x1234, 3, 300));
    let ok = match &c {
        Ok(v) => match (replay(v), replay(v)) {
            (Ok((a, da)), Ok((b, db))) => a == b && da == db && da == v.digest,
            _ => false,
        },
        Err(_) => false,
    };
    let det = match &c {
        Ok(v) => format!("digest={:016x}", v.digest),
        Err(_) => "build failed".to_string(),
    };
    s.add_dyn("50114-J1-回放复跑", ok, &det);

    // J1-2 格式头正确：PDF 以 %PDF- 起，PS 以 %!PS 起。
    let pdf = render(&spec(12, VecFormat::Pdf, 1, 1, 64));
    let ps = render(&spec(13, VecFormat::Ps, 1, 1, 64));
    let ok = match (pdf, ps) {
        (Ok(a), Ok(b)) => {
            a.starts_with(b"%PDF-1.7\n") && b.starts_with(b"%!PS-Adobe-3.0\n")
        }
        _ => false,
    };
    s.add("50114-J1-格式头", ok, if ok { "" } else { "格式头不符" });

    // J1-3 页标记数严格等于页数（3 页 → 3 个 %%Page:）。
    let three = render(&spec(14, VecFormat::Pdf, 7, 3, 96));
    let ok = match three {
        Ok(v) => {
            count_marker(&v, b"%%Page:") == 3
                && count_marker(&v, b"%%EOF") == 1
                && count_marker(&v, b"%%Page:") == 3
        }
        Err(_) => false,
    };
    s.add("50114-J1-页标记", ok, if ok { "" } else { "页标记计数不符" });

    // J1-4 字节守恒：总长 == 头 + 尾 + 正文 + 每页标记行 + 每页一个换行。
    let body_spec = spec(15, VecFormat::Ps, 3, 4, 400);
    let mut expect = VecFormat::Ps.header().len() + VecFormat::Ps.trailer().len();
    let mut p = 0usize;
    while p < 4 {
        expect += format!("%%Page: {} {}\n", p + 1, 4).len() + 1;
        p += 1;
    }
    let rendered = render(&body_spec);
    let ok = match &rendered {
        Ok(v) => {
            v.len() == expect + body_spec.body_len as usize
                && count_marker(v, b"%%Page:") == 4
        }
        Err(_) => false,
    };
    let det = match &rendered {
        Ok(v) => format!("len={} expect={}", v.len(), expect + 400),
        Err(_) => "render failed".to_string(),
    };
    s.add_dyn("50114-J1-字节守恒", ok, &det);

    // J1-5 种子绑定：换种子必换摘要（同一规范其余字段不动）。
    let a = VectorCase::build(spec(16, VecFormat::Pdf, 100, 2, 128));
    let b = VectorCase::build(spec(16, VecFormat::Pdf, 101, 2, 128));
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x.digest != y.digest && x.checksum != y.checksum,
        _ => false,
    };
    s.add("50114-J1-种子绑定", ok, if ok { "" } else { "换种子摘要不变" });

    // J1-6 账根顺序敏感：同三条不同顺序根不同。
    let set = sample_set();
    let mut reversed = CaseSet::new();
    let mut i = set.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = set.cases().get(i) {
            let _ = reversed.register(c.spec);
        }
    }
    let ok = set.len() == 3 && reversed.len() == 3 && set.root() != reversed.root();
    let det = format!("r={:016x} rr={:016x}", set.root(), reversed.root());
    s.add_dyn("50114-J1-根顺序敏感", ok, &det);

    // J1-7 文本往返：根、条数、逐条条目逐位一致。
    let text = encode_set(&set);
    let back = parse_set(&text);
    let ok = match back {
        Ok(p) => p.root() == set.root() && p.len() == set.len() && p.cases() == set.cases(),
        Err(_) => false,
    };
    let det = format!("len={} root={:016x}", set.len(), set.root());
    s.add_dyn("50114-J1-账本往返", ok, &det);

    // J1-8 重复 id 拒绝且账守恒。
    let mut dup = sample_set();
    let before = dup.root();
    let again = dup.register(spec(2, VecFormat::Ps, 0x22, 2, 256));
    let ok = again.err() == Some(VecErr::Dup) && dup.root() == before && dup.len() == 3;
    s.add("50114-J1-重复拒绝", ok, if ok { "" } else { "重复入账" });

    // J1-9 容量 24：第 25 条拒绝，账根不动。
    let mut cap = CaseSet::new();
    let mut acc = 0usize;
    let mut k = 0u32;
    while k < CASE_CAP as u32 {
        if cap.register(spec(100 + k, VecFormat::Pdf, k + 1, 1, 64)).is_ok() {
            acc += 1;
        }
        k += 1;
    }
    let before = cap.root();
    let over = cap.register(spec(9999, VecFormat::Ps, 1, 1, 64));
    let ok = acc == CASE_CAP
        && over.err() == Some(VecErr::Capacity)
        && cap.len() == CASE_CAP
        && cap.root() == before;
    let det = format!("acc={} len={}", acc, cap.len());
    s.add_dyn("50114-J1-容量封顶", ok, &det);

    // J1-10 确定性：同一批规范两次建账根逐位一致。
    let x = sample_set();
    let y = sample_set();
    let ok = x.root() == y.root() && x.cases() == y.cases() && x.len() == y.len();
    let det = format!("r={:016x}", x.root());
    s.add_dyn("50114-J1-建账确定", ok, &det);

    // J1-11 账内每条都可回放，且回放摘要等于登记摘要。
    let mut all = true;
    let mut i = 0usize;
    while i < x.len() {
        match x.cases().get(i) {
            Some(c) => match replay(c) {
                Ok((_, d)) => {
                    if d != c.digest {
                        all = false;
                    }
                }
                Err(_) => all = false,
            },
            None => all = false,
        }
        i += 1;
    }
    s.add("50114-J1-全量回放", all, if all { "" } else { "有向量不可回放" });

    // J1-12 未知 id 回放拒绝（Id）。
    let ok = x.replay_id(424242).err() == Some(VecErr::Id);
    s.add("50114-J1-未知用例", ok, if ok { "" } else { "未知 id 被接受" });

    // J1-13 注入核 11/11。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50114-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50114_checks_green() {
        let set = run_unx50114_checks();
        assert!(set.all_passed(), "UNX-F50114 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50114_replay_reproducible() {
        let c = VectorCase::build(spec(31, VecFormat::Pdf, 77, 2, 128));
        match c {
            Ok(v) => {
                let a = replay(&v);
                let b = replay(&v);
                match (a, b) {
                    (Ok((x, dx)), Ok((y, dy))) => {
                        assert_eq!(x, y);
                        assert_eq!(dx, dy);
                    }
                    _ => panic!("replay failed"),
                }
            }
            Err(_) => panic!("build failed"),
        }
    }

    #[test]
    fn unx50114_set_roundtrip() {
        let s = sample_set();
        let text = encode_set(&s);
        match parse_set(&text) {
            Ok(p) => assert_eq!(p.root(), s.root()),
            Err(_) => panic!("set rejected"),
        }
    }
}
