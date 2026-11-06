//! UNX-F11419 三工具判据双机同码验证
//!
//! cat/mv/chmod 的判据不能只在一条机器上自说自话。本册把同一工具、同一序号
//! 的观测码同时记在 A/B 两台机器上：两码相等才算同码，同码才允许入格；两码
//! 不等记 `EDIFF`，只累加差异计数，不占格。这样「双机一致」不是一句口号，
//! 而是每一格都能回查的硬条件。
//!
//! 轮次（epoch）把一次验证与下一次验证隔开：epoch 为零的账是废账，任何一条
//! 观测都先被 `ESTALE` 挡下，避免上一轮的残留码冒充本轮结果。
//!
//! 闸门次序（`record`）：轮次 → 工具 → 序号 → 分组次序 → 重复 → 双机同码
//! → 网格容量。分组次序只要求工具号不回退（cat → mv → chmod），同一工具内
//! 序号可以任意排列；重复由 `(tool, idx)` 唯一性单独判定，这样「乱序」与
//! 「重复」不会互相遮蔽。
//!
//! 收册（`finish`）要求三件工具各至少 `WANT` 格：只验 cat 的账不能宣称三工具
//! 已同码，覆盖不足记 `EGAP`。
//!
//! 账本契约：十四格，八成功六失败；成功格记录入格计数、收册条数与 mv 覆盖，
//! 失败格覆盖过期/工具/序号/重复/异码/表满六类；乱序与覆盖不足在判据和注入
//! 里单独验证。成功值 `[1,2,3,4,5,6,6,2]`，失败码依次为
//! `ESTALE/ETOOL/EIDX/EDUP/EDIFF/EFULL`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf030;

/// 轮次过期或为零。
pub const DM_ESTALE: u32 = SEG_BASE + 0x01;
/// 未知工具号。
pub const DM_ETOOL: u32 = SEG_BASE + 0x02;
/// 序号为零或越界。
pub const DM_EIDX: u32 = SEG_BASE + 0x03;
/// 同机同工具同序号重复入格。
pub const DM_EDUP: u32 = SEG_BASE + 0x04;
/// 双机观测码不一致。
pub const DM_EDIFF: u32 = SEG_BASE + 0x05;
/// 工具分组次序回退。
pub const DM_EORDER: u32 = SEG_BASE + 0x06;
/// 三工具覆盖不足，不能收册。
pub const DM_EGAP: u32 = SEG_BASE + 0x07;
/// 双机网格已满。
pub const DM_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 双机网格容量。
pub const CAP: usize = 12;
/// 单工具序号上界。
pub const IDX_MAX: u8 = 8;
/// 每工具最少格数。
pub const WANT: u8 = 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 工具：cat。
pub const TOOL_CAT: u8 = 1;
/// 工具：mv。
pub const TOOL_MV: u8 = 2;
/// 工具：chmod。
pub const TOOL_CHMOD: u8 = 3;

/// 操作码：开账与轮次闸门。
pub const OP_OPEN: u8 = 1;
/// 操作码：双机入格。
pub const OP_REC: u8 = 2;
/// 操作码：收册。
pub const OP_FIN: u8 = 3;
/// 操作码：覆盖计数。
pub const OP_TOOL: u8 = 4;

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

/// 工具号是否合法。
fn tool_ok(tool: u8) -> bool {
    tool == TOOL_CAT || tool == TOOL_MV || tool == TOOL_CHMOD
}

/// 一格双机观测：同一工具同一序号在 A/B 两机的观测码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 工具号。
    pub tool: u8,
    /// 序号。
    pub idx: u8,
    /// A 机观测码。
    pub a: u32,
    /// B 机观测码。
    pub b: u32,
}

/// 双机网格。
#[derive(Clone, Copy, Debug)]
pub struct Dual {
    epoch: u32,
    rows: [Cell; CAP],
    n: usize,
    last_tool: u8,
    diff: u32,
}

impl Dual {
    /// 开一轮空白网格。
    pub const fn open(epoch: u32) -> Dual {
        Dual {
            epoch,
            rows: [Cell { tool: 0, idx: 0, a: 0, b: 0 }; CAP],
            n: 0,
            last_tool: 0,
            diff: 0,
        }
    }

    /// 轮次闸门：零轮次为废账。
    pub fn stamp(epoch: u32) -> Result<u32, u32> {
        if epoch == 0 {
            Err(DM_ESTALE)
        } else {
            Ok(epoch)
        }
    }

    /// 本轮轮次。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 已入格数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空格。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 格。
    pub fn cell_at(&self, i: usize) -> Option<Cell> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 差异计数。
    pub fn diff_count(&self) -> u32 {
        self.diff
    }

    /// 查同工具同序号的格。
    fn find(&self, tool: u8, idx: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.tool == tool && c.idx == idx {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 某工具已入格数。
    pub fn per_tool(&self, tool: u8) -> Result<u8, u32> {
        if !tool_ok(tool) {
            return Err(DM_ETOOL);
        }
        let mut k = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.tool == tool {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        Ok(k)
    }

    /// 双机入格。
    ///
    /// 闸门次序：轮次 → 工具 → 序号 → 分组次序 → 重复 → 双机同码 → 表满。
    pub fn record(&mut self, tool: u8, idx: u8, a: u32, b: u32) -> Result<usize, u32> {
        if self.epoch == 0 {
            return Err(DM_ESTALE);
        }
        if !tool_ok(tool) {
            return Err(DM_ETOOL);
        }
        if idx == 0 || idx > IDX_MAX {
            return Err(DM_EIDX);
        }
        if tool < self.last_tool {
            return Err(DM_EORDER);
        }
        if self.find(tool, idx).is_some() {
            return Err(DM_EDUP);
        }
        if a != b {
            self.diff = self.diff.saturating_add(1);
            return Err(DM_EDIFF);
        }
        if self.n >= CAP {
            return Err(DM_EFULL);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Cell { tool, idx, a, b };
        }
        self.n = self.n.saturating_add(1);
        self.last_tool = tool;
        Ok(self.n)
    }

    /// 收册：三工具各至少 `WANT` 格。
    pub fn finish(&self) -> Result<u32, u32> {
        let mut tool = TOOL_CAT;
        while tool <= TOOL_CHMOD {
            match self.per_tool(tool) {
                Ok(k) => {
                    if k < WANT {
                        return Err(DM_EGAP);
                    }
                }
                Err(c) => return Err(c),
            }
            tool = tool.saturating_add(1);
        }
        Ok(self.n as u32)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.epoch == 0 || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let c = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !tool_ok(c.tool) || c.idx == 0 || c.idx > IDX_MAX || c.a != c.b {
                return false;
            }
            if i > 0 {
                if let Some(p) = self.rows.get(i.saturating_sub(1)) {
                    if c.tool < p.tool {
                        return false;
                    }
                }
            }
            let mut j = i.saturating_add(1);
            while j < self.n {
                if let Some(w) = self.rows.get(j) {
                    if w.tool == c.tool && w.idx == c.idx {
                        return false;
                    }
                }
                j = j.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if self.n > 0 {
            if let Some(last) = self.rows.get(self.n.saturating_sub(1)) {
                if self.last_tool != last.tool {
                    return false;
                }
            }
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
            return Err(DM_ESLOT);
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
            return Err(DM_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 6, 2];
        const CODES: [u32; 6] =
            [DM_ESTALE, DM_ETOOL, DM_EIDX, DM_EDUP, DM_EDIFF, DM_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(DM_EBAL),
            };
            if v.slot as usize != i {
                return Err(DM_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(DM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(DM_EVALUE);
                        }
                    }
                    None => return Err(DM_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(DM_EVALUE);
                        }
                    }
                    None => return Err(DM_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(DM_EBAL);
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
            return Err(DM_EBAL);
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
            Err(DM_ESEAL)
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

/// 十四格账本：正路六格 → 收册 → 覆盖 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut d = Dual::open(7);

    let _ = l.register(cell(0, OP_REC, d.record(TOOL_CAT, 1, 0, 0)));
    let _ = l.register(cell(1, OP_REC, d.record(TOOL_CAT, 2, 1, 1)));
    let _ = l.register(cell(2, OP_REC, d.record(TOOL_MV, 1, 2, 2)));
    let _ = l.register(cell(3, OP_REC, d.record(TOOL_MV, 2, 1, 1)));
    let _ = l.register(cell(4, OP_REC, d.record(TOOL_CHMOD, 1, 0, 0)));
    let _ = l.register(cell(5, OP_REC, d.record(TOOL_CHMOD, 2, 2, 2)));
    let _ = l.register(word_cell(6, OP_FIN, d.finish()));
    let _ = l.register(byte_cell(7, OP_TOOL, d.per_tool(TOOL_MV)));

    let _ = l.register(word_cell(8, OP_OPEN, Dual::stamp(0)));

    let mut t = Dual::open(7);
    let _ = l.register(cell(9, OP_REC, t.record(9, 1, 0, 0)));

    let mut i = Dual::open(7);
    let _ = l.register(cell(10, OP_REC, i.record(TOOL_CAT, 0, 0, 0)));

    let mut u = Dual::open(7);
    let _ = u.record(TOOL_CAT, 1, 0, 0);
    let _ = l.register(cell(11, OP_REC, u.record(TOOL_CAT, 1, 0, 0)));

    let mut f = Dual::open(7);
    let _ = l.register(cell(12, OP_REC, f.record(TOOL_MV, 1, 0, 2)));

    let mut g = Dual::open(7);
    let mut k = 1u8;
    while k <= IDX_MAX {
        let _ = g.record(TOOL_CAT, k, 0, 0);
        k = k.saturating_add(1);
    }
    let mut k = 1u8;
    while k <= 4 {
        let _ = g.record(TOOL_MV, k, 0, 0);
        k = k.saturating_add(1);
    }
    let _ = l.register(cell(13, OP_REC, g.record(TOOL_CHMOD, 1, 0, 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三工具各两格，同码收册。
    let mut a = Dual::open(9);
    let r1 = a.record(TOOL_CAT, 1, 0, 0);
    let r2 = a.record(TOOL_CAT, 2, 1, 1);
    let r3 = a.record(TOOL_MV, 1, 2, 2);
    let r4 = a.record(TOOL_MV, 2, 1, 1);
    let r5 = a.record(TOOL_CHMOD, 1, 0, 0);
    let r6 = a.record(TOOL_CHMOD, 2, 2, 2);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && r4 == Ok(4)
        && r5 == Ok(5)
        && r6 == Ok(6)
        && a.len() == 6
        && a.epoch() == 9
        && a.per_tool(TOOL_CAT) == Ok(2)
        && a.per_tool(TOOL_MV) == Ok(2)
        && a.per_tool(TOOL_CHMOD) == Ok(2)
        && a.finish() == Ok(6)
        && a.diff_count() == 0
        && a.cell_at(0) == Some(Cell { tool: TOOL_CAT, idx: 1, a: 0, b: 0 })
        && a.cell_at(5) == Some(Cell { tool: TOOL_CHMOD, idx: 2, a: 2, b: 2 })
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Dual::open(5);
    let tool = b.record(9, 1, 0, 0) == Err(DM_ETOOL);
    let idx_zero = b.record(TOOL_CAT, 0, 0, 0) == Err(DM_EIDX);
    let idx_high = b.record(TOOL_CAT, IDX_MAX + 1, 0, 0) == Err(DM_EIDX);
    let _ = b.record(TOOL_MV, 1, 0, 0);
    let order = b.record(TOOL_CAT, 2, 0, 0) == Err(DM_EORDER);
    let stale = Dual::open(0).record(TOOL_CAT, 1, 0, 0) == Err(DM_ESTALE);
    let mut c = Dual::open(5);
    let _ = c.record(TOOL_CAT, 1, 0, 0);
    let dup = c.record(TOOL_CAT, 1, 0, 0) == Err(DM_EDUP);
    let mut d = Dual::open(5);
    let before = d.len();
    let diff = d.record(TOOL_MV, 1, 1, 2) == Err(DM_EDIFF);
    let diff_kept = d.len() == before && d.diff_count() == 1;
    let mut e = Dual::open(5);
    let _ = e.record(TOOL_CAT, 1, 0, 0);
    let _ = e.record(TOOL_CAT, 2, 0, 0);
    let gap = e.finish() == Err(DM_EGAP);
    let mut f = Dual::open(5);
    let mut k = 1u8;
    while k <= IDX_MAX {
        let _ = f.record(TOOL_CAT, k, 0, 0);
        k = k.saturating_add(1);
    }
    let mut k = 1u8;
    while k <= 4 {
        let _ = f.record(TOOL_MV, k, 0, 0);
        k = k.saturating_add(1);
    }
    let full = f.record(TOOL_CHMOD, 1, 0, 0) == Err(DM_EFULL) && f.len() == CAP;
    out[2] = tool
        && idx_zero
        && idx_high
        && order
        && stale
        && dup
        && diff
        && diff_kept
        && gap
        && full;

    // 三路：同码入格、异码留痕、工具不回退。
    let mut p = Dual::open(11);
    let _ = p.record(TOOL_CAT, 1, 0, 0);
    let _ = p.record(TOOL_CAT, 2, 1, 1);
    let _ = p.record(TOOL_MV, 1, 2, 2);
    let _ = p.record(TOOL_MV, 2, 0, 0);
    let _ = p.record(TOOL_CHMOD, 1, 1, 1);
    let _ = p.record(TOOL_CHMOD, 2, 2, 2);
    let same = p.consistent()
        && p.finish() == Ok(6)
        && p.per_tool(TOOL_MV) == Ok(2)
        && p.per_tool(0) == Err(DM_ETOOL)
        && p.cell_at(3) == Some(Cell { tool: TOOL_MV, idx: 2, a: 0, b: 0 })
        && p.epoch() == 11;
    let mut q = Dual::open(11);
    let _ = q.record(TOOL_CAT, 1, 0, 0);
    let bad = q.record(TOOL_CAT, 2, 0, 1) == Err(DM_EDIFF)
        && q.len() == 1
        && q.diff_count() == 1
        && q.consistent();
    let mut r = Dual::open(11);
    let _ = r.record(TOOL_CHMOD, 1, 0, 0);
    let back = r.record(TOOL_CAT, 1, 0, 0) == Err(DM_EORDER);
    let nostale = !Dual::open(0).consistent();
    out[3] = same && bad && back && nostale;

    out
}

/// 十三项判据。
pub fn run_unx11419_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11419.three_tool_dual_machine");
    let l = build_ledger();

    s.add_dyn(
        "DM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DM2-入格计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4)
            && l.sample(4).map(|x| x.value) == Some(5)
            && l.sample(5).map(|x| x.value) == Some(6)
            && l.sample(0).map(|x| x.op) == Some(OP_REC),
        "static",
    );

    s.add(
        "DM3-收册与覆盖计数",
        l.sample(6).map(|x| x.value) == Some(6)
            && l.sample(6).map(|x| x.op) == Some(OP_FIN)
            && l.sample(7).map(|x| x.value) == Some(2)
            && l.sample(7).map(|x| x.op) == Some(OP_TOOL),
        "static",
    );

    s.add(
        "DM4-六类失败归位",
        l.sample(8).map(|x| x.code) == Some(DM_ESTALE)
            && l.sample(9).map(|x| x.code) == Some(DM_ETOOL)
            && l.sample(10).map(|x| x.code) == Some(DM_EIDX)
            && l.sample(11).map(|x| x.code) == Some(DM_EDUP)
            && l.sample(12).map(|x| x.code) == Some(DM_EDIFF)
            && l.sample(13).map(|x| x.code) == Some(DM_EFULL)
            && l.sample(8).map(|x| x.ok) == Some(false)
            && l.sample(13).map(|x| x.ok) == Some(false),
        "static",
    );

    s.add(
        "DM5-工具与序号闸门",
        {
            let mut a = Dual::open(4);
            let t1 = a.record(9, 1, 0, 0) == Err(DM_ETOOL);
            let t2 = a.record(0, 1, 0, 0) == Err(DM_ETOOL);
            let t3 = a.per_tool(9) == Err(DM_ETOOL);
            let i1 = a.record(TOOL_CAT, 0, 0, 0) == Err(DM_EIDX);
            let i2 = a.record(TOOL_CAT, IDX_MAX + 1, 0, 0) == Err(DM_EIDX);
            t1 && t2 && t3 && i1 && i2 && a.is_empty() && a.consistent()
        },
        "static",
    );

    s.add(
        "DM6-工具分组不回退",
        {
            let mut a = Dual::open(4);
            let c1 = a.record(TOOL_MV, 1, 0, 0);
            let bad = a.record(TOOL_CAT, 1, 0, 0) == Err(DM_EORDER);
            let c2 = a.record(TOOL_MV, 2, 0, 0) == Ok(2);
            let c3 = a.record(TOOL_CHMOD, 1, 0, 0) == Ok(3);
            c1 == Ok(1) && bad && c2 && c3 && a.consistent() && a.finish() == Err(DM_EGAP)
        },
        "static",
    );

    s.add(
        "DM7-重复与异码留痕",
        {
            let mut a = Dual::open(4);
            let _ = a.record(TOOL_CAT, 1, 0, 0);
            let dup = a.record(TOOL_CAT, 1, 0, 0) == Err(DM_EDUP);
            let mut b = Dual::open(4);
            let diff = b.record(TOOL_CAT, 1, 5, 6) == Err(DM_EDIFF);
            let mut c = Dual::open(4);
            let _ = c.record(TOOL_CAT, 1, 0, 0);
            let _ = c.record(TOOL_CAT, 2, 1, 1);
            let _ = c.record(TOOL_MV, 2, 0, 0);
            let cross = c.record(TOOL_MV, 2, 0, 0) == Err(DM_EDUP);
            dup && diff
                && b.is_empty()
                && b.diff_count() == 1
                && b.consistent()
                && cross
                && c.len() == 3
        },
        "static",
    );

    s.add(
        "DM8-覆盖不足与网格满",
        {
            let mut a = Dual::open(4);
            let _ = a.record(TOOL_CAT, 1, 0, 0);
            let _ = a.record(TOOL_CAT, 2, 0, 0);
            let gap = a.finish() == Err(DM_EGAP);
            let _ = a.record(TOOL_MV, 1, 0, 0);
            let _ = a.record(TOOL_MV, 2, 0, 0);
            let _ = a.record(TOOL_CHMOD, 1, 0, 0);
            let partial = a.finish() == Err(DM_EGAP);
            let _ = a.record(TOOL_CHMOD, 2, 0, 0);
            let mut f = Dual::open(4);
            let mut k = 1u8;
            while k <= IDX_MAX {
                let _ = f.record(TOOL_CAT, k, 0, 0);
                k = k.saturating_add(1);
            }
            let mut k = 1u8;
            while k <= 4 {
                let _ = f.record(TOOL_MV, k, 0, 0);
                k = k.saturating_add(1);
            }
            gap && partial
                && a.finish() == Ok(6)
                && f.len() == CAP
                && f.record(TOOL_CHMOD, 1, 0, 0) == Err(DM_EFULL)
                && f.consistent()
        },
        "static",
    );

    s.add(
        "DM9-双机同码入格",
        {
            let mut a = Dual::open(2);
            let _ = a.record(TOOL_CAT, 1, 0, 0);
            let _ = a.record(TOOL_CAT, 2, 1, 1);
            let _ = a.record(TOOL_MV, 1, 2, 2);
            let _ = a.record(TOOL_MV, 2, 0, 0);
            let _ = a.record(TOOL_CHMOD, 1, 1, 1);
            let _ = a.record(TOOL_CHMOD, 2, 2, 2);
            a.consistent()
                && a.finish() == Ok(6)
                && a.per_tool(TOOL_CAT) == Ok(2)
                && a.per_tool(TOOL_MV) == Ok(2)
                && a.per_tool(TOOL_CHMOD) == Ok(2)
                && a.cell_at(2) == Some(Cell { tool: TOOL_MV, idx: 1, a: 2, b: 2 })
        },
        "static",
    );

    s.add(
        "DM10-异码不占格",
        {
            let mut a = Dual::open(3);
            let _ = a.record(TOOL_CAT, 1, 0, 0);
            let before = a.len();
            let diff = a.record(TOOL_CAT, 2, 7, 9) == Err(DM_EDIFF);
            let kept = a.len() == before && a.diff_count() == 1;
            let retry = a.record(TOOL_CAT, 2, 7, 7) == Ok(2);
            let next = a.record(TOOL_MV, 1, 3, 3) == Ok(3);
            diff && kept && retry && next && a.finish() == Err(DM_EGAP) && a.consistent()
        },
        "static",
    );

    s.add(
        "DM11-覆盖与格值回查",
        {
            let mut a = Dual::open(3);
            let _ = a.record(TOOL_CAT, 1, 0, 0);
            let _ = a.record(TOOL_CAT, 2, 1, 1);
            let _ = a.record(TOOL_MV, 1, 0, 0);
            let _ = a.record(TOOL_MV, 2, 0, 0);
            let _ = a.record(TOOL_CHMOD, 1, 1, 1);
            let _ = a.record(TOOL_CHMOD, 2, 2, 2);
            let _ = a.record(TOOL_CHMOD, 3, 3, 3);
            a.per_tool(TOOL_CAT) == Ok(2)
                && a.per_tool(TOOL_MV) == Ok(2)
                && a.per_tool(TOOL_CHMOD) == Ok(3)
                && a.per_tool(0) == Err(DM_ETOOL)
                && a.finish() == Ok(7)
                && a.cell_at(5) == Some(Cell { tool: TOOL_CHMOD, idx: 2, a: 2, b: 2 })
                && a.cell_at(6) == Some(Cell { tool: TOOL_CHMOD, idx: 3, a: 3, b: 3 })
                && a.cell_at(7).is_none()
        },
        "static",
    );

    s.add(
        "DM12-段基与错误码布局",
        SEG_BASE == 0xf030
            && DM_ESTALE == SEG_BASE + 0x01
            && DM_ETOOL == SEG_BASE + 0x02
            && DM_EIDX == SEG_BASE + 0x03
            && DM_EDUP == SEG_BASE + 0x04
            && DM_EDIFF == SEG_BASE + 0x05
            && DM_EORDER == SEG_BASE + 0x06
            && DM_EGAP == SEG_BASE + 0x07
            && DM_EFULL == SEG_BASE + 0x08
            && DM_EBAL == SEG_BASE + 0x09
            && DM_ESLOT == SEG_BASE + 0x0a
            && DM_EVALUE == SEG_BASE + 0x0b
            && DM_ESEAL == SEG_BASE + 0x0c
            && DM_ENOBS == SEG_BASE + 0x0d
            && SAMPLES == 14
            && CAP == 12
            && IDX_MAX == 8
            && WANT == 2,
        "static",
    );

    s.add(
        "DM13-冻结重放四注入",
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

    fn grid() -> Dual {
        let mut d = Dual::open(7);
        let _ = d.record(TOOL_CAT, 1, 0, 0);
        let _ = d.record(TOOL_CAT, 2, 1, 1);
        let _ = d.record(TOOL_MV, 1, 2, 2);
        let _ = d.record(TOOL_MV, 2, 1, 1);
        let _ = d.record(TOOL_CHMOD, 1, 0, 0);
        let _ = d.record(TOOL_CHMOD, 2, 2, 2);
        d
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(0).map(|x| x.value), Some(1));
        assert_eq!(l.sample(5).map(|x| x.value), Some(6));
        assert_eq!(l.sample(7).map(|x| x.value), Some(2));
        assert_eq!(l.sample(13).map(|x| x.code), Some(DM_EFULL));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11419_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn epoch_gate() {
        assert_eq!(Dual::stamp(0), Err(DM_ESTALE));
        assert_eq!(Dual::stamp(5), Ok(5));
        let d = Dual::open(5);
        assert_eq!(d.epoch(), 5);
        assert!(d.is_empty());
        assert!(d.consistent());
        let z = Dual::open(0);
        assert!(!z.consistent());
    }

    #[test]
    fn record_gates() {
        let mut d = Dual::open(1);
        assert_eq!(d.record(9, 1, 0, 0), Err(DM_ETOOL));
        assert_eq!(d.record(TOOL_CAT, 0, 0, 0), Err(DM_EIDX));
        assert_eq!(d.record(TOOL_CAT, 9, 0, 0), Err(DM_EIDX));
        assert_eq!(d.record(TOOL_CAT, 1, 0, 0), Ok(1));
        assert_eq!(d.record(TOOL_MV, 1, 0, 0), Ok(2));
        assert_eq!(d.record(TOOL_CAT, 2, 0, 0), Err(DM_EORDER));
    }

    #[test]
    fn dup_and_diff() {
        let mut d = Dual::open(1);
        assert_eq!(d.record(TOOL_CAT, 1, 0, 0), Ok(1));
        assert_eq!(d.record(TOOL_CAT, 1, 0, 0), Err(DM_EDUP));
        let mut e = Dual::open(1);
        assert_eq!(e.record(TOOL_CAT, 1, 4, 5), Err(DM_EDIFF));
        assert_eq!(e.len(), 0);
        assert_eq!(e.diff_count(), 1);
        assert!(e.consistent());
    }

    #[test]
    fn finish_and_capacity() {
        let d = grid();
        assert_eq!(d.finish(), Ok(6));
        assert!(d.consistent());
        let mut f = Dual::open(2);
        let mut k = 1u8;
        while k <= IDX_MAX {
            assert_eq!(f.record(TOOL_CAT, k, 0, 0), Ok(k as usize));
            k = k.saturating_add(1);
        }
        let mut k = 1u8;
        while k <= 4 {
            let _ = f.record(TOOL_MV, k, 0, 0);
            k = k.saturating_add(1);
        }
        assert_eq!(f.len(), CAP);
        assert_eq!(f.record(TOOL_CHMOD, 1, 0, 0), Err(DM_EFULL));
        assert!(f.consistent());
    }

    #[test]
    fn coverage_and_cells() {
        let d = grid();
        assert_eq!(d.per_tool(TOOL_CAT), Ok(2));
        assert_eq!(d.per_tool(TOOL_MV), Ok(2));
        assert_eq!(d.per_tool(TOOL_CHMOD), Ok(2));
        assert_eq!(d.per_tool(0), Err(DM_ETOOL));
        assert_eq!(d.cell_at(0), Some(Cell { tool: TOOL_CAT, idx: 1, a: 0, b: 0 }));
        assert_eq!(d.cell_at(5), Some(Cell { tool: TOOL_CHMOD, idx: 2, a: 2, b: 2 }));
        assert!(d.cell_at(6).is_none());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
