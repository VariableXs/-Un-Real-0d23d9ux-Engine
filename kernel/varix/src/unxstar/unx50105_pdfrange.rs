//! UNX-F50105 PDF 页选择范围透传（页码区间裁剪不重排）（0x43940 段）
//!
//! 锚点：UNX-F50105-J1 区间裁剪 ×30 页序一致零重排；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43940`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 9`，
//! 逐项见 [`RangeError`]。
//!
//! 用户输入的页范围（`1-5,8,10-`）在直通场景里有两个硬约束：
//! 1. **只裁剪、不重排**——输出永远是文档升序的子序列。区间合法但乱序
//!    （`6-10,1-5`）要规范成升序集合，而不是按输入顺序重排页面；
//! 2. **守恒**——`选中页数 + 跳过页数 = 总页数`，且每个页号恰好出现一次。
//!    任何重叠/相邻区段在解析期合并，合并只改「段边界」，不改页序。
//!
//! 结构清单：
//! * [`Segment`] / [`PageRange`]：区间集合与规范化（排序 + 合并）；
//! * [`parse_ranges`]：文本解析器（单页/闭区间/开区间）；
//! * [`clip`] / [`Clipped`]：按总页数裁剪并自证守恒；
//! * [`PageCursor`]：直通下发游标（只前进，绝不回退重排）；
//! * [`clip_thirty`]：30 组选择 × 30 页裁剪核；
//! * [`injection_core`]：12 类坏样本注入核。
//!
//! 错误纪律：数字解析走 u64 中转并饱和，切片前先 `get`；`Clipped::verify`
//! 是内核内部的最后一道守恒闸，校验不过不返回结果。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43940 独占 0x20）。
pub const SEG_BASE: u32 = 0x43940;

const E_EMPTY: u16 = 1;
const E_SYNTAX: u16 = 2;
const E_ZERO: u16 = 3;
const E_REVERSED: u16 = 4;
const E_OVERFLOW: u16 = 5;
const E_BEYOND: u16 = 6;
const E_TOTAL: u16 = 7;
const E_RUN: u16 = 8;
const E_CAPACITY: u16 = 9;

/// 单份文档允许的最大页数：超过 65535 页的 PDF 直通在办公场景不可信，
/// 同时避免 `Vec<u32>` 被病态总页数拖爆。
pub const MAX_TOTAL: u32 = 65_535;
/// 页范围最多可以写多少个原始区段（规范化前）。
pub const MAX_SEGMENTS: usize = 64;

/// 页范围诊断。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeError {
    /// 输入为空或全是逗号/空白。
    Empty,
    /// 语法不成立（多横线、非数字、空段）。
    Syntax,
    /// 页号 0（PDF 页码 1 起）。
    Zero,
    /// 区间 start > end。
    Reversed,
    /// 数字超过 u32。
    Overflow,
    /// 区间越过文档总页数。
    Beyond,
    /// 总页数为 0 或超过上限。
    Total,
    /// 裁剪结果未通过守恒自证（内部一致性被破坏）。
    Run,
    /// 原始区段数超过上限。
    Capacity,
}

impl RangeError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            RangeError::Empty => E_EMPTY,
            RangeError::Syntax => E_SYNTAX,
            RangeError::Zero => E_ZERO,
            RangeError::Reversed => E_REVERSED,
            RangeError::Overflow => E_OVERFLOW,
            RangeError::Beyond => E_BEYOND,
            RangeError::Total => E_TOTAL,
            RangeError::Run => E_RUN,
            RangeError::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            RangeError::Empty => "page range empty",
            RangeError::Syntax => "page range syntax invalid",
            RangeError::Zero => "page number zero",
            RangeError::Reversed => "range start greater than end",
            RangeError::Overflow => "page number overflows u32",
            RangeError::Beyond => "range exceeds document page count",
            RangeError::Total => "document page count unusable",
            RangeError::Run => "clipped sequence violates invariants",
            RangeError::Capacity => "too many raw range segments",
        }
    }
}

/// 闭区间（1 起，含两端）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// 起始页。
    pub start: u32,
    /// 结束页（含）。
    pub end: u32,
}

impl Segment {
    /// 构造（调用方保证 1 ≤ start ≤ end）。
    pub const fn new(start: u32, end: u32) -> Segment {
        Segment { start, end }
    }

    /// 页数（u64 防溢出）。
    pub fn len(&self) -> u64 {
        if self.end < self.start {
            0
        } else {
            (self.end - self.start) as u64 + 1
        }
    }

    /// 是否包含某页。
    pub fn contains(&self, page: u32) -> bool {
        page >= self.start && page <= self.end
    }

    /// 两段是否重叠或相邻（规范化合并条件）。
    pub fn touches(&self, other: &Segment) -> bool {
        other.start <= self.end.saturating_add(1) && self.start <= other.end.saturating_add(1)
    }

    /// `a-b`，单页写 `a`。
    pub fn label(&self) -> String {
        if self.start == self.end {
            format!("{}", self.start)
        } else {
            format!("{}-{}", self.start, self.end)
        }
    }
}

/// 规范化后的页范围集合：区段按 start 升序、互不重叠且不相邻。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRange {
    /// 规范化区段。
    pub segments: Vec<Segment>,
}

impl PageRange {
    /// 空集合（内部构造用）。
    pub fn empty() -> PageRange {
        PageRange {
            segments: Vec::new(),
        }
    }

    /// 插入并就地规范化：插入排序 + 相邻合并。区段数 ≤64，插入排序
    /// 足够且不引入排序分配，结果只与集合内容有关（与输入顺序无关）。
    pub fn insert(&mut self, seg: Segment) {
        self.segments.push(seg);
        let mut i = 1usize;
        while i < self.segments.len() {
            let mut j = i;
            while j > 0 {
                let a = self.segments.get(j - 1).copied();
                let b = self.segments.get(j).copied();
                match (a, b) {
                    (Some(x), Some(y)) if x.start > y.start => {
                        self.segments.swap(j - 1, j);
                        j -= 1;
                    }
                    _ => break,
                }
            }
            i += 1;
        }
        let mut out: Vec<Segment> = Vec::with_capacity(self.segments.len());
        let mut k = 0usize;
        while k < self.segments.len() {
            if let Some(cur) = self.segments.get(k).copied() {
                match out.last_mut() {
                    Some(prev) if prev.touches(&cur) => {
                        if cur.end > prev.end {
                            prev.end = cur.end;
                        }
                    }
                    _ => out.push(cur),
                }
            }
            k += 1;
        }
        self.segments = out;
    }

    /// 选中页数（u64）。
    pub fn count(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.segments.len() {
            if let Some(s) = self.segments.get(i) {
                n = n.saturating_add(s.len());
            }
            i += 1;
        }
        n
    }

    /// 是否包含某页。
    pub fn contains(&self, page: u32) -> bool {
        let mut i = 0usize;
        while i < self.segments.len() {
            if let Some(s) = self.segments.get(i) {
                if s.contains(page) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 规范化文本：`1-3,7,9-12`；空集合写空串。
    pub fn to_spec(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.segments.len() {
            if let Some(seg) = self.segments.get(i) {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&seg.label());
            }
            i += 1;
        }
        s
    }
}

fn parse_num(part: &[u8]) -> Result<u32, RangeError> {
    if part.is_empty() {
        return Err(RangeError::Syntax);
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < part.len() {
        let b = match part.get(i) {
            Some(x) => *x,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(RangeError::Syntax);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
        if v > u32::MAX as u64 {
            return Err(RangeError::Overflow);
        }
        i += 1;
    }
    if v == 0 {
        return Err(RangeError::Zero);
    }
    Ok(v as u32)
}

fn trim(d: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = d.len();
    while a < b && matches!(d.get(a), Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')) {
        a += 1;
    }
    while b > a && matches!(d.get(b - 1), Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')) {
        b -= 1;
    }
    match d.get(a..b) {
        Some(v) => v,
        None => b"",
    }
}

/// 解析页范围文本。`total` 用于把 `5-` 解析成 `5..=total`。
///
/// 语法：`N` / `N-M` / `N-` / `-M`，逗号分隔；页号 1 起。
/// 空白只允许出现在整段两端（`1-5, 8` 合法，`1 - 5` 非法）。
pub fn parse_ranges(text: &[u8], total: u32) -> Result<PageRange, RangeError> {
    if total == 0 || total > MAX_TOTAL {
        return Err(RangeError::Total);
    }
    let body = trim(text);
    if body.is_empty() {
        return Err(RangeError::Empty);
    }
    // 全是逗号/空白（如 " , "）等同于空选择，先于语法检查归为 Empty。
    let mut only_sep = true;
    let mut q = 0usize;
    while q < body.len() {
        match body.get(q) {
            Some(b',') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {}
            _ => only_sep = false,
        }
        q += 1;
    }
    if only_sep {
        return Err(RangeError::Empty);
    }
    let mut range = PageRange::empty();
    let mut raw = 0usize;
    let mut at = 0usize;
    loop {
        // 取下一段（以逗号分隔；到结尾时取剩余部分）
        let mut end = at;
        while end < body.len() && body[end] != b',' {
            end += 1;
        }
        let part = match body.get(at..end) {
            Some(v) => trim(v),
            None => return Err(RangeError::Syntax),
        };
        if part.is_empty() {
            return Err(RangeError::Syntax);
        }
        raw += 1;
        if raw > MAX_SEGMENTS {
            return Err(RangeError::Capacity);
        }
        let seg = parse_one(part, total)?;
        range.insert(seg);
        if end >= body.len() {
            break;
        }
        at = end + 1;
    }
    if range.segments.is_empty() {
        return Err(RangeError::Empty);
    }
    Ok(range)
}

fn parse_one(part: &[u8], total: u32) -> Result<Segment, RangeError> {
    let mut dash: Option<usize> = None;
    let mut i = 0usize;
    while i < part.len() {
        if let Some(b) = part.get(i) {
            if *b == b'-' {
                if dash.is_some() {
                    return Err(RangeError::Syntax);
                }
                dash = Some(i);
            }
        }
        i += 1;
    }
    match dash {
        None => {
            let n = parse_num(part)?;
            Ok(Segment::new(n, n))
        }
        Some(p) => {
            let left = match part.get(..p) {
                Some(v) => v,
                None => return Err(RangeError::Syntax),
            };
            let right = match part.get(p + 1..) {
                Some(v) => v,
                None => return Err(RangeError::Syntax),
            };
            let start = if left.is_empty() { 1 } else { parse_num(left)? };
            let end = if right.is_empty() { total } else { parse_num(right)? };
            if start == 0 || end == 0 {
                return Err(RangeError::Zero);
            }
            if start > end {
                return Err(RangeError::Reversed);
            }
            Ok(Segment::new(start, end))
        }
    }
}

/// 裁剪结果：升序页号 + 区段 + 跳过计数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clipped {
    /// 文档总页数。
    pub total: u32,
    /// 选中的页号（严格升序、无重复）。
    pub pages: Vec<u32>,
    /// 选中区段（与 pages 的连续段一一对应）。
    pub runs: Vec<Segment>,
    /// 未选中的页数。
    pub skipped: u32,
}

impl Clipped {
    /// 守恒自证：
    /// * `pages` 严格升序且在 `1..=total`；
    /// * `pages.len() + skipped == total`；
    /// * `runs` 的页数与区段数一致，且每个 run 与 pages 的连续段吻合。
    pub fn verify(&self) -> Result<(), RangeError> {
        if self.total == 0 || self.total > MAX_TOTAL {
            return Err(RangeError::Total);
        }
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < self.pages.len() {
            let p = match self.pages.get(i) {
                Some(v) => *v,
                None => return Err(RangeError::Run),
            };
            if p == 0 || p > self.total {
                return Err(RangeError::Beyond);
            }
            if p <= prev {
                return Err(RangeError::Run);
            }
            prev = p;
            i += 1;
        }
        let selected = self.pages.len() as u64;
        if selected.saturating_add(self.skipped as u64) != self.total as u64 {
            return Err(RangeError::Run);
        }
        let run_pages = {
            let mut n = 0u64;
            let mut k = 0usize;
            while k < self.runs.len() {
                if let Some(r) = self.runs.get(k) {
                    if r.start == 0 || r.end > self.total || r.start > r.end {
                        return Err(RangeError::Run);
                    }
                    n = n.saturating_add(r.len());
                }
                k += 1;
            }
            n
        };
        if run_pages != selected {
            return Err(RangeError::Run);
        }
        // runs 与 pages 的连续段必须同构：逐段核对首尾页。
        let mut idx = 0usize;
        let mut k = 0usize;
        while k < self.runs.len() {
            if let Some(r) = self.runs.get(k) {
                if self.pages.get(idx) != Some(&r.start) {
                    return Err(RangeError::Run);
                }
                idx += (r.end - r.start) as usize;
                if self.pages.get(idx) != Some(&r.end) {
                    return Err(RangeError::Run);
                }
                idx += 1;
            }
            k += 1;
        }
        if idx != self.pages.len() {
            return Err(RangeError::Run);
        }
        Ok(())
    }

    /// 一行摘要（写回软件面）。
    pub fn summary(&self) -> String {
        format!(
            "pages {}/{} selected skipped={} runs={}",
            self.pages.len(),
            self.total,
            self.skipped,
            self.runs.len()
        )
    }

    /// 选中页数。
    pub fn selected(&self) -> usize {
        self.pages.len()
    }

    /// 第一页（空选择为 0）。
    pub fn first(&self) -> u32 {
        self.pages.first().copied().unwrap_or(0)
    }

    /// 最后一页（空选择为 0）。
    pub fn last(&self) -> u32 {
        self.pages.last().copied().unwrap_or(0)
    }
}

/// 按文档总页数裁剪：越界、空集、病态总页数全部显式拒。
pub fn clip(range: &PageRange, total: u32) -> Result<Clipped, RangeError> {
    if total == 0 || total > MAX_TOTAL {
        return Err(RangeError::Total);
    }
    let mut pages: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < range.segments.len() {
        let seg = match range.segments.get(i) {
            Some(v) => *v,
            None => return Err(RangeError::Run),
        };
        if seg.start == 0 || seg.end == 0 {
            return Err(RangeError::Zero);
        }
        if seg.start > seg.end {
            return Err(RangeError::Reversed);
        }
        if seg.end > total {
            return Err(RangeError::Beyond);
        }
        let mut p = seg.start;
        while p <= seg.end {
            pages.push(p);
            if p == seg.end {
                break;
            }
            p += 1;
        }
        i += 1;
    }
    if pages.is_empty() {
        return Err(RangeError::Empty);
    }
    let selected = pages.len() as u32;
    let out = Clipped {
        total,
        pages,
        runs: range.segments.clone(),
        skipped: total.saturating_sub(selected),
    };
    out.verify()?;
    Ok(out)
}

/// 下发游标：只按升序吐页，不回退、不跳过（直通链的页序保证）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageCursor {
    /// 选中页快照。
    pub pages: Vec<u32>,
    /// 当前位置。
    pub pos: usize,
}

impl PageCursor {
    /// 从裁剪结果建立游标。
    pub fn new(c: &Clipped) -> PageCursor {
        PageCursor {
            pages: c.pages.clone(),
            pos: 0,
        }
    }

    /// 下一页；耗尽返回 `None`。
    pub fn next(&mut self) -> Option<u32> {
        let v = self.pages.get(self.pos).copied();
        if v.is_some() {
            self.pos += 1;
        }
        v
    }

    /// 剩余页数。
    pub fn remaining(&self) -> usize {
        self.pages.len().saturating_sub(self.pos)
    }

    /// 复位到第一页（同一选择重复下发用）。
    pub fn reset(&mut self) {
        self.pos = 0;
    }
}

// ---------------------------------------------------------------------------
// 30 组选择 × 30 页裁剪核
// ---------------------------------------------------------------------------

const SPECS: [&str; 30] = [
    "1",
    "30",
    "1-30",
    "1-5",
    "28-30",
    "5-5",
    "1-3,7,9-12",
    "2-4,3-9",
    "10-20,21-25",
    "1-1,3-3,5-5",
    "7-",
    "-5",
    "1,2,3,4,5",
    "6-10,1-5",
    "30-30",
    "15",
    "2-29",
    "1-10,11-20,21-30",
    "3-7,5-9,12-15",
    "8,9,10,11",
    "1-2,29-30",
    "4-4,6-6,8-8",
    "12-18",
    "1-7,20-30",
    "25-",
    "-29",
    "2-2",
    "23-27,1-4",
    "9-9,10-10,11-11",
    "1-15,16-30",
];

/// 30 组选择在 30 页文档上的裁剪核。返回 (通过数, 总数)。
/// 通过条件同时包含：升序、无重复、守恒、往返规范化一致、游标顺序一致。
pub fn clip_thirty() -> (usize, usize) {
    let total = 30u32;
    let mut hits = 0usize;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            let ok = match parse_ranges(spec.as_bytes(), total) {
                Ok(r) => match clip(&r, total) {
                    Ok(c) => {
                        let mut asc = c.verify().is_ok();
                        let mut k = 1usize;
                        while k < c.pages.len() {
                            match (c.pages.get(k - 1), c.pages.get(k)) {
                                (Some(a), Some(b)) => {
                                    if *b != a.saturating_add(1) && *b <= *a {
                                        asc = false;
                                    }
                                    if *b <= *a {
                                        asc = false;
                                    }
                                }
                                _ => asc = false,
                            }
                            k += 1;
                        }
                        let rt = match parse_ranges(r.to_spec().as_bytes(), total) {
                            Ok(r2) => r2 == r && clip(&r2, total) == Ok(c.clone()),
                            Err(_) => false,
                        };
                        let mut cur = PageCursor::new(&c);
                        let mut seq_ok = true;
                        let mut expect = 0usize;
                        loop {
                            match cur.next() {
                                Some(p) => {
                                    if c.pages.get(expect) != Some(&p) {
                                        seq_ok = false;
                                    }
                                    expect += 1;
                                }
                                None => break,
                            }
                        }
                        let consume_ok = expect == c.pages.len()
                            && cur.remaining() == 0
                            && c.skipped as usize + c.pages.len() == total as usize;
                        asc && rt && seq_ok && consume_ok
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            };
            if ok {
                hits += 1;
            }
        }
        i += 1;
    }
    (hits, SPECS.len())
}

// ---------------------------------------------------------------------------
// 注入核：12 类坏样本
// ---------------------------------------------------------------------------

/// 12 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let total = 12usize;
    let mut caught = 0usize;

    if parse_ranges(b"", 30).err() == Some(RangeError::Empty) {
        caught += 1;
    }
    if parse_ranges(b" , ", 30).err() == Some(RangeError::Empty) {
        caught += 1;
    }
    if parse_ranges(b"0", 30).err() == Some(RangeError::Zero) {
        caught += 1;
    }
    if parse_ranges(b"5-3", 30).err() == Some(RangeError::Reversed) {
        caught += 1;
    }
    if parse_ranges(b"a", 30).err() == Some(RangeError::Syntax) {
        caught += 1;
    }
    if parse_ranges(b"1-2-3", 30).err() == Some(RangeError::Syntax) {
        caught += 1;
    }
    if parse_ranges(b"1,,3", 30).err() == Some(RangeError::Syntax) {
        caught += 1;
    }
    if parse_ranges(b"4294967296", 30).err() == Some(RangeError::Overflow) {
        caught += 1;
    }
    // 越界：解析通过，裁剪时被总页数挡住
    let beyond = match parse_ranges(b"1-31", 30) {
        Ok(r) => clip(&r, 30).err(),
        Err(e) => Some(e),
    };
    if beyond == Some(RangeError::Beyond) {
        caught += 1;
    }
    // 总页数 0
    if clip(&PageRange::empty(), 0).err() == Some(RangeError::Total) {
        caught += 1;
    }
    // 手工构造不守恒结果：页序 3,2 必须被 verify 挡住
    let bad = Clipped {
        total: 10,
        pages: {
            let mut v = Vec::new();
            v.push(3u32);
            v.push(2u32);
            v
        },
        runs: Vec::new(),
        skipped: 8,
    };
    if bad.verify().err() == Some(RangeError::Run) {
        caught += 1;
    }
    // 65 个原始段 → Capacity（上限 64）
    let mut many = String::new();
    let mut i = 0usize;
    while i < 65 {
        if i > 0 {
            many.push(',');
        }
        many.push('1');
        i += 1;
    }
    if parse_ranges(many.as_bytes(), 30).err() == Some(RangeError::Capacity) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50105 自检：16 条判据。
pub fn run_unx50105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50105");

    // J1-1 三型语法：单页 / 闭区间 / 两端开区间
    let a = parse_ranges(b"7", 30);
    let b = parse_ranges(b"3-9", 30);
    let c = parse_ranges(b"7-", 30);
    let d = parse_ranges(b"-5", 30);
    let ok = match (&a, &b, &c, &d) {
        (Ok(x), Ok(y), Ok(z), Ok(w)) => {
            x.segments == {
                let mut v = Vec::new();
                v.push(Segment::new(7, 7));
                v
            } && y.segments == {
                let mut v = Vec::new();
                v.push(Segment::new(3, 9));
                v
            } && z.segments == {
                let mut v = Vec::new();
                v.push(Segment::new(7, 30));
                v
            } && w.segments == {
                let mut v = Vec::new();
                v.push(Segment::new(1, 5));
                v
            }
        }
        _ => false,
    };
    s.add("50105-J1-三型语法", ok, if ok { "" } else { "开闭区间解析错" });

    // J1-2 规范化：重叠 + 相邻合并成一段
    let r = parse_ranges(b"2-4,3-9,10-12", 30);
    let ok = match &r {
        Ok(v) => {
            v.segments.len() == 1
                && v.segments.get(0) == Some(&Segment::new(2, 12))
                && v.count() == 11
        }
        Err(_) => false,
    };
    s.add("50105-J1-重叠合并", ok, if ok { "" } else { "重叠/相邻未合并" });

    // J1-3 乱序输入规范成升序（只裁剪不重排）；5 与 6 相邻合并为 1-10
    let r = parse_ranges(b"6-10,1-5", 30);
    let ok = match &r {
        Ok(v) => {
            v.segments.len() == 1
                && v.segments.get(0) == Some(&Segment::new(1, 10))
                && v.to_spec() == "1-10"
                && v.count() == 10
        }
        Err(_) => false,
    };
    let det = match &r {
        Ok(v) => v.to_spec(),
        Err(e) => format!("err=0x{:x}", e.code()),
    };
    s.add_dyn("50105-J1-乱序规范", ok, &det);

    // J1-4 裁剪守恒：选中 + 跳过 = 总页数，位数一致
    let r = parse_ranges(b"1-3,7,9-12", 30);
    let ok = match r {
        Ok(v) => match clip(&v, 30) {
            Ok(c) => {
                c.verify().is_ok()
                    && c.selected() == 8
                    && c.skipped == 22
                    && c.first() == 1
                    && c.last() == 12
                    && c.pages.get(3) == Some(&7)
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-裁剪守恒", ok, if ok { "" } else { "守恒破坏" });

    // J1-5 页序严格升序无重复
    let r = parse_ranges(b"9-12,1-3,7,5-9", 30);
    let ok = match r {
        Ok(v) => match clip(&v, 30) {
            Ok(c) => {
                let mut mono = true;
                let mut i = 1usize;
                while i < c.pages.len() {
                    match (c.pages.get(i - 1), c.pages.get(i)) {
                        (Some(a), Some(b)) => {
                            if b <= a {
                                mono = false;
                            }
                        }
                        _ => mono = false,
                    }
                    i += 1;
                }
                mono && c.selected() == 11
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-升序无重", ok, if ok { "" } else { "页序重排/重复" });

    // J1-6 往返：to_spec → parse → clip 完全一致
    let ok = match parse_ranges(b"2-4,3-9,12,14-20,21-25", 30) {
        Ok(v) => match (clip(&v, 30), parse_ranges(v.to_spec().as_bytes(), 30)) {
            (Ok(c), Ok(v2)) => v2 == v && clip(&v2, 30) == Ok(c),
            _ => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-规范化往返", ok, if ok { "" } else { "往返不一致" });

    // J1-7 30 组选择 ×30 页：页序一致零重排
    let (hits, total) = clip_thirty();
    let ok = hits == total && total == 30;
    let det = format!("裁剪 {}/{}", hits, total);
    s.add_dyn("50105-J1-裁剪30", ok, &det);

    // J1-8 游标：逐页吐出与选中序列逐位相同，耗尽后为 None
    let cursor_ok = match parse_ranges(b"4-6,10", 30) {
        Ok(v) => match clip(&v, 30) {
            Ok(c) => {
                let mut cur = PageCursor::new(&c);
                let mut want: Vec<u32> = Vec::new();
                let mut i = 0usize;
                while i < c.pages.len() {
                    if let Some(p) = c.pages.get(i) {
                        want.push(*p);
                    }
                    i += 1;
                }
                let mut got: Vec<u32> = Vec::new();
                loop {
                    match cur.next() {
                        Some(p) => got.push(p),
                        None => break,
                    }
                }
                got == want && cur.remaining() == 0
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-游标页序", cursor_ok, if cursor_ok { "" } else { "游标乱序" });

    // J1-9 越界必拒：31 页选择在 30 页文档上
    let ok = match parse_ranges(b"1-31", 30) {
        Ok(v) => clip(&v, 30).err() == Some(RangeError::Beyond),
        Err(_) => false,
    };
    s.add("50105-J1-越界必拒", ok, if ok { "" } else { "越界未拒" });

    // J1-10 五个码位互不串扰：Zero/Reversed/Syntax/Overflow/Total
    let ok = parse_ranges(b"0", 30).err() == Some(RangeError::Zero)
        && parse_ranges(b"5-3", 30).err() == Some(RangeError::Reversed)
        && parse_ranges(b"x", 30).err() == Some(RangeError::Syntax)
        && parse_ranges(b"99999999999", 30).err() == Some(RangeError::Overflow)
        && parse_ranges(b"1", 0).err() == Some(RangeError::Total);
    s.add("50105-J1-码位分工", ok, if ok { "" } else { "诊断码串扰" });

    // J1-11 容量：65 段拒，64 段收
    let build = |n: usize| -> String {
        let mut t = String::new();
        let mut i = 0usize;
        while i < n {
            if i > 0 {
                t.push(',');
            }
            t.push('1');
            i += 1;
        }
        t
    };
    let ok = parse_ranges(build(65).as_bytes(), 30).err() == Some(RangeError::Capacity)
        && parse_ranges(build(64).as_bytes(), 30).is_ok();
    s.add("50105-J1-段数上限", ok, if ok { "" } else { "容量边界错" });

    // J1-12 内部自证：手工坏结果必须 Run
    let bad = Clipped {
        total: 10,
        pages: {
            let mut v = Vec::new();
            v.push(2u32);
            v.push(2u32);
            v
        },
        runs: Vec::new(),
        skipped: 8,
    };
    let ok = bad.verify().err() == Some(RangeError::Run);
    s.add("50105-J1-自证拦截", ok, if ok { "" } else { "坏结果未拦" });

    // J1-13 确定性：同文本两次解析/裁剪逐字段一致
    let ok = match (parse_ranges(b"3-7,1-2,20-25", 30), parse_ranges(b"3-7,1-2,20-25", 30)) {
        (Ok(a), Ok(b)) => a == b && clip(&a, 30) == clip(&b, 30),
        _ => false,
    };
    s.add("50105-J1-确定重复", ok, if ok { "" } else { "非确定" });

    // J1-14 单页选择：first==last，跳过 = total-1
    let ok = match parse_ranges(b"17", 30) {
        Ok(v) => match clip(&v, 30) {
            Ok(c) => c.selected() == 1 && c.first() == 17 && c.last() == 17 && c.skipped == 29,
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-单页", ok, if ok { "" } else { "单页语义错" });

    // J1-15 摘要非空（软件面写回）
    let ok = match parse_ranges(b"1-3", 30) {
        Ok(v) => match clip(&v, 30) {
            Ok(c) => {
                let sum = c.summary();
                !sum.is_empty() && sum.contains("selected") && sum.contains("runs=1")
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50105-J1-摘要非空", ok, if ok { "" } else { "摘要空" });

    // J1-16 12 类注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total && total == 12;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50105-J1-注入12", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50105_checks_green() {
        let set = run_unx50105_checks();
        assert!(set.all_passed(), "UNX-F50105 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50105_thirty_specs() {
        let (hits, total) = clip_thirty();
        assert_eq!(total, 30);
        assert_eq!(hits, total);
    }

    #[test]
    fn unx50105_injection_12() {
        let (caught, total) = injection_core();
        assert_eq!(total, 12);
        assert_eq!(caught, total);
    }

    #[test]
    fn unx50105_no_reorder() {
        let r = parse_ranges(b"9-12,1-3,7", 30);
        assert!(r.is_ok());
        if let Ok(v) = r {
            let c = clip(&v, 30);
            assert!(c.is_ok());
            if let Ok(x) = c {
                assert_eq!(x.pages.first(), Some(&1u32));
                assert_eq!(x.pages.last(), Some(&12u32));
                assert_eq!(x.selected(), 8);
            }
        }
    }
}
