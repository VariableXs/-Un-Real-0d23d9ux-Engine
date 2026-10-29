//! UNX-F11147 B27 清账（SysV IPC 错误矩阵判据终重放·回滚账）（0x1f2f0 段）
//!
//! 纯功能模型：SysV IPC 的六族调用分为「取用族」（shmget / msgget / semget）
//! 与「操作族」（shmat / msgsnd / semop）。取用族成功即产生一个驻留资源，
//! 操作族失败则必须把本轮占用的资源回滚干净——这是本卡的第二本账。
//!
//! 双账结构：
//!   - 错误矩阵账：每例预先登记期望 errno，重放逐例比对；
//!   - 回滚账：期望失败的例，其键必须在回滚账上恰好出现一次；期望成功的例，
//!     其键必须在驻留账上恰好出现一次，且不得出现在回滚账上（ELIVE）。
//!   两账交叉必须自洽：驻留数 = 成功例数，回滚数 = 失败例数，键集合互斥。
//!
//! 规则面（对齐 System V IPC 语义的裁剪版）：
//!   - 键 0（IPC_PRIVATE）在取用族合法、在操作族落 EINVAL；
//!   - 键最高位置 1 视为越权，落 EACCES；
//!   - 取用族键能整除 3 视为已存在，落 EEXIST；
//!   - 操作族键能整除 5 视为资源已撤，落 EIDRM；
//!   - 其余组合成功。
//!
//! 拒止面：例号越界、重复登记、例序倒挂、期望码越界、未登记即观测、
//! 缺实测、不符、回滚账与矩阵账不符、回滚成功例、重复回滚、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2f0;
/// 例号越界
pub const SV_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const SV_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const SV_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const SV_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const SV_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const SV_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const SV_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 回滚账与矩阵账不符
pub const SV_ERB: u32 = SEG_BASE + 0x08;
/// 回滚了应驻留的成功例
pub const SV_ELIVE: u32 = SEG_BASE + 0x09;
/// 已封盘
pub const SV_EFROZEN: u32 = SEG_BASE + 0x0a;

/// 调用族数。
pub const OPS: usize = 6;
/// 族名（前三个取用族、后三个操作族）。
pub const OP_NAMES: [&str; OPS] = ["shmget", "msgget", "semget", "shmat", "msgsnd", "semop"];
/// 取用族个数。
pub const GET_OPS: usize = 3;
/// 每族例数。
pub const PER_OP: usize = 3;
/// 总例数。
pub const CASES: usize = OPS * PER_OP;
/// 回滚账容量。
pub const MAX_KEYS: usize = CASES;
/// IPC_PRIVATE。
pub const IPC_PRIVATE: u32 = 0;
/// 权限位高标。
pub const KEY_DENY: u32 = 0x8000_0000;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;

/// EACCES
pub const EACCES: u32 = 13;
/// EEXIST
pub const EEXIST: u32 = 17;
/// EINVAL
pub const EINVAL: u32 = 22;
/// EIDRM
pub const EIDRM: u32 = 43;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 是否取用族。
pub fn is_get_op(op: u8) -> bool {
    (op as usize) < GET_OPS
}

/// 规则纯函数：由（调用, 键）给出应有 errno。
pub fn rule_error(op: u8, key: u32) -> u32 {
    if op as usize >= OPS {
        return EINVAL;
    }
    if key & KEY_DENY != 0 {
        return EACCES;
    }
    if is_get_op(op) {
        // 私有键新建必成功；其余键能整除 3 视为已存在。
        if key == IPC_PRIVATE {
            0
        } else if key % 3 == 0 {
            EEXIST
        } else {
            0
        }
    } else if key == IPC_PRIVATE {
        EINVAL
    } else if key % 5 == 0 {
        EIDRM
    } else {
        0
    }
}

/// 符号键：私有键统一记作 0，其余原样。
pub fn norm_key(key: u32) -> u32 {
    key & !KEY_DENY
}

/// 矩阵一例。
#[derive(Clone, Copy, Debug)]
pub struct SvCase {
    pub op: u8,
    pub key: u32,
    pub expected: u32,
    pub got: u32,
    pub seen: bool,
    pub live: bool,
}

impl SvCase {
    const EMPTY: SvCase = SvCase {
        op: 0,
        key: 0,
        expected: 0,
        got: 0,
        seen: false,
        live: false,
    };

    /// 已测且相符。
    pub fn agrees(&self) -> bool {
        self.live && self.seen && self.got == self.expected
    }

    /// 规则复算一致。
    pub fn rule_agrees(&self) -> bool {
        self.live && rule_error(self.op, self.key) == self.expected
    }

    /// 本例期望成功（即应驻留）。
    pub fn expects_live(&self) -> bool {
        self.live && self.expected == 0
    }

    /// 本例期望失败（即应回滚）。
    pub fn expects_rollback(&self) -> bool {
        self.live && self.expected != 0
    }
}

/// 错误矩阵 + 回滚账。
#[derive(Clone, Debug)]
pub struct RollbackLedger {
    pub base: u32,
    cases: [SvCase; CASES],
    /// 回滚账：按例号记录是否已回滚。
    rolled: [bool; CASES],
    registered: usize,
    observed: usize,
    cursor: usize,
    rollbacks: u32,
    rejects: u32,
    frozen: bool,
}

impl RollbackLedger {
    /// 新建空账。
    pub fn new(base: u32) -> RollbackLedger {
        RollbackLedger {
            base,
            cases: [SvCase::EMPTY; CASES],
            rolled: [false; CASES],
            registered: 0,
            observed: 0,
            cursor: 0,
            rollbacks: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一例。
    pub fn register(&mut self, id: usize, op: u8, key: u32, expected: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SV_EFROZEN));
        }
        if id >= CASES || op as usize >= OPS {
            return Err(self.fail(SV_ECASE));
        }
        if let Some(c) = self.cases.get(id) {
            if c.live {
                return Err(self.fail(SV_EDUP));
            }
        }
        if id < self.cursor {
            return Err(self.fail(SV_EORDER));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(SV_ENOEXP));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            *slot = SvCase {
                op,
                key,
                expected,
                got: 0,
                seen: false,
                live: true,
            };
        }
        self.registered = self.registered.wrapping_add(1);
        self.cursor = id.wrapping_add(1);
        Ok(())
    }

    /// 登记并按规则复算期望。
    pub fn register_ruled(&mut self, id: usize, op: u8, key: u32) -> Result<(), u32> {
        let e = rule_error(op, key);
        self.register(id, op, key, e)
    }

    /// 观测实测。
    pub fn observe(&mut self, id: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SV_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(SV_ECASE));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(SV_ENOEXP));
        }
        let c = match self.cases.get(id) {
            Some(c) => *c,
            None => return Err(self.fail(SV_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(SV_EUNREG));
        }
        if c.seen {
            return Err(self.fail(SV_EDUP));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            slot.got = errno;
            slot.seen = true;
        }
        self.observed = self.observed.wrapping_add(1);
        Ok(())
    }
}

impl RollbackLedger {
    /// 回滚一例：只许回滚已实测的失败例，且每例只许回滚一次。
    pub fn rollback(&mut self, id: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(self.fail(SV_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(SV_ECASE));
        }
        let c = match self.cases.get(id) {
            Some(c) => *c,
            None => return Err(self.fail(SV_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(SV_EUNREG));
        }
        if !c.seen {
            return Err(self.fail(SV_ESHORT));
        }
        if c.expected == 0 {
            return Err(self.fail(SV_ELIVE));
        }
        if let Some(done) = self.rolled.get(id) {
            if *done {
                return Err(self.fail(SV_ERB));
            }
        }
        if let Some(slot) = self.rolled.get_mut(id) {
            *slot = true;
        }
        self.rollbacks = self.rollbacks.wrapping_add(1);
        Ok(c.expected)
    }

    /// 只读单例。
    pub fn case(&self, id: usize) -> Option<SvCase> {
        self.cases.get(id).copied()
    }

    /// 该例是否已回滚。
    pub fn is_rolled(&self, id: usize) -> bool {
        match self.rolled.get(id) {
            Some(v) => *v,
            None => false,
        }
    }

    /// 登记例数。
    pub fn registered(&self) -> usize {
        self.registered
    }

    /// 已测例数。
    pub fn observed(&self) -> usize {
        self.observed
    }

    /// 回滚次数。
    pub fn rollbacks(&self) -> u32 {
        self.rollbacks
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 驻留例数（期望成功且在册）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.expects_live() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 应回滚例数（期望失败且在册）。
    pub fn rollback_targets(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.expects_rollback() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 回滚账是否闭合：应回滚例全部回滚、成功例一例未滚、次数与目标数相等。
    pub fn rollback_closed(&self) -> bool {
        let mut wanted = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.expects_rollback() {
                    wanted = wanted.wrapping_add(1);
                    if !self.is_rolled(i) {
                        return false;
                    }
                } else if c.live && self.is_rolled(i) {
                    return false;
                }
            }
            i = i.wrapping_add(1);
        }
        wanted == self.rollbacks as usize
    }

    /// 不符例数。
    pub fn mismatches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live && c.seen && c.got != c.expected {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 缺口：未登记或未实测。
    pub fn gaps(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            match self.cases.get(i) {
                Some(c) => {
                    if !c.live || !c.seen {
                        n = n.wrapping_add(1);
                    }
                }
                None => n = n.wrapping_add(1),
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 每族例数。
    pub fn op_table(&self) -> [u32; OPS] {
        let mut out = [0u32; OPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    if let Some(slot) = out.get_mut(c.op as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 规则复算一致例数。
    pub fn rule_agreement(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.rule_agrees() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 错误码直方：成功 / EACCES / EEXIST / EINVAL / EIDRM 五桶。
    pub fn errno_histogram(&self) -> [u32; 5] {
        let mut out = [0u32; 5];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let b = if c.expected == 0 {
                        0usize
                    } else if c.expected == EACCES {
                        1usize
                    } else if c.expected == EEXIST {
                        2usize
                    } else if c.expected == EINVAL {
                        3usize
                    } else if c.expected == EIDRM {
                        4usize
                    } else {
                        0usize
                    };
                    if let Some(slot) = out.get_mut(b) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

impl RollbackLedger {
    /// 折叠指纹：基址、三本计数、拒止，再逐例并入（调用, 键, 期望, 实测, 回滚位）。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.registered as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.observed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rollbacks as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    h ^= c.op as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.key as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.expected as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    if c.seen {
                        h ^= c.got as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    let r = match self.rolled.get(i) {
                        Some(v) => *v as u64,
                        None => 0,
                    };
                    h ^= r;
                    h = h.wrapping_mul(FNV_PRIME);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 终重放：矩阵齐、逐例相符、回滚账闭合。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.registered != CASES || self.observed != CASES {
            return Err(self.fail(SV_ESHORT));
        }
        if self.gaps() != 0 {
            return Err(self.fail(SV_ESHORT));
        }
        if self.mismatches() != 0 {
            return Err(self.fail(SV_EMISMATCH));
        }
        if !self.rollback_closed() {
            return Err(self.fail(SV_ERB));
        }
        self.freeze()
    }

    /// 封盘：幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        let root = self.fold();
        if self.frozen {
            return Ok(root);
        }
        self.frozen = true;
        Ok(self.fold())
    }
}

/// 十八例的键表：每族三键，按族序排列。
pub const KEYS: [u32; CASES] = [
    // shmget：私有键、已存在键、普通键
    IPC_PRIVATE, 3, 1,
    // msgget：普通键、越权键、普通键
    1, KEY_DENY | 3, 4,
    // semget：私有键、已存在键、普通键
    IPC_PRIVATE, 3, 2,
    // shmat：私有键（操作族非法）、已撤键、普通键
    IPC_PRIVATE, 5, 2,
    // msgsnd：越权键、已撤键、普通键
    KEY_DENY | 7, 5, 6,
    // semop：私有键、已撤键、普通键
    IPC_PRIVATE, 10, 9,
];

/// 第 id 号用例的（调用, 键）：族 = id / 3，例位 = id % 3。
pub fn spec_of(id: usize) -> (u8, u32) {
    let op = (id / PER_OP) as u8;
    let key = match KEYS.get(id) {
        Some(k) => *k,
        None => 1,
    };
    (op, key)
}

/// 搭建全账：登记、回填、逐失败例回滚，最后终重放。
pub fn build_ledger(base: u32) -> (RollbackLedger, Result<u64, u32>) {
    let mut l = RollbackLedger::new(base);
    let mut i = 0usize;
    while i < CASES {
        let (op, key) = spec_of(i);
        if l.register_ruled(i, op, key).is_err() {
            return (l, Err(SV_ECASE));
        }
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < CASES {
        let want = match l.case(k) {
            Some(c) => c.expected,
            None => 0,
        };
        if l.observe(k, want).is_err() {
            return (l, Err(SV_ECASE));
        }
        k = k.wrapping_add(1);
    }
    let mut r = 0usize;
    while r < CASES {
        let failed = match l.case(r) {
            Some(c) => c.expected != 0,
            None => false,
        };
        if failed && l.rollback(r).is_err() {
            return (l, Err(SV_ERB));
        }
        r = r.wrapping_add(1);
    }
    let out = l.replay();
    (l, out)
}

/// 四路注入：回滚成功例、重复回滚、观测未登记例、未测先回滚。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 回滚一个应驻留的成功例必被拒（ELIVE）。
    let mut a = RollbackLedger::new(0x2400);
    let mut ok1 = a.register_ruled(0, 0, IPC_PRIVATE).is_ok();
    ok1 = ok1 && a.observe(0, 0).is_ok();
    out[0] = ok1 && a.rollback(0).err() == Some(SV_ELIVE);

    // ② 同一失败例回滚两次，第二次必被拒（ERB）。
    let mut b = RollbackLedger::new(0x2401);
    let mut ok2 = b.register_ruled(0, 0, 3).is_ok();
    ok2 = ok2 && b.observe(0, EEXIST).is_ok();
    let first = b.rollback(0);
    let twice = b.rollback(0);
    out[1] = ok2 && first == Ok(EEXIST) && twice.err() == Some(SV_ERB);

    // ③ 观测未登记例必被拒。
    let mut c = RollbackLedger::new(0x2402);
    out[2] = c.observe(7, 0).err() == Some(SV_EUNREG);

    // ④ 未实测就回滚必被拒（回滚账不许抢在实测之前）。
    let mut d = RollbackLedger::new(0x2403);
    let ok4 = d.register_ruled(0, 0, 3).is_ok();
    out[3] = ok4 && d.rollback(0).err() == Some(SV_ESHORT);

    out
}

/// 判据集：R1~R13。
pub fn run_unx11147_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11147.b27_sysv_errmatrix_rollback");

    // R1 常量面：段基址、六族、每族三例、键表齐。
    let consts = SEG_BASE == 0x1f2f0
        && OPS == 6
        && GET_OPS == 3
        && PER_OP == 3
        && CASES == 18
        && KEYS.len() == CASES
        && OP_NAMES.len() == OPS;
    s.add("R1-常量面", consts, "六族 x 三例 = 18，键表 18 项");

    // R2 全账搭建成功并封盘。
    let (l2, r2) = build_ledger(0x1400);
    s.add(
        "R2-搭建封盘",
        r2.is_ok() && l2.frozen(),
        "build_ledger 回滚账闭合后封盘",
    );

    // R3 矩阵齐、实测齐。
    s.add_dyn(
        "R3-两账齐",
        l2.registered() == CASES && l2.observed() == CASES,
        &format!("reg={} obs={}", l2.registered(), l2.observed()),
    );

    // R4 逐例闭合。
    s.add_dyn(
        "R4-逐例闭合",
        l2.mismatches() == 0 && l2.gaps() == 0,
        &format!("mismatch={} gaps={}", l2.mismatches(), l2.gaps()),
    );

    // R5 回滚账闭合：应滚全滚、成功例一例未滚、次数对平。
    s.add_dyn(
        "R5-回滚闭合",
        l2.rollback_closed()
            && l2.rollbacks() as usize == l2.rollback_targets()
            && l2.live_count().wrapping_add(l2.rollback_targets()) == CASES,
        &format!(
            "rolled={} targets={} live={}",
            l2.rollbacks(),
            l2.rollback_targets(),
            l2.live_count()
        ),
    );

    // R6 规则复算一致。
    s.add_dyn(
        "R6-规则一致",
        l2.rule_agreement() == CASES,
        &format!("agree={}/{}", l2.rule_agreement(), CASES),
    );

    // R7 六族覆盖：每族三例。
    let t7 = l2.op_table();
    let mut ok7 = true;
    let mut i7 = 0usize;
    while i7 < OPS {
        ok7 = ok7 && t7[i7] == PER_OP as u32;
        i7 = i7.wrapping_add(1);
    }
    s.add_dyn("R7-六族覆盖", ok7, &format!("{:?}", t7));

    // R8 错误码直方：五桶皆非零、总和等于用例数。
    let h8 = l2.errno_histogram();
    let mut sum8 = 0u32;
    let mut i8 = 0usize;
    let mut all8 = true;
    while i8 < 5 {
        sum8 = sum8.wrapping_add(h8[i8]);
        all8 = all8 && h8[i8] > 0;
        i8 = i8.wrapping_add(1);
    }
    s.add_dyn(
        "R8-直方五桶",
        all8 && sum8 == CASES as u32,
        &format!(
            "ok={} eacces={} eexist={} einval={} eidrm={}",
            h8[0], h8[1], h8[2], h8[3], h8[4]
        ),
    );

    // R9 回滚成功例被拒。
    let mut m9 = RollbackLedger::new(0x1401);
    let mut ok9 = m9.register_ruled(0, 0, IPC_PRIVATE).is_ok();
    ok9 = ok9 && m9.observe(0, 0).is_ok();
    let e9 = m9.rollback(0).err();
    s.add(
        "R9-误滚拒止",
        ok9 && e9 == Some(SV_ELIVE) && m9.rejects() == 1,
        "成功例回滚落 SV_ELIVE",
    );

    // R10 重复回滚被拒。
    let mut m10 = RollbackLedger::new(0x1402);
    let mut ok10 = m10.register_ruled(0, 0, 3).is_ok();
    ok10 = ok10 && m10.observe(0, EEXIST).is_ok();
    let a10 = m10.rollback(0);
    let b10 = m10.rollback(0);
    s.add(
        "R10-重复回滚",
        ok10 && a10.is_ok() && b10.err() == Some(SV_ERB),
        "同例二次回滚落 SV_ERB",
    );

    // R11 未闭合时终重放被拒：只登记不回滚。
    let mut m11 = RollbackLedger::new(0x1403);
    let mut i11 = 0usize;
    let mut ok11 = true;
    while i11 < CASES {
        let (op, key) = spec_of(i11);
        ok11 = ok11 && m11.register_ruled(i11, op, key).is_ok();
        let want = rule_error(op, key);
        ok11 = ok11 && m11.observe(i11, want).is_ok();
        i11 = i11.wrapping_add(1);
    }
    let e11 = m11.replay().err();
    s.add(
        "R11-回滚缺位",
        ok11 && e11 == Some(SV_ERB),
        "只回填不回滚时终重放落 SV_ERB",
    );

    // R12 封盘幂等 + 四路注入。
    let mut m12 = RollbackLedger::new(0x1404);
    let ra = m12.freeze();
    let rb = m12.freeze();
    let inj = inject_four();
    s.add_dyn(
        "R12-幂等注入",
        ra.is_ok() && ra == rb && inj[0] && inj[1] && inj[2] && inj[3],
        &format!(
            "root={:#x} inj={}/{}/{}/{}",
            ra.unwrap_or(0),
            inj[0],
            inj[1],
            inj[2],
            inj[3]
        ),
    );

    // R13 干净账拒止为零。
    s.add_dyn(
        "R13-干净账",
        l2.rejects() == 0,
        &format!("rejects={}", l2.rejects()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11147_flow() {
        let (l, r) = build_ledger(0x1000);
        assert!(r.is_ok());
        assert!(l.frozen());
        assert_eq!(l.registered(), CASES);
        assert_eq!(l.observed(), CASES);
        assert_eq!(l.mismatches(), 0);
        assert!(l.rollback_closed());
        // 九例驻留、九例回滚。
        assert_eq!(l.live_count(), 9);
        assert_eq!(l.rollback_targets(), 9);
        assert_eq!(l.rollbacks(), 9);
        assert_eq!(l.rule_agreement(), CASES);
    }

    #[test]
    fn f11147_gates() {
        let mut l = RollbackLedger::new(0x3400);
        assert!(l.register_ruled(0, 0, IPC_PRIVATE).is_ok());
        // 重复登记
        assert_eq!(l.register_ruled(0, 0, 1).err(), Some(SV_EDUP));
        // 例号与族号越界
        assert_eq!(l.register_ruled(18, 0, 1).err(), Some(SV_ECASE));
        assert_eq!(l.register_ruled(1, OPS as u8, 1).err(), Some(SV_ECASE));
        // 序倒挂
        assert!(l.register_ruled(2, 0, 1).is_ok());
        assert_eq!(l.register_ruled(1, 0, 1).err(), Some(SV_EORDER));
        // 期望码越界
        assert_eq!(l.register(3, 1, 1, 999).err(), Some(SV_ENOEXP));
        // 未登记即观测
        assert_eq!(l.observe(5, 0).err(), Some(SV_EUNREG));
        // 重复观测
        assert!(l.observe(0, 0).is_ok());
        assert_eq!(l.observe(0, 0).err(), Some(SV_EDUP));
        // 未测先回滚
        assert_eq!(l.rollback(2).err(), Some(SV_ESHORT));
        // 回滚应驻留例
        assert_eq!(l.rollback(0).err(), Some(SV_ELIVE));
        assert_eq!(l.rejects(), 9);
        // 封盘后再动账
        assert!(l.freeze().is_ok());
        assert_eq!(l.observe(2, 0).err(), Some(SV_EFROZEN));
        assert_eq!(l.rollback(2).err(), Some(SV_EFROZEN));
        assert_eq!(l.rejects(), 11);
    }

    #[test]
    fn f11147_rules() {
        // 取用族：私有键成功，整除 3 已存在。
        assert_eq!(rule_error(0, IPC_PRIVATE), 0);
        assert_eq!(rule_error(0, 3), EEXIST);
        assert_eq!(rule_error(0, 4), 0);
        // 操作族：私有键非法，整除 5 已撤。
        assert_eq!(rule_error(3, IPC_PRIVATE), EINVAL);
        assert_eq!(rule_error(3, 5), EIDRM);
        assert_eq!(rule_error(3, 4), 0);
        // 越权位优先于一切。
        assert_eq!(rule_error(0, KEY_DENY | 3), EACCES);
        assert_eq!(rule_error(3, KEY_DENY), EACCES);
        assert_eq!(rule_error(3, KEY_DENY | IPC_PRIVATE), EACCES);
        // 族号越界。
        assert_eq!(rule_error(OPS as u8, 1), EINVAL);
        // 取用族判定与键归一。
        assert!(is_get_op(0) && is_get_op(2) && !is_get_op(3));
        assert_eq!(norm_key(KEY_DENY | 7), 7);
    }

    #[test]
    fn f11147_ledger_shape_and_inject() {
        let (l, r) = build_ledger(0x1005);
        assert!(r.is_ok());
        // 五桶：成功 9、EACCES 2、EEXIST 2、EINVAL 2、EIDRM 3。
        assert_eq!(l.errno_histogram(), [9, 2, 2, 2, 3]);
        assert_eq!(l.op_table(), [3, 3, 3, 3, 3, 3]);
        assert_eq!(l.rejects(), 0);
        assert_eq!(inject_four(), [true, true, true, true]);
        // 回滚账与矩阵账互斥：每个回滚位都落在失败例上。
        let mut i = 0usize;
        while i < CASES {
            if l.is_rolled(i) {
                let c = l.case(i).expect("在册");
                assert!(c.expected != 0);
            }
            i += 1;
        }
    }

    #[test]
    fn f11147_checks_green() {
        let cs = run_unx11147_checks();
        assert!(cs.all_passed(), "checks red in unx11147");
    }
}
