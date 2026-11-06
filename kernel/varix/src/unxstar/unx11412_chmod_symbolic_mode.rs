//! UNX-F11412 chmod 符号模式（u/g/o/a 与 ±= 组合）
//!
//! # 领域模型
//!
//! 符号模式是一串以逗号分隔的子句，每个子句形如
//! `[ugoa]*[+-=][rwxXstugo]*`：
//!
//! * `who`：`u`、`g`、`o` 或 `a`（等于三者并集）。可以叠加（`go`、`ug`），
//!   重复字母幂等。省略 `who` 时等价于 `a`，但要与 `umask` 的补集相交：
//!   进程掩码里被禁止的位既不会被 `+` 授予，也不会被 `-` 清除，`=`
//!   也只在掩码允许的范围内设置——这正是 `chmod =rwx` 在 umask 022 下
//!   得到 0755 而不是 0777 的原因。
//! * 运算符：`+` 并集、`-` 差集、`=` 先清空所选 who 的 rwx 与对应特殊位，
//!   再并上给定的类别。
//! * 类别：`r`、`w`、`x`；`X` 为条件执行位（目标目录或已存在任一执行位
//!   时才置 x）；`s` 为 setuid/setgid（按 who 分发：`u`→setuid、
//!   `g`→setgid，省略 who 时两者都设）；`t` 为 sticky（who 必须含 `o`
//!   或省略）；`u`/`g`/`o` 出现在运算符右侧表示“复制该类别的 rwx 三元组”。
//!
//! 复制与 rwxXst 不能混用，`s` 不能配 `o`，`t` 不能配 `u`/`g`，这些
//! 属于结构性错误而不是普通非法字符，分别归入 `ECLASS` 与 `EORDER`。
//! 子句从左到右依次作用，后写者覆盖先写者（`u+x,u-x` 等于没动）。
//!
//! # 闸门次序
//!
//! `apply_mode(旧模式, 是否目录, umask, 参数串, 是否属主, 是否有特权, 是否只读)`
//! 的顺序是：
//!
//! 1. 解析整串为定长子句表：空串/空子句 → `EEMPTY`；非法 who 字母 →
//!    `EWHO`；缺运算符 → `EOP`；类别先于运算符 → `EORDER`；非法类别、
//!    复制与类别混用 → `ECLASS`；子句超过 8 条或单条超过 8 字节 →
//!    `EFULL`；
//! 2. 文件系统只读 → `EROFS`；
//! 3. 非属主且无特权 → `EPERM`；
//! 4. 逐条作用，返回 12 位新模式。
//!
//! 整个参数串必须在触碰权限闸门之前解析完毕：一个只读挂载上的空模式串
//! 仍然先得到 `EEMPTY`，语法永远先于状态。
//!
//! # 账本契约
//!
//! 十四格、八成功六失败。成功格覆盖：`u+x`、`go-w`、`a=r`、
//! `u=rwx,g=rx,o=`、`g=u`（复制）、`=rwx`（省略 who + umask）、
//! `o+t`（sticky）、`a+X`（目录条件执行）。失败格依次为 `EEMPTY`、
//! `EWHO`、`EOP`、`EORDER`、`ECLASS`、`EFULL`。
//!
//! `audit()` 核对格数、成败计数、槽位编号、成功值序列
//! `[500, 420, 292, 488, 432, 448, 1023, 493]` 与错误码序列
//! `[EEMPTY, EWHO, EOP, EORDER, ECLASS, EFULL]`。
//!
//! `seal()` 用 FNV-1a 折叠槽位、操作码、成败、错误码、观测值与拒绝
//! 计数；`freeze()` 固化；`replay()` 重建并比对，指纹不符即 `ESEAL`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址：本卡独占的错误码窗口。
pub const SEG_BASE: u32 = 0xefc0;

/// 参数串为空，或逗号之间出现空子句。
pub const SY_EEMPTY: u32 = SEG_BASE + 0x01;
/// 运算符之前出现非 who 字母。
pub const SY_EWHO: u32 = SEG_BASE + 0x02;
/// 缺少运算符。
pub const SY_EOP: u32 = SEG_BASE + 0x03;
/// 类别先于运算符，或特殊位与 who 不相容。
pub const SY_EORDER: u32 = SEG_BASE + 0x04;
/// 非法类别，或复制与 rwxXst 混用。
pub const SY_ECLASS: u32 = SEG_BASE + 0x05;
/// 子句过多或单条子句过长。
pub const SY_EFULL: u32 = SEG_BASE + 0x06;
/// 文件系统只读。
pub const SY_EROFS: u32 = SEG_BASE + 0x07;
/// 非属主且无特权。
pub const SY_EPERM: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SY_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错乱。
pub const SY_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const SY_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SY_ESEAL: u32 = SEG_BASE + 0x0c;
/// 账本无此观测。
pub const SY_ENOBS: u32 = SEG_BASE + 0x0d;

/// 子句数上限。
pub const CAP: usize = 8;
/// 单条子句字节数上限。
pub const CAP_CHARS: usize = 8;
/// 模式位上限。
pub const MODE_MAX: u16 = 0o7777;

/// who 位：属主。
pub const W_U: u8 = 1;
/// who 位：属组。
pub const W_G: u8 = 2;
/// who 位：其他。
pub const W_O: u8 = 4;

/// 解析操作。
pub const OP_PARSE: u8 = 1;
/// 作用操作。
pub const OP_APPLY: u8 = 2;

/// 样本格数。
pub const SAMPLES: usize = 14;

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

/// 解析后的一个符号子句。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clause {
    /// who 位（0 表示省略）。
    pub who: u8,
    /// 运算符字节：`+`、`-`、`=`。
    pub op: u8,
    /// rwx 位（X 归一为 x，另由 `cond_x` 标记条件性）。
    pub bits: u16,
    /// 复制来源 who 位（0 表示不复制）。
    pub copy: u8,
    /// 是否含条件执行位 `X`。
    pub cond_x: bool,
    /// 特殊位（setuid/setgid/sticky）。
    pub special: u16,
}

impl Clause {
    /// 空白子句。
    pub const fn blank() -> Clause {
        Clause { who: 0, op: 0, bits: 0, copy: 0, cond_x: false, special: 0 }
    }
}

/// 解析结果：定长子句表。
#[derive(Clone, Copy, Debug)]
pub struct Program {
    /// 子句表。
    pub clauses: [Clause; CAP],
    /// 有效条数。
    pub n: usize,
}

impl Program {
    /// 空程序。
    pub const fn blank() -> Program {
        Program { clauses: [Clause::blank(); CAP], n: 0 }
    }

    /// 取第 i 条。
    pub fn clause(&self, i: usize) -> Option<Clause> {
        if i >= self.n {
            return None;
        }
        self.clauses.get(i).copied()
    }

    /// 逐条作用。
    pub fn apply(&self, old: u16, is_dir: bool, umask: u16) -> u16 {
        let mut mode = old & MODE_MAX;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.clauses.get(i) {
                mode = apply_clause(mode, *c, is_dir, umask);
            }
            i += 1;
        }
        mode & MODE_MAX
    }
}

/// 解析符号模式。
pub fn parse_sym(spec: &[u8]) -> Result<Program, u32> {
    let mut p = Program::blank();
    if spec.is_empty() {
        return Err(SY_EEMPTY);
    }
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= spec.len() {
        let at_end = i == spec.len();
        let comma = spec.get(i).copied() == Some(b',');
        if at_end || comma {
            let end = i;
            if end == start {
                return Err(SY_EEMPTY);
            }
            let part = match spec.get(start..end) {
                Some(v) => v,
                None => return Err(SY_EEMPTY),
            };
            if part.len() > CAP_CHARS {
                return Err(SY_EFULL);
            }
            if p.n >= CAP {
                return Err(SY_EFULL);
            }
            let c = parse_clause(part)?;
            if let Some(slot) = p.clauses.get_mut(p.n) {
                *slot = c;
            }
            p.n = p.n.saturating_add(1);
            start = end.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    Ok(p)
}

/// 解析单条子句。
fn parse_clause(part: &[u8]) -> Result<Clause, u32> {
    let mut c = Clause::blank();
    let mut who = 0u8;
    let mut i = 0usize;
    while i < part.len() {
        let b = match part.get(i) {
            Some(v) => *v,
            None => break,
        };
        let bit = match b {
            b'u' => W_U,
            b'g' => W_G,
            b'o' => W_O,
            b'a' => W_U | W_G | W_O,
            _ => 0,
        };
        if bit == 0 {
            break;
        }
        who |= bit;
        i = i.saturating_add(1);
    }
    c.who = who;
    let op = match part.get(i) {
        Some(v) => *v,
        None => return Err(SY_EOP),
    };
    if op != b'+' && op != b'-' && op != b'=' {
        if op == b'r' || op == b'w' || op == b'x' || op == b'X' || op == b's' || op == b't' {
            return Err(SY_EORDER);
        }
        return Err(SY_EWHO);
    }
    c.op = op;
    i = i.saturating_add(1);
    if i == part.len() {
        return Ok(c);
    }

    let mut bits = 0u16;
    let mut cond_x = false;
    let mut copy = 0u8;
    let mut special = 0u16;
    let mut classes = 0usize;
    let mut mark_s = false;
    let mut mark_t = false;
    while i < part.len() {
        let b = match part.get(i) {
            Some(v) => *v,
            None => break,
        };
        match b {
            b'r' => bits |= 4,
            b'w' => bits |= 2,
            b'x' => bits |= 1,
            b'X' => {
                bits |= 1;
                cond_x = true;
            }
            b's' => mark_s = true,
            b't' => mark_t = true,
            b'u' => copy |= W_U,
            b'g' => copy |= W_G,
            b'o' => copy |= W_O,
            _ => return Err(SY_ECLASS),
        }
        classes = classes.saturating_add(1);
        i = i.saturating_add(1);
    }
    if copy != 0 && (classes != 1 || bits != 0 || mark_s || mark_t) {
        return Err(SY_ECLASS);
    }
    if mark_s {
        if who != 0 && (who & (W_U | W_G)) == 0 {
            return Err(SY_EORDER);
        }
        if who & W_U != 0 || who == 0 {
            special |= 0o4000;
        }
        if who & W_G != 0 || who == 0 {
            special |= 0o2000;
        }
    }
    if mark_t {
        if who != 0 && (who & W_O) == 0 {
            return Err(SY_EORDER);
        }
        special |= 0o1000;
    }
    c.bits = bits;
    c.cond_x = cond_x;
    c.copy = copy;
    c.special = special;
    Ok(c)
}

/// 作用一条子句。
fn apply_clause(mode: u16, c: Clause, is_dir: bool, umask: u16) -> u16 {
    let src = mode & MODE_MAX;
    let who = if c.who == 0 { W_U | W_G | W_O } else { c.who };
    let mut out = src;

    if c.op == b'=' {
        let mut clear = 0u16;
        if who & W_U != 0 {
            clear |= 0o700 | 0o4000;
        }
        if who & W_G != 0 {
            clear |= 0o070 | 0o2000;
        }
        if who & W_O != 0 {
            clear |= 0o007 | 0o1000;
        }
        out &= !clear;
    }

    let class_bits = [W_U, W_G, W_O];
    let class_shifts = [6u16, 3u16, 0u16];
    let mut k = 0usize;
    while k < 3 {
        let cb = match class_bits.get(k) {
            Some(v) => *v,
            None => break,
        };
        let sh = match class_shifts.get(k) {
            Some(v) => *v,
            None => break,
        };
        if who & cb != 0 {
            let allowed = if c.who == 0 {
                7u16 & !((umask >> sh) & 7)
            } else {
                7u16
            };
            let mut triple = c.bits & 7;
            if c.cond_x {
                if is_dir || (src & 0o111) != 0 {
                    triple |= 1;
                } else {
                    triple &= !1;
                }
            }
            let mut source = triple;
            if c.copy != 0 {
                let ssh = if c.copy & W_U != 0 {
                    6u16
                } else if c.copy & W_G != 0 {
                    3u16
                } else {
                    0u16
                };
                source = (src >> ssh) & 7;
            }
            let grant = (source & allowed) << sh;
            match c.op {
                b'+' => out |= grant,
                b'-' => out &= !grant,
                _ => out |= grant,
            }
        }
        k = k.saturating_add(1);
    }

    match c.op {
        b'+' => out |= c.special,
        b'-' => out &= !c.special,
        _ => out |= c.special,
    }
    out & MODE_MAX
}

/// 施加符号模式：解析 → 只读 → 身份 → 逐条作用。
pub fn apply_mode(
    old: u16,
    is_dir: bool,
    umask: u16,
    spec: &[u8],
    owner: bool,
    capable: bool,
    ro: bool,
) -> Result<u16, u32> {
    let p = parse_sym(spec)?;
    if ro {
        return Err(SY_EROFS);
    }
    if !owner && !capable {
        return Err(SY_EPERM);
    }
    Ok(p.apply(old, is_dir, umask))
}

/// 模式 → 十进制打包（供人读的观测值）。
pub fn render(mode: u16) -> u32 {
    let mut v = (mode & MODE_MAX) as u32;
    let mut out = 0u32;
    let mut mul = 1u32;
    let mut i = 0u32;
    while i < 4 {
        out = out.saturating_add((v & 7).saturating_mul(mul));
        v >>= 3;
        mul = mul.saturating_mul(10);
        i += 1;
    }
    out
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
        }
        self.len = self.len.saturating_add(1);
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
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SY_EBAL);
        }
        const VALS: [u32; 8] = [500, 420, 292, 488, 432, 448, 1023, 493];
        const CODES: [u32; 6] =
            [SY_EEMPTY, SY_EWHO, SY_EOP, SY_EORDER, SY_ECLASS, SY_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SY_EBAL),
            };
            if r.slot as usize != i {
                return Err(SY_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(SY_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(SY_EVALUE);
                        }
                    }
                    None => return Err(SY_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                if r.value != 0 {
                    return Err(SY_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(SY_EVALUE);
                        }
                    }
                    None => return Err(SY_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SY_EBAL);
        }
        Ok(())
    }

    /// FNV-1a 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.slot as u64);
                h = fold(h, r.op as u64);
                h = fold(h, r.ok as u64);
                h = fold(h, r.code as u64);
                h = fold(h, r.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(SY_ESEAL);
        }
        let fresh = build_ledger();
        let again = fresh.seal();
        if again == self.seal() && again == self.root {
            Ok(again)
        } else {
            Err(SY_ESEAL)
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

/// 固定参数：旧模式、是否目录、umask、属主、特权、只读。
const OLD_A: u16 = 0o664;
/// 样本 umask。
const SAMPLE_MASK: u16 = 0o022;

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let m = SAMPLE_MASK;

    let _ = l.register(word_cell(
        0,
        OP_APPLY,
        apply_mode(OLD_A, false, m, b"u+x", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        1,
        OP_APPLY,
        apply_mode(OLD_A, false, m, b"go-w", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        2,
        OP_APPLY,
        apply_mode(OLD_A, false, m, b"a=r", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        3,
        OP_APPLY,
        apply_mode(OLD_A, false, m, b"u=rwx,g=rx,o=", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        4,
        OP_APPLY,
        apply_mode(0o640, false, m, b"g=u", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        5,
        OP_APPLY,
        apply_mode(0o777, false, 0o077, b"=rwx", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        6,
        OP_APPLY,
        apply_mode(0o777, true, m, b"o+t", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        7,
        OP_APPLY,
        apply_mode(0o644, true, m, b"a+X", true, false, false).map(|v| v as u32),
    ));

    let _ = l.register(word_cell(
        8,
        OP_PARSE,
        apply_mode(OLD_A, false, m, b"", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        9,
        OP_PARSE,
        apply_mode(OLD_A, false, m, b"v+x", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        10,
        OP_PARSE,
        apply_mode(OLD_A, false, m, b"u", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        11,
        OP_PARSE,
        apply_mode(OLD_A, false, m, b"o+s", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        12,
        OP_PARSE,
        apply_mode(OLD_A, false, m, b"u+z", true, false, false).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(
        13,
        OP_PARSE,
        apply_mode(
            OLD_A,
            false,
            m,
            b"u+r,g+r,o+r,u+w,g+w,o+w,u+x,g+x,o+x",
            true,
            false,
            false,
        )
        .map(|v| v as u32),
    ));

    l
}

/// 四路注入：账本身、模式代数、umask 省略 who、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let mut f = l;
    let frozen = f.freeze();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && frozen.is_ok()
        && f.is_frozen()
        && f.root() != 0
        && f.replay() == Ok(f.root())
        && f.seal() == f.root();

    let m = SAMPLE_MASK;
    out[1] = apply_mode(0o664, false, m, b"u+x", true, false, false) == Ok(0o764)
        && apply_mode(0o664, false, m, b"go-w", true, false, false) == Ok(0o644)
        && apply_mode(0o664, false, m, b"a=r", true, false, false) == Ok(0o444)
        && apply_mode(0o664, false, m, b"u+x,u-x", true, false, false) == Ok(0o664)
        && apply_mode(0o640, false, m, b"g=u", true, false, false) == Ok(0o660)
        && apply_mode(0o750, false, m, b"a=u", true, false, false) == Ok(0o777)
        && apply_mode(0o755, false, m, b"u+s", true, false, false) == Ok(0o4755)
        && apply_mode(0o755, false, m, b"g+s", true, false, false) == Ok(0o2755)
        && apply_mode(0o777, true, m, b"o+t", true, false, false) == Ok(0o1777)
        && apply_mode(0o644, true, m, b"a+X", true, false, false) == Ok(0o755)
        && apply_mode(0o644, false, m, b"a+X", true, false, false) == Ok(0o644);

    out[2] = apply_mode(0o777, false, 0o022, b"=rwx", true, false, false) == Ok(0o755)
        && apply_mode(0o777, false, 0o077, b"=rwx", true, false, false) == Ok(0o700)
        && apply_mode(0o444, false, 0o022, b"+w", true, false, false) == Ok(0o644)
        && apply_mode(0o666, false, 0o022, b"-w", true, false, false) == Ok(0o466)
        && apply_mode(0o666, false, 0o022, b"u+w", true, false, false) == Ok(0o666)
        && apply_mode(0o555, false, 0o022, b"a+w", true, false, false) == Ok(0o777);

    out[3] = apply_mode(0o644, false, 0o022, b"", true, false, true) == Err(SY_EEMPTY)
        && apply_mode(0o644, false, 0o022, b"u+z", true, false, true) == Err(SY_ECLASS)
        && apply_mode(0o644, false, 0o022, b"u+x", true, false, true) == Err(SY_EROFS)
        && apply_mode(0o644, false, 0o022, b"u+x", false, false, false) == Err(SY_EPERM)
        && apply_mode(0o644, false, 0o022, b"u+x", false, true, false) == Ok(0o744)
        && apply_mode(0o644, false, 0o022, b"u+x", false, true, true) == Err(SY_EROFS)
        && apply_mode(0o644, false, 0o022, b"v+x", true, false, false) == Err(SY_EWHO)
        && apply_mode(0o644, false, 0o022, b"u", true, false, false) == Err(SY_EOP)
        && apply_mode(0o644, false, 0o022, b"rx", true, false, false) == Err(SY_EORDER);

    out
}

/// 十三项检查。
pub fn run_unx11412_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11412");
    let l = build_ledger();

    s.add(
        "SY1-台账格数与审计",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        "static",
    );

    s.add(
        "SY2-八正例值序",
        {
            const VALS: [u32; 8] = [500, 420, 292, 488, 432, 448, 1023, 493];
            let mut good = true;
            let mut vi = 0usize;
            let mut i = 0usize;
            while i < SAMPLES {
                match l.sample(i) {
                    Some(r) => {
                        if r.ok {
                            if VALS.get(vi) != Some(&r.value) {
                                good = false;
                            }
                            vi = vi.saturating_add(1);
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && vi == 8
        },
        "static",
    );

    s.add(
        "SY3-六错误码序",
        {
            const CODES: [u32; 6] =
                [SY_EEMPTY, SY_EWHO, SY_EOP, SY_EORDER, SY_ECLASS, SY_EFULL];
            let mut good = true;
            let mut ci = 0usize;
            let mut i = 0usize;
            while i < SAMPLES {
                match l.sample(i) {
                    Some(r) => {
                        if !r.ok {
                            if CODES.get(ci) != Some(&r.code) {
                                good = false;
                            }
                            ci = ci.saturating_add(1);
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && ci == 6
        },
        "static",
    );

    s.add(
        "SY4-加减与赋值基本律",
        {
            let m = SAMPLE_MASK;
            apply_mode(0o664, false, m, b"u+x", true, false, false) == Ok(0o764)
                && apply_mode(0o664, false, m, b"go-w", true, false, false) == Ok(0o644)
                && apply_mode(0o664, false, m, b"a=r", true, false, false) == Ok(0o444)
                && apply_mode(0o664, false, m, b"u+x,u-x", true, false, false) == Ok(0o664)
                && apply_mode(0o664, false, m, b"a=rwx,o=", true, false, false) == Ok(0o770)
                && apply_mode(0o664, false, m, b"o=", true, false, false) == Ok(0o660)
                && apply_mode(0, false, m, b"a+r", true, false, false) == Ok(0o444)
        },
        "static",
    );

    s.add(
        "SY5-省略who与掩码",
        apply_mode(0o777, false, 0o022, b"=rwx", true, false, false) == Ok(0o755)
            && apply_mode(0o777, false, 0o077, b"=rwx", true, false, false) == Ok(0o700)
            && apply_mode(0o444, false, 0o022, b"+w", true, false, false) == Ok(0o644)
            && apply_mode(0o666, false, 0o022, b"-w", true, false, false) == Ok(0o466)
            && apply_mode(0o666, false, 0o022, b"u+w", true, false, false) == Ok(0o666)
            && apply_mode(0o555, false, 0o022, b"a+w", true, false, false) == Ok(0o777),
        "static",
    );

    s.add(
        "SY6-复制类别",
        apply_mode(0o640, false, 0o022, b"g=u", true, false, false) == Ok(0o660)
            && apply_mode(0o764, false, 0o022, b"o=g", true, false, false) == Ok(0o766)
            && apply_mode(0o750, false, 0o022, b"a=u", true, false, false) == Ok(0o777)
            && apply_mode(0o740, false, 0o022, b"g=u,o=g", true, false, false) == Ok(0o777)
            && apply_mode(0o777, false, 0o022, b"u-g", true, false, false) == Ok(0o077)
            && apply_mode(0o770, false, 0o022, b"o+u", true, false, false) == Ok(0o777)
            && apply_mode(0o777, false, 0o022, b"u+ur", true, false, false) == Err(SY_ECLASS),
        "static",
    );

    s.add(
        "SY7-特殊位",
        apply_mode(0o755, false, 0o022, b"u+s", true, false, false) == Ok(0o4755)
            && apply_mode(0o755, false, 0o022, b"g+s", true, false, false) == Ok(0o2755)
            && apply_mode(0o755, false, 0o022, b"u+s,g+s", true, false, false) == Ok(0o6755)
            && apply_mode(0o6755, false, 0o022, b"a-s", true, false, false) == Ok(0o755)
            && apply_mode(0o777, true, 0o022, b"o+t", true, false, false) == Ok(0o1777)
            && apply_mode(0o1777, true, 0o022, b"a-t", true, false, false) == Ok(0o777)
            && apply_mode(0o777, false, 0o022, b"o+s", true, false, false) == Err(SY_EORDER)
            && apply_mode(0o777, false, 0o022, b"u+t", true, false, false) == Err(SY_EORDER),
        "static",
    );

    let (sy8_ok, sy8_dir, sy8_file) = {
        let m = SAMPLE_MASK;
        let dir = apply_mode(0o644, true, m, b"a+X", true, false, false);
        let file = apply_mode(0o644, false, m, b"a+X", true, false, false);
        let hasx = apply_mode(0o744, false, m, b"a+X", true, false, false);
        let one = apply_mode(0o600, true, m, b"u+X", true, false, false);
        let none = apply_mode(0o600, true, m, b"u-x", true, false, false);
        let good = dir == Ok(0o755)
            && file == Ok(0o644)
            && hasx == Ok(0o755)
            && one == Ok(0o700)
            && none == Ok(0o600);
        (good, dir.unwrap_or(0), file.unwrap_or(0))
    };
    s.add_dyn(
        "SY8-条件执行位X",
        sy8_ok,
        &format!("dir={} file={}", render(sy8_dir), render(sy8_file)),
    );

    s.add(
        "SY9-门序优先",
        {
            let m = SAMPLE_MASK;
            apply_mode(0o644, false, m, b"", true, false, true) == Err(SY_EEMPTY)
                && apply_mode(0o644, false, m, b"u+z", true, false, true) == Err(SY_ECLASS)
                && apply_mode(0o644, false, m, b"u+x", true, false, true) == Err(SY_EROFS)
                && apply_mode(0o644, false, m, b"u+x", false, false, false) == Err(SY_EPERM)
                && apply_mode(0o644, false, m, b"u+x", false, true, false) == Ok(0o744)
                && apply_mode(0o644, false, m, b"u+x", false, true, true) == Err(SY_EROFS)
                && apply_mode(0o644, false, m, b"u+x", true, false, false) == Ok(0o744)
        },
        "static",
    );

    s.add(
        "SY10-错误分类",
        apply_mode(0o644, false, 0o022, b"v+x", true, false, false) == Err(SY_EWHO)
            && apply_mode(0o644, false, 0o022, b"u v", true, false, false) == Err(SY_EWHO)
            && apply_mode(0o644, false, 0o022, b"u", true, false, false) == Err(SY_EOP)
            && apply_mode(0o644, false, 0o022, b"rx", true, false, false) == Err(SY_EORDER)
            && apply_mode(0o644, false, 0o022, b"t", true, false, false) == Err(SY_EORDER)
            && apply_mode(0o644, false, 0o022, b"u+z", true, false, false) == Err(SY_ECLASS)
            && apply_mode(0o644, false, 0o022, b"u+ur", true, false, false) == Err(SY_ECLASS)
            && apply_mode(0o644, false, 0o022, b"u+go", true, false, false) == Err(SY_ECLASS)
            && apply_mode(
                0o644,
                false,
                0o022,
                b"u+r,g+r,o+r,u+w,g+w,o+w,u+x,g+x,o+x",
                true,
                false,
                false,
            ) == Err(SY_EFULL),
        "static",
    );

    s.add(
        "SY11-逗号与空子句",
        apply_mode(0o644, false, 0o022, b"u+x,", true, false, false) == Err(SY_EEMPTY)
            && apply_mode(0o644, false, 0o022, b",u+x", true, false, false) == Err(SY_EEMPTY)
            && apply_mode(0o644, false, 0o022, b"u+x,,g-w", true, false, false) == Err(SY_EEMPTY)
            && apply_mode(0o644, false, 0o022, b"u+x,g-w,o+r", true, false, false) == Ok(0o744)
            && {
                let p = parse_sym(b"u+x,g-w,o+r");
                matches!(p, Ok(v) if v.n == 3 && v.clause(0).map(|c| c.op) == Some(b'+'))
            },
        "static",
    );

    s.add(
        "SY12-组合与末次优先",
        apply_mode(0o000, false, 0o022, b"u=rwx,g=u", true, false, false) == Ok(0o770)
            && apply_mode(0o740, false, 0o022, b"g=u,o=g", true, false, false) == Ok(0o777)
            && apply_mode(0o777, false, 0o022, b"a=rwx,o=", true, false, false) == Ok(0o770)
            && apply_mode(0o755, false, 0o022, b"u+s,g+s,a-s,o+t", true, false, false)
                == Ok(0o1755)
            && apply_mode(0o664, false, 0o022, b"a+X,u+x", true, false, false) == Ok(0o764),
        "static",
    );

    s.add(
        "SY13-冻结重放四注入",
        {
            let mut f = l;
            let sealed = f.freeze();
            let inj = inject_four();
            sealed.is_ok()
                && f.is_frozen()
                && f.root() != 0
                && f.replay() == Ok(f.root())
                && inj[0]
                && inj[1]
                && inj[2]
                && inj[3]
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
        assert!(l.audit().is_ok());
        assert!(!l.is_empty());
        let mut f = l;
        let seal = f.freeze().unwrap();
        assert_ne!(seal, 0);
        assert!(f.is_frozen());
        assert_eq!(f.replay(), Ok(seal));
        assert_eq!(f.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11412_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn basic_ops() {
        let m = 0o022;
        assert_eq!(apply_mode(0o664, false, m, b"u+x", true, false, false), Ok(0o764));
        assert_eq!(apply_mode(0o664, false, m, b"go-w", true, false, false), Ok(0o644));
        assert_eq!(apply_mode(0o664, false, m, b"a=r", true, false, false), Ok(0o444));
        assert_eq!(apply_mode(0o664, false, m, b"o=", true, false, false), Ok(0o660));
        assert_eq!(apply_mode(0o664, false, m, b"u+x,u-x", true, false, false), Ok(0o664));
        assert_eq!(apply_mode(0, false, m, b"a+rw", true, false, false), Ok(0o666));
    }

    #[test]
    fn copy_and_special() {
        let m = 0o022;
        assert_eq!(apply_mode(0o640, false, m, b"g=u", true, false, false), Ok(0o660));
        assert_eq!(apply_mode(0o750, false, m, b"a=u", true, false, false), Ok(0o777));
        assert_eq!(apply_mode(0o755, false, m, b"u+s", true, false, false), Ok(0o4755));
        assert_eq!(apply_mode(0o755, false, m, b"g+s", true, false, false), Ok(0o2755));
        assert_eq!(apply_mode(0o6755, false, m, b"a-s", true, false, false), Ok(0o755));
        assert_eq!(apply_mode(0o777, true, m, b"o+t", true, false, false), Ok(0o1777));
        assert_eq!(apply_mode(0o777, false, m, b"u+ur", true, false, false), Err(SY_ECLASS));
    }

    #[test]
    fn umask_omitted_who() {
        assert_eq!(apply_mode(0o777, false, 0o022, b"=rwx", true, false, false), Ok(0o755));
        assert_eq!(apply_mode(0o777, false, 0o077, b"=rwx", true, false, false), Ok(0o700));
        assert_eq!(apply_mode(0o444, false, 0o022, b"+w", true, false, false), Ok(0o644));
        assert_eq!(apply_mode(0o666, false, 0o022, b"-w", true, false, false), Ok(0o466));
        assert_eq!(apply_mode(0o666, false, 0o022, b"u+w", true, false, false), Ok(0o666));
    }

    #[test]
    fn conditional_x() {
        assert_eq!(apply_mode(0o644, true, 0o022, b"a+X", true, false, false), Ok(0o755));
        assert_eq!(apply_mode(0o644, false, 0o022, b"a+X", true, false, false), Ok(0o644));
        assert_eq!(apply_mode(0o744, false, 0o022, b"a+X", true, false, false), Ok(0o755));
        assert_eq!(apply_mode(0o600, true, 0o022, b"u+X", true, false, false), Ok(0o700));
    }

    #[test]
    fn gate_order_and_errors() {
        assert_eq!(apply_mode(0o644, false, 0o022, b"", true, false, true), Err(SY_EEMPTY));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u+z", true, false, true), Err(SY_ECLASS));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u+x", true, false, true), Err(SY_EROFS));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u+x", false, false, false), Err(SY_EPERM));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u+x", false, true, false), Ok(0o744));
        assert_eq!(apply_mode(0o644, false, 0o022, b"v+x", true, false, false), Err(SY_EWHO));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u", true, false, false), Err(SY_EOP));
        assert_eq!(apply_mode(0o644, false, 0o022, b"rx", true, false, false), Err(SY_EORDER));
        assert_eq!(apply_mode(0o644, false, 0o022, b"o+s", true, false, false), Err(SY_EORDER));
        assert_eq!(apply_mode(0o644, false, 0o022, b"u+x,", true, false, false), Err(SY_EEMPTY));
    }

    #[test]
    fn inject_four_holds() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
    }
}
