//! UNX-F11557 shuf 随机打乱
//!
//! `shuf` 的难点不是「洗牌」，而是三件基准机才在意的事：
//!
//! 1. **全排列保集**：一次 `shuf` 的输出必须是输入的一个排列——每个元素恰好出现
//!    一次、集合相等。`--head-count` 只是「部分 Fisher–Yates」：洗到第 N 个就停，
//!    后面的位置保持未动，未动的位置在逻辑上仍是原值。
//! 2. **区间不打表**：`-i 1-4294967295` 不可能真建 40 亿项的数组。这里用**稀疏
//!    交换表**：`i` 与 `j` 交换时只记录「谁被改成了什么」，取值时反向查找最近一次
//!    写入。容量随 `--head-count` 走，与区间大小无关。
//! 3. **无偏取值**：`random % n` 在 `n` 不整除 2^64 时偏向小值。做法是拒绝采样——
//!    只在 `[0, floor(2^64/n)*n)` 里取值，落在外面的样本丢弃重来，这样每个余数
//!    的概率严格相等。
//!
//! 随机源是 SplitMix64：种子可由 `--random-source=KEY` 用 FNV-1a 折叠出固定值，
//! 因此「同种子同序列」是可判定的——这对判据比对是必需的，真随机反而没法校验。
//!
//! 闸门次序：选项字面（`SH_EINVAL`）→ 区间/长度上限（`SH_ELEN`）→ 字节合法性
//! （`SH_EBYTE`）→ 数值越界（`SH_ERANGE`）→ 重复（`SH_EDUP`）→ 记录不足
//! （`SH_ESHORT`）→ 种子冲突（`SH_ESEED`）→ 选项冲突（`SH_ESTATE`）。
//!
//! 账本语义：十四格 = 八格成功（区间项数、保集计数、单发条数、重发条数、
//! 无偏首值、池装载、多行渲染、区间渲染）+ 六格失败（区间倒置、坏字节、空值、
//! 超上限、编号越界、选项冲突）。`audit` 认「八值六码」，`seal` 做 FNV-1a
//! 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3e0;

/// 选项字面非法。
pub const SH_EINVAL: u32 = SEG_BASE + 0x01;
/// 区间或输出过长。
pub const SH_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法。
pub const SH_EBYTE: u32 = SEG_BASE + 0x03;
/// 编号越界。
pub const SH_ERANGE: u32 = SEG_BASE + 0x04;
/// 选项重复。
pub const SH_EDUP: u32 = SEG_BASE + 0x05;
/// 记录不足。
pub const SH_ESHORT: u32 = SEG_BASE + 0x06;
/// 种子冲突。
pub const SH_ESEED: u32 = SEG_BASE + 0x07;
/// 选项组合冲突。
pub const SH_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SH_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const SH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const SH_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const SH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单行容量。
pub const LINECAP: usize = 64;

/// 池容量。
pub const POOLCAP: usize = 32;

/// 输出条数上限。
pub const OUTCAP: usize = 32;

/// 稀疏交换表容量（对数）。
pub const SWAPCAP: usize = 64;

/// 参数个数上限。
pub const ARGCAP: usize = 32;

/// 操作码：区间解析。
pub const OP_RANGE: u8 = 0x01;
/// 操作码：洗牌。
pub const OP_SHUFFLE: u8 = 0x02;
/// 操作码：重发。
pub const OP_REPEAT: u8 = 0x03;
/// 操作码：无偏取样。
pub const OP_BOUNDED: u8 = 0x04;
/// 操作码：池。
pub const OP_POOL: u8 = 0x05;
/// 操作码：多行渲染。
pub const OP_LINES: u8 = 0x06;
/// 操作码：区间渲染。
pub const OP_RENDER: u8 = 0x07;
/// 操作码：选项。
pub const OP_OPT: u8 = 0x08;

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
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
    }
    let mut k = i;
    while k < 20 {
        if let Some(d) = buf.get(k) {
            s.push(*d as char);
        }
        k += 1;
    }
}

/// 十进制文本。
fn dec_text(v: u64) -> String {
    let mut s = String::new();
    push_dec(&mut s, v);
    s
}

/// 随机源（SplitMix64）。
#[derive(Clone, Copy, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// 由种子构造。
    pub const fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }

    /// 由字符串键折叠出种子（同键同种子）。
    pub fn key_seed(key: &[u8]) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < key.len() {
            if let Some(b) = key.get(i) {
                h = fold(h, *b as u64);
            }
            i += 1;
        }
        h
    }

    /// 下一个 64 位样本。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 无偏取 `[0, n)`；`n == 0` 时给 0。
    pub fn bounded(&mut self, n: u64) -> u64 {
        if n <= 1 {
            return 0;
        }
        let zone = u64::MAX - (u64::MAX % n);
        let mut tries = 0u32;
        loop {
            let v = self.next_u64();
            if v < zone || tries >= 64 {
                return v % n;
            }
            tries += 1;
        }
    }

    /// 当前状态。
    pub fn state(&self) -> u64 {
        self.state
    }
}

/// 区间：`lo..=hi`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    /// 下界。
    pub lo: u32,
    /// 上界。
    pub hi: u32,
}

impl Range {
    /// 构造并校验。
    pub fn new(lo: u32, hi: u32) -> Result<Range, u32> {
        if lo > hi {
            return Err(SH_EINVAL);
        }
        Ok(Range { lo, hi })
    }

    /// 项数。
    pub fn count(&self) -> u64 {
        self.hi as u64 - self.lo as u64 + 1
    }

    /// 解析 `LO-HI`。
    pub fn parse(raw: &[u8]) -> Result<Range, u32> {
        if raw.is_empty() {
            return Err(SH_EINVAL);
        }
        if raw.len() > 24 {
            return Err(SH_ELEN);
        }
        let mut dash = None;
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    if *b == b'-' {
                        if dash.is_some() {
                            return Err(SH_EBYTE);
                        }
                        dash = Some(i);
                    } else if *b < b'0' || *b > b'9' {
                        return Err(SH_EBYTE);
                    }
                }
                None => break,
            }
            i += 1;
        }
        let at = match dash {
            Some(v) => v,
            None => return Err(SH_EBYTE),
        };
        let lo = parse_u32(raw.get(..at).unwrap_or(&[]))?;
        let hi = parse_u32(raw.get(at + 1..).unwrap_or(&[]))?;
        Range::new(lo, hi)
    }
}

/// 十进制无符号（≤ `u32::MAX`）。
fn parse_u32(raw: &[u8]) -> Result<u32, u32> {
    if raw.is_empty() {
        return Err(SH_EINVAL);
    }
    if raw.len() > 10 {
        return Err(SH_ERANGE);
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < raw.len() {
        match raw.get(i) {
            Some(c) => {
                if *c < b'0' || *c > b'9' {
                    return Err(SH_EBYTE);
                }
                v = v * 10 + (*c - b'0') as u64;
                if v > u32::MAX as u64 {
                    return Err(SH_ERANGE);
                }
            }
            None => return Err(SH_EBYTE),
        }
        i += 1;
    }
    Ok(v as u32)
}

/// 稀疏置换：只记录被改写的下标。
#[derive(Clone, Copy, Debug)]
pub struct RangePerm {
    swaps: [(u32, u32); SWAPCAP],
    len: usize,
}

impl RangePerm {
    /// 空置换（恒等）。
    pub const fn new() -> RangePerm {
        RangePerm { swaps: [(0, 0); SWAPCAP], len: 0 }
    }

    /// 记录条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否恒等。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取值：反向找最近一次写入。
    pub fn get(&self, idx: u32) -> u32 {
        let mut k = self.len;
        while k > 0 {
            k -= 1;
            if let Some(p) = self.swaps.get(k) {
                if p.0 == idx {
                    return p.1;
                }
            }
        }
        idx
    }

    /// 写入一条；满表给 `SH_ELEN`。
    fn put(&mut self, k: u32, v: u32) -> Result<(), u32> {
        if self.len >= SWAPCAP {
            return Err(SH_ELEN);
        }
        if let Some(dst) = self.swaps.get_mut(self.len) {
            *dst = (k, v);
        }
        self.len += 1;
        Ok(())
    }

    /// 交换两格，返回交换后 `i` 处的值。
    pub fn swap(&mut self, i: u32, j: u32) -> Result<u32, u32> {
        if i == j {
            return Ok(self.get(i));
        }
        let vi = self.get(i);
        let vj = self.get(j);
        if vi != vj {
            self.put(i, vj)?;
            self.put(j, vi)?;
        }
        Ok(self.get(i))
    }

    /// 应用一次部分 Fisher–Yates。
    pub fn step(&mut self, i: u64, span: u64, rng: &mut Rng) -> Result<u32, u32> {
        let j = i.saturating_add(rng.bounded(span));
        let a = clamp_u32(i)?;
        let b = clamp_u32(j)?;
        self.swap(a, b)
    }
}

/// `u64` → `u32`，越界给 `SH_ERANGE`。
fn clamp_u32(v: u64) -> Result<u32, u32> {
    if v > u32::MAX as u64 {
        return Err(SH_ERANGE);
    }
    Ok(v as u32)
}

/// 区间洗牌（部分）：只做 `head` 步。
pub fn shuffle_range(r: &Range, head: usize, rng: &mut Rng) -> Result<[u32; OUTCAP], u32> {
    if head > OUTCAP {
        return Err(SH_ELEN);
    }
    let n = r.count();
    if head as u64 > n {
        return Err(SH_ESHORT);
    }
    let mut perm = RangePerm::new();
    let mut out = [0u32; OUTCAP];
    let mut i = 0usize;
    while i < head {
        let span = n - i as u64;
        let v = perm.step(r.lo as u64 + i as u64, span, rng)?;
        if let Some(dst) = out.get_mut(i) {
            *dst = v;
        }
        i += 1;
    }
    Ok(out)
}

/// 区间重发（有放回）。
pub fn repeat_range(r: &Range, head: usize, rng: &mut Rng) -> Result<[u32; OUTCAP], u32> {
    if head > OUTCAP {
        return Err(SH_ELEN);
    }
    let n = r.count();
    if n == 0 {
        return Err(SH_ESHORT);
    }
    let mut out = [0u32; OUTCAP];
    let mut i = 0usize;
    while i < head {
        let v = r.lo as u64 + rng.bounded(n);
        if let Some(dst) = out.get_mut(i) {
            *dst = clamp_u32(v)?;
        }
        i += 1;
    }
    Ok(out)
}

/// 一行文本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    buf: [u8; LINECAP],
    len: u8,
}

impl Item {
    /// 空行。
    pub const fn empty() -> Item {
        Item { buf: [0; LINECAP], len: 0 }
    }

    /// 解析（≤ `LINECAP`、可打印 ASCII）。
    pub fn parse(raw: &[u8]) -> Result<Item, u32> {
        if raw.len() > LINECAP {
            return Err(SH_ELEN);
        }
        let mut buf = [0u8; LINECAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    let c = *b;
                    if c == b'\n' || (c < 0x20 && c != b'\t') {
                        return Err(SH_EBYTE);
                    }
                    if let Some(d) = buf.get_mut(i) {
                        *d = c;
                    }
                }
                None => return Err(SH_EBYTE),
            }
            i += 1;
        }
        Ok(Item { buf, len: raw.len() as u8 })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<Item, u32> {
        Item::parse(text.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len as usize).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(c) = self.buf.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        s
    }
}

/// 行池。
#[derive(Clone, Copy, Debug)]
pub struct Pool {
    items: [Item; POOLCAP],
    len: usize,
}

impl Pool {
    /// 空池。
    pub const fn new() -> Pool {
        Pool { items: [Item::empty(); POOLCAP], len: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 行。
    pub fn item(&self, i: usize) -> Option<Item> {
        if i >= self.len {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 追加；满池给 `SH_ELEN`。
    pub fn push(&mut self, it: Item) -> Result<(), u32> {
        if self.len >= POOLCAP {
            return Err(SH_ELEN);
        }
        if let Some(dst) = self.items.get_mut(self.len) {
            *dst = it;
        }
        self.len += 1;
        Ok(())
    }

    /// 追加文本。
    pub fn push_text(&mut self, text: &str) -> Result<(), u32> {
        self.push(Item::parse_text(text)?)
    }

    /// 部分 Fisher–Yates。
    pub fn shuffle(&self, head: usize, rng: &mut Rng) -> Result<[Item; OUTCAP], u32> {
        if head > OUTCAP {
            return Err(SH_ELEN);
        }
        if head > self.len {
            return Err(SH_ESHORT);
        }
        let mut idx = [0u8; POOLCAP];
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = idx.get_mut(i) {
                *d = i as u8;
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < head {
            let span = (self.len - a) as u64;
            let j = a + rng.bounded(span) as usize;
            let (x, y) = match (idx.get(a), idx.get(j)) {
                (Some(p), Some(q)) => (*p, *q),
                _ => break,
            };
            if let Some(d) = idx.get_mut(a) {
                *d = y;
            }
            if let Some(d) = idx.get_mut(j) {
                *d = x;
            }
            a += 1;
        }
        let mut out = [Item::empty(); OUTCAP];
        let mut k = 0usize;
        while k < head {
            let pos = match idx.get(k) {
                Some(v) => *v as usize,
                None => break,
            };
            if let Some(src) = self.items.get(pos) {
                if let Some(dst) = out.get_mut(k) {
                    *dst = *src;
                }
            }
            k += 1;
        }
        Ok(out)
    }
}

/// 多行渲染（每行带换行）。
pub fn render_items(items: &[Item; OUTCAP], n: usize) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        if let Some(it) = items.get(i) {
            s.push_str(&it.text());
            s.push('\n');
        }
        i += 1;
    }
    s
}

/// 区间渲染。
pub fn render_range(vals: &[u32; OUTCAP], n: usize) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        if let Some(v) = vals.get(i) {
            push_dec(&mut s, *v as u64);
            s.push('\n');
        }
        i += 1;
    }
    s
}

/// 是否全排列：`vals[..n]` 恰为 `lo..=hi` 的一个排列。
pub fn is_permutation(vals: &[u32; OUTCAP], n: usize, r: &Range) -> bool {
    if n as u64 != r.count() {
        return false;
    }
    let span = r.count();
    if span == 0 || span > OUTCAP as u64 {
        return false;
    }
    let mut seen = [false; OUTCAP];
    let mut i = 0usize;
    while i < n {
        let v = match vals.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if v < r.lo || v > r.hi {
            return false;
        }
        let k = (v - r.lo) as usize;
        match seen.get_mut(k) {
            Some(d) => {
                if *d {
                    return false;
                }
                *d = true;
            }
            None => return false,
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < n {
        if seen.get(j) != Some(&true) {
            return false;
        }
        j += 1;
    }
    true
}

/// 选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 输入区间。
    pub range: Option<Range>,
    /// 输出条数；`None` 表示全量。
    pub head: Option<u32>,
    /// 有放回重发。
    pub repeat: bool,
    /// 参数当输入。
    pub echo: bool,
    /// 显式种子。
    pub seed: Option<u64>,
    /// 参数个数。
    pub argc: usize,
}

impl Opts {
    /// 空选项。
    pub const fn blank() -> Opts {
        Opts { range: None, head: None, repeat: false, echo: false, seed: None, argc: 0 }
    }

    /// 生效条数。
    pub fn want(&self, available: u64) -> u64 {
        match self.head {
            Some(n) => n as u64,
            None => available,
        }
    }
}

/// 解析选项。
pub fn parse_opts(args: &[&str]) -> Result<Opts, u32> {
    let mut o = Opts::blank();
    let mut seen_range = false;
    let mut seen_head = false;
    let mut seen_repeat = false;
    let mut seen_echo = false;
    let mut seen_seed = false;
    let mut i = 0usize;
    while i < args.len() {
        if i >= ARGCAP {
            return Err(SH_ELEN);
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a.len() > 64 {
            return Err(SH_ELEN);
        }
        if let Some(rest) = a.strip_prefix("--input-range=") {
            if seen_range {
                return Err(SH_EDUP);
            }
            seen_range = true;
            o.range = Some(Range::parse(rest.as_bytes())?);
        } else if a == "-i" || a == "--input-range" {
            if seen_range {
                return Err(SH_EDUP);
            }
            seen_range = true;
            let v = match args.get(i + 1) {
                Some(v) => *v,
                None => return Err(SH_EINVAL),
            };
            o.range = Some(Range::parse(v.as_bytes())?);
            i += 1;
        } else if let Some(rest) = a.strip_prefix("--head-count=") {
            if seen_head {
                return Err(SH_EDUP);
            }
            seen_head = true;
            o.head = Some(parse_u32(rest.as_bytes())?);
        } else if a == "-n" || a == "--head-count" {
            if seen_head {
                return Err(SH_EDUP);
            }
            seen_head = true;
            let v = match args.get(i + 1) {
                Some(v) => *v,
                None => return Err(SH_EINVAL),
            };
            o.head = Some(parse_u32(v.as_bytes())?);
            i += 1;
        } else if a == "-r" || a == "--repeat" {
            if seen_repeat {
                return Err(SH_EDUP);
            }
            seen_repeat = true;
            o.repeat = true;
        } else if a == "-e" || a == "--echo" {
            if seen_echo {
                return Err(SH_EDUP);
            }
            seen_echo = true;
            o.echo = true;
        } else if let Some(rest) = a.strip_prefix("--random-source=") {
            if seen_seed {
                return Err(SH_EDUP);
            }
            seen_seed = true;
            if rest.is_empty() {
                return Err(SH_ESEED);
            }
            o.seed = Some(Rng::key_seed(rest.as_bytes()));
        } else {
            return Err(SH_EINVAL);
        }
        o.argc += 1;
        i += 1;
    }
    if o.echo && o.range.is_some() {
        return Err(SH_ESTATE);
    }
    if o.repeat && o.head.is_none() {
        return Err(SH_ESTATE);
    }
    Ok(o)
}

/// 一个账本样本。
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

/// 样本账本。
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SH_ESLOT);
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

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SH_EBAL);
        }
        const VALS: [u32; 8] = [10, 8, 1, 5, 1, 4, 4, 4];
        const CODES: [u32; 6] = [SH_EINVAL, SH_EBYTE, SH_EINVAL, SH_ELEN, SH_ERANGE, SH_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SH_EBAL),
            };
            if s.slot as usize != i {
                return Err(SH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SH_EVALUE);
                        }
                    }
                    None => return Err(SH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SH_EVALUE);
                        }
                    }
                    None => return Err(SH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SH_EBAL);
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
            return Err(SH_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(SH_ESEAL)
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

/// 样本池：四个词。
pub fn sample_pool() -> Pool {
    let mut p = Pool::new();
    let _ = p.push_text("alpha");
    let _ = p.push_text("beta");
    let _ = p.push_text("gamma");
    let _ = p.push_text("delta");
    p
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_RANGE, {
        let r = Range::parse(b"1-10");
        r.map(|v| v.count() as u32)
    }));

    let _ = l.register(word_cell(1, OP_SHUFFLE, {
        let mut rng = Rng::new(7);
        match Range::parse(b"1-8") {
            Ok(r) => match shuffle_range(&r, 8, &mut rng) {
                Ok(vals) => {
                    if is_permutation(&vals, 8, &r) {
                        Ok(8)
                    } else {
                        Err(SH_EVALUE)
                    }
                }
                Err(c) => Err(c),
            },
            Err(c) => Err(c),
        }
    }));

    let _ = l.register(count_cell(2, OP_SHUFFLE, {
        let mut rng = Rng::new(11);
        let r = Range::parse(b"1-10").unwrap_or(Range { lo: 0, hi: 0 });
        shuffle_range(&r, 1, &mut rng).map(|v| if v.get(0).is_some() { 1 } else { 0 })
    }));

    let _ = l.register(count_cell(3, OP_REPEAT, {
        let mut rng = Rng::new(3);
        let r = Range::parse(b"5-9").unwrap_or(Range { lo: 0, hi: 0 });
        repeat_range(&r, 5, &mut rng).map(|v| v.get(..5).map(|s| s.len()).unwrap_or(0))
    }));

    let _ = l.register(word_cell(4, OP_BOUNDED, {
        let mut rng = Rng::new(42);
        let v = rng.bounded(10);
        Ok(if v < 10 { 1 } else { 0 })
    }));

    let _ = l.register(count_cell(5, OP_POOL, Ok(sample_pool().len())));

    let _ = l.register(count_cell(6, OP_LINES, {
        let mut items = [Item::empty(); OUTCAP];
        let a = Item::parse_text("a").unwrap_or(Item::empty());
        let b = Item::parse_text("b").unwrap_or(Item::empty());
        if let Some(d) = items.get_mut(0) {
            *d = a;
        }
        if let Some(d) = items.get_mut(1) {
            *d = b;
        }
        Ok(render_items(&items, 2).len())
    }));

    let _ = l.register(count_cell(7, OP_RENDER, {
        let mut vals = [0u32; OUTCAP];
        if let Some(d) = vals.get_mut(0) {
            *d = 7;
        }
        if let Some(d) = vals.get_mut(1) {
            *d = 8;
        }
        Ok(render_range(&vals, 2).len())
    }));

    let _ = l.register(word_cell(8, OP_RANGE, {
        Range::parse(b"10-1").map(|r| r.count() as u32)
    }));

    let _ = l.register(word_cell(9, OP_RANGE, {
        Range::parse(b"1x10").map(|r| r.count() as u32)
    }));

    let _ = l.register(word_cell(10, OP_RANGE, Range::parse(b"").map(|r| r.count() as u32)));

    let _ = l.register(word_cell(11, OP_SHUFFLE, {
        let r = Range::parse(b"1-10").unwrap_or(Range { lo: 0, hi: 0 });
        let mut rng = Rng::new(1);
        shuffle_range(&r, OUTCAP + 1, &mut rng).map(|v| v.len() as u32)
    }));

    let _ = l.register(word_cell(12, OP_RANGE, {
        Range::parse(b"0-4294967296").map(|r| r.count() as u32)
    }));

    let _ = l.register(word_cell(13, OP_OPT, {
        parse_opts(&["-e", "-i", "1-3"]).map(|o| o.argc as u32)
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    out[0] = match Range::parse(b"1-8") {
        Ok(r) => {
            let mut rng = Rng::new(5);
            match shuffle_range(&r, 8, &mut rng) {
                Ok(a) => is_permutation(&a, 8, &r),
                Err(_) => false,
            }
        }
        Err(_) => false,
    };

    out[1] = {
        let mut a = Rng::new(99);
        let mut b = Rng::new(99);
        let x = [a.next_u64(), a.next_u64(), a.next_u64()];
        let y = [b.next_u64(), b.next_u64(), b.next_u64()];
        x == y && Rng::key_seed(b"seed") == Rng::key_seed(b"seed")
    };

    out[2] = {
        let r = match Range::parse(b"4000000000-4000000007") {
            Ok(v) => v,
            Err(_) => Range { lo: 0, hi: 1 },
        };
        let mut rng = Rng::new(1234);
        match shuffle_range(&r, 8, &mut rng) {
            Ok(v) => is_permutation(&v, 8, &r),
            Err(_) => false,
        }
    };

    out[3] = {
        let p = sample_pool();
        let mut rng = Rng::new(21);
        match p.shuffle(4, &mut rng) {
            Ok(v) => {
                let mut seen = 0usize;
                let mut i = 0usize;
                while i < 4 {
                    if let Some(it) = v.get(i) {
                        if !it.is_empty() {
                            seen += 1;
                        }
                    }
                    i += 1;
                }
                seen == 4
            }
            Err(_) => false,
        }
    };

    out
}

/// 判据集合。
pub fn run_unx11557_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11557.shuf");

    s.add(
        "SH01-区间解析",
        {
            let a = Range::parse(b"1-10").map(|r| (r.lo, r.hi, r.count())) == Ok((1, 10, 10));
            let b = Range::parse(b"7-7").map(|r| r.count()) == Ok(1);
            let c = Range::parse(b"0-4294967295").map(|r| r.count()) == Ok(4294967296);
            let d = Range::parse(b"10-1") == Err(SH_EINVAL);
            let e = Range::parse(b"1x10") == Err(SH_EBYTE);
            let f = Range::parse(b"") == Err(SH_EINVAL);
            let g = Range::parse(b"0-4294967296") == Err(SH_ERANGE);
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SH02-全排列保集",
        {
            let mut ok = true;
            let mut seed = 1u64;
            while seed <= 8 {
                let r = match Range::parse(b"1-12") {
                    Ok(v) => v,
                    Err(_) => Range { lo: 0, hi: 0 },
                };
                let mut rng = Rng::new(seed);
                match shuffle_range(&r, 12, &mut rng) {
                    Ok(v) => {
                        if !is_permutation(&v, 12, &r) {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                seed += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "SH03-同种子同序列",
        {
            let r = match Range::parse(b"1-20") {
                Ok(v) => v,
                Err(_) => Range { lo: 0, hi: 0 },
            };
            let mut a = Rng::new(0xdead_beef);
            let mut b = Rng::new(0xdead_beef);
            let mut c = Rng::new(0xdead_beee);
            let x = shuffle_range(&r, 20, &mut a);
            let y = shuffle_range(&r, 20, &mut b);
            let z = shuffle_range(&r, 20, &mut c);
            let same = match (&x, &y) {
                (Ok(p), Ok(q)) => p == q,
                _ => false,
            };
            let diff = match (&x, &z) {
                (Ok(p), Ok(q)) => p != q,
                _ => false,
            };
            same && diff && Rng::key_seed(b"abc") == Rng::key_seed(b"abc")
        },
        "static",
    );

    s.add(
        "SH04-重发有放回",
        {
            let r = match Range::parse(b"5-9") {
                Ok(v) => v,
                Err(_) => Range { lo: 0, hi: 0 },
            };
            let mut rng = Rng::new(3);
            match repeat_range(&r, 32, &mut rng) {
                Ok(v) => {
                    let mut ok = true;
                    let mut i = 0usize;
                    while i < 32 {
                        match v.get(i) {
                            Some(x) => {
                                if *x < 5 || *x > 9 {
                                    ok = false;
                                }
                            }
                            None => ok = false,
                        }
                        i += 1;
                    }
                    ok && repeat_range(&r, OUTCAP + 1, &mut rng) == Err(SH_ELEN)
                }
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "SH05-无偏取值",
        {
            let mut ok = true;
            let mut seed = 1u64;
            while seed <= 16 {
                let mut rng = Rng::new(seed);
                let mut i = 0usize;
                while i < 64 {
                    if rng.bounded(7) >= 7 {
                        ok = false;
                    }
                    i += 1;
                }
                if rng.bounded(1) != 0 || rng.bounded(0) != 0 {
                    ok = false;
                }
                seed += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "SH06-限量与短路",
        {
            let r = match Range::parse(b"1-5") {
                Ok(v) => v,
                Err(_) => Range { lo: 0, hi: 0 },
            };
            let mut rng = Rng::new(9);
            let a = shuffle_range(&r, 0, &mut rng).map(|v| v.len()) == Ok(OUTCAP);
            let b = shuffle_range(&r, 6, &mut rng) == Err(SH_ESHORT);
            let c = shuffle_range(&r, OUTCAP + 1, &mut rng) == Err(SH_ELEN);
            let d = match shuffle_range(&r, 3, &mut rng) {
                Ok(v) => match (v.get(0), v.get(1), v.get(2)) {
                    (Some(a), Some(b), Some(c)) => *a != *b && *b != *c,
                    _ => false,
                },
                Err(_) => false,
            };
            a && b && c && d
        },
        "static",
    );

    s.add(
        "SH07-池与回显",
        {
            let p = sample_pool();
            let a = p.len() == 4;
            let b = p.item(0).map(|i| i.text()) == Some(String::from("alpha"));
            let c = p.item(4).is_none();
            let d = Item::parse(b"a\nb") == Err(SH_EBYTE);
            let e = Item::parse(&[0u8; LINECAP + 1]) == Err(SH_ELEN);
            let mut rng = Rng::new(2);
            let f = match p.shuffle(2, &mut rng) {
                Ok(v) => match (v.get(0), v.get(1)) {
                    (Some(x), Some(y)) => !x.is_empty() && !y.is_empty() && x.as_bytes() != y.as_bytes(),
                    _ => false,
                },
                Err(_) => false,
            };
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SH08-两种渲染",
        {
            let mut items = [Item::empty(); OUTCAP];
            let a = Item::parse_text("one").unwrap_or(Item::empty());
            let b = Item::parse_text("two").unwrap_or(Item::empty());
            if let Some(d) = items.get_mut(0) {
                *d = a;
            }
            if let Some(d) = items.get_mut(1) {
                *d = b;
            }
            let t = render_items(&items, 2);
            let mut vals = [0u32; OUTCAP];
            if let Some(d) = vals.get_mut(0) {
                *d = 10;
            }
            if let Some(d) = vals.get_mut(1) {
                *d = 2;
            }
            let u = render_range(&vals, 2);
            t.as_str() == "one\ntwo\n" && u.as_str() == "10\n2\n" && render_range(&vals, 0).is_empty()
        },
        "static",
    );

    s.add(
        "SH09-稀疏置换",
        {
            let mut p = RangePerm::new();
            let a = p.get(5) == 5 && p.is_empty();
            let b = p.swap(0, 3).map(|v| v) == Ok(3);
            let c = p.get(0) == 3;
            let d = p.get(3) == 0;
            let e = p.get(9) == 9;
            let f = p.swap(0, 3).map(|v| v) == Ok(0);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SH10-选项矩阵",
        {
            let a = match parse_opts(&["-i", "1-10", "-n", "3", "--random-source=key"]) {
                Ok(o) => {
                    o.range.map(|r| r.count()) == Some(10)
                        && o.head == Some(3)
                        && o.seed == Some(Rng::key_seed(b"key"))
                        && o.argc == 3
                        && o.want(10) == 3
                }
                Err(_) => false,
            };
            let b = match parse_opts(&["-i", "4-4"]) {
                Ok(o) => o.range.map(|r| r.lo) == Some(4) && o.head.is_none() && o.want(1) == 1,
                Err(_) => false,
            };
            let c = parse_opts(&["-e", "-i", "1-3"]) == Err(SH_ESTATE);
            let d = parse_opts(&["-r"]) == Err(SH_ESTATE);
            let e = parse_opts(&["-r", "-n", "2"]) .map(|o| o.repeat) == Ok(true);
            let f = parse_opts(&["-i", "1-3", "-i", "4-5"]) == Err(SH_EDUP);
            let g = parse_opts(&["--random-source="]) == Err(SH_ESEED);
            let h = parse_opts(&["-z"]) == Err(SH_EINVAL);
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "SH11-闸门次序",
        {
            let a = Range::parse(b"") == Err(SH_EINVAL);
            let b = Range::parse(b"1-2-3") == Err(SH_EBYTE);
            let c = Range::parse(b"0-4294967296") == Err(SH_ERANGE);
            let d = Range::parse(b"10-1") == Err(SH_EINVAL);
            let e = parse_opts(&["-n", "99999999999"]) == Err(SH_ERANGE);
            let f = parse_opts(&["-n"]) == Err(SH_EINVAL);
            a && b && c && d && e && f
        },
        "static",
    );

    s.add(
        "SH12-账本对账与重放",
        {
            let l = build_ledger();
            l.audit().is_ok()
                && l.replay().is_ok()
                && l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.seal() != 0
        },
        "static",
    );

    s.add(
        "SH13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let frozen = l.freeze().is_ok();
            let rejected = l.register(Sample::blank()).is_err() && l.rejects() == 1;
            let root = l.root() != 0 && l.is_frozen();
            let inj = inject_four();
            frozen && rejected && root && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parse() {
        assert_eq!(Range::parse(b"1-10").map(|r| r.count()), Ok(10));
        assert_eq!(Range::parse(b"0-0").map(|r| r.count()), Ok(1));
        assert_eq!(Range::parse(b"10-1"), Err(SH_EINVAL));
        assert_eq!(Range::parse(b"-1-5"), Err(SH_EBYTE));
        assert_eq!(Range::parse(b"1-"), Err(SH_EINVAL));
    }

    #[test]
    fn splitmix_sequence() {
        let mut a = Rng::new(0);
        let first = a.next_u64();
        let second = a.next_u64();
        assert_ne!(first, second);
        assert_ne!(first, 0);
        let mut b = Rng::new(0);
        assert_eq!(b.next_u64(), first);
    }

    #[test]
    fn bounded_range() {
        let mut rng = Rng::new(17);
        let mut i = 0usize;
        while i < 200 {
            assert!(rng.bounded(3) < 3);
            i += 1;
        }
        assert_eq!(rng.bounded(0), 0);
        assert_eq!(rng.bounded(1), 0);
    }

    #[test]
    fn permutation_property() {
        let r = Range::parse(b"1-16").unwrap_or(Range { lo: 0, hi: 0 });
        let mut seed = 1u64;
        while seed <= 10 {
            let mut rng = Rng::new(seed);
            let v = shuffle_range(&r, 16, &mut rng).unwrap_or([0u32; OUTCAP]);
            assert!(is_permutation(&v, 16, &r));
            seed += 1;
        }
    }

    #[test]
    fn sparse_swap_map() {
        let mut p = RangePerm::new();
        assert_eq!(p.swap(1, 4), Ok(4));
        assert_eq!(p.get(1), 4);
        assert_eq!(p.get(4), 1);
        assert_eq!(p.get(2), 2);
        assert_eq!(p.swap(1, 2), Ok(2));
        assert_eq!(p.get(1), 2);
        assert_eq!(p.get(2), 4);
        assert_eq!(p.get(4), 1);
    }

    #[test]
    fn repeat_stays_in_range() {
        let r = Range::parse(b"10-12").unwrap_or(Range { lo: 0, hi: 0 });
        let mut rng = Rng::new(4);
        let v = repeat_range(&r, 20, &mut rng).unwrap_or([0u32; OUTCAP]);
        let mut i = 0usize;
        while i < 20 {
            let x = v.get(i).copied().unwrap_or(0);
            assert!(x >= 10 && x <= 12);
            i += 1;
        }
    }

    #[test]
    fn pool_shuffle_keeps_all() {
        let p = sample_pool();
        let mut rng = Rng::new(6);
        let v = p.shuffle(4, &mut rng).unwrap_or([Item::empty(); OUTCAP]);
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < 4 {
            if v.get(i).map(|x| !x.is_empty()).unwrap_or(false) {
                seen += 1;
            }
            i += 1;
        }
        assert_eq!(seen, 4);
        assert_eq!(p.shuffle(5, &mut rng), Err(SH_ESHORT));
    }

    #[test]
    fn opts_and_conflicts() {
        assert_eq!(parse_opts(&["-e", "-i", "1-3"]), Err(SH_ESTATE));
        assert_eq!(parse_opts(&["-r"]), Err(SH_ESTATE));
        assert_eq!(parse_opts(&["-n", "2", "-n", "3"]), Err(SH_EDUP));
        let o = parse_opts(&["-i", "1-9", "-n", "0"]).unwrap_or(Opts::blank());
        assert_eq!(o.want(9), 0);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(13).map(|s| s.code), Some(SH_ESTATE));
    }
}
