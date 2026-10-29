//! UNX-F11181 闭账场搭建（域闭账账与终账序）（0x1f510 段）
//!
//! 模型：B40 开场前先搭一座闭账场。场上有八个阶，每阶登记阶号、终账序秩、
//! 依赖掩码、闭账位与阶封。与按号递增的清单不同，终账序由「秩」决定：
//! 八阶的秩恰为一至八的双射，登记次序仍是阶号一至八。依赖掩码对秩一阶
//! 必须为零，对其余阶恰为一位——指向终账序中紧前的那一阶。规则：阶号与秩
//! 各不得重复；闭账位必须已亮；阶封由阶号、秩与依赖复算。终核要求全链可追
//! ——自任一阶沿依赖逐级回溯必达秩一阶；并要求拓扑合法——依赖阶的秩恰为
//! 本阶秩减一。全部通过后按秩排序折叠出终账序签名并冻结；二次冻结与再写
//! 返回冻结码。依赖形状、闭账位与阶封的越界值由规则函数判定。

use crate::checks::CheckSet;

/// 段基址（B40 段 · 闭账场）。
const SEG_BASE: u32 = 0x1f510;

/// 阶号或秩越界。
pub const OR_ENO: u32 = SEG_BASE + 0x01;
/// 阶号或秩重复。
pub const OR_EDUP: u32 = SEG_BASE + 0x02;
/// 阶号倒序。
pub const OR_EORDER: u32 = SEG_BASE + 0x03;
/// 依赖掩码形状不符。
pub const OR_EDEP: u32 = SEG_BASE + 0x04;
/// 闭账位未亮。
pub const OR_ECLOSED: u32 = SEG_BASE + 0x05;
/// 阶封不符。
pub const OR_ESEAL: u32 = SEG_BASE + 0x06;
/// 全链或拓扑不成立。
pub const OR_ECHAIN: u32 = SEG_BASE + 0x07;
/// 阶数不齐。
pub const OR_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const OR_EFROZEN: u32 = SEG_BASE + 0x09;

/// 阶数。
pub const STAGES: usize = 8;
/// 秩基。
pub const RANK_BASE: u8 = 1;
/// 全秩掩码。
pub const RANK_MASK: u8 = 0xff;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一阶闭账。
#[derive(Clone, Copy, Debug)]
pub struct Stage {
    pub id: u8,
    pub rank: u8,
    pub deps: u8,
    pub closed: bool,
    pub seal: u32,
}

impl Stage {
    /// 空阶。
    pub const fn blank() -> Stage {
        Stage {
            id: 0,
            rank: 0,
            deps: 0,
            closed: false,
            seal: 0,
        }
    }

    /// 依赖形状：秩一阶零依赖，其余阶恰一位依赖。
    pub fn dep_shape_ok(&self) -> bool {
        if self.rank < RANK_BASE || self.rank as usize > STAGES {
            return false;
        }
        if self.rank == RANK_BASE {
            self.deps == 0
        } else {
            self.deps != 0 && self.deps.count_ones() == 1
        }
    }

    /// 阶封：阶号、秩、依赖卷成 32 位。
    pub fn seal_of(id: u8, rank: u8, deps: u8) -> u32 {
        let mut x = (id as u32).wrapping_mul(0x9e37_79b1) ^ (rank as u32);
        x = x.rotate_left(17) ^ (deps as u32);
        let y = x.wrapping_mul(0x85eb_ca6b);
        if y == 0 {
            1
        } else {
            y
        }
    }

    /// 阶封自洽。
    pub fn seal_ok(&self) -> bool {
        self.seal == Stage::seal_of(self.id, self.rank, self.deps)
    }
}

/// 闭账场台账。
#[derive(Clone)]
pub struct StageLedger {
    stages: [Stage; STAGES],
    count: usize,
    accepts: u32,
    rejects: u32,
    frozen: bool,
    locked: u64,
}

impl StageLedger {
    /// 空台账。
    pub const fn new() -> StageLedger {
        StageLedger {
            stages: [Stage::blank(); STAGES],
            count: 0,
            accepts: 0,
            rejects: 0,
            frozen: false,
            locked: 0,
        }
    }

    /// 记一次拒止并返回码。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收阶数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 受理数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 冻结位。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结值。
    pub fn locked(&self) -> u64 {
        self.locked
    }

    /// 按登记位取阶。
    pub fn stage_at(&self, pos: usize) -> Option<Stage> {
        if pos < self.count {
            self.stages.get(pos).copied()
        } else {
            None
        }
    }

    /// 按阶号找阶。
    pub fn find(&self, id: u8) -> Option<Stage> {
        let mut i = 0usize;
        while i < self.count {
            if let Some(st) = self.stages.get(i) {
                if st.id == id {
                    return Some(*st);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 秩掩码。
    pub fn rank_mask(&self) -> u8 {
        let mut mask = 0u8;
        let mut i = 0usize;
        while i < self.count {
            if let Some(st) = self.stages.get(i) {
                mask |= 1u8 << (st.rank.wrapping_sub(RANK_BASE) & 7);
            }
            i = i.wrapping_add(1);
        }
        mask
    }

    /// 收一阶：冻结、阶号域、秩域、重复、倒序、依赖形状、闭账位、阶封。
    pub fn register(&mut self, st: Stage) -> Result<usize, u32> {
        if self.frozen {
            return Err(self.fail(OR_EFROZEN));
        }
        if st.id < 1 || st.id as usize > STAGES {
            return Err(self.fail(OR_ENO));
        }
        if st.rank < RANK_BASE || st.rank as usize > STAGES {
            return Err(self.fail(OR_ENO));
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(prev) = self.stages.get(i) {
                if prev.id == st.id {
                    return Err(self.fail(OR_EDUP));
                }
                if prev.rank == st.rank {
                    return Err(self.fail(OR_EDUP));
                }
            }
            i = i.wrapping_add(1);
        }
        if self.count > 0 {
            if let Some(last) = self.stages.get(self.count.wrapping_sub(1)) {
                if st.id <= last.id {
                    return Err(self.fail(OR_EORDER));
                }
            }
        }
        if !st.dep_shape_ok() {
            return Err(self.fail(OR_EDEP));
        }
        if !st.closed {
            return Err(self.fail(OR_ECLOSED));
        }
        if !st.seal_ok() {
            return Err(self.fail(OR_ESEAL));
        }
        if let Some(slot) = self.stages.get_mut(self.count) {
            *slot = st;
        }
        self.count = self.count.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.count)
    }

    /// 观一阶：按阶号取秩、依赖与闭账位。
    pub fn observe(&mut self, id: usize) -> Result<(u8, u8, bool), u32> {
        if self.frozen {
            return Err(self.fail(OR_EFROZEN));
        }
        if id < 1 || id > STAGES {
            return Err(self.fail(OR_ENO));
        }
        match self.find(id as u8) {
            Some(st) => {
                self.accepts = self.accepts.wrapping_add(1);
                Ok((st.rank, st.deps, st.closed))
            }
            None => Err(self.fail(OR_ENO)),
        }
    }
}

/// 阶号对应的终账序秩：1→6、2→3、3→8、4→5、5→2、6→7、7→4、8→1。
pub fn rank_of(id: u8) -> u8 {
    if id < 1 || id as usize > STAGES {
        return 0;
    }
    (id.wrapping_mul(5) % STAGES as u8).wrapping_add(1)
}

/// 阶号对应的依赖掩码：指向秩紧前的那一阶，秩一阶为零。
pub fn dep_of(id: u8) -> u8 {
    let rank = rank_of(id);
    if rank <= RANK_BASE {
        return 0;
    }
    // 依赖阶的秩为 rank-1，其阶号按 rank_of 的逆式算出。
    let dep_rank = rank.wrapping_sub(2);
    let mut dep_id = dep_rank.wrapping_mul(5) % STAGES as u8;
    if dep_id == 0 {
        dep_id = STAGES as u8;
    }
    1u8 << dep_id.wrapping_sub(1)
}

/// 按阶号造规格阶。
fn stage_spec(id: usize) -> Stage {
    let sid = id as u8;
    let rank = rank_of(sid);
    let deps = dep_of(sid);
    Stage {
        id: sid,
        rank,
        deps,
        closed: true,
        seal: Stage::seal_of(sid, rank, deps),
    }
}

/// 规格四元组：秩、依赖、闭账位、阶封。
pub fn spec_of(id: usize) -> (u8, u8, bool, u32) {
    if id < 1 || id > STAGES {
        return (0, 0, false, 0);
    }
    let st = stage_spec(id);
    (st.rank, st.deps, st.closed, st.seal)
}

impl StageLedger {
    /// 秩为一至八的双射。
    pub fn ranks_bijective(&self) -> bool {
        self.count == STAGES && self.rank_mask() == RANK_MASK
    }

    /// 闭账位全亮。
    pub fn all_closed(&self) -> bool {
        if self.count != STAGES {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            match self.stages.get(i) {
                Some(st) => {
                    if !st.closed {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 阶封全部自洽。
    pub fn seals_ok(&self) -> bool {
        if self.count != STAGES {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            match self.stages.get(i) {
                Some(st) => {
                    if !st.seal_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 依赖均指向已收的阶，且秩一阶零依赖。
    pub fn deps_resolve(&self) -> bool {
        if self.count != STAGES {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            match self.stages.get(i) {
                Some(st) => {
                    if st.rank == RANK_BASE {
                        if st.deps != 0 {
                            return false;
                        }
                    } else {
                        if st.deps.count_ones() != 1 {
                            return false;
                        }
                        let dep_id = st.deps.trailing_zeros().wrapping_add(1) as u8;
                        if self.find(dep_id).is_none() {
                            return false;
                        }
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 拓扑合法：依赖阶的秩恰为本阶秩减一。
    pub fn topology_ok(&self) -> bool {
        if self.count != STAGES {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            match self.stages.get(i) {
                Some(st) => {
                    if st.rank == RANK_BASE {
                        if st.deps != 0 {
                            return false;
                        }
                    } else {
                        if st.deps.count_ones() != 1 {
                            return false;
                        }
                        let dep_id = st.deps.trailing_zeros().wrapping_add(1) as u8;
                        match self.find(dep_id) {
                            Some(dep) => {
                                if dep.rank.wrapping_add(1) != st.rank {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }
}

impl StageLedger {
    /// 全链可追：任一阶沿依赖逐级回溯必达秩一阶。
    pub fn chain_to_root(&self) -> bool {
        if self.count != STAGES {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            match self.stages.get(i) {
                Some(st) => {
                    let mut cur = *st;
                    let mut steps = 0usize;
                    while cur.rank > RANK_BASE && steps < STAGES {
                        if cur.deps.count_ones() != 1 {
                            return false;
                        }
                        let dep_id = cur.deps.trailing_zeros().wrapping_add(1) as u8;
                        match self.find(dep_id) {
                            Some(dep) => cur = dep,
                            None => return false,
                        }
                        steps = steps.wrapping_add(1);
                    }
                    if cur.rank != RANK_BASE {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 终账序签名：按秩一至八串联各阶五元。
    pub fn order_signature(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut rank = RANK_BASE;
        while rank as usize <= STAGES {
            let mut i = 0usize;
            while i < self.count {
                if let Some(st) = self.stages.get(i) {
                    if st.rank == rank {
                        h = (h ^ st.id as u64).wrapping_mul(FNV_PRIME);
                        h = (h ^ st.rank as u64).wrapping_mul(FNV_PRIME);
                        h = (h ^ st.deps as u64).wrapping_mul(FNV_PRIME);
                        h = (h ^ st.closed as u64).wrapping_mul(FNV_PRIME);
                        h = (h ^ st.seal as u64).wrapping_mul(FNV_PRIME);
                    }
                }
                i = i.wrapping_add(1);
            }
            rank = rank.wrapping_add(1);
        }
        h
    }
}

impl StageLedger {
    /// 折叠台账全部字段。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.count as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.locked).wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < self.count {
            if let Some(st) = self.stages.get(i) {
                h = (h ^ st.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.rank as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.deps as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.closed as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ st.seal as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        (h ^ self.order_signature()).wrapping_mul(FNV_PRIME)
    }

    /// 重放：齐、双射、依赖、拓扑、全链、闭账位、阶封逐项复核。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.count != STAGES {
            return Err(OR_ECOUNT);
        }
        if !self.ranks_bijective() {
            return Err(OR_EDUP);
        }
        if !self.deps_resolve() {
            return Err(OR_EDEP);
        }
        if !self.topology_ok() || !self.chain_to_root() {
            return Err(OR_ECHAIN);
        }
        if !self.all_closed() {
            return Err(OR_ECLOSED);
        }
        if !self.seals_ok() {
            return Err(OR_ESEAL);
        }
        Ok(self.fold())
    }

    /// 冻结终账序签名；二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(OR_EFROZEN));
        }
        match self.replay() {
            Ok(_) => {
                self.locked = self.order_signature();
                self.frozen = true;
                Ok(self.locked)
            }
            Err(code) => Err(self.fail(code)),
        }
    }
}

/// 按阶号、秩、依赖造阶（闭账位可控），阶封现算。
fn bare_stage(id: u8, rank: u8, deps: u8, closed: bool) -> Stage {
    Stage {
        id,
        rank,
        deps,
        closed,
        seal: Stage::seal_of(id, rank, deps),
    }
}

/// 按规格搭一座八阶闭账场。
pub fn build_ledger() -> StageLedger {
    let mut l = StageLedger::new();
    let mut id = 1usize;
    while id <= STAGES {
        let _ = l.register(stage_spec(id));
        id = id.wrapping_add(1);
    }
    l
}

/// 四路注入：重复阶号、重复秩、坏依赖形状、冻结后再写。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l1 = StageLedger::new();
    let _ = l1.register(stage_spec(1));
    out[0] = l1.register(stage_spec(1)) == Err(OR_EDUP);

    let mut l2 = StageLedger::new();
    let _ = l2.register(stage_spec(1));
    let twin_rank = stage_spec(1).rank;
    out[1] = l2.register(bare_stage(2, twin_rank, 0x01, true)) == Err(OR_EDUP);

    let mut l3 = StageLedger::new();
    let _ = l3.register(stage_spec(1));
    out[2] = l3.register(bare_stage(2, 3, 0b11, true)) == Err(OR_EDEP);

    let mut l4 = build_ledger();
    let _ = l4.freeze();
    out[3] = l4.register(stage_spec(1)) == Err(OR_EFROZEN);

    out
}

/// 十三项自检。
pub fn run_unx11181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11181");
    let base = build_ledger();

    let mut ids = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.stage_at(i) {
            Some(st) => {
                if st.id as usize != i.wrapping_add(1) {
                    ids = false;
                }
            }
            None => ids = false,
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "S1-八阶齐",
        base.len() == STAGES && base.accepted() == STAGES as u32 && base.rejected() == 0 && ids,
        "八阶按阶号一至八收齐且无拒止",
    );

    s.add(
        "S2-闭账场全链搭建",
        base.chain_to_root(),
        "任一阶沿依赖逐级回溯必达秩一阶",
    );
    s.add(
        "S3-终账序双射",
        base.ranks_bijective() && base.rank_mask() == RANK_MASK,
        "八阶的秩恰为一至八各一次",
    );
    s.add(
        "S4-拓扑序合法",
        base.topology_ok(),
        "依赖阶的秩恰为本阶秩减一",
    );
    s.add(
        "S5-闭账位全亮",
        base.all_closed(),
        "八阶闭账位均为真",
    );
    s.add(
        "S6-阶封可复算",
        base.seals_ok(),
        "八阶阶封均等于阶号秩依赖的复算值",
    );
    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => {
            v2 == Err(OR_EFROZEN) && fz.locked() == v && fz.locked() == base.order_signature()
        }
        Err(_) => false,
    };
    s.add(
        "S8-终账序冻结",
        single && fz.is_frozen(),
        "首冻锁终账序签名，二次冻结返回冻结码",
    );

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放全链拓扑与闭账位后折叠同值",
    );

    let mut per = true;
    let mut k = 0usize;
    while k < base.len() {
        match base.stage_at(k) {
            Some(st) => {
                let (rank, deps, closed, seal) = spec_of(st.id as usize);
                if st.rank != rank || st.deps != deps || st.closed != closed || st.seal != seal {
                    per = false;
                }
            }
            None => per = false,
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "S10-逐阶规格对平",
        per,
        "每阶的秩、依赖、闭账位与阶封均等于规格",
    );

    let mut la = StageLedger::new();
    let lo = la.register(bare_stage(0, 1, 0, true));
    let hi = la.register(bare_stage((STAGES + 1) as u8, 1, 0, true));
    let rank_lo = la.register(bare_stage(1, 0, 0, true));
    let rank_hi = la.register(bare_stage(1, (STAGES + 1) as u8, 0, true));
    s.add(
        "X11-阶号秩越界",
        lo == Err(OR_ENO)
            && hi == Err(OR_ENO)
            && rank_lo == Err(OR_ENO)
            && rank_hi == Err(OR_ENO)
            && la.accepted() == 0
            && la.rejected() == 4,
        "阶号 0、9 与秩 0、9 均判越界",
    );

    let mut lb = StageLedger::new();
    let first = stage_spec(1);
    let _ = lb.register(first);
    let dup_id = lb.register(first);
    let mut lc = StageLedger::new();
    let _ = lc.register(stage_spec(1));
    let twin_rank = stage_spec(1).rank;
    let dup_rank = lc.register(bare_stage(2, twin_rank, 0x01, true));
    let mut ld = StageLedger::new();
    let _ = ld.register(stage_spec(2));
    let back = ld.register(stage_spec(1));
    s.add(
        "X12-重复倒序",
        dup_id == Err(OR_EDUP)
            && dup_rank == Err(OR_EDUP)
            && back == Err(OR_EORDER)
            && lb.accepted() == 1
            && lc.accepted() == 1
            && ld.rejected() == 1,
        "阶号重复、秩重复与倒序补阶分别拒收",
    );

    let mut le = StageLedger::new();
    let _ = le.register(stage_spec(1));
    let wide = le.register(bare_stage(2, 3, 0b11, true));
    let shut = le.register(bare_stage(2, 3, 0x01, false));
    let mut bad_seal = bare_stage(2, 3, 0x01, true);
    bad_seal.seal ^= 1;
    let sealed = le.register(bad_seal);
    s.add(
        "X13-形状闭位阶封",
        wide == Err(OR_EDEP)
            && shut == Err(OR_ECLOSED)
            && sealed == Err(OR_ESEAL)
            && le.accepted() == 1
            && le.rejected() == 3,
        "坏依赖形状、未闭账与坏阶封分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11181_orders_bijective() {
        let l = build_ledger();
        assert_eq!(l.len(), STAGES);
        assert!(l.ranks_bijective());
        assert_eq!(l.rank_mask(), RANK_MASK);
        assert!(l.topology_ok());
        assert!(l.chain_to_root());
        assert!(l.deps_resolve());
    }

    #[test]
    fn f11181_register_gates() {
        let mut l = StageLedger::new();
        assert_eq!(l.register(stage_spec(1)), Ok(1));
        assert_eq!(l.register(stage_spec(1)), Err(OR_EDUP));
        let mut m = StageLedger::new();
        assert_eq!(m.register(stage_spec(2)), Ok(1));
        assert_eq!(m.register(stage_spec(1)), Err(OR_EORDER));
        assert_eq!(m.register(bare_stage(9, 3, 0x01, true)), Err(OR_ENO));
    }

    #[test]
    fn f11181_shape_closed_seal() {
        let mut l = StageLedger::new();
        assert_eq!(l.register(stage_spec(1)), Ok(1));
        assert_eq!(l.register(bare_stage(2, 3, 0b11, true)), Err(OR_EDEP));
        assert_eq!(l.register(bare_stage(2, 3, 0x01, false)), Err(OR_ECLOSED));
        let mut bad = bare_stage(2, 3, 0x01, true);
        bad.seal ^= 1;
        assert_eq!(l.register(bad), Err(OR_ESEAL));
        assert_eq!(l.accepted(), 1);
        assert_eq!(l.rejected(), 3);
    }

    #[test]
    fn f11181_freeze_locks_order() {
        let mut l = build_ledger();
        let sig = l.order_signature();
        assert_eq!(l.freeze(), Ok(sig));
        assert!(l.is_frozen());
        assert_eq!(l.locked(), sig);
        assert_eq!(l.freeze(), Err(OR_EFROZEN));
        assert_eq!(l.register(stage_spec(1)), Err(OR_EFROZEN));
    }

    #[test]
    fn f11181_specs() {
        assert_eq!(rank_of(1), 6);
        assert_eq!(rank_of(8), 1);
        assert_eq!(dep_of(8), 0);
        assert_eq!(dep_of(1), 0x08);
        assert_eq!(dep_of(3), 0x20);
        assert_eq!(spec_of(0), (0, 0, false, 0));
        let l = build_ledger();
        assert_eq!(l.find(3).map(|st| st.rank), Some(8));
    }

    #[test]
    fn f11181_replay_and_checks() {
        let l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        assert!(inject_four().iter().all(|v| *v));
        let s = run_unx11181_checks();
        assert!(s.all_passed(), "{:?}", s);
    }
}
