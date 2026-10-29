//! UNX-F11143 B22 清账（信号错误矩阵判据终重放）（0x1f2b0 段）
//!
//! 纯功能模型：信号族的八类调用（signal / sigaction / sigprocmask /
//! sigpending / sigsuspend / sigwait / kill / sigtimedwait）各有自己的
//! 合法性规则。矩阵每一例由（调用, 信号号, 模式）三元组确定，期望 errno
//! 既可由登记时给定、也可由规则纯函数复算——重放要求两账逐例一致。
//!
//! 规则面（与 POSIX 语义对齐的裁剪版）：
//!   - 信号号 0 只在 kill 上合法（探活语义，返回 0）；
//!   - SIGKILL(9) / SIGSTOP(19) 不可捕获、不可屏蔽，凡涉安装或屏蔽即 EINVAL；
//!     对它们的 kill 因无权限裁剪为 EPERM(1)；
//!   - 信号号大于 64 视为非法，一律 EINVAL；
//!   - sigprocmask 只有 BLOCK/UNBLOCK/SETMASK 三种模式，第四种越界 EINVAL；
//!   - 其余组合返回 0。
//!
//! 拒止面：用例号越界、重复登记、用例序倒挂、期望码越界、未登记即观测、
//! 缺实测、不符、冻结后再动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f2b0;
/// 用例号越界
pub const SG_ECASE: u32 = SEG_BASE + 0x01;
/// 重复登记
pub const SG_EDUP: u32 = SEG_BASE + 0x02;
/// 用例序倒挂
pub const SG_EORDER: u32 = SEG_BASE + 0x03;
/// 期望码越界
pub const SG_ENOEXP: u32 = SEG_BASE + 0x04;
/// 未登记即观测
pub const SG_EUNREG: u32 = SEG_BASE + 0x05;
/// 未测即重放
pub const SG_ESHORT: u32 = SEG_BASE + 0x06;
/// 实测与期望不符
pub const SG_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 空矩阵
pub const SG_EEMPTY: u32 = SEG_BASE + 0x08;
/// 已封盘
pub const SG_EFROZEN: u32 = SEG_BASE + 0x09;

/// 调用族数。
pub const OPS: usize = 8;
/// 族名。
pub const OP_NAMES: [&str; OPS] = [
    "signal",
    "sigaction",
    "sigprocmask",
    "sigpending",
    "sigsuspend",
    "sigwait",
    "kill",
    "sigtimedwait",
];
/// 用例数（每族两到三例）。
pub const CASES: usize = 20;
/// 参与轮转的信号号：探活 0、SIGHUP 1、SIGKILL 9、SIGSTOP 19、SIGRTMIN 34、上界 64。
pub const SIGNUMS: [u32; 6] = [0, 1, 9, 19, 34, 64];
/// 屏蔽模式数。
pub const MODES: u8 = 4;
/// errno 上界。
pub const ERRNO_MAX: u16 = 120;

/// 合法模式上界（BLOCK=0 / UNBLOCK=1 / SETMASK=2 / 越界=3）。
pub const MODE_SETMASK: u8 = 2;
/// SIGKILL。
pub const SIGKILL: u32 = 9;
/// SIGSTOP。
pub const SIGSTOP: u32 = 19;
/// 信号号上界。
pub const SIG_MAX: u32 = 64;
/// EPERM。
pub const EPERM: u32 = 1;
/// EINVAL。
pub const EINVAL: u32 = 22;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

const OP_SIGNAL: u8 = 0;
const OP_SIGACTION: u8 = 1;
const OP_SIGPROCMASK: u8 = 2;
const OP_SIGPENDING: u8 = 3;
const OP_SIGSUSPEND: u8 = 4;
const OP_SIGWAIT: u8 = 5;
const OP_KILL: u8 = 6;
const OP_SIGTIMEDWAIT: u8 = 7;

/// 单例三元组（调用, 信号号, 模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub op: u8,
    pub sig: u32,
    pub mode: u8,
}

/// 规则纯函数：由三元组给出该例应有的 errno（0 表示应成功）。
pub fn rule_error(spec: Spec) -> u32 {
    if spec.op >= OPS as u8 {
        return EINVAL;
    }
    if spec.sig > SIG_MAX {
        return EINVAL;
    }
    if spec.flag_invalid() {
        return EINVAL;
    }
    let fatal = spec.sig == SIGKILL || spec.sig == SIGSTOP;
    match spec.op {
        // 探活：信号号 0 是唯一合法用法，其余按权限裁剪为 EPERM。
        OP_KILL => {
            if spec.sig == 0 {
                0
            } else if fatal {
                EPERM
            } else {
                0
            }
        }
        // 安装处置：不可捕获的两个信号必被拒。
        OP_SIGNAL | OP_SIGACTION => {
            if spec.sig == 0 || fatal {
                EINVAL
            } else {
                0
            }
        }
        // 屏蔽：不可屏蔽的两个信号必被拒。
        OP_SIGPROCMASK => {
            if spec.sig == 0 || fatal {
                EINVAL
            } else {
                0
            }
        }
        // 挂起集合查询无副作用，任何信号号都合法（0 表示空集探测）。
        OP_SIGPENDING => 0,
        // 等待族：0 不是可等待信号。
        OP_SIGSUSPEND | OP_SIGWAIT | OP_SIGTIMEDWAIT => {
            if spec.sig == 0 {
                EINVAL
            } else {
                0
            }
        }
        _ => EINVAL,
    }
}

impl Spec {
    /// 只有 sigprocmask 读模式；其余调用模式位必须为 0。
    fn flag_invalid(&self) -> bool {
        if self.op == OP_SIGPROCMASK {
            self.mode > MODE_SETMASK
        } else {
            self.mode != 0
        }
    }

    /// 该例是否落在应成功的一侧。
    pub fn expects_success(&self) -> bool {
        rule_error(*self) == 0
    }
}

/// 矩阵中的一例：三元组 + 登记期望 + 实测。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub spec: Spec,
    pub expected: u32,
    pub got: u32,
    pub seen: bool,
    pub live: bool,
}

impl Case {
    const EMPTY: Case = Case {
        spec: Spec { op: 0, sig: 0, mode: 0 },
        expected: 0,
        got: 0,
        seen: false,
        live: false,
    };

    /// 该例是否相符（须已实测）。
    pub fn agrees(&self) -> bool {
        self.live && self.seen && self.got == self.expected
    }

    /// 该例是否与规则复算一致。
    pub fn rule_agrees(&self) -> bool {
        self.live && rule_error(self.spec) == self.expected
    }
}

/// 信号错误矩阵。
#[derive(Clone, Debug)]
pub struct SigMatrix {
    pub base: u32,
    cases: [Case; CASES],
    registered: usize,
    cursor: usize,
    observed: usize,
    rejects: u32,
    frozen: bool,
}

impl SigMatrix {
    /// 新建空矩阵。
    pub fn new(base: u32) -> SigMatrix {
        SigMatrix {
            base,
            cases: [Case::EMPTY; CASES],
            registered: 0,
            cursor: 0,
            observed: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一例：号段内、不重复、严格递增、期望码不越界。
    pub fn register(&mut self, id: usize, spec: Spec, expected: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(SG_ECASE));
        }
        if let Some(slot) = self.cases.get(id) {
            if slot.live {
                return Err(self.fail(SG_EDUP));
            }
        }
        if id < self.cursor {
            return Err(self.fail(SG_EORDER));
        }
        if expected > ERRNO_MAX as u32 {
            return Err(self.fail(SG_ENOEXP));
        }
        if spec.op >= OPS as u8 {
            return Err(self.fail(SG_ECASE));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            *slot = Case {
                spec,
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

    /// 登记并按规则复算期望（供构造使用）。
    pub fn register_ruled(&mut self, id: usize, spec: Spec) -> Result<(), u32> {
        let expected = rule_error(spec);
        self.register(id, spec, expected)
    }

    /// 观测实测：须已登记、只能测一次、值在码域内。
    pub fn observe(&mut self, id: usize, errno: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(SG_EFROZEN));
        }
        if id >= CASES {
            return Err(self.fail(SG_ECASE));
        }
        if errno > ERRNO_MAX as u32 {
            return Err(self.fail(SG_ENOEXP));
        }
        let live = match self.cases.get(id) {
            Some(c) => c.live,
            None => false,
        };
        if !live {
            return Err(self.fail(SG_EUNREG));
        }
        let seen = match self.cases.get(id) {
            Some(c) => c.seen,
            None => false,
        };
        if seen {
            return Err(self.fail(SG_EDUP));
        }
        if let Some(slot) = self.cases.get_mut(id) {
            slot.got = errno;
            slot.seen = true;
        }
        self.observed = self.observed.wrapping_add(1);
        Ok(())
    }
}

impl SigMatrix {
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

    /// 是否已封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 单例只读取用。
    pub fn case(&self, id: usize) -> Option<Case> {
        self.cases.get(id).copied()
    }

    /// 不符的用例号清单（升序）。
    pub fn mismatches(&self) -> [usize; CASES] {
        let mut out = [0usize; CASES];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live && c.seen && c.got != c.expected {
                    if let Some(slot) = out.get_mut(n) {
                        *slot = i;
                    }
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 不符例数。
    pub fn mismatch_count(&self) -> usize {
        let mut n = 0usize;
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

    /// 缺口：应登记而未登记、或已登记未实测的例数。
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

    /// 每族用例数分布。
    pub fn op_table(&self) -> [u32; OPS] {
        let mut out = [0u32; OPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    if let Some(slot) = out.get_mut(c.spec.op as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 族覆盖：每族都至少一例。
    pub fn ops_covered(&self) -> bool {
        let mut i = 0usize;
        let t = self.op_table();
        while i < OPS {
            match t.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 信号类覆盖：探活 / 普通 / 不可捕获 / 实时 / 上界 五类各至少一例。
    pub fn signal_classes_covered(&self) -> bool {
        let (mut zero, mut plain, mut fatal, mut rt, mut top) = (0u32, 0u32, 0u32, 0u32, 0u32);
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let s = c.spec.sig;
                    if s == 0 {
                        zero = zero.wrapping_add(1);
                    } else if s == SIGKILL || s == SIGSTOP {
                        fatal = fatal.wrapping_add(1);
                    } else if s == SIG_MAX {
                        top = top.wrapping_add(1);
                    } else if s >= 34 {
                        rt = rt.wrapping_add(1);
                    } else {
                        plain = plain.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        zero > 0 && plain > 0 && fatal > 0 && rt > 0 && top > 0
    }

    /// 期望码直方桶：成功 / EPERM / EINVAL / 其他 四桶。
    pub fn expected_histogram(&self) -> [u32; 4] {
        let mut out = [0u32; 4];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.live {
                    let b = if c.expected == 0 {
                        0usize
                    } else if c.expected == EPERM {
                        1usize
                    } else if c.expected == EINVAL {
                        2usize
                    } else {
                        3usize
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

    /// 规则复算一致性：登记期望与 rule_error 逐例相等。
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
}

impl SigMatrix {
    /// 折叠指纹：基址、登记/实测数、拒止数，逐例并入三元组、期望与实测。
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
                    h ^= (c.spec.op as u64) << 32;
                    h ^= c.spec.sig as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    h ^= c.spec.mode as u64;
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

    /// 重放：非空、用例齐、实测全到、逐例相符。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.registered == 0 {
            return Err(self.fail(SG_EEMPTY));
        }
        if self.registered != CASES {
            return Err(self.fail(SG_ESHORT));
        }
        if self.observed != CASES {
            return Err(self.fail(SG_ESHORT));
        }
        if self.mismatch_count() != 0 {
            return Err(self.fail(SG_EMISMATCH));
        }
        self.freeze()
    }

    /// 封盘：幂等，返回指纹。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        let root = self.fold();
        if self.frozen {
            return Ok(root);
        }
        self.frozen = true;
        Ok(self.fold())
    }
}

/// 第 i 号用例的三元组（构造期使用；末两例为刻意越界样本）。
pub fn spec_of(i: usize) -> Spec {
    if i == CASES - 2 {
        return Spec { op: OP_SIGPROCMASK, sig: 1, mode: MODES };
    }
    if i == CASES - 1 {
        return Spec { op: OP_SIGACTION, sig: SIG_MAX + 1, mode: 0 };
    }
    Spec {
        op: (i % OPS) as u8,
        sig: SIGNUMS[i % SIGNUMS.len()],
        mode: if i % OPS == OP_SIGPROCMASK as usize {
            (i / OPS) as u8
        } else {
            0
        },
    }
}

/// 搭建全矩阵：逐例按规则登记期望，再以同值回填实测，最后重放封盘。
pub fn build_matrix(base: u32) -> (SigMatrix, Result<u64, u32>) {
    let mut m = SigMatrix::new(base);
    let mut i = 0usize;
    while i < CASES {
        let spec = spec_of(i);
        if m.register_ruled(i, spec).is_err() {
            return (m, Err(SG_ECASE));
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
            return (m, Err(SG_ECASE));
        }
        k = k.wrapping_add(1);
    }
    let r = m.replay();
    (m, r)
}

/// 四路注入：每路都在独立实例上验证一类拒止确实生效。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 封盘后登记必被拒。
    let (mut a, ok1) = build_matrix(0x2000);
    out[0] = ok1.is_ok() && a.register(0, spec_of(0), 0).err() == Some(SG_EFROZEN);

    // ② 重复登记必被拒。
    let mut b = SigMatrix::new(0x2001);
    let first = b.register_ruled(0, spec_of(0));
    let twice = b.register_ruled(0, spec_of(0));
    out[1] = first.is_ok() && twice.err() == Some(SG_EDUP);

    // ③ 未登记即观测必被拒。
    let mut c = SigMatrix::new(0x2002);
    out[2] = c.observe(7, 0).err() == Some(SG_EUNREG);

    // ④ 期望与实测不符必被抓。
    let mut d = SigMatrix::new(0x2003);
    // 0 号三元组按规则应落 EINVAL，此处刻意回填 0（成功）以制造不符。
    let mut ok4 = d.register_ruled(0, spec_of(0)).is_ok();
    ok4 = ok4 && d.observe(0, 0).is_ok();
    out[3] = ok4 && d.mismatch_count() == 1;

    out
}

/// 判据集：S1~S13。
pub fn run_unx11143_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11143.b22_signal_errmatrix");

    // S1 常量面：段基址、用例数、族数、信号号轮转表。
    let consts_ok = SEG_BASE == 0x1f2b0
        && CASES == 20
        && OPS == 8
        && OP_NAMES.len() == OPS
        && SIGNUMS.len() == 6
        && SIGNUMS[0] == 0
        && SIGNUMS[5] == SIG_MAX;
    s.add("S1-常量面", consts_ok, "段基址 0x1f2b0 起，20 例 8 族 6 号轮转");

    // S2 空矩阵重放必被拒（空账不得过）。
    let mut empty = SigMatrix::new(0x1001);
    let e2 = empty.replay().err();
    s.add(
        "S2-空矩阵拒止",
        e2 == Some(SG_EEMPTY) && empty.rejects() == 1,
        "空矩阵 replay 落 SG_EEMPTY",
    );

    // S3 半满重放必被拒（用例不齐）。
    let mut half = SigMatrix::new(0x1002);
    let mut ok3 = true;
    let mut i = 0usize;
    while i < 3 {
        ok3 = ok3 && half.register_ruled(i, spec_of(i)).is_ok();
        i = i.wrapping_add(1);
    }
    let e3 = half.replay().err();
    s.add(
        "S3-半满拒止",
        ok3 && e3 == Some(SG_ESHORT),
        "仅 3 例时 replay 落 SG_ESHORT",
    );

    // S4 全矩阵搭建成功并封盘。
    let (m4, r4) = build_matrix(0x1003);
    s.add(
        "S4-搭建封盘",
        r4.is_ok() && m4.frozen(),
        "build_matrix 重放通过并封盘",
    );

    // S5 用例齐。
    s.add_dyn(        "S5-用例齐",
        m4.registered() == CASES,
        &format!("registered={} 期望={}", m4.registered(), CASES),
    );

    // S6 实测齐。
    s.add_dyn(        "S6-实测齐",
        m4.observed() == CASES,
        &format!("observed={} 期望={}", m4.observed(), CASES),
    );

    // S7 零不符、零缺口。
    s.add_dyn(        "S7-闭合",
        m4.mismatch_count() == 0 && m4.gaps() == 0,
        &format!("mismatch={} gaps={}", m4.mismatch_count(), m4.gaps()),
    );

    // S8 登记期望与规则复算逐例一致。
    s.add_dyn(        "S8-规则一致",
        m4.rule_agreement() == CASES,
        &format!("rule_agree={}/{}", m4.rule_agreement(), CASES),
    );

    // S9 族覆盖：八族各至少一例，计数总和等于用例数。
    let t9 = m4.op_table();
    let mut sum9 = 0u32;
    let mut i9 = 0usize;
    while i9 < OPS {
        sum9 = sum9.wrapping_add(t9[i9]);
        i9 = i9.wrapping_add(1);
    }
    s.add_dyn(        "S9-族覆盖",
        m4.ops_covered() && sum9 == CASES as u32,
        &format!("sum={} 族={}", sum9, OPS),
    );

    // S10 信号类覆盖：探活 / 普通 / 不可捕获 / 实时 / 上界。
    s.add(
        "S10-信号类覆盖",
        m4.signal_classes_covered(),
        "五类信号号在册",
    );

    // S11 期望直方：成功、EPERM、EINVAL 三桶皆有，第四桶为零。
    let h11 = m4.expected_histogram();
    let tot11 = h11[0].wrapping_add(h11[1]).wrapping_add(h11[2]);
    s.add_dyn(        "S11-期望直方",
        h11[0] > 0 && h11[1] > 0 && h11[2] > 0 && h11[3] == 0 && tot11 == CASES as u32,
        &format!(
            "ok={} eperm={} einval={} other={}",
            h11[0], h11[1], h11[2], h11[3]
        ),
    );

    // S12 封盘幂等：二次冻结指纹不变。
    let mut m12 = SigMatrix::new(0x1004);
    let ra = m12.freeze();
    let rb = m12.freeze();
    s.add_dyn(        "S12-幂等",
        ra.is_ok() && rb.is_ok() && ra == rb && m12.frozen(),
        &format!("root={:#x}", ra.unwrap_or(0)),
    );

    // S13 四路注入全过，且干净实例拒止计数为零。
    let inj = inject_four();
    let mut all13 = true;
    let mut k13 = 0usize;
    while k13 < 4 {
        all13 = all13 && inj[k13];
        k13 = k13.wrapping_add(1);
    }
    s.add_dyn(        "S13-四注入",
        all13 && m4.rejects() == 0,
        &format!(
            "inj={}/{}/{}/{} rejects={}",
            inj[0], inj[1], inj[2], inj[3], m4.rejects()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11143_flow() {
        let (m, r) = build_matrix(0x1000);
        assert!(r.is_ok());
        assert_eq!(m.registered(), CASES);
        assert_eq!(m.observed(), CASES);
        assert_eq!(m.mismatch_count(), 0);
        assert_eq!(m.gaps(), 0);
        assert_eq!(m.rule_agreement(), CASES);
        assert!(m.frozen());
        assert!(m.ops_covered());
        assert!(m.signal_classes_covered());
        let mut m2 = m.clone();
        let a = m2.freeze();
        let b = m2.freeze();
        assert_eq!(a, b);
    }

    #[test]
    fn f11143_gates() {
        let mut m = SigMatrix::new(0x3000);
        assert!(m.register_ruled(0, spec_of(0)).is_ok());
        // 重复登记
        assert_eq!(m.register_ruled(0, spec_of(0)).err(), Some(SG_EDUP));
        // 号段越界
        assert_eq!(m.register(40, spec_of(1), 0).err(), Some(SG_ECASE));
        // 序倒挂：先到 5，再回头补 3
        assert!(m.register_ruled(5, spec_of(5)).is_ok());
        assert_eq!(m.register_ruled(3, spec_of(3)).err(), Some(SG_EORDER));
        // 期望码越界
        assert_eq!(m.register(6, spec_of(6), 999).err(), Some(SG_ENOEXP));
        // 未登记即观测
        assert_eq!(m.observe(2, 0).err(), Some(SG_EUNREG));
        // 重复观测
        assert!(m.observe(0, 0).is_ok());
        assert_eq!(m.observe(0, 0).err(), Some(SG_EDUP));
        assert_eq!(m.rejects(), 6);
        // 封盘后再动账：登记与观测皆被拒
        assert!(m.freeze().is_ok());
        assert_eq!(m.register_ruled(7, spec_of(7)).err(), Some(SG_EFROZEN));
        assert_eq!(m.observe(5, 0).err(), Some(SG_EFROZEN));
        assert_eq!(m.rejects(), 8);
    }

    #[test]
    fn f11143_rules() {
        // 探活：kill 0 应成功；其余调用遇 0 一律 EINVAL。
        assert_eq!(rule_error(Spec { op: OP_KILL, sig: 0, mode: 0 }), 0);
        assert_eq!(rule_error(Spec { op: OP_SIGACTION, sig: 0, mode: 0 }), EINVAL);
        assert_eq!(rule_error(Spec { op: OP_SIGWAIT, sig: 0, mode: 0 }), EINVAL);
        // 不可捕获 / 不可屏蔽的两个信号。
        assert_eq!(rule_error(Spec { op: OP_SIGACTION, sig: SIGKILL, mode: 0 }), EINVAL);
        assert_eq!(rule_error(Spec { op: OP_SIGNAL, sig: SIGSTOP, mode: 0 }), EINVAL);
        assert_eq!(rule_error(Spec { op: OP_SIGPROCMASK, sig: SIGSTOP, mode: 1 }), EINVAL);
        assert_eq!(rule_error(Spec { op: OP_KILL, sig: SIGKILL, mode: 0 }), EPERM);
        // 挂起集合查询无副作用，任何号都可通过。
        assert_eq!(rule_error(Spec { op: OP_SIGPENDING, sig: SIGKILL, mode: 0 }), 0);
        assert_eq!(rule_error(Spec { op: OP_SIGPENDING, sig: 0, mode: 0 }), 0);
        // 模式位：只有 sigprocmask 读模式，且上界为 SETMASK。
        assert_eq!(rule_error(Spec { op: OP_SIGPROCMASK, sig: 1, mode: MODE_SETMASK }), 0);
        assert_eq!(rule_error(Spec { op: OP_SIGPROCMASK, sig: 1, mode: MODE_SETMASK + 1 }), EINVAL);
        assert_eq!(rule_error(Spec { op: OP_SIGWAIT, sig: 1, mode: 1 }), EINVAL);
        // 信号号上界之外一律非法。
        assert_eq!(rule_error(Spec { op: OP_KILL, sig: SIG_MAX + 1, mode: 0 }), EINVAL);
        // 调用族越界。
        assert_eq!(rule_error(Spec { op: OPS as u8, sig: 1, mode: 0 }), EINVAL);
        assert!(Spec { op: OP_KILL, sig: 0, mode: 0 }.expects_success());
        assert!(!Spec { op: OP_KILL, sig: SIGKILL, mode: 0 }.expects_success());
    }

    #[test]
    fn f11143_matrix_shape() {
        let (m, r) = build_matrix(0x1005);
        assert!(r.is_ok());
        let t = m.op_table();
        let mut i = 0usize;
        let mut sum = 0u32;
        while i < OPS {
            assert!(t[i] >= 1);
            sum = sum.wrapping_add(t[i]);
            i = i.wrapping_add(1);
        }
        assert_eq!(sum, CASES as u32);
        let h = m.expected_histogram();
        assert!(h[0] > 0 && h[1] > 0 && h[2] > 0);
        assert_eq!(h[3], 0);
        assert_eq!(h[0].wrapping_add(h[1]).wrapping_add(h[2]), CASES as u32);
        // 末两例是刻意越界样本，期望均为 EINVAL。
        assert_eq!(m.case(CASES - 2).map(|c| c.expected), Some(EINVAL));
        assert_eq!(m.case(CASES - 1).map(|c| c.expected), Some(EINVAL));
    }

    fn f11143_inject() {
        let inj = inject_four();
        assert_eq!(inj, [true, true, true, true]);
        let (m, r) = build_matrix(0x1006);
        assert!(r.is_ok());
        assert_eq!(m.rejects(), 0);
    }

    fn f11143_checks_green() {
        let cs = run_unx11143_checks();
        assert!(cs.all_passed(), "checks red in unx11143");
    }

    #[test]
    fn f11143_all() {
        f11143_flow();
        f11143_gates();
        f11143_rules();
        f11143_matrix_shape();
        f11143_inject();
        f11143_checks_green();
    }
}
