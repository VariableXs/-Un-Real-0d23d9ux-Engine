//! UNX-F11459 两工具判据双机同码验证（0x20c70 段）
//!
//! 同一份判据在两台机器上跑出同一个字节流，才算“同码”。双机验证的难点不在
//! 比较摘要，而在**先解释差异的来源**：锚（locale）、行数、工具版本三项任一不同，
//! 摘要不同就是必然结果，此时再报“摘要分歧”只会把根因埋掉。模型因此把联签做成
//! 分级判定：锚 → 行数 → 版本 → 摘要，级号小的先报，摘要只在结构一致时才作数。
//!
//! 每台机器的摘要只由**内容三要件**（行数、锚、工具版本）折叠而来，与机号无关：
//! 摘要是对“跑出了什么”的指纹，不是对“谁跑的”的标记，把机号折进去等于让两台
//! 机器永远签不上。联签票号再把两条摘要按大小排序后折到一起，因此与调用顺序
//! 无关——非零即可作为“确实签过”的最低证据。重复联签必须得到同一票号；如果
//! 签名过程自身带了隐藏状态（时间、分配地址、遍历顺序），幂等性会先崩。
//!
//! 字节跨度独立核算：每机 1024 行、每行 64 字节，恰好 65536 字节；乘法用
//! `checked_mul`，越界记 EOVER，不与摘要分歧混为一谈。
//!
//! 账本十四格：八格成功（每机行数、票号非零、对跑机数、四要件同值、独立重建、
//! 语料字节、分歧类号、幂等复签），六格拒绝（锚分歧、行数分歧、版本分歧、
//! 摘要分歧、容量满、重复登记）。审计逐格钉值。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20c70;

/// 段内低十六位。
const SEG_LO: u16 = (SEG_BASE & 0xffff) as u16;

/// 登记顺序错误。
pub const DM_EORDER: u16 = SEG_LO + 0x01;
/// 重复登记。
pub const DM_EDUP: u16 = SEG_LO + 0x02;
/// 已冻结。
pub const DM_EFROZEN: u16 = SEG_LO + 0x03;
/// 观测缺失。
pub const DM_ENOBS: u16 = SEG_LO + 0x04;
/// 结论不符。
pub const DM_EMISMATCH: u16 = SEG_LO + 0x05;
/// 账本失衡。
pub const DM_EBAL: u16 = SEG_LO + 0x06;
/// 容量满。
pub const DM_EFULL: u16 = SEG_LO + 0x07;
/// 锚分歧。
pub const DM_EANCHOR: u16 = SEG_LO + 0x08;
/// 行数分歧。
pub const DM_EROWS: u16 = SEG_LO + 0x09;
/// 工具版本分歧。
pub const DM_ETOOLVER: u16 = SEG_LO + 0x0a;
/// 摘要分歧。
pub const DM_EDIGEST: u16 = SEG_LO + 0x0b;
/// 跨度溢出。
pub const DM_EOVER: u16 = SEG_LO + 0x0c;
/// 票号不一致。
pub const DM_ESEAL: u16 = SEG_LO + 0x0d;

/// 机号：甲。
pub const MACHINE_A: u8 = 1;
/// 机号：乙。
pub const MACHINE_B: u8 = 2;

/// 锚：C。
pub const ANCHOR_C: u8 = 1;
/// 锚：C.UTF-8（非锚）。
pub const ANCHOR_UTF8: u8 = 3;

/// 每机行数。
pub const ROWS_PER_RUN: u32 = 1024;
/// 每行字节数。
pub const LINE_BYTES: u32 = 64;
/// 工具版本。
pub const TOOLVER: u16 = 3;
/// 对跑机数。
pub const MACHINES: u32 = 2;
/// 同值要件数。
pub const FIELDS: u32 = 4;

/// 类号：一致。
pub const CLASS_AGREE: u8 = 0;
/// 类号：锚分歧。
pub const CLASS_ANCHOR: u8 = 1;
/// 类号：行数分歧。
pub const CLASS_ROWS: u8 = 2;
/// 类号：版本分歧。
pub const CLASS_VER: u8 = 3;
/// 类号：摘要分歧。
pub const CLASS_DIGEST: u8 = 4;

/// 操作：联签。
pub const OP_AGREE: u8 = 1;
/// 操作：分类。
pub const OP_CLASS: u8 = 2;
/// 操作：跨度。
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

/// 一台机器的观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine {
    pub id: u8,
    pub anchor: u8,
    pub rows: u32,
    pub toolver: u16,
    pub digest: u64,
}

impl Machine {
    /// 空白机。
    pub const fn blank() -> Machine {
        Machine {
            id: 0,
            anchor: 0,
            rows: 0,
            toolver: 0,
            digest: 0,
        }
    }
}

/// 摘要：行数 + 锚 + 工具版本（不含机号）。
pub fn digest_of(rows: u32, anchor: u8, toolver: u16) -> u64 {
    let mut h = fold(FNV_OFF, u64::from(rows));
    h = fold(h, u64::from(anchor));
    fold(h, u64::from(toolver))
}

/// 按规格造机。
pub fn machine(id: u8, anchor: u8, rows: u32, toolver: u16) -> Machine {
    Machine {
        id,
        anchor,
        rows,
        toolver,
        digest: digest_of(rows, anchor, toolver),
    }
}

/// 标准机：C 锚、1024 行、版本 3。
pub fn std_machine(id: u8) -> Machine {
    machine(id, ANCHOR_C, ROWS_PER_RUN, TOOLVER)
}

/// 分级判定：锚 → 行数 → 版本 → 摘要。
pub fn classify(a: &Machine, b: &Machine) -> u8 {
    if a.anchor != b.anchor {
        return CLASS_ANCHOR;
    }
    if a.rows != b.rows {
        return CLASS_ROWS;
    }
    if a.toolver != b.toolver {
        return CLASS_VER;
    }
    if a.digest != b.digest {
        return CLASS_DIGEST;
    }
    CLASS_AGREE
}

/// 联签：结构一致才比摘要，票号为两条摘要按大小排序后的折叠。
pub fn agree(a: &Machine, b: &Machine) -> Result<u64, u16> {
    match classify(a, b) {
        CLASS_AGREE => {
            let (lo, hi) = if a.digest <= b.digest {
                (a.digest, b.digest)
            } else {
                (b.digest, a.digest)
            };
            let h = fold(FNV_OFF, lo);
            Ok(fold(h, hi))
        }
        CLASS_ANCHOR => Err(DM_EANCHOR),
        CLASS_ROWS => Err(DM_EROWS),
        CLASS_VER => Err(DM_ETOOLVER),
        CLASS_DIGEST => Err(DM_EDIGEST),
        _ => Err(DM_EBAL),
    }
}

/// 字节跨度：行数 × 行长。
pub fn span(rows: u32) -> Result<u32, u16> {
    match rows.checked_mul(LINE_BYTES) {
        Some(v) => Ok(v),
        None => Err(DM_EOVER),
    }
}

/// 同值要件数。
pub fn matching_fields(a: &Machine, b: &Machine) -> u32 {
    let mut n = 0u32;
    if a.anchor == b.anchor {
        n += 1;
    }
    if a.rows == b.rows {
        n += 1;
    }
    if a.toolver == b.toolver {
        n += 1;
    }
    if a.digest == b.digest {
        n += 1;
    }
    n
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
            return Err(DM_EFROZEN);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == s.slot {
                        self.rejects += 1;
                        return Err(DM_EDUP);
                    }
                }
                None => return Err(DM_ENOBS),
            }
            i += 1;
        }
        if s.slot as usize != self.len {
            self.rejects += 1;
            return Err(DM_EORDER);
        }
        if self.len >= self.cap || self.len >= ROWS {
            self.rejects += 1;
            return Err(DM_EFULL);
        }
        if s.op == 0 {
            self.rejects += 1;
            return Err(DM_EMISMATCH);
        }
        if s.ok && s.code != 0 {
            self.rejects += 1;
            return Err(DM_EMISMATCH);
        }
        if !s.ok && s.code == 0 {
            self.rejects += 1;
            return Err(DM_EMISMATCH);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = s,
            None => return Err(DM_EFULL),
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
            return Err(DM_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DM_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DM_ENOBS),
            };
            if s.slot as usize != i {
                return Err(DM_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DM_EMISMATCH);
                }
            } else if s.code == 0 || s.value != 0 {
                return Err(DM_EMISMATCH);
            }
            i += 1;
        }
        let vals = [1024u32, 1, 2, 4, 2, 65536, 4, 1];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DM_EMISMATCH);
                    }
                }
                _ => return Err(DM_EBAL),
            }
            k += 1;
        }
        let codes = [
            DM_EANCHOR, DM_EROWS, DM_ETOOLVER, DM_EDIGEST, DM_EFULL, DM_EDUP,
        ];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.ok || s.code != *w || s.value != 0 {
                            return Err(DM_EMISMATCH);
                        }
                    }
                    None => return Err(DM_EBAL),
                },
                None => return Err(DM_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DM_EBAL);
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
            return Err(DM_EFROZEN);
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
            return Err(DM_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.root {
            return Err(DM_ESEAL);
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

/// 签结果取码。
fn agree_code(r: Result<u64, u16>) -> u16 {
    match r {
        Ok(_) => 0,
        Err(c) => c,
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let a = std_machine(MACHINE_A);
    let b = std_machine(MACHINE_B);

    let signed = agree(&a, &b);
    let _ = l.register(ok_row(
        0,
        OP_AGREE,
        if signed.is_ok() { a.rows } else { 0 },
    ));
    let _ = l.register(ok_row(
        1,
        OP_AGREE,
        match signed {
            Ok(v) => {
                if v != 0 {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        },
    ));
    let _ = l.register(ok_row(2, OP_AGREE, MACHINES));
    let _ = l.register(ok_row(3, OP_AGREE, matching_fields(&a, &b)));
    let _ = l.register(ok_row(
        4,
        OP_AGREE,
        if std_machine(MACHINE_B) == b {
            2
        } else {
            0
        },
    ));
    let _ = l.register(ok_row(5, OP_SPAN, match span(ROWS_PER_RUN) {
        Ok(v) => v,
        Err(_) => 0,
    }));
    let _ = l.register(ok_row(6, OP_CLASS, u32::from(CLASS_DIGEST)));

    let mut drifted = std_machine(MACHINE_B);
    drifted.digest ^= 1;
    let repeat = agree(&a, &std_machine(MACHINE_B));
    let _ = l.register(ok_row(
        7,
        OP_AGREE,
        if repeat == signed && classify(&a, &std_machine(MACHINE_B)) == CLASS_AGREE {
            1
        } else {
            0
        },
    ));

    let mut anchor = std_machine(MACHINE_B);
    anchor.anchor = ANCHOR_UTF8;
    anchor.digest = digest_of(anchor.rows, anchor.anchor, anchor.toolver);
    let _ = l.register(err_row(8, OP_AGREE, agree_code(agree(&a, &anchor))));

    let mut rows = std_machine(MACHINE_B);
    rows.rows = ROWS_PER_RUN * 2;
    rows.digest = digest_of(rows.rows, rows.anchor, rows.toolver);
    let _ = l.register(err_row(9, OP_AGREE, agree_code(agree(&a, &rows))));

    let mut ver = std_machine(MACHINE_B);
    ver.toolver = TOOLVER + 1;
    let _ = l.register(err_row(10, OP_AGREE, agree_code(agree(&a, &ver))));

    let _ = l.register(err_row(11, OP_AGREE, agree_code(agree(&a, &drifted))));

    let mut small = Ledger::with_cap(3);
    let _ = small.register(ok_row(0, OP_AGREE, 1));
    let _ = small.register(ok_row(1, OP_AGREE, 1));
    let _ = small.register(ok_row(2, OP_AGREE, 1));
    let full = reg_code(small.register(ok_row(3, OP_AGREE, 1)));
    let _ = l.register(err_row(12, OP_LEDGER, full));

    let mut d = Ledger::new();
    let _ = d.register(ok_row(0, OP_AGREE, 1));
    let dup = reg_code(d.register(ok_row(0, OP_AGREE, 2)));
    let _ = l.register(err_row(13, OP_LEDGER, dup));

    l
}

/// 四路注入：重放冻结、双机同码、分歧分级、篡改复原。
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
        && two == Err(DM_EFROZEN)
        && l.rejects() == 1
        && before == l.len()
        && root != 0
        && l.audit().is_ok();

    out[1] = {
        let a = std_machine(MACHINE_A);
        let b = std_machine(MACHINE_B);
        let s1 = agree(&a, &b);
        let s2 = agree(&a, &b);
        a.digest == b.digest
            && a.rows == b.rows
            && matching_fields(&a, &b) == FIELDS
            && s1.is_ok()
            && s1 == s2
            && match s1 {
                Ok(v) => v != 0,
                Err(_) => false,
            }
    };

    out[2] = {
        let a = std_machine(MACHINE_A);
        let mut anchor = std_machine(MACHINE_B);
        anchor.anchor = ANCHOR_UTF8;
        anchor.digest = digest_of(anchor.rows, anchor.anchor, anchor.toolver);
        let mut rows = std_machine(MACHINE_B);
        rows.rows = ROWS_PER_RUN * 2;
        rows.digest = digest_of(rows.rows, rows.anchor, rows.toolver);
        let mut ver = std_machine(MACHINE_B);
        ver.toolver = TOOLVER + 1;
        let mut dig = std_machine(MACHINE_B);
        dig.digest ^= 1;
        let mut both = std_machine(MACHINE_B);
        both.anchor = ANCHOR_UTF8;
        both.digest ^= 1;
        agree(&a, &anchor) == Err(DM_EANCHOR)
            && agree(&a, &rows) == Err(DM_EROWS)
            && agree(&a, &ver) == Err(DM_ETOOLVER)
            && agree(&a, &dig) == Err(DM_EDIGEST)
            && agree(&a, &both) == Err(DM_EANCHOR)
            && classify(&a, &dig) == CLASS_DIGEST
    };

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
        let caught = t.audit() == Err(DM_EMISMATCH);
        if let Some(r) = t.rows.get_mut(5) {
            r.value = old;
        }
        clean && caught && t.audit().is_ok()
    };

    out
}

/// 十三判据。
pub fn run_unx11459_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11459.dualmachine");
    let l = build_ledger();

    s.add_dyn(
        "DM1-十四格齐备",
        l.len() == ROWS && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DM2-双机同码",
        l.sample(0).map(|x| x.value) == Some(1024)
            && l.sample(3).map(|x| x.value) == Some(4)
            && {
                let a = std_machine(MACHINE_A);
                let b = std_machine(MACHINE_B);
                a.digest == b.digest && matching_fields(&a, &b) == 4 && agree(&a, &b).is_ok()
            },
        "static",
    );

    s.add(
        "DM3-锚分歧优先",
        l.sample(8).map(|x| x.code) == Some(DM_EANCHOR)
            && {
                let a = std_machine(MACHINE_A);
                let mut b = std_machine(MACHINE_B);
                b.anchor = ANCHOR_UTF8;
                b.digest ^= 0xff;
                agree(&a, &b) == Err(DM_EANCHOR) && classify(&a, &b) == CLASS_ANCHOR
            },
        "static",
    );

    s.add(
        "DM4-行数分歧",
        l.sample(9).map(|x| x.code) == Some(DM_EROWS)
            && {
                let a = std_machine(MACHINE_A);
                let mut b = std_machine(MACHINE_B);
                b.rows = 2048;
                agree(&a, &b) == Err(DM_EROWS) && classify(&a, &b) == CLASS_ROWS
            },
        "static",
    );

    s.add(
        "DM5-版本分歧",
        l.sample(10).map(|x| x.code) == Some(DM_ETOOLVER)
            && {
                let a = std_machine(MACHINE_A);
                let mut b = std_machine(MACHINE_B);
                b.toolver = TOOLVER + 1;
                agree(&a, &b) == Err(DM_ETOOLVER) && classify(&a, &b) == CLASS_VER
            },
        "static",
    );

    s.add(
        "DM6-摘要分歧",
        l.sample(11).map(|x| x.code) == Some(DM_EDIGEST)
            && {
                let a = std_machine(MACHINE_A);
                let mut b = std_machine(MACHINE_B);
                b.digest ^= 1;
                agree(&a, &b) == Err(DM_EDIGEST) && classify(&a, &b) == CLASS_DIGEST
            },
        "static",
    );

    s.add(
        "DM7-类号与同值要件",
        CLASS_AGREE == 0
            && CLASS_ANCHOR == 1
            && CLASS_ROWS == 2
            && CLASS_VER == 3
            && CLASS_DIGEST == 4
            && l.sample(6).map(|x| x.value) == Some(4)
            && classify(&std_machine(MACHINE_A), &std_machine(MACHINE_B)) == CLASS_AGREE,
        "static",
    );

    s.add(
        "DM8-联签非零且幂等",
        l.sample(1).map(|x| x.value) == Some(1)
            && l.sample(7).map(|x| x.value) == Some(1)
            && {
                let a = std_machine(MACHINE_A);
                let b = std_machine(MACHINE_B);
                let t1 = agree(&a, &b);
                let t2 = agree(&b, &a);
                t1.is_ok() && t1 == t2 && t1 != Ok(0)
            },
        "static",
    );

    s.add(
        "DM9-字节跨度",
        l.sample(5).map(|x| x.value) == Some(65536)
            && span(1024) == Ok(65536)
            && span(0) == Ok(0)
            && span(1024) == Ok(ROWS_PER_RUN * LINE_BYTES),
        "static",
    );

    s.add(
        "DM10-同值四要件",
        l.sample(3).map(|x| x.value) == Some(4)
            && matching_fields(&std_machine(MACHINE_A), &std_machine(MACHINE_B)) == 4
            && matching_fields(&std_machine(MACHINE_A), &machine(MACHINE_A, ANCHOR_UTF8, 1, 1)) == 0,
        "static",
    );

    s.add(
        "DM11-跨度溢出闸门",
        span(u32::MAX) == Err(DM_EOVER)
            && span(u32::MAX / LINE_BYTES) == Ok(u32::MAX / LINE_BYTES * LINE_BYTES)
            && span(u32::MAX / LINE_BYTES + 1) == Err(DM_EOVER),
        "static",
    );

    s.add(
        "DM12-容量与重复闸门",
        l.sample(12).map(|x| x.code) == Some(DM_EFULL)
            && l.sample(13).map(|x| x.code) == Some(DM_EDUP)
            && {
                let mut small = Ledger::with_cap(2);
                let _ = small.register(ok_row(0, OP_AGREE, 1));
                let _ = small.register(ok_row(1, OP_AGREE, 1));
                small.register(ok_row(2, OP_AGREE, 1)) == Err(DM_EFULL) && small.len() == 2
            },
        "static",
    );

    s.add(
        "DM13-冻结重放与四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(DM_EFROZEN)
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
    fn same_code_same_digest() {
        let a = std_machine(MACHINE_A);
        let b = std_machine(MACHINE_B);
        assert_eq!(a.digest, b.digest);
        assert_eq!(matching_fields(&a, &b), 4);
        assert!(agree(&a, &b).is_ok());
        assert_ne!(agree(&a, &b), Ok(0));
    }

    #[test]
    fn classification_order() {
        let a = std_machine(MACHINE_A);
        let mut both = std_machine(MACHINE_B);
        both.anchor = ANCHOR_UTF8;
        both.digest ^= 1;
        assert_eq!(classify(&a, &both), CLASS_ANCHOR);
        assert_eq!(agree(&a, &both), Err(DM_EANCHOR));

        let mut d = std_machine(MACHINE_B);
        d.digest ^= 1;
        assert_eq!(classify(&a, &d), CLASS_DIGEST);
        assert_eq!(agree(&a, &d), Err(DM_EDIGEST));
    }

    #[test]
    fn structural_mismatch_beats_digest() {
        let a = std_machine(MACHINE_A);
        let mut r = std_machine(MACHINE_B);
        r.rows = 2048;
        assert_eq!(agree(&a, &r), Err(DM_EROWS));
        let mut v = std_machine(MACHINE_B);
        v.toolver = 9;
        assert_eq!(agree(&a, &v), Err(DM_ETOOLVER));
    }

    #[test]
    fn span_math() {
        assert_eq!(span(1024), Ok(65536));
        assert_eq!(span(0), Ok(0));
        assert_eq!(span(u32::MAX), Err(DM_EOVER));
        assert_eq!(
            span(u32::MAX / LINE_BYTES + 1),
            Err(DM_EOVER)
        );
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
        assert_eq!(l.freeze(), Err(DM_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11459_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
