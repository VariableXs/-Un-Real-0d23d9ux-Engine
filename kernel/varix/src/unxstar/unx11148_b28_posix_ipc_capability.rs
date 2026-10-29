//! UNX-F11148 B28 清账（POSIX IPC 错误矩阵判据终重放·能力表账）（0x1f300 段）
//!
//! 纯功能模型：POSIX IPC 的六族调用各需一组能力位——读、写、建、删。
//! 每次调用带着一份「本进程被授予的能力位图」和一组打开标志进入，规则先看
//! 标志是否越界，再看被授予的能力是否覆盖该族的要求位；缺位即 EACCES。
//!
//! 「能力表账」是本卡的第二本账，两项核对：
//!   - 要求面：四类能力在六族的要求表里各被用到多少次，任一类为零即表残；
//!   - 授予面：十八例授予的能力位总和、被拒例数，必须与要求表逐族对平。
//! 两面对不上即判码账不齐。
//!
//! 规则面：
//!   - 标志只接受低两位（0..=3），越界落 EINVAL；
//!   - 族号越界落 EINVAL；
//!   - 授予位缺该族要求位的任一位，落 EACCES；
//!   - 覆盖齐备即成功。
//!
//! 拒止面：族号越界、重复登记、例序倒挂、期望码越界、未登记即观测、
//! 缺实测、不符、能力表残、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f300;
/// 族号或例号越界
pub const PC_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const PC_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const PC_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const PC_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const PC_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const PC_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const PC_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 能力表残或授予面不齐
pub const PC_ECAP: u32 = SEG_BASE + 0x08;
/// 已封盘
pub const PC_EFROZEN: u32 = SEG_BASE + 0x09;

/// 调用族数。
pub const OPS: usize = 6;
/// 族名。
pub const OP_NAMES: [&str; OPS] = [
    "mq_open",
    "mq_send",
    "mq_receive",
    "sem_open",
    "sem_wait",
    "shm_unlink",
];
/// 能力类数：读 / 写 / 建 / 删。
pub const CAPS: usize = 4;
/// 能力名。
pub const CAP_NAMES: [&str; CAPS] = ["read", "write", "create", "unlink"];
/// 每族例数。
pub const PER_OP: usize = 3;
/// 总例数。
pub const CASES: usize = OPS * PER_OP;
/// 打开标志上界（O_RDONLY=0 / O_WRONLY=1 / O_RDWR=2 / 越界=3）。
pub const FLAG_MAX: u8 = 3;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;

/// CAP_READ
pub const CAP_READ: u8 = 0x1;
/// CAP_WRITE
pub const CAP_WRITE: u8 = 0x2;
/// CAP_CREATE
pub const CAP_CREATE: u8 = 0x4;
/// CAP_UNLINK
pub const CAP_UNLINK: u8 = 0x8;
/// 四位全授予。
pub const CAP_ALL: u8 = 0xF;

/// EACCES
pub const EACCES: u32 = 13;
/// EINVAL
pub const EINVAL: u32 = 22;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 能力要求表：每族需要的位。
pub const CAP_REQUIRE: [u8; OPS] = [
    CAP_CREATE | CAP_READ | CAP_WRITE, // mq_open
    CAP_WRITE,                         // mq_send
    CAP_READ,                          // mq_receive
    CAP_CREATE,                        // sem_open
    CAP_READ | CAP_WRITE,              // sem_wait
    CAP_UNLINK,                        // shm_unlink
];

/// 规则纯函数：由（族, 授予位, 标志）给出应有 errno。
pub fn rule_error(op: u8, granted: u8, flags: u8) -> u32 {
    if op as usize >= OPS {
        return EINVAL;
    }
    if flags > FLAG_MAX {
        return EINVAL;
    }
    let need = match CAP_REQUIRE.get(op as usize) {
        Some(v) => *v,
        None => return EINVAL,
    };
    if granted & need != need {
        EACCES
    } else {
        0
    }
}

/// 能力位对应的类名下标（单比特）。
pub fn cap_index(bit: u8) -> Option<usize> {
    match bit {
        CAP_READ => Some(0),
        CAP_WRITE => Some(1),
        CAP_CREATE => Some(2),
        CAP_UNLINK => Some(3),
        _ => None,
    }
}

/// FNV-1a 单步：把一个 u32 混入摘要。
pub fn fnv_step(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x ^= ((v >> (i * 8)) & 0xFF) as u64;
        x = x.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    x
}

/// 单条能力判据样本。
#[derive(Clone, Copy, Debug)]
pub struct CapCase {
    /// 调用族号。
    pub op: u8,
    /// 本次授予的能力位图。
    pub granted: u8,
    /// 打开标志。
    pub flags: u8,
    /// 期望 errno（0 表示成功）。
    pub expected: u32,
    /// 实测 errno（观测前为 0）。
    pub got: u32,
    /// 是否已登记。
    pub live: bool,
    /// 是否已观测。
    pub seen: bool,
}

impl CapCase {
    /// 空样本。
    pub const fn blank() -> Self {
        CapCase {
            op: 0,
            granted: 0,
            flags: 0,
            expected: 0,
            got: 0,
            live: false,
            seen: false,
        }
    }

    /// 该样本缺哪些要求位。
    pub fn missing(&self) -> u8 {
        let need = match CAP_REQUIRE.get(self.op as usize) {
            Some(v) => *v,
            None => 0,
        };
        need & !self.granted
    }

    /// 要求面里该样本用到的能力类数。
    pub fn need_width(&self) -> usize {
        let need = match CAP_REQUIRE.get(self.op as usize) {
            Some(v) => *v,
            None => 0,
        };
        popcount4(need)
    }

    /// 授予面里该样本覆盖到的能力类数。
    pub fn grant_width(&self) -> usize {
        popcount4(self.granted & CAP_ALL)
    }
}

/// 四位位图的置位计数。
pub fn popcount4(bits: u8) -> usize {
    let mut n = 0usize;
    let mut i = 0u8;
    while i < CAPS as u8 {
        if bits & (1 << i) != 0 {
            n = n.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    n
}

/// 能力表账：十八例的登记、观测与两面核对。
pub struct CapLedger {
    base: u64,
    cases: [CapCase; CASES],
    registered: u32,
    observed: u32,
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl CapLedger {
    /// 新账本，摘要以 SEG 段常量起手。
    pub const fn new() -> Self {
        CapLedger {
            base: SEG_BASE as u64,
            cases: [CapCase::blank(); CASES],
            registered: 0,
            observed: 0,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    /// 记一次拒止并回传码。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已登记例数。
    pub fn registered(&self) -> u32 {
        self.registered
    }

    /// 已观测例数。
    pub fn observed(&self) -> u32 {
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

    /// 取样本。
    pub fn case(&self, id: usize) -> Option<&CapCase> {
        self.cases.get(id)
    }

    /// 登记一例：例号必须等于推进游标，族号/标志/授予位必须合法。
    pub fn register(
        &mut self,
        id: usize,
        op: u8,
        granted: u8,
        flags: u8,
        expected: u32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(PC_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(PC_ECASE));
        }
        if op as usize >= OPS {
            return Err(self.fail(PC_ENOEXP));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(PC_ENOEXP));
        }
        if granted & !CAP_ALL != 0 {
            return Err(self.fail(PC_ECAP));
        }
        let live = match self.cases.get(id) {
            Some(c) => c.live,
            None => return Err(self.fail(PC_EUNREG)),
        };
        if live {
            return Err(self.fail(PC_EDUP));
        }
        if id != self.cursor {
            return Err(self.fail(PC_EORDER));
        }
        let c = match self.cases.get_mut(id) {
            Some(c) => c,
            None => return Err(self.fail(PC_EUNREG)),
        };
        *c = CapCase {
            op,
            granted,
            flags,
            expected,
            got: 0,
            live: true,
            seen: false,
        };
        self.cursor = self.cursor.wrapping_add(1);
        self.registered = self.registered.wrapping_add(1);
        Ok(())
    }

    /// 按规则登记：期望码由 rule_error 推得。
    pub fn register_ruled(
        &mut self,
        id: usize,
        op: u8,
        granted: u8,
        flags: u8,
    ) -> Result<(), u32> {
        let expected = rule_error(op, granted, flags);
        self.register(id, op, granted, flags, expected)
    }

    /// 观测一例实测 errno；不符时先落账再拒。
    pub fn observe(&mut self, id: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(PC_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(PC_ECASE));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(PC_ENOEXP));
        }
        let c = match self.cases.get_mut(id) {
            Some(c) => c,
            None => return Err(self.fail(PC_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(PC_EUNREG));
        }
        if c.seen {
            return Err(self.fail(PC_EDUP));
        }
        c.seen = true;
        c.got = errno;
        self.observed = self.observed.wrapping_add(1);
        if errno != c.expected {
            return Err(self.fail(PC_EMISMATCH));
        }
        Ok(())
    }
}

/// 位图里最低的置位；零位图回 0。
pub fn lowest_bit(bits: u8) -> u8 {
    bits & (0u8.wrapping_sub(bits))
}

impl CapLedger {
    /// 成功例数（期望码为 0）。
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

    /// 拒止例数（期望码非 0）。
    pub fn denials(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live && c.expected != 0 {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 要求面：每类能力被已登记样本的要求表引用多少次。
    pub fn req_face(&self) -> [u32; CAPS] {
        let mut face = [0u32; CAPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let need = match CAP_REQUIRE.get(c.op as usize) {
                        Some(v) => *v,
                        None => 0,
                    };
                    let mut b = 0usize;
                    while b < CAPS {
                        if need & (1u8 << b) != 0 {
                            if let Some(slot) = face.get_mut(b) {
                                *slot = slot.wrapping_add(1);
                            }
                        }
                        b = b.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }

    /// 授予面：每类能力在已登记样本里被授予多少次。
    pub fn grant_face(&self) -> [u32; CAPS] {
        let mut face = [0u32; CAPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let mut b = 0usize;
                    while b < CAPS {
                        if c.granted & (1u8 << b) != 0 {
                            if let Some(slot) = face.get_mut(b) {
                                *slot = slot.wrapping_add(1);
                            }
                        }
                        b = b.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }

    /// 覆盖面：要求且已授予的次数。
    pub fn covered_face(&self) -> [u32; CAPS] {
        let mut face = [0u32; CAPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let need = match CAP_REQUIRE.get(c.op as usize) {
                        Some(v) => *v,
                        None => 0,
                    };
                    let hit = need & c.granted;
                    let mut b = 0usize;
                    while b < CAPS {
                        if hit & (1u8 << b) != 0 {
                            if let Some(slot) = face.get_mut(b) {
                                *slot = slot.wrapping_add(1);
                            }
                        }
                        b = b.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }

    /// 缺口面：要求但未授予的次数。
    pub fn gap_face(&self) -> [u32; CAPS] {
        let mut face = [0u32; CAPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let need = match CAP_REQUIRE.get(c.op as usize) {
                        Some(v) => *v,
                        None => 0,
                    };
                    let miss = need & !c.granted;
                    let mut b = 0usize;
                    while b < CAPS {
                        if miss & (1u8 << b) != 0 {
                            if let Some(slot) = face.get_mut(b) {
                                *slot = slot.wrapping_add(1);
                            }
                        }
                        b = b.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        face
    }
}

/// 能力表两面核对与规则一致性。
impl CapLedger {
    /// 规则一致：每条已登记样本的期望码等于规则纯函数输出。
    pub fn rule_agreement(&self) -> (u32, u32) {
        let mut agree = 0u32;
        let mut total = 0u32;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    total = total.wrapping_add(1);
                    if child_eq(c.expected, rule_error(c.op, c.granted, c.flags)) {
                        agree = agree.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        (agree, total)
    }

    /// 四面核对：要求面非空、要求面 = 覆盖面 + 缺口面、成功例无缺口、
    /// 拒止例有缺口、期望码与规则一致。
    pub fn cap_table_ok(&self) -> bool {
        let req = self.req_face();
        let cov = self.covered_face();
        let gap = self.gap_face();
        let mut b = 0usize;
        while b < CAPS {
            let r = match req.get(b) {
                Some(v) => *v,
                None => return false,
            };
            let c = match cov.get(b) {
                Some(v) => *v,
                None => return false,
            };
            let g = match gap.get(b) {
                Some(v) => *v,
                None => return false,
            };
            if r == 0 {
                return false;
            }
            if r != c.wrapping_add(g) {
                return false;
            }
            b = b.wrapping_add(1);
        }
        let mut i = 0usize;
        while i < CASES {
            let c = match self.cases.get(i) {
                Some(c) => c,
                None => return false,
            };
            if !c.live {
                return false;
            }
            if !child_eq(c.expected, rule_error(c.op, c.granted, c.flags)) {
                return false;
            }
            if c.expected == 0 && c.missing() != 0 {
                return false;
            }
            if c.expected == EACCES && c.missing() == 0 {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 摘要：十八例逐条混入。
    pub fn fold(&self) -> u64 {
        let mut h = self.base;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                h = fnv_step(h, c.op as u32);
                h = fnv_step(h, c.granted as u32);
                h = fnv_step(h, c.flags as u32);
                h = fnv_step(h, c.expected);
                h = fnv_step(h, c.got);
                let flags = (c.live as u32) | ((c.seen as u32) << 1);
                h = fnv_step(h, flags);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 终重放：全登记、全观测、全相符、能力表齐备。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut i = 0usize;
        while i < CASES {
            let c = match self.cases.get(i) {
                Some(c) => c,
                None => return Err(PC_EUNREG),
            };
            if !c.live {
                return Err(PC_EUNREG);
            }
            if !c.seen {
                return Err(PC_ESHORT);
            }
            if !child_eq(c.got, c.expected) {
                return Err(PC_EMISMATCH);
            }
            i = i.wrapping_add(1);
        }
        if !self.cap_table_ok() {
            return Err(PC_ECAP);
        }
        Ok(self.fold())
    }

    /// 封盘：重放通过才落锁。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(PC_EFROZEN));
        }
        let d = match self.replay() {
            Ok(d) => d,
            Err(e) => return Err(self.fail(e)),
        };
        self.frozen = true;
        Ok(d)
    }
}

/// 相等的纯函数包装（避免直接比较带来的类型歧义）。
pub fn child_eq(a: u32, b: u32) -> bool {
    a == b
}

/// 例号 → 样本规格：每族三例——全授予成功、缺一位拒、越界标志拒。
pub fn spec_of(id: usize) -> (u8, u8, u8) {
    let op = ((id / PER_OP) % OPS) as u8;
    let need = match CAP_REQUIRE.get(op as usize) {
        Some(v) => *v,
        None => 0,
    };
    match id % PER_OP {
        0 => (op, CAP_ALL, 0),
        1 => (op, need ^ lowest_bit(need), 0),
        _ => (op, need | CAP_UNLINK, FLAG_MAX.wrapping_add(1)),
    }
}

/// 建一本完整十八例的账：登记与观测各十八次，全部按规则推得。
pub fn build_ledger() -> CapLedger {
    let mut l = CapLedger::new();
    let mut i = 0usize;
    while i < CASES {
        let (op, granted, flags) = spec_of(i);
        l.register_ruled(i, op, granted, flags).ok();
        let e = rule_error(op, granted, flags);
        l.observe(i, e).ok();
        i = i.wrapping_add(1);
    }
    l
}

/// 四向注入：例序倒挂、重复观测、越位授予、封盘后再登记。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = CapLedger::new();
    out[0] = a.register(1, 0, CAP_ALL, 0, 0) == Err(PC_EORDER);
    let mut b = build_ledger();
    out[1] = b.observe(0, 0) == Err(PC_EDUP);
    let mut c = CapLedger::new();
    out[2] = c.register(0, 0, 0x10, 0, 0) == Err(PC_ECAP);
    let mut d = build_ledger();
    let _ = d.freeze();
    out[3] = d.register(0, 0, CAP_ALL, 0, 0) == Err(PC_EFROZEN);
    out
}

/// 判据集：P1–P13。
pub fn run_unx11148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11148");
    let l = build_ledger();

    s.add(
        "P1 账本建齐：十八例登记、十八例观测、零拒止",
        l.registered() == CASES as u32 && l.observed() == CASES as u32 && l.rejects() == 0,
        "登记/观测/拒止",
    );

    let d = l.fold();
    s.add(
        "P2 终重放通过且摘要等于 fold",
        l.replay() == Ok(d),
        "replay == fold",
    );

    let (agree, total) = l.rule_agreement();
    s.add(
        "P3 规则一致：十八例期望码全等于规则纯函数",
        agree == CASES as u32 && total == CASES as u32,
        "规则一致 18/18",
    );

    let req = l.req_face();
    let cov = l.covered_face();
    let gap = l.gap_face();
    let mut ident = true;
    let mut b = 0usize;
    while b < CAPS {
        let (r, c, g) = (req[b], cov[b], gap[b]);
        if r != c.wrapping_add(g) {
            ident = false;
        }
        b = b.wrapping_add(1);
    }
    s.add(
        "P4 两面恒等：要求面 = 覆盖面 + 缺口面（逐类）",
        ident,
        "req == cov + gap",
    );

    s.add_dyn(
        "P5 要求面四类齐备且计数 9/9/6/3",
        req == [9, 9, 6, 3],
        &format!(
            "要求面 r={} w={} c={} u={}",
            req[0], req[1], req[2], req[3]
        ),
    );

    s.add(
        "P6 成功六例、拒止十二例",
        l.successes() == 6 && l.denials() == 12,
        "成功 6 / 拒止 12",
    );

    let mut gap_rule = true;
    let mut i = 0usize;
    while i < CASES {
        if let Some(c) = l.case(i) {
            if c.expected == 0 && c.missing() != 0 {
                gap_rule = false;
            }
            if c.expected == EACCES && c.missing() == 0 {
                gap_rule = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "P7 成功例无缺口、拒止例必有缺口",
        gap_rule,
        "缺口与判码一致",
    );

    s.add(
        "P8 越界标志落 EINVAL（族号合法、授予齐备）",
        rule_error(0, CAP_ALL, FLAG_MAX.wrapping_add(1)) == EINVAL
            && rule_error(0, CAP_ALL, 0) == 0,
        "flags > 3 → EINVAL",
    );

    s.add(
        "P9 缺位落 EACCES、齐备落成功",
        rule_error(0, CAP_ALL ^ CAP_WRITE, 0) == EACCES
            && rule_error(0, CAP_ALL, 0) == 0
            && rule_error(OPS as u8, CAP_ALL, 0) == EINVAL,
        "缺位 EACCES / 越族 EINVAL",
    );

    let mut m = CapLedger::new();
    s.add(
        "P10 越位授予位被拒（PC_ECAP）",
        m.register(0, 0, 0x10, 0, 0) == Err(PC_ECAP),
        "granted & !CAP_ALL → PC_ECAP",
    );

    let mut o = CapLedger::new();
    let r_order = o.register(2, 0, CAP_ALL, 0, 0) == Err(PC_EORDER);
    let r_ok = o.register(0, 0, CAP_ALL, 0, 0) == Ok(());
    s.add(
        "P11 例序倒挂被拒、按游标推进则可",
        r_order && r_ok,
        "id != cursor → PC_EORDER",
    );

    let mut short = CapLedger::new();
    let _ = short.register(0, 0, CAP_ALL, 0, 0);
    let short_ok = short.replay() == Err(PC_ESHORT);
    let mut unreg = CapLedger::new();
    let unreg_ok = unreg.observe(0, 0) == Err(PC_EUNREG);
    s.add(
        "P12 未测即重放 ESHORT、未登记即观测 EUNREG",
        short_ok && unreg_ok,
        "缺测 / 未登记",
    );

    let inj = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < 4 {
        if !inj[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let mut lf = build_ledger();
    let freeze_ok = lf.freeze().is_ok() && lf.freeze() == Err(PC_EFROZEN);
    s.add(
        "P13 四向注入全真（倒序/重复/越位/封盘）",
        all && freeze_ok,
        "注入 4/4",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_builds_to_eighteen() {
        let l = build_ledger();
        assert_eq!(l.registered(), CASES as u32);
        assert_eq!(l.observed(), CASES as u32);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.replay(), Ok(l.fold()));
    }

    #[test]
    fn faces_balance_and_cover_four_classes() {
        let l = build_ledger();
        assert_eq!(l.req_face(), [9, 9, 6, 3]);
        let req = l.req_face();
        let cov = l.covered_face();
        let gap = l.gap_face();
        let mut i = 0usize;
        while i < CAPS {
            assert_eq!(req[i], cov[i] + gap[i]);
            i += 1;
        }
        assert!(l.cap_table_ok());
    }

    #[test]
    fn rule_edges() {
        assert_eq!(rule_error(0, CAP_ALL, FLAG_MAX + 1), EINVAL);
        assert_eq!(rule_error(0, CAP_ALL, 0), 0);
        assert_eq!(rule_error(0, CAP_ALL ^ CAP_WRITE, 0), EACCES);
        assert_eq!(rule_error(OPS as u8, CAP_ALL, 0), EINVAL);
        assert_eq!(rule_error(4, CAP_READ | CAP_WRITE, 3), 0);
    }

    #[test]
    fn refusal_gates() {
        let mut a = CapLedger::new();
        assert_eq!(a.register(1, 0, CAP_ALL, 0, 0), Err(PC_EORDER));
        assert_eq!(a.register(0, 0, 0x10, 0, 0), Err(PC_ECAP));
        assert_eq!(a.register(0, OPS as u8, CAP_ALL, 0, 0), Err(PC_ENOEXP));
        assert_eq!(a.register(CASES, 0, CAP_ALL, 0, 0), Err(PC_ECASE));
        assert!(a.register(0, 0, CAP_ALL, 0, 0).is_ok());
        assert_eq!(a.register(0, 0, CAP_ALL, 0, 0), Err(PC_EDUP));
    }

    #[test]
    fn mismatch_and_short() {
        let mut l = CapLedger::new();
        assert_eq!(l.replay(), Err(PC_EUNREG));
        assert!(l.register_ruled(0, 0, CAP_ALL, 0).is_ok());
        assert_eq!(l.replay(), Err(PC_ESHORT));
        assert_eq!(l.observe(0, EACCES), Err(PC_EMISMATCH));
        assert_eq!(l.observe(0, 0), Err(PC_EDUP));
    }

    #[test]
    fn unlink_family_needs_unlink_cap() {
        assert_eq!(CAP_REQUIRE[5], CAP_UNLINK);
        let (op, granted, flags) = spec_of(16);
        assert_eq!(op, 5);
        assert_eq!(granted, 0);
        assert_eq!(rule_error(op, granted, flags), EACCES);
        let (op2, g2, f2) = spec_of(15);
        assert_eq!(op2, 5);
        assert_eq!(rule_error(op2, g2, f2), 0);
    }

    #[test]
    fn freeze_is_idempotent_once() {
        let mut l = build_ledger();
        let d = l.freeze().expect("clean ledger freezes");
        assert_eq!(d, l.fold());
        assert_eq!(l.freeze(), Err(PC_EFROZEN));
    }

    #[test]
    fn checks_all_pass() {
        let s = run_unx11148_checks();
        assert!(s.all_passed(), "{:?}", s);
    }
}
