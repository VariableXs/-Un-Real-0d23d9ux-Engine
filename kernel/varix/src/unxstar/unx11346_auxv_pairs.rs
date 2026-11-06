//! UNX-F11346 /proc/self/auxv 呈现（0xeba0 段）
//!
//! `/proc/self/auxv` 把内核在 exec 时放在用户栈上的辅助向量原样暴露出来：
//! 一串 `(a_type, a_val)` 对，每对在 64 位上占 **16 字节**（两个小端 u64），
//! 最后以 `AT_NULL`（类型 0、值 0）收尾。它有三层真值：
//!
//! 1. **布局真值**：记录宽度恒 16，类型在前、值在后，均按本机字节序
//!    （x86-64/varix 为小端）。消费者若按 8 字节读或按 RGB 式交错读，
//!    会把类型和值拼错；
//! 2. **终止真值**：`AT_NULL` 不是普通条目，而是终止符。内核在它之后不再
//!    写任何内容，因此「有效对数」不含终止项，但总字节数必须**含**它；
//! 3. **类型真值**：少数类型的值有硬约束。`AT_PAGESZ` 必须等于页大小，
//!    `AT_ENTRY` 必须非零，指针类（`AT_BASE`/`AT_ENTRY`/`AT_RANDOM`/
//!    `AT_EXECFN`）必须 8 字节对齐。违背这些约束的向量不是内核产物，
//!    构造阶段就该拒绝。
//!
//! 闸门次序（构造）：对数 → 显式 AT_NULL → 重复类型 → 指针对齐 →
//! 页大小真值 → 入口非零。先做结构性检查，再做类型语义检查；页大小是
//! 全系统常量，比入口值更容易判定，所以排在入口之前。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeba0;

/// 输入中出现显式 AT_NULL。
pub const AU_ENULL: u32 = SEG_BASE + 0x01;
/// 重复类型。
pub const AU_EDUP: u32 = SEG_BASE + 0x02;
/// 对数超过上限。
pub const AU_EFULL: u32 = SEG_BASE + 0x03;
/// 指针类值未对齐。
pub const AU_EALIGN: u32 = SEG_BASE + 0x04;
/// 页大小真值不符。
pub const AU_EPAGESZ: u32 = SEG_BASE + 0x05;
/// 入口地址为零。
pub const AU_ENTRY: u32 = SEG_BASE + 0x06;
/// 类型未找到。
pub const AU_ENOENT: u32 = SEG_BASE + 0x07;
/// 读取偏移越过末尾。
pub const AU_EOFF: u32 = SEG_BASE + 0x08;
/// 条目下标越界。
pub const AU_EINDEX: u32 = SEG_BASE + 0x09;
/// 字节下标越过末尾。
pub const AU_ENOBS: u32 = SEG_BASE + 0x0a;
/// 账本失衡。
pub const AU_EBAL: u32 = SEG_BASE + 0x0b;
/// 账本槽位乱序。
pub const AU_EORDER: u32 = SEG_BASE + 0x0c;
/// 观测值与复算不符。
pub const AU_EMISMATCH: u32 = SEG_BASE + 0x0d;
/// 冻结后重放不一致。
pub const AU_ESEAL: u32 = SEG_BASE + 0x0e;

/// 终止符类型。
pub const AT_NULL: u64 = 0;
/// 页大小。
pub const AT_PAGESZ: u64 = 6;
/// 解释器基址。
pub const AT_BASE: u64 = 7;
/// 程序入口。
pub const AT_ENTRY: u64 = 9;
/// 真实用户 ID。
pub const AT_UID: u64 = 11;
/// 有效用户 ID。
pub const AT_EUID: u64 = 12;
/// 随机字节指针。
pub const AT_RANDOM: u64 = 25;
/// 可执行文件名指针。
pub const AT_EXECFN: u64 = 31;

/// 页大小真值。
pub const PAGE: u64 = 0x1000;
/// 单记录宽度。
pub const RECORD: usize = 16;
/// 对数上限。
pub const MAX_PAIRS: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：构造。
pub const OP_SET: u8 = 1;
/// 操作码：对数。
pub const OP_COUNT: u8 = 2;
/// 操作码：查值。
pub const OP_FIND: u8 = 3;
/// 操作码：类型列。
pub const OP_TYPE: u8 = 4;
/// 操作码：值列。
pub const OP_VAL: u8 = 5;
/// 操作码：取字节。
pub const OP_BYTE: u8 = 6;
/// 操作码：总字节数。
pub const OP_TOTAL: u8 = 7;

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

/// 是否指针类类型（其值必须 8 字节对齐）。
pub fn pointer_type(t: u64) -> bool {
    t == AT_BASE || t == AT_ENTRY || t == AT_RANDOM || t == AT_EXECFN
}

/// 辅助向量。
#[derive(Clone, Copy, Debug)]
pub struct Auxv {
    types: [u64; MAX_PAIRS],
    vals: [u64; MAX_PAIRS],
    count: usize,
}

impl Auxv {
    /// 空向量（只有终止符）。
    pub const fn new() -> Auxv {
        Auxv { types: [0u64; MAX_PAIRS], vals: [0u64; MAX_PAIRS], count: 0 }
    }

    /// 构造向量。
    ///
    /// 成功返回字节流总长（含 16 字节终止符）。校验全部通过才落盘。
    pub fn set(&mut self, pairs: &[(u64, u64)]) -> Result<usize, u32> {
        if pairs.len() > MAX_PAIRS {
            return Err(AU_EFULL);
        }
        let mut i = 0usize;
        while i < pairs.len() {
            let (t, v) = match pairs.get(i) {
                Some(p) => *p,
                None => return Err(AU_EFULL),
            };
            if t == AT_NULL {
                return Err(AU_ENULL);
            }
            // 重复类型：与前面所有类型比较。
            let mut j = 0usize;
            while j < i {
                if let Some(q) = pairs.get(j) {
                    if q.0 == t {
                        return Err(AU_EDUP);
                    }
                }
                j += 1;
            }
            if pointer_type(t) && v % 8 != 0 {
                return Err(AU_EALIGN);
            }
            if t == AT_PAGESZ && v != PAGE {
                return Err(AU_EPAGESZ);
            }
            if t == AT_ENTRY && v == 0 {
                return Err(AU_ENTRY);
            }
            i += 1;
        }

        i = 0;
        while i < pairs.len() {
            if let Some(p) = pairs.get(i) {
                if let Some(dst) = self.types.get_mut(i) {
                    *dst = p.0;
                }
                if let Some(dst) = self.vals.get_mut(i) {
                    *dst = p.1;
                }
            }
            i += 1;
        }
        self.count = pairs.len();
        Ok(self.total_bytes())
    }

    /// 有效对数（不含终止符）。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 总字节数（含终止符）。
    pub fn total_bytes(&self) -> usize {
        (self.count + 1) * RECORD
    }

    /// 第 i 对的类型。
    pub fn type_at(&self, i: usize) -> Result<u64, u32> {
        if i >= self.count {
            return Err(AU_EINDEX);
        }
        match self.types.get(i) {
            Some(v) => Ok(*v),
            None => Err(AU_EINDEX),
        }
    }

    /// 第 i 对的值。
    pub fn val_at(&self, i: usize) -> Result<u64, u32> {
        if i >= self.count {
            return Err(AU_EINDEX);
        }
        match self.vals.get(i) {
            Some(v) => Ok(*v),
            None => Err(AU_EINDEX),
        }
    }

    /// 第 i 对。
    pub fn pair(&self, i: usize) -> Result<(u64, u64), u32> {
        Ok((self.type_at(i)?, self.val_at(i)?))
    }

    /// 按类型查值。
    ///
    /// 终止符 `AT_NULL` 不算条目，查它返回 `ENOENT`。
    pub fn find(&self, t: u64) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < self.count {
            if self.type_at(i) == Ok(t) {
                return self.val_at(i);
            }
            i += 1;
        }
        Err(AU_ENOENT)
    }

    /// 字节流中第 i 个字节。
    ///
    /// 有效对之后到总长之前是终止符的 16 个零字节，可逐字节取 0；
    /// 越过总长报 `ENOBS`。
    pub fn byte_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.total_bytes() {
            return Err(AU_ENOBS);
        }
        let body = self.count * RECORD;
        if i >= body {
            return Ok(0);
        }
        let rec = i / RECORD;
        let rel = i % RECORD;
        let word = if rel < 8 {
            match self.types.get(rec) {
                Some(v) => *v,
                None => return Err(AU_ENOBS),
            }
        } else {
            match self.vals.get(rec) {
                Some(v) => *v,
                None => return Err(AU_ENOBS),
            }
        };
        let shift = (rel % 8) * 8;
        Ok(((word >> shift) & 0xff) as u32)
    }

    /// 模拟带偏移的 `read(2)`。
    pub fn read_from(&self, off: usize, size: usize) -> Result<u32, u32> {
        let total = self.total_bytes();
        if off > total {
            return Err(AU_EOFF);
        }
        let avail = total - off;
        let n = if size < avail { size } else { avail };
        Ok(n as u32)
    }

    /// 终止符是否全零。
    pub fn ends_with_null(&self) -> bool {
        let body = self.count * RECORD;
        let mut i = body;
        while i < self.total_bytes() {
            if self.byte_at(i) != Ok(0) {
                return false;
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
            return Err(AU_EORDER);
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
            return Err(AU_EBAL);
        }
        const VALS: [u32; 8] = [96, 5, 4096, 4198400, 11, 1000, 6, 96];
        const CODES: [u32; 6] = [
            AU_ENULL,
            AU_EDUP,
            AU_EFULL,
            AU_EALIGN,
            AU_EPAGESZ,
            AU_ENTRY,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AU_EBAL),
            };
            if s.slot as usize != i {
                return Err(AU_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AU_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AU_EMISMATCH);
                        }
                    }
                    None => return Err(AU_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AU_EMISMATCH);
                        }
                    }
                    None => return Err(AU_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AU_EBAL);
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
            return Err(AU_EBAL);
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
            Err(AU_ESEAL)
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

/// 账本用的五对：页大小、解释器基址、入口、UID、EUID。
pub fn sample_pairs() -> [(u64, u64); 5] {
    [
        (AT_PAGESZ, PAGE),
        (AT_BASE, 0x7f0000000000),
        (AT_ENTRY, 0x401000),
        (AT_UID, 1000),
        (AT_EUID, 1000),
    ]
}

/// 十四格账本：布局 → 查询 → 类型/值列 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = Auxv::new();
    let pairs = sample_pairs();

    let _ = l.register(cell(0, OP_SET, a.set(&pairs)));
    let _ = l.register(cell(1, OP_COUNT, Ok(a.count())));
    let _ = l.register(cell(2, OP_FIND, a.find(AT_PAGESZ).map(|v| v as usize)));
    let _ = l.register(cell(3, OP_FIND, a.find(AT_ENTRY).map(|v| v as usize)));
    let _ = l.register(cell(4, OP_TYPE, a.type_at(3).map(|v| v as usize)));
    let _ = l.register(cell(5, OP_VAL, a.val_at(3).map(|v| v as usize)));
    let _ = l.register(word_cell(6, OP_BYTE, a.byte_at(0)));
    let _ = l.register(cell(7, OP_TOTAL, Ok(a.total_bytes())));

    let null: [(u64, u64); 1] = [(AT_NULL, 0)];
    let _ = l.register(cell(8, OP_SET, a.set(&null)));

    let dup: [(u64, u64); 2] = [(AT_UID, 1), (AT_UID, 2)];
    let _ = l.register(cell(9, OP_SET, a.set(&dup)));

    let many: [(u64, u64); 9] = [
        (1, 1),
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 5),
        (6, 6),
        (8, 8),
        (10, 10),
        (14, 14),
    ];
    let _ = l.register(cell(10, OP_SET, a.set(&many)));

    let unaligned: [(u64, u64); 1] = [(AT_RANDOM, 0x1001)];
    let _ = l.register(cell(11, OP_SET, a.set(&unaligned)));

    let badpage: [(u64, u64); 1] = [(AT_PAGESZ, 0x2000)];
    let _ = l.register(cell(12, OP_SET, a.set(&badpage)));

    let zero_entry: [(u64, u64); 1] = [(AT_ENTRY, 0)];
    let _ = l.register(cell(13, OP_SET, a.set(&zero_entry)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：16 字节记录布局、小端字段与零终止符。
    let pairs = sample_pairs();
    let mut a = Auxv::new();
    let set = a.set(&pairs);
    out[1] = set == Ok(96)
        && a.total_bytes() == 96
        && a.count() == 5
        && a.pair(0) == Ok((AT_PAGESZ, PAGE))
        && a.pair(4) == Ok((AT_EUID, 1000))
        && a.byte_at(0) == Ok(6)
        && a.byte_at(1) == Ok(0)
        && a.byte_at(8) == Ok(0)
        && a.byte_at(9) == Ok(0x10)
        && a.byte_at(15) == Ok(0)
        && a.byte_at(16) == Ok(7)
        && a.byte_at(80) == Ok(0)
        && a.byte_at(95) == Ok(0)
        && a.byte_at(96) == Err(AU_ENOBS)
        && a.ends_with_null();

    // 二路：按类型查询与偏移读取。
    let mut d = Auxv::new();
    let _ = d.set(&pairs);
    out[2] = d.find(AT_PAGESZ) == Ok(4096)
        && d.find(AT_ENTRY) == Ok(0x401000)
        && d.find(AT_EUID) == Ok(1000)
        && d.find(AT_RANDOM) == Err(AU_ENOENT)
        && d.find(AT_NULL) == Err(AU_ENOENT)
        && d.pair(5) == Err(AU_EINDEX)
        && d.val_at(6) == Err(AU_EINDEX)
        && d.read_from(0, 16) == Ok(16)
        && d.read_from(80, 16) == Ok(16)
        && d.read_from(80, 100) == Ok(16)
        && d.read_from(96, 8) == Ok(0)
        && d.read_from(97, 1) == Err(AU_EOFF);

    // 三路：闸门次序、失败不改状态、空向量只有终止符。
    let mut g = Auxv::new();
    let _ = g.set(&pairs);
    let many: [(u64, u64); 9] = [
        (1, 1),
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 5),
        (6, 6),
        (8, 8),
        (10, 10),
        (14, 14),
    ];
    let mixed: [(u64, u64); 9] = [
        (AT_NULL, 0),
        (AT_UID, 1),
        (AT_UID, 2),
        (AT_RANDOM, 0x1001),
        (AT_PAGESZ, 0x2000),
        (AT_ENTRY, 0),
        (1, 1),
        (2, 2),
        (3, 3),
    ];
    let count_first = g.set(&many) == Err(AU_EFULL) && g.set(&mixed) == Err(AU_EFULL);
    let null: [(u64, u64); 1] = [(AT_NULL, 0)];
    let dup: [(u64, u64); 2] = [(AT_UID, 1), (AT_UID, 2)];
    let unaligned: [(u64, u64); 1] = [(AT_RANDOM, 0x1001)];
    let badpage: [(u64, u64); 1] = [(AT_PAGESZ, 0x2000)];
    let zero_entry: [(u64, u64); 1] = [(AT_ENTRY, 0)];
    let goodpage: [(u64, u64); 1] = [(AT_PAGESZ, PAGE)];
    let gates = g.set(&null) == Err(AU_ENULL)
        && g.set(&dup) == Err(AU_EDUP)
        && g.set(&unaligned) == Err(AU_EALIGN)
        && g.set(&badpage) == Err(AU_EPAGESZ)
        && g.set(&zero_entry) == Err(AU_ENTRY)
        && g.count() == 5
        && g.find(AT_ENTRY) == Ok(0x401000)
        && g.set(&goodpage) == Ok(32)
        && g.count() == 1;

    let z = Auxv::new();
    let empty_ok = z.count() == 0
        && z.total_bytes() == 16
        && z.byte_at(0) == Ok(0)
        && z.byte_at(15) == Ok(0)
        && z.byte_at(16) == Err(AU_ENOBS)
        && z.ends_with_null()
        && z.find(AT_PAGESZ) == Err(AU_ENOENT)
        && z.read_from(16, 4) == Ok(0)
        && z.read_from(17, 1) == Err(AU_EOFF);
    out[3] = count_first && gates && empty_ok;

    out
}

/// 十三项判据。
pub fn run_unx11346_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11346_auxv_pairs");
    let l = build_ledger();

    s.add_dyn(
        "AV1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "AV2-总字节数含终止符",
        l.sample(0).map(|x| x.value) == Some(96),
        "static",
    );

    s.add(
        "AV3-有效对数",
        l.sample(1).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "AV4-页大小真值",
        l.sample(2).map(|x| x.value) == Some(4096),
        "static",
    );

    s.add(
        "AV5-入口地址",
        l.sample(3).map(|x| x.value) == Some(0x401000),
        "static",
    );

    s.add(
        "AV6-类型列",
        l.sample(4).map(|x| x.value) == Some(11),
        "static",
    );

    s.add(
        "AV7-值列",
        l.sample(5).map(|x| x.value) == Some(1000),
        "static",
    );

    s.add(
        "AV8-首字节小端",
        {
            let pairs = sample_pairs();
            let mut a = Auxv::new();
            let _ = a.set(&pairs);
            l.sample(6).map(|x| x.value) == Some(6)
                && a.byte_at(0) == Ok(6)
                && a.byte_at(1) == Ok(0)
                && a.byte_at(8) == Ok(0)
                && a.byte_at(9) == Ok(0x10)
        },
        "static",
    );

    s.add(
        "AV9-终止符全零",
        {
            let pairs = sample_pairs();
            let mut a = Auxv::new();
            let _ = a.set(&pairs);
            l.sample(7).map(|x| x.value) == Some(96)
                && a.total_bytes() == 96
                && a.byte_at(80) == Ok(0)
                && a.byte_at(95) == Ok(0)
                && a.byte_at(96) == Err(AU_ENOBS)
                && a.ends_with_null()
                && a.find(AT_NULL) == Err(AU_ENOENT)
        },
        "static",
    );

    s.add(
        "AV10-显式终止与重复",
        l.sample(8).map(|x| x.code) == Some(AU_ENULL)
            && l.sample(9).map(|x| x.code) == Some(AU_EDUP),
        "static",
    );

    s.add(
        "AV11-容量与指针对齐",
        l.sample(10).map(|x| x.code) == Some(AU_EFULL)
            && l.sample(11).map(|x| x.code) == Some(AU_EALIGN),
        "static",
    );

    s.add(
        "AV12-页大小与入口闸门",
        l.sample(12).map(|x| x.code) == Some(AU_EPAGESZ)
            && l.sample(13).map(|x| x.code) == Some(AU_ENTRY),
        "static",
    );

    s.add(
        "AV13-冻结重放四注入",
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

    fn five() -> Auxv {
        let pairs = sample_pairs();
        let mut a = Auxv::new();
        let _ = a.set(&pairs);
        a
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
        let s = run_unx11346_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn record_layout() {
        let a = five();
        assert_eq!(a.total_bytes(), 96);
        assert_eq!(a.count(), 5);
        assert_eq!(a.byte_at(0), Ok(6));
        assert_eq!(a.byte_at(1), Ok(0));
        assert_eq!(a.byte_at(8), Ok(0));
        assert_eq!(a.byte_at(9), Ok(0x10));
        assert_eq!(a.byte_at(16), Ok(7));
        assert_eq!(a.byte_at(96), Err(AU_ENOBS));
        assert!(a.ends_with_null());
    }

    #[test]
    fn lookups() {
        let a = five();
        assert_eq!(a.find(AT_PAGESZ), Ok(4096));
        assert_eq!(a.find(AT_ENTRY), Ok(0x401000));
        assert_eq!(a.find(AT_RANDOM), Err(AU_ENOENT));
        assert_eq!(a.find(AT_NULL), Err(AU_ENOENT));
        assert_eq!(a.pair(0), Ok((AT_PAGESZ, PAGE)));
        assert_eq!(a.pair(5), Err(AU_EINDEX));
    }

    #[test]
    fn partial_reads() {
        let a = five();
        assert_eq!(a.read_from(0, 16), Ok(16));
        assert_eq!(a.read_from(80, 16), Ok(16));
        assert_eq!(a.read_from(96, 8), Ok(0));
        assert_eq!(a.read_from(97, 1), Err(AU_EOFF));
    }

    #[test]
    fn rejects_structural() {
        let mut a = Auxv::new();
        let null: [(u64, u64); 1] = [(AT_NULL, 0)];
        let dup: [(u64, u64); 2] = [(AT_UID, 1), (AT_UID, 2)];
        let many: [(u64, u64); 9] = [
            (1, 1),
            (2, 2),
            (3, 3),
            (4, 4),
            (5, 5),
            (6, 6),
            (8, 8),
            (10, 10),
            (14, 14),
        ];
        assert_eq!(a.set(&null), Err(AU_ENULL));
        assert_eq!(a.set(&dup), Err(AU_EDUP));
        assert_eq!(a.set(&many), Err(AU_EFULL));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn rejects_type_semantics() {
        let mut a = Auxv::new();
        let unaligned: [(u64, u64); 1] = [(AT_RANDOM, 0x1001)];
        let badpage: [(u64, u64); 1] = [(AT_PAGESZ, 0x2000)];
        let zero_entry: [(u64, u64); 1] = [(AT_ENTRY, 0)];
        assert_eq!(a.set(&unaligned), Err(AU_EALIGN));
        assert_eq!(a.set(&badpage), Err(AU_EPAGESZ));
        assert_eq!(a.set(&zero_entry), Err(AU_ENTRY));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn accepts_boundary_values() {
        let mut a = Auxv::new();
        let random: [(u64, u64); 1] = [(AT_RANDOM, 0x1008)];
        let base: [(u64, u64); 1] = [(AT_BASE, 0)];
        let entry: [(u64, u64); 1] = [(AT_ENTRY, 8)];
        assert_eq!(a.set(&random), Ok(32));
        assert_eq!(a.set(&base), Ok(32));
        assert_eq!(a.set(&entry), Ok(32));
        assert_eq!(a.find(AT_ENTRY), Ok(8));
    }

    #[test]
    fn empty_auxv() {
        let mut a = Auxv::new();
        assert_eq!(a.set(&[]), Ok(16));
        assert_eq!(a.count(), 0);
        assert_eq!(a.total_bytes(), 16);
        assert_eq!(a.byte_at(0), Ok(0));
        assert_eq!(a.find(AT_PAGESZ), Err(AU_ENOENT));
        assert_eq!(a.read_from(16, 4), Ok(0));
        assert!(a.ends_with_null());
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
