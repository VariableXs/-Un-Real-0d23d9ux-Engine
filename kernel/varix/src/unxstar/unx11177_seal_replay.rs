//! UNX-F11177 域验收封存（验收面全家当封存）（0x1f4d0 段）
//!
//! 模型：验收面封存分两半——封存件清单与复跑指引。封存件登记件号、类别
//! （场、账、协议、基线四类）、字节数与封条；复跑指引登记步号、命令号、
//! 前置步与退出码。规则：件号一至六严格递增，类别在四类内，字节非零，
//! 封条须由件号类别字节复算；步号一至五严格递增，命令号在一至三十二，
//! 前置步须早于本步，退出码必须为零。清单头部另记申报字节，终核须与
//! 封存件字节合计对平；四类必须全部被覆盖。复跑即按步序执行指引，逐步
//! 核验前置与退出码，返回执行步数。收件收步记受理，越界重复倒序类别
//! 封条配方申报不符记拒止；冻结后二次冻结与再写返回冻结码。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 验收封存）。
const SEG_BASE: u32 = 0x1f4d0;

/// 件号或步号越界。
pub const SL_ENO: u32 = SEG_BASE + 0x01;
/// 件号或步号重复。
pub const SL_EDUP: u32 = SEG_BASE + 0x02;
/// 件号或步号倒序。
pub const SL_EORDER: u32 = SEG_BASE + 0x03;
/// 类别越界。
pub const SL_EKIND: u32 = SEG_BASE + 0x04;
/// 封条不符。
pub const SL_ESTAMP: u32 = SEG_BASE + 0x05;
/// 字节为零或越上界。
pub const SL_EBYTES: u32 = SEG_BASE + 0x06;
/// 配方非法（命令号、前置步、退出码）。
pub const SL_ERECIPE: u32 = SEG_BASE + 0x07;
/// 申报字节不符或未申报。
pub const SL_EDECL: u32 = SEG_BASE + 0x08;
/// 封存面不齐。
pub const SL_ECOUNT: u32 = SEG_BASE + 0x09;
/// 终值已冻结。
pub const SL_EFROZEN: u32 = SEG_BASE + 0x0a;

/// 封存件数。
pub const SEALS: usize = 6;
/// 复跑步数。
pub const STEPS: usize = 5;
/// 类别数。
pub const KINDS: u8 = 4;
/// 命令号上界。
pub const CMD_MAX: u16 = 32;
/// 单件字节上界。
pub const BYTES_MAX: u32 = 8192;
/// 四类全覆盖掩码。
pub const KIND_MASK: u8 = 0x0f;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一件封存。
#[derive(Clone, Copy, Debug)]
pub struct Seal {
    pub id: u8,
    pub kind: u8,
    pub bytes: u32,
    pub stamp: u32,
}

impl Seal {
    /// 空件。
    pub const fn blank() -> Seal {
        Seal {
            id: 0,
            kind: 0,
            bytes: 0,
            stamp: 0,
        }
    }

    /// 封条：件号、类别、字节卷成 32 位。
    pub fn stamp_of(id: u8, kind: u8, bytes: u32) -> u32 {
        let mut x = (id as u32).wrapping_mul(0x9e37_79b1) ^ (kind as u32);
        x = x.rotate_left(15) ^ bytes;
        let y = x.wrapping_mul(0x85eb_ca6b);
        if y == 0 {
            1
        } else {
            y
        }
    }

    /// 类别、字节与封条自洽。
    pub fn ok(&self) -> bool {
        self.kind < KINDS
            && self.bytes > 0
            && self.bytes <= BYTES_MAX
            && self.stamp == Seal::stamp_of(self.id, self.kind, self.bytes)
    }
}

/// 一步复跑指引。
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub no: u8,
    pub cmd: u16,
    pub after: u8,
    pub exit: i16,
}

impl Step {
    /// 空步。
    pub const fn blank() -> Step {
        Step {
            no: 0,
            cmd: 0,
            after: 0,
            exit: 0,
        }
    }

    /// 命令号在域内、前置步早于本步、退出码为零。
    pub fn ok(&self) -> bool {
        self.cmd >= 1 && self.cmd <= CMD_MAX && self.after < self.no && self.exit == 0
    }
}

/// 验收封存账。
#[derive(Clone, Copy, Debug)]
pub struct SealLedger {
    seals: [Seal; SEALS],
    steps: [Step; STEPS],
    n_seals: usize,
    n_steps: usize,
    accepts: u32,
    rejects: u32,
    declared: Option<u32>,
    frozen: bool,
    settled: Option<u64>,
}

impl SealLedger {
    /// 空账。
    pub const fn new() -> SealLedger {
        SealLedger {
            seals: [Seal::blank(); SEALS],
            steps: [Step::blank(); STEPS],
            n_seals: 0,
            n_steps: 0,
            accepts: 0,
            rejects: 0,
            declared: None,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已封件数。
    pub fn sealed(&self) -> usize {
        self.n_seals
    }

    /// 已列步数。
    pub fn step_count(&self) -> usize {
        self.n_steps
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 申报字节。
    pub fn declared(&self) -> Option<u32> {
        self.declared
    }

    /// 第 i 件。
    pub fn seal_at(&self, i: usize) -> Option<Seal> {
        if i >= self.n_seals {
            return None;
        }
        self.seals.get(i).copied()
    }

    /// 第 i 步。
    pub fn step_at(&self, i: usize) -> Option<Step> {
        if i >= self.n_steps {
            return None;
        }
        self.steps.get(i).copied()
    }

    /// 按件号查。
    pub fn find_seal(&self, id: u8) -> Option<Seal> {
        let mut i = 0usize;
        while i < self.n_seals {
            if let Some(s) = self.seals.get(i) {
                if s.id == id {
                    return Some(*s);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收封存件：冻结 → 件号 → 类别 → 字节 → 重复 → 倒序 → 封条。
    pub fn register_seal(&mut self, s: Seal) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(SL_EFROZEN));
        }
        if s.id == 0 || s.id as usize > SEALS {
            return Err(self.fail(SL_ENO));
        }
        if s.kind >= KINDS {
            return Err(self.fail(SL_EKIND));
        }
        if s.bytes == 0 || s.bytes > BYTES_MAX {
            return Err(self.fail(SL_EBYTES));
        }
        if self.find_seal(s.id).is_some() {
            return Err(self.fail(SL_EDUP));
        }
        if self.n_seals > 0 {
            if let Some(last) = self.seals.get(self.n_seals - 1) {
                if s.id <= last.id {
                    return Err(self.fail(SL_EORDER));
                }
            }
        }
        if s.stamp != Seal::stamp_of(s.id, s.kind, s.bytes) {
            return Err(self.fail(SL_ESTAMP));
        }
        match self.seals.get_mut(self.n_seals) {
            Some(slot) => *slot = s,
            None => return Err(self.fail(SL_ECOUNT)),
        }
        self.n_seals = self.n_seals.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 收复跑步：冻结 → 步号 → 命令/前置/退出 → 重复 → 倒序。
    pub fn register_step(&mut self, st: Step) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(SL_EFROZEN));
        }
        if st.no == 0 || st.no as usize > STEPS {
            return Err(self.fail(SL_ENO));
        }
        if st.cmd == 0 || st.cmd > CMD_MAX || st.after >= st.no || st.exit != 0 {
            return Err(self.fail(SL_ERECIPE));
        }
        if self.n_steps > 0 {
            if let Some(last) = self.steps.get(self.n_steps - 1) {
                if st.no == last.no {
                    return Err(self.fail(SL_EDUP));
                }
                if st.no < last.no {
                    return Err(self.fail(SL_EORDER));
                }
            }
        }
        match self.steps.get_mut(self.n_steps) {
            Some(slot) => *slot = st,
            None => return Err(self.fail(SL_ECOUNT)),
        }
        self.n_steps = self.n_steps.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 封存件字节合计。
    pub fn bytes_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n_seals {
            if let Some(s) = self.seals.get(i) {
                sum = sum.wrapping_add(s.bytes);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 四类覆盖掩码。
    pub fn kind_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < self.n_seals {
            if let Some(s) = self.seals.get(i) {
                if s.kind < KINDS {
                    mask |= 1u8 << s.kind;
                }
            }
            i = i.wrapping_add(1);
        }
        mask
    }

    /// 四类全覆盖。
    pub fn kinds_full(&self) -> bool {
        self.kind_mask() == KIND_MASK
    }

    /// 封存件全齐且封条自洽、件号连续。
    pub fn seals_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_seals {
            match self.seals.get(i) {
                Some(s) => {
                    if s.id as usize != i.wrapping_add(1) || !s.ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        self.n_seals == SEALS
    }

    /// 步骤前置与退出码全合。
    pub fn steps_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.n_steps {
            match self.steps.get(i) {
                Some(st) => {
                    if !st.ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 复跑指引执行：按步序核验前置与退出码，返回执行步数。
    pub fn execute(&self) -> Result<u32, u32> {
        if self.n_steps != STEPS {
            return Err(SL_ECOUNT);
        }
        let mut i = 0usize;
        let mut ran = 0u32;
        while i < self.n_steps {
            let st = match self.steps.get(i) {
                Some(v) => *v,
                None => return Err(SL_ERECIPE),
            };
            if st.no as usize != i.wrapping_add(1) {
                return Err(SL_EORDER);
            }
            if st.cmd == 0 || st.cmd > CMD_MAX {
                return Err(SL_ERECIPE);
            }
            if st.after >= st.no {
                return Err(SL_ERECIPE);
            }
            if st.exit != 0 {
                return Err(SL_ERECIPE);
            }
            ran = ran.wrapping_add(1);
            i = i.wrapping_add(1);
        }
        Ok(ran)
    }

    /// 申报字节：须在封存件齐后与合计相等。
    pub fn declare(&mut self, total: u32) -> Result<u32, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(SL_EFROZEN));
        }
        if self.n_seals != SEALS {
            return Err(self.fail(SL_ECOUNT));
        }
        if total != self.bytes_total() {
            return Err(self.fail(SL_EDECL));
        }
        self.declared = Some(total);
        Ok(total)
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n_seals {
            if let Some(s) = self.seals.get(i) {
                h = (h ^ s.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.kind as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.bytes as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ s.stamp as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < self.n_steps {
            if let Some(st) = self.steps.get(j) {
                h = (h ^ st.no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.cmd as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.after as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.exit as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        let dv = match self.declared {
            Some(v) => v as u64,
            None => 0,
        };
        h = (h ^ self.n_seals as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.n_steps as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ dv).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：封存面齐、四类满、复跑可执行、申报对平。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.seals_ok() {
            return Err(SL_ECOUNT);
        }
        if !self.kinds_full() {
            return Err(SL_EKIND);
        }
        let ran = match self.execute() {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        if ran as usize != STEPS {
            return Err(SL_ECOUNT);
        }
        match self.declared {
            Some(v) => {
                if v != self.bytes_total() {
                    return Err(SL_EDECL);
                }
            }
            None => return Err(SL_EDECL),
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(SL_EFROZEN));
        }
        let v = match self.replay() {
            Ok(v) => v,
            Err(e) => return Err(self.fail(e)),
        };
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 第 id 件规格：类别、字节；命令号仅对步有意义。
pub fn spec_of(id: usize) -> (u8, u32, u16) {
    if id == 0 || id > SEALS {
        return (0, 0, 0);
    }
    let kind = ((id - 1) % KINDS as usize) as u8;
    let bytes = 512u32.wrapping_add((id as u32) * 64);
    let cmd = 10u16.wrapping_add((id as u16) * 2);
    (kind, bytes, cmd)
}

/// 规格封存件。
pub fn seal_spec(id: u8) -> Seal {
    let mut s = Seal::blank();
    let spec = spec_of(id as usize);
    s.id = id;
    s.kind = spec.0;
    s.bytes = spec.1;
    s.stamp = Seal::stamp_of(id, s.kind, s.bytes);
    s
}

/// 规格复跑步：前置为前一步，退出码为零。
pub fn step_spec(no: u8) -> Step {
    let mut st = Step::blank();
    if no == 0 || no as usize > STEPS {
        return st;
    }
    st.no = no;
    st.cmd = 10u16.wrapping_add((no as u16) * 2);
    st.after = no.wrapping_sub(1);
    st.exit = 0;
    st
}

/// 构建标准封存账：六件封存、五步复跑、申报字节。
pub fn build_ledger() -> SealLedger {
    let mut l = SealLedger::new();
    let mut id = 1u8;
    while id <= SEALS as u8 {
        let _ = l.register_seal(seal_spec(id));
        id = id.wrapping_add(1);
    }
    let mut no = 1u8;
    while no <= STEPS as u8 {
        let _ = l.register_step(step_spec(no));
        no = no.wrapping_add(1);
    }
    let total = l.bytes_total();
    let _ = l.declare(total);
    l
}

/// 四探针：越界、重复/倒序、类别/封条/配方、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = SealLedger::new();
    let _ = a.register_seal(seal_spec(1));
    let mut zero = seal_spec(1);
    zero.id = 0;
    let mut far = seal_spec(1);
    far.id = (SEALS + 1) as u8;
    if a.register_seal(zero) == Err(SL_ENO) && a.register_seal(far) == Err(SL_ENO) {
        r[0] = true;
    }

    let mut b = SealLedger::new();
    let s1 = seal_spec(1);
    let _ = b.register_seal(s1);
    if b.register_seal(s1) == Err(SL_EDUP) {
        r[1] = true;
    }
    let mut back = SealLedger::new();
    let _ = back.register_seal(seal_spec(2));
    if back.register_seal(seal_spec(1)) != Err(SL_EORDER) {
        r[1] = false;
    }

    let mut c = SealLedger::new();
    let _ = c.register_seal(seal_spec(1));
    let mut odd = seal_spec(2);
    odd.kind = KINDS;
    let mut forged = seal_spec(2);
    forged.stamp ^= 1;
    let mut badstep = step_spec(1);
    badstep.exit = 1;
    if c.register_seal(odd) == Err(SL_EKIND)
        && c.register_seal(forged) == Err(SL_ESTAMP)
        && c.register_step(badstep) == Err(SL_ERECIPE)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(SL_EFROZEN)
        && d.register_seal(seal_spec(1)) == Err(SL_EFROZEN)
        && d.register_step(step_spec(1)) == Err(SL_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11177");
    let base = build_ledger();

    s.add(
        "S1-封存逐件核销",
        base.sealed() == SEALS
            && base.step_count() == STEPS
            && base.accepted() == (SEALS + STEPS) as u32
            && base.rejected() == 0,
        "六件封存加五步指引全收，零拒止",
    );

    s.add(
        "S2-封条逐件可复算",
        base.seals_ok(),
        "件号连续且每件封条由件号类别字节复算",
    );

    s.add(
        "S3-四类全覆盖",
        base.kinds_full() && base.kind_mask() == KIND_MASK,
        "场账协议基线四类各至少一件",
    );

    let mut spec_bytes = 0u32;
    let mut id = 1usize;
    while id <= SEALS {
        spec_bytes = spec_bytes.wrapping_add(spec_of(id).1);
        id = id.wrapping_add(1);
    }
    s.add(
        "S4-字节对平",
        base.bytes_total() == spec_bytes && base.declared() == Some(spec_bytes),
        "封存字节合计、规格合计与申报值三方相等",
    );

    s.add(
        "S5-复跑可执行",
        base.execute() == Ok(STEPS as u32) && base.step_count() == STEPS,
        "指引五步全部执行且返回步数为五",
    );

    let mut pre = true;
    let mut k = 0usize;
    while k < base.step_count() {
        match base.step_at(k) {
            Some(st) => {
                if st.after >= st.no || !st.ok() {
                    pre = false;
                }
            }
            None => pre = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S6-前置步合法", pre, "每步前置均早于本步且命令号在域内");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(SL_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放封存面与指引全过且指纹一致",
    );

    let mut exits = true;
    let mut m = 0usize;
    while m < base.step_count() {
        match base.step_at(m) {
            Some(st) => {
                if st.exit != 0 {
                    exits = false;
                }
            }
            None => exits = false,
        }
        m = m.wrapping_add(1);
    }
    s.add("S10-退出码全零", exits, "五步退出码均为零");

    let mut la = SealLedger::new();
    let _ = la.register_seal(seal_spec(1));
    let mut zero = seal_spec(1);
    zero.id = 0;
    let mut far = seal_spec(1);
    far.id = (SEALS + 1) as u8;
    let r0 = la.register_seal(zero);
    let r1 = la.register_seal(far);
    s.add(
        "X11-越界拒收",
        r0 == Err(SL_ENO) && r1 == Err(SL_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "件号 0 与 7 均判越界",
    );

    let mut lb = SealLedger::new();
    let s1 = seal_spec(1);
    let _ = lb.register_seal(s1);
    let dup = lb.register_seal(s1);
    let mut lc = SealLedger::new();
    let _ = lc.register_seal(seal_spec(2));
    let ord = lc.register_seal(seal_spec(1));
    s.add(
        "X12-重复倒序",
        dup == Err(SL_EDUP) && ord == Err(SL_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重封一件判重复、二件后补一件判倒序",
    );

    let mut ld = SealLedger::new();
    let _ = ld.register_seal(seal_spec(1));
    let mut odd = seal_spec(2);
    odd.kind = KINDS;
    let mut forged = seal_spec(2);
    forged.stamp ^= 1;
    let mut badstep = step_spec(1);
    badstep.exit = 1;
    let kd = ld.register_seal(odd);
    let st = ld.register_seal(forged);
    let rc = ld.register_step(badstep);
    s.add(
        "X13-类别封条配方",
        kd == Err(SL_EKIND)
            && st == Err(SL_ESTAMP)
            && rc == Err(SL_ERECIPE)
            && ld.accepted() == 1
            && ld.rejected() == 3,
        "类别越界、封条不符、退出码非零分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11177_seal_flow() {
        let l = build_ledger();
        assert_eq!(l.sealed(), SEALS);
        assert_eq!(l.step_count(), STEPS);
        assert_eq!(l.accepted(), (SEALS + STEPS) as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.seals_ok());
        assert!(l.kinds_full());
        assert!(l.steps_ok());
        assert_eq!(l.execute(), Ok(STEPS as u32));
        assert_eq!(l.declared(), Some(l.bytes_total()));
        assert_eq!(l.seal_at(0).map(|s| s.kind), Some(0));
        assert!(l.seal_at(SEALS).is_none());
        assert!(l.step_at(STEPS).is_none());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11177_spec_shape() {
        assert_eq!(spec_of(0), (0, 0, 0));
        assert_eq!(spec_of(SEALS + 1), (0, 0, 0));
        let mut kinds = 0u8;
        let mut id = 1u8;
        while id <= SEALS as u8 {
            let s = seal_spec(id);
            assert!(s.ok());
            kinds |= 1u8 << s.kind;
            id = id.wrapping_add(1);
        }
        assert_eq!(kinds, KIND_MASK);
        assert_eq!(step_spec(1).after, 0);
        assert!(step_spec(STEPS as u8 + 1).cmd == 0);
        assert!(step_spec(2).ok());
    }

    #[test]
    fn f11177_gates() {
        let mut a = SealLedger::new();
        assert!(a.register_seal(seal_spec(1)).is_ok());
        assert_eq!(a.register_seal(seal_spec(1)), Err(SL_EDUP));
        let mut zero = seal_spec(2);
        zero.bytes = 0;
        assert_eq!(a.register_seal(zero), Err(SL_EBYTES));
        let mut odd = seal_spec(2);
        odd.kind = KINDS;
        assert_eq!(a.register_seal(odd), Err(SL_EKIND));
        let mut forged = seal_spec(2);
        forged.stamp ^= 1;
        assert_eq!(a.register_seal(forged), Err(SL_ESTAMP));
        let mut cmd0 = step_spec(1);
        cmd0.cmd = 0;
        assert_eq!(a.register_step(cmd0), Err(SL_ERECIPE));
        let mut after = step_spec(1);
        after.after = 4;
        assert_eq!(a.register_step(after), Err(SL_ERECIPE));
        assert_eq!(a.accepted(), 1);
        assert_eq!(a.rejected(), 6);
    }

    #[test]
    fn f11177_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(SL_EFROZEN));
        assert_eq!(l.register_seal(seal_spec(1)), Err(SL_EFROZEN));
        assert_eq!(l.declare(1), Err(SL_EFROZEN));
    }

    #[test]
    fn f11177_checks_green() {
        let s = run_unx11177_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
