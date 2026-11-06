//! UNX-F11540 B17 批 ktest 断言集聚合
//!
//! B17 批一共两张卡：11538（哈希族）与 11539（校验工具的 stdin 档）。两张
//! 卡各自有判据，但判据散在两个模块里，跑一遍要看两处输出，接的人容易漏
//! 一条。这单把本批 **十九条判据** 收进同一个断言集，做成 ktest 生态面上的
//! 一次命令全跑：跑完给出总数、通过数、失败数与通过率，十九条全绿通过率
//! 必须正好是 100%，少一条就不算绿。
//!
//! 这单不做哈希、不做参数解析，做的是**聚合**：
//!
//! 1. **断言模型**。一条断言记四个事实：编号（本批内 1..=19）、卡号
//!    （11538 或 11539）、分组（哈希 / 标准输入）、结果（过或不过）。编号
//!    必须严格递增，前十条归哈希组、后九条归标准输入组；卡号与分组必须
//!    一一对上，否则编号再对也是脏数据。
//! 2. **集合闸门**。入集时按次序卡五道：卡号与分组不符给 `KT_ECARD`；
//!    编号越界（0 或超过 `CASE_CAP`）给 `KT_ESLOT`；与上一条编号相同给
//!    `KT_EDUP`；比上一条小给 `KT_ESEQ`；集满十九条再塞给 `KT_ESLOT`。
//! 3. **聚合运行**。`measure` 只统计不判定，空集也能给出四元组；`run` 是
//!    ktest 语义：空集 `KT_ESHORT`，只要有一条没过就 `KT_EFAIL`，全过才
//!    返回报告。通过率是整数百分比 `passed * 100 / total`，十九条全过时
//!    恰好 100。
//! 4. **判据本体**。十九条断言的布尔值全部由真调用算出：哈希组十条覆盖
//!    SHA-1/BLAKE2b 的已知向量、流式与一次性一致、普通行与标签行的往返、
//!    算法名反推、摘要长度与大小写；标准输入组九条覆盖参数分类、目标顺序、
//!    冲突、`-` 档名、stdin 与文件同摘要、三种渲染、核验计数与缺失。
//!
//! 闸门次序：卡号（`KT_ECARD`）→ 编号范围（`KT_ESLOT`）→ 编号重复
//! （`KT_EDUP`）→ 编号乱序（`KT_ESEQ`）→ 容量（`KT_ESLOT`）→ 空集
//! （`KT_ESHORT`）→ 判据未过（`KT_EFAIL`）→ 状态（`KT_ESTATE`）。
//!
//! 账本语义：十四格 = 八格成功（断言数、通过数、通过率、卡号数、编号和、
//! 哈希条数、标准输入条数、运行总数）+ 六格失败（未知卡号、编号重复、
//! 编号乱序、判据未过、空集、容量溢出）。`audit` 认「八值六码」，
//! `seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

use super::unx11538_hash_family::{
    Algorithm, Digest, Entry, Hasher, HS_ERANGE,
};
use super::unx11539_stdin_dash::{
    prepare, render_plan, same_digest, sample_registry, verify, Opt, Registry, Target, SD_ECONF,
    SD_EDUP, SD_EINVAL, SD_ESHORT, SAMPLE_CHECK_FILE,
};

/// 段基址。
pub const SEG_BASE: u32 = 0xf2d0;

/// 卡号未知或与分组不符。
pub const KT_ECARD: u32 = SEG_BASE + 0x01;
/// 编号重复。
pub const KT_EDUP: u32 = SEG_BASE + 0x02;
/// 编号乱序。
pub const KT_ESEQ: u32 = SEG_BASE + 0x03;
/// 判据未过。
pub const KT_EFAIL: u32 = SEG_BASE + 0x04;
/// 断言集为空。
pub const KT_ESHORT: u32 = SEG_BASE + 0x05;
/// 名称过长。
pub const KT_ELEN: u32 = SEG_BASE + 0x06;
/// 名称字节非法。
pub const KT_EBYTE: u32 = SEG_BASE + 0x07;
/// 状态冲突。
pub const KT_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const KT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位或容量溢出。
pub const KT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const KT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const KT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const KT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 本批断言总数。
pub const CASE_CAP: usize = 19;

/// 哈希组条数。
pub const HASH_CASES: usize = 10;

/// 标准输入组条数。
pub const STDIN_CASES: usize = 9;

/// 哈希卡号。
pub const HASH_CARD: u16 = 11538;

/// 标准输入卡号。
pub const STDIN_CARD: u16 = 11539;

/// 名字容量。
pub const NAME_CAP: usize = 32;

/// 操作码：建集。
pub const OP_BUILD: u8 = 0x01;
/// 操作码：入集。
pub const OP_PUSH: u8 = 0x02;
/// 操作码：运行。
pub const OP_RUN: u8 = 0x03;
/// 操作码：统计。
pub const OP_MEASURE: u8 = 0x04;
/// 操作码：清点。
pub const OP_MANIFEST: u8 = 0x05;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 0x06;
/// 操作码：容量。
pub const OP_SLOT: u8 = 0x07;
/// 操作码：账本。
pub const OP_LEDGER: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        if let Some(b) = buf.get(j) {
            s.push(*b as char);
        }
        j += 1;
    }
}

/// 字节比较。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 分组
// ---------------------------------------------------------------------------

/// 断言分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// 哈希族（11538）。
    Hash,
    /// 标准输入档（11539）。
    Stdin,
}

impl Group {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Group::Hash => "hash",
            Group::Stdin => "stdin",
        }
    }

    /// 组内代号。
    pub fn tag(&self) -> u8 {
        match self {
            Group::Hash => 1,
            Group::Stdin => 2,
        }
    }

    /// 归属卡号。
    pub fn card(&self) -> u16 {
        match self {
            Group::Hash => HASH_CARD,
            Group::Stdin => STDIN_CARD,
        }
    }
}

// ---------------------------------------------------------------------------
// 断言
// ---------------------------------------------------------------------------

/// 一条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    id: u16,
    card: u16,
    group: Group,
    ok: bool,
}

impl Case {
    /// 空白断言。
    pub const fn blank() -> Case {
        Case { id: 0, card: 0, group: Group::Hash, ok: false }
    }

    /// 按分组构造；卡号由分组决定。
    pub fn new(id: u16, group: Group, ok: bool) -> Case {
        Case { id, card: group.card(), group, ok }
    }

    /// 按外部卡号构造，用于验证卡号闸门。
    pub fn with_card(id: u16, card: u16, group: Group, ok: bool) -> Case {
        Case { id, card, group, ok }
    }

    /// 编号。
    pub fn id(&self) -> u16 {
        self.id
    }

    /// 卡号。
    pub fn card(&self) -> u16 {
        self.card
    }

    /// 分组。
    pub fn group(&self) -> Group {
        self.group
    }

    /// 结果。
    pub fn ok(&self) -> bool {
        self.ok
    }

    /// 卡号与分组是否自洽。
    pub fn valid(&self) -> bool {
        self.card == self.group.card() && self.id >= 1 && (self.id as usize) <= CASE_CAP
    }

    /// 名称，形如 `11538#01`。
    pub fn label(&self) -> String {
        let mut s = String::new();
        push_dec(&mut s, self.card as u64);
        s.push('#');
        if self.id < 10 {
            s.push('0');
        }
        push_dec(&mut s, self.id as u64);
        s
    }

    /// 一行结果，形如 `[ok] 11538#01 hash`。
    pub fn line(&self) -> String {
        let mut s = String::new();
        s.push('[');
        s.push_str(if self.ok { "ok" } else { "fail" });
        s.push(']');
        s.push(' ');
        s.push_str(self.label().as_str());
        s.push(' ');
        s.push_str(self.group.text());
        s
    }
}

// ---------------------------------------------------------------------------
// 断言集
// ---------------------------------------------------------------------------

/// 有序断言集。
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    rows: [Case; CASE_CAP],
    len: usize,
}

impl Suite {
    /// 空集。
    pub const fn new() -> Suite {
        Suite { rows: [Case::blank(); CASE_CAP], len: 0 }
    }

    /// 入集。次序：卡号 → 编号范围 → 容量 → 重复 → 乱序。
    pub fn push(&mut self, c: Case) -> Result<usize, u32> {
        if c.card != c.group.card() {
            return Err(KT_ECARD);
        }
        if c.id == 0 || (c.id as usize) > CASE_CAP {
            return Err(KT_ESLOT);
        }
        if self.len >= CASE_CAP {
            return Err(KT_ESLOT);
        }
        if self.len > 0 {
            if let Some(prev) = self.rows.get(self.len - 1) {
                if c.id == prev.id {
                    return Err(KT_EDUP);
                }
                if c.id < prev.id {
                    return Err(KT_ESEQ);
                }
            }
        }
        let at = self.len;
        if let Some(dst) = self.rows.get_mut(at) {
            *dst = c;
        }
        self.len += 1;
        Ok(at)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
    pub fn at(&self, i: usize) -> Option<Case> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 首条。
    pub fn first(&self) -> Option<Case> {
        self.at(0)
    }

    /// 末条。
    pub fn last(&self) -> Option<Case> {
        if self.len == 0 {
            return None;
        }
        self.at(self.len - 1)
    }

    /// 按编号取。
    pub fn by_id(&self, id: u16) -> Option<Case> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                if c.id == id {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 组内条数。
    pub fn group_count(&self, g: Group) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                if c.group == g {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 通过条数。
    pub fn passed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                if c.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 未过条数。
    pub fn failed(&self) -> usize {
        self.len - self.passed()
    }

    /// 编号和。
    pub fn id_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                n = n.saturating_add(c.id as u32);
            }
            i += 1;
        }
        n
    }

    /// 不同卡号数（按相邻去重计数）。
    pub fn card_count(&self) -> usize {
        if self.len == 0 {
            return 0;
        }
        let mut n = 1usize;
        let mut i = 1usize;
        while i < self.len {
            let a = self.rows.get(i - 1).map(|c| c.card).unwrap_or(0);
            let b = self.rows.get(i).map(|c| c.card).unwrap_or(0);
            if a != b {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 首条未过。
    pub fn first_failed(&self) -> Option<Case> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                if !c.ok {
                    return Some(*c);
                }
            }
            i += 1;
        }
        None
    }

    /// 清单是否合规：编号 1..=19 递增，前十哈希、后九标准输入，卡号对齐。
    pub fn manifest_ok(&self) -> bool {
        if self.len != CASE_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let c = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if c.id as usize != i + 1 {
                return false;
            }
            let want = if i < HASH_CASES { Group::Hash } else { Group::Stdin };
            if c.group != want || c.card != want.card() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 统计。空集给出全零，不报错。
    pub fn measure(&self) -> Report {
        let total = self.len as u32;
        let passed = self.passed() as u32;
        let failed = total.saturating_sub(passed);
        let rate = if total == 0 { 0 } else { passed.saturating_mul(100) / total };
        Report {
            total,
            passed,
            failed,
            rate,
            hash: self.group_count(Group::Hash) as u32,
            stdin: self.group_count(Group::Stdin) as u32,
        }
    }

    /// ktest 语义运行：空集短，有一条没过即败，全过才给报告。
    pub fn run(&self) -> Result<Report, u32> {
        if self.is_empty() {
            return Err(KT_ESHORT);
        }
        let rep = self.measure();
        if rep.failed > 0 {
            return Err(KT_EFAIL);
        }
        Ok(rep)
    }

    /// 渲染全部断言行。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.rows.get(i) {
                s.push_str(c.line().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }

    /// 是否接受该卡号。
    pub fn accepts_card(card: u16) -> bool {
        card == HASH_CARD || card == STDIN_CARD
    }
}

// ---------------------------------------------------------------------------
// 运行报告
// ---------------------------------------------------------------------------

/// 聚合统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 通过。
    pub passed: u32,
    /// 未过。
    pub failed: u32,
    /// 通过率（整数百分比）。
    pub rate: u32,
    /// 哈希组条数。
    pub hash: u32,
    /// 标准输入组条数。
    pub stdin: u32,
}

impl Report {
    /// 空白。
    pub const fn blank() -> Report {
        Report { total: 0, passed: 0, failed: 0, rate: 0, hash: 0, stdin: 0 }
    }

    /// 是否全绿。
    pub fn all_green(&self) -> bool {
        self.total > 0 && self.failed == 0 && self.passed == self.total && self.rate == 100
    }

    /// 三行文本：总账 + 两组分部。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("total ");
        push_dec(&mut s, self.total as u64);
        s.push_str(" passed ");
        push_dec(&mut s, self.passed as u64);
        s.push_str(" failed ");
        push_dec(&mut s, self.failed as u64);
        s.push_str(" rate ");
        push_dec(&mut s, self.rate as u64);
        s.push('%');
        s.push('\n');
        s.push_str("hash ");
        push_dec(&mut s, self.passed_of(self.hash) as u64);
        s.push('/');
        push_dec(&mut s, self.hash as u64);
        s.push('\n');
        s.push_str("stdin ");
        push_dec(&mut s, self.passed_of(self.stdin) as u64);
        s.push('/');
        push_dec(&mut s, self.stdin as u64);
        s.push('\n');
        s
    }

    /// 组内通过数。全绿时等于组内条数；有失败时按总数比例保守折算。
    fn passed_of(&self, group_total: u32) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let scaled = self.passed.saturating_mul(group_total) / self.total;
        if scaled > group_total {
            group_total
        } else {
            scaled
        }
    }
}

/// ktest 一次命令。
pub fn command_line() -> String {
    let mut s = String::new();
    s.push_str("ktest run b17.all");
    s
}

/// 一次命令的一行回显。
pub fn command_echo() -> String {
    let mut s = String::new();
    s.push_str("$ ");
    s.push_str(command_line().as_str());
    s
}

// ---------------------------------------------------------------------------
// 十九条断言
// ---------------------------------------------------------------------------

/// SHA-1("abc")。
const SHA1_ABC: &[u8] = b"a9993e364706816aba3e25717850c26c9cd0d89d";

/// SHA-1("")。
const SHA1_EMPTY: &[u8] = b"da39a3ee5e6b4b0d3255bfef95601890afd80709";

/// BLAKE2b-256("abc")。
const B2B256_ABC: &[u8] = b"bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319";

/// BLAKE2b-512("abc")。
const B2B512_ABC: &[u8] =
    b"ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923";

/// 流式样例内容。
const STREAM_DATA: &[u8] = b"streaming-b17";

/// 摘要是否等于给定十六进制。
fn digest_is(alg: Algorithm, data: &[u8], want: &[u8]) -> bool {
    match alg.digest(data) {
        Ok(d) => d.hex_is(want),
        Err(_) => false,
    }
}

/// 分片喂入的摘要。
fn stream_digest(alg: Algorithm, data: &[u8], chunks: &[usize]) -> Option<Digest> {
    let mut h = match Hasher::new(alg) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let mut at = 0usize;
    let mut ci = 0usize;
    while at < data.len() {
        let step = match chunks.get(ci % chunks.len()) {
            Some(v) => *v,
            None => return None,
        };
        let end = if at + step > data.len() { data.len() } else { at + step };
        let piece = match data.get(at..end) {
            Some(v) => v,
            None => return None,
        };
        if h.update(piece).is_err() {
            return None;
        }
        at = end;
        ci += 1;
    }
    h.finish().ok()
}

/// 哈希组第 n 条（1..=10）。
fn hash_case(n: u16) -> Case {
    let ok = match n {
        1 => digest_is(Algorithm::Sha1, b"abc", SHA1_ABC),
        2 => digest_is(Algorithm::Sha1, b"", SHA1_EMPTY),
        3 => digest_is(Algorithm::Blake2b256, b"abc", B2B256_ABC),
        4 => digest_is(Algorithm::Blake2b512, b"abc", B2B512_ABC),
        5 => {
            let one = match Algorithm::Sha1.digest(STREAM_DATA) {
                Ok(v) => v,
                Err(_) => Digest::blank(),
            };
            let chunks: [usize; 4] = [1, 2, 4, 7];
            match stream_digest(Algorithm::Sha1, STREAM_DATA, &chunks) {
                Some(v) => v.equals(&one),
                None => false,
            }
        }
        6 => {
            let one = match Algorithm::Blake2b512.digest(STREAM_DATA) {
                Ok(v) => v,
                Err(_) => Digest::blank(),
            };
            let chunks: [usize; 3] = [3, 5, 8];
            match stream_digest(Algorithm::Blake2b512, STREAM_DATA, &chunks) {
                Some(v) => v.equals(&one),
                None => false,
            }
        }
        7 => {
            let e = match Entry::from_content(Algorithm::Sha1, b"payload", b"round.txt", false) {
                Ok(v) => v,
                Err(_) => Entry::blank(),
            };
            let line = e.render();
            match Entry::parse(line.as_str().as_bytes()) {
                Ok(p) => {
                    !p.tagged()
                        && p.alg() == Algorithm::Sha1
                        && eq_bytes(p.name(), b"round.txt")
                        && p.digest().equals(e.digest())
                }
                Err(_) => false,
            }
        }
        8 => {
            let e = match Entry::from_content(Algorithm::Blake2b256, b"tagged", b"tag.bin", true) {
                Ok(v) => v,
                Err(_) => Entry::blank(),
            };
            let line = e.tag_line();
            match Entry::parse(line.as_str().as_bytes()) {
                Ok(p) => {
                    p.tagged()
                        && p.alg() == Algorithm::Blake2b256
                        && eq_bytes(p.name(), b"tag.bin")
                        && p.digest().equals(e.digest())
                }
                Err(_) => false,
            }
        }
        9 => {
            let a = Algorithm::from_hexlen(40).ok() == Some(Algorithm::Sha1);
            let b = Algorithm::from_hexlen(64).ok() == Some(Algorithm::Blake2b256);
            let c = Algorithm::from_hexlen(128).ok() == Some(Algorithm::Blake2b512);
            let d = Algorithm::from_hexlen(41).err() == Some(HS_ERANGE);
            let e = Algorithm::from_hexlen(0).err() == Some(HS_ERANGE);
            a && b && c && d && e
        }
        10 => match Algorithm::Sha1.digest(b"abc") {
            Ok(d) => {
                let hex = d.hex();
                let up = d.upper();
                d.len() == 20
                    && d.hexlen() == 40
                    && hex.len() == 40
                    && up.len() == 40
                    && d.byte(0) == Some(0xa9)
                    && d.hex_is(SHA1_ABC)
                    && up.as_str() != hex.as_str()
            }
            Err(_) => false,
        },
        _ => false,
    };
    Case::new(n, Group::Hash, ok)
}

/// 标准输入组第 n 条（11..=19，本地号 m = n - 10 取 1..=9）。
fn stdin_case(n: u16) -> Case {
    let m = n.saturating_sub(HASH_CASES as u16);
    let ok = match m {
        1 => {
            classify_is(b"-", Opt::Stdin)
                && classify_is(b"--", Opt::End)
                && classify_is(b"--tag", Opt::Tag)
                && classify_is(b"data.txt", Opt::File)
                && classify_err(b"--wat", SD_EINVAL)
        }
        2 => match prepare(&[b"alpha", b"-", b"beta"]) {
            Ok(p) => {
                p.len() == 3
                    && p.stdin_targets() == 1
                    && p.at(0).map(|t| t.named(b"alpha")).unwrap_or(false)
                    && p.at(1).map(|t| t.is_stdin()).unwrap_or(false)
                    && p.at(2).map(|t| t.named(b"beta")).unwrap_or(false)
            }
            Err(_) => false,
        },
        3 => {
            prepare(&[b"--binary", b"--text", b"-"]).err() == Some(SD_ECONF)
                && prepare(&[b"--quiet", b"-"]).err() == Some(SD_ECONF)
                && prepare(&[b"--tag", b"--tag", b"-"]).err() == Some(SD_EDUP)
        }
        4 => {
            let t = Target::stdin();
            let a = t.is_stdin() && t.name_text().as_str() == "-" && t.named(b"-");
            let b = Target::path(b"").err() == Some(SD_ESHORT);
            let c = Target::path(b"data.txt")
                .map(|p| !p.is_stdin() && p.named(b"data.txt"))
                .unwrap_or(false);
            a && b && c
        }
        5 => {
            let r = sample_registry();
            let f = match Target::path(b"data.txt") {
                Ok(v) => v,
                Err(_) => Target::blank(),
            };
            let st = Target::stdin();
            same_digest(&f, &st, Algorithm::Sha1, &r) == Ok(true)
                && same_digest(&f, &st, Algorithm::Blake2b256, &r) == Ok(true)
                && same_digest(&f, &st, Algorithm::Blake2b512, &r) == Ok(true)
        }
        6 => {
            let r = sample_registry();
            let p = match prepare(&[b"--tag", b"-"]) {
                Ok(v) => v,
                Err(_) => return Case::new(n, Group::Stdin, false),
            };
            match render_plan(&p, Algorithm::Sha1, &r) {
                Ok(out) => match Entry::parse(out.as_str().as_bytes()) {
                    Ok(e) => {
                        e.tagged()
                            && e.alg() == Algorithm::Sha1
                            && e.name_text().as_str() == "-"
                            && out.as_str().starts_with("SHA1 (-) = ")
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        }
        7 => {
            let r = sample_registry();
            let p = match prepare(&[b"--binary", b"data.txt"]) {
                Ok(v) => v,
                Err(_) => return Case::new(n, Group::Stdin, false),
            };
            match render_plan(&p, Algorithm::Sha1, &r) {
                Ok(out) => {
                    let star = out.as_str().contains(" *data.txt");
                    let back = match Entry::parse(out.as_str().as_bytes()) {
                        Ok(e) => !e.tagged() && e.name_text().as_str() == "data.txt",
                        Err(_) => false,
                    };
                    star && back
                }
                Err(_) => false,
            }
        }
        8 => {
            let mut r = Registry::new();
            if r.insert(b"data.txt", b"abc").is_err() || r.set_stdin(b"abc").is_err() {
                return Case::new(n, Group::Stdin, false);
            }
            if r.insert(b"check.txt", SAMPLE_CHECK_FILE).is_err() {
                return Case::new(n, Group::Stdin, false);
            }
            let p = match prepare(&[b"--check", b"check.txt"]) {
                Ok(v) => v,
                Err(_) => return Case::new(n, Group::Stdin, false),
            };
            match verify(&p, &r) {
                Ok(rep) => rep.checked == 2 && rep.good == 2 && rep.all_good(),
                Err(_) => false,
            }
        }
        9 => {
            let mut r = Registry::new();
            if r
                .insert(
                    b"check.txt",
                    b"a9993e364706816aba3e25717850c26c9cd0d89d  gone\n",
                )
                .is_err()
            {
                return Case::new(n, Group::Stdin, false);
            }
            let p = match prepare(&[b"--check", b"check.txt"]) {
                Ok(v) => v,
                Err(_) => return Case::new(n, Group::Stdin, false),
            };
            match verify(&p, &r) {
                Ok(rep) => rep.checked == 1 && rep.missing == 1 && rep.good == 0 && !rep.all_good(),
                Err(_) => false,
            }
        }
        _ => false,
    };
    Case::new(n, Group::Stdin, ok)
}

/// 参数分类是否等于期望。
fn classify_is(arg: &[u8], want: Opt) -> bool {
    match super::unx11539_stdin_dash::classify(arg) {
        Ok(v) => v == want,
        Err(_) => false,
    }
}

/// 参数分类是否等于期望错误。
fn classify_err(arg: &[u8], want: u32) -> bool {
    super::unx11539_stdin_dash::classify(arg).err() == Some(want)
}

/// 建集成十九条。
pub fn build_suite() -> Suite {
    let mut s = Suite::new();
    let mut i = 1u16;
    while i <= HASH_CASES as u16 {
        let _ = s.push(hash_case(i));
        i += 1;
    }
    while i <= CASE_CAP as u16 {
        let _ = s.push(stdin_case(i));
        i += 1;
    }
    s
}

/// 一条判据故意翻红，用于失败汇聚。
pub fn failing_suite() -> Suite {
    let base = build_suite();
    let mut s = Suite::new();
    let mut i = 0usize;
    while i < base.len() {
        if let Some(c) = base.at(i) {
            let flip = if c.id() == 7 { false } else { c.ok() };
            let _ = s.push(Case::new(c.id(), c.group(), flip));
        }
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 账本观测
// ---------------------------------------------------------------------------

/// 样例断言集。
fn sample_suite() -> Result<Suite, u32> {
    let s = build_suite();
    if !s.manifest_ok() {
        return Err(KT_ESTATE);
    }
    Ok(s)
}

/// 样例运行。
fn sample_run() -> Result<Report, u32> {
    let s = sample_suite()?;
    s.run()
}

/// 编号重复探针。
fn dup_probe() -> Result<u32, u32> {
    let mut s = Suite::new();
    let _ = s.push(Case::new(1, Group::Hash, true));
    s.push(Case::new(1, Group::Hash, true)).map(|_| 0)
}

/// 编号乱序探针。
fn seq_probe() -> Result<u32, u32> {
    let mut s = Suite::new();
    let _ = s.push(Case::new(5, Group::Hash, true));
    s.push(Case::new(3, Group::Hash, true)).map(|_| 0)
}

/// 未知卡号探针。
fn card_probe() -> Result<u32, u32> {
    let mut s = Suite::new();
    s.push(Case::with_card(1, 11540, Group::Hash, true)).map(|_| 0)
}

/// 判据未过探针。
fn fail_probe() -> Result<u32, u32> {
    failing_suite().run().map(|r| r.failed)
}

/// 空集探针。
fn empty_probe() -> Result<u32, u32> {
    Suite::new().run().map(|r| r.total)
}

/// 容量探针：集满后再塞。
fn slot_probe() -> Result<u32, u32> {
    let mut s = Suite::new();
    let mut i = 1u16;
    while i <= CASE_CAP as u16 {
        let g = if (i as usize) <= HASH_CASES { Group::Hash } else { Group::Stdin };
        let _ = s.push(Case::new(i, g, true));
        i += 1;
    }
    s.push(Case::with_card(1, HASH_CARD, Group::Hash, true)).map(|_| 0)
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KT_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [19, 19, 100, 2, 190, 10, 9, 19];
        const CODES: [u32; 6] = [KT_ECARD, KT_EDUP, KT_ESEQ, KT_EFAIL, KT_ESHORT, KT_ESLOT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KT_EBAL),
            };
            if s.slot as usize != i {
                return Err(KT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KT_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KT_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(KT_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_BUILD, sample_suite().map(|s| s.len())));

    let _ = l.register(count_cell(1, OP_RUN, sample_run().map(|r| r.passed as usize)));

    let _ = l.register(word_cell(2, OP_RUN, sample_run().map(|r| r.rate)));

    let _ = l.register(count_cell(3, OP_MANIFEST, sample_suite().map(|s| s.card_count())));

    let _ = l.register(word_cell(4, OP_BUILD, sample_suite().map(|s| s.id_sum())));

    let _ = l.register(count_cell(5, OP_BUILD, sample_suite().map(|s| s.group_count(Group::Hash))));

    let _ = l.register(count_cell(6, OP_BUILD, sample_suite().map(|s| s.group_count(Group::Stdin))));

    let _ = l.register(count_cell(7, OP_MEASURE, sample_suite().map(|s| s.len())));

    let _ = l.register(word_cell(8, OP_PUSH, card_probe()));

    let _ = l.register(word_cell(9, OP_PUSH, dup_probe()));

    let _ = l.register(word_cell(10, OP_PUSH, seq_probe()));

    let _ = l.register(word_cell(11, OP_RUN, fail_probe()));

    let _ = l.register(word_cell(12, OP_RUN, empty_probe()));

    let _ = l.register(word_cell(13, OP_SLOT, slot_probe()));

    l
}

// ---------------------------------------------------------------------------
// 注入判据
// ---------------------------------------------------------------------------

/// 构成与清单。
fn composition_ok() -> bool {
    let s = build_suite();
    let a = s.len() == CASE_CAP;
    let b = s.manifest_ok();
    let c = s.group_count(Group::Hash) == HASH_CASES;
    let d = s.group_count(Group::Stdin) == STDIN_CASES;
    let e = s.id_sum() == 190;
    let f = s.card_count() == 2;
    let g = s.first().map(|c| c.id()).unwrap_or(0) == 1;
    let h = s.last().map(|c| c.id()).unwrap_or(0) == CASE_CAP as u16;
    a && b && c && d && e && f && g && h
}

/// 编号闸门。
fn numbering_gates_ok() -> bool {
    let a = dup_probe().err() == Some(KT_EDUP);
    let b = seq_probe().err() == Some(KT_ESEQ);
    let c = {
        let mut s = Suite::new();
        s.push(Case::with_card(0, HASH_CARD, Group::Hash, true)).err() == Some(KT_ESLOT)
    };
    let d = {
        let mut s = Suite::new();
        s.push(Case::new(20, Group::Hash, true)).err() == Some(KT_ESLOT)
    };
    let e = slot_probe().err() == Some(KT_ESLOT);
    a && b && c && d && e
}

/// 卡号闸门。
fn card_gates_ok() -> bool {
    let a = card_probe().err() == Some(KT_ECARD);
    let b = Suite::accepts_card(HASH_CARD);
    let c = Suite::accepts_card(STDIN_CARD);
    let d = !Suite::accepts_card(11540);
    let e = {
        let mut s = Suite::new();
        let ok = s.push(Case::with_card(1, STDIN_CARD, Group::Hash, true)).is_ok();
        !ok
    };
    a && b && c && d && e
}

/// 哈希十条。
fn hash_ten_ok() -> bool {
    let mut n = 1u16;
    while n <= HASH_CASES as u16 {
        if !hash_case(n).ok() {
            return false;
        }
        n += 1;
    }
    true
}

/// 标准输入九条。
fn stdin_nine_ok() -> bool {
    let mut n = (HASH_CASES as u16) + 1;
    while n <= CASE_CAP as u16 {
        if !stdin_case(n).ok() {
            return false;
        }
        n += 1;
    }
    true
}

/// 一次命令与报告。
fn run_report_ok() -> bool {
    let s = build_suite();
    let rep = match s.run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let text = rep.text();
    let lines = text.as_str().matches('\n').count();
    command_echo().as_str() == "$ ktest run b17.all"
        && command_line().as_str() == "ktest run b17.all"
        && rep.all_green()
        && rep.total == CASE_CAP as u32
        && rep.passed == CASE_CAP as u32
        && rep.failed == 0
        && rep.rate == 100
        && text.as_str().starts_with("total 19 passed 19 failed 0 rate 100%")
        && lines == 3
        && s.render().as_str().contains("[ok] 11538#01 hash")
        && s.render().as_str().contains("[ok] 11539#19 stdin")
}

/// 失败汇聚。
fn failure_gates_ok() -> bool {
    let bad = failing_suite();
    let rep = bad.measure();
    let a = bad.len() == CASE_CAP;
    let b = rep.failed == 1;
    let c = rep.passed == (CASE_CAP as u32) - 1;
    let d = rep.rate == 94;
    let e = bad.run().err() == Some(KT_EFAIL);
    let f = bad.first_failed().map(|c| c.id()).unwrap_or(0) == 7;
    let g = Suite::new().run().err() == Some(KT_ESHORT);
    let h = Suite::new().measure().total == 0 && Suite::new().measure().rate == 0;
    let i = {
        let mut s = Suite::new();
        let ok = s.push(Case::new(1, Group::Hash, false)).is_ok();
        ok && s.run().err() == Some(KT_EFAIL)
    };
    a && b && c && d && e && f && g && h && i
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l2.register(probe) != Err(KT_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = composition_ok() && numbering_gates_ok() && card_gates_ok();
    out[1] = hash_ten_ok() && stdin_nine_ok();
    out[2] = ledger_ok();
    out[3] = run_report_ok() && failure_gates_ok();
    out
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据集合。
pub fn run_unx11540_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11540.ktest");

    s.add(
        "KT01-断言集构成",
        {
            composition_ok()
        },
        "static",
    );

    s.add(
        "KT02-编号闸门",
        {
            numbering_gates_ok()
        },
        "static",
    );

    s.add(
        "KT03-卡号闸门",
        {
            card_gates_ok()
        },
        "static",
    );

    s.add(
        "KT04-哈希十条",
        {
            hash_ten_ok()
        },
        "static",
    );

    s.add(
        "KT05-标准输入九条",
        {
            stdin_nine_ok()
        },
        "static",
    );

    s.add(
        "KT06-一次命令",
        {
            let s = build_suite();
            let rep = match s.run() {
                Ok(v) => v,
                Err(_) => Report::blank(),
            };
            command_line().as_str() == "ktest run b17.all"
                && command_echo().as_str() == "$ ktest run b17.all"
                && rep.all_green()
                && s.render().as_str().contains("[ok] ")
        },
        "static",
    );

    s.add(
        "KT07-通过率",
        {
            let rep = build_suite().measure();
            let bad = failing_suite().measure();
            rep.rate == 100
                && rep.passed == rep.total
                && rep.failed == 0
                && bad.rate == 94
                && bad.failed == 1
                && Report::blank().rate == 0
        },
        "static",
    );

    s.add(
        "KT08-失败汇聚",
        {
            let bad = failing_suite();
            bad.run().err() == Some(KT_EFAIL)
                && bad.first_failed().map(|c| c.id()).unwrap_or(0) == 7
                && bad.measure().failed == 1
                && Suite::new().run().err() == Some(KT_ESHORT)
        },
        "static",
    );

    s.add(
        "KT09-空集短报",
        {
            let e = Suite::new();
            e.is_empty()
                && e.run().err() == Some(KT_ESHORT)
                && e.measure().total == 0
                && e.measure().rate == 0
                && e.render().is_empty()
                && e.first().is_none()
        },
        "static",
    );

    s.add(
        "KT10-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8;
            let c = l.err_count() == 6;
            let d = l.audit() == Ok(());
            let e = l.sample(0).map(|s| s.value).unwrap_or(0) == 19;
            let f = l.sample(13).map(|s| s.code).unwrap_or(0) == KT_ESLOT;
            let g = {
                let mut m = Ledger::new();
                let s = Sample { slot: 3, op: 0, ok: false, code: 0, value: 0 };
                m.register(s) == Err(KT_ESLOT)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "KT11-冻结重放",
        {
            let mut l = build_ledger();
            let same = l.seal() == build_ledger().seal();
            let root = match l.freeze() {
                Ok(v) => v,
                Err(_) => 0,
            };
            let frozen = l.is_frozen() && root == l.seal();
            let replay = match l.replay() {
                Ok(v) => v == root,
                Err(_) => false,
            };
            let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
            let rejected = l.register(probe) == Err(KT_ESLOT) && l.rejects() == 1;
            let again = l.freeze() == Err(KT_EBAL) && l.rejects() == 2;
            same && frozen && replay && rejected && again
        },
        "static",
    );

    s.add(
        "KT12-渲染回读",
        {
            let s = build_suite();
            let mut ok = true;
            let mut i = 0usize;
            while i < s.len() {
                if let Some(c) = s.at(i) {
                    let line = c.line();
                    if !line.as_str().starts_with("[ok] ") {
                        ok = false;
                    }
                    if !line.as_str().ends_with(c.group().text()) {
                        ok = false;
                    }
                }
                i += 1;
            }
            let rep = s.measure();
            let text = rep.text();
            ok && text.as_str().ends_with('\n')
                && text.as_str().contains("hash 10/10")
                && text.as_str().contains("stdin 9/9")
        },
        "static",
    );

    s.add(
        "KT13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_composition() {
        let s = build_suite();
        assert_eq!(s.len(), CASE_CAP);
        assert!(s.manifest_ok());
        assert_eq!(s.group_count(Group::Hash), HASH_CASES);
        assert_eq!(s.group_count(Group::Stdin), STDIN_CASES);
        assert_eq!(s.id_sum(), 190);
        assert_eq!(s.card_count(), 2);
        assert_eq!(s.first().map(|c| c.id()), Some(1));
        assert_eq!(s.last().map(|c| c.id()), Some(19));
    }

    #[test]
    fn case_labels() {
        let a = Case::new(1, Group::Hash, true);
        assert_eq!(a.label().as_str(), "11538#01");
        assert_eq!(a.line().as_str(), "[ok] 11538#01 hash");
        let b = Case::new(11, Group::Stdin, false);
        assert_eq!(b.label().as_str(), "11539#11");
        assert!(b.line().as_str().starts_with("[fail] "));
        assert!(a.valid());
        assert!(!Case::with_card(1, 1, Group::Hash, true).valid());
    }

    #[test]
    fn push_gates() {
        let mut s = Suite::new();
        assert_eq!(s.push(Case::with_card(1, 11540, Group::Hash, true)), Err(KT_ECARD));
        assert_eq!(s.push(Case::new(0, Group::Hash, true)), Err(KT_ESLOT));
        assert_eq!(s.push(Case::new(20, Group::Hash, true)), Err(KT_ESLOT));
        assert_eq!(s.push(Case::new(1, Group::Hash, true)), Ok(0));
        assert_eq!(s.push(Case::new(1, Group::Hash, true)), Err(KT_EDUP));
        assert_eq!(s.push(Case::new(2, Group::Hash, true)), Ok(1));
        assert_eq!(s.push(Case::new(2, Group::Hash, true)), Err(KT_EDUP));
        assert_eq!(s.push(Case::with_card(1, HASH_CARD, Group::Hash, true)), Err(KT_ESEQ));
    }

    #[test]
    fn capacity_gate() {
        assert_eq!(slot_probe().err(), Some(KT_ESLOT));
        let mut s = Suite::new();
        let mut i = 1u16;
        while i <= CASE_CAP as u16 {
            let g = if (i as usize) <= HASH_CASES { Group::Hash } else { Group::Stdin };
            assert!(s.push(Case::new(i, g, true)).is_ok());
            i += 1;
        }
        assert_eq!(s.len(), CASE_CAP);
    }

    #[test]
    fn suite_runs_green() {
        let s = build_suite();
        let rep = s.run().expect("green");
        assert_eq!(rep.total, 19);
        assert_eq!(rep.passed, 19);
        assert_eq!(rep.failed, 0);
        assert_eq!(rep.rate, 100);
        assert!(rep.all_green());
        assert_eq!(rep.hash, 10);
        assert_eq!(rep.stdin, 9);
    }

    #[test]
    fn failing_suite_gate() {
        let bad = failing_suite();
        assert_eq!(bad.len(), CASE_CAP);
        assert_eq!(bad.run(), Err(KT_EFAIL));
        assert_eq!(bad.first_failed().map(|c| c.id()), Some(7));
        let rep = bad.measure();
        assert_eq!(rep.failed, 1);
        assert_eq!(rep.passed, 18);
        assert_eq!(rep.rate, 94);
    }

    #[test]
    fn empty_suite_gate() {
        let e = Suite::new();
        assert!(e.is_empty());
        assert_eq!(e.run(), Err(KT_ESHORT));
        assert_eq!(e.measure(), Report::blank());
        assert!(e.render().is_empty());
        assert_eq!(e.first(), None);
        assert_eq!(e.last(), None);
        assert_eq!(e.card_count(), 0);
    }

    #[test]
    fn hash_cases_all_green() {
        assert!(hash_ten_ok());
        assert!(digest_is(Algorithm::Sha1, b"abc", SHA1_ABC));
        assert!(digest_is(Algorithm::Blake2b256, b"abc", B2B256_ABC));
        assert!(digest_is(Algorithm::Blake2b512, b"abc", B2B512_ABC));
    }

    #[test]
    fn stdin_cases_all_green() {
        assert!(stdin_nine_ok());
        let mut n = (HASH_CASES as u16) + 1;
        while n <= CASE_CAP as u16 {
            let c = stdin_case(n);
            assert_eq!(c.card(), STDIN_CARD);
            assert_eq!(c.group(), Group::Stdin);
            assert!(c.ok());
            n += 1;
        }
    }

    #[test]
    fn command_shape() {
        assert_eq!(command_line().as_str(), "ktest run b17.all");
        assert_eq!(command_echo().as_str(), "$ ktest run b17.all");
    }

    #[test]
    fn report_text_three_lines() {
        let text = build_suite().measure().text();
        assert!(text.as_str().starts_with("total 19 passed 19 failed 0 rate 100%"));
        assert!(text.as_str().contains("hash 10/10"));
        assert!(text.as_str().contains("stdin 9/9"));
        assert_eq!(text.as_str().matches('\n').count(), 3);
    }

    #[test]
    fn card_gate_table() {
        assert!(Suite::accepts_card(HASH_CARD));
        assert!(Suite::accepts_card(STDIN_CARD));
        assert!(!Suite::accepts_card(0));
        assert!(!Suite::accepts_card(11540));
        assert_eq!(card_probe().err(), Some(KT_ECARD));
        let mut s = Suite::new();
        assert_eq!(
            s.push(Case::with_card(1, STDIN_CARD, Group::Hash, true)),
            Err(KT_ECARD)
        );
    }

    #[test]
    fn by_id_lookup() {
        let s = build_suite();
        assert_eq!(s.by_id(1).map(|c| c.card()), Some(HASH_CARD));
        assert_eq!(s.by_id(10).map(|c| c.card()), Some(HASH_CARD));
        assert_eq!(s.by_id(11).map(|c| c.card()), Some(STDIN_CARD));
        assert_eq!(s.by_id(19).map(|c| c.card()), Some(STDIN_CARD));
        assert_eq!(s.by_id(20), None);
        assert_eq!(s.by_id(0), None);
    }

    #[test]
    fn suite_render_all_ok() {
        let text = build_suite().render();
        assert!(text.as_str().starts_with("[ok] 11538#01 hash\n"));
        assert!(text.as_str().contains("[ok] 11539#19 stdin\n"));
        assert_eq!(text.as_str().matches("\n[ok] ").count(), 18);
        assert!(!text.as_str().contains("[fail]"));
    }

    #[test]
    fn ledger_balanced() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(0).map(|s| s.value), Some(19));
        assert_eq!(l.sample(2).map(|s| s.value), Some(100));
        assert_eq!(l.sample(13).map(|s| s.code), Some(KT_ESLOT));
    }

    #[test]
    fn ledger_replay_and_freeze() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(KT_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(KT_ESLOT));
    }

    #[test]
    fn inject_four_green() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn build_is_deterministic() {
        let a = build_suite();
        let b = build_suite();
        assert_eq!(a.id_sum(), b.id_sum());
        assert_eq!(a.passed(), b.passed());
        assert_eq!(a.card_count(), b.card_count());
        let mut i = 0usize;
        while i < a.len() {
            assert_eq!(a.at(i), b.at(i));
            i += 1;
        }
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11540_checks();
        assert!(s.all_passed());
    }
}
