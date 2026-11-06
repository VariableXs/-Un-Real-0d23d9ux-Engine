//! UNX-F50106 直通任务副本数语义（打印机侧 copies 透传档）（0x43960 段）
//!
//! 锚点：UNX-F50106-J1 副本 ×10 透传一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43960`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 9`，
//! 逐项见 [`CopyError`]。
//!
//! 打印机侧的 `copies` 不是「重复打印几遍」这么简单，直通档必须把三件事
//! 同时说清楚：
//! 1. **份数**——`1..=99`，0 份是空作业、100 份以上超出设备面板档，
//!    两者都不允许被静默钳位；
//! 2. **分份方式**——`Collated`（逐份：每份走完整页序）与 `Uncollated`
//!    （逐页：同一页先印完所有份）。同一票数据在两种模式下「份内容」一致、
//!    「全局顺序」不同；
//! 3. **留痕**——每份一行 [`CopyRecord`]，序号与份号严格对应，印章由
//!    (job, 份号, 页摘要) 复算，两两不同，防止「一份冒充十份」。
//!
//! 结构清单：
//! * [`CopyError`]：诊断闭集；
//! * [`CopyMode`] / [`CopySpec`] / [`parse_copies`]：份数与分份语义；
//! * [`CopyPlan`]：页序 × 份数的展开计划（含摘要与印章）；
//! * [`CopyLedger`] / [`CopyRecord`] / [`deliver`]：逐份留痕账本；
//! * [`copies_ten`]：10 份透传一致性核；
//! * [`injection_core`]：12 例坏样本注入核。
//!
//! 错误纪律：文本解析走 u64 饱和中转；所有切片先 `get`；索引不进
//! 裸下标；账本只追加、封账对顺序敏感。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43960 独占 0x20）。
pub const SEG_BASE: u32 = 0x43960;

const E_SYNTAX: u16 = 1;
const E_EMPTY: u16 = 2;
const E_ZERO: u16 = 3;
const E_LIMIT: u16 = 4;
const E_OVERFLOW: u16 = 5;
const E_MODE: u16 = 6;
const E_PAYLOAD: u16 = 7;
const E_ORDER: u16 = 8;
const E_CAPACITY: u16 = 9;

/// 最少份数（1 份）。
pub const COPY_MIN: u16 = 1;
/// 设备面板可透传的最大份数。
pub const COPY_MAX: u16 = 99;
/// 单票允许的最大页数（与直通档的页数上限一致）。
pub const MAX_PAGES: usize = 4096;
/// 账本最大行数（≥ 份数上限，留出对照余量）。
pub const MAX_LEDGER: usize = 256;
/// 展开序列的安全上限：超过它就走显式 Capacity，而不是把内存拖爆。
pub const MAX_SHEETS: u32 = 65_536;

/// 副本语义诊断。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyError {
    /// 文本含非数字字符。
    Syntax,
    /// 作业未分配或份数文本为空。
    Empty,
    /// 份数为 0。
    Zero,
    /// 份数超过设备档（> 99）。
    Limit,
    /// 数字超过 u32。
    Overflow,
    /// 分份方式标签不在闭集内。
    Mode,
    /// 页载荷缺失或含 0 页。
    Payload,
    /// 页序/份号/账本序号次序错误。
    Order,
    /// 页数、份数展开或账本容量超限。
    Capacity,
}

impl CopyError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            CopyError::Syntax => E_SYNTAX,
            CopyError::Empty => E_EMPTY,
            CopyError::Zero => E_ZERO,
            CopyError::Limit => E_LIMIT,
            CopyError::Overflow => E_OVERFLOW,
            CopyError::Mode => E_MODE,
            CopyError::Payload => E_PAYLOAD,
            CopyError::Order => E_ORDER,
            CopyError::Capacity => E_CAPACITY,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            CopyError::Syntax => "copies text has non-digit byte",
            CopyError::Empty => "job id or copies text empty",
            CopyError::Zero => "copies count is zero",
            CopyError::Limit => "copies count above device limit",
            CopyError::Overflow => "copies number overflows u32",
            CopyError::Mode => "copy mode tag unknown",
            CopyError::Payload => "page payload missing or zero page",
            CopyError::Order => "sequence or ledger order broken",
            CopyError::Capacity => "page, sheet or ledger capacity exceeded",
        }
    }
}

/// 分份方式（打印机侧语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMode {
    /// 逐份：第 1 份走完整页序，再第 2 份。
    Collated,
    /// 逐页：同一页印完所有份，再下一页。
    Uncollated,
}

impl CopyMode {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            CopyMode::Collated => "collated",
            CopyMode::Uncollated => "uncollated",
        }
    }

    /// 稳定标签值（用于复算与序列化）。
    pub fn tag(self) -> u8 {
        match self {
            CopyMode::Collated => 1,
            CopyMode::Uncollated => 2,
        }
    }

    /// 由标签还原；闭集外显式报 Mode。
    pub fn from_tag(tag: u8) -> Result<CopyMode, CopyError> {
        match tag {
            1 => Ok(CopyMode::Collated),
            2 => Ok(CopyMode::Uncollated),
            _ => Err(CopyError::Mode),
        }
    }
}

/// 份数规格：份数 + 分份方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopySpec {
    /// 份数（1..=99）。
    pub copies: u16,
    /// 分份方式。
    pub mode: CopyMode,
}

impl CopySpec {
    /// 构造。
    pub const fn new(copies: u16, mode: CopyMode) -> CopySpec {
        CopySpec { copies, mode }
    }

    /// 校验：0 份与超档份数都显式拒，不钳位。
    pub fn validate(&self) -> Result<(), CopyError> {
        if self.copies == 0 {
            return Err(CopyError::Zero);
        }
        if self.copies > COPY_MAX {
            return Err(CopyError::Limit);
        }
        Ok(())
    }
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

/// 解析份数文本（两端允许空白）。`0` 是 Zero，`> 99` 是 Limit。
pub fn parse_copies(text: &[u8]) -> Result<u16, CopyError> {
    let body = trim(text);
    if body.is_empty() {
        return Err(CopyError::Empty);
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < body.len() {
        let b = match body.get(i) {
            Some(x) => *x,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(CopyError::Syntax);
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
        if v > u32::MAX as u64 {
            return Err(CopyError::Overflow);
        }
        i += 1;
    }
    if v == 0 {
        return Err(CopyError::Zero);
    }
    if v > COPY_MAX as u64 {
        return Err(CopyError::Limit);
    }
    Ok(v as u16)
}

/// FNV 喂字节。
fn feed(h: &mut u32, v: u32) {
    *h ^= v;
    *h = (*h).wrapping_mul(0x0100_0193);
}

/// 页摘要：job + 页序（与份号无关，十份共享同一内容摘要）。
fn digest_of(job: u32, pages: &[u32]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    feed(&mut h, job);
    let mut i = 0usize;
    while i < pages.len() {
        if let Some(p) = pages.get(i) {
            feed(&mut h, *p);
        }
        i += 1;
    }
    h
}

/// 序列切片摘要：用于核对「某一份吐出的页是否就是选中页序」。
fn digest_span(job: u32, seq: &[(u16, u32)], start: usize, end: usize) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    feed(&mut h, job);
    let mut i = start;
    while i < end {
        if let Some((_k, p)) = seq.get(i) {
            feed(&mut h, *p);
        }
        i += 1;
    }
    h
}

/// 份展开计划：一票作业 × 份数 × 页序的完整描述。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPlan {
    /// 作业号（0 = 未分配）。
    pub job: u32,
    /// 份数规格。
    pub spec: CopySpec,
    /// 选中页（严格升序、1 起、无重复）。
    pub pages: Vec<u32>,
    /// 页摘要（份内容一致性基准）。
    pub page_digest: u32,
}

impl CopyPlan {
    /// 份数。
    pub fn copies(&self) -> u16 {
        self.spec.copies
    }

    /// 每份页数（u16 饱和）。
    pub fn page_count(&self) -> u16 {
        if self.pages.len() > u16::MAX as usize {
            u16::MAX
        } else {
            self.pages.len() as u16
        }
    }

    /// 总面数 = 页数 × 份数（u32 饱和）。
    pub fn sheets(&self) -> u32 {
        (self.pages.len() as u32).saturating_mul(self.spec.copies as u32)
    }

    /// 第 `copy` 份的印章：含份号，十份两两不同。
    pub fn copy_digest(&self, copy: u16) -> u32 {
        let mut h = self.page_digest;
        feed(&mut h, copy as u32);
        feed(&mut h, self.spec.mode.tag() as u32);
        if h == 0 {
            1
        } else {
            h
        }
    }

    /// 展开成 (份号, 页号) 序列；逐份/逐页顺序不同但面数一致。
    pub fn sequence(&self) -> Result<Vec<(u16, u32)>, CopyError> {
        let total = self.sheets();
        if total > MAX_SHEETS {
            return Err(CopyError::Capacity);
        }
        let mut out: Vec<(u16, u32)> = Vec::with_capacity(total as usize);
        match self.spec.mode {
            CopyMode::Collated => {
                let mut k = COPY_MIN;
                loop {
                    let mut i = 0usize;
                    while i < self.pages.len() {
                        if let Some(p) = self.pages.get(i) {
                            out.push((k, *p));
                        }
                        i += 1;
                    }
                    if k >= self.spec.copies {
                        break;
                    }
                    k = k.saturating_add(1);
                }
            }
            CopyMode::Uncollated => {
                let mut i = 0usize;
                while i < self.pages.len() {
                    if let Some(p) = self.pages.get(i) {
                        let mut k = COPY_MIN;
                        loop {
                            out.push((k, *p));
                            if k >= self.spec.copies {
                                break;
                            }
                            k = k.saturating_add(1);
                        }
                    }
                    i += 1;
                }
            }
        }
        Ok(out)
    }

    /// 一行摘要（软件面写回）。
    pub fn summary(&self) -> String {
        format!(
            "job={} copies={} mode={} pages={} sheets={} digest=0x{:x}",
            self.job,
            self.spec.copies,
            self.spec.mode.label(),
            self.pages.len(),
            self.sheets(),
            self.page_digest
        )
    }
}

/// 建立展开计划；作业号、份数、页载荷、页序全部显式校验。
pub fn plan(job: u32, spec: CopySpec, pages: &[u32]) -> Result<CopyPlan, CopyError> {
    if job == 0 {
        return Err(CopyError::Empty);
    }
    spec.validate()?;
    if pages.is_empty() {
        return Err(CopyError::Payload);
    }
    if pages.len() > MAX_PAGES {
        return Err(CopyError::Capacity);
    }
    let mut i = 0usize;
    let mut prev = 0u32;
    while i < pages.len() {
        let p = match pages.get(i) {
            Some(v) => *v,
            None => return Err(CopyError::Payload),
        };
        if p == 0 {
            return Err(CopyError::Payload);
        }
        if p <= prev {
            return Err(CopyError::Order);
        }
        prev = p;
        i += 1;
    }
    Ok(CopyPlan {
        job,
        spec,
        pages: pages.to_vec(),
        page_digest: digest_of(job, pages),
    })
}

/// 逐份留痕行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyRecord {
    /// 账本序号（1 起，等于份号）。
    pub seq: u32,
    /// 份号。
    pub copy: u16,
    /// 该份页数。
    pub pages: u16,
    /// 该份印章。
    pub stamp: u32,
}

/// 逐份审计账本（append-only）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyLedger {
    /// 容量。
    pub capacity: usize,
    /// 该账本服务的份数上限。
    pub copies: u16,
    /// 行。
    pub records: Vec<CopyRecord>,
}

impl CopyLedger {
    /// 新建。
    pub fn new(capacity: usize, copies: u16) -> CopyLedger {
        CopyLedger {
            capacity,
            copies,
            records: Vec::new(),
        }
    }

    /// 追加一份。份号必须等于下一个序号（第 n 行必须是第 n 份）。
    pub fn append(&mut self, copy: u16, pages: u16, stamp: u32) -> Result<u32, CopyError> {
        if self.records.len() >= self.capacity {
            return Err(CopyError::Capacity);
        }
        if copy == 0 || copy > self.copies {
            return Err(CopyError::Order);
        }
        let seq = self.records.len() as u32 + 1;
        if copy as u32 != seq {
            return Err(CopyError::Order);
        }
        self.records.push(CopyRecord {
            seq,
            copy,
            pages,
            stamp,
        });
        Ok(seq)
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 最后一行。
    pub fn last(&self) -> Option<&CopyRecord> {
        let n = self.records.len();
        if n == 0 {
            None
        } else {
            self.records.get(n - 1)
        }
    }

    /// 某份的行数（正常应为 0 或 1；重复交付会暴露）。
    pub fn count_copy(&self, copy: u16) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.copy == copy {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 链式封账（对行顺序敏感）。
    pub fn seal(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                feed(&mut h, r.seq);
                feed(&mut h, r.copy as u32);
                feed(&mut h, r.pages as u32);
                feed(&mut h, r.stamp);
            }
            i += 1;
        }
        h
    }

    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ledger copies={} rows={} seal=0x{:x}",
            self.copies,
            self.records.len(),
            self.seal()
        )
    }
}

/// 把计划落成逐份账本：份号顺序、印章两两不同、页数逐份一致。
pub fn deliver(p: &CopyPlan) -> Result<CopyLedger, CopyError> {
    let copies = p.copies();
    let cap = if (copies as usize) > MAX_LEDGER {
        MAX_LEDGER
    } else {
        copies as usize
    };
    let mut led = CopyLedger::new(cap, copies);
    let mut k = COPY_MIN;
    loop {
        let stamp = p.copy_digest(k);
        led.append(k, p.page_count(), stamp)?;
        if k >= copies {
            break;
        }
        k = k.saturating_add(1);
    }
    // 印章两两不同：同一枚章不能冒充两份。
    let mut a = 0usize;
    while a < led.records.len() {
        let mut b = a + 1;
        while b < led.records.len() {
            match (led.records.get(a), led.records.get(b)) {
                (Some(x), Some(y)) => {
                    if x.stamp == y.stamp {
                        return Err(CopyError::Order);
                    }
                }
                _ => return Err(CopyError::Order),
            }
            b += 1;
        }
        a += 1;
    }
    Ok(led)
}

// ---------------------------------------------------------------------------
// 10 份透传一致性核
// ---------------------------------------------------------------------------

/// 10 份 × 5 页的透传核：逐份核对「内容摘要一致、印章互异、账本行对齐，
/// 且该份吐出的页序切片与原页序同摘要」。返回 (通过份数, 总份数)。
pub fn copies_ten() -> (usize, usize) {
    const TEN: usize = 10;
    let spec = CopySpec::new(10, CopyMode::Collated);
    let pages = [1u32, 2, 3, 4, 5];
    let p = match plan(0x0050_1060, spec, &pages) {
        Ok(v) => v,
        Err(_) => return (0, TEN),
    };
    let led = match deliver(&p) {
        Ok(v) => v,
        Err(_) => return (0, TEN),
    };
    let seq = match p.sequence() {
        Ok(v) => v,
        Err(_) => return (0, TEN),
    };
    let n = p.pages.len();
    let mut hits = 0usize;
    let mut k = COPY_MIN;
    loop {
        let mut ok = p.sheets() == 50;
        // 份内容摘要与整票页摘要一致。
        let start = ((k as usize) - 1).saturating_mul(n);
        let end = start.saturating_add(n);
        ok = ok
            && digest_span(p.job, &seq, start, end) == p.page_digest
            && p.copy_digest(k) != p.page_digest;
        // 账本第 k 行必须恰好是第 k 份。
        match led.records.get((k as usize) - 1) {
            Some(r) => {
                ok = ok && r.copy == k && r.seq == k as u32 && r.pages == p.page_count();
                ok = ok && r.stamp == p.copy_digest(k);
            }
            None => ok = false,
        }
        // 其余份的印章不得与之相同（抽样全量两两核对由 deliver 保证，
        // 这里再核对相邻份，防止账本被外部篡改）。
        if k > COPY_MIN {
            match (led.records.get((k as usize) - 2), led.records.get((k as usize) - 1)) {
                (Some(x), Some(y)) => {
                    if x.stamp == y.stamp {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
        }
        if ok {
            hits += 1;
        }
        if k >= 10 {
            break;
        }
        k = k.saturating_add(1);
    }
    (hits, TEN)
}

// ---------------------------------------------------------------------------
// 注入核：12 例坏样本
// ---------------------------------------------------------------------------

/// 12 例坏样本，覆盖 9 个诊断码，逐例断言具体码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;
    let good_pages = [1u32, 2, 3];
    let spec1 = CopySpec::new(1, CopyMode::Collated);

    // 1) 作业号为 0
    if plan(0, spec1, &good_pages).err() == Some(CopyError::Empty) {
        caught += 1;
    }
    // 2) 份数文本为空
    if parse_copies(b"").err() == Some(CopyError::Empty) {
        caught += 1;
    }
    // 3) 份数 0
    if parse_copies(b"0").err() == Some(CopyError::Zero) {
        caught += 1;
    }
    // 4) 份数超档 100
    if parse_copies(b"100").err() == Some(CopyError::Limit) {
        caught += 1;
    }
    // 5) 数字超 u32
    if parse_copies(b"4294967296").err() == Some(CopyError::Overflow) {
        caught += 1;
    }
    // 6) 非数字
    if parse_copies(b"4x").err() == Some(CopyError::Syntax) {
        caught += 1;
    }
    // 7) 分份标签闭集外
    if CopyMode::from_tag(9).err() == Some(CopyError::Mode) {
        caught += 1;
    }
    // 8) 页载荷为空
    if plan(7, spec1, &[]).err() == Some(CopyError::Payload) {
        caught += 1;
    }
    // 9) 页载荷含 0 页
    if plan(7, spec1, &[0u32]).err() == Some(CopyError::Payload) {
        caught += 1;
    }
    // 10) 页序倒置
    if plan(7, spec1, &[5u32, 4]).err() == Some(CopyError::Order) {
        caught += 1;
    }
    // 11) 账本首行必须是第 1 份
    let mut led = CopyLedger::new(3, 3);
    if led.append(2, 1, 0xabc).err() == Some(CopyError::Order) {
        caught += 1;
    }
    // 12) 账本容量满
    let mut led = CopyLedger::new(2, 3);
    let _ = led.append(1, 1, 0x11);
    let _ = led.append(2, 1, 0x22);
    if led.append(3, 1, 0x33).err() == Some(CopyError::Capacity) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F50106 自检：13 条判据。
pub fn run_unx50106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50106");

    // J1-1 核心：10 份 × 5 页透传一致
    let (hits, total) = copies_ten();
    let ok = hits == total && total == 10;
    let det = format!("透传 {}/{}", hits, total);
    s.add_dyn("50106-J1-副本10", ok, &det);

    // J1-2 单份：面数 = 页数，账本 1 行，印章非 0
    let single = match plan(7, CopySpec::new(1, CopyMode::Collated), &[3u32, 4]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let led = deliver(&single);
    let ok = match &led {
        Ok(l) => {
            single.sheets() == 2
                && l.len() == 1
                && l.count_copy(1) == 1
                && single.copy_digest(1) != 0
                && l.last().map(|r| r.pages) == Some(2)
        }
        Err(_) => false,
    };
    s.add("50106-J1-单份", ok, if ok { "" } else { "单份语义错" });

    // J1-3 面数守恒：7 页 × 6 份 = 42 面，每份 7 页
    let six = match plan(8, CopySpec::new(6, CopyMode::Collated), &[2u32, 4, 6, 8, 10, 12, 14]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = match six.sequence() {
        Ok(seq) => {
            // 逐份点页数：每一份都必须恰好 7 页，总量 42 面。
            let mut all_seven = true;
            let mut k = 1u16;
            loop {
                let mut n = 0usize;
                let mut i = 0usize;
                while i < seq.len() {
                    if let Some((c, _p)) = seq.get(i) {
                        if *c == k {
                            n += 1;
                        }
                    }
                    i += 1;
                }
                if n != 7 {
                    all_seven = false;
                }
                if k >= 6 {
                    break;
                }
                k = k.saturating_add(1);
            }
            seq.len() == 42 && six.sheets() == 42 && all_seven
        }
        Err(_) => false,
    };
    s.add("50106-J1-面数守恒", ok, if ok { "" } else { "面数不守恒" });

    // J1-4 逐份顺序：1,2 × 3 份 → (1,1)(1,2)(2,1)(2,2)(3,1)(3,2)
    let coll = match plan(9, CopySpec::new(3, CopyMode::Collated), &[1u32, 2]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = match coll.sequence() {
        Ok(seq) => {
            seq.get(0) == Some(&(1u16, 1u32))
                && seq.get(1) == Some(&(1, 2))
                && seq.get(2) == Some(&(2, 1))
                && seq.get(3) == Some(&(2, 2))
                && seq.get(4) == Some(&(3, 1))
                && seq.get(5) == Some(&(3, 2))
                && seq.len() == 6
        }
        Err(_) => false,
    };
    s.add("50106-J1-逐份顺序", ok, if ok { "" } else { "逐份顺序错" });

    // J1-5 逐页顺序：同一页先把 N 份印完
    let unc = match plan(9, CopySpec::new(3, CopyMode::Uncollated), &[1u32, 2]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = match unc.sequence() {
        Ok(seq) => {
            seq.get(0) == Some(&(1u16, 1u32))
                && seq.get(1) == Some(&(2, 1))
                && seq.get(2) == Some(&(3, 1))
                && seq.get(3) == Some(&(1, 2))
                && seq.get(4) == Some(&(2, 2))
                && seq.get(5) == Some(&(3, 2))
                && seq.len() == 6
                && unc.sheets() == coll.sheets()
        }
        Err(_) => false,
    };
    s.add("50106-J1-逐页顺序", ok, if ok { "" } else { "逐页顺序错" });

    // J1-6 边界：1 与 99 收，0 与 100 拒；标签往返一致
    let ok = parse_copies(b"1") == Ok(1)
        && parse_copies(b"99") == Ok(99)
        && parse_copies(b" 5 ") == Ok(5)
        && parse_copies(b"0").err() == Some(CopyError::Zero)
        && parse_copies(b"100").err() == Some(CopyError::Limit)
        && plan(7, CopySpec::new(0, CopyMode::Collated), &[1u32]).err() == Some(CopyError::Zero)
        && plan(7, CopySpec::new(100, CopyMode::Collated), &[1u32]).err() == Some(CopyError::Limit)
        && CopyMode::from_tag(CopyMode::Collated.tag()) == Ok(CopyMode::Collated)
        && CopyMode::from_tag(CopyMode::Uncollated.tag()) == Ok(CopyMode::Uncollated);
    s.add("50106-J1-份数边界", ok, if ok { "" } else { "边界钳位/串码" });

    // J1-7 溢出与超档分家的码位：超 u32 走 Overflow，超档走 Limit
    let ok = parse_copies(b"4294967296").err() == Some(CopyError::Overflow)
        && parse_copies(b"100000").err() == Some(CopyError::Limit)
        && parse_copies(b"x").err() == Some(CopyError::Syntax)
        && parse_copies(b"-3").err() == Some(CopyError::Syntax)
        && parse_copies(b"+3").err() == Some(CopyError::Syntax);
    s.add("50106-J1-码位分工", ok, if ok { "" } else { "Overflow/Limit 串扰" });

    // J1-8 账本：10 行序号 1..10、每份一行、封账非 0
    let ten = match plan(10, CopySpec::new(10, CopyMode::Uncollated), &[1u32, 2, 3, 4, 5]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = match deliver(&ten) {
        Ok(l) => {
            let mut mono = l.len() == 10;
            let mut i = 0usize;
            while i < l.records.len() {
                match l.records.get(i) {
                    Some(r) => {
                        if r.seq as usize != i + 1 || r.copy as usize != i + 1 {
                            mono = false;
                        }
                    }
                    None => mono = false,
                }
                i += 1;
            }
            let mut each = true;
            let mut k = 1u16;
            loop {
                if l.count_copy(k) != 1 {
                    each = false;
                }
                if k >= 10 {
                    break;
                }
                k += 1;
            }
            mono && each && l.seal() != 0 && l.summary().contains("rows=10")
        }
        Err(_) => false,
    };
    let det = match deliver(&ten) {
        Ok(l) => l.summary(),
        Err(e) => format!("err=0x{:x}", e.code()),
    };
    s.add_dyn("50106-J1-账本对齐", ok, &det);

    // J1-9 乱序追加与满容都显式拒
    let mut led = CopyLedger::new(3, 3);
    let first_bad = led.append(2, 1, 0xabc).err() == Some(CopyError::Order);
    let ok1 = led.append(1, 1, 0x111).is_ok();
    let dup = led.append(1, 1, 0x222).err() == Some(CopyError::Order);
    let ok2 = led.append(2, 1, 0x333).is_ok() && led.append(3, 1, 0x444).is_ok();
    let cap = led.append(4, 1, 0x555).err() == Some(CopyError::Capacity);
    let ok = first_bad && ok1 && dup && ok2 && cap;
    s.add("50106-J1-乱序必拒", ok, if ok { "" } else { "乱序/满容未拒" });

    // J1-10 确定性：同输入两次计划逐字段一致，两次交付封账一致
    let a = plan(11, CopySpec::new(4, CopyMode::Collated), &[3u32, 6, 9]);
    let b = plan(11, CopySpec::new(4, CopyMode::Collated), &[3u32, 6, 9]);
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => {
            x == y
                && x.summary() == y.summary()
                && match (deliver(x), deliver(y)) {
                    (Ok(l1), Ok(l2)) => l1 == l2 && l1.seal() == l2.seal(),
                    _ => false,
                }
        }
        _ => false,
    };
    s.add("50106-J1-确定重复", ok, if ok { "" } else { "非确定" });

    // J1-11 页范围 × 副本：3 页（2,5,7）× 4 份 = 12 面，逐份切片同摘要
    let mixed = match plan(12, CopySpec::new(4, CopyMode::Collated), &[2u32, 5, 7]) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok = match mixed.sequence() {
        Ok(seq) => {
            let mut same = mixed.sheets() == 12 && seq.len() == 12;
            let mut k = 0usize;
            while k < 4 {
                let start = k.saturating_mul(3);
                if digest_span(mixed.job, &seq, start, start + 3) != mixed.page_digest {
                    same = false;
                }
                k += 1;
            }
            same && mixed.page_count() == 3
        }
        Err(_) => false,
    };
    s.add("50106-J1-范围副本", ok, if ok { "" } else { "范围×副本不一致" });

    // J1-12 摘要非空（软件面写回）
    let ok = match plan(13, CopySpec::new(10, CopyMode::Collated), &[1u32, 2, 3, 4, 5]) {
        Ok(p) => {
            let sum = p.summary();
            !sum.is_empty()
                && sum.contains("copies=10")
                && sum.contains("sheets=50")
                && sum.contains("collated")
        }
        Err(_) => false,
    };
    s.add("50106-J1-摘要非空", ok, if ok { "" } else { "摘要空" });

    // J1-13 12 例注入全抓
    let (caught, total) = injection_core();
    let ok = caught == total && total == 12;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50106-J1-注入12", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50106_checks_green() {
        let set = run_unx50106_checks();
        assert!(set.all_passed(), "UNX-F50106 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50106_copies_ten() {
        let (hits, total) = copies_ten();
        assert_eq!(total, 10);
        assert_eq!(hits, total);
    }

    #[test]
    fn unx50106_injection_12() {
        let (caught, total) = injection_core();
        assert_eq!(total, 12);
        assert_eq!(caught, total);
    }

    #[test]
    fn unx50106_single_copy() {
        let spec = CopySpec::new(1, CopyMode::Collated);
        let p = plan(21, spec, &[2u32, 4]);
        assert!(p.is_ok());
        if let Ok(v) = p {
            assert_eq!(v.sheets(), 2);
            assert_eq!(v.page_count(), 2);
            let led = deliver(&v);
            assert!(led.is_ok());
            if let Ok(l) = led {
                assert_eq!(l.len(), 1);
                assert_eq!(l.last().map(|r| r.copy), Some(1));
            }
        }
    }
}
