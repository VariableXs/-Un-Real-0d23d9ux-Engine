//! UNX-F11415 cat/mv/chmod 退出码聚合规则
//!
//! 三件工具各有一套退出码字母表：`cat` 只用 0/1/2，`mv` 与 `chmod` 还会
//! 用 4 表示不可继续的致命失败。把一批调用聚合时，必须先把各工具的原始码
//! 归一成档位，再按「最坏档获胜」而不是「末条获胜」出总码：
//!
//! - 档位：0 通过、1 轻微、2 用法/部分失败、3 致命；
//! - 总码：0 `0`、1 `1`、2 `2`、3 `4`；
//! - 后续的成功记录不能把先前的失败洗白，最先失败的序号要单独留档。
//!
//! 批还带两个约束：**声明量**（必须恰好记录 `expect` 条，少一条不收）与
//! **熔断器**（一旦出现致命档，后续结果拒绝入账，防止在已经不可继续的链上
//! 继续累加假成功）。
//!
//! 闸门次序（`record`）：工具 → 序号范围 → 重复 → 倒序 → 码表 → 熔断 →
//! 声明量。`finalize` 只查声明量，`exit_code` 只读已入账档位。
//!
//! 账本契约：十四格，八成功六失败；失败覆盖工具/码/序号/重复/倒序/超记，
//! 少记与熔断在判据里单独注入。成功格记录累计条数、总码与最坏档。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeff0;

/// 未知工具号。
pub const EA_ETOOL: u32 = SEG_BASE + 0x01;
/// 退出码不在该工具的字母表内。
pub const EA_ECODE: u32 = SEG_BASE + 0x02;
/// 序号为零或超过上界。
pub const EA_ESEQ: u32 = SEG_BASE + 0x03;
/// 序号重复。
pub const EA_EDUP: u32 = SEG_BASE + 0x04;
/// 序号不升序。
pub const EA_EORDER: u32 = SEG_BASE + 0x05;
/// 记录数超过声明量。
pub const EA_EOVER: u32 = SEG_BASE + 0x06;
/// 记录数少于声明量。
pub const EA_EMISS: u32 = SEG_BASE + 0x07;
/// 熔断后仍试图入账。
pub const EA_EFUSE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const EA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const EA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const EA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const EA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const EA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 记录表容量。
pub const CAP: usize = 10;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 序号上界。
pub const SEQ_MAX: u16 = 99;

/// 工具：cat。
pub const TOOL_CAT: u8 = 1;
/// 工具：mv。
pub const TOOL_MV: u8 = 2;
/// 工具：chmod。
pub const TOOL_CHMOD: u8 = 3;

/// 档位：通过。
pub const TIER_OK: u8 = 0;
/// 档位：轻微。
pub const TIER_MINOR: u8 = 1;
/// 档位：用法/部分。
pub const TIER_USAGE: u8 = 2;
/// 档位：致命。
pub const TIER_FATAL: u8 = 3;

/// 操作码：登记一条结果。
pub const OP_REC: u8 = 1;
/// 操作码：收批。
pub const OP_FIN: u8 = 2;
/// 操作码：观察最坏档。
pub const OP_WORST: u8 = 3;
/// 操作码：观察首个失败序号。
pub const OP_FIRST: u8 = 4;

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

/// 工具 × 原始码 → 档位。工具非法归 `ETOOL`，码非法归 `ECODE`。
pub fn tier_of(tool: u8, code: u8) -> Result<u8, u32> {
    let legal = match tool {
        TOOL_CAT => code <= 2,
        TOOL_MV | TOOL_CHMOD => code == 0 || code == 1 || code == 2 || code == 4,
        _ => return Err(EA_ETOOL),
    };
    if !legal {
        return Err(EA_ECODE);
    }
    Ok(match code {
        0 => TIER_OK,
        1 => TIER_MINOR,
        2 => TIER_USAGE,
        _ => TIER_FATAL,
    })
}

/// 档位 → 总退出码。
pub fn exit_of(tier: u8) -> u32 {
    match tier {
        TIER_OK => 0,
        TIER_MINOR => 1,
        TIER_USAGE => 2,
        _ => 4,
    }
}

/// 一条已归一的调用结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    /// 工具。
    pub tool: u8,
    /// 全局序号。
    pub seq: u16,
    /// 原始退出码。
    pub code: u8,
    /// 归一档位。
    pub tier: u8,
}

/// 一批调用结果。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    expect: u8,
    fuse: bool,
    rows: [Rec; CAP],
    n: usize,
    fused: bool,
}

impl Batch {
    /// 开户：声明量必须落在 `1..=CAP`，否则归 `EOVER`。
    pub fn open(expect: u8, fuse: bool) -> Result<Batch, u32> {
        if expect == 0 || expect as usize > CAP {
            return Err(EA_EOVER);
        }
        Ok(Batch {
            expect,
            fuse,
            rows: [Rec { tool: 0, seq: 0, code: 0, tier: 0 }; CAP],
            n: 0,
            fused: false,
        })
    }

    /// 声明量。
    pub fn expect(&self) -> usize {
        self.expect as usize
    }

    /// 已入账条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已熔断。
    pub fn fused(&self) -> bool {
        self.fused
    }

    /// 第 i 条。
    pub fn rec_at(&self, i: usize) -> Option<Rec> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 最后一条序号。
    pub fn last_seq(&self) -> Option<u16> {
        if self.n == 0 {
            return None;
        }
        self.rows.get(self.n - 1).map(|r| r.seq)
    }

    /// 序号是否已存在。
    fn has_seq(&self, seq: u16) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.seq == seq {
                    return true;
                }
            }
            i = i.saturating_add(1);
        }
        false
    }

    /// 登记一条结果，返回累计条数。
    ///
    /// 闸门次序：工具 → 序号范围 → 重复 → 倒序 → 码表 → 熔断 → 声明量。
    pub fn record(&mut self, tool: u8, seq: u16, code: u8) -> Result<usize, u32> {
        if tool != TOOL_CAT && tool != TOOL_MV && tool != TOOL_CHMOD {
            return Err(EA_ETOOL);
        }
        if seq == 0 || seq > SEQ_MAX {
            return Err(EA_ESEQ);
        }
        if self.has_seq(seq) {
            return Err(EA_EDUP);
        }
        if let Some(last) = self.last_seq() {
            if seq <= last {
                return Err(EA_EORDER);
            }
        }
        let tier = tier_of(tool, code)?;
        if self.fused {
            return Err(EA_EFUSE);
        }
        if self.n >= self.expect as usize {
            return Err(EA_EOVER);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Rec { tool, seq, code, tier };
        }
        self.n = self.n.saturating_add(1);
        if tier == TIER_FATAL && self.fuse {
            self.fused = true;
        }
        Ok(self.n)
    }

    /// 失败条数。
    pub fn fail_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.tier != TIER_OK {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        k
    }

    /// 首个失败的序号。
    pub fn first_fail(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.tier != TIER_OK {
                    return Some(r.seq);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 最坏档。
    pub fn worst(&self) -> u8 {
        let mut w = TIER_OK;
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.tier > w {
                    w = r.tier;
                }
            }
            i = i.saturating_add(1);
        }
        w
    }

    /// 聚合总码：最坏档映射，不被末条成功洗白。
    pub fn exit_code(&self) -> u32 {
        exit_of(self.worst())
    }

    /// 收批：条数必须恰为声明量。
    pub fn finalize(&self) -> Result<u32, u32> {
        if self.n != self.expect as usize {
            return Err(EA_EMISS);
        }
        Ok(self.exit_code())
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.expect == 0 || self.expect as usize > CAP || self.n > self.expect as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            match tier_of(r.tool, r.code) {
                Ok(t) => {
                    if t != r.tier {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            if i > 0 {
                match self.rows.get(i - 1) {
                    Some(prev) => {
                        if prev.seq >= r.seq {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            i = i.saturating_add(1);
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
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EA_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
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
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
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
            return Err(EA_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 2, 2];
        const CODES: [u32; 6] =
            [EA_ETOOL, EA_ECODE, EA_ESEQ, EA_EDUP, EA_EORDER, EA_EOVER];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(EA_EBAL),
            };
            if v.slot as usize != i {
                return Err(EA_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(EA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(EA_EVALUE);
                        }
                    }
                    None => return Err(EA_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(EA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EA_EBAL);
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
            Err(EA_ESEAL)
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

/// 十四格账本：六条记录 → 收批 → 最坏档 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = match Batch::open(6, false) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_REC, b.record(TOOL_CAT, 1, 0)));
    let _ = l.register(cell(1, OP_REC, b.record(TOOL_MV, 2, 1)));
    let _ = l.register(cell(2, OP_REC, b.record(TOOL_CHMOD, 3, 0)));
    let _ = l.register(cell(3, OP_REC, b.record(TOOL_CAT, 4, 2)));
    let _ = l.register(cell(4, OP_REC, b.record(TOOL_MV, 5, 0)));
    let _ = l.register(cell(5, OP_REC, b.record(TOOL_CHMOD, 7, 1)));
    let _ = l.register(word_cell(6, OP_FIN, b.finalize()));
    let _ = l.register(byte_cell(7, OP_WORST, Ok(b.worst())));

    let _ = l.register(cell(8, OP_REC, b.record(9, 8, 0)));
    let _ = l.register(cell(9, OP_REC, b.record(TOOL_CAT, 8, 3)));
    let _ = l.register(cell(10, OP_REC, b.record(TOOL_CAT, 0, 0)));
    let _ = l.register(cell(11, OP_REC, b.record(TOOL_CAT, 7, 0)));
    let _ = l.register(cell(12, OP_REC, b.record(TOOL_CAT, 6, 0)));
    let _ = l.register(cell(13, OP_REC, b.record(TOOL_CAT, 8, 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：最坏档获胜，末条成功不洗白，首个失败可定位。
    let mut a = match Batch::open(4, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = a.record(TOOL_CAT, 1, 0);
    let _ = a.record(TOOL_MV, 2, 4);
    let _ = a.record(TOOL_CHMOD, 3, 0);
    let _ = a.record(TOOL_CAT, 4, 0);
    out[1] = a.count() == 4
        && a.finalize() == Ok(4)
        && a.worst() == TIER_FATAL
        && a.first_fail() == Some(2)
        && a.fail_count() == 1
        && a.last_seq() == Some(4)
        && a.consistent();

    // 二路：八类失败各自可辨。
    let open_zero = matches!(Batch::open(0, false), Err(EA_EOVER));
    let open_big = matches!(Batch::open((CAP + 1) as u8, false), Err(EA_EOVER));
    let mut b = match Batch::open(3, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let tool = b.record(9, 1, 0) == Err(EA_ETOOL);
    let code = b.record(TOOL_CAT, 1, 4) == Err(EA_ECODE);
    let seq_zero = b.record(TOOL_CAT, 0, 0) == Err(EA_ESEQ);
    let seq_high = b.record(TOOL_CAT, SEQ_MAX + 1, 0) == Err(EA_ESEQ);
    let ok1 = b.record(TOOL_MV, 1, 0) == Ok(1);
    let dup = b.record(TOOL_CAT, 1, 0) == Err(EA_EDUP);
    let _ = b.record(TOOL_CHMOD, 3, 0);
    let lower = b.record(TOOL_CAT, 2, 0) == Err(EA_EORDER);
    let _ = b.record(TOOL_CAT, 4, 0);
    let over = b.record(TOOL_CAT, 5, 0) == Err(EA_EOVER);
    let mut c = match Batch::open(2, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = c.record(TOOL_CAT, 1, 0);
    let miss = c.finalize() == Err(EA_EMISS);
    let mut d = match Batch::open(3, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = d.record(TOOL_MV, 1, 4);
    let fused = d.fused() && d.record(TOOL_CHMOD, 2, 0) == Err(EA_EFUSE);
    out[2] = open_zero
        && open_big
        && tool
        && code
        && seq_zero
        && seq_high
        && ok1
        && dup
        && lower
        && over
        && miss
        && fused;

    // 三路：三工具码表、档位映射与熔断边界。
    let cat_ok = tier_of(TOOL_CAT, 0) == Ok(TIER_OK)
        && tier_of(TOOL_CAT, 1) == Ok(TIER_MINOR)
        && tier_of(TOOL_CAT, 2) == Ok(TIER_USAGE)
        && tier_of(TOOL_CAT, 4) == Err(EA_ECODE);
    let mv_ok = tier_of(TOOL_MV, 4) == Ok(TIER_FATAL)
        && tier_of(TOOL_MV, 3) == Err(EA_ECODE)
        && tier_of(TOOL_CHMOD, 4) == Ok(TIER_FATAL)
        && tier_of(TOOL_CHMOD, 3) == Err(EA_ECODE)
        && tier_of(9, 0) == Err(EA_ETOOL);
    let map = exit_of(TIER_OK) == 0
        && exit_of(TIER_MINOR) == 1
        && exit_of(TIER_USAGE) == 2
        && exit_of(TIER_FATAL) == 4;
    let mut q = match Batch::open(3, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = q.record(TOOL_CAT, 1, 1);
    let soft = !q.fused();
    let _ = q.record(TOOL_CAT, 2, 2);
    let still = !q.fused();
    let _ = q.record(TOOL_MV, 3, 4);
    let armed = q.fused();
    out[3] = cat_ok && mv_ok && map && soft && still && armed && q.finalize() == Ok(4);

    out
}

/// 十三项判据。
pub fn run_unx11415_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11415.cat_mv_chmod_exit_aggregate");
    let l = build_ledger();

    s.add_dyn(
        "EA1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "EA2-累计与最坏档",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(5).map(|x| x.value) == Some(6)
            && l.sample(6).map(|x| x.value) == Some(2)
            && l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "EA3-末条成功不洗白",
        {
            let mut b = match Batch::open(3, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.record(TOOL_CAT, 1, 2);
            let _ = b.record(TOOL_CAT, 2, 0);
            let _ = b.record(TOOL_MV, 3, 0);
            b.finalize() == Ok(2)
                && b.worst() == TIER_USAGE
                && b.exit_code() == 2
                && b.fail_count() == 1
        },
        "static",
    );

    s.add(
        "EA4-首个失败与计数",
        {
            let mut b = match Batch::open(4, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.record(TOOL_CAT, 1, 0);
            let _ = b.record(TOOL_MV, 2, 1);
            let _ = b.record(TOOL_CHMOD, 3, 4);
            let _ = b.record(TOOL_CAT, 4, 2);
            b.first_fail() == Some(2)
                && b.fail_count() == 3
                && b.worst() == TIER_FATAL
                && b.finalize() == Ok(4)
        },
        "static",
    );

    s.add(
        "EA5-三工具码表",
        {
            tier_of(TOOL_CAT, 2) == Ok(TIER_USAGE)
                && tier_of(TOOL_CAT, 4) == Err(EA_ECODE)
                && tier_of(TOOL_MV, 4) == Ok(TIER_FATAL)
                && tier_of(TOOL_MV, 3) == Err(EA_ECODE)
                && tier_of(TOOL_CHMOD, 4) == Ok(TIER_FATAL)
                && tier_of(TOOL_CHMOD, 5) == Err(EA_ECODE)
                && tier_of(0, 0) == Err(EA_ETOOL)
                && exit_of(TIER_OK) == 0
                && exit_of(TIER_FATAL) == 4
                && l.sample(8).map(|x| x.code) == Some(EA_ETOOL)
        },
        "static",
    );

    s.add(
        "EA6-序号范围闸门",
        {
            let mut b = match Batch::open(3, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            b.record(TOOL_CAT, 0, 0) == Err(EA_ESEQ)
                && b.record(TOOL_CAT, SEQ_MAX + 1, 0) == Err(EA_ESEQ)
                && b.record(TOOL_CAT, 1, 0) == Ok(1)
                && b.last_seq() == Some(1)
                && l.sample(10).map(|x| x.code) == Some(EA_ESEQ)
        },
        "static",
    );

    s.add(
        "EA7-重复优先于倒序",
        {
            let mut b = match Batch::open(4, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.record(TOOL_CAT, 5, 0);
            let dup = b.record(TOOL_CAT, 5, 0) == Err(EA_EDUP);
            let order = b.record(TOOL_MV, 4, 0) == Err(EA_EORDER);
            let next = b.record(TOOL_MV, 6, 0) == Ok(2);
            dup && order
                && next
                && b.consistent()
                && l.sample(11).map(|x| x.code) == Some(EA_EDUP)
                && l.sample(12).map(|x| x.code) == Some(EA_EORDER)
        },
        "static",
    );

    s.add(
        "EA8-声明量开户闸门",
        {
            matches!(Batch::open(0, false), Err(EA_EOVER))
                && matches!(Batch::open((CAP + 1) as u8, false), Err(EA_EOVER))
                && Batch::open(CAP as u8, false).is_ok()
                && Batch::open(1, true).is_ok()
        },
        "static",
    );

    s.add(
        "EA9-多记被拒",
        {
            let mut b = match Batch::open(2, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.record(TOOL_CAT, 1, 0);
            let _ = b.record(TOOL_MV, 2, 0);
            b.record(TOOL_CHMOD, 3, 0) == Err(EA_EOVER)
                && b.count() == 2
                && b.finalize() == Ok(0)
                && l.sample(13).map(|x| x.code) == Some(EA_EOVER)
        },
        "static",
    );

    s.add(
        "EA10-少记不收",
        {
            let mut b = match Batch::open(3, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let empty = b.finalize() == Err(EA_EMISS);
            let _ = b.record(TOOL_CAT, 1, 0);
            let _ = b.record(TOOL_MV, 2, 0);
            let short = b.finalize() == Err(EA_EMISS);
            let _ = b.record(TOOL_CHMOD, 3, 0);
            empty && short && b.finalize() == Ok(0)
        },
        "static",
    );

    s.add(
        "EA11-熔断器只认致命档",
        {
            let mut soft = match Batch::open(4, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = soft.record(TOOL_CAT, 1, 1);
            let _ = soft.record(TOOL_MV, 2, 2);
            let not_yet = !soft.fused();
            let _ = soft.record(TOOL_CHMOD, 3, 4);
            let armed = soft.fused();
            let blocked = soft.record(TOOL_CAT, 4, 0) == Err(EA_EFUSE);
            let mut hard = match Batch::open(3, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = hard.record(TOOL_MV, 1, 4);
            let _ = hard.record(TOOL_CAT, 2, 0);
            not_yet && armed && blocked && !hard.fused() && hard.count() == 2
        },
        "static",
    );

    s.add(
        "EA12-聚合不变式",
        {
            let mut b = match Batch::open(5, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.record(TOOL_CAT, 1, 0);
            let _ = b.record(TOOL_MV, 2, 1);
            let _ = b.record(TOOL_CHMOD, 3, 2);
            let _ = b.record(TOOL_MV, 4, 4);
            let _ = b.record(TOOL_MV, 5, 0);
            b.consistent()
                && b.exit_code() == 4
                && b.finalize() == Ok(4)
                && b.fail_count() == 3
                && b.first_fail() == Some(2)
                && b.rec_at(4) == Some(Rec { tool: TOOL_MV, seq: 5, code: 0, tier: TIER_OK })
                && b.rec_at(5).is_none()
        },
        "static",
    );

    s.add(
        "EA13-冻结重放四注入",
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

    fn batch(expect: u8, fuse: bool) -> Batch {
        match Batch::open(expect, fuse) {
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
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11415_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn worst_wins() {
        let mut b = batch(3, false);
        assert_eq!(b.record(TOOL_CAT, 1, 0), Ok(1));
        assert_eq!(b.record(TOOL_MV, 2, 4), Ok(2));
        assert_eq!(b.record(TOOL_CAT, 3, 0), Ok(3));
        assert_eq!(b.finalize(), Ok(4));
        assert_eq!(b.worst(), TIER_FATAL);
        assert_eq!(b.first_fail(), Some(2));
        assert!(b.consistent());
    }

    #[test]
    fn tool_tables() {
        assert_eq!(tier_of(TOOL_CAT, 2), Ok(TIER_USAGE));
        assert_eq!(tier_of(TOOL_CAT, 4), Err(EA_ECODE));
        assert_eq!(tier_of(TOOL_MV, 4), Ok(TIER_FATAL));
        assert_eq!(tier_of(TOOL_CHMOD, 3), Err(EA_ECODE));
        assert_eq!(tier_of(9, 0), Err(EA_ETOOL));
    }

    #[test]
    fn sequence_gates() {
        let mut b = batch(4, false);
        assert_eq!(b.record(TOOL_CAT, 0, 0), Err(EA_ESEQ));
        assert_eq!(b.record(TOOL_CAT, 5, 0), Ok(1));
        assert_eq!(b.record(TOOL_CAT, 5, 0), Err(EA_EDUP));
        assert_eq!(b.record(TOOL_CAT, 4, 0), Err(EA_EORDER));
        assert_eq!(b.record(TOOL_CAT, 6, 0), Ok(2));
    }

    #[test]
    fn declared_count() {
        let mut b = batch(2, false);
        assert_eq!(b.finalize(), Err(EA_EMISS));
        assert_eq!(b.record(TOOL_CAT, 1, 0), Ok(1));
        assert_eq!(b.record(TOOL_MV, 2, 0), Ok(2));
        assert_eq!(b.finalize(), Ok(0));
        assert_eq!(b.record(TOOL_CHMOD, 3, 0), Err(EA_EOVER));
        assert!(matches!(Batch::open(0, false), Err(EA_EOVER)));
    }

    #[test]
    fn fuse_only_on_fatal() {
        let mut b = batch(4, true);
        assert_eq!(b.record(TOOL_CAT, 1, 1), Ok(1));
        assert!(!b.fused());
        assert_eq!(b.record(TOOL_MV, 2, 4), Ok(2));
        assert!(b.fused());
        assert_eq!(b.record(TOOL_CHMOD, 3, 0), Err(EA_EFUSE));
        assert_eq!(b.count(), 2);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
