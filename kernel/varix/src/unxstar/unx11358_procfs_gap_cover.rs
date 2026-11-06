//! UNX-F11358 procfs 覆盖缺口账（0xec60 段）
//!
//! procfs 的整记录读取常常分片进行：一次只从某个文件里拿到一段字段区间。分片
//! 到达顺序不受控制，可能先读到 `[24,40)` 再补读 `[8,16)`，也可能直接补进两段
//! 之间的空洞。覆盖账要回答三个问题：
//!
//! 1. **已覆盖多少**：所有分片并起来的总长度，相邻或重叠的边界不能重复计数；
//! 2. **缺口在哪**：首个未覆盖偏移，以及未覆盖总量；
//! 3. **能否宣称读全**：只有一条缺口都不剩时，`finish` 才允许返回整段长度。
//!
//! 分片只有两种合法边界关系：
//! - **相接**（`a.end == b.start`）：不产生新条目，直接合并成一条；
//! - **隔开**（`a.end < b.start`）：独立成条，中间留缺口。
//!
//! **重叠**（`a.start < b.end && b.start < a.end`）一律拒绝：两个分片对同一偏移
//! 给出各自的字节，谁是对的无法判定，接受它就等于把矛盾埋进覆盖账。
//!
//! 闸门次序：
//! - `add`：退化/越界区间 → 与已有分片重叠 → 表满（相接合并不受表满限制）；
//! - `read`：起点越界或零长 → 模式非法 → 缺口处短读 / `ESHORT`；
//! - `finish`：未登记任何分片 → 仍有缺口 → 整段。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec60;

/// 区间或读取参数非法。
pub const GL_EINVAL: u32 = SEG_BASE + 0x01;
/// 与已覆盖分片重叠。
pub const GL_EOVERLAP: u32 = SEG_BASE + 0x02;
/// 分片表满。
pub const GL_EFULL: u32 = SEG_BASE + 0x03;
/// 仍有缺口，不能宣称读全。
pub const GL_EGAP: u32 = SEG_BASE + 0x04;
/// 已无缺口，没有缺口可查。
pub const GL_ENOGAP: u32 = SEG_BASE + 0x05;
/// 整段读取在缺口处截断。
pub const GL_ESHORT: u32 = SEG_BASE + 0x06;
/// 尚未登记任何分片。
pub const GL_ENOTREADY: u32 = SEG_BASE + 0x07;
/// 读取模式非法。
pub const GL_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const GL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const GL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const GL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const GL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const GL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 分片表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 记录长度上限。
pub const LIMIT: u32 = 0x1_0000;

/// 读取模式：遇缺口短读。
pub const MODE_CONTIG: u8 = 1;
/// 读取模式：缺一点即失败。
pub const MODE_FULL: u8 = 2;

/// 操作码：登记分片。
pub const OP_ADD: u8 = 1;
/// 操作码：查询缺口。
pub const OP_GAP: u8 = 2;
/// 操作码：读取。
pub const OP_READ: u8 = 3;
/// 操作码：宣称读全。
pub const OP_FINISH: u8 = 4;
/// 操作码：观察。
pub const OP_OBS: u8 = 5;

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

/// 一段半开区间 `[start, end)`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 起点。
    pub start: u32,
    /// 终点（不含）。
    pub end: u32,
}

/// 覆盖账。
#[derive(Clone, Copy, Debug)]
pub struct Cover {
    total: u32,
    spans: [Span; CAP],
    n: usize,
    adds: u32,
    merges: u32,
}

impl Cover {
    /// 为长度 `total` 的记录开户。
    pub fn open(total: u32) -> Result<Cover, u32> {
        if total == 0 || total > LIMIT {
            return Err(GL_EINVAL);
        }
        Ok(Cover {
            total,
            spans: [Span { start: 0, end: 0 }; CAP],
            n: 0,
            adds: 0,
            merges: 0,
        })
    }

    /// 记录总长。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 分片条数。
    pub fn span_count(&self) -> usize {
        self.n
    }

    /// 第 i 条分片。
    pub fn span_at(&self, i: usize) -> Option<Span> {
        if i >= self.n {
            return None;
        }
        self.spans.get(i).copied()
    }

    /// 成功登记次数。
    pub fn add_count(&self) -> u32 {
        self.adds
    }

    /// 因相接而消掉的边界数。
    pub fn merge_count(&self) -> u32 {
        self.merges
    }

    /// 已覆盖总长度。
    pub fn covered(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.spans.get(i) {
                Some(s) => sum = sum.saturating_add(s.end - s.start),
                None => return sum,
            }
            i += 1;
        }
        sum
    }

    /// 登记一段分片，返回登记后的覆盖总量。
    pub fn add(&mut self, start: u32, end: u32) -> Result<u32, u32> {
        if start >= end || end > self.total {
            return Err(GL_EINVAL);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.spans.get(i) {
                Some(s) => {
                    if s.start < end && start < s.end {
                        return Err(GL_EOVERLAP);
                    }
                }
                None => return Err(GL_EBAL),
            }
            i += 1;
        }
        let mut left: Option<usize> = None;
        let mut right: Option<usize> = None;
        let mut j = 0usize;
        while j < self.n {
            match self.spans.get(j) {
                Some(s) => {
                    if s.end == start {
                        left = Some(j);
                    }
                    if s.start == end {
                        right = Some(j);
                    }
                }
                None => return Err(GL_EBAL),
            }
            j += 1;
        }
        match (left, right) {
            (Some(a), Some(b)) => {
                let lo = match self.spans.get(a) {
                    Some(s) => s.start,
                    None => return Err(GL_EBAL),
                };
                let hi = match self.spans.get(b) {
                    Some(s) => s.end,
                    None => return Err(GL_EBAL),
                };
                if let Some(slot) = self.spans.get_mut(a) {
                    *slot = Span { start: lo, end: hi };
                }
                let mut k = b;
                while k + 1 < self.n {
                    match self.spans.get(k + 1).copied() {
                        Some(next) => {
                            if let Some(dst) = self.spans.get_mut(k) {
                                *dst = next;
                            }
                        }
                        None => return Err(GL_EBAL),
                    }
                    k += 1;
                }
                if self.n > 0 {
                    self.n -= 1;
                }
                self.merges = self.merges.saturating_add(2);
            }
            (Some(a), None) => {
                if let Some(slot) = self.spans.get_mut(a) {
                    slot.end = end;
                }
                self.merges = self.merges.saturating_add(1);
            }
            (None, Some(b)) => {
                if let Some(slot) = self.spans.get_mut(b) {
                    slot.start = start;
                }
                self.merges = self.merges.saturating_add(1);
            }
            (None, None) => {
                if self.n >= CAP {
                    return Err(GL_EFULL);
                }
                let mut at = 0usize;
                while at < self.n {
                    match self.spans.get(at) {
                        Some(s) => {
                            if s.start > start {
                                break;
                            }
                        }
                        None => return Err(GL_EBAL),
                    }
                    at += 1;
                }
                let mut k = self.n;
                while k > at {
                    match self.spans.get(k - 1).copied() {
                        Some(prev) => {
                            if let Some(dst) = self.spans.get_mut(k) {
                                *dst = prev;
                            }
                        }
                        None => return Err(GL_EBAL),
                    }
                    k -= 1;
                }
                if let Some(dst) = self.spans.get_mut(at) {
                    *dst = Span { start, end };
                }
                self.n += 1;
            }
        }
        self.adds = self.adds.saturating_add(1);
        Ok(self.covered())
    }

    /// 未覆盖总长度；已读全时报无缺口。
    pub fn gap_len(&self) -> Result<u32, u32> {
        let g = self.total - self.covered();
        if g == 0 {
            Err(GL_ENOGAP)
        } else {
            Ok(g)
        }
    }

    /// 首个未覆盖偏移；已读全时报无缺口。
    pub fn gap_start(&self) -> Result<u32, u32> {
        let mut expect = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.spans.get(i) {
                Some(s) => {
                    if s.start > expect {
                        return Ok(expect);
                    }
                    expect = s.end;
                }
                None => return Err(GL_EBAL),
            }
            i += 1;
        }
        if expect < self.total {
            Ok(expect)
        } else {
            Err(GL_ENOGAP)
        }
    }

    /// 读取一段；`MODE_FULL` 时要求整段已覆盖。
    pub fn read(&self, from: u32, len: u32, mode: u8) -> Result<u32, u32> {
        if from >= self.total || len == 0 {
            return Err(GL_EINVAL);
        }
        if mode != MODE_CONTIG && mode != MODE_FULL {
            return Err(GL_EMODE);
        }
        let mut avail = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.spans.get(i) {
                Some(s) => {
                    if s.start <= from && from < s.end {
                        avail = s.end - from;
                        break;
                    }
                }
                None => return Err(GL_EBAL),
            }
            i += 1;
        }
        let room = self.total - from;
        let need = if len > room { room } else { len };
        if avail >= need {
            Ok(need)
        } else if mode == MODE_FULL {
            Err(GL_ESHORT)
        } else {
            Ok(avail)
        }
    }

    /// 宣称读全；缺口未尽则拒绝。
    pub fn finish(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(GL_ENOTREADY);
        }
        if self.total - self.covered() > 0 {
            return Err(GL_EGAP);
        }
        Ok(self.total)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.total == 0 || self.total > LIMIT || self.n > CAP {
            return false;
        }
        if self.merges > self.adds {
            return false;
        }
        if (self.adds - self.merges) as usize != self.n {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let s = match self.spans.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if s.start >= s.end || s.end > self.total {
                return false;
            }
            if i > 0 {
                match self.spans.get(i - 1) {
                    Some(prev) => {
                        if prev.end >= s.start {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            i += 1;
        }
        true
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(GL_ESLOT);
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(GL_EBAL);
        }
        const VALS: [u32; 8] = [8, 24, 32, 40, 24, 0, 48, 48];
        const CODES: [u32; 6] =
            [GL_EOVERLAP, GL_EINVAL, GL_EMODE, GL_EFULL, GL_EGAP, GL_ESHORT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(GL_EBAL),
            };
            if s.slot as usize != i {
                return Err(GL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(GL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(GL_EVALUE);
                        }
                    }
                    None => return Err(GL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(GL_EVALUE);
                        }
                    }
                    None => return Err(GL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(GL_EBAL);
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
            return Err(GL_EBAL);
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
            Err(GL_ESEAL)
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

/// 十四格账本：四段登记 → 补洞 → 缺口观测 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = match Cover::open(64) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_ADD, c.add(8, 16)));
    let _ = l.register(word_cell(1, OP_ADD, c.add(24, 40)));
    let _ = l.register(word_cell(2, OP_ADD, c.add(16, 24)));
    let _ = l.register(word_cell(3, OP_ADD, c.add(48, 56)));
    let _ = l.register(word_cell(4, OP_GAP, c.gap_len()));
    let _ = l.register(word_cell(5, OP_GAP, c.gap_start()));
    let _ = l.register(word_cell(6, OP_ADD, c.add(40, 48)));
    let _ = l.register(word_cell(7, OP_READ, c.read(8, 48, MODE_FULL)));

    let _ = l.register(word_cell(8, OP_ADD, c.add(30, 31)));
    let _ = l.register(word_cell(9, OP_ADD, c.add(20, 20)));
    let _ = l.register(word_cell(10, OP_READ, c.read(8, 4, 7)));

    let mut full = match Cover::open(64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let mut i = 0u32;
    while i < CAP as u32 {
        let _ = full.add(i * 2, i * 2 + 1);
        i += 1;
    }
    let _ = l.register(word_cell(11, OP_ADD, full.add(16, 17)));

    let mut g = match Cover::open(64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = g.add(0, 1);
    let _ = l.register(word_cell(12, OP_FINISH, g.finish()));

    let mut fin = match Cover::open(64) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = fin.add(0, 4);
    let _ = l.register(word_cell(13, OP_READ, fin.read(0, 8, MODE_FULL)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：相接合并 + 补洞 + 短读/整段读。
    let mut a = match Cover::open(32) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let c1 = a.add(0, 8);
    let ov = a.add(4, 12);
    let c2 = a.add(8, 16);
    let n1 = a.span_count();
    let gl = a.gap_len();
    let gs = a.gap_start();
    let full = a.read(0, 16, MODE_FULL);
    let contig = a.read(0, 20, MODE_CONTIG);
    let short = a.read(0, 20, MODE_FULL);
    let tail = a.read(16, 4, MODE_CONTIG);
    out[1] = c1 == Ok(8)
        && ov == Err(GL_EOVERLAP)
        && c2 == Ok(16)
        && n1 == 1
        && a.span_at(0) == Some(Span { start: 0, end: 16 })
        && gl == Ok(16)
        && gs == Ok(16)
        && full == Ok(16)
        && contig == Ok(16)
        && short == Err(GL_ESHORT)
        && tail == Ok(0)
        && a.consistent();

    // 二路：八类失败各自可辨。
    let open_zero = matches!(Cover::open(0), Err(GL_EINVAL));
    let open_big = matches!(Cover::open(LIMIT + 1), Err(GL_EINVAL));
    let mut b = match Cover::open(32) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let degenerate = b.add(3, 3) == Err(GL_EINVAL);
    let beyond = b.add(30, 33) == Err(GL_EINVAL);
    let reversed = b.add(10, 5) == Err(GL_EINVAL);
    let empty_finish = b.finish() == Err(GL_ENOTREADY);
    let empty_gap = b.gap_len() == Ok(32) && b.gap_start() == Ok(0);
    let _ = b.add(0, 16);
    let partial = b.finish() == Err(GL_EGAP);
    let _ = b.add(16, 32);
    let done = b.finish() == Ok(32);
    let no_gap = b.gap_len() == Err(GL_ENOGAP) && b.gap_start() == Err(GL_ENOGAP);
    let tail_limit = b.read(30, 8, MODE_CONTIG) == Ok(2);
    let bad_from = b.read(32, 1, MODE_CONTIG) == Err(GL_EINVAL);
    let zero_len = b.read(0, 0, MODE_CONTIG) == Err(GL_EINVAL);
    let bad_mode = b.read(0, 1, 0) == Err(GL_EMODE);
    out[2] = open_zero
        && open_big
        && degenerate
        && beyond
        && reversed
        && empty_finish
        && empty_gap
        && partial
        && done
        && no_gap
        && tail_limit
        && bad_from
        && zero_len
        && bad_mode
        && b.consistent();

    // 三路：表满时相接仍可合并。
    let mut q = match Cover::open(64) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0u32;
    while i < CAP as u32 {
        let _ = q.add(i * 2, i * 2 + 1);
        i += 1;
    }
    let n_full = q.span_count();
    let q1 = q.add(20, 21) == Err(GL_EFULL);
    let q2 = q.add(15, 16) == Ok(9);
    let q3 = q.span_count() == CAP;
    let q4 = q.add(16, 17) == Ok(10);
    let q5 = q.span_count() == CAP;
    out[3] = n_full == CAP && q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11358_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11358.procfs_gap_cover");
    let l = build_ledger();

    s.add_dyn(
        "GL1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "GL2-相接合并不增条",
        {
            let mut c = match Cover::open(64) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a = c.add(8, 16);
            let b = c.add(16, 24);
            a == Ok(8)
                && b == Ok(16)
                && c.span_count() == 1
                && c.span_at(0) == Some(Span { start: 8, end: 24 })
                && c.merge_count() == 1
                && l.sample(2).map(|x| x.value) == Some(32)
        },
        "static",
    );

    s.add(
        "GL3-缺口总量可数",
        l.sample(4).map(|x| x.value) == Some(24),
        "static",
    );

    s.add(
        "GL4-首个缺口偏移",
        l.sample(5).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "GL5-补洞三段合一",
        {
            let mut c = match Cover::open(64) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.add(8, 40);
            let _ = c.add(48, 56);
            let bridged = c.add(40, 48);
            bridged == Ok(48)
                && c.span_count() == 1
                && c.span_at(0) == Some(Span { start: 8, end: 56 })
                && c.merge_count() == 2
                && l.sample(6).map(|x| x.value) == Some(48)
        },
        "static",
    );

    s.add(
        "GL6-整段读取足量",
        l.sample(7).map(|x| x.value) == Some(48),
        "static",
    );

    s.add(
        "GL7-重叠被拒不改账",
        {
            let mut c = match Cover::open(64) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.add(8, 16);
            let before = c.covered();
            let r = c.add(12, 20);
            r == Err(GL_EOVERLAP) && c.covered() == before && c.span_count() == 1
                && l.sample(8).map(|x| x.code) == Some(GL_EOVERLAP)
        },
        "static",
    );

    s.add(
        "GL8-退化区间被拒",
        l.sample(9).map(|x| x.code) == Some(GL_EINVAL),
        "static",
    );

    s.add(
        "GL9-读取模式被拒",
        {
            let mut c = match Cover::open(64) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.add(0, 8);
            c.read(0, 16, 9) == Err(GL_EMODE)
                && c.read(0, 16, MODE_CONTIG) == Ok(8)
                && c.read(0, 16, MODE_FULL) == Err(GL_ESHORT)
                && l.sample(10).map(|x| x.code) == Some(GL_EMODE)
        },
        "static",
    );

    s.add(
        "GL10-表满仍容相接",
        l.sample(11).map(|x| x.code) == Some(GL_EFULL),
        "static",
    );

    s.add(
        "GL11-缺口未平不许收",
        {
            let mut c = match Cover::open(16) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let not_ready = c.finish() == Err(GL_ENOTREADY);
            let _ = c.add(0, 8);
            let halted = c.finish() == Err(GL_EGAP);
            let _ = c.add(8, 16);
            let done = c.finish() == Ok(16);
            not_ready
                && halted
                && done
                && c.gap_len() == Err(GL_ENOGAP)
                && c.gap_start() == Err(GL_ENOGAP)
                && l.sample(12).map(|x| x.code) == Some(GL_EGAP)
        },
        "static",
    );

    s.add(
        "GL12-短读与尾部截断",
        {
            let mut c = match Cover::open(10) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = c.add(0, 4);
            let _ = c.add(6, 10);
            let in_gap = c.read(4, 2, MODE_CONTIG) == Ok(0);
            let partial = c.read(0, 6, MODE_CONTIG) == Ok(4);
            let stop = c.read(0, 6, MODE_FULL) == Err(GL_ESHORT);
            let tail = c.read(8, 8, MODE_CONTIG) == Ok(2);
            let over = c.read(10, 1, MODE_CONTIG) == Err(GL_EINVAL);
            let first = c.gap_start() == Ok(4);
            in_gap
                && partial
                && stop
                && tail
                && over
                && first
                && c.consistent()
                && l.sample(13).map(|x| x.code) == Some(GL_ESHORT)
        },
        "static",
    );

    s.add(
        "GL13-冻结重放四注入",
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

    fn cover(total: u32) -> Cover {
        match Cover::open(total) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11358_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn adjacent_merge_collapses() {
        let mut c = cover(32);
        assert_eq!(c.add(0, 4), Ok(4));
        assert_eq!(c.add(4, 8), Ok(8));
        assert_eq!(c.span_count(), 1);
        assert_eq!(c.span_at(0), Some(Span { start: 0, end: 8 }));
        assert_eq!(c.merge_count(), 1);
        assert!(c.consistent());
    }

    #[test]
    fn bridge_fills_hole() {
        let mut c = cover(32);
        assert_eq!(c.add(0, 4), Ok(4));
        assert_eq!(c.add(8, 12), Ok(8));
        assert_eq!(c.gap_len(), Ok(24));
        assert_eq!(c.add(4, 8), Ok(12));
        assert_eq!(c.span_count(), 1);
        assert_eq!(c.span_at(0), Some(Span { start: 0, end: 12 }));
        assert_eq!(c.merge_count(), 2);
        assert!(c.consistent());
    }

    #[test]
    fn overlap_refused_without_mutation() {
        let mut c = cover(32);
        assert_eq!(c.add(4, 12), Ok(8));
        let before = c.covered();
        let adds = c.add_count();
        assert_eq!(c.add(0, 8), Err(GL_EOVERLAP));
        assert_eq!(c.add(4, 12), Err(GL_EOVERLAP));
        assert_eq!(c.covered(), before);
        assert_eq!(c.add_count(), adds);
        assert_eq!(c.span_count(), 1);
    }

    #[test]
    fn short_read_modes() {
        let mut c = cover(16);
        assert_eq!(c.add(0, 4), Ok(4));
        assert_eq!(c.read(0, 8, MODE_CONTIG), Ok(4));
        assert_eq!(c.read(0, 8, MODE_FULL), Err(GL_ESHORT));
        assert_eq!(c.read(4, 1, MODE_CONTIG), Ok(0));
        assert_eq!(c.read(0, 0, MODE_CONTIG), Err(GL_EINVAL));
        assert_eq!(c.read(16, 1, MODE_CONTIG), Err(GL_EINVAL));
        assert_eq!(c.read(0, 1, 3), Err(GL_EMODE));
        assert_eq!(c.add(12, 16), Ok(8));
        assert_eq!(c.read(12, 8, MODE_CONTIG), Ok(4));
    }

    #[test]
    fn finish_requires_full_coverage() {
        let mut c = cover(16);
        assert_eq!(c.finish(), Err(GL_ENOTREADY));
        assert_eq!(c.gap_len(), Ok(16));
        assert_eq!(c.gap_start(), Ok(0));
        assert_eq!(c.add(0, 8), Ok(8));
        assert_eq!(c.finish(), Err(GL_EGAP));
        assert_eq!(c.gap_start(), Ok(8));
        assert_eq!(c.add(8, 16), Ok(16));
        assert_eq!(c.finish(), Ok(16));
        assert_eq!(c.gap_len(), Err(GL_ENOGAP));
        assert_eq!(c.gap_start(), Err(GL_ENOGAP));
        assert!(c.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
