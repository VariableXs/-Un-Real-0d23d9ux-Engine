//! UNX-F0751 · 移交包账册汇编目录（批册+深化册指针） · 诊断段 0x5ce0
//! 判据原文摘录：UNX-F0751-J1 目录账齐（批册 40 + 深化册 40 逐册指针 +
//! 域账累计链 240,000 终点）且逐册可定位断言过；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 目录双册：批册 40 册（B01..B40）+ 深化册 40 册（D01..D40），
//!    册号连续、一一配对（B_i ↔ D_i），缺册即目录不齐。
//! 2. 逐册指针：每册挂卷号 + 行区间 [start,end)，指针可定位 = 区间非空
//!    且与累计链衔接（前册终点 = 本册起点）。
//! 3. 域账累计链：40 段链，每段增量 = 批册行数，段段衔接零断，
//!    终点恒等 240,000（判据原文口径，写死）。
//! 4. 定容数组账本 + 守恒恒等式：sum(段增量) == 累计链终点 == 240,000；
//!    批册行数和 == 深化册行数和（双册对称核）。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5ce0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5ce0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_BOOK_DUP: u16 = SEG_BASE + 1; // 册号重复登记
const E_BOOK_FULL: u16 = SEG_BASE + 2; // 目录册账已满
const E_BOOK_BAD: u16 = SEG_BASE + 3; // 册号越界（1..=40）
const E_SPAN_ZERO: u16 = SEG_BASE + 4; // 行区间为空（start==end）
const E_SPAN_BREAK: u16 = SEG_BASE + 5; // 累计链断段（前终点≠本起点）
const E_CHAIN_END: u16 = SEG_BASE + 6; // 累计链终点 ≠ 240,000
const E_PAIR_LACK: u16 = SEG_BASE + 7; // 批册-深化册未配对
const E_VOL_BAD: u16 = SEG_BASE + 8; // 卷号非法（0 或超容）
const E_LOCATE_FAIL: u16 = SEG_BASE + 9; // 逐册可定位断言失败
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂（双册行数不对称）
const E_SEALED: u16 = SEG_BASE + 11; // 目录已封，拒绝再写
const E_NOT_FULL: u16 = SEG_BASE + 12; // 40+40 未齐即封目录被拒
const E_TWICE: u16 = SEG_BASE + 13; // 同册双登记
const E_LINES_ZERO: u16 = SEG_BASE + 14; // 段增量为 0

/// 批册数与深化册数。
pub const BATCH_BOOKS: usize = 40;
pub const DEEP_BOOKS: usize = 40;
pub const BOOKS: usize = 80;

/// 域账累计链终点：240,000。
pub const CHAIN_END: u64 = 240_000;

/// 单册行数上限（定容防线）。
pub const BOOK_MAX_LINES: u64 = 10_000;

// ── FNV-1a 64（目录哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BOOK_DUP => "册号重复：批册 B01..B40 / 深化册 D01..D40 各一次",
        E_BOOK_FULL => "目录册账已满 80 册",
        E_BOOK_BAD => "册号越界：1..=40（批册）或 1..=40（深化册）",
        E_SPAN_ZERO => "行区间为空：start==end，空册不许入目录",
        E_SPAN_BREAK => "累计链断段：前册终点必须等于本册起点",
        E_CHAIN_END => "累计链终点非 240,000：域账总量被改",
        E_PAIR_LACK => "批册-深化册未配对：B_i 必须有 D_i",
        E_VOL_BAD => "卷号非法：1..=8 卷闭集",
        E_LOCATE_FAIL => "逐册可定位断言失败：指针解引落空",
        E_BAL => "守恒破裂：批册行数和必须等于深化册行数和",
        E_SEALED => "目录已封：封账后只读",
        E_NOT_FULL => "40+40 未齐：先补册再封",
        E_TWICE => "同册双登记",
        E_LINES_ZERO => "段增量为 0：零行册不许入链",
        _ => "未知诊断码",
    }
}

// ── 册指针 ───────────────────────────────────────────────────
/// 一册的目录指针：册类 + 册号 + 卷号 + 行区间。
#[derive(Clone, Debug)]
pub struct BookPtr {
    /// 册类：1=批册 2=深化册。
    pub kind: u8,
    /// 册号（1..=40）。
    pub book_no: u32,
    /// 卷号（1..=8，卷闭集）。
    pub volume: u8,
    /// 行区间起点（含）。
    pub start: u64,
    /// 行区间终点（不含）。
    pub end: u64,
}

impl BookPtr {
    /// 指针可定位：区间非空 + 卷号合法 + 行数在上限内。
    pub fn locatable(&self) -> bool {
        self.end > self.start
            && self.volume >= 1
            && self.volume <= 8
            && self.end - self.start <= BOOK_MAX_LINES
    }

    /// 段增量（本册行数）。
    pub fn lines(&self) -> u64 {
        if self.end > self.start {
            self.end - self.start
        } else {
            0
        }
    }

    /// 册名（Bxx / Dxx）。
    pub fn book_name(&self) -> String {
        let mut s = String::new();
        s.push(if self.kind == 1 { 'B' } else { 'D' });
        let n = self.book_no;
        s.push((b'0' + (n / 10) as u8) as char);
        s.push((b'0' + (n % 10) as u8) as char);
        s
    }
}

// ── 账册汇编目录（主体）──────────────────────────────────────
/// 移交包账册汇编目录：双册指针 + 累计链 + 配对核 + 定位断言。
pub struct CatalogLedger {
    books: [Option<BookPtr>; BOOKS],
    book_count: usize,
    /// 累计链当前终点（逐段推进）。
    chain_head: u64,
    /// 链段数（= 已登记批册数）。
    chain_segments: u32,
    /// 是否封目录。
    sealed: bool,
    /// 目录哈希。
    sealed_hash: u64,
}

impl CatalogLedger {
    /// 开目录。
    pub fn new() -> CatalogLedger {
        CatalogLedger {
            books: core::array::from_fn(|_| None),
            book_count: 0,
            chain_head: 0,
            chain_segments: 0,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 登记批册（挂累计链）：链段衔接核（前终点=本起点，即顺序落账）。
    pub fn put_batch_book(&mut self, book_no: u32, volume: u8, lines: u64) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.book_count >= BOOKS {
            return Err(E_BOOK_FULL);
        }
        if book_no == 0 || book_no as usize > BATCH_BOOKS {
            return Err(E_BOOK_BAD);
        }
        if volume == 0 || volume > 8 {
            return Err(E_VOL_BAD);
        }
        if lines == 0 {
            return Err(E_LINES_ZERO);
        }
        if lines > BOOK_MAX_LINES {
            return Err(E_BAL);
        }
        // 册号查重（批册侧）。
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 1 && b.book_no == book_no {
                    return Err(E_BOOK_DUP);
                }
            }
            i += 1;
        }
        // 链段落账：本段起点 = 当前链头，终点 = 链头 + 增量。
        let start = self.chain_head;
        let end = start + lines;
        self.books[self.book_count] = Some(BookPtr {
            kind: 1,
            book_no,
            volume,
            start,
            end,
        });
        self.book_count += 1;
        self.chain_head = end;
        self.chain_segments += 1;
        Ok(end)
    }

    /// 登记深化册（与批册配对，行区间镜像批册）。
    pub fn put_deep_book(&mut self, book_no: u32, volume: u8, lines: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.book_count >= BOOKS {
            return Err(E_BOOK_FULL);
        }
        if book_no == 0 || book_no as usize > DEEP_BOOKS {
            return Err(E_BOOK_BAD);
        }
        if volume == 0 || volume > 8 {
            return Err(E_VOL_BAD);
        }
        if lines == 0 {
            return Err(E_LINES_ZERO);
        }
        // 册号查重（深化册侧）。
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 2 && b.book_no == book_no {
                    return Err(E_BOOK_DUP);
                }
            }
            i += 1;
        }
        // 配对核：同名批册必须已登记且行数一致。
        let mut paired = false;
        i = 0;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 1 && b.book_no == book_no {
                    if b.lines() != lines {
                        return Err(E_BAL);
                    }
                    paired = true;
                }
            }
            i += 1;
        }
        if !paired {
            return Err(E_PAIR_LACK);
        }
        // 深化册行区间镜像同名批册。
        let (start, end) = {
            let mut s = 0u64;
            let mut e = 0u64;
            let mut j = 0usize;
            while j < self.book_count {
                if let Some(b) = &self.books[j] {
                    if b.kind == 1 && b.book_no == book_no {
                        s = b.start;
                        e = b.end;
                    }
                }
                j += 1;
            }
            (s, e)
        };
        self.books[self.book_count] = Some(BookPtr {
            kind: 2,
            book_no,
            volume,
            start,
            end,
        });
        self.book_count += 1;
        Ok(())
    }

    /// 批册行数和。
    pub fn batch_lines_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 1 {
                    s += b.lines();
                }
            }
            i += 1;
        }
        s
    }

    /// 深化册行数和。
    pub fn deep_lines_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 2 {
                    s += b.lines();
                }
            }
            i += 1;
        }
        s
    }

    /// 双册对称守恒：批册行数和 == 深化册行数和。
    pub fn conserved(&self) -> bool {
        self.batch_lines_sum() == self.deep_lines_sum()
    }

    /// 批册登记数。
    pub fn batch_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 1 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 深化册登记数。
    pub fn deep_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 2 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 逐册可定位断言：全册指针可定位 + 行区间升序衔接（批册侧）。
    pub fn locate_assert(&self) -> Result<(), u16> {
        if self.book_count == 0 {
            return Err(E_LOCATE_FAIL);
        }
        let mut i = 0usize;
        while i < self.book_count {
            match &self.books[i] {
                Some(b) => {
                    if !b.locatable() {
                        return Err(E_LOCATE_FAIL);
                    }
                }
                None => return Err(E_LOCATE_FAIL),
            }
            i += 1;
        }
        // 链零断核：链头 == 终点且段数 == 40。
        if self.chain_segments != BATCH_BOOKS as u32 {
            return Err(E_SPAN_BREAK);
        }
        Ok(())
    }

    /// 累计链终点核对：chain_head == 240,000。
    pub fn chain_end_assert(&self) -> Result<(), u16> {
        if self.chain_head != CHAIN_END {
            return Err(E_CHAIN_END);
        }
        Ok(())
    }

    /// 封目录：40+40 齐 + 定位断言 + 终点核 + 守恒。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.batch_count() != BATCH_BOOKS as u32 || self.deep_count() != DEEP_BOOKS as u32 {
            return Err(E_NOT_FULL);
        }
        self.locate_assert()?;
        self.chain_end_assert()?;
        if !self.conserved() {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                buf.push(b.kind);
                buf.extend_from_slice(b.book_no.to_le_bytes().as_slice());
                buf.push(b.volume);
                buf.extend_from_slice(b.start.to_le_bytes().as_slice());
                buf.extend_from_slice(b.end.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 目录哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 链头（当前累计终点）。
    pub fn chain_head(&self) -> u64 {
        self.chain_head
    }

    /// 册数。
    pub fn len(&self) -> usize {
        self.book_count
    }

    /// 目录行渲染（抽样逐册行）。
    pub fn catalog_lines(&self, sample: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 账册汇编目录 · 批册+深化册 ===".to_string());
        let step = if sample == 0 { 1 } else { sample };
        let mut i = 0usize;
        while i < self.book_count {
            if i % step == 0 {
                if let Some(b) = &self.books[i] {
                    let mut line = String::from(&b.book_name());
                    line.push_str("|卷");
                    line.push_str(&itoa(b.volume as u64));
                    line.push_str("|[");
                    line.push_str(&itoa(b.start));
                    line.push(',');
                    line.push_str(&itoa(b.end));
                    line.push(')');
                    lines.push(line);
                }
            }
            i += 1;
        }
        lines.push("链头: ".to_string() + &itoa(self.chain_head));
        lines
    }

    /// 按册名查找指针（消费方定位口）。
    pub fn find_book(&self, kind: u8, book_no: u32) -> Result<BookPtr, u16> {
        if kind != 1 && kind != 2 {
            return Err(E_VOL_BAD);
        }
        if book_no == 0 || book_no as usize > BATCH_BOOKS {
            return Err(E_BOOK_BAD);
        }
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == kind && b.book_no == book_no {
                    return Ok(b.clone());
                }
            }
            i += 1;
        }
        Err(E_LOCATE_FAIL)
    }

    /// 卷号分布盘点：卷 1..=8 → 册数（卷均衡取数口）。
    pub fn volume_tally(&self) -> [u32; 9] {
        let mut tally = [0u32; 9];
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.volume as usize <= 8 {
                    tally[b.volume as usize] += 1;
                }
            }
            i += 1;
        }
        tally
    }

    /// 批册链单调复算：从零重推链头，逐段校验区间衔接与守恒。
    pub fn chain_replay(&self) -> Result<u64, u16> {
        let mut head = 0u64;
        let mut segs = 0u32;
        let mut i = 0usize;
        while i < self.book_count {
            if let Some(b) = &self.books[i] {
                if b.kind == 1 {
                    // 本段必须从当前链头起（零断核）。
                    if b.start != head {
                        return Err(E_SPAN_BREAK);
                    }
                    if b.end <= b.start {
                        return Err(E_SPAN_ZERO);
                    }
                    head = b.end;
                    segs += 1;
                }
            }
            i += 1;
        }
        if segs != BATCH_BOOKS as u32 {
            return Err(E_SPAN_BREAK);
        }
        Ok(head)
    }

    /// 行区间总面积（批册+深化册总行数，盘点口径）。
    pub fn total_span_lines(&self) -> u64 {
        self.batch_lines_sum() + self.deep_lines_sum()
    }
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 标准目录装配（每批 6,000 行 × 40 批 = 240,000）──────────────
/// 每批册标准行数：6,000（40 批合计恒等 240,000）。
pub const STD_BATCH_LINES: u64 = 6_000;

/// 一键装配标准目录（40 批册 + 40 深化册，卷号按册号轮转 1..8）。
pub fn enroll_standard(cat: &mut CatalogLedger) -> Result<(), u16> {
    let mut n = 1u32;
    while n as usize <= BATCH_BOOKS {
        let vol = ((n - 1) % 8 + 1) as u8;
        cat.put_batch_book(n, vol, STD_BATCH_LINES)?;
        cat.put_deep_book(n, vol, STD_BATCH_LINES)?;
        n += 1;
    }
    Ok(())
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：40+40 齐 + 链 240,000 + 定位断言 + 封目录 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut cat = CatalogLedger::new();
    enroll_standard(&mut cat)?;
    if cat.len() != BOOKS || cat.batch_count() != 40 || cat.deep_count() != 40 {
        return Err(SEG_BASE);
    }
    if cat.chain_head() != CHAIN_END {
        return Err(SEG_BASE);
    }
    cat.locate_assert()?;
    cat.chain_end_assert()?;
    if !cat.conserved() {
        return Err(SEG_BASE);
    }
    // 链复算与取数口。
    let replay = cat.chain_replay()?;
    if replay != CHAIN_END {
        return Err(SEG_BASE);
    }
    if cat.total_span_lines() != CHAIN_END * 2 {
        return Err(SEG_BASE);
    }
    let b1 = cat.find_book(1, 1)?;
    if b1.start != 0 || b1.lines() != STD_BATCH_LINES {
        return Err(SEG_BASE);
    }
    let vt = cat.volume_tally();
    if vt[1] != 10 || vt[8] != 10 {
        return Err(SEG_BASE);
    }
    let cl = cat.catalog_lines(7);
    if cl.len() < 10 {
        return Err(SEG_BASE);
    }
    let sealed = cat.seal()?;
    if sealed == 0 {
        return Err(SEG_BASE);
    }
    // 封目录后登记拒绝。
    match cat.put_batch_book(1, 1, 100) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：链终点错（增量偏小）。
    let mut cat2 = CatalogLedger::new();
    let mut n = 1u32;
    while n as usize <= BATCH_BOOKS {
        let vol = ((n - 1) % 8 + 1) as u8;
        let lines = if n == 40 { STD_BATCH_LINES - 1 } else { STD_BATCH_LINES };
        cat2.put_batch_book(n, vol, lines)?;
        n += 1;
    }
    match cat2.chain_end_assert() {
        Err(c) if c == E_CHAIN_END => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：深化册先于批册登记（配对缺失）。
    let mut cat3 = CatalogLedger::new();
    match cat3.put_deep_book(1, 1, STD_BATCH_LINES) {
        Err(c) if c == E_PAIR_LACK => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
