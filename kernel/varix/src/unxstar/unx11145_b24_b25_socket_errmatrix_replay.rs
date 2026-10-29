//! UNX-F11145 B24–B25 清账（unix socket 错误矩阵两批判据终重放）（0x1f2d0 段）
//!
//! 纯功能模型：socket 域的两批清账（B24 / B25）各十二例，合计二十四例。
//! 两批之外还有一本「序表」：全局重放顺序必须与批内位置严格挂接——
//! 全局序号 = 批号 * 12 + 批内序号，一序一例、一例一序，双射成立才算
//! 序表账闭合。挂接错了（序号与（批, 位）不符）、一号挂两例、一例挂两号，
//! 都要当场拒止。
//!
//! 规则面（对齐 socket(2) 系调用语义的裁剪版）：
//!   - socket 的参数低两位取 2 表示不支持的地址族，落 EAFNOSUPPORT(97)；
//!   - bind 到特权端口（1..=1023）落 EACCES(13)；
//!   - connect 的参数为奇数视为无监听方，落 ECONNREFUSED(111)；
//!   - listen 的 backlog 为 0 落 EINVAL(22)；
//!   - send / recv 超过 65535 字节落 EMSGSIZE(90)；
//!   - shutdown 的 how 大于 2 落 EINVAL；
//!   - getsockopt / setsockopt 的选项号大于 3 落 ENOPROTOOPT(92)；
//!   - accept 一律成功。
//!
//! 拒止面：例号越界、重复登记、例序倒挂、期望码越界、未登记即观测、
//! 缺实测、不符、序号越界、序号重挂、挂接张冠李戴、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2d0;
/// 例号越界
pub const SK_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const SK_EDUP: u32 = SEG_BASE + 0x02;
/// 例序倒挂
pub const SK_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const SK_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const SK_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const SK_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const SK_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 全局序号越界或重挂
pub const SK_ESEQ: u32 = SEG_BASE + 0x08;
/// 挂接张冠李戴
pub const SK_EMAP: u32 = SEG_BASE + 0x09;
/// 已封盘
pub const SK_EFROZEN: u32 = SEG_BASE + 0x0a;

/// 批数：B24 / B25。
pub const BATCHES: usize = 2;
/// 批名。
pub const BATCH_NAMES: [&str; BATCHES] = ["B24", "B25"];
/// 每批例数。
pub const PER_BATCH: usize = 12;
/// 总例数。
pub const CASES: usize = BATCHES * PER_BATCH;
/// 调用族数。
pub const OPS: usize = 10;
/// 族名。
pub const OP_NAMES: [&str; OPS] = [
    "socket",
    "bind",
    "connect",
    "listen",
    "accept",
    "send",
    "recv",
    "shutdown",
    "getsockopt",
    "setsockopt",
];
/// 特权端口上界。
pub const PRIV_PORT_MAX: u32 = 1023;
/// 单次收发字节上界。
pub const MSG_MAX: u32 = 65_535;
/// 选项号上界。
pub const OPT_MAX: u32 = 3;
/// 关闭方式上界（SHUT_RD / SHUT_WR / SHUT_RDWR）。
pub const HOW_MAX: u32 = 2;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;

/// EACCES
pub const EACCES: u32 = 13;
/// EINVAL
pub const EINVAL: u32 = 22;
/// EMSGSIZE
pub const EMSGSIZE: u32 = 90;
/// ENOPROTOOPT
pub const ENOPROTOOPT: u32 = 92;
/// EAFNOSUPPORT
pub const EAFNOSUPPORT: u32 = 97;
/// ECONNREFUSED
pub const ECONNREFUSED: u32 = 111;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

const OP_SOCKET: u8 = 0;
const OP_BIND: u8 = 1;
const OP_CONNECT: u8 = 2;
const OP_LISTEN: u8 = 3;
const OP_ACCEPT: u8 = 4;
const OP_SEND: u8 = 5;
const OP_RECV: u8 = 6;
const OP_SHUTDOWN: u8 = 7;
const OP_GETSOCKOPT: u8 = 8;
const OP_SETSOCKOPT: u8 = 9;

/// 规则纯函数：由（调用, 参数）给出应有 errno。
pub fn rule_error(op: u8, arg: u32) -> u32 {
    if op as usize >= OPS {
        return EINVAL;
    }
    match op {
        OP_SOCKET => {
            if arg % 4 == 2 {
                EAFNOSUPPORT
            } else {
                0
            }
        }
        OP_BIND => {
            if arg >= 1 && arg <= PRIV_PORT_MAX {
                EACCES
            } else {
                0
            }
        }
        OP_CONNECT => {
            if arg % 2 == 1 {
                ECONNREFUSED
            } else {
                0
            }
        }
        OP_LISTEN => {
            if arg == 0 {
                EINVAL
            } else {
                0
            }
        }
        OP_SEND | OP_RECV => {
            if arg > MSG_MAX {
                EMSGSIZE
            } else {
                0
            }
        }
        OP_SHUTDOWN => {
            if arg > HOW_MAX {
                EINVAL
            } else {
                0
            }
        }
        OP_GETSOCKOPT | OP_SETSOCKOPT => {
            if arg > OPT_MAX {
                ENOPROTOOPT
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// 第（批, 位）例的调用与参数。
pub fn spec_of(batch: usize, idx: usize) -> (u8, u32) {
    let g = batch.wrapping_mul(PER_BATCH).wrapping_add(idx);
    let op = (g % OPS) as u8;
    // 参数取值：让每条规则分支都能被踩到。
    let arg = match op {
        OP_SOCKET => (g as u32) % 4,
        OP_BIND => (g as u32) % 2 * 2000,
        OP_CONNECT => (g as u32) % 3,
        OP_LISTEN => (g as u32) % 2,
        OP_SEND | OP_RECV => (g as u32) % 2 * 70_000,
        OP_SHUTDOWN => (g as u32) % 4,
        OP_GETSOCKOPT => (g as u32) % 5,
        _ => (g as u32) % 3,
    };
    (op, arg)
}

/// 批内一例。
#[derive(Clone, Copy, Debug)]
pub struct SockCase {
    pub op: u8,
    pub arg: u32,
    pub expected: u32,
    pub got: u32,
    pub seen: bool,
    pub live: bool,
}

impl SockCase {
    const EMPTY: SockCase = SockCase {
        op: 0,
        arg: 0,
        expected: 0,
        got: 0,
        seen: false,
        live: false,
    };

    /// 已测且相符。
    pub fn agrees(&self) -> bool {
        self.live && self.seen && self.got == self.expected
    }

    /// 期望与规则复算一致。
    pub fn rule_agrees(&self) -> bool {
        self.live && rule_error(self.op, self.arg) == self.expected
    }
}

/// 单批账：十二例。
#[derive(Clone, Debug)]
pub struct Batch {
    pub no: u8,
    cases: [SockCase; PER_BATCH],
    registered: usize,
    observed: usize,
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl Batch {
    /// 新建空批。
    pub fn new(no: u8) -> Batch {
        Batch {
            no,
            cases: [SockCase::EMPTY; PER_BATCH],
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

    /// 登记批内第 idx 例。
    pub fn register(&mut self, idx: usize, op: u8, arg: u32, expected: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SK_EFROZEN));
        }
        if idx >= PER_BATCH || op as usize >= OPS {
            return Err(self.fail(SK_ECASE));
        }
        if let Some(c) = self.cases.get(idx) {
            if c.live {
                return Err(self.fail(SK_EDUP));
            }
        }
        if idx < self.cursor {
            return Err(self.fail(SK_EORDER));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(SK_ENOEXP));
        }
        if let Some(slot) = self.cases.get_mut(idx) {
            *slot = SockCase {
                op,
                arg,
                expected,
                got: 0,
                seen: false,
                live: true,
            };
        }
        self.registered = self.registered.wrapping_add(1);
        self.cursor = idx.wrapping_add(1);
        Ok(())
    }

    /// 登记并按规则复算期望。
    pub fn register_ruled(&mut self, idx: usize, op: u8, arg: u32) -> Result<(), u32> {
        let e = rule_error(op, arg);
        self.register(idx, op, arg, e)
    }

    /// 观测批内第 idx 例的实测。
    pub fn observe(&mut self, idx: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SK_EFROZEN));
        }
        if idx >= PER_BATCH {
            return Err(self.fail(SK_ECASE));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(SK_ENOEXP));
        }
        let c = match self.cases.get(idx) {
            Some(c) => *c,
            None => return Err(self.fail(SK_EUNREG)),
        };
        if !c.live {
            return Err(self.fail(SK_EUNREG));
        }
        if c.seen {
            return Err(self.fail(SK_EDUP));
        }
        if let Some(slot) = self.cases.get_mut(idx) {
            slot.got = errno;
            slot.seen = true;
        }
        self.observed = self.observed.wrapping_add(1);
        Ok(())
    }

    /// 只读单例。
    pub fn case(&self, idx: usize) -> Option<SockCase> {
        self.cases.get(idx).copied()
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

impl Batch {
    /// 不符例数。
    pub fn mismatches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < PER_BATCH {
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
        while i < PER_BATCH {
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

    /// 本批使用的调用族分布。
    pub fn op_table(&self) -> [u32; OPS] {
        let mut out = [0u32; OPS];
        let mut i = 0usize;
        while i < PER_BATCH {
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
        while i < PER_BATCH {
            if let Some(c) = self.cases.get(i) {
                if c.rule_agrees() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 批指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ self.no as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.registered as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.observed as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < PER_BATCH {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    h ^= c.op as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.arg as u64;
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

    /// 封盘：幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        let root = self.digest();
        if self.frozen {
            return Ok(root);
        }
        self.frozen = true;
        Ok(self.digest())
    }
}

/// 双批账 + 序表。
#[derive(Clone, Debug)]
pub struct TwinLedger {
    pub base: u32,
    batches: [Batch; BATCHES],
    /// 序表：全局序号 -> (批号 + 1)，0 表示空槽。
    seq_batch: [u8; CASES],
    /// 序表：全局序号 -> (批内序号 + 1)，0 表示空槽。
    seq_slot: [u8; CASES],
    attached: usize,
    rejects: u32,
    frozen: bool,
}

impl TwinLedger {
    /// 新建双批空账。
    pub fn new(base: u32) -> TwinLedger {
        TwinLedger {
            base,
            batches: [Batch::new(0), Batch::new(1)],
            seq_batch: [0u8; CASES],
            seq_slot: [0u8; CASES],
            attached: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 取某一批（可写）。
    pub fn batch(&mut self, b: usize) -> Option<&mut Batch> {
        self.batches.get_mut(b)
    }

    /// 取某一批（只读）。
    pub fn batch_ref(&self, b: usize) -> Option<&Batch> {
        self.batches.get(b)
    }

    /// 序表挂接：序号必须等于 批号 * 12 + 批内序号，一序一例、一例一序。
    pub fn attach(&mut self, seq: usize, batch: usize, idx: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SK_EFROZEN));
        }
        if seq >= CASES || batch >= BATCHES || idx >= PER_BATCH {
            return Err(self.fail(SK_ESEQ));
        }
        if batch.wrapping_mul(PER_BATCH).wrapping_add(idx) != seq {
            return Err(self.fail(SK_EMAP));
        }
        let mut dup_seq = false;
        if let Some(slot) = self.seq_batch.get(seq) {
            dup_seq = *slot != 0;
        }
        if dup_seq {
            return Err(self.fail(SK_ESEQ));
        }
        // 一例挂两号：扫一遍已挂接项，看（批, 位）是否已出现。
        let tag = (batch as u8).wrapping_add(1);
        let mut i = 0usize;
        while i < CASES {
            if let (Some(b0), Some(s0)) = (self.seq_batch.get(i), self.seq_slot.get(i)) {
                if *b0 == tag && *s0 == (idx as u8).wrapping_add(1) {
                    return Err(self.fail(SK_EMAP));
                }
            }
            i = i.wrapping_add(1);
        }
        if let Some(slot) = self.seq_batch.get_mut(seq) {
            *slot = tag;
        }
        if let Some(slot) = self.seq_slot.get_mut(seq) {
            *slot = (idx as u8).wrapping_add(1);
        }
        self.attached = self.attached.wrapping_add(1);
        Ok(())
    }

    /// 序表是否双射闭合：二十四个槽全满，且每（批, 位）恰好一次。
    pub fn seq_bijective(&self) -> bool {
        if self.attached != CASES {
            return false;
        }
        let mut seen = [false; CASES];
        let mut i = 0usize;
        while i < CASES {
            let b = match self.seq_batch.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let k = match self.seq_slot.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if b == 0 || k == 0 {
                return false;
            }
            let g = (b as usize - 1)
                .wrapping_mul(PER_BATCH)
                .wrapping_add(k as usize - 1);
            if g >= CASES {
                return false;
            }
            if let Some(f) = seen.get_mut(g) {
                if *f {
                    return false;
                }
                *f = true;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 已挂接序号数。
    pub fn attached(&self) -> usize {
        self.attached
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

impl TwinLedger {
    /// 双批账指纹：基址、拒止、两批分指纹，再并入整张序表。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get(i) {
                h ^= b.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        i = 0usize;
        while i < CASES {
            if let (Some(b0), Some(s0)) = (self.seq_batch.get(i), self.seq_slot.get(i)) {
                h ^= ((*b0 as u64) << 8) | (*s0 as u64);
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 终重放：序表齐、两批齐、逐例相符、序表双射。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.attached != CASES {
            return Err(self.fail(SK_ESHORT));
        }
        let mut i = 0usize;
        while i < BATCHES {
            let (reg, obs) = match self.batches.get(i) {
                Some(b) => (b.registered(), b.observed()),
                None => (0, 0),
            };
            if reg != PER_BATCH || obs != PER_BATCH {
                return Err(self.fail(SK_ESHORT));
            }
            i = i.wrapping_add(1);
        }
        let mut mis = 0u32;
        let mut j = 0usize;
        while j < BATCHES {
            if let Some(b) = self.batches.get(j) {
                mis = mis.wrapping_add(b.mismatches());
            }
            j = j.wrapping_add(1);
        }
        if mis != 0 {
            return Err(self.fail(SK_EMISMATCH));
        }
        if !self.seq_bijective() {
            return Err(self.fail(SK_ESEQ));
        }
        self.freeze()
    }

    /// 封盘：两批一并冻住，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = self.batches.get_mut(i) {
                let _ = b.freeze();
            }
            i = i.wrapping_add(1);
        }
        self.frozen = true;
        Ok(self.fold())
    }
}

/// 搭建双批账：登记、回填、挂序表，最后终重放。
pub fn build_twin(base: u32) -> (TwinLedger, Result<u64, u32>) {
    let mut t = TwinLedger::new(base);
    let mut b = 0usize;
    while b < BATCHES {
        let mut idx = 0usize;
        while idx < PER_BATCH {
            let (op, arg) = spec_of(b, idx);
            let reg = match t.batch(b) {
                Some(x) => x.register_ruled(idx, op, arg),
                None => Err(SK_ECASE),
            };
            if reg.is_err() {
                return (t, Err(SK_ECASE));
            }
            let want = match t.batch_ref(b).and_then(|x| x.case(idx)) {
                Some(c) => c.expected,
                None => 0,
            };
            let obs = match t.batch(b) {
                Some(x) => x.observe(idx, want),
                None => Err(SK_ECASE),
            };
            if obs.is_err() {
                return (t, Err(SK_ECASE));
            }
            let seq = b.wrapping_mul(PER_BATCH).wrapping_add(idx);
            if t.attach(seq, b, idx).is_err() {
                return (t, Err(SK_EMAP));
            }
            idx = idx.wrapping_add(1);
        }
        b = b.wrapping_add(1);
    }
    let r = t.replay();
    (t, r)
}

/// 四路注入：挂接错位、序号重挂、封盘后登记、实测不符。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 挂接张冠李戴：序号 3 挂到（批 0, 位 2）。
    let mut a = TwinLedger::new(0x2200);
    out[0] = a.attach(3, 0, 2).err() == Some(SK_EMAP);

    // ② 一序挂两例。
    let mut b = TwinLedger::new(0x2201);
    let first = b.attach(0, 0, 0);
    let twice = b.attach(0, 0, 0);
    out[1] = first.is_ok() && twice.err() == Some(SK_ESEQ);

    // ③ 封盘后批次仍不可动账。
    let (mut c, ok3) = build_twin(0x2202);
    let after = match c.batch(0) {
        Some(x) => x.register_ruled(0, OP_SOCKET, 0),
        None => Err(SK_ECASE),
    };
    out[2] = ok3.is_ok() && c.frozen() && after.err() == Some(SK_EFROZEN);

    // ④ 实测不符必被批账抓到。
    let mut d = Batch::new(0);
    let mut ok4 = d.register_ruled(0, OP_ACCEPT, 0).is_ok();
    ok4 = ok4 && d.observe(0, EINVAL).is_ok();
    out[3] = ok4 && d.mismatches() == 1;

    out
}

/// 判据集：K1~K13。
pub fn run_unx11145_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11145.b24_b25_socket_errmatrix");

    // K1 常量面：段基址、两批、每批十二、总二十四、十族。
    let consts = SEG_BASE == 0x1f2d0
        && BATCHES == 2
        && PER_BATCH == 12
        && CASES == 24
        && OPS == 10
        && OP_NAMES.len() == OPS
        && BATCH_NAMES.len() == BATCHES;
    s.add("K1-常量面", consts, "两批各 12 例 = 24，十族调用");

    // K2 双批账搭建成功并封盘。
    let (t2, r2) = build_twin(0x1200);
    s.add(
        "K2-搭建封盘",
        r2.is_ok() && t2.frozen(),
        "build_twin 终重放通过并封盘",
    );

    // K3 两批各自齐账。
    let mut ok3 = true;
    let mut i3 = 0usize;
    while i3 < BATCHES {
        match t2.batch_ref(i3) {
            Some(b) => {
                ok3 = ok3 && b.registered() == PER_BATCH && b.observed() == PER_BATCH;
                ok3 = ok3 && b.mismatches() == 0 && b.gaps() == 0;
            }
            None => ok3 = false,
        }
        i3 = i3.wrapping_add(1);
    }
    s.add("K3-两批齐账", ok3, "两批皆登记齐、实测齐、无缺口");

    // K4 序表双射：二十四槽全满且无重挂。
    s.add_dyn(
        "K4-序表双射",
        t2.seq_bijective() && t2.attached() == CASES,
        &format!("attached={}/{} bij={}", t2.attached(), CASES, t2.seq_bijective()),
    );

    // K5 规则复算一致：两批合计二十四例全对。
    let mut agree5 = 0usize;
    let mut i5 = 0usize;
    while i5 < BATCHES {
        if let Some(b) = t2.batch_ref(i5) {
            agree5 = agree5.wrapping_add(b.rule_agreement());
        }
        i5 = i5.wrapping_add(1);
    }
    s.add_dyn(
        "K5-规则一致",
        agree5 == CASES,
        &format!("agree={}/{}", agree5, CASES),
    );

    // K6 十族覆盖：合计二十四例覆盖十族。
    let mut cov6 = [0u32; OPS];
    let mut i6 = 0usize;
    while i6 < BATCHES {
        if let Some(b) = t2.batch_ref(i6) {
            let t = b.op_table();
            let mut j = 0usize;
            while j < OPS {
                cov6[j] = cov6[j].wrapping_add(t[j]);
                j = j.wrapping_add(1);
            }
        }
        i6 = i6.wrapping_add(1);
    }
    let mut all6 = true;
    let mut sum6 = 0u32;
    let mut k6 = 0usize;
    while k6 < OPS {
        all6 = all6 && cov6[k6] > 0;
        sum6 = sum6.wrapping_add(cov6[k6]);
        k6 = k6.wrapping_add(1);
    }
    s.add_dyn(
        "K6-十族覆盖",
        all6 && sum6 == CASES as u32,
        &format!("sum={} ops={}", sum6, OPS),
    );

    // K7 两批指纹互异（B24 / B25 不得撞成一本）。
    let d0 = t2.batch_ref(0).map(|b| b.digest()).unwrap_or(0);
    let d1 = t2.batch_ref(1).map(|b| b.digest()).unwrap_or(1);
    s.add_dyn(
        "K7-两批互异",
        d0 != d1,
        &format!("b24={:x} b25={:x}", d0, d1),
    );

    // K8 挂接错位被拒。
    let mut m8 = TwinLedger::new(0x1201);
    let e8 = m8.attach(3, 0, 2).err();
    s.add(
        "K8-错位拒止",
        e8 == Some(SK_EMAP) && m8.rejects() == 1,
        "序号与（批, 位）不符落 SK_EMAP",
    );

    // K9 一序挂两例被拒。
    let mut m9 = TwinLedger::new(0x1202);
    let a9 = m9.attach(0, 0, 0);
    let b9 = m9.attach(0, 0, 0);
    s.add(
        "K9-重挂拒止",
        a9.is_ok() && b9.err() == Some(SK_ESEQ),
        "同序二次挂接落 SK_ESEQ",
    );

    // K10 序表不全时终重放必被拒。
    let mut m10 = TwinLedger::new(0x1203);
    let e10 = m10.replay().err();
    s.add(
        "K10-序表不全",
        e10 == Some(SK_ESHORT),
        "空序表 replay 落 SK_ESHORT",
    );

    // K11 封盘幂等。
    let (t11, r11) = build_twin(0x1204);
    let mut t11b = t11.clone();
    let ra = t11b.freeze();
    let rb = t11b.freeze();
    s.add_dyn(
        "K11-幂等",
        r11.is_ok() && ra.is_ok() && ra == rb,
        &format!("root={:#x}", ra.unwrap_or(0)),
    );

    // K12 四路注入全过。
    let inj = inject_four();
    s.add_dyn(
        "K12-四注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        &format!("{}/{}/{}/{}", inj[0], inj[1], inj[2], inj[3]),
    );

    // K13 干净账拒止为零。
    s.add_dyn(
        "K13-干净账",
        t2.rejects() == 0,
        &format!("rejects={}", t2.rejects()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11145_flow() {
        let (t, r) = build_twin(0x1000);
        assert!(r.is_ok());
        assert!(t.frozen());
        assert_eq!(t.attached(), CASES);
        assert!(t.seq_bijective());
        let mut i = 0usize;
        while i < BATCHES {
            let b = t.batch_ref(i).expect("批在册");
            assert_eq!(b.registered(), PER_BATCH);
            assert_eq!(b.observed(), PER_BATCH);
            assert_eq!(b.mismatches(), 0);
            assert_eq!(b.gaps(), 0);
            i += 1;
        }
    }

    #[test]
    fn f11145_gates() {
        // 批次层拒止。
        let mut b = Batch::new(0);
        assert!(b.register_ruled(0, OP_SOCKET, 0).is_ok());
        assert_eq!(b.register_ruled(0, OP_SOCKET, 0).err(), Some(SK_EDUP));
        assert_eq!(b.register(20, OP_SOCKET, 0, 0).err(), Some(SK_ECASE));
        assert_eq!(b.register(1, OPS as u8, 0, 0).err(), Some(SK_ECASE));
        assert!(b.register_ruled(3, OP_BIND, 80).is_ok());
        assert_eq!(b.register_ruled(2, OP_BIND, 80).err(), Some(SK_EORDER));
        assert_eq!(b.register(4, OP_BIND, 80, 999).err(), Some(SK_ENOEXP));
        assert_eq!(b.observe(1, 0).err(), Some(SK_EUNREG));
        assert!(b.observe(0, 0).is_ok());
        assert_eq!(b.observe(0, 0).err(), Some(SK_EDUP));
        assert_eq!(b.rejects(), 7);
        assert!(b.freeze().is_ok());
        assert_eq!(b.register_ruled(5, OP_BIND, 0).err(), Some(SK_EFROZEN));

        // 序表层拒止。
        let mut t = TwinLedger::new(0x3000);
        assert_eq!(t.attach(24, 0, 0).err(), Some(SK_ESEQ));
        assert_eq!(t.attach(0, 2, 0).err(), Some(SK_ESEQ));
        assert_eq!(t.attach(3, 0, 2).err(), Some(SK_EMAP));
        assert!(t.attach(0, 0, 0).is_ok());
        assert_eq!(t.attach(0, 0, 0).err(), Some(SK_ESEQ));
        assert_eq!(t.rejects(), 4);
        assert!(t.freeze().is_ok());
        assert_eq!(t.attach(1, 0, 1).err(), Some(SK_EFROZEN));
    }

    #[test]
    fn f11145_rules() {
        // socket：低两位取 2 视为不支持的地址族。
        assert_eq!(rule_error(OP_SOCKET, 2), EAFNOSUPPORT);
        assert_eq!(rule_error(OP_SOCKET, 0), 0);
        assert_eq!(rule_error(OP_SOCKET, 3), 0);
        // bind：特权端口。
        assert_eq!(rule_error(OP_BIND, 80), EACCES);
        assert_eq!(rule_error(OP_BIND, PRIV_PORT_MAX), EACCES);
        assert_eq!(rule_error(OP_BIND, PRIV_PORT_MAX + 1), 0);
        assert_eq!(rule_error(OP_BIND, 0), 0);
        // connect：奇数视为无监听方。
        assert_eq!(rule_error(OP_CONNECT, 1), ECONNREFUSED);
        assert_eq!(rule_error(OP_CONNECT, 2), 0);
        // listen：backlog 为 0。
        assert_eq!(rule_error(OP_LISTEN, 0), EINVAL);
        assert_eq!(rule_error(OP_LISTEN, 1), 0);
        // accept 一律成功。
        assert_eq!(rule_error(OP_ACCEPT, 0), 0);
        assert_eq!(rule_error(OP_ACCEPT, 99), 0);
        // send / recv 字节上界。
        assert_eq!(rule_error(OP_SEND, MSG_MAX), 0);
        assert_eq!(rule_error(OP_SEND, MSG_MAX + 1), EMSGSIZE);
        assert_eq!(rule_error(OP_RECV, MSG_MAX + 1), EMSGSIZE);
        // shutdown how 上界。
        assert_eq!(rule_error(OP_SHUTDOWN, HOW_MAX), 0);
        assert_eq!(rule_error(OP_SHUTDOWN, HOW_MAX + 1), EINVAL);
        // 选项号上界。
        assert_eq!(rule_error(OP_GETSOCKOPT, OPT_MAX), 0);
        assert_eq!(rule_error(OP_GETSOCKOPT, OPT_MAX + 1), ENOPROTOOPT);
        assert_eq!(rule_error(OP_SETSOCKOPT, OPT_MAX + 1), ENOPROTOOPT);
        // 族越界。
        assert_eq!(rule_error(OPS as u8, 0), EINVAL);
    }

    #[test]
    fn f11145_seq_and_inject() {
        let (t, r) = build_twin(0x1005);
        assert!(r.is_ok());
        assert_eq!(t.rejects(), 0);
        assert_eq!(inject_four(), [true, true, true, true]);
        // 十条族在二十四例中皆出现。
        let mut cov = [0u32; OPS];
        let mut i = 0usize;
        while i < BATCHES {
            if let Some(b) = t.batch_ref(i) {
                let tb = b.op_table();
                let mut j = 0usize;
                while j < OPS {
                    cov[j] = cov[j].wrapping_add(tb[j]);
                    j += 1;
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < OPS {
            assert!(cov[k] > 0, "族 {} 未覆盖", OP_NAMES[k]);
            k += 1;
        }
    }

    #[test]
    fn f11145_checks_green() {
        let cs = run_unx11145_checks();
        assert!(cs.all_passed(), "checks red in unx11145");
    }
}
