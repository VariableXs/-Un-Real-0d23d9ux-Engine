//! UNX-F11386 ls 退出码与错误输出
//!
//! ls 的退出码只看**诊断类别**：无诊断 0；只有次要问题（权限提示、编码提示）
//! 1；出现严重问题（无法访问、读取失败）2——部分失败与全失败都是 2，不按
//! 条数加权。stderr 文本按类别前缀 + 操作数字节计量，诊断按类别升序登记。
//!
//! 闸门次序：
//! - `fault`：未知类别 → 空操作数 → 操作数超长 → 类别倒序 → 重复诊断 →
//!   诊断表满；
//! - `status`：诊断表越界（防御）；
//! - `stderr_len`：无诊断不可量文本 → 文本超限。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee20;

/// 未知诊断类别。
pub const LSX_EKIND: u32 = SEG_BASE + 0x01;
/// 空操作数。
pub const LSX_EOPND: u32 = SEG_BASE + 0x02;
/// 操作数超长。
pub const LSX_ERANGE: u32 = SEG_BASE + 0x03;
/// 类别倒序。
pub const LSX_EORDER: u32 = SEG_BASE + 0x04;
/// 重复诊断。
pub const LSX_EDUP: u32 = SEG_BASE + 0x05;
/// 诊断表满。
pub const LSX_EFULL: u32 = SEG_BASE + 0x06;
/// 无诊断可量。
pub const LSX_ENONE: u32 = SEG_BASE + 0x07;
/// stderr 文本超限。
pub const LSX_ETEXT: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSX_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSX_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSX_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSX_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSX_ENOBS: u32 = SEG_BASE + 0x0d;

/// 诊断表容量。
pub const CAP: usize = 8;
/// 操作数字节上限。
pub const OP_MAX: usize = 24;
/// stderr 文本字节上限。
pub const TEXT_CAP: u32 = 200;
/// 每条诊断的类别前缀字节数。
pub const PREFIX: u32 = 12;
/// 无法访问。
pub const KIND_ACCESS: u8 = 1;
/// 读取失败。
pub const KIND_READ: u8 = 2;
/// 权限提示。
pub const KIND_PERM: u8 = 3;
/// 编码提示。
pub const KIND_ENC: u8 = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：登记诊断。
pub const OP_FAULT: u8 = 1;
/// 操作码：取退出码。
pub const OP_STATUS: u8 = 2;
/// 操作码：量 stderr。
pub const OP_TEXT: u8 = 3;
/// 操作码：开户。
pub const OP_OPEN: u8 = 4;

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

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    /// 类别。
    pub kind: u8,
    /// 操作数字节数。
    pub olen: u32,
}

impl Fault {
    /// 空白。
    pub const fn blank() -> Fault {
        Fault { kind: 0, olen: 0 }
    }
}

/// 一次运行的诊断账。
#[derive(Clone, Copy, Debug)]
pub struct Run {
    f: [Fault; CAP],
    n: usize,
    last: u8,
}

impl Run {
    /// 空运行（无诊断）。
    pub const fn new() -> Run {
        Run { f: [Fault::blank(); CAP], n: 0, last: 0 }
    }

    /// 诊断条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条诊断。
    pub fn at(&self, i: usize) -> Option<Fault> {
        if i >= self.n {
            return None;
        }
        self.f.get(i).copied()
    }

    /// 登记一条诊断，返回累计条数。
    ///
    /// 闸门次序：类别 → 空操作数 → 操作数超长 → 类别倒序 → 重复 → 表满。
    pub fn fault(&mut self, kind: u8, olen: u32) -> Result<usize, u32> {
        if kind < KIND_ACCESS || kind > KIND_ENC {
            return Err(LSX_EKIND);
        }
        if olen == 0 {
            return Err(LSX_EOPND);
        }
        if olen as usize > OP_MAX {
            return Err(LSX_ERANGE);
        }
        if self.last != 0 && kind < self.last {
            return Err(LSX_EORDER);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.f.get(i) {
                Some(v) => {
                    if v.kind == kind && v.olen == olen {
                        return Err(LSX_EDUP);
                    }
                }
                None => return Err(LSX_EFULL),
            }
            i += 1;
        }
        if self.n >= CAP {
            return Err(LSX_EFULL);
        }
        if let Some(slot) = self.f.get_mut(self.n) {
            *slot = Fault { kind, olen };
        }
        self.n += 1;
        self.last = kind;
        Ok(self.n)
    }

    /// 退出码：无诊断 0，仅次要 1，有严重 2。
    ///
    /// 闸门：诊断表越界（防御）。
    pub fn status(&self) -> Result<u32, u32> {
        if self.n > CAP {
            return Err(LSX_EORDER);
        }
        if self.n == 0 {
            return Ok(0);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.f.get(i) {
                Some(v) => {
                    if v.kind == KIND_ACCESS || v.kind == KIND_READ {
                        return Ok(2);
                    }
                }
                None => return Err(LSX_EORDER),
            }
            i += 1;
        }
        Ok(1)
    }

    /// stderr 文本字节数。
    ///
    /// 闸门次序：无诊断 → 文本超限。
    pub fn stderr_len(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(LSX_ENONE);
        }
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let v = match self.f.get(i) {
                Some(x) => *x,
                None => return Err(LSX_ENONE),
            };
            total = total.saturating_add(PREFIX).saturating_add(v.olen);
            i += 1;
        }
        if total > TEXT_CAP {
            return Err(LSX_ETEXT);
        }
        Ok(total)
    }

    /// 最严重类别（0 表示无诊断）。
    pub fn worst(&self) -> u8 {
        let mut w = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.f.get(i) {
                if w == 0 || v.kind < w {
                    w = v.kind;
                }
            }
            i += 1;
        }
        w
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.f.get(i) {
                h = fold(h, v.kind as u64);
                h = fold(h, v.olen as u64);
            }
            i += 1;
        }
        h = fold(h, self.n as u64);
        fold(h, self.last as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        let mut prev = 0u8;
        while i < self.n {
            let v = match self.f.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if v.kind < KIND_ACCESS || v.kind > KIND_ENC {
                return false;
            }
            if v.olen == 0 || v.olen as usize > OP_MAX {
                return false;
            }
            if prev != 0 && v.kind < prev {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.f.get(j) {
                    Some(p) => {
                        if p.kind == v.kind && p.olen == v.olen {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            prev = v.kind;
            i += 1;
        }
        if self.n == 0 {
            return self.last == 0;
        }
        self.last == prev
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(LSX_ESLOT);
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
            return Err(LSX_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 2, 3, 45, 4, 63, 2];
        const CODES: [u32; 6] =
            [LSX_EKIND, LSX_EOPND, LSX_EORDER, LSX_EDUP, LSX_ERANGE, LSX_ENONE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSX_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSX_EVALUE);
                        }
                    }
                    None => return Err(LSX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSX_EVALUE);
                        }
                    }
                    None => return Err(LSX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSX_EBAL);
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
            return Err(LSX_EBAL);
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
            Err(LSX_ESEAL)
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

/// 十四格账本：8 允 + 6 拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut r = Run::new();

    let _ = l.register(cell(0, OP_FAULT, r.fault(KIND_ACCESS, 4)));
    let _ = l.register(cell(1, OP_FAULT, r.fault(KIND_READ, 2)));
    let _ = l.register(word_cell(2, OP_STATUS, r.status()));
    let _ = l.register(cell(3, OP_FAULT, r.fault(KIND_PERM, 3)));
    let _ = l.register(word_cell(4, OP_TEXT, r.stderr_len()));
    let _ = l.register(cell(5, OP_FAULT, r.fault(KIND_ENC, 6)));
    let _ = l.register(word_cell(6, OP_TEXT, r.stderr_len()));
    let _ = l.register(word_cell(7, OP_STATUS, r.status()));

    let _ = l.register(cell(8, OP_FAULT, r.fault(0, 1)));
    let _ = l.register(cell(9, OP_FAULT, r.fault(KIND_ENC, 0)));
    let _ = l.register(cell(10, OP_FAULT, r.fault(KIND_PERM, 2)));
    let _ = l.register(cell(11, OP_FAULT, r.fault(KIND_ENC, 6)));
    let _ = l.register(cell(12, OP_FAULT, r.fault(KIND_ENC, (OP_MAX + 1) as u32)));

    let empty = Run::new();
    let _ = l.register(word_cell(13, OP_TEXT, empty.stderr_len()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：严重 + 次要 → 退出 2；文本按前缀累计。
    let mut r = Run::new();
    let r1 = r.fault(KIND_READ, 5);
    let r2 = r.fault(KIND_PERM, 4);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r.status() == Ok(2)
        && r.stderr_len() == Ok(33)
        && r.worst() == KIND_READ
        && r.at(0) == Some(Fault { kind: KIND_READ, olen: 5 })
        && r.count() == 2
        && r.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = Run::new();
    let kind = b.fault(0, 1) == Err(LSX_EKIND);
    let opnd = b.fault(KIND_ACCESS, 0) == Err(LSX_EOPND);
    let range = b.fault(KIND_ACCESS, (OP_MAX + 1) as u32) == Err(LSX_ERANGE);
    let first = b.fault(KIND_ACCESS, 3) == Ok(1);
    let order = b.fault(KIND_READ, 1) == Ok(2);
    let back = b.fault(KIND_ACCESS, 1) == Err(LSX_EORDER);
    let dup = b.fault(KIND_READ, 1) == Err(LSX_EDUP);
    let untouched = b.count() == 2;
    let none = Run::new().stderr_len() == Err(LSX_ENONE);
    let mut q = Run::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = q.fault(KIND_PERM, 1 + i);
        i += 1;
    }
    let full = q.fault(KIND_PERM, OP_MAX as u32) == Err(LSX_EFULL);
    let mut t = Run::new();
    let mut k = 0u32;
    while (k as usize) < CAP {
        let _ = t.fault(KIND_ACCESS, OP_MAX as u32 - k);
        k += 1;
    }
    let text = t.stderr_len() == Err(LSX_ETEXT);
    out[2] = kind
        && opnd
        && range
        && first
        && order
        && back
        && dup
        && untouched
        && none
        && full
        && text
        && b.consistent()
        && q.consistent();

    // 三路：退出码阶梯与表满。
    let mut m = Run::new();
    let s0 = m.status() == Ok(0);
    let l1 = m.fault(KIND_PERM, 7);
    let s1 = m.status() == Ok(1);
    let l2 = m.fault(KIND_ENC, 8);
    let s2 = m.status() == Ok(1);
    let mut n = Run::new();
    let _ = n.fault(KIND_ACCESS, 9);
    let s3 = n.status() == Ok(2);
    let mut c = Run::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = c.fault(KIND_ENC, 1 + i);
        i += 1;
    }
    let q1 = c.count() == CAP;
    let q2 = c.fault(KIND_ENC, OP_MAX as u32) == Err(LSX_EFULL);
    out[3] = s0
        && l1 == Ok(1)
        && s1
        && l2 == Ok(2)
        && s2
        && s3
        && q1
        && q2
        && c.consistent()
        && m.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11386_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11386.ls_exit_stderr");
    let l = build_ledger();

    s.add_dyn(
        "LSX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSX2-部分失败退出 2",
        l.sample(2).map(|x| x.value) == Some(2) && l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "LSX3-仅次要问题退出 1",
        {
            let mut r = Run::new();
            let _ = r.fault(KIND_PERM, 7);
            let _ = r.fault(KIND_ENC, 8);
            r.status() == Ok(1) && r.worst() == KIND_PERM && r.consistent()
        },
        "static",
    );

    s.add(
        "LSX4-无诊断退出 0",
        {
            let r = Run::new();
            r.status() == Ok(0)
                && r.count() == 0
                && r.worst() == 0
                && r.stderr_len() == Err(LSX_ENONE)
                && r.consistent()
        },
        "static",
    );

    s.add(
        "LSX5-未知类别被拒",
        {
            let mut r = Run::new();
            r.fault(0, 1) == Err(LSX_EKIND)
                && r.fault(5, 1) == Err(LSX_EKIND)
                && r.count() == 0
                && l.sample(8).map(|x| x.code) == Some(LSX_EKIND)
        },
        "static",
    );

    s.add(
        "LSX6-空操作数被拒",
        {
            let mut r = Run::new();
            r.fault(KIND_ACCESS, 0) == Err(LSX_EOPND)
                && r.count() == 0
                && l.sample(9).map(|x| x.code) == Some(LSX_EOPND)
        },
        "static",
    );

    s.add(
        "LSX7-类别倒序被拒",
        {
            let mut r = Run::new();
            let _ = r.fault(KIND_ENC, 3);
            r.fault(KIND_PERM, 3) == Err(LSX_EORDER)
                && r.count() == 1
                && r.consistent()
                && l.sample(10).map(|x| x.code) == Some(LSX_EORDER)
        },
        "static",
    );

    s.add(
        "LSX8-重复诊断被拒",
        {
            let mut r = Run::new();
            let _ = r.fault(KIND_ENC, 6);
            r.fault(KIND_ENC, 6) == Err(LSX_EDUP)
                && r.fault(KIND_ENC, 7) == Ok(2)
                && r.consistent()
                && l.sample(11).map(|x| x.code) == Some(LSX_EDUP)
        },
        "static",
    );

    s.add(
        "LSX9-操作数超长被拒",
        {
            let mut r = Run::new();
            r.fault(KIND_ACCESS, (OP_MAX + 1) as u32) == Err(LSX_ERANGE)
                && r.fault(KIND_ACCESS, OP_MAX as u32) == Ok(1)
                && r.consistent()
                && l.sample(12).map(|x| x.code) == Some(LSX_ERANGE)
        },
        "static",
    );

    s.add(
        "LSX10-空账不可量文本",
        {
            let r = Run::new();
            r.stderr_len() == Err(LSX_ENONE)
                && l.sample(13).map(|x| x.code) == Some(LSX_ENONE)
        },
        "static",
    );

    s.add(
        "LSX11-文本超限被拒",
        {
            let mut r = Run::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = r.fault(KIND_ACCESS, OP_MAX as u32 - i);
                i += 1;
            }
            r.count() == CAP
                && r.stderr_len() == Err(LSX_ETEXT)
                && r.consistent()
        },
        "static",
    );

    s.add(
        "LSX12-表满被拒",
        {
            let mut r = Run::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = r.fault(KIND_ENC, 1 + i);
                i += 1;
            }
            r.count() == CAP
                && r.fault(KIND_ENC, OP_MAX as u32) == Err(LSX_EFULL)
                && r.consistent()
                && l.sample(5).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "LSX13-冻结重放四注入",
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
        let s = run_unx11386_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn status_ladder() {
        let mut r = Run::new();
        assert_eq!(r.status(), Ok(0));
        assert_eq!(r.fault(KIND_PERM, 3), Ok(1));
        assert_eq!(r.status(), Ok(1));
        assert_eq!(r.fault(KIND_READ, 4), Err(LSX_EORDER));
        assert_eq!(r.fault(KIND_ENC, 5), Ok(2));
        assert_eq!(r.status(), Ok(1));
        let mut q = Run::new();
        let _ = q.fault(KIND_ACCESS, 6);
        assert_eq!(q.status(), Ok(2));
    }

    #[test]
    fn stderr_prefix_math() {
        let mut r = Run::new();
        assert_eq!(r.stderr_len(), Err(LSX_ENONE));
        let _ = r.fault(KIND_ACCESS, 4);
        assert_eq!(r.stderr_len(), Ok(16));
        let _ = r.fault(KIND_ENC, 6);
        assert_eq!(r.stderr_len(), Ok(34));
    }

    #[test]
    fn kind_gate() {
        let mut r = Run::new();
        assert_eq!(r.fault(0, 1), Err(LSX_EKIND));
        assert_eq!(r.fault(5, 1), Err(LSX_EKIND));
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn order_and_dup() {
        let mut r = Run::new();
        assert_eq!(r.fault(KIND_ENC, 3), Ok(1));
        assert_eq!(r.fault(KIND_ENC, 3), Err(LSX_EDUP));
        assert_eq!(r.fault(KIND_PERM, 2), Err(LSX_EORDER));
        assert_eq!(r.fault(KIND_ENC, 4), Ok(2));
        assert_eq!(r.count(), 2);
        assert!(r.consistent());
    }

    #[test]
    fn text_cap() {
        let mut r = Run::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert_eq!(r.fault(KIND_ACCESS, OP_MAX as u32 - i), Ok(i as usize + 1));
            i += 1;
        }
        assert_eq!(r.stderr_len(), Err(LSX_ETEXT));
        assert_eq!(r.fault(KIND_ACCESS, 1), Err(LSX_EFULL));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
