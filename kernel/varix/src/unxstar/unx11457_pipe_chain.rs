//! UNX-F11457 组合探针（管道链）（0x20c50 段）
//!
//! `grep pat f | sort | grep -c x` 是一条最常见的判据命令，但它的退出码语义常被
//! 误解：POSIX 规定管道的退出码是**最后一段**的退出码，前面各段的失败默认被吞掉。
//! 只有 `set -o pipefail`（或 shell 的等价开关）才把“第一个非零”抬成管道状态。
//! 因此同一条链在两种开关下可以给出不同答案，而两边的数据流完全一样——把这两件
//! 事混在一起，就会出现“修好了退出码、数据却少了”的假绿。
//!
//! 数据流侧另有一条独立不变量：`grep` 按千分比过滤条数（`checked_mul`，溢出记
//! EOVER，比例超 1000 记 EKEEP），`sort` 只改顺序不改条数，`grep -c` 把任意非空
//! 流压成一行。三段链从 1000 条出发，过滤千分之二百五十得 250 条，排序后仍是
//! 250 条，计数后是 1 行。
//!
//! 下游提前退出会让上游写管道时收到 SIGPIPE（128+13=141）：写入量超过管道缓冲
//! 即判破裂。模型把它做成独立函数，不与退出码状态混用。闸门次序：空链（EEMPTY）
//! → 段数超限（EMANY）→ 逐段种类（ESTAGE）→ 比例（EKEEP）→ 算术（EOVER）。
//!
//! 账本十四格：八格成功（全零链、末位非零、pipefail 中段、pipefail 首段、末段
//! 错误、链长、过滤条数、SIGPIPE），六格拒绝（空链、超长链、乘法溢出、未知段、
//! 容量满、重复登记）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c50;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const PC_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const PC_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const PC_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const PC_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const PC_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const PC_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const PC_EFULL: u16 = SEG_LO + 0x07;
/// 空管道。
pub const PC_EEMPTY: u16 = SEG_LO + 0x08;
/// 段数超限。
pub const PC_EMANY: u16 = SEG_LO + 0x09;
/// 算术溢出。
pub const PC_EOVER: u16 = SEG_LO + 0x0a;
/// 未知段种类。
pub const PC_ESTAGE: u16 = SEG_LO + 0x0b;
/// 过滤比例非法。
pub const PC_EKEEP: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const PC_ESEAL: u16 = SEG_LO + 0x0d;

/// 段种类：grep 过滤。
pub const K_GREP: u8 = 1;
/// 段种类：sort 排序。
pub const K_SORT: u8 = 2;
/// 段种类：grep -c 计数。
pub const K_COUNT: u8 = 3;

/// SIGPIPE 退出状态（128 + 13）。
pub const SIGPIPE_STATUS: u8 = 141;
/// 管道缓冲字节数。
pub const PIPE_BUF: u32 = 16;
/// 链段上限。
pub const MAX_STAGES: usize = 6;
/// 比例分母。
pub const PER_MILLE: u32 = 1000;

/// 操作：管道状态。
pub const OP_STATUS: u8 = 1;
/// 操作：数据流。
pub const OP_FLOW: u8 = 2;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 3;

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

/// 一段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub kind: u8,
    pub keep: u32,
}

impl Stage {
    /// 构造。
    pub const fn make(kind: u8, keep: u32) -> Stage {
        Stage { kind, keep }
    }
}

/// 管道状态：默认末位，pipefail 取首个非零。
pub fn pipeline_status(stages: &[u8], pipefail: bool) -> Result<u8, u16> {
    if stages.is_empty() {
        return Err(PC_EEMPTY);
    }
    if stages.len() > MAX_STAGES {
        return Err(PC_EMANY);
    }
    let mut i = 0usize;
    let mut first_bad = 0u8;
    while i < stages.len() {
        let c = match stages.get(i) {
            Some(v) => *v,
            None => return Err(PC_ENOBS),
        };
        if c != 0 && first_bad == 0 {
            first_bad = c;
        }
        i += 1;
    }
    if pipefail {
        return Ok(first_bad);
    }
    match stages.get(stages.len() - 1) {
        Some(v) => Ok(*v),
        None => Err(PC_EBAL),
    }
}

/// 千分比过滤。
pub fn filter(input: u32, keep: u32) -> Result<u32, u16> {
    if keep > PER_MILLE {
        return Err(PC_EKEEP);
    }
    match input.checked_mul(keep) {
        Some(v) => Ok(v / PER_MILLE),
        None => Err(PC_EOVER),
    }
}

/// 三段链数据流：过滤 → 排序（条数不变）→ 计数。
pub fn chain(stages: &[Stage], input: u32) -> Result<u32, u16> {
    if stages.is_empty() {
        return Err(PC_EEMPTY);
    }
    if stages.len() > MAX_STAGES {
        return Err(PC_EMANY);
    }
    let mut recs = input;
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return Err(PC_ENOBS),
        };
        match s.kind {
            K_GREP => recs = filter(recs, s.keep)?,
            K_SORT => {}
            K_COUNT => recs = if recs > 0 { 1 } else { 0 },
            _ => return Err(PC_ESTAGE),
        }
        i += 1;
    }
    Ok(recs)
}

/// 链长（逐段校验种类）。
pub fn stage_count(stages: &[Stage]) -> Result<u32, u16> {
    if stages.is_empty() {
        return Err(PC_EEMPTY);
    }
    if stages.len() > MAX_STAGES {
        return Err(PC_EMANY);
    }
    let mut i = 0usize;
    while i < stages.len() {
        let s = match stages.get(i) {
            Some(v) => *v,
            None => return Err(PC_ENOBS),
        };
        if s.kind != K_GREP && s.kind != K_SORT && s.kind != K_COUNT {
            return Err(PC_ESTAGE);
        }
        i += 1;
    }
    match u32::try_from(stages.len()) {
        Ok(v) => Ok(v),
        Err(_) => Err(PC_EMANY),
    }
}

/// SIGPIPE 判定：写入超过缓冲即破裂。
pub fn sigpipe(written: u32, buffer: u32) -> u8 {
    if buffer == 0 || written > buffer {
        SIGPIPE_STATUS
    } else {
        0
    }
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
            return Err(PC_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(PC_EDUP);
                    }
                }
                None => return Err(PC_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(PC_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(PC_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(PC_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(PC_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(PC_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(PC_EFULL),
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
            return Err(PC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(PC_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PC_ENOBS),
            };
            if s.slot as usize != i {
                return Err(PC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PC_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(PC_EMISMATCH);
            }
            i += 1;
        }
        let vals = [0u32, 1, 1, 2, 2, 3, 250, 141];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(PC_EMISMATCH);
                    }
                }
                _ => return Err(PC_EBAL),
            }
            k += 1;
        }
        let codes = [
            PC_EEMPTY, PC_EMANY, PC_EOVER, PC_ESTAGE, PC_EFULL, PC_EDUP,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(PC_EMISMATCH);
                        }
                    }
                    None => return Err(PC_EBAL),
                },
                None => return Err(PC_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(PC_EBAL);
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
            return Err(PC_EFROZEN);
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
            return Err(PC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(PC_ESEAL);
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

/// 状态结果取数值。
fn status_of(r: Result<u8, u16>) -> u32 {
    match r {
        Ok(v) => u32::from(v),
        Err(_) => 0xffff_ffff,
    }
}

/// 三段标准链。
pub fn std_chain() -> [Stage; 3] {
    [
        Stage::make(K_GREP, 250),
        Stage::make(K_SORT, 0),
        Stage::make(K_COUNT, 0),
    ]
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(ok_row(0, OP_STATUS, status_of(pipeline_status(&[0, 0, 0], false))));
    let _ = l.register(ok_row(1, OP_STATUS, status_of(pipeline_status(&[0, 0, 1], false))));
    let _ = l.register(ok_row(2, OP_STATUS, status_of(pipeline_status(&[0, 1, 0], true))));
    let _ = l.register(ok_row(3, OP_STATUS, status_of(pipeline_status(&[2, 0, 0], true))));
    let _ = l.register(ok_row(4, OP_STATUS, status_of(pipeline_status(&[0, 1, 2], false))));

    let sc = std_chain();
    let count = match stage_count(&sc) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = l.register(ok_row(5, OP_FLOW, count));

    let filtered = match filter(1000, 250) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = l.register(ok_row(6, OP_FLOW, filtered));

    let _ = l.register(ok_row(7, OP_FLOW, u32::from(sigpipe(17, PIPE_BUF))));

    let empty = match pipeline_status(&[], false) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_STATUS, empty));

    let many = match pipeline_status(&[0, 0, 0, 0, 0, 0, 0], false) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_STATUS, many));

    let over = match filter(u32::MAX, 1000) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_FLOW, over));

    let bad = [Stage::make(9, 0)];
    let stage = match chain(&bad, 1000) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_FLOW, stage));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_STATUS, 1));
    let _ = small.register(ok_row(1, OP_STATUS, 1));
    let _ = small.register(ok_row(2, OP_STATUS, 1));
    let full = reg_code(small.register(ok_row(3, OP_STATUS, 1)));
    let _ = l.register(err_row(12, OP_LEDGER, full));

    let mut d = Ledger::new();
    let _ = d.register(ok_row(0, OP_STATUS, 1));
    let dup = reg_code(d.register(ok_row(0, OP_STATUS, 2)));
    let _ = l.register(err_row(13, OP_LEDGER, dup));

    l
}

/// 四路注入：重放冻结、pipefail 双口径、数据流、篡改复原。
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
        && two == Err(PC_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = pipeline_status(&[0, 1, 0], false) == Ok(0)
        && pipeline_status(&[0, 1, 0], true) == Ok(1)
        && pipeline_status(&[2, 0, 0], false) == Ok(0)
        && pipeline_status(&[2, 0, 0], true) == Ok(2)
        && pipeline_status(&[0, 0, 0], true) == Ok(0);

    out[2] = {
        let sc = std_chain();
        chain(&sc, 1000) == Ok(1)
            && chain(&sc, 0) == Ok(0)
            && filter(1000, 250) == Ok(250)
            && filter(1000, 1000) == Ok(1000)
            && sigpipe(PIPE_BUF, PIPE_BUF) == 0
            && sigpipe(PIPE_BUF + 1, PIPE_BUF) == SIGPIPE_STATUS
    };

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(6) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(6) {
            r.value = old.saturating_add(1);
        }
        let caught = t.audit() == Err(PC_EMISMATCH);
        if let Some(r) = t.rows.get_mut(6) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11457_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11457.pipechain");
    let l = build_ledger();

    s.add_dyn(
        "PC1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PC2-末位决定",
        l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(1).map(|x| x.value) == Some(1)
            && pipeline_status(&[0, 0, 0], false) == Ok(0)
            && pipeline_status(&[0, 0, 1], false) == Ok(1),
        "static",
    );

    s.add(
        "PC3-pipefail中段",
        l.sample(2).map(|x| x.value) == Some(1)
            && pipeline_status(&[0, 1, 0], true) == Ok(1)
            && pipeline_status(&[0, 0, 1], true) == Ok(1),
        "static",
    );

    s.add(
        "PC4-pipefail首段",
        l.sample(3).map(|x| x.value) == Some(2)
            && pipeline_status(&[2, 0, 0], true) == Ok(2)
            && pipeline_status(&[2, 1, 0], true) == Ok(2),
        "static",
    );

    s.add(
        "PC5-末段错误",
        l.sample(4).map(|x| x.value) == Some(2)
            && pipeline_status(&[0, 1, 2], false) == Ok(2)
            && pipeline_status(&[0, 1, 2], true) == Ok(1),
        "static",
    );

    s.add(
        "PC6-链长与三段流",
        l.sample(5).map(|x| x.value) == Some(3)
            && stage_count(&std_chain()) == Ok(3)
            && chain(&std_chain(), 1000) == Ok(1)
            && chain(&std_chain(), 0) == Ok(0),
        "static",
    );

    s.add(
        "PC7-过滤千分比",
        l.sample(6).map(|x| x.value) == Some(250)
            && filter(1000, 250) == Ok(250)
            && filter(0, 250) == Ok(0)
            && filter(1000, 1000) == Ok(1000)
            && filter(999, 1000) == Ok(999),
        "static",
    );

    s.add(
        "PC8-SIGPIPE判定",
        l.sample(7).map(|x| x.value) == Some(141)
            && sigpipe(1, PIPE_BUF) == 0
            && sigpipe(PIPE_BUF, PIPE_BUF) == 0
            && sigpipe(PIPE_BUF + 1, PIPE_BUF) == SIGPIPE_STATUS
            && sigpipe(1, 0) == SIGPIPE_STATUS,
        "static",
    );

    s.add(
        "PC9-空链闸门",
        l.sample(8).map(|x| x.code) == Some(PC_EEMPTY)
            && pipeline_status(&[], false) == Err(PC_EEMPTY)
            && pipeline_status(&[], true) == Err(PC_EEMPTY)
            && chain(&[], 1) == Err(PC_EEMPTY),
        "static",
    );

    s.add(
        "PC10-超长链闸门",
        l.sample(9).map(|x| x.code) == Some(PC_EMANY)
            && pipeline_status(&[0, 0, 0, 0, 0, 0, 0], false) == Err(PC_EMANY)
            && pipeline_status(&[0, 0, 0, 0, 0, 0], false).is_ok(),
        "static",
    );

    s.add(
        "PC11-溢出与比例闸门",
        l.sample(10).map(|x| x.code) == Some(PC_EOVER)
            && filter(u32::MAX, 1000) == Err(PC_EOVER)
            && filter(1000, 1001) == Err(PC_EKEEP)
            && filter(u32::MAX, 1) == Ok(u32::MAX / 1000),
        "static",
    );

    s.add(
        "PC12-未知段与容量",
        l.sample(11).map(|x| x.code) == Some(PC_ESTAGE)
            && l.sample(12).map(|x| x.code) == Some(PC_EFULL)
            && l.sample(13).map(|x| x.code) == Some(PC_EDUP)
            && chain(&[Stage::make(9, 0)], 1) == Err(PC_ESTAGE)
            && stage_count(&[Stage::make(0, 0)]) == Err(PC_ESTAGE),
        "static",
    );

    s.add(
        "PC13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(PC_EFROZEN)
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
    fn last_stage_wins_by_default() {
        assert_eq!(pipeline_status(&[0, 1, 0], false), Ok(0));
        assert_eq!(pipeline_status(&[0, 0, 1], false), Ok(1));
        assert_eq!(pipeline_status(&[2, 0, 0], false), Ok(0));
    }

    #[test]
    fn pipefail_takes_first_nonzero() {
        assert_eq!(pipeline_status(&[0, 1, 0], true), Ok(1));
        assert_eq!(pipeline_status(&[2, 1, 0], true), Ok(2));
        assert_eq!(pipeline_status(&[0, 0, 0], true), Ok(0));
    }

    #[test]
    fn chain_flows_three_stages() {
        let sc = std_chain();
        assert_eq!(stage_count(&sc), Ok(3));
        assert_eq!(chain(&sc, 1000), Ok(1));
        assert_eq!(chain(&sc, 0), Ok(0));
        assert_eq!(filter(1000, 250), Ok(250));
        assert_eq!(filter(1000, 1000), Ok(1000));
    }

    #[test]
    fn gate_order() {
        assert_eq!(pipeline_status(&[], false), Err(PC_EEMPTY));
        assert_eq!(pipeline_status(&[0, 0, 0, 0, 0, 0, 0], false), Err(PC_EMANY));
        assert_eq!(filter(u32::MAX, 1000), Err(PC_EOVER));
        assert_eq!(filter(1, 1001), Err(PC_EKEEP));
        assert_eq!(chain(&[Stage::make(9, 0)], 1), Err(PC_ESTAGE));
    }

    #[test]
    fn sigpipe_threshold() {
        assert_eq!(sigpipe(16, 16), 0);
        assert_eq!(sigpipe(17, 16), SIGPIPE_STATUS);
        assert_eq!(sigpipe(1, 0), SIGPIPE_STATUS);
        assert_eq!(SIGPIPE_STATUS, 141);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(PC_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11457_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
