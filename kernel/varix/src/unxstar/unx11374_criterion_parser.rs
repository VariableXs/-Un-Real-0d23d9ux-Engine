//! UNX-F11374 判据库解析器（母版协议字段 → 运行期结构）
//!
//! 一条判据在母版里是七个字段：`tool` / `args` / `stdin` / `cwd 预置文件态` /
//! `expect_stdout` / `expect_exit` / `compare_mode`。解析器逐字段收窄，任何
//! 一个字段越界都显式拒收，绝不把半条坏判据塞进运行期库：
//!
//! 1. **工具闸门**：`tool` 必须落在已知工具号区间；0 是空工具，直接拒。
//! 2. **模式闸门**：`compare_mode` 只有全字节/关键列/退出码三档，越档拒收。
//! 3. **预置态闸门**：`cwd` 为 0 表示没有预置目录，沙盒无法落地，拒收。
//!
//! 闸门次序：
//! - `parse`：tool → compare_mode → expect_exit → cwd → expect_stdout →
//!   args → 重复三元组 → 库满；
//! - `pick` / `drift`：下标越界。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed60;

/// 工具号非法。
pub const PS_ETOOL: u32 = SEG_BASE + 0x01;
/// 比对模式越档。
pub const PS_EMODE: u32 = SEG_BASE + 0x02;
/// 期望退出码越界。
pub const PS_EEXIT: u32 = SEG_BASE + 0x03;
/// 预置目录非法。
pub const PS_ECWD: u32 = SEG_BASE + 0x04;
/// 期望输出观测越界。
pub const PS_EOUT: u32 = SEG_BASE + 0x05;
/// 参数个数越界。
pub const PS_EARGS: u32 = SEG_BASE + 0x06;
/// 三元组重复。
pub const PS_EDUP: u32 = SEG_BASE + 0x07;
/// 判据库已满。
pub const PS_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 判据库容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 工具号上界。
pub const TOOL_MAX: u32 = 64;
/// 预置目录号上界。
pub const CWD_MAX: u32 = 32;
/// 期望输出长度上界。
pub const OUT_MAX: u32 = 32;
/// 参数个数上界。
pub const ARGS_MAX: u32 = 16;
/// 七字段个数。
pub const FIELDS: usize = 7;

/// 操作码：解析一条判据。
pub const OP_PARSE: u8 = 1;
/// 操作码：取期望输出。
pub const OP_PICK: u8 = 2;
/// 操作码：与母版字段比对。
pub const OP_DRIFT: u8 = 3;
/// 操作码：库指纹。
pub const OP_SEAL: u8 = 4;

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

/// 运行期判据结构。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// 工具号。
    pub tool: u32,
    /// 参数个数。
    pub args: u32,
    /// 标准输入长度。
    pub stdin: u32,
    /// 预置目录号。
    pub cwd: u32,
    /// 期望输出长度。
    pub out: u32,
    /// 期望退出码。
    pub exit: u32,
    /// 比对模式。
    pub mode: u32,
}

/// 判据库。
#[derive(Clone, Copy, Debug)]
pub struct Lib {
    row: [Spec; CAP],
    n: usize,
}

impl Lib {
    /// 空库。
    pub const fn new() -> Lib {
        Lib {
            row: [Spec { tool: 0, args: 0, stdin: 0, cwd: 0, out: 0, exit: 0, mode: 0 }; CAP],
            n: 0,
        }
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Spec> {
        if i >= self.n {
            return None;
        }
        self.row.get(i).copied()
    }

    fn find(&self, s: &Spec) -> bool {
        let mut i = 0usize;
        while i < self.n {
            match self.row.get(i) {
                Some(e) => {
                    if e.tool == s.tool && e.args == s.args && e.cwd == s.cwd {
                        return true;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        false
    }

    /// 解析七字段；顺序为 tool/args/stdin/cwd/out/exit/mode。
    pub fn parse(&mut self, rec: [u32; FIELDS]) -> Result<usize, u32> {
        let tool = match rec.get(0) {
            Some(v) => *v,
            None => return Err(PS_ETOOL),
        };
        let args = match rec.get(1) {
            Some(v) => *v,
            None => return Err(PS_EARGS),
        };
        let stdin = match rec.get(2) {
            Some(v) => *v,
            None => return Err(PS_EOUT),
        };
        let cwd = match rec.get(3) {
            Some(v) => *v,
            None => return Err(PS_ECWD),
        };
        let out = match rec.get(4) {
            Some(v) => *v,
            None => return Err(PS_EOUT),
        };
        let exit = match rec.get(5) {
            Some(v) => *v,
            None => return Err(PS_EEXIT),
        };
        let mode = match rec.get(6) {
            Some(v) => *v,
            None => return Err(PS_EMODE),
        };
        if tool == 0 || tool > TOOL_MAX {
            return Err(PS_ETOOL);
        }
        if mode > 2 {
            return Err(PS_EMODE);
        }
        if exit > 255 {
            return Err(PS_EEXIT);
        }
        if cwd == 0 || cwd > CWD_MAX {
            return Err(PS_ECWD);
        }
        if out > OUT_MAX {
            return Err(PS_EOUT);
        }
        if args > ARGS_MAX {
            return Err(PS_EARGS);
        }
        let spec = Spec { tool, args, stdin, cwd, out, exit, mode };
        if self.find(&spec) {
            return Err(PS_EDUP);
        }
        if self.n >= CAP {
            return Err(PS_EFULL);
        }
        if let Some(slot) = self.row.get_mut(self.n) {
            *slot = spec;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 取第 i 条的期望输出长度。
    pub fn pick(&self, i: usize) -> Result<u32, u32> {
        match self.row.get(i) {
            Some(s) => {
                if i >= self.n {
                    Err(PS_EOUT)
                } else {
                    Ok(s.out)
                }
            }
            None => Err(PS_EOUT),
        }
    }

    /// 与母版七字段逐项比对，返回漂移字段数。
    pub fn drift(&self, i: usize, rec: [u32; FIELDS]) -> Result<u32, u32> {
        if i >= self.n {
            return Err(PS_EOUT);
        }
        let s = match self.row.get(i) {
            Some(v) => *v,
            None => return Err(PS_EOUT),
        };
        let mut d = 0u32;
        if s.tool != rec.first().copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.args != rec.get(1).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.stdin != rec.get(2).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.cwd != rec.get(3).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.out != rec.get(4).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.exit != rec.get(5).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        if s.mode != rec.get(6).copied().unwrap_or(0) {
            d = d.saturating_add(1);
        }
        Ok(d)
    }

    /// 库指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.row.get(i) {
                h = fold(h, s.tool as u64);
                h = fold(h, s.args as u64);
                h = fold(h, s.cwd as u64);
                h = fold(h, s.out as u64);
                h = fold(h, s.exit as u64);
                h = fold(h, s.mode as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let s = match self.row.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if s.tool == 0 || s.tool > TOOL_MAX {
                return false;
            }
            if s.mode > 2 || s.exit > 255 || s.cwd == 0 || s.cwd > CWD_MAX {
                return false;
            }
            if s.out > OUT_MAX || s.args > ARGS_MAX {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.row.get(j) {
                    Some(p) => {
                        if p.tool == s.tool && p.args == s.args && p.cwd == s.cwd {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
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

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(PS_ESLOT);
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
            return Err(PS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 0, 4];
        const CODES: [u32; 6] = [PS_ETOOL, PS_EMODE, PS_ECWD, PS_EOUT, PS_EDUP, PS_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PS_EBAL),
            };
            if s.slot as usize != i {
                return Err(PS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PS_EBAL);
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
            return Err(PS_EBAL);
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
            Err(PS_ESEAL)
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

/// 十四格账本：六条合法判据 → 工具/模式/目录/输出越界 → 重复 → 库满。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut lib = Lib::new();

    let _ = l.register(cell(0, OP_PARSE, lib.parse([1, 0, 0, 1, 0, 0, 0])));
    let _ = l.register(cell(1, OP_PARSE, lib.parse([2, 2, 0, 1, 4, 0, 1])));
    let _ = l.register(cell(2, OP_PARSE, lib.parse([0, 0, 0, 1, 0, 0, 0])));
    let _ = l.register(cell(3, OP_PARSE, lib.parse([3, 1, 0, 1, 0, 0, 3])));
    let _ = l.register(cell(4, OP_PARSE, lib.parse([3, 1, 0, 1, 0, 0, 2])));
    let _ = l.register(cell(5, OP_PARSE, lib.parse([3, 1, 0, 0, 0, 0, 0])));
    let _ = l.register(cell(6, OP_PARSE, lib.parse([4, 1, 0, 1, 999, 0, 0])));
    let _ = l.register(cell(7, OP_PARSE, lib.parse([4, 1, 0, 1, 8, 0, 0])));
    let _ = l.register(cell(8, OP_PARSE, lib.parse([4, 7, 0, 1, 9, 0, 0])));
    let _ = l.register(cell(9, OP_PARSE, lib.parse([3, 1, 0, 1, 0, 0, 2])));
    let _ = l.register(cell(10, OP_PARSE, lib.parse([5, 0, 0, 1, 0, 0, 0])));
    let _ = l.register(cell(11, OP_PARSE, lib.parse([6, 0, 0, 1, 0, 0, 0])));
    let _ = l.register(word_cell(12, OP_DRIFT, lib.drift(0, [1, 0, 0, 1, 0, 0, 0])));
    let _ = l.register(word_cell(13, OP_PICK, lib.pick(1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：七字段全部落位、无漂移。
    let mut a = Lib::new();
    let r1 = a.parse([1, 0, 0, 1, 0, 0, 0]);
    let r2 = a.parse([2, 2, 0, 2, 4, 0, 1]);
    let r3 = a.parse([3, 3, 1, 3, 8, 7, 2]);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && a.at(2)
            == Some(Spec { tool: 3, args: 3, stdin: 1, cwd: 3, out: 8, exit: 7, mode: 2 })
        && a.pick(1) == Ok(4)
        && a.drift(2, [3, 3, 1, 3, 8, 7, 2]) == Ok(0)
        && a.drift(2, [3, 3, 1, 3, 8, 7, 0]) == Ok(1)
        && a.seal() != 0
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不入库。
    let mut b = Lib::new();
    let tool = b.parse([0, 0, 0, 1, 0, 0, 0]) == Err(PS_ETOOL);
    let mode = b.parse([1, 0, 0, 1, 0, 0, 3]) == Err(PS_EMODE);
    let exit = b.parse([1, 0, 0, 1, 0, 256, 0]) == Err(PS_EEXIT);
    let cwd = b.parse([1, 0, 0, 0, 0, 0, 0]) == Err(PS_ECWD);
    let out_bad = b.parse([1, 0, 0, 1, 999, 0, 0]) == Err(PS_EOUT);
    let args = b.parse([1, 99, 0, 1, 0, 0, 0]) == Err(PS_EARGS);
    let untouched = b.count() == 0;
    let ok = b.parse([1, 1, 0, 1, 1, 0, 0]) == Ok(1);
    let dup = b.parse([1, 1, 0, 1, 0, 0, 2]) == Err(PS_EDUP);
    let kept = b.count() == 1;
    let mut c = Lib::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = c.parse([i + 1, 0, 0, 1, 0, 0, 0]);
        i += 1;
    }
    let full = c.parse([9, 0, 0, 1, 0, 0, 0]) == Err(PS_EFULL);
    out[2] = tool
        && mode
        && exit
        && cwd
        && out_bad
        && args
        && untouched
        && ok
        && dup
        && kept
        && full
        && b.consistent()
        && c.consistent();

    // 三路：库满硬拒，观测下标越界可辨。
    let mut q = Lib::new();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = q.parse([i + 1, i, 0, 1, i, 0, 0]);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.parse([CAP as u32 + 1, 0, 0, 1, 0, 0, 0]) == Err(PS_EFULL);
    let q3 = q.pick(CAP) == Err(PS_EOUT);
    let q4 = q.drift(CAP, [0; FIELDS]) == Err(PS_EOUT);
    let q5 = q.pick(2) == Ok(2);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11374_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11374.criterion_parser");
    let l = build_ledger();

    s.add_dyn(
        "PS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PS2-七字段逐项落位",
        {
            let mut a = Lib::new();
            let r = a.parse([2, 3, 4, 5, 6, 7, 1]);
            r == Ok(1)
                && a.at(0)
                    == Some(Spec { tool: 2, args: 3, stdin: 4, cwd: 5, out: 6, exit: 7, mode: 1 })
                && a.pick(0) == Ok(6)
                && a.drift(0, [2, 3, 4, 5, 6, 7, 1]) == Ok(0)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "PS3-工具号非法拒收",
        {
            let mut a = Lib::new();
            a.parse([0, 0, 0, 1, 0, 0, 0]) == Err(PS_ETOOL)
                && a.parse([TOOL_MAX + 1, 0, 0, 1, 0, 0, 0]) == Err(PS_ETOOL)
                && a.count() == 0
                && l.sample(2).map(|x| x.code) == Some(PS_ETOOL)
        },
        "static",
    );

    s.add(
        "PS4-比对模式越档拒收",
        {
            let mut a = Lib::new();
            let bad = a.parse([1, 0, 0, 1, 0, 0, 3]) == Err(PS_EMODE);
            let ok = a.parse([1, 0, 0, 1, 0, 0, 2]) == Ok(1);
            bad && ok && a.count() == 1 && l.sample(3).map(|x| x.code) == Some(PS_EMODE)
        },
        "static",
    );

    s.add(
        "PS5-退出码越界拒收",
        {
            let mut a = Lib::new();
            a.parse([1, 0, 0, 1, 0, 256, 0]) == Err(PS_EEXIT)
                && a.parse([1, 0, 0, 1, 0, 255, 0]) == Ok(1)
                && a.count() == 1
        },
        "static",
    );

    s.add(
        "PS6-预置目录非法拒收",
        {
            let mut a = Lib::new();
            let zero = a.parse([1, 0, 0, 0, 0, 0, 0]) == Err(PS_ECWD);
            let high = a.parse([1, 0, 0, CWD_MAX + 1, 0, 0, 0]) == Err(PS_ECWD);
            zero && high
                && a.count() == 0
                && l.sample(5).map(|x| x.code) == Some(PS_ECWD)
        },
        "static",
    );

    s.add(
        "PS7-期望输出越界拒收",
        {
            let mut a = Lib::new();
            a.parse([1, 0, 0, 1, 999, 0, 0]) == Err(PS_EOUT)
                && a.parse([1, 0, 0, 1, OUT_MAX, 0, 0]) == Ok(1)
                && a.count() == 1
                && l.sample(6).map(|x| x.code) == Some(PS_EOUT)
        },
        "static",
    );

    s.add(
        "PS8-参数个数越界拒收",
        {
            let mut a = Lib::new();
            a.parse([1, ARGS_MAX + 1, 0, 1, 0, 0, 0]) == Err(PS_EARGS)
                && a.parse([1, ARGS_MAX, 0, 1, 0, 0, 0]) == Ok(1)
                && a.count() == 1
        },
        "static",
    );

    s.add(
        "PS9-重复三元组拒收",
        {
            let mut a = Lib::new();
            let _ = a.parse([2, 2, 0, 1, 4, 0, 1]);
            let dup = a.parse([2, 2, 9, 1, 9, 9, 2]) == Err(PS_EDUP);
            let fresh = a.parse([2, 3, 0, 1, 0, 0, 0]) == Ok(2);
            dup && fresh && a.count() == 2 && l.sample(9).map(|x| x.code) == Some(PS_EDUP)
        },
        "static",
    );

    s.add(
        "PS10-库满硬上限",
        {
            let mut a = Lib::new();
            let mut i = 0u32;
            while (i as usize) < CAP {
                let _ = a.parse([i + 1, 0, 0, 1, 0, 0, 0]);
                i += 1;
            }
            a.count() == CAP
                && a.parse([CAP as u32 + 1, 0, 0, 1, 0, 0, 0]) == Err(PS_EFULL)
                && a.consistent()
                && l.sample(11).map(|x| x.code) == Some(PS_EFULL)
        },
        "static",
    );

    s.add(
        "PS11-观测下标越界",
        {
            let mut a = Lib::new();
            let _ = a.parse([1, 0, 0, 1, 5, 0, 0]);
            a.pick(1) == Err(PS_EOUT)
                && a.drift(1, [0; FIELDS]) == Err(PS_EOUT)
                && a.pick(0) == Ok(5)
                && a.at(1).is_none()
                && a.consistent()
        },
        "static",
    );

    s.add(
        "PS12-母版字段漂移计数",
        {
            let mut a = Lib::new();
            let _ = a.parse([1, 2, 3, 4, 5, 6, 1]);
            a.drift(0, [1, 2, 3, 4, 5, 6, 1]) == Ok(0)
                && a.drift(0, [9, 2, 3, 4, 5, 6, 1]) == Ok(1)
                && a.drift(0, [9, 9, 9, 9, 9, 9, 9]) == Ok(7)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "PS13-冻结重放四注入",
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
        let s = run_unx11374_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn seven_fields_roundtrip() {
        let mut a = Lib::new();
        assert_eq!(a.parse([4, 5, 6, 7, 8, 9, 2]), Ok(1));
        assert_eq!(
            a.at(0),
            Some(Spec { tool: 4, args: 5, stdin: 6, cwd: 7, out: 8, exit: 9, mode: 2 })
        );
        assert_eq!(a.drift(0, [4, 5, 6, 7, 8, 9, 2]), Ok(0));
    }

    #[test]
    fn bad_criterion_rejected() {
        let mut a = Lib::new();
        assert_eq!(a.parse([0, 0, 0, 1, 0, 0, 0]), Err(PS_ETOOL));
        assert_eq!(a.parse([1, 0, 0, 0, 0, 0, 0]), Err(PS_ECWD));
        assert_eq!(a.parse([1, 0, 0, 1, 0, 0, 9]), Err(PS_EMODE));
        assert_eq!(a.parse([1, 0, 0, 1, 0, 300, 0]), Err(PS_EEXIT));
        assert_eq!(a.parse([1, 0, 0, 1, 300, 0, 0]), Err(PS_EOUT));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn duplicate_triple() {
        let mut a = Lib::new();
        assert_eq!(a.parse([2, 2, 0, 1, 4, 0, 1]), Ok(1));
        assert_eq!(a.parse([2, 2, 1, 1, 5, 1, 2]), Err(PS_EDUP));
        assert_eq!(a.count(), 1);
    }

    #[test]
    fn capacity_limit() {
        let mut a = Lib::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert_eq!(a.parse([i + 1, 0, 0, 1, 0, 0, 0]), Ok(i as usize + 1));
            i += 1;
        }
        assert_eq!(a.parse([CAP as u32 + 1, 0, 0, 1, 0, 0, 0]), Err(PS_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
