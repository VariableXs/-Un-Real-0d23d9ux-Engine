//! UNX-F11142 B21 清账（管道 FIFO 错误矩阵判据终重放）（0x1f2a0 段）
//!
//! 纯功能模型：管道 / FIFO 的六族调用各带一张「错误矩阵」——每个用例预先
//! 登记期望 errno（0 表示应成功），重放时逐例比对实测值。重放要求用例齐、
//! 实测全到、逐例相符；用例号越界、重复登记、期望码越界、缺实测、不符、
//! 冻结后再动账，一律拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2a0;
/// 用例号越界
pub const PF_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const PF_EDUP: u32 = SEG_BASE + 0x02;
/// 用例序倒挂
pub const PF_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const PF_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未测即重放
pub const PF_ESHORT: u32 = SEG_BASE + 0x05;
/// 实测与期望不符
pub const PF_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 空矩阵
pub const PF_EEMPTY: u32 = SEG_BASE + 0x07;
/// 已封盘
pub const PF_EFROZEN: u32 = SEG_BASE + 0x08;

/// 调用族数。
pub const OPS: usize = 6;
/// 族名：open / read / write / close / pipe / fcntl。
pub const OP_NAMES: [&str; OPS] = ["open", "read", "write", "close", "pipe", "fcntl"];
/// 用例数（每族三例）。
pub const CASES: usize = 18;
/// errno 上界（视作合法码域）。
pub const ERRNO_MAX: u16 = 120;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个矩阵用例。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub id: u8,
    pub op: u8,
    pub expected: u16,
    pub observed: Option<u16>,
}

/// 管道错误矩阵重放账。
#[derive(Clone, Debug)]
pub struct PipeMatrix {
    cases: Vec<Case>,
    sealed: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub replays: u64,
}

impl PipeMatrix {
    /// 空矩阵。
    pub fn new() -> PipeMatrix {
        PipeMatrix {
            cases: Vec::new(),
            sealed: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            replays: 0,
        }
    }

    /// 已登记用例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否空矩阵。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 取某用例。
    pub fn case(&self, id: usize) -> Option<Case> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id as usize == id {
                return Some(self.cases[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 登记一例：封盘→用例号→重复→序→期望码。
    pub fn register(&mut self, id: usize, op: usize, expected: u16) -> Result<u64, u32> {
        if self.frozen || self.sealed.is_some() {
            return self.fail(PF_EFROZEN);
        }
        if id >= CASES {
            return self.fail(PF_ECASE);
        }
        if self.case(id).is_some() {
            return self.fail(PF_EDUP);
        }
        if let Some(last) = self.cases.last() {
            if id <= last.id as usize {
                return self.fail(PF_EORDER);
            }
        }
        if op >= OPS {
            return self.fail(PF_ECASE);
        }
        if expected > ERRNO_MAX {
            return self.fail(PF_ENOEXP);
        }
        self.cases.push(Case {
            id: id as u8,
            op: op as u8,
            expected,
            observed: None,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 记一例实测值：未登记用例拒。
    pub fn observe(&mut self, id: usize, errno: u16) -> Result<u64, u32> {
        if self.frozen || self.sealed.is_some() {
            return self.fail(PF_EFROZEN);
        }
        if errno > ERRNO_MAX {
            return self.fail(PF_ENOEXP);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id as usize == id {
                self.cases[i].observed = Some(errno);
                self.accepts = self.accepts.wrapping_add(1);
                return Ok(self.fold());
            }
            i = i.wrapping_add(1);
        }
        self.fail(PF_ECASE)
    }

    /// 已测例数。
    pub fn observed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].observed.is_some() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 不符例数。
    pub fn mismatches(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = self.cases[i];
            if let Some(o) = c.observed {
                if o != c.expected {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 缺口用例号。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut n = 0usize;
        while n < CASES {
            if self.case(n).is_none() {
                out.push(n as u32);
            }
            n = n.wrapping_add(1);
        }
        out
    }

    /// 每族用例数。
    pub fn op_table(&self) -> [usize; OPS] {
        let mut t = [0usize; OPS];
        let mut i = 0usize;
        while i < self.cases.len() {
            let op = self.cases[i].op as usize;
            if let Some(slot) = t.get_mut(op) {
                *slot = slot.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        t
    }

    /// 矩阵指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = self.cases[i];
            h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.op as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.expected as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.observed.unwrap_or(0xffff) as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.cases.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终重放：用例齐 → 实测全到 → 逐例相符。
    pub fn replay(&mut self) -> Result<u64, u32> {
        self.replays = self.replays.wrapping_add(1);
        if self.sealed.is_some() {
            return self.fail(PF_EFROZEN);
        }
        if self.cases.is_empty() {
            return self.fail(PF_EEMPTY);
        }
        if self.cases.len() != CASES || !self.gaps().is_empty() {
            return self.fail(PF_ESHORT);
        }
        if self.observed() != CASES {
            return self.fail(PF_ESHORT);
        }
        if self.mismatches() != 0 {
            return self.fail(PF_EMISMATCH);
        }
        let v = self.fold();
        self.sealed = Some(v);
        Ok(v)
    }

    /// 封盘：锁单值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.sealed;
        }
        if self.sealed.is_none() {
            self.sealed = Some(self.fold());
        }
        self.frozen = true;
        self.sealed
    }

    /// 封盘值。
    pub fn sealed_value(&self) -> Option<u64> {
        self.sealed
    }

    /// 是否已封盘。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建矩阵：十八例、六族各三例，期望码按例号铺开，实测与期望同值。
pub fn build_matrix() -> (PipeMatrix, bool) {
    let mut m = PipeMatrix::new();
    let mut id = 0usize;
    while id < CASES {
        let op = id % OPS;
        let expected = expected_of(id);
        if m.register(id, op, expected).is_err() {
            return (m, false);
        }
        id = id.wrapping_add(1);
    }
    let mut id = 0usize;
    while id < CASES {
        if m.observe(id, expected_of(id)).is_err() {
            return (m, false);
        }
        id = id.wrapping_add(1);
    }
    let ok = m.replay().is_ok();
    (m, ok)
}

/// 用例的期望 errno：每三例留一例成功（0），其余按例号取 1..=4。
pub fn expected_of(id: usize) -> u16 {
    if id % 3 == 0 {
        0
    } else {
        ((id % 4).wrapping_add(1)) as u16
    }
}

/// 四类注入：用例号越界 / 重复登记与序倒挂 / 期望码越界与实测不符 / 封盘后动账。
pub fn inject_four() -> [bool; 4] {
    // ① 用例号与族号越界
    let mut a0 = PipeMatrix::new();
    let r0 = a0.register(CASES, 0, 0).err() == Some(PF_ECASE)
        && a0.register(0, OPS, 0).err() == Some(PF_ECASE);

    // ② 重复登记与序倒挂
    let mut a1 = PipeMatrix::new();
    let s1 = a1.register(0, 0, 0).is_ok();
    let s2 = a1.register(1, 1, 1).is_ok();
    let r1 = s1
        && s2
        && a1.register(0, 0, 0).err() == Some(PF_EDUP)
        && a1.register(2, 2, 0).is_ok()
        && a1.register(1, 1, 1).err() == Some(PF_EDUP);

    // ③ 期望码越界、缺实测、实测不符
    let mut a2 = PipeMatrix::new();
    let bad_exp = a2.register(0, 0, ERRNO_MAX.wrapping_add(1)).err() == Some(PF_ENOEXP);
    let mut a3 = PipeMatrix::new();
    let _ = a3.register(0, 0, 3);
    let short = a3.replay().err() == Some(PF_ESHORT);
    let _ = a3.observe(0, 3);
    let still_short = a3.replay().err() == Some(PF_ESHORT);
    let mut a4 = PipeMatrix::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = a4.register(id, id % OPS, expected_of(id));
        id = id.wrapping_add(1);
    }
    let _ = a4.observe(5, expected_of(5).wrapping_add(1));
    let mut id = 0usize;
    while id < CASES {
        if id != 5 {
            let _ = a4.observe(id, expected_of(id));
        }
        id = id.wrapping_add(1);
    }
    let mismatch = a4.replay().err() == Some(PF_EMISMATCH);
    let r2 = bad_exp && short && still_short && mismatch;

    // ④ 封盘后登记与二次重放
    let (mut a5, ok5) = build_matrix();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.register(0, 0, 0).err() == Some(PF_EFROZEN)
        && a5.observe(0, 0).err() == Some(PF_EFROZEN)
        && a5.replay().err() == Some(PF_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11142 B21 管道错误矩阵终重放。
pub fn run_unx11142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11142");
    let (mut m, ok) = build_matrix();

    // P1 十八例齐备
    s.add_dyn(
        "P1-十八例齐备",
        ok && m.len() == CASES && m.gaps().is_empty(),
        &format!("cases={}/{} gaps={}", m.len(), CASES, m.gaps().len()),
    );

    // P2 六族各三例
    let t = m.op_table();
    let mut even = true;
    let mut i = 0usize;
    while i < OPS {
        if t[i] != CASES / OPS {
            even = false;
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "P2-六族各三例",
        even,
        &format!("t={},{},{},{},{},{}", t[0], t[1], t[2], t[3], t[4], t[5]),
    );

    // P3 实测全到
    s.add_dyn(
        "P3-实测全到",
        m.observed() == CASES,
        &format!("observed={}/{}", m.observed(), CASES),
    );

    // P4 逐例相符
    s.add_dyn(
        "P4-逐例相符",
        m.mismatches() == 0,
        &format!("mismatches={}", m.mismatches()),
    );

    // P5 成功例占比合口径
    let mut successes = 0usize;
    let mut i = 0usize;
    while i < CASES {
        if let Some(c) = m.case(i) {
            if c.expected == 0 {
                successes = successes.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "P5-成功例占比",
        successes == CASES / 3,
        &format!("success={}/{}", successes, CASES / 3),
    );

    // P6 封盘值即指纹
    let root = m.sealed_value();
    s.add(
        "P6-封盘值即指纹",
        root == Some(m.fold()) && m.freeze() == root,
        "封盘值等于矩阵指纹，冻结不改变它",
    );

    // P7 冻结幂等
    let f1 = m.freeze();
    let f2 = m.freeze();
    s.add("P7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // P8 封盘后拒动
    s.add(
        "P8-封盘后拒动",
        m.is_frozen()
            && m.register(0, 0, 0).err() == Some(PF_EFROZEN)
            && m.observe(0, 0).err() == Some(PF_EFROZEN)
            && m.replay().err() == Some(PF_EFROZEN),
        "封盘后登记 / 实测 / 重放三路皆拒",
    );

    // P9 空矩阵拒重放
    let mut e = PipeMatrix::new();
    s.add(
        "P9-空矩阵拒重放",
        e.replay().err() == Some(PF_EEMPTY) && e.is_empty(),
        "空矩阵不得重放",
    );

    // P10 未齐拒重放
    let mut h = PipeMatrix::new();
    let _ = h.register(0, 0, 0);
    let _ = h.observe(0, 0);
    s.add_dyn(
        "P10-未齐拒重放",
        h.replay().err() == Some(PF_ESHORT) && h.gaps().len() == CASES - 1,
        &format!("cases={} gaps={}", h.len(), h.gaps().len()),
    );

    // P11 缺实测拒重放
    let mut u = PipeMatrix::new();
    let mut i = 0usize;
    while i < CASES {
        let _ = u.register(i, i % OPS, expected_of(i));
        i = i.wrapping_add(1);
    }
    let mut i = 0usize;
    while i < CASES - 1 {
        let _ = u.observe(i, expected_of(i));
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "P11-缺实测拒重放",
        u.replay().err() == Some(PF_ESHORT) && u.observed() == CASES - 1,
        &format!("observed={}", u.observed()),
    );

    // P12 不符拒重放
    let mut w = PipeMatrix::new();
    let mut i = 0usize;
    while i < CASES {
        let _ = w.register(i, i % OPS, expected_of(i));
        let shift = if i == 7 { 1 } else { 0 };
        let _ = w.observe(i, expected_of(i).wrapping_add(shift));
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "P12-不符拒重放",
        w.replay().err() == Some(PF_EMISMATCH) && w.mismatches() == 1,
        &format!("mismatches={}", w.mismatches()),
    );

    // P13 注入四拒 + 终盘明细（新矩阵重算）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fresh, ok2) = build_matrix();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "P13-注入四拒与终盘明细",
        all && ok2 && fresh.rejects == 0 && fresh.accepts == (CASES as u64) * 2 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11142_matrix_flow() {
        let (mut m, ok) = build_matrix();
        assert!(ok);
        assert_eq!(m.len(), CASES);
        assert_eq!(m.observed(), CASES);
        assert_eq!(m.mismatches(), 0);
        let t = m.op_table();
        assert_eq!(t[0], CASES / OPS);
        assert_eq!(t[5], CASES / OPS);
        let root = m.sealed_value();
        assert_eq!(root, Some(m.fold()));
        assert_eq!(m.freeze(), root);
        assert_eq!(m.freeze(), root);
        assert_eq!(m.register(0, 0, 0), Err(PF_EFROZEN));
    }

    #[test]
    fn f11142_gates() {
        let mut m = PipeMatrix::new();
        assert_eq!(m.register(CASES, 0, 0), Err(PF_ECASE));
        assert_eq!(m.register(0, OPS, 0), Err(PF_ECASE));
        assert_eq!(m.register(0, 0, ERRNO_MAX + 1), Err(PF_ENOEXP));
        assert!(m.register(0, 0, 0).is_ok());
        assert_eq!(m.register(0, 0, 0), Err(PF_EDUP));
        assert_eq!(m.observe(9, 0), Err(PF_ECASE));
        assert_eq!(m.rejects, 5);
        assert_eq!(m.accepts, 1);
    }

    #[test]
    fn f11142_checks_green() {
        let cs = run_unx11142_checks();
        assert!(cs.all_passed());
    }
}
