//! UNX-F11178 接力账（向 B40 域闭账段）（0x1f4e0 段）
//!
//! 模型：B39 收尾时把余额像接力棒一样逐段交给 B40。账上先开闸记入值，
//! 再按六段登记每段的入值、损耗与出值；每段恒等式为「入值减损耗等于出值」，
//! 且损耗不得大于入值、出值必须为正；相邻两段还须首尾相接——下一段入值
//! 等于上一段出值，首段入值等于开闸值。末段出值即交给 B40 的闭账值，关闸
//! 时申报的关账值必须与末段出值相等。终核另验总量守恒：开闸值减六段损耗
//! 合计等于关账值。越界、重复、倒序、算术破裂、承接断裂、未开闸与关账
//! 不符均记拒止；冻结后二次冻结与再写返回冻结码。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 接力账）。
const SEG_BASE: u32 = 0x1f4e0;

/// 段号越界。
pub const RL_ENO: u32 = SEG_BASE + 0x01;
/// 段号重复。
pub const RL_EDUP: u32 = SEG_BASE + 0x02;
/// 段号倒序。
pub const RL_EORDER: u32 = SEG_BASE + 0x03;
/// 段内算术破裂。
pub const RL_EARITH: u32 = SEG_BASE + 0x04;
/// 承接断裂。
pub const RL_ECHAIN: u32 = SEG_BASE + 0x05;
/// 未开闸或开闸值非法。
pub const RL_EHEAD: u32 = SEG_BASE + 0x06;
/// 关账值不符。
pub const RL_ETAIL: u32 = SEG_BASE + 0x07;
/// 段数不齐。
pub const RL_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const RL_EFROZEN: u32 = SEG_BASE + 0x09;

/// 段数。
pub const STAGES: usize = 6;
/// 开闸值。
pub const OPENING: u32 = 4096;
/// 单段损耗步长。
pub const LOSS_STEP: u32 = 64;
/// 交接口槽标识（B40 闭账段第一位）。
pub const B40_SLOT: u32 = 0x40;
/// 段值上界。
pub const VALUE_MAX: u32 = 65_536;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一段接力。
#[derive(Clone, Copy, Debug)]
pub struct Relay {
    pub stage: u8,
    pub carry_in: u32,
    pub loss: u32,
    pub carry_out: u32,
}

impl Relay {
    /// 空段。
    pub const fn blank() -> Relay {
        Relay {
            stage: 0,
            carry_in: 0,
            loss: 0,
            carry_out: 0,
        }
    }

    /// 段内算术：损耗不越入值，出值等于入值减损耗且为正。
    pub fn arith_ok(&self) -> bool {
        if self.carry_in == 0 || self.carry_in > VALUE_MAX {
            return false;
        }
        if self.loss > self.carry_in {
            return false;
        }
        let out = self.carry_in.wrapping_sub(self.loss);
        out > 0 && self.carry_out == out
    }
}

/// 接力账。
#[derive(Clone, Copy, Debug)]
pub struct RelayLedger {
    relays: [Relay; STAGES],
    len: usize,
    accepts: u32,
    rejects: u32,
    opened: Option<u32>,
    closed: Option<u32>,
    frozen: bool,
    settled: Option<u64>,
}

impl RelayLedger {
    /// 空账。
    pub const fn new() -> RelayLedger {
        RelayLedger {
            relays: [Relay::blank(); STAGES],
            len: 0,
            accepts: 0,
            rejects: 0,
            opened: None,
            closed: None,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已记段数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 开闸值。
    pub fn opened(&self) -> Option<u32> {
        self.opened
    }

    /// 关账值。
    pub fn closed(&self) -> Option<u32> {
        self.closed
    }

    /// 末段。
    pub fn last(&self) -> Option<Relay> {
        if self.len == 0 {
            return None;
        }
        self.relays.get(self.len - 1).copied()
    }

    /// 第 i 段。
    pub fn relay_at(&self, i: usize) -> Option<Relay> {
        if i >= self.len {
            return None;
        }
        self.relays.get(i).copied()
    }

    /// 按段号查序号。
    pub fn find(&self, stage: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.relays.get(i) {
                if r.stage == stage {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 开闸：冻结 → 值域 → 重复开闸。
    pub fn open(&mut self, value: u32) -> Result<u32, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RL_EFROZEN));
        }
        if self.opened.is_some() {
            return Err(self.fail(RL_EHEAD));
        }
        if value == 0 || value > VALUE_MAX {
            return Err(self.fail(RL_EHEAD));
        }
        self.opened = Some(value);
        Ok(value)
    }

    /// 收段：冻结 → 开闸 → 段号 → 重复 → 倒序 → 算术 → 承接。
    pub fn register(&mut self, r: Relay) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RL_EFROZEN));
        }
        if self.opened.is_none() {
            return Err(self.fail(RL_EHEAD));
        }
        if r.stage == 0 || r.stage as usize > STAGES {
            return Err(self.fail(RL_ENO));
        }
        if self.find(r.stage).is_some() {
            return Err(self.fail(RL_EDUP));
        }
        let expected_in = match self.last() {
            Some(last) => {
                if r.stage <= last.stage {
                    return Err(self.fail(RL_EORDER));
                }
                last.carry_out
            }
            None => match self.opened {
                Some(v) => v,
                None => return Err(self.fail(RL_EHEAD)),
            },
        };
        if !r.arith_ok() {
            return Err(self.fail(RL_EARITH));
        }
        if r.carry_in != expected_in {
            return Err(self.fail(RL_ECHAIN));
        }
        match self.relays.get_mut(self.len) {
            Some(slot) => *slot = r,
            None => return Err(self.fail(RL_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按段号观察收段：入值承接现链尾，损耗按段号递增。
    pub fn observe(&mut self, stage: usize) -> Result<u64, u32> {
        if stage == 0 || stage > STAGES {
            return Err(self.fail(RL_ENO));
        }
        let prev_out = match self.last() {
            Some(last) => last.carry_out,
            None => match self.opened {
                Some(v) => v,
                None => return Err(self.fail(RL_EHEAD)),
            },
        };
        let spec = spec_from(prev_out, stage as u8);
        self.register(spec)
    }

    /// 关闸：冻结 → 段数齐 → 申报值等于末段出值。
    pub fn close(&mut self, value: u32) -> Result<u32, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RL_EFROZEN));
        }
        if self.len != STAGES {
            return Err(self.fail(RL_ECOUNT));
        }
        let tail = match self.last() {
            Some(last) => last.carry_out,
            None => return Err(self.fail(RL_ECOUNT)),
        };
        if value != tail {
            return Err(self.fail(RL_ETAIL));
        }
        self.closed = Some(value);
        Ok(value)
    }

    /// 损耗合计。
    pub fn total_loss(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.relays.get(i) {
                sum = sum.wrapping_add(r.loss);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 逐段算术全合。
    pub fn rows_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.relays.get(i) {
                Some(r) => {
                    if r.stage as usize != i.wrapping_add(1) || !r.arith_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 承接链：首段入值等于开闸值，逐段入值接上段出值。
    pub fn chain_ok(&self) -> bool {
        let mut expected = match self.opened {
            Some(v) => v,
            None => return false,
        };
        let mut i = 0usize;
        while i < self.len {
            match self.relays.get(i) {
                Some(r) => {
                    if r.carry_in != expected {
                        return false;
                    }
                    expected = r.carry_out;
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 总量守恒：开闸值减损耗合计等于关账值。
    pub fn conserved(&self) -> bool {
        match (self.opened, self.closed) {
            (Some(o), Some(c)) => {
                if self.total_loss() > o {
                    return false;
                }
                o.wrapping_sub(self.total_loss()) == c
            }
            _ => false,
        }
    }

    /// 交接口值（末段出值，经关闸确认）。
    pub fn handoff_value(&self) -> Option<u32> {
        match (self.closed, self.last()) {
            (Some(c), Some(last)) => {
                if c == last.carry_out {
                    Some(c)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.relays.get(i) {
                h = (h ^ r.stage as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.carry_in as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.loss as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.carry_out as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let o = match self.opened {
            Some(v) => v as u64,
            None => 0,
        };
        let c = match self.closed {
            Some(v) => v as u64,
            None => 0,
        };
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ o).wrapping_mul(FNV_PRIME);
        h = (h ^ c).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：六段齐、算式与承接全过、守恒成立、交接值落槽。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.opened.is_none() {
            return Err(RL_EHEAD);
        }
        if self.len != STAGES {
            return Err(RL_ECOUNT);
        }
        if !self.rows_ok() {
            return Err(RL_EARITH);
        }
        if !self.chain_ok() {
            return Err(RL_ECHAIN);
        }
        if !self.conserved() {
            return Err(RL_ETAIL);
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(RL_EFROZEN));
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

/// 给定入值与段号生成一段规格接力。
pub fn spec_from(carry_in: u32, stage: u8) -> Relay {
    let mut r = Relay::blank();
    if stage == 0 || stage as usize > STAGES {
        return r;
    }
    let loss = LOSS_STEP.wrapping_mul(stage as u32);
    r.stage = stage;
    r.carry_in = carry_in;
    r.loss = loss;
    r.carry_out = carry_in.saturating_sub(loss);
    r
}

/// 第 stage 段规格：入值由开闸值逐段扣减损耗推得。
pub fn spec_of(stage: usize) -> (u32, u32, u32) {
    if stage == 0 || stage > STAGES {
        return (0, 0, 0);
    }
    let mut carry = OPENING;
    let mut k = 1usize;
    while k <= stage {
        carry = carry.saturating_sub(LOSS_STEP.wrapping_mul(k as u32));
        k = k.wrapping_add(1);
    }
    let loss = LOSS_STEP.wrapping_mul(stage as u32);
    let carry_in = carry.wrapping_add(loss);
    (carry_in, loss, carry)
}

/// 规格关账值。
pub fn closing_spec() -> u32 {
    spec_of(STAGES).2
}

/// 构建标准接力账：开闸 → 六段 → 关闸。
pub fn build_ledger() -> RelayLedger {
    let mut l = RelayLedger::new();
    let _ = l.open(OPENING);
    let mut stage = 1usize;
    while stage <= STAGES {
        let _ = l.observe(stage);
        stage = stage.wrapping_add(1);
    }
    let tail = match l.last() {
        Some(r) => r.carry_out,
        None => 0,
    };
    let _ = l.close(tail);
    l
}

/// 四探针：越界、重复/倒序、算术/承接、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = RelayLedger::new();
    let _ = a.open(OPENING);
    let _ = a.observe(1);
    let mut zero = spec_from(OPENING, 1);
    zero.stage = 0;
    let mut far = spec_from(OPENING, 1);
    far.stage = (STAGES + 1) as u8;
    if a.register(zero) == Err(RL_ENO) && a.register(far) == Err(RL_ENO) {
        r[0] = true;
    }

    let mut b = RelayLedger::new();
    let _ = b.open(OPENING);
    let b1 = spec_from(OPENING, 1);
    let _ = b.register(b1);
    if b.register(b1) == Err(RL_EDUP) {
        r[1] = true;
    }
    let mut back = RelayLedger::new();
    let _ = back.open(OPENING);
    let _ = back.register(spec_from(OPENING, 2));
    if back.register(spec_from(OPENING, 1)) != Err(RL_EORDER) {
        r[1] = false;
    }

    let mut c = RelayLedger::new();
    let _ = c.open(OPENING);
    let _ = c.register(spec_from(OPENING, 1));
    let out1 = spec_from(OPENING, 1).carry_out;
    let mut broken = spec_from(out1, 2);
    broken.carry_out = broken.carry_out.wrapping_add(1);
    let mut displaced = spec_from(out1.wrapping_add(1), 2);
    displaced.carry_out = displaced.carry_in.wrapping_sub(displaced.loss);
    let mut overdrawn = spec_from(out1, 2);
    overdrawn.loss = overdrawn.carry_in.wrapping_add(1);
    if c.register(broken) == Err(RL_EARITH)
        && c.register(displaced) == Err(RL_ECHAIN)
        && c.register(overdrawn) == Err(RL_EARITH)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(RL_EFROZEN)
        && d.register(spec_from(1, 1)) == Err(RL_EFROZEN)
        && d.open(OPENING) == Err(RL_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11178");
    let base = build_ledger();

    s.add(
        "S1-接力账逐项对平",
        base.len() == STAGES
            && base.accepted() == STAGES as u32
            && base.rejected() == 0
            && base.rows_ok(),
        "六段全收，逐段入值减损耗等于出值",
    );

    s.add(
        "S2-逐段承接",
        base.chain_ok() && base.opened() == Some(OPENING),
        "首段入值等于开闸值，逐段入值接上段出值",
    );

    let asserted = base.conserved() && base.handoff_value().is_some();
    let handoff = match (base.handoff_value(), base.closed()) {
        (Some(h), Some(c)) => h == c,
        _ => false,
    };
    s.add(
        "S3-接力断言成立",
        asserted && handoff,
        "开闸值减损耗合计等于关账值，交接值等于关账值",
    );

    let mut spec_loss = 0u32;
    let mut stage = 1usize;
    while stage <= STAGES {
        spec_loss = spec_loss.wrapping_add(spec_of(stage).1);
        stage = stage.wrapping_add(1);
    }
    s.add(
        "S4-总量守恒复算",
        base.total_loss() == spec_loss
            && base.closed() == Some(closing_spec())
            && base.opened() == Some(OPENING.wrapping_sub(spec_loss).wrapping_add(spec_loss)),
        "损耗合计与规格相等，关账值等于规格闭账值",
    );

    let mut positive = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.relay_at(i) {
            Some(r) => {
                if r.carry_in == 0 || r.carry_out == 0 || r.loss > r.carry_in {
                    positive = false;
                }
            }
            None => positive = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("S5-闸值非零", positive, "每段入值与出值均正且损耗不越入值");

    let mut per_stage = true;
    let mut k = 0usize;
    while k < base.len() {
        match base.relay_at(k) {
            Some(r) => {
                let spec = spec_of(r.stage as usize);
                if r.carry_in != spec.0 || r.loss != spec.1 || r.carry_out != spec.2 {
                    per_stage = false;
                }
            }
            None => per_stage = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S6-逐段规格对平", per_stage, "每段入值损耗出值均等于规格");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(RL_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放六段承接与守恒全过且指纹一致",
    );

    let mut early = RelayLedger::new();
    let _ = early.open(OPENING);
    let _ = early.observe(1);
    let _ = early.observe(2);
    let _ = early.observe(3);
    let short = early.close(OPENING);
    let mut wrong = RelayLedger::new();
    let _ = wrong.open(OPENING);
    let mut st = 1usize;
    while st <= STAGES {
        let _ = wrong.observe(st);
        st = st.wrapping_add(1);
    }
    let bad_tail = wrong.close(OPENING);
    s.add(
        "S10-关账闸",
        short == Err(RL_ECOUNT) && bad_tail == Err(RL_ETAIL),
        "段数不齐判不齐，关账值不符判不符",
    );

    let mut la = RelayLedger::new();
    let _ = la.open(OPENING);
    let _ = la.observe(1);
    let mut zero = spec_from(OPENING, 1);
    zero.stage = 0;
    let mut far = spec_from(OPENING, 1);
    far.stage = (STAGES + 1) as u8;
    let r0 = la.register(zero);
    let r1 = la.register(far);
    s.add(
        "X11-越界拒收",
        r0 == Err(RL_ENO) && r1 == Err(RL_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "段号 0 与 7 均判越界",
    );

    let mut lb = RelayLedger::new();
    let _ = lb.open(OPENING);
    let b1 = spec_from(OPENING, 1);
    let _ = lb.register(b1);
    let dup = lb.register(b1);
    let mut lc = RelayLedger::new();
    let _ = lc.open(OPENING);
    let _ = lc.register(spec_from(OPENING, 2));
    let ord = lc.register(spec_from(OPENING, 1));
    s.add(
        "X12-重复倒序",
        dup == Err(RL_EDUP) && ord == Err(RL_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重记一段判重复、二段后补一段判倒序",
    );

    let mut ld = RelayLedger::new();
    let _ = ld.open(OPENING);
    let _ = ld.register(spec_from(OPENING, 1));
    let out1 = spec_from(OPENING, 1).carry_out;
    let mut broken = spec_from(out1, 2);
    broken.carry_out = broken.carry_out.wrapping_add(1);
    let mut displaced = spec_from(out1.wrapping_add(1), 2);
    displaced.carry_out = displaced.carry_in.wrapping_sub(displaced.loss);
    let mut overdrawn = spec_from(out1, 2);
    overdrawn.loss = overdrawn.carry_in.wrapping_add(1);
    let ar = ld.register(broken);
    let ch = ld.register(displaced);
    let ov = ld.register(overdrawn);
    s.add(
        "X13-算术承接损耗",
        ar == Err(RL_EARITH)
            && ch == Err(RL_ECHAIN)
            && ov == Err(RL_EARITH)
            && ld.accepted() == 1
            && ld.rejected() == 3,
        "算式不符、承接错位、损耗越入值分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11178_relay_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), STAGES);
        assert_eq!(l.accepted(), STAGES as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.rows_ok());
        assert!(l.chain_ok());
        assert!(l.conserved());
        assert_eq!(l.opened(), Some(OPENING));
        assert_eq!(l.closed(), Some(closing_spec()));
        assert_eq!(l.handoff_value(), Some(closing_spec()));
        assert_eq!(l.relay_at(0).map(|r| r.carry_in), Some(OPENING));
        assert!(l.relay_at(STAGES).is_none());
        assert!(l.find(6).is_some());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11178_spec_chain() {
        assert_eq!(spec_of(0), (0, 0, 0));
        assert_eq!(spec_of(STAGES + 1), (0, 0, 0));
        let mut prev = OPENING;
        let mut stage = 1usize;
        while stage <= STAGES {
            let (inp, loss, out) = spec_of(stage);
            assert_eq!(inp, prev);
            assert_eq!(inp.wrapping_sub(loss), out);
            assert!(out > 0);
            prev = out;
            stage = stage.wrapping_add(1);
        }
        assert_eq!(prev, closing_spec());
        assert_eq!(closing_spec(), OPENING - LOSS_STEP * 21);
        assert_eq!(spec_from(OPENING, 0).carry_in, 0);
    }

    #[test]
    fn f11178_gates() {
        let mut a = RelayLedger::new();
        assert_eq!(a.register(spec_from(OPENING, 1)), Err(RL_EHEAD));
        assert_eq!(a.open(0), Err(RL_EHEAD));
        assert_eq!(a.open(OPENING), Ok(OPENING));
        assert_eq!(a.open(OPENING), Err(RL_EHEAD));
        assert!(a.observe(1).is_ok());
        assert_eq!(a.observe(1), Err(RL_EDUP));
        assert!(a.observe(3).is_ok());
        assert_eq!(a.observe(2), Err(RL_EORDER));
        let mut b = RelayLedger::new();
        let _ = b.open(OPENING);
        let mut broken = spec_from(OPENING, 1);
        broken.carry_out = broken.carry_out.wrapping_add(1);
        assert_eq!(b.register(broken), Err(RL_EARITH));
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 5);
    }

    #[test]
    fn f11178_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(RL_EFROZEN));
        assert_eq!(l.open(OPENING), Err(RL_EFROZEN));
    }

    #[test]
    fn f11178_checks_green() {
        let s = run_unx11178_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
