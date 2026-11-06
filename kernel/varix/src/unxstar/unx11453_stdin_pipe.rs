//! UNX-F11453 `sort`/`grep` stdin 管道语义（0x20c10 段）
//!
//! 两个工具对“输入从哪来”的口径一致却不相同：无操作数时读标准输入，操作数
//! `-` 也读标准输入，其余操作数按文件名打开。差别在于标准输入只能被真正消费
//! 一次——`sort - -` 的第二个 `-` 读到的是已经排空的流，得到零条记录，这**不是
//! 错误**；而同名的普通文件出现两次就会被完整读两遍。把“重复的标准输入”误判成
//! 重复文件（或反过来）会让去重统计整体偏移。
//!
//! 目录操作数是主要的分歧点：`sort dir` 直接失败（EISDIR），`grep dir` 打印诊断
//! 后跳过并继续处理其余操作数。模型把两种行为分开记账：排序路径返回 SD_EDIR，
//! 检索路径累加 skipped，只有全部操作数都被跳过、一个有效源都不剩时才是
//! SD_ENOSRC。空操作数（`""`）与内嵌 NUL 是两种不同的坏输入，分别记 EOPERAND
//! 与 ENUL，闸门次序固定为：工具 → 操作数个数 → 逐操作数分类。
//!
//! 账本十四格：八格成功（隐式标准输入、显式 `-`、双文件、混合、双 `-` 排空、
//! 目录跳过、三文件消费数、八操作数满额），六格拒绝（空操作数、超个数、排序遇
//! 目录、NUL 字节、容量满、重复登记）。审计逐格钉住消费数与跳过数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c10;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const SD_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const SD_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const SD_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const SD_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const SD_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const SD_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const SD_EFULL: u16 = SEG_LO + 0x07;
/// 未知工具。
pub const SD_ETOOL: u16 = SEG_LO + 0x08;
/// 空操作数。
pub const SD_EOPERAND: u16 = SEG_LO + 0x09;
/// 操作数内嵌 NUL。
pub const SD_ENUL: u16 = SEG_LO + 0x0a;
/// 排序遇目录。
pub const SD_EDIR: u16 = SEG_LO + 0x0b;
/// 无有效输入源。
pub const SD_ENOSRC: u16 = SEG_LO + 0x0c;
/// 操作数超上限。
pub const SD_EMANY_OPS: u16 = SEG_LO + 0x0e;
/// 票号不一致。
pub const SD_ESEAL: u16 = SEG_LO + 0x0d;

/// 工具：sort。
pub const TOOL_SORT: u8 = 1;
/// 工具：grep。
pub const TOOL_GREP: u8 = 2;

/// 源类型：标准输入。
pub const SRC_STDIN: u8 = 1;
/// 源类型：普通文件。
pub const SRC_FILE: u8 = 2;
/// 源类型：目录（仅记录，不作输入）。
pub const SRC_DIR: u8 = 3;

/// 操作：编排输入源。
pub const OP_PLAN: u8 = 1;
/// 操作：统计消费数。
pub const OP_CONSUME: u8 = 2;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 3;

/// 操作数上限。
pub const MAX_OPS: usize = 8;
/// 账本格数。
pub const ROWS: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 输入源编排结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kinds: [u8; MAX_OPS],
    pub n: u8,
    pub stdin_uses: u8,
    pub drained: u8,
    pub skipped: u8,
}

impl Plan {
    /// 空编排。
    pub const fn blank() -> Plan {
        Plan {
            kinds: [0; MAX_OPS],
            n: 0,
            stdin_uses: 0,
            drained: 0,
            skipped: 0,
        }
    }

    /// 取第 i 个有效源。
    pub fn kind(&self, i: usize) -> Option<u8> {
        if i >= self.n as usize {
            return None;
        }
        self.kinds.get(i).copied()
    }
}

/// 编排：无操作数读标准输入，`-` 也是标准输入，目录按工具分开处置。
pub fn plan(ops: &[u8], tool: u8) -> Result<Plan, u16> {
    if tool != TOOL_SORT && tool != TOOL_GREP {
        return Err(SD_ETOOL);
    }
    if ops.len() > MAX_OPS {
        return Err(SD_EMANY_OPS);
    }
    let mut p = Plan::blank();
    if ops.is_empty() {
        if let Some(slot) = p.kinds.get_mut(0) {
            *slot = SRC_STDIN;
        }
        p.n = 1;
        p.stdin_uses = 1;
        return Ok(p);
    }
    let mut i = 0usize;
    while i < ops.len() {
        let b = match ops.get(i) {
            Some(v) => *v,
            None => return Err(SD_ENOBS),
        };
        let kind = match b {
            b'-' => SRC_STDIN,
            b'f' => SRC_FILE,
            b'd' => SRC_DIR,
            0 => return Err(SD_ENUL),
            _ => return Err(SD_EOPERAND),
        };
        if kind == SRC_DIR {
            if tool == TOOL_SORT {
                return Err(SD_EDIR);
            }
            p.skipped = p.skipped.saturating_add(1);
            i += 1;
            continue;
        }
        if kind == SRC_STDIN {
            p.stdin_uses = p.stdin_uses.saturating_add(1);
            if p.stdin_uses > 1 {
                // 标准输入只能被消费一次，再一次读得空流。
                p.drained = p.drained.saturating_add(1);
            }
        }
        if p.n as usize >= MAX_OPS {
            return Err(SD_EMANY_OPS);
        }
        if let Some(slot) = p.kinds.get_mut(p.n as usize) {
            *slot = kind;
        }
        p.n = p.n.saturating_add(1);
        i += 1;
    }
    if p.n == 0 {
        return Err(SD_ENOSRC);
    }
    Ok(p)
}

/// 实际消费的源个数（排空的标准输入不算）。
pub fn consumed(p: &Plan) -> u32 {
    u32::from(p.n).saturating_sub(u32::from(p.drained))
}

/// 文件是否被完整读两遍（重复文件为真，重复标准输入为假）。
pub fn rereads(p: &Plan) -> bool {
    u32::from(p.n) > consumed(p)
}

/// 首个有效源类型。
pub fn effective(p: &Plan) -> Option<u8> {
    p.kind(0)
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u16,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 审计回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub rows: u32,
    pub ok: u32,
    pub err: u32,
    pub root: u64,
}

/// 样本账本。
pub struct Ledger {
    rows: [Sample; ROWS],
    len: usize,
    cap: usize,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 默认容量。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: ROWS,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); ROWS],
            len: 0,
            cap: if cap > ROWS { ROWS } else { cap },
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 重复 → 顺序 → 容量 → 一致性。
    pub fn register(&mut self, s: Sample) -> Result<(), u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(SD_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(SD_EDUP);
                    }
                }
                None => return Err(SD_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(SD_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(SD_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(SD_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(SD_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(SD_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(SD_EFULL),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
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
    pub fn audit(&self) -> Result<AuditReport, u16> {
        if self.len != ROWS {
            return Err(SD_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SD_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SD_ENOBS),
            };
            if s.slot as usize != i {
                return Err(SD_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SD_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(SD_EMISMATCH);
            }
            i += 1;
        }
        let vals = [1u32, 1, 2, 2, 1, 1, 3, 8];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(SD_EMISMATCH);
                    }
                }
                _ => return Err(SD_EBAL),
            }
            k += 1;
        }
        let codes = [
            SD_EOPERAND, SD_EMANY_OPS, SD_EDIR, SD_ENUL, SD_EFULL, SD_EDUP,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(SD_EMISMATCH);
                        }
                    }
                    None => return Err(SD_EBAL),
                },
                None => return Err(SD_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(SD_EBAL);
        }
        Ok(AuditReport {
            rows: self.len as u32,
            ok: self.ok_count() as u32,
            err: self.err_count() as u32,
            root: self.seal(),
        })
    }

    /// 熔断票号。
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
    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            self.rejects += 1;
            return Err(SD_EFROZEN);
        }
        let s = self.seal();
        self.root = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u16> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(SD_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(SD_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 越闸次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 成功格。
fn ok_row(slot: u8, op: u8, value: u32) -> Sample {
    Sample {
        slot,
        op,
        ok: true,
        code: 0,
        value,
    }
}

/// 失败格。
fn err_row(slot: u8, op: u8, code: u16) -> Sample {
    Sample {
        slot,
        op,
        ok: false,
        code,
        value: 0,
    }
}

/// 取登记错误码。
fn reg_code(r: Result<(), u16>) -> u16 {
    match r {
        Ok(()) => 0,
        Err(c) => c,
    }
}

/// 编排结果取有效源个数。
fn source_count(r: Result<Plan, u16>) -> u32 {
    match r {
        Ok(p) => u32::from(p.n),
        Err(_) => 0,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(ok_row(0, OP_PLAN, source_count(plan(b"", TOOL_SORT))));
    let _ = l.register(ok_row(1, OP_PLAN, source_count(plan(b"-", TOOL_GREP))));
    let _ = l.register(ok_row(2, OP_PLAN, source_count(plan(b"ff", TOOL_SORT))));
    let _ = l.register(ok_row(3, OP_PLAN, source_count(plan(b"-f", TOOL_GREP))));

    let drained = match plan(b"--", TOOL_SORT) {
        Ok(p) => u32::from(p.drained),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(4, OP_CONSUME, drained));

    let skipped = match plan(b"df", TOOL_GREP) {
        Ok(p) => u32::from(p.skipped),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(5, OP_CONSUME, skipped));

    let three = match plan(b"fff", TOOL_SORT) {
        Ok(p) => consumed(&p),
        Err(_) => 0,
    };
    let _ = l.register(ok_row(6, OP_CONSUME, three));

    let _ = l.register(ok_row(7, OP_PLAN, source_count(plan(b"ffffffff", TOOL_SORT))));

    let empty_op = match plan(b"?", TOOL_SORT) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_PLAN, empty_op));

    let many = match plan(b"fffffffff", TOOL_SORT) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_PLAN, many));

    let dir = match plan(b"d", TOOL_SORT) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_PLAN, dir));

    let nul = match plan(&[0u8], TOOL_GREP) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_PLAN, nul));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_PLAN, 1));
    let _ = small.register(ok_row(1, OP_PLAN, 1));
    let _ = small.register(ok_row(2, OP_PLAN, 1));
    let full = reg_code(small.register(ok_row(3, OP_PLAN, 1)));
    let _ = l.register(err_row(12, OP_LEDGER, full));

    let mut d = Ledger::new();
    let _ = d.register(ok_row(0, OP_PLAN, 1));
    let dup = reg_code(d.register(ok_row(0, OP_PLAN, 2)));
    let _ = l.register(err_row(13, OP_LEDGER, dup));

    l
}

/// 四路注入：重放冻结、标准输入单次消费、目录分歧、篡改复原。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let before = l.len();
    let rep = l.replay();
    let one = l.freeze();
    let two = l.freeze();
    let root = l.root();
    out[0] = rep == one
        && one == Ok(root)
        && two == Err(SD_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = match (plan(b"--", TOOL_SORT), plan(b"ff", TOOL_SORT)) {
        (Ok(a), Ok(b)) => {
            a.stdin_uses == 2
                && a.drained == 1
                && consumed(&a) == 1
                && consumed(&b) == 2
                && !rereads(&b)
                && consumed(&a) < consumed(&b)
        }
        _ => false,
    };

    out[2] = plan(b"d", TOOL_SORT) == Err(SD_EDIR)
        && plan(b"d", TOOL_GREP) == Err(SD_ENOSRC)
        && match plan(b"df", TOOL_GREP) {
            Ok(p) => p.skipped == 1 && p.n == 1 && effective(&p) == Some(SRC_FILE),
            Err(_) => false,
        };

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(6) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(6) {
            r.value = old.saturating_add(7);
        }
        let caught = t.audit() == Err(SD_EMISMATCH);
        if let Some(r) = t.rows.get_mut(6) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11453_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11453.stdinpipe");
    let l = build_ledger();

    s.add_dyn(
        "SD1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SD2-隐式标准输入",
        l.sample(0).map(|x| x.value) == Some(1)
            && match plan(b"", TOOL_SORT) {
                Ok(p) => p.n == 1 && p.stdin_uses == 1 && effective(&p) == Some(SRC_STDIN),
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD3-显式短横",
        l.sample(1).map(|x| x.value) == Some(1)
            && match plan(b"-", TOOL_GREP) {
                Ok(p) => p.kind(0) == Some(SRC_STDIN) && consumed(&p) == 1,
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD4-双文件双读",
        l.sample(2).map(|x| x.value) == Some(2)
            && match plan(b"ff", TOOL_SORT) {
                Ok(p) => consumed(&p) == 2 && !rereads(&p),
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD5-混合源",
        l.sample(3).map(|x| x.value) == Some(2)
            && match plan(b"-f", TOOL_GREP) {
                Ok(p) => p.kind(0) == Some(SRC_STDIN)
                    && p.kind(1) == Some(SRC_FILE)
                    && p.kind(2).is_none()
                    && consumed(&p) == 2,
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD6-双横排空不报错",
        l.sample(4).map(|x| x.value) == Some(1)
            && match plan(b"--", TOOL_SORT) {
                Ok(p) => p.stdin_uses == 2
                    && p.drained == 1
                    && consumed(&p) == 1
                    && rereads(&p)
                    && p.n == 2,
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD7-目录歧义",
        l.sample(5).map(|x| x.value) == Some(1)
            && plan(b"d", TOOL_SORT) == Err(SD_EDIR)
            && plan(b"d", TOOL_GREP) == Err(SD_ENOSRC),
        "static",
    );

    s.add(
        "SD8-三文件消费数",
        l.sample(6).map(|x| x.value) == Some(3)
            && match plan(b"fff", TOOL_SORT) {
                Ok(p) => p.n == 3 && consumed(&p) == 3 && !rereads(&p),
                Err(_) => false,
            },
        "static",
    );

    s.add(
        "SD9-满额八操作数",
        l.sample(7).map(|x| x.value) == Some(8)
            && plan(b"ffffffff", TOOL_SORT).is_ok()
            && plan(b"fffffffff", TOOL_SORT) == Err(SD_EMANY_OPS),
        "static",
    );

    s.add(
        "SD10-空操作数闸门",
        l.sample(8).map(|x| x.code) == Some(SD_EOPERAND)
            && plan(b"?", TOOL_SORT) == Err(SD_EOPERAND)
            && plan(b"", TOOL_SORT).is_ok(),
        "static",
    );

    s.add(
        "SD11-目录与NUL分档",
        l.sample(10).map(|x| x.code) == Some(SD_EDIR)
            && l.sample(11).map(|x| x.code) == Some(SD_ENUL)
            && plan(&[0u8], TOOL_GREP) == Err(SD_ENUL)
            && plan(b"d", TOOL_SORT) == Err(SD_EDIR),
        "static",
    );

    s.add(
        "SD12-容量与重复闸门",
        l.sample(12).map(|x| x.code) == Some(SD_EFULL)
            && l.sample(13).map(|x| x.code) == Some(SD_EDUP)
            && {
                let mut d = Ledger::new();
                let _ = d.register(ok_row(0, OP_PLAN, 1));
                d.register(ok_row(0, OP_PLAN, 2)) == Err(SD_EDUP)
                    && d.register(ok_row(2, OP_PLAN, 2)) == Err(SD_EORDER)
                    && d.len() == 1
            },
        "static",
    );

    s.add(
        "SD13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(SD_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_stdin() {
        let p = plan(b"", TOOL_SORT).unwrap_or(Plan::blank());
        assert_eq!(p.n, 1);
        assert_eq!(p.stdin_uses, 1);
        assert_eq!(effective(&p), Some(SRC_STDIN));
        assert_eq!(consumed(&p), 1);
    }

    #[test]
    fn stdin_consumed_once() {
        let p = plan(b"--", TOOL_SORT).unwrap_or(Plan::blank());
        assert_eq!(p.stdin_uses, 2);
        assert_eq!(p.drained, 1);
        assert_eq!(consumed(&p), 1);
        assert!(rereads(&p));
        let q = plan(b"ff", TOOL_SORT).unwrap_or(Plan::blank());
        assert_eq!(consumed(&q), 2);
        assert!(!rereads(&q));
    }

    #[test]
    fn directory_diverges_by_tool() {
        assert_eq!(plan(b"d", TOOL_SORT), Err(SD_EDIR));
        assert_eq!(plan(b"d", TOOL_GREP), Err(SD_ENOSRC));
        let p = plan(b"df", TOOL_GREP).unwrap_or(Plan::blank());
        assert_eq!(p.skipped, 1);
        assert_eq!(p.n, 1);
    }

    #[test]
    fn operand_gates() {
        assert_eq!(plan(b"?", TOOL_SORT), Err(SD_EOPERAND));
        assert_eq!(plan(&[0u8], TOOL_SORT), Err(SD_ENUL));
        assert_eq!(plan(b"fffffffff", TOOL_SORT), Err(SD_EMANY_OPS));
        assert_eq!(plan(b"f", 9), Err(SD_ETOOL));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SD_EFROZEN));
        assert!(l.is_frozen());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11453_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
