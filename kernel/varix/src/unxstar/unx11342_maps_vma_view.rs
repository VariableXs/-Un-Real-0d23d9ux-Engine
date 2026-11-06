//! UNX-F11342 maps 映射区间语义（VMA 呈现规则，0xeb60 段）
//!
//! `/proc/self/maps` 呈现的不是「调用了几次 mmap」，而是**当前 VMA 表经过
//! 合并后的视图**。内核在挂入新 VMA 时只做两件事：按地址排序、与相邻同质
//! VMA 合并。因此用户态看到的行数与 syscall 次数无关，而由下面三条呈现
//! 规则决定：
//!
//! 1. 区间左闭右开，`[start, end)`。地址恰好等于 `end` 已不属于本区间，
//!    这是 `find` 必须区分 `end-1` 与 `end` 的原因；
//! 2. 相邻（`a.end == b.start`）且权限相同、种类相同、文件偏移连续
//!    （`b.off == a.off + a.bytes()`）才合并；匿名区间偏移恒为 0，只要权限
//!    与种类一致就合并；
//! 3. 两行之间未被任何 VMA 覆盖的字节是**空洞**，呈现上不占行，但
//!    `munmap`/越界访问的判定必须先跨过它。空洞长度是区间语义的一部分。
//!
//! 闸门次序（插入时）：对齐 → 页数 → 种类 → 重叠 → 容量。对齐与页数是最
//! 廉价的参数错误，先报；种类是调用方传错枚举；重叠意味着调用方对现有
//! 布局的判断已经错了，优先于「表满」报出——表满只是资源问题，重叠是
//! 语义错误。
//!
//! 合并只在「呈现」时发生，底层 VMA 仍按插入粒度保存；这样 `row_start`、
//! `row_end` 能还原每一行的真实边界，而 `find` 仍能定位到最小粒度区间。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb60;

/// 起始地址未页对齐。
pub const VX_EALIGN: u32 = SEG_BASE + 0x01;
/// 页数为零或超过上限。
pub const VX_ELEN: u32 = SEG_BASE + 0x02;
/// 区间种类未知。
pub const VX_EKIND: u32 = SEG_BASE + 0x03;
/// 与既有区间重叠。
pub const VX_EOVERLAP: u32 = SEG_BASE + 0x04;
/// VMA 表已满。
pub const VX_EFULL: u32 = SEG_BASE + 0x05;
/// 地址未命中或行号越界。
pub const VX_EGAP: u32 = SEG_BASE + 0x06;
/// 行号与区间次序不一致。
pub const VX_EORDER: u32 = SEG_BASE + 0x07;
/// 账本失衡。
pub const VX_EBAL: u32 = SEG_BASE + 0x08;
/// 账本槽位乱序。
pub const VX_EBALORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const VX_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const VX_ESEAL: u32 = SEG_BASE + 0x0b;

/// 页大小。
pub const PAGE: u64 = 0x1000;
/// 单区间页数上限。
pub const MAX_PAGES: u32 = 64;
/// VMA 表容量。
pub const MAX_REGIONS: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 种类：匿名。
pub const KIND_ANON: u8 = 0;
/// 种类：文件映射。
pub const KIND_FILE: u8 = 1;
/// 种类：栈。
pub const KIND_STACK: u8 = 2;
/// 种类：堆。
pub const KIND_HEAP: u8 = 3;

/// 权限位：读。
pub const PERM_R: u8 = 1;
/// 权限位：写。
pub const PERM_W: u8 = 2;
/// 权限位：执行。
pub const PERM_X: u8 = 4;
/// 读写。
pub const PERM_RW: u8 = PERM_R | PERM_W;
/// 读执行。
pub const PERM_RX: u8 = PERM_R | PERM_X;

/// 操作码：插入。
pub const OP_INSERT: u8 = 1;
/// 操作码：查找。
pub const OP_FIND: u8 = 2;
/// 操作码：行数。
pub const OP_ROWS: u8 = 3;
/// 操作码：总页数。
pub const OP_PAGES: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一个最小粒度 VMA。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vma {
    /// 起始地址（含）。
    pub start: u64,
    /// 页数。
    pub pages: u32,
    /// 权限位。
    pub perms: u8,
    /// 种类。
    pub kind: u8,
    /// 文件偏移；匿名区间为 0。
    pub off: u64,
}

impl Vma {
    /// 空白 VMA。
    pub const fn blank() -> Vma {
        Vma { start: 0, pages: 0, perms: 0, kind: KIND_ANON, off: 0 }
    }

    /// 结束地址（不含）。
    pub fn end(&self) -> u64 {
        self.start + self.bytes()
    }

    /// 字节长度。
    pub fn bytes(&self) -> u64 {
        (self.pages as u64) * PAGE
    }

    /// 是否含该地址。
    pub fn contains(&self, addr: u64) -> bool {
        self.start <= addr && addr < self.end()
    }
}

/// 是否已知种类。
pub fn known_kind(kind: u8) -> bool {
    kind == KIND_ANON || kind == KIND_FILE || kind == KIND_STACK || kind == KIND_HEAP
}

/// VMA 表。
#[derive(Clone, Copy, Debug)]
pub struct Vmas {
    items: [Vma; MAX_REGIONS],
    len: usize,
}

impl Vmas {
    /// 空表。
    pub const fn new() -> Vmas {
        Vmas { items: [Vma::blank(); MAX_REGIONS], len: 0 }
    }

    /// 区间个数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 个区间。
    pub fn item(&self, i: usize) -> Option<Vma> {
        if i >= self.len {
            return None;
        }
        self.items.get(i).copied()
    }

    /// 第 i 个区间的起点。
    pub fn start_of(&self, i: usize) -> Option<u64> {
        self.item(i).map(|v| v.start)
    }

    /// 第 i 个区间的页数。
    pub fn pages_of(&self, i: usize) -> Option<u32> {
        self.item(i).map(|v| v.pages)
    }

    /// 第 i 个区间的偏移。
    pub fn offset_of(&self, i: usize) -> Option<u64> {
        self.item(i).map(|v| v.off)
    }

    /// 与 `[start, start+pages*PAGE)` 是否重叠。
    pub fn would_overlap(&self, start: u64, pages: u32) -> bool {
        let end = start + (pages as u64) * PAGE;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                if start < v.end() && v.start < end {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 插入一个区间。
    ///
    /// 返回插入的页数。闸门次序：对齐 → 页数 → 种类 → 重叠 → 容量。
    pub fn insert(
        &mut self,
        start: u64,
        pages: u32,
        perms: u8,
        kind: u8,
        off: u64,
    ) -> Result<u32, u32> {
        if start % PAGE != 0 {
            return Err(VX_EALIGN);
        }
        if pages == 0 || pages > MAX_PAGES {
            return Err(VX_ELEN);
        }
        if !known_kind(kind) {
            return Err(VX_EKIND);
        }
        if self.would_overlap(start, pages) {
            return Err(VX_EOVERLAP);
        }
        if self.len >= MAX_REGIONS {
            return Err(VX_EFULL);
        }
        let v = Vma { start, pages, perms, kind, off };

        // 找到第一个起点更大的槽位；表始终按起点升序。
        let mut pos = self.len;
        let mut i = 0usize;
        while i < self.len {
            match self.items.get(i) {
                Some(cur) => {
                    if start < cur.start {
                        pos = i;
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }

        // 从尾部右移，给 pos 腾位置。
        let mut j = self.len;
        while j > pos {
            if let Some(src) = self.items.get(j - 1).copied() {
                if let Some(dst) = self.items.get_mut(j) {
                    *dst = src;
                }
            }
            j -= 1;
        }
        if let Some(dst) = self.items.get_mut(pos) {
            *dst = v;
        }
        self.len += 1;
        Ok(pages)
    }

    /// 按地址查找最小粒度区间，返回下标。
    pub fn find(&self, addr: u64) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                if v.contains(addr) {
                    return Ok(i as u32);
                }
            }
            i += 1;
        }
        Err(VX_EGAP)
    }

    /// 总页数。
    pub fn total_pages(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.items.get(i) {
                n += v.pages;
            }
            i += 1;
        }
        n
    }

    /// 相邻两区间是否在呈现上合并。
    ///
    /// 条件是：首尾相接、权限相同、种类相同，且文件映射的偏移连续。
    pub fn mergeable(a: &Vma, b: &Vma) -> bool {
        if a.end() != b.start || a.perms != b.perms || a.kind != b.kind {
            return false;
        }
        if a.kind == KIND_FILE {
            b.off == a.off + a.bytes()
        } else {
            b.off == a.off
        }
    }

    /// 呈现行数。
    pub fn row_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let head = if i == 0 {
                true
            } else {
                match (self.items.get(i - 1), self.items.get(i)) {
                    (Some(a), Some(b)) => !Vmas::mergeable(a, b),
                    _ => true,
                }
            };
            if head {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 第 row 行的首个区间下标。
    pub fn row_head(&self, row: usize) -> Result<usize, u32> {
        if row >= self.row_count() {
            return Err(VX_EGAP);
        }
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let head = if i == 0 {
                true
            } else {
                match (self.items.get(i - 1), self.items.get(i)) {
                    (Some(a), Some(b)) => !Vmas::mergeable(a, b),
                    _ => true,
                }
            };
            if head {
                if seen == row {
                    return Ok(i);
                }
                seen += 1;
            }
            i += 1;
        }
        Err(VX_EGAP)
    }

    /// 第 row 行的起始地址。
    pub fn row_start(&self, row: usize) -> Result<u64, u32> {
        let head = self.row_head(row)?;
        match self.items.get(head) {
            Some(v) => Ok(v.start),
            None => Err(VX_EGAP),
        }
    }

    /// 第 row 行的结束地址（不含），即最后一个被合并区间的尾。
    pub fn row_end(&self, row: usize) -> Result<u64, u32> {
        let head = self.row_head(row)?;
        let mut end = match self.items.get(head) {
            Some(v) => v.end(),
            None => return Err(VX_EGAP),
        };
        let mut i = head;
        while i + 1 < self.len {
            match (self.items.get(i), self.items.get(i + 1)) {
                (Some(a), Some(b)) => {
                    if Vmas::mergeable(a, b) {
                        end = b.end();
                        i += 1;
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        Ok(end)
    }

    /// 第 row 行之后到下一行起点之间的空洞字节数。
    ///
    /// 最后一行之后没有「下一行」，按 0 计；两行紧邻时也为 0。
    pub fn gap_after(&self, row: usize) -> Result<u64, u32> {
        let end = self.row_end(row)?;
        if row + 1 >= self.row_count() {
            return Ok(0);
        }
        let next = self.row_start(row + 1)?;
        if next >= end {
            Ok(next - end)
        } else {
            Err(VX_EORDER)
        }
    }

    /// `[a, b)` 是否被区间完全覆盖。
    ///
    /// 返回 `Ok(1)` 覆盖、`Ok(0)` 不覆盖；`b <= a` 是退化查询，报 `VX_ELEN`。
    pub fn covers(&self, a: u64, b: u64) -> Result<u32, u32> {
        if b <= a {
            return Err(VX_ELEN);
        }
        let mut cur = a;
        let mut i = 0usize;
        while i < self.len {
            match self.items.get(i) {
                Some(v) => {
                    if v.start <= cur && cur < v.end() {
                        if v.end() >= b {
                            return Ok(1);
                        }
                        cur = v.end();
                    } else if v.start > cur {
                        return Ok(0);
                    }
                }
                None => break,
            }
            i += 1;
        }
        Ok(0)
    }

    /// 呈现行权限（取行首区间的权限）。
    pub fn row_perms(&self, row: usize) -> Result<u8, u32> {
        let head = self.row_head(row)?;
        match self.items.get(head) {
            Some(v) => Ok(v.perms),
            None => Err(VX_EGAP),
        }
    }
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

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
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

    /// 登记一格；槽位必须严格递增。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(VX_EBALORDER);
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

    /// 对账：8 成功格按序钉住取值，6 失败格按序钉住错误码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(VX_EBAL);
        }
        const VALS: [u32; 8] = [3, 2, 2, 1, 1, 4, 1, 9];
        const CODES: [u32; 6] = [
            VX_EALIGN,
            VX_ELEN,
            VX_EKIND,
            VX_EOVERLAP,
            VX_EFULL,
            VX_EGAP,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VX_EBAL),
            };
            if s.slot as usize != i {
                return Err(VX_EBALORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VX_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VX_EMISMATCH);
                        }
                    }
                    None => return Err(VX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VX_EMISMATCH);
                        }
                    }
                    None => return Err(VX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VX_EBAL);
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
            self.rejects += 1;
            return Err(VX_EBAL);
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
            Err(VX_ESEAL)
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

/// 十四格账本：五段布局 → 行数 → 归属 → 总页 → 六类闸门。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = Vmas::new();

    // 成功格：堆、栈、可执行文件段、偏移连续的数据段、偏移断裂的段。
    let _ = l.register(word_cell(0, OP_INSERT, m.insert(0x10000, 3, PERM_RW, KIND_HEAP, 0)));
    let _ = l.register(word_cell(1, OP_INSERT, m.insert(0x20000, 2, PERM_RW, KIND_STACK, 0)));
    let _ = l.register(word_cell(2, OP_INSERT, m.insert(0x30000, 2, PERM_RX, KIND_FILE, 0x1000)));
    let _ = l.register(word_cell(3, OP_INSERT, m.insert(0x32000, 1, PERM_RX, KIND_FILE, 0x3000)));
    let _ = l.register(word_cell(4, OP_INSERT, m.insert(0x33000, 1, PERM_RX, KIND_FILE, 0x9000)));
    let _ = l.register(cell(5, OP_ROWS, Ok(m.row_count())));
    let _ = l.register(word_cell(6, OP_FIND, m.find(0x21000)));
    let _ = l.register(word_cell(7, OP_PAGES, Ok(m.total_pages())));

    // 失败格：对齐、零页、未知种类、重叠、表满、未命中。
    let _ = l.register(word_cell(8, OP_INSERT, m.insert(0x10001, 1, PERM_RW, KIND_ANON, 0)));
    let _ = l.register(word_cell(9, OP_INSERT, m.insert(0x14000, 0, PERM_RW, KIND_ANON, 0)));
    let _ = l.register(word_cell(10, OP_INSERT, m.insert(0x14000, 1, PERM_RW, 9, 0)));
    let _ = l.register(word_cell(11, OP_INSERT, m.insert(0x11000, 1, PERM_RW, KIND_ANON, 0)));
    let _ = l.register(word_cell(12, OP_INSERT, {
        let _ = m.insert(0x40000, 1, PERM_RW, KIND_ANON, 0);
        m.insert(0x50000, 1, PERM_RW, KIND_ANON, 0)
    }));
    let _ = l.register(word_cell(13, OP_FIND, m.find(0x90000)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：合并边界。文本段与数据段偏移连续，应合成一行；
    // 断裂段另起一行，且两行之间没有空洞。
    let mut m = Vmas::new();
    let _ = m.insert(0x10000, 3, PERM_RW, KIND_HEAP, 0);
    let _ = m.insert(0x20000, 2, PERM_RW, KIND_STACK, 0);
    let _ = m.insert(0x30000, 2, PERM_RX, KIND_FILE, 0x1000);
    let _ = m.insert(0x32000, 1, PERM_RX, KIND_FILE, 0x3000);
    let _ = m.insert(0x33000, 1, PERM_RX, KIND_FILE, 0x9000);
    out[1] = m.row_count() == 4
        && m.row_start(0) == Ok(0x10000)
        && m.row_end(0) == Ok(0x13000)
        && m.row_start(1) == Ok(0x20000)
        && m.row_end(1) == Ok(0x22000)
        && m.row_start(2) == Ok(0x30000)
        && m.row_end(2) == Ok(0x33000)
        && m.row_start(3) == Ok(0x33000)
        && m.row_end(3) == Ok(0x34000)
        && m.gap_after(0) == Ok(0xd000)
        && m.gap_after(1) == Ok(0xe000)
        && m.gap_after(2) == Ok(0)
        && m.gap_after(3) == Ok(0)
        && m.row_perms(2) == Ok(PERM_RX)
        && m.row_perms(0) == Ok(PERM_RW);

    // 二路：左闭右开与覆盖判定。
    let mut n = Vmas::new();
    let _ = n.insert(0x10000, 3, PERM_RW, KIND_HEAP, 0);
    let _ = n.insert(0x20000, 2, PERM_RW, KIND_STACK, 0);
    out[2] = n.find(0x10000) == Ok(0)
        && n.find(0x12fff) == Ok(0)
        && n.find(0x13000) == Err(VX_EGAP)
        && n.find(0x21fff) == Ok(1)
        && n.find(0x22000) == Err(VX_EGAP)
        && n.covers(0x10000, 0x13000) == Ok(1)
        && n.covers(0x12fff, 0x13000) == Ok(1)
        && n.covers(0x12000, 0x21000) == Ok(0)
        && n.covers(0x20000, 0x20000) == Err(VX_ELEN)
        && n.covers(0x20001, 0x20000) == Err(VX_ELEN);

    // 三路：排序插入与闸门优先级。
    let mut p = Vmas::new();
    let early_hi = p.insert(0x30000, 1, PERM_RW, KIND_ANON, 0);
    let late_lo = p.insert(0x10000, 1, PERM_RW, KIND_ANON, 0);
    let sorted = p.start_of(0) == Some(0x10000)
        && p.start_of(1) == Some(0x30000)
        && p.find(0x10000) == Ok(0)
        && p.find(0x30000) == Ok(1);

    let mut q = Vmas::new();
    let align_first = q.insert(0x1001, 0, PERM_RW, 9, 0) == Err(VX_EALIGN);
    let len_before_kind = q.insert(0x1000, 0, PERM_RW, 9, 0) == Err(VX_ELEN);
    let kind_before_overlap = q.insert(0x1000, 1, PERM_RW, 9, 0) == Err(VX_EKIND);
    let huge = q.insert(0x1000, MAX_PAGES + 1, PERM_RW, KIND_ANON, 0) == Err(VX_ELEN);

    let mut full = Vmas::new();
    let mut k = 0u64;
    while k < MAX_REGIONS as u64 {
        let _ = full.insert(0x10000 + k * 0x1000, 1, PERM_RW, KIND_ANON, 0);
        k += 1;
    }
    let overlap_before_full =
        full.insert(0x12000, 1, PERM_RW, KIND_ANON, 0) == Err(VX_EOVERLAP);
    let full_after = full.insert(0x20000, 1, PERM_RW, KIND_ANON, 0) == Err(VX_EFULL);
    out[3] = early_hi == Ok(1)
        && late_lo == Ok(1)
        && sorted
        && align_first
        && len_before_kind
        && kind_before_overlap
        && huge
        && overlap_before_full
        && full_after
        && full.len() == MAX_REGIONS;

    out
}

/// 十三项判据。
pub fn run_unx11342_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11342.maps_vma_view");
    let l = build_ledger();

    s.add_dyn(
        "VX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "VX2-堆区插入页数",
        l.sample(0).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "VX3-栈区插入页数",
        l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "VX4-偏移连续即合并",
        {
            let mut m = Vmas::new();
            let _ = m.insert(0x30000, 2, PERM_RX, KIND_FILE, 0x1000);
            let _ = m.insert(0x32000, 1, PERM_RX, KIND_FILE, 0x3000);
            l.sample(3).map(|x| x.value) == Some(1)
                && m.row_count() == 1
                && m.row_start(0) == Ok(0x30000)
                && m.row_end(0) == Ok(0x33000)
        },
        "static",
    );

    s.add(
        "VX5-偏移断裂即断行",
        {
            let mut m = Vmas::new();
            let _ = m.insert(0x30000, 2, PERM_RX, KIND_FILE, 0x1000);
            let _ = m.insert(0x32000, 1, PERM_RX, KIND_FILE, 0x3000);
            let _ = m.insert(0x33000, 1, PERM_RX, KIND_FILE, 0x9000);
            l.sample(4).map(|x| x.value) == Some(1)
                && m.row_count() == 2
                && m.row_start(1) == Ok(0x33000)
                && m.offset_of(2) == Some(0x9000)
        },
        "static",
    );

    s.add(
        "VX6-呈现行数",
        l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "VX7-地址归属",
        l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "VX8-总页数",
        l.sample(7).map(|x| x.value) == Some(9),
        "static",
    );

    s.add(
        "VX9-对齐闸门",
        l.sample(8).map(|x| x.code) == Some(VX_EALIGN),
        "static",
    );

    s.add(
        "VX10-零页与未知种类",
        l.sample(9).map(|x| x.code) == Some(VX_ELEN)
            && l.sample(10).map(|x| x.code) == Some(VX_EKIND),
        "static",
    );

    s.add(
        "VX11-重叠与表满",
        l.sample(11).map(|x| x.code) == Some(VX_EOVERLAP)
            && l.sample(12).map(|x| x.code) == Some(VX_EFULL),
        "static",
    );

    s.add(
        "VX12-未命中地址",
        l.sample(13).map(|x| x.code) == Some(VX_EGAP),
        "static",
    );

    s.add(
        "VX13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn five() -> Vmas {
        let mut m = Vmas::new();
        let _ = m.insert(0x10000, 3, PERM_RW, KIND_HEAP, 0);
        let _ = m.insert(0x20000, 2, PERM_RW, KIND_STACK, 0);
        let _ = m.insert(0x30000, 2, PERM_RX, KIND_FILE, 0x1000);
        let _ = m.insert(0x32000, 1, PERM_RX, KIND_FILE, 0x3000);
        let _ = m.insert(0x33000, 1, PERM_RX, KIND_FILE, 0x9000);
        m
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11342_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn rows_merge_by_offset() {
        let m = five();
        assert_eq!(m.row_count(), 4);
        assert_eq!(m.row_start(2), Ok(0x30000));
        assert_eq!(m.row_end(2), Ok(0x33000));
        assert_eq!(m.row_start(3), Ok(0x33000));
        assert_eq!(m.gap_after(2), Ok(0));
        assert_eq!(m.gap_after(0), Ok(0xd000));
    }

    #[test]
    fn end_is_exclusive() {
        let m = five();
        assert_eq!(m.find(0x12fff), Ok(0));
        assert_eq!(m.find(0x13000), Err(VX_EGAP));
        assert_eq!(m.find(0x22000), Err(VX_EGAP));
        assert_eq!(m.covers(0x1ffff, 0x20000), Ok(0));
    }

    #[test]
    fn insertion_keeps_order() {
        let mut m = Vmas::new();
        assert_eq!(m.insert(0x50000, 1, PERM_RW, KIND_ANON, 0), Ok(1));
        assert_eq!(m.insert(0x10000, 2, PERM_RW, KIND_ANON, 0), Ok(2));
        assert_eq!(m.insert(0x30000, 1, PERM_RW, KIND_ANON, 0), Ok(1));
        assert_eq!(m.start_of(0), Some(0x10000));
        assert_eq!(m.start_of(1), Some(0x30000));
        assert_eq!(m.start_of(2), Some(0x50000));
        assert_eq!(m.find(0x30fff), Ok(1));
    }

    #[test]
    fn gate_priority() {
        let mut m = Vmas::new();
        assert_eq!(m.insert(0x1001, 0, PERM_RW, 9, 0), Err(VX_EALIGN));
        assert_eq!(m.insert(0x1000, 0, PERM_RW, 9, 0), Err(VX_ELEN));
        assert_eq!(m.insert(0x1000, MAX_PAGES + 1, PERM_RW, 9, 0), Err(VX_ELEN));
        assert_eq!(m.insert(0x1000, 1, PERM_RW, 9, 0), Err(VX_EKIND));
        assert_eq!(m.insert(0x1000, 1, PERM_RW, KIND_ANON, 0), Ok(1));
        assert_eq!(m.insert(0x1000, 1, PERM_RW, KIND_ANON, 0), Err(VX_EOVERLAP));
    }

    #[test]
    fn table_full_after_overlap() {
        let mut m = Vmas::new();
        let mut i = 0u64;
        while i < MAX_REGIONS as u64 {
            assert_eq!(m.insert(0x10000 + i * 0x1000, 1, PERM_RW, KIND_ANON, 0), Ok(1));
            i += 1;
        }
        assert_eq!(m.insert(0x10000, 1, PERM_RW, KIND_ANON, 0), Err(VX_EOVERLAP));
        assert_eq!(m.insert(0x20000, 1, PERM_RW, KIND_ANON, 0), Err(VX_EFULL));
        assert_eq!(m.len(), MAX_REGIONS);
    }

    #[test]
    fn coverage_and_gaps() {
        let m = five();
        assert_eq!(m.covers(0x30000, 0x34000), Ok(1));
        assert_eq!(m.covers(0x30000, 0x35000), Ok(0));
        assert_eq!(m.covers(0x13000, 0x20000), Ok(0));
        assert_eq!(m.gap_after(3), Ok(0));
        assert_eq!(m.gap_after(4), Err(VX_EGAP));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
