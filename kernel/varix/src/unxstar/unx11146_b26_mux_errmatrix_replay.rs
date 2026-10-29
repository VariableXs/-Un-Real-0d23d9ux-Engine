//! UNX-F11146 B26 清账（复用器错误矩阵判据终重放·四码账）（0x1f2e0 段）
//!
//! 纯功能模型：复用器的四族接口——select / poll / epoll / futex——共享一张
//! 错误面，但各有各的边界。每例由（族, 事件掩码, 辅助参数）三元组确定，
//! 期望 errno 由规则纯函数独算，登记时再独立落一份，重放要求两账逐例相符。
//!
//! 「四码账」是本卡的第二本账：EINVAL / EBADF / ENOMEM / EAGAIN 四个码
//! 必须在本批用例中各出现至少一次，四码计数之和必须等于用例总数，
//! 且不得出现四码之外的第五码——码面外溢即判拒。
//!
//! 规则面（对齐 select/poll/epoll/futex 语义的裁剪版）：
//!   - 事件掩码为 0 表示空兴趣集，落 EINVAL；
//!   - 掩码超出低八位（0xFF）视为非法位，落 EINVAL；
//!   - 辅助参数取哨兵 0xFFFF_FFFF 表示坏描述符，落 EBADF；
//!   - select 的 nfds 为 0 或超过 1024 落 EINVAL；
//!   - poll 的 nfds 超过 2048 落 ENOMEM；
//!   - epoll 的 maxevents 为 0 落 EINVAL、超过 4096 落 ENOMEM；
//!   - futex 的 op 超过 3 落 EINVAL；FUTEX_WAIT 且观测值与期望不符落 EAGAIN；
//!   - 其余组合成功。
//!
//! 拒止面：族号越界、掩码越界、重复登记、例序倒挂、期望码越界、未登记即
//! 观测、缺实测、不符、码账不齐、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2e0;
/// 族号越界
pub const MX_EFAM: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const MX_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const MX_EORDER: u32 = SEG_BASE + 0x03;
/// 掩码越界
pub const MX_EMASK: u32 = SEG_BASE + 0x04;
/// 期望码越界
pub const MX_ENOEXP: u32 = SEG_BASE + 0x05;
/// 未登记即观测
pub const MX_EUNREG: u32 = SEG_BASE + 0x06;
/// 未测即重放
pub const MX_ESHORT: u32 = SEG_BASE + 0x07;
/// 实测与期望不符
pub const MX_EMISMATCH: u32 = SEG_BASE + 0x08;
/// 码账不齐或码面外溢
pub const MX_ECODE: u32 = SEG_BASE + 0x09;
/// 已封盘
pub const MX_EFROZEN: u32 = SEG_BASE + 0x0a;

/// 复用器族数。
pub const FAMILIES: usize = 4;
/// 族名。
pub const FAMILY_NAMES: [&str; FAMILIES] = ["select", "poll", "epoll", "futex"];
/// 每族例数。
pub const PER_FAMILY: usize = 6;
/// 总例数。
pub const CASES: usize = FAMILIES * PER_FAMILY;
/// 合法掩码上界（低八位）。
pub const MASK_VALID: u32 = 0xFF;
/// select 的 nfds 上界。
pub const SELECT_MAX: u32 = 1024;
/// poll 的 nfds 上界。
pub const POLL_MAX: u32 = 2048;
/// epoll 的 maxevents 上界。
pub const EPOLL_MAX: u32 = 4096;
/// futex 的 op 上界（WAIT / WAKE / REQUEUE / CMP_REQUEUE）。
pub const FUTEX_OPS: u32 = 4;
/// FUTEX_WAIT。
pub const FUTEX_WAIT: u32 = 0;
/// FUTEX_WAIT 的期望观测值。
pub const FUTEX_EXPECT: u32 = 7;
/// 坏描述符哨兵。
pub const BAD_FD: u32 = 0xFFFF_FFFF;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;

/// EAGAIN
pub const EAGAIN: u32 = 11;
/// ENOMEM
pub const ENOMEM: u32 = 12;
/// EBADF
pub const EBADF: u32 = 9;
/// EINVAL
pub const EINVAL: u32 = 22;

/// 四码账的槽位顺序：EINVAL / EBADF / ENOMEM / EAGAIN。
pub const CODE_SLOTS: usize = 4;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

const F_SELECT: u8 = 0;
const F_POLL: u8 = 1;
const F_EPOLL: u8 = 2;
const F_FUTEX: u8 = 3;

/// 规则纯函数：由三元组给出应有 errno。
pub fn rule_error(family: u8, mask: u32, aux: u32) -> u32 {
    if family as usize >= FAMILIES {
        return EINVAL;
    }
    if mask == 0 || (mask & !MASK_VALID) != 0 {
        return EINVAL;
    }
    if aux == BAD_FD {
        return EBADF;
    }
    match family {
        F_SELECT => {
            if aux == 0 || aux > SELECT_MAX {
                EINVAL
            } else {
                0
            }
        }
        F_POLL => {
            if aux > POLL_MAX {
                ENOMEM
            } else {
                0
            }
        }
        F_EPOLL => {
            if aux == 0 {
                EINVAL
            } else if aux > EPOLL_MAX {
                ENOMEM
            } else {
                0
            }
        }
        F_FUTEX => {
            if aux >= FUTEX_OPS {
                EINVAL
            } else if aux == FUTEX_WAIT && mask != FUTEX_EXPECT {
                EAGAIN
            } else {
                0
            }
        }
        _ => EINVAL,
    }
}

/// errno 落在四码账的哪一槽；不在四码内返回 None。
pub fn code_slot(errno: u32) -> Option<usize> {
    if errno == EINVAL {
        Some(0)
    } else if errno == EBADF {
        Some(1)
    } else if errno == ENOMEM {
        Some(2)
    } else if errno == EAGAIN {
        Some(3)
    } else {
        None
    }
}

/// 矩阵一例。
#[derive(Clone, Copy, Debug)]
pub struct MuxCase {
    pub family: u8,
    pub mask: u32,
    pub aux: u32,
    pub expected: u32,
    pub got: u32,
    pub seen: bool,
    pub live: bool,
}

impl MuxCase {
    const EMPTY: MuxCase = MuxCase {
        family: 0,
        mask: 0,
        aux: 0,
        expected: 0,
        got: 0,
        seen: false,
        live: false,
    };

    /// 已测且相符。
    pub fn agrees(&self) -> bool {
        self.live && self.seen && self.got == self.expected
    }

    /// 登记期望与规则复算一致。
    pub fn rule_agrees(&self) -> bool {
        self.live && rule_error(self.family, self.mask, self.aux) == self.expected
    }

    /// 本例期望是否落在四码之内（成功码 0 不计外溢）。
    pub fn in_four_codes(&self) -> bool {
        self.live && (self.expected == 0 || code_slot(self.expected).is_some())
    }
}

/// 复用器错误矩阵 + 四码账。
#[derive(Clone, Debug)]
pub struct MuxMatrix {
    pub base: u32,
    cases: [MuxCase; CASES],
    registered: usize,
    observed: usize,
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl MuxMatrix {
    /// 新建空矩阵。
    pub fn new(base: u32) -> MuxMatrix {
        MuxMatrix {
            base,
            cases: [MuxCase::EMPTY; CASES],
            registered: 0,
            observed: 0,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一例：族号、掩码、期望码三重把关。
    pub fn register(
        &mut self,
        id: usize,
        family: u8,
        mask: u32,
        aux: u32,
        expected: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MX_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(MX_EFAM));
        }
        if family as usize >= FAMILIES {
            return Err(self.fail(MX_EFAM));
        }
        if (mask & !MASK_VALID) != 0 {
            return Err(self.fail(MX_EMASK));
        }
        if let Some(c) = self.cases.get(id) {
            if c.live {
                return Err(self.fail(MX_EDUP));
            }
        }
        if id < self.cursor {
            return Err(self.fail(MX_EORDER));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(MX_ENOEXP));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            *slot = MuxCase {
                family,
                mask,
                aux,
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
    pub fn register_ruled(
        &mut self,
        id: usize,
        family: u8,
        mask: u32,
        aux: u32,
    ) -> Result<(), u32> {
        let e = rule_error(family, mask, aux);
        self.register(id, family, mask, aux, e)
    }

    /// 观测实测。
    pub fn observe(&mut self, id: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(MX_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(MX_EFAM));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(MX_ENOEXP));
        }
        let c = match self.cases.get(id) {
            Some(c) => *c,
            None => return Err(self.fail(MX_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(MX_EUNREG));
        }
        if c.seen {
            return Err(self.fail(MX_EDUP));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            slot.got = errno;
            slot.seen = true;
        }
        self.observed = self.observed.wrapping_add(1);
        Ok(())
    }

    /// 只读单例。
    pub fn case(&self, id: usize) -> Option<MuxCase> {
        self.cases.get(id).copied()
    }

    /// 登记例数。
    pub fn registered(&self) -> usize {
        self.registered
    }

    /// 已测例数。
    pub fn observed(&self) -> usize {
        self.observed
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }
}

impl MuxMatrix {
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

    /// 每族登记例数。
    pub fn family_table(&self) -> [u32; FAMILIES] {
        let mut out = [0u32; FAMILIES];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    if let Some(slot) = out.get_mut(c.family as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 四码账：EINVAL / EBADF / ENOMEM / EAGAIN 各多少例。
    pub fn code_ledger(&self) -> [u32; CODE_SLOTS] {
        let mut out = [0u32; CODE_SLOTS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    if let Some(slot) = code_slot(c.expected) {
                        if let Some(v) = out.get_mut(slot) {
                            *v = v.wrapping_add(1);
                        }
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 码面外溢例数：期望码不在四码之内。
    pub fn code_overflow(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live && !c.in_four_codes() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 成功例数（期望码为 0 的在册例）。
    pub fn successes(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live && c.expected == 0 {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 四码账是否齐：四槽皆非零、无外溢、四码计数加成功例数等于用例数。
    pub fn code_ledger_ok(&self) -> bool {
        let l = self.code_ledger();
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < CODE_SLOTS {
            if l[i] == 0 {
                return false;
            }
            sum = sum.wrapping_add(l[i]);
            i = i.wrapping_add(1);
        }
        sum.wrapping_add(self.successes()) == self.registered as u32 && self.code_overflow() == 0
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

    /// 折叠指纹：基址、计数、拒止，再逐例并入三元组、期望与实测。
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
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    h ^= c.family as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.mask as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.aux as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.expected as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    if c.seen {
                        h ^= c.got as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 终重放：用例齐、实测齐、逐例相符、四码账齐。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.registered != CASES || self.observed != CASES {
            return Err(self.fail(MX_ESHORT));
        }
        if self.gaps() != 0 {
            return Err(self.fail(MX_ESHORT));
        }
        if self.mismatches() != 0 {
            return Err(self.fail(MX_EMISMATCH));
        }
        if !self.code_ledger_ok() {
            return Err(self.fail(MX_ECODE));
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

/// 每族六例的固定探针表：（掩码, 辅助参数）。
pub const PROBES: [(u32, u32); PER_FAMILY] = [
    (0, 0),           // 空兴趣集 -> EINVAL
    (1, BAD_FD),      // 坏描述符 -> EBADF
    (1, 5000),        // 超限 -> EINVAL / ENOMEM（按族而定）
    (MASK_VALID, 1),  // 全位掩码 -> 各族成功
    (FUTEX_EXPECT, 0), // futex 值相符 -> 成功；其余族按参数判定
    (2, 0),           // futex 值不符 -> EAGAIN；其余族按参数判定
];

/// 第 id 号用例的三元组：族 = id % 4，例位 = id / 4。
pub fn spec_of(id: usize) -> (u8, u32, u32) {
    let family = (id % FAMILIES) as u8;
    let idx = id / FAMILIES;
    let (mask, aux) = match PROBES.get(idx) {
        Some(p) => *p,
        None => (1, 1),
    };
    (family, mask, aux)
}

/// 搭建全矩阵：逐例按规则登记并回填期望值，最后终重放。
pub fn build_matrix(base: u32) -> (MuxMatrix, Result<u64, u32>) {
    let mut m = MuxMatrix::new(base);
    let mut i = 0usize;
    while i < CASES {
        let (family, mask, aux) = spec_of(i);
        if m.register_ruled(i, family, mask, aux).is_err() {
            return (m, Err(MX_EFAM));
        }
        i = i.wrapping_add(1);
    }
    let mut k = 0usize;
    while k < CASES {
        let want = match m.case(k) {
            Some(c) => c.expected,
            None => 0,
        };
        if m.observe(k, want).is_err() {
            return (m, Err(MX_EFAM));
        }
        k = k.wrapping_add(1);
    }
    let r = m.replay();
    (m, r)
}

/// 四路注入：封盘后动账、重复登记、未登记观测、四码账外溢。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 封盘后登记必被拒。
    let (mut a, ok1) = build_matrix(0x2300);
    let (f0, m0, x0) = spec_of(0);
    out[0] = ok1.is_ok() && a.register(0, f0, m0, x0, 0).err() == Some(MX_EFROZEN);

    // ② 重复登记必被拒。
    let mut b = MuxMatrix::new(0x2301);
    let first = b.register_ruled(0, F_POLL, 1, 8);
    let twice = b.register_ruled(0, F_POLL, 1, 8);
    out[1] = first.is_ok() && twice.err() == Some(MX_EDUP);

    // ③ 未登记即观测必被拒。
    let mut c = MuxMatrix::new(0x2302);
    out[2] = c.observe(5, 0).err() == Some(MX_EUNREG);

    // ④ 四码账外溢：二十四例全登记为成功码（0），码账四槽全空，
    //    终重放必须落在 MX_ECODE 而不是放过。
    let mut d = MuxMatrix::new(0x2303);
    let mut i4 = 0usize;
    while i4 < CASES {
        if d.register(i4, F_POLL, 1, 1, 0).is_err() {
            break;
        }
        if d.observe(i4, 0).is_err() {
            break;
        }
        i4 = i4.wrapping_add(1);
    }
    let e4 = d.replay().err();
    out[3] = i4 == CASES && !d.code_ledger_ok() && e4 == Some(MX_ECODE);

    out
}

/// 判据集：X1~X13。
pub fn run_unx11146_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11146.b26_mux_errmatrix");

    // X1 常量面：段基址、四族、每族六例、四码槽。
    let consts = SEG_BASE == 0x1f2e0
        && FAMILIES == 4
        && PER_FAMILY == 6
        && CASES == 24
        && CODE_SLOTS == 4
        && FAMILY_NAMES.len() == FAMILIES
        && PROBES.len() == PER_FAMILY;
    s.add("X1-常量面", consts, "四族 x 六例 = 24，四码账");

    // X2 全矩阵搭建成功并封盘。
    let (m2, r2) = build_matrix(0x1300);
    s.add(
        "X2-搭建封盘",
        r2.is_ok() && m2.frozen(),
        "build_matrix 终重放通过并封盘",
    );

    // X3 用例齐、实测齐。
    s.add_dyn(
        "X3-两账齐",
        m2.registered() == CASES && m2.observed() == CASES,
        &format!("reg={} obs={}", m2.registered(), m2.observed()),
    );

    // X4 逐例闭合：不符与缺口均为零。
    s.add_dyn(
        "X4-逐例闭合",
        m2.mismatches() == 0 && m2.gaps() == 0,
        &format!("mismatch={} gaps={}", m2.mismatches(), m2.gaps()),
    );

    // X5 规则复算一致。
    s.add_dyn(
        "X5-规则一致",
        m2.rule_agreement() == CASES,
        &format!("agree={}/{}", m2.rule_agreement(), CASES),
    );

    // X6 四族覆盖：每族六例。
    let t6 = m2.family_table();
    let mut ok6 = true;
    let mut i6 = 0usize;
    while i6 < FAMILIES {
        ok6 = ok6 && t6[i6] == PER_FAMILY as u32;
        i6 = i6.wrapping_add(1);
    }
    s.add_dyn(
        "X6-四族覆盖",
        ok6,
        &format!("{:?}", t6),
    );

    // X7 四码账齐：四槽皆非零、无外溢、计数和等于用例数。
    let l7 = m2.code_ledger();
    s.add_dyn(
        "X7-四码齐",
        m2.code_ledger_ok() && l7[0] > 0 && l7[1] > 0 && l7[2] > 0 && l7[3] > 0,
        &format!("einval={} ebadf={} enomem={} eagain={}", l7[0], l7[1], l7[2], l7[3]),
    );

    // X8 掩码闸门：越位掩码登记即拒；空掩码可登记但规则判 EINVAL。
    let mut m8 = MuxMatrix::new(0x1301);
    let e8a = m8.register(0, F_POLL, 0x100, 1, 0).err();
    let ok8 = m8.register_ruled(0, F_POLL, 0, 1).is_ok();
    let want8 = m8.case(0).map(|c| c.expected);
    s.add(
        "X8-掩码闸门",
        e8a == Some(MX_EMASK) && ok8 && want8 == Some(EINVAL) && m8.rejects() == 1,
        "越位掩码落 MX_EMASK；空掩码登记后规则判 EINVAL",
    );

    // X9 族号越界被拒。
    let mut m9 = MuxMatrix::new(0x1302);
    let e9 = m9.register(0, FAMILIES as u8, 1, 1, 0).err();
    s.add(
        "X9-族号拒止",
        e9 == Some(MX_EFAM) && m9.rejects() == 1,
        "族号 4 落 MX_EFAM",
    );

    // X10 四码外溢时终重放必被拒。
    let mut m10 = MuxMatrix::new(0x1303);
    let mut i10 = 0usize;
    let mut ok10 = true;
    while i10 < CASES {
        ok10 = ok10 && m10.register(i10, F_POLL, 1, 1, 0).is_ok();
        ok10 = ok10 && m10.observe(i10, 0).is_ok();
        i10 = i10.wrapping_add(1);
    }
    let e10 = m10.replay().err();
    s.add(
        "X10-码面外溢",
        ok10 && e10 == Some(MX_ECODE),
        "成功码铺满二十四例时重放落 MX_ECODE",
    );

    // X11 封盘幂等。
    let mut m11 = MuxMatrix::new(0x1304);
    let ra = m11.freeze();
    let rb = m11.freeze();
    s.add_dyn(
        "X11-幂等",
        ra.is_ok() && ra == rb,
        &format!("root={:#x}", ra.unwrap_or(0)),
    );

    // X12 四路注入全过。
    let inj = inject_four();
    s.add_dyn(
        "X12-四注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        &format!("{}/{}/{}/{}", inj[0], inj[1], inj[2], inj[3]),
    );

    // X13 干净矩阵拒止为零。
    s.add_dyn(
        "X13-干净账",
        m2.rejects() == 0,
        &format!("rejects={}", m2.rejects()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11146_flow() {
        let (m, r) = build_matrix(0x1000);
        assert!(r.is_ok());
        assert!(m.frozen());
        assert_eq!(m.registered(), CASES);
        assert_eq!(m.observed(), CASES);
        assert_eq!(m.mismatches(), 0);
        assert_eq!(m.gaps(), 0);
        assert!(m.code_ledger_ok());
        let l = m.code_ledger();
        assert!(l[0] > 0 && l[1] > 0 && l[2] > 0 && l[3] > 0);
        assert_eq!(m.rule_agreement(), CASES);
    }

    #[test]
    fn f11146_gates() {
        let mut m = MuxMatrix::new(0x3200);
        assert!(m.register_ruled(0, F_POLL, 1, 1).is_ok());
        // 重复登记
        assert_eq!(m.register_ruled(0, F_POLL, 1, 1).err(), Some(MX_EDUP));
        // 例号越界
        assert_eq!(m.register_ruled(24, F_POLL, 1, 1).err(), Some(MX_EFAM));
        // 族号越界
        assert_eq!(m.register_ruled(1, 9, 1, 1).err(), Some(MX_EFAM));
        // 掩码越界
        assert_eq!(m.register_ruled(1, F_POLL, 0x100, 1).err(), Some(MX_EMASK));
        // 序倒挂
        assert!(m.register_ruled(3, F_POLL, 1, 1).is_ok());
        assert_eq!(m.register_ruled(2, F_POLL, 1, 1).err(), Some(MX_EORDER));
        // 期望码越界
        assert_eq!(m.register(4, F_POLL, 1, 1, 999).err(), Some(MX_ENOEXP));
        // 未登记即观测
        assert_eq!(m.observe(5, 0).err(), Some(MX_EUNREG));
        // 重复观测
        assert!(m.observe(0, 0).is_ok());
        assert_eq!(m.observe(0, 0).err(), Some(MX_EDUP));
        assert_eq!(m.rejects(), 8);
        // 封盘后再动账
        assert!(m.freeze().is_ok());
        assert_eq!(m.register_ruled(6, F_POLL, 1, 1).err(), Some(MX_EFROZEN));
        assert_eq!(m.observe(3, 0).err(), Some(MX_EFROZEN));
        assert_eq!(m.rejects(), 10);
    }

    #[test]
    fn f11146_rules() {
        // 空掩码与越位掩码。
        assert_eq!(rule_error(F_SELECT, 0, 1), EINVAL);
        assert_eq!(rule_error(F_SELECT, 0x100, 1), EINVAL);
        // 坏描述符优先于族内规则。
        assert_eq!(rule_error(F_SELECT, 1, BAD_FD), EBADF);
        assert_eq!(rule_error(F_EPOLL, 1, BAD_FD), EBADF);
        // select 的 nfds 边界。
        assert_eq!(rule_error(F_SELECT, 1, SELECT_MAX), 0);
        assert_eq!(rule_error(F_SELECT, 1, SELECT_MAX + 1), EINVAL);
        assert_eq!(rule_error(F_SELECT, 1, 0), EINVAL);
        // poll 的 nfds 超限落 ENOMEM。
        assert_eq!(rule_error(F_POLL, 1, POLL_MAX), 0);
        assert_eq!(rule_error(F_POLL, 1, POLL_MAX + 1), ENOMEM);
        // epoll 的 maxevents。
        assert_eq!(rule_error(F_EPOLL, 1, 0), EINVAL);
        assert_eq!(rule_error(F_EPOLL, 1, EPOLL_MAX), 0);
        assert_eq!(rule_error(F_EPOLL, 1, EPOLL_MAX + 1), ENOMEM);
        // futex 的 op 与观测值。
        assert_eq!(rule_error(F_FUTEX, 1, FUTEX_OPS), EINVAL);
        assert_eq!(rule_error(F_FUTEX, FUTEX_EXPECT, FUTEX_WAIT), 0);
        assert_eq!(rule_error(F_FUTEX, 1, FUTEX_WAIT), EAGAIN);
        assert_eq!(rule_error(F_FUTEX, 1, 1), 0);
        // 族号越界。
        assert_eq!(rule_error(FAMILIES as u8, 1, 1), EINVAL);
        // 码槽映射。
        assert_eq!(code_slot(EINVAL), Some(0));
        assert_eq!(code_slot(EBADF), Some(1));
        assert_eq!(code_slot(ENOMEM), Some(2));
        assert_eq!(code_slot(EAGAIN), Some(3));
        assert_eq!(code_slot(0), None);
    }

    #[test]
    fn f11146_ledger_and_inject() {
        let (m, r) = build_matrix(0x1005);
        assert!(r.is_ok());
        assert_eq!(m.rejects(), 0);
        let l = m.code_ledger();
        // EINVAL 十、EBADF 四、ENOMEM 二、EAGAIN 一，成功七例不计入码账。
        assert_eq!(l, [10, 4, 2, 1]);
        assert_eq!(m.code_overflow(), 0);
        assert_eq!(inject_four(), [true, true, true, true]);
        // 四族表各六例。
        assert_eq!(m.family_table(), [6, 6, 6, 6]);
    }

    #[test]
    fn f11146_checks_green() {
        let cs = run_unx11146_checks();
        assert!(cs.all_passed(), "checks red in unx11146");
    }
}
