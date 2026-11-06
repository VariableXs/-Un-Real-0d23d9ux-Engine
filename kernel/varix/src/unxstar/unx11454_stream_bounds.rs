//! UNX-F11454 `sort`/`grep` 大件流式边界（0x20c20 段）
//!
//! 两个工具都宣称“流式”，但流式不是无界：`sort` 必须把输入切成能装进内存的
//! 运行段（run）落盘再归并，`grep` 按块读取并对超长行单独处置。边界一旦算错，
//! 症状不是报错而是**悄悄多切一段**或**少读一行**——统计数字仍然自洽，只有把
//! 段数、尾段大小、归并趟数放在一起核对才会露馅。
//!
//! 模型把边界拆成四个独立函数：`runs` 按容量切段（records/cap 向上取整，零记录
//! 返回零段），`tail_records` 给出尾段条数，`merge_passes` 按扇入求归并趟数
//! （扇入必须 ≥ 2，趟数上限 8，超过记 EDEPTH），`span_bytes` 求字节跨度
//! （`checked_mul`，溢出记 EOVER）。闸门次序是：零容量（ECAP）→ 硬上限
//! （ECOUNT）→ 扇入（EFAN）→ 深度（EDEPTH）→ 算术（EOVER）。同一函数里两个
//! 错误同时成立时，先声明的那个说了算，审计因此只有一个正确答案。
//!
//! 高水位（chunk + 行容量）单独记账：它既是缓冲区上界，也是“一行最长能有多长”
//! 的显式预算。尾段条数恒小于容量，这条不变量与段数一起构成切分正确性的充分
//! 条件——只验段数不验尾段，会在整除边界上放过把尾段算成满段的实现。
//!
//! 账本十四格：八格成功（千条四段、整百一段、零条零段、单趟归并、两趟归并、
//! 字节跨度、尾段条数、高水位），六格拒绝（零容量、超硬上限、扇入非法、深度
//! 超限、字节溢出、容量满）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c20;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const SB_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const SB_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const SB_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const SB_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const SB_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const SB_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const SB_EFULL: u16 = SEG_LO + 0x07;
/// 容量或行长非法。
pub const SB_ECAP: u16 = SEG_LO + 0x08;
/// 记录数超硬上限。
pub const SB_ECOUNT: u16 = SEG_LO + 0x09;
/// 扇入非法。
pub const SB_EFAN: u16 = SEG_LO + 0x0a;
/// 归并深度超限。
pub const SB_EDEPTH: u16 = SEG_LO + 0x0b;
/// 算术溢出。
pub const SB_EOVER: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const SB_ESEAL: u16 = SEG_LO + 0x0d;

/// 单行容量。
pub const LINE_CAP: u32 = 256;
/// 单个运行段条数。
pub const REC_CAP: u32 = 300;
/// 归并扇入。
pub const FAN_IN: u32 = 4;
/// 读块字节数。
pub const CHUNK: u32 = 4096;
/// 记录数硬上限。
pub const HARD_RECORDS: u32 = 1_000_000;
/// 归并趟数上限。
pub const MAX_PASSES: u32 = 8;

/// 操作：切段。
pub const OP_RUNS: u8 = 1;
/// 操作：归并。
pub const OP_MERGE: u8 = 2;
/// 操作：字节跨度。
pub const OP_SPAN: u8 = 3;
/// 操作：账本自检。
pub const OP_LEDGER: u8 = 4;

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

/// 切段：零记录零段，余数单独成段。
pub fn runs(records: u32, cap: u32) -> Result<u32, u16> {
    if cap == 0 {
        return Err(SB_ECAP);
    }
    if records > HARD_RECORDS {
        return Err(SB_ECOUNT);
    }
    if records == 0 {
        return Ok(0);
    }
    let full = records / cap;
    let rem = records % cap;
    if rem == 0 {
        Ok(full)
    } else {
        match full.checked_add(1) {
            Some(v) => Ok(v),
            None => Err(SB_EOVER),
        }
    }
}

/// 尾段条数：整除时为零。
pub fn tail_records(records: u32, cap: u32) -> Result<u32, u16> {
    if cap == 0 {
        return Err(SB_ECAP);
    }
    if records > HARD_RECORDS {
        return Err(SB_ECOUNT);
    }
    Ok(records % cap)
}

/// 归并趟数：每趟把段数除以扇入并向上取整。
pub fn merge_passes(mut runs: u32, fan: u32) -> Result<u32, u16> {
    if fan < 2 {
        return Err(SB_EFAN);
    }
    if runs == 0 {
        return Ok(0);
    }
    let mut p = 0u32;
    while runs > 1 {
        if p >= MAX_PASSES {
            return Err(SB_EDEPTH);
        }
        let q = runs / fan;
        let r = runs % fan;
        runs = if r == 0 { q } else { q + 1 };
        p += 1;
    }
    Ok(p)
}

/// 字节跨度：`checked_mul`，行长必须在容量内。
pub fn span_bytes(records: u32, line: u32) -> Result<u32, u16> {
    if line == 0 || line > LINE_CAP {
        return Err(SB_ECAP);
    }
    match records.checked_mul(line) {
        Some(v) => Ok(v),
        None => Err(SB_EOVER),
    }
}

/// 缓冲高水位：读块加一行。
pub fn high_water(chunk: u32, line: u32) -> u32 {
    chunk.saturating_add(line)
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
            return Err(SB_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(SB_EDUP);
                    }
                }
                None => return Err(SB_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(SB_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(SB_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(SB_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(SB_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(SB_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(SB_EFULL),
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
            return Err(SB_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SB_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SB_ENOBS),
            };
            if s.slot as usize != i {
                return Err(SB_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SB_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(SB_EMISMATCH);
            }
            i += 1;
        }
        let vals = [4u32, 1, 0, 1, 2, 12000, 100, 4352];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(SB_EMISMATCH);
                    }
                }
                _ => return Err(SB_EBAL),
            }
            k += 1;
        }
        let codes = [
            SB_ECAP, SB_ECOUNT, SB_EFAN, SB_EDEPTH, SB_EOVER, SB_EFULL,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(SB_EMISMATCH);
                        }
                    }
                    None => return Err(SB_EBAL),
                },
                None => return Err(SB_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(SB_EBAL);
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
            return Err(SB_EFROZEN);
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
            return Err(SB_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(SB_ESEAL);
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

/// 结果取数值。
fn value_of(r: Result<u32, u16>) -> u32 {
    match r {
        Ok(v) => v,
        Err(_) => 0xffff_ffff,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(ok_row(0, OP_RUNS, value_of(runs(1000, REC_CAP))));
    let _ = l.register(ok_row(1, OP_RUNS, value_of(runs(300, REC_CAP))));
    let _ = l.register(ok_row(2, OP_RUNS, value_of(runs(0, REC_CAP))));

    let _ = l.register(ok_row(3, OP_MERGE, value_of(merge_passes(4, FAN_IN))));
    let _ = l.register(ok_row(4, OP_MERGE, value_of(merge_passes(16, FAN_IN))));

    let _ = l.register(ok_row(5, OP_SPAN, value_of(span_bytes(1000, 12))));
    let _ = l.register(ok_row(6, OP_RUNS, value_of(tail_records(1000, REC_CAP))));
    let _ = l.register(ok_row(7, OP_SPAN, high_water(CHUNK, LINE_CAP)));

    let zero = match runs(1000, 0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(8, OP_RUNS, zero));

    let many = match runs(HARD_RECORDS + 1, REC_CAP) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(9, OP_RUNS, many));

    let fan = match merge_passes(8, 1) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(10, OP_MERGE, fan));

    let depth = match merge_passes(u32::MAX, 2) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(11, OP_MERGE, depth));

    let over = match span_bytes(u32::MAX, 2) {
        Ok(_) => 0,
        Err(c) => c,
    };
    let _ = l.register(err_row(12, OP_SPAN, over));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_RUNS, 1));
    let _ = small.register(ok_row(1, OP_RUNS, 1));
    let _ = small.register(ok_row(2, OP_RUNS, 1));
    let full = reg_code(small.register(ok_row(3, OP_RUNS, 1)));
    let _ = l.register(err_row(13, OP_LEDGER, full));

    l
}

/// 四路注入：重放冻结、切分不变量、归并最小性、篡改复原。
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
        && two == Err(SB_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = match (runs(1000, 300), tail_records(1000, 300)) {
        (Ok(r), Ok(t)) => {
            r == 4
                && t == 100
                && t < 300
                && r * 300 >= 1000
                && (r - 1) * 300 < 1000
                && runs(299, 300) == Ok(1)
                && runs(301, 300) == Ok(2)
        }
        _ => false,
    };

    out[2] = merge_passes(4, 4) == Ok(1)
        && merge_passes(16, 4) == Ok(2)
        && merge_passes(17, 4) == Ok(3)
        && merge_passes(64, 4) == Ok(3)
        && merge_passes(1, 4) == Ok(0)
        && merge_passes(0, 4) == Ok(0);

    out[3] = {
        let mut t = build_ledger();
        let clean = t.audit().is_ok();
        let old = match t.sample(5) {
            Some(s) => s.value,
            None => 0,
        };
        if let Some(r) = t.rows.get_mut(5) {
            r.value = old.saturating_add(1);
        }
        let caught = t.audit() == Err(SB_EMISMATCH);
        if let Some(r) = t.rows.get_mut(5) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11454_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11454.streambounds");
    let l = build_ledger();

    s.add_dyn(
        "SB1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SB2-千条切成四段",
        l.sample(0).map(|x| x.value) == Some(4)
            && runs(1000, 300) == Ok(4)
            && tail_records(1000, 300) == Ok(100),
        "static",
    );

    s.add(
        "SB3-整百一段",
        l.sample(1).map(|x| x.value) == Some(1)
            && runs(300, 300) == Ok(1)
            && tail_records(300, 300) == Ok(0)
            && runs(299, 300) == Ok(1),
        "static",
    );

    s.add(
        "SB4-零条零段",
        l.sample(2).map(|x| x.value) == Some(0)
            && runs(0, 300) == Ok(0)
            && tail_records(0, 300) == Ok(0)
            && merge_passes(0, 4) == Ok(0),
        "static",
    );

    s.add(
        "SB5-单趟归并",
        l.sample(3).map(|x| x.value) == Some(1)
            && merge_passes(4, 4) == Ok(1)
            && merge_passes(2, 4) == Ok(1)
            && merge_passes(1, 4) == Ok(0),
        "static",
    );

    s.add(
        "SB6-两趟归并",
        l.sample(4).map(|x| x.value) == Some(2)
            && merge_passes(16, 4) == Ok(2)
            && merge_passes(5, 4) == Ok(2)
            && merge_passes(17, 4) == Ok(3),
        "static",
    );

    s.add(
        "SB7-字节跨度",
        l.sample(5).map(|x| x.value) == Some(12000)
            && span_bytes(1000, 12) == Ok(12000)
            && span_bytes(0, 12) == Ok(0)
            && span_bytes(256, 256) == Ok(65536),
        "static",
    );

    s.add(
        "SB8-尾段条数",
        l.sample(6).map(|x| x.value) == Some(100)
            && tail_records(1000, 300) == Ok(100)
            && tail_records(301, 300) == Ok(1)
            && tail_records(299, 300) == Ok(299),
        "static",
    );

    s.add(
        "SB9-高水位",
        l.sample(7).map(|x| x.value) == Some(4352)
            && high_water(CHUNK, LINE_CAP) == 4352
            && high_water(CHUNK, LINE_CAP) > CHUNK
            && high_water(u32::MAX, 1) == u32::MAX,
        "static",
    );

    s.add(
        "SB10-零容量闸门",
        l.sample(8).map(|x| x.code) == Some(SB_ECAP)
            && runs(10, 0) == Err(SB_ECAP)
            && tail_records(10, 0) == Err(SB_ECAP)
            && span_bytes(10, 0) == Err(SB_ECAP)
            && span_bytes(10, LINE_CAP + 1) == Err(SB_ECAP),
        "static",
    );

    s.add(
        "SB11-硬上限与扇入",
        l.sample(9).map(|x| x.code) == Some(SB_ECOUNT)
            && runs(HARD_RECORDS + 1, REC_CAP) == Err(SB_ECOUNT)
            && runs(HARD_RECORDS, REC_CAP).is_ok()
            && l.sample(10).map(|x| x.code) == Some(SB_EFAN)
            && merge_passes(8, 0) == Err(SB_EFAN)
            && merge_passes(8, 1) == Err(SB_EFAN),
        "static",
    );

    s.add(
        "SB12-深度与溢出闸门",
        l.sample(11).map(|x| x.code) == Some(SB_EDEPTH)
            && l.sample(12).map(|x| x.code) == Some(SB_EOVER)
            && merge_passes(u32::MAX, 2) == Err(SB_EDEPTH)
            && span_bytes(u32::MAX, 2) == Err(SB_EOVER)
            && span_bytes(u32::MAX / 2, 2) == Ok(u32::MAX - 1),
        "static",
    );

    s.add(
        "SB13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(SB_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && f.sample(13).map(|x| x.code) == Some(SB_EFULL)
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
    fn spill_runs_round_up() {
        assert_eq!(runs(1000, 300), Ok(4));
        assert_eq!(runs(300, 300), Ok(1));
        assert_eq!(runs(301, 300), Ok(2));
        assert_eq!(runs(0, 300), Ok(0));
        assert_eq!(runs(1, 300), Ok(1));
    }

    #[test]
    fn tail_is_remainder() {
        assert_eq!(tail_records(1000, 300), Ok(100));
        assert_eq!(tail_records(300, 300), Ok(0));
        assert_eq!(tail_records(299, 300), Ok(299));
    }

    #[test]
    fn merge_passes_are_minimal() {
        assert_eq!(merge_passes(1, 4), Ok(0));
        assert_eq!(merge_passes(4, 4), Ok(1));
        assert_eq!(merge_passes(5, 4), Ok(2));
        assert_eq!(merge_passes(16, 4), Ok(2));
        assert_eq!(merge_passes(17, 4), Ok(3));
        assert_eq!(merge_passes(u32::MAX, 2), Err(SB_EDEPTH));
    }

    #[test]
    fn span_and_caps() {
        assert_eq!(span_bytes(1000, 12), Ok(12000));
        assert_eq!(span_bytes(0, 12), Ok(0));
        assert_eq!(span_bytes(u32::MAX, 2), Err(SB_EOVER));
        assert_eq!(span_bytes(1, 0), Err(SB_ECAP));
        assert_eq!(runs(1, 0), Err(SB_ECAP));
        assert_eq!(runs(HARD_RECORDS + 1, 300), Err(SB_ECOUNT));
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
        assert_eq!(l.freeze(), Err(SB_EFROZEN));
        assert!(l.is_frozen());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11454_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
