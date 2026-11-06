//! UNX-F11376 结果比对器（compare_mode 三模引擎）
//!
//! 快照结果按 `compare_mode` 分三档比对：**全字节**、**关键列**、**退出码**。
//! 动态值（时间戳/端口等）不进全字节比对，但必须显式声明豁免位，否则拒收：
//!
//! 1. **模式闸门**：只有 0/1/2 三档模式；未装模式不许比对。
//! 2. **豁免闸门**：动态值必须由调用方标成「可豁免」。没标的动态值按不一致
//!    处理，防止用通配把真差异洗掉。
//! 3. **退出码闸门**：退出码只收 0..=255；越界不是「不一致」，是坏输入。
//!
//! 闸门次序：
//! - `feed`：未装模式/退出码模式 → 行容量 → 动态豁免 → 相等/豁免 → 不一致；
//! - `exit`：未装模式 → 退出码越界 → 行容量 → 相等/不一致；
//! - `verdict`：未装模式或零行。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed80;

/// 比对模式越档。
pub const CM_EMODE: u32 = SEG_BASE + 0x01;
/// 当前模式不允许该操作。
pub const CM_EWANT: u32 = SEG_BASE + 0x02;
/// 动态值未声明豁免。
pub const CM_EDYN: u32 = SEG_BASE + 0x03;
/// 比对不一致。
pub const CM_EMISS: u32 = SEG_BASE + 0x04;
/// 退出码越界。
pub const CM_EEXIT: u32 = SEG_BASE + 0x05;
/// 退出码不一致。
pub const CM_EEXITM: u32 = SEG_BASE + 0x06;
/// 行容量已满。
pub const CM_EFULL: u32 = SEG_BASE + 0x07;
/// 零行或未装模式，无法裁定。
pub const CM_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 比对行容量。
pub const CAP: u32 = 7;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 比对模式：全字节。
pub const MODE_BYTE: u32 = 0;
/// 比对模式：关键列。
pub const MODE_KEY: u32 = 1;
/// 比对模式：退出码。
pub const MODE_EXIT: u32 = 2;
/// 尚未装模式。
pub const MODE_NONE: u32 = 3;
/// 动态值哨兵。
pub const DYN: u32 = 0xffff_ffff;
/// 退出码上界。
pub const EXIT_MAX: u32 = 255;

/// 操作码：装模式。
pub const OP_ARM: u8 = 1;
/// 操作码：喂一行。
pub const OP_FEED: u8 = 2;
/// 操作码：比退出码。
pub const OP_EXIT: u8 = 3;
/// 操作码：裁定。
pub const OP_VERDICT: u8 = 4;

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

/// 三模比对引擎。
#[derive(Clone, Copy, Debug)]
pub struct Eng {
    mode: u32,
    rows: u32,
    miss: u32,
    exempt: u32,
}

impl Eng {
    /// 未装模式的空引擎。
    pub const fn new() -> Eng {
        Eng { mode: MODE_NONE, rows: 0, miss: 0, exempt: 0 }
    }

    /// 当前模式。
    pub fn mode(&self) -> u32 {
        self.mode
    }

    /// 已比对行数。
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// 不一致行数。
    pub fn miss(&self) -> u32 {
        self.miss
    }

    /// 动态值豁免数。
    pub fn exempt(&self) -> u32 {
        self.exempt
    }

    /// 装模式。
    pub fn arm(&mut self, mode: u32) -> Result<u32, u32> {
        if mode > MODE_EXIT {
            return Err(CM_EMODE);
        }
        self.mode = mode;
        Ok(mode)
    }

    /// 喂一行期望/实际值；`dyn_ok` 为真表示该行是已声明豁免的动态值。
    pub fn feed(&mut self, exp: u32, act: u32, dyn_ok: bool) -> Result<u32, u32> {
        if self.mode == MODE_NONE || self.mode == MODE_EXIT {
            return Err(CM_EWANT);
        }
        if self.rows >= CAP {
            return Err(CM_EFULL);
        }
        if exp == DYN && !dyn_ok {
            return Err(CM_EDYN);
        }
        if exp == act {
            self.rows = self.rows.saturating_add(1);
            return Ok(self.rows);
        }
        if exp == DYN {
            self.exempt = self.exempt.saturating_add(1);
            self.rows = self.rows.saturating_add(1);
            return Ok(self.rows);
        }
        self.miss = self.miss.saturating_add(1);
        Err(CM_EMISS)
    }

    /// 比对退出码。
    pub fn exit(&mut self, exp: u32, act: u32) -> Result<u32, u32> {
        if self.mode == MODE_NONE {
            return Err(CM_EWANT);
        }
        if exp > EXIT_MAX || act > EXIT_MAX {
            return Err(CM_EEXIT);
        }
        if self.rows >= CAP {
            return Err(CM_EFULL);
        }
        if exp == act {
            self.rows = self.rows.saturating_add(1);
            Ok(self.rows)
        } else {
            self.miss = self.miss.saturating_add(1);
            Err(CM_EEXITM)
        }
    }

    /// 裁定：至少要有一行，且零不一致。
    pub fn verdict(&self) -> Result<u32, u32> {
        if self.mode == MODE_NONE || self.rows == 0 {
            return Err(CM_EGAP);
        }
        if self.miss != 0 {
            return Err(CM_EMISS);
        }
        Ok(self.rows)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.mode as u64);
        h = fold(h, self.rows as u64);
        h = fold(h, self.miss as u64);
        fold(h, self.exempt as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        self.mode <= MODE_NONE
            && self.rows <= CAP
            && self.exempt <= self.rows
            && self.miss <= CAP
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
            return Err(CM_ESLOT);
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
            return Err(CM_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 3, 2, 0, 4, 5];
        const CODES: [u32; 6] = [CM_EDYN, CM_EMISS, CM_EWANT, CM_EEXITM, CM_EEXIT, CM_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CM_EBAL),
            };
            if s.slot as usize != i {
                return Err(CM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CM_EBAL);
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
            return Err(CM_EBAL);
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
            Err(CM_ESEAL)
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

/// 十四格账本：全字节三行 → 动态未豁免/不一致 → 退出码模式错位 →
/// 退出码不一致/越界 → 回到全字节补两行 → 行满。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut e = Eng::new();

    let _ = l.register(word_cell(0, OP_ARM, e.arm(MODE_BYTE)));
    let _ = l.register(word_cell(1, OP_FEED, e.feed(0xaa, 0xaa, false)));
    let _ = l.register(word_cell(2, OP_FEED, e.feed(0xbb, 0xbb, false)));
    let _ = l.register(word_cell(3, OP_FEED, e.feed(DYN, 0x1234, true)));
    let _ = l.register(word_cell(4, OP_FEED, e.feed(DYN, 0x1234, false)));
    let _ = l.register(word_cell(5, OP_FEED, e.feed(0x11, 0x22, false)));
    let _ = l.register(word_cell(6, OP_ARM, e.arm(MODE_EXIT)));
    let _ = l.register(word_cell(7, OP_FEED, e.feed(0xaa, 0xaa, false)));
    let _ = l.register(word_cell(8, OP_EXIT, e.exit(0, 1)));
    let _ = l.register(word_cell(9, OP_EXIT, e.exit(300, 0)));
    let _ = l.register(word_cell(10, OP_ARM, e.arm(MODE_BYTE)));
    let _ = l.register(word_cell(11, OP_FEED, e.feed(0xcc, 0xcc, false)));
    let _ = l.register(word_cell(12, OP_FEED, e.feed(0xdd, 0xdd, false)));

    let _ = e.feed(0xee, 0xee, false);
    let _ = e.feed(0xef, 0xef, false);
    let _ = l.register(word_cell(13, OP_FEED, e.feed(0xf0, 0xf0, false)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：全字节 + 动态豁免 + 裁定。
    let mut a = Eng::new();
    let m = a.arm(MODE_BYTE);
    let f1 = a.feed(0x10, 0x10, false);
    let f2 = a.feed(DYN, 0x99, true);
    let v = a.verdict();
    out[1] = m == Ok(MODE_BYTE)
        && f1 == Ok(1)
        && f2 == Ok(2)
        && v == Ok(2)
        && a.exempt() == 1
        && a.miss() == 0
        && a.rows() == 2
        && a.seal() != 0
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改行数。
    let mut b = Eng::new();
    let mode = b.arm(MODE_EXIT + 1) == Err(CM_EMODE);
    let want = b.feed(1, 1, false) == Err(CM_EWANT);
    let gap = b.verdict() == Err(CM_EGAP);
    let armed = b.arm(MODE_KEY) == Ok(MODE_KEY);
    let initial = b.rows() == 0;
    let dynb = b.feed(DYN, 7, false) == Err(CM_EDYN) && b.rows() == 0;
    let miss = b.feed(1, 2, false) == Err(CM_EMISS) && b.rows() == 0 && b.miss() == 1;
    let ok = b.feed(1, 1, false) == Ok(1);
    let _ = b.arm(MODE_EXIT);
    let exit_want = b.feed(1, 1, false) == Err(CM_EWANT);
    let range = b.exit(256, 0) == Err(CM_EEXIT);
    let em = b.exit(0, 4) == Err(CM_EEXITM) && b.rows() == 1;
    let mut c = Eng::new();
    let _ = c.arm(MODE_BYTE);
    let mut i = 0u32;
    while i < CAP {
        let _ = c.feed(1, 1, false);
        i += 1;
    }
    let full = c.feed(1, 1, false) == Err(CM_EFULL);
    out[2] = mode
        && want
        && gap
        && armed
        && initial
        && dynb
        && miss
        && ok
        && exit_want
        && range
        && em
        && full
        && b.consistent()
        && c.consistent();

    // 三路：行容量硬上限，换新引擎重新计。
    let mut q = Eng::new();
    let _ = q.arm(MODE_BYTE);
    let mut i = 0u32;
    while i < CAP {
        let _ = q.feed(0, 0, false);
        i += 1;
    }
    let q1 = q.rows() == CAP;
    let q2 = q.feed(0, 0, false) == Err(CM_EFULL);
    let q3 = q.verdict() == Ok(CAP);
    let mut fresh = Eng::new();
    let q4 = fresh.arm(MODE_BYTE) == Ok(MODE_BYTE) && fresh.feed(0, 0, false) == Ok(1);
    out[3] = q1 && q2 && q3 && q4 && q.consistent() && fresh.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11376_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11376.compare_modes");
    let l = build_ledger();

    s.add_dyn(
        "CM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CM2-全字节三模之一",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_BYTE);
            a.feed(0x11, 0x11, false) == Ok(1)
                && a.feed(0x12, 0x13, false) == Err(CM_EMISS)
                && a.rows() == 1
                && a.miss() == 1
                && a.verdict() == Err(CM_EMISS)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "CM3-动态值豁免",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_KEY);
            let f = a.feed(DYN, 0x7777, true);
            f == Ok(1)
                && a.exempt() == 1
                && a.miss() == 0
                && a.verdict() == Ok(1)
                && l.sample(3).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "CM4-动态值未豁免被拒",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_BYTE);
            a.feed(DYN, 0x7777, false) == Err(CM_EDYN)
                && a.rows() == 0
                && a.exempt() == 0
                && a.miss() == 0
                && l.sample(4).map(|x| x.code) == Some(CM_EDYN)
        },
        "static",
    );

    s.add(
        "CM5-退出码模式",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_EXIT);
            let bad = a.feed(0, 0, false) == Err(CM_EWANT);
            let good = a.exit(0, 0) == Ok(1);
            let wrong = a.exit(0, 2) == Err(CM_EEXITM);
            bad && good && wrong && a.rows() == 1 && a.miss() == 1 && a.consistent()
        },
        "static",
    );

    s.add(
        "CM6-退出码越界被拒",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_EXIT);
            a.exit(EXIT_MAX + 1, 0) == Err(CM_EEXIT)
                && a.exit(0, EXIT_MAX + 1) == Err(CM_EEXIT)
                && a.rows() == 0
                && l.sample(9).map(|x| x.code) == Some(CM_EEXIT)
        },
        "static",
    );

    s.add(
        "CM7-模式越档被拒",
        {
            let mut a = Eng::new();
            a.arm(MODE_EXIT + 1) == Err(CM_EMODE)
                && a.mode() == MODE_NONE
                && a.arm(MODE_BYTE) == Ok(MODE_BYTE)
                && a.mode() == MODE_BYTE
        },
        "static",
    );

    s.add(
        "CM8-未装模式不许比对",
        {
            let mut a = Eng::new();
            let f = a.feed(1, 1, false) == Err(CM_EWANT);
            let e = a.exit(0, 0) == Err(CM_EWANT);
            let v = a.verdict() == Err(CM_EGAP);
            f && e && v && a.rows() == 0 && a.consistent()
        },
        "static",
    );

    s.add(
        "CM9-不一致计数可查",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_BYTE);
            let _ = a.feed(1, 2, false);
            let _ = a.feed(3, 4, false);
            let _ = a.feed(5, 5, false);
            a.miss() == 2
                && a.rows() == 1
                && a.verdict() == Err(CM_EMISS)
                && l.sample(5).map(|x| x.code) == Some(CM_EMISS)
        },
        "static",
    );

    s.add(
        "CM10-零行不许裁定",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_KEY);
            a.verdict() == Err(CM_EGAP) && a.rows() == 0 && a.consistent()
        },
        "static",
    );

    s.add(
        "CM11-行容量硬上限",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_BYTE);
            let mut i = 0u32;
            while i < CAP {
                let _ = a.feed(0, 0, false);
                i += 1;
            }
            a.rows() == CAP
                && a.feed(0, 0, false) == Err(CM_EFULL)
                && a.verdict() == Ok(CAP)
                && a.consistent()
                && l.sample(13).map(|x| x.code) == Some(CM_EFULL)
        },
        "static",
    );

    s.add(
        "CM12-失败不改行数",
        {
            let mut a = Eng::new();
            let _ = a.arm(MODE_KEY);
            let _ = a.feed(1, 1, false);
            let _ = a.feed(DYN, 0, false);
            let _ = a.arm(MODE_EXIT + 5);
            let _ = a.feed(4, 5, false);
            a.rows() == 1
                && a.miss() == 1
                && a.exempt() == 0
                && a.mode() == MODE_KEY
                && a.verdict() == Err(CM_EMISS)
                && a.consistent()
        },
        "static"
    );

    s.add(
        "CM13-冻结重放四注入",
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
        let s = run_unx11376_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn byte_mode_mismatch() {
        let mut e = Eng::new();
        assert_eq!(e.arm(MODE_BYTE), Ok(MODE_BYTE));
        assert_eq!(e.feed(1, 1, false), Ok(1));
        assert_eq!(e.feed(2, 3, false), Err(CM_EMISS));
        assert_eq!(e.miss(), 1);
        assert_eq!(e.verdict(), Err(CM_EMISS));
    }

    #[test]
    fn dynamic_exemption() {
        let mut e = Eng::new();
        assert_eq!(e.arm(MODE_KEY), Ok(MODE_KEY));
        assert_eq!(e.feed(DYN, 42, true), Ok(1));
        assert_eq!(e.feed(DYN, 42, false), Err(CM_EDYN));
        assert_eq!(e.exempt(), 1);
        assert_eq!(e.verdict(), Ok(1));
    }

    #[test]
    fn exit_mode_only_exit() {
        let mut e = Eng::new();
        assert_eq!(e.arm(MODE_EXIT), Ok(MODE_EXIT));
        assert_eq!(e.feed(0, 0, false), Err(CM_EWANT));
        assert_eq!(e.exit(0, 0), Ok(1));
        assert_eq!(e.exit(1, 2), Err(CM_EEXITM));
        assert_eq!(e.exit(300, 0), Err(CM_EEXIT));
    }

    #[test]
    fn capacity_and_fresh_engine() {
        let mut e = Eng::new();
        let _ = e.arm(MODE_BYTE);
        let mut i = 0u32;
        while i < CAP {
            let _ = e.feed(0, 0, false);
            i += 1;
        }
        assert_eq!(e.feed(0, 0, false), Err(CM_EFULL));
        assert_eq!(e.verdict(), Ok(CAP));
        let mut f = Eng::new();
        assert_eq!(f.arm(MODE_BYTE), Ok(MODE_BYTE));
        assert_eq!(f.feed(0, 0, false), Ok(1));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
