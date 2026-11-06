//! UNX-F9232 fd 复用竞态注入（close-then-use TOCTOU） · 0x7A780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9232 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9232-J1 close-then-use 竞态
//!   注入判据过，槽位复用窗口可观测格过，100 次注入零越权写格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① close-then-use 的正确防线是「入口捕获 (fd, 代际, 对象) 三元组，提交时
//!      复验代际」；只复验 fd 号不验代际，等于把复用槽当成原对象，越权写就
//!      从这条缝里发生。
//!   ② 漏洞策略与守卫策略必须同脚本双跑：守卫零越权 + 漏洞基线必泄漏，才能
//!      证明测试本身有杀伤力，而不是恒零的空断言。
//!   ③ 槽位复用窗口的可观测定义：close 后代际 +1、下一次 open 命中最低空槽、
//!      fd 号不变、代际严格增加、槽内对象变成注入者指定的新对象。三者缺一
//!      都不算「可观测」。
//!   ④ 100 发 = 5 种攻击形态 × 20 轮；轮次改变填充槽数（fd 号漂移）与代际
//!      churn，防止实现把 fd 3 写死；每种形态都要两种策略各跑一遍。
//!   ⑤ 双重 close 是状态机探针：第二次 close 必须 EBADF 且不得改动槽位；
//!      double reuse 让代际连跳两次，旧句柄仍必须失效。
//!   ⑥ 「关闭另一个 fd」不得误伤原句柄：守卫策略在 CloseOther 形态仍成功写
//!      Public，证明代际复验只针对目标槽，不会把无关槽位的变化算成冲突。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x7A780;

pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;
pub const EMFILE: i32 = 24;

pub const FIRST_FD: i32 = 3;
pub const MAX_FDS: usize = 4;

/// 审计码闭集：前四码对齐 errno，Leak 是审计专用负值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    Ok,
    Ebadf,
    Emfile,
    Einval,
    Leak,
}

impl RaceCode {
    pub const ALL: [RaceCode; 5] = [
        RaceCode::Ok,
        RaceCode::Ebadf,
        RaceCode::Emfile,
        RaceCode::Einval,
        RaceCode::Leak,
    ];

    pub fn val(self) -> i32 {
        match self {
            RaceCode::Ok => 0,
            RaceCode::Ebadf => EBADF,
            RaceCode::Emfile => EMFILE,
            RaceCode::Einval => EINVAL,
            RaceCode::Leak => -13,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RaceCode::Ok => "OK",
            RaceCode::Ebadf => "EBADF",
            RaceCode::Emfile => "EMFILE",
            RaceCode::Einval => "EINVAL",
            RaceCode::Leak => "LEAK",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::Ok => "提交时三元组仍一致，允许落到原对象",
            RaceCode::Ebadf => "槽位已关或代际已变，按坏句柄拒绝提交",
            RaceCode::Emfile => "槽位耗尽，先拒绝新打开再谈复用",
            RaceCode::Einval => "参数或形态非法，修复注入脚本而非吞错",
            RaceCode::Leak => "旧句柄落到新对象，越权写成立，必须回滚审计",
        }
    }
}

/// 对象种类：只有 Public 是调用者被授权写的目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Public,
    Secret,
    Dir,
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Target::Public => "public",
            Target::Secret => "secret",
            Target::Dir => "dir",
        }
    }
}

/// 入口捕获的三元组：fd 号 + 代际 + 当时对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    pub fd: i32,
    pub gen: u16,
    pub target: Target,
}

#[derive(Clone, Copy)]
struct Slot {
    target: Target,
    gen: u16,
    open: bool,
}

/// 槽位表：只允许 Free→Open→Free→(复用)Open 的合法迁移。
pub struct FdTable {
    slots: [Slot; MAX_FDS],
    count: usize,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable {
            slots: [Slot { target: Target::Public, gen: 0, open: false }; MAX_FDS],
            count: 0,
        }
    }

    fn index(&self, fd: i32) -> Option<usize> {
        if fd < FIRST_FD {
            return None;
        }
        let off = (fd - FIRST_FD) as usize;
        if off >= MAX_FDS {
            return None;
        }
        Some(off)
    }

    /// 命中最底层空槽；若该槽曾被用过，代际已由 close 抬升。
    pub fn open(&mut self, target: Target) -> Result<(i32, u16), i32> {
        let mut i = 0usize;
        while i < MAX_FDS {
            let free = match self.slots.get(i) {
                Some(slot) => !slot.open,
                None => false,
            };
            if free {
                let pair = match self.slots.get_mut(i) {
                    Some(slot) => {
                        slot.open = true;
                        slot.target = target;
                        (FIRST_FD + i as i32, slot.gen)
                    }
                    None => return Err(EMFILE),
                };
                self.count = self.count.saturating_add(1);
                return Ok(pair);
            }
            i += 1;
        }
        Err(EMFILE)
    }

    /// 关闭成功才抬代际；对已关槽再关一次必须 EBADF 且零副作用。
    pub fn close(&mut self, fd: i32) -> Result<u16, i32> {
        let idx = match self.index(fd) {
            Some(v) => v,
            None => return Err(EBADF),
        };
        let gen = match self.slots.get_mut(idx) {
            Some(slot) => {
                if !slot.open {
                    return Err(EBADF);
                }
                slot.open = false;
                slot.gen = slot.gen.wrapping_add(1);
                slot.gen
            }
            None => return Err(EBADF),
        };
        self.count = self.count.saturating_sub(1);
        Ok(gen)
    }

    pub fn describe(&self, fd: i32) -> Option<Handle> {
        let idx = self.index(fd)?;
        let slot = self.slots.get(idx)?;
        if !slot.open {
            return None;
        }
        Some(Handle { fd, gen: slot.gen, target: slot.target })
    }

    pub fn gen(&self, fd: i32) -> Option<u16> {
        match self.describe(fd) {
            Some(h) => Some(h.gen),
            None => None,
        }
    }

    pub fn open_count(&self) -> usize {
        self.count
    }
}

/// 提交策略：守卫 = 复验代际；漏洞 = 只按 fd 号现查现用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    Guarded,
    Vulnerable,
}

impl Policy {
    pub fn label(self) -> &'static str {
        match self {
            Policy::Guarded => "guarded",
            Policy::Vulnerable => "vulnerable",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Pending {
    pub handle: Handle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub code: i32,
    pub landed: Option<Target>,
    pub stale: bool,
}

/// 提交期复验：守卫策略以「代际一致」为唯一放行条件。
pub fn commit(policy: Policy, t: &FdTable, pending: Pending) -> Outcome {
    match policy {
        Policy::Guarded => match t.describe(pending.handle.fd) {
            Some(now) if now.gen == pending.handle.gen => Outcome {
                code: 0,
                landed: Some(now.target),
                stale: false,
            },
            _ => Outcome { code: EBADF, landed: None, stale: true },
        },
        Policy::Vulnerable => match t.describe(pending.handle.fd) {
            Some(now) => Outcome {
                code: 0,
                landed: Some(now.target),
                stale: now.gen != pending.handle.gen,
            },
            None => Outcome { code: EBADF, landed: None, stale: false },
        },
    }
}

/// 五种攻击形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pattern {
    CloseReopenSecret,
    CloseReopenSame,
    DoubleCloseSecret,
    DoubleReuseSecret,
    CloseOther,
}

pub const PATTERNS: [Pattern; 5] = [
    Pattern::CloseReopenSecret,
    Pattern::CloseReopenSame,
    Pattern::DoubleCloseSecret,
    Pattern::DoubleReuseSecret,
    Pattern::CloseOther,
];

pub const ROUNDS: u8 = 20;

impl Pattern {
    pub fn label(self) -> &'static str {
        match self {
            Pattern::CloseReopenSecret => "关闭后复用为secret",
            Pattern::CloseReopenSame => "关闭后复用同目标",
            Pattern::DoubleCloseSecret => "双重close后复用",
            Pattern::DoubleReuseSecret => "连续两次复用",
            Pattern::CloseOther => "关闭相邻fd",
        }
    }

    /// 该形态在提交前是否动过目标槽（CloseOther 动的是相邻槽）。
    pub fn touches_target(self) -> bool {
        match self {
            Pattern::CloseOther => false,
            _ => true,
        }
    }

    /// 复用次数（决定代际应增加多少）。
    pub fn reuse_times(self) -> u16 {
        match self {
            Pattern::DoubleReuseSecret => 2,
            _ => 1,
        }
    }
}

/// 单次注入结果（含窗口观测三元组）。
#[derive(Clone, Copy)]
pub struct Case {
    pub pattern: Pattern,
    pub round: u8,
    pub policy: Policy,
    pub code: i32,
    pub landed: Option<Target>,
    pub unauthorized: bool,
    pub stale: bool,
    pub window_fd: i32,
    pub window_target: Option<Target>,
    pub gen_before: u16,
    pub gen_after: u16,
    pub window_ok: bool,
    pub setup_ok: bool,
}

fn failed_case(pattern: Pattern, round: u8, policy: Policy) -> Case {
    Case {
        pattern,
        round,
        policy,
        code: EINVAL,
        landed: None,
        unauthorized: false,
        stale: false,
        window_fd: -1,
        window_target: None,
        gen_before: 0,
        gen_after: 0,
        window_ok: false,
        setup_ok: false,
    }
}

/// 执行一条攻击脚本：填充槽 → 代际 churn → 入口捕获 → 竞态步 → 提交。
pub fn run_case(policy: Policy, pattern: Pattern, round: u8) -> Case {
    let mut t = FdTable::new();

    let fillers = (round % 3) as usize;
    let mut i = 0usize;
    while i < fillers {
        let _ = t.open(Target::Dir);
        i += 1;
    }

    let fd = match t.open(Target::Public) {
        Ok(pair) => pair.0,
        Err(_) => return failed_case(pattern, round, policy),
    };

    let churn = (round % 4) as usize;
    let mut c = 0usize;
    while c < churn {
        let _ = t.close(fd);
        let _ = t.open(Target::Public);
        c += 1;
    }

    let handle = match t.describe(fd) {
        Some(h) => h,
        None => return failed_case(pattern, round, policy),
    };
    let intended = handle.target;
    let pending = Pending { handle };

    let other = match pattern {
        Pattern::CloseOther => match t.open(Target::Dir) {
            Ok(pair) => Some(pair.0),
            Err(_) => None,
        },
        _ => None,
    };

    let mut window_fd = fd;
    let mut gen_before = match t.gen(fd) {
        Some(g) => g,
        None => 0,
    };

    match pattern {
        Pattern::CloseReopenSecret => {
            let _ = t.close(fd);
            let _ = t.open(Target::Secret);
        }
        Pattern::CloseReopenSame => {
            let _ = t.close(fd);
            let _ = t.open(Target::Public);
        }
        Pattern::DoubleCloseSecret => {
            let _ = t.close(fd);
            let _ = t.close(fd);
            let _ = t.open(Target::Secret);
        }
        Pattern::DoubleReuseSecret => {
            let _ = t.close(fd);
            let _ = t.open(Target::Secret);
            let _ = t.close(fd);
            let _ = t.open(Target::Secret);
        }
        Pattern::CloseOther => match other {
            Some(ofd) => {
                window_fd = ofd;
                gen_before = match t.gen(ofd) {
                    Some(g) => g,
                    None => 0,
                };
                let _ = t.close(ofd);
                let _ = t.open(Target::Secret);
            }
            None => return failed_case(pattern, round, policy),
        },
    }

    let expected_target = match pattern {
        Pattern::CloseReopenSame => Target::Public,
        _ => Target::Secret,
    };
    let observed = t.describe(window_fd);
    let gen_after = match observed {
        Some(h) => h.gen,
        None => 0,
    };
    let window_ok = match observed {
        Some(h) => {
            h.gen == gen_after
                && h.target == expected_target
                && gen_after == gen_before.wrapping_add(pattern.reuse_times())
                && h.fd == window_fd
        }
        None => false,
    };

    let outcome = commit(policy, &t, pending);
    let unauthorized = match outcome.landed {
        Some(landed) => landed != intended,
        None => false,
    };

    Case {
        pattern,
        round,
        policy,
        code: outcome.code,
        landed: outcome.landed,
        unauthorized,
        stale: outcome.stale,
        window_fd,
        window_target: Some(expected_target),
        gen_before,
        gen_after,
        window_ok,
        setup_ok: true,
    }
}

/// 逐形态人工期望（守卫码/落点、漏洞落点、是否越权）。
#[derive(Clone, Copy)]
pub struct PatternSpec {
    pub pattern: Pattern,
    pub guard_code: RaceCode,
    pub guard_landed: Option<Target>,
    pub vuln_landed: Option<Target>,
    pub unauthorized: bool,
}

pub const SPECS: [PatternSpec; 5] = [
    PatternSpec { pattern: Pattern::CloseReopenSecret, guard_code: RaceCode::Ebadf, guard_landed: None, vuln_landed: Some(Target::Secret), unauthorized: true },
    PatternSpec { pattern: Pattern::CloseReopenSame, guard_code: RaceCode::Ebadf, guard_landed: None, vuln_landed: Some(Target::Public), unauthorized: false },
    PatternSpec { pattern: Pattern::DoubleCloseSecret, guard_code: RaceCode::Ebadf, guard_landed: None, vuln_landed: Some(Target::Secret), unauthorized: true },
    PatternSpec { pattern: Pattern::DoubleReuseSecret, guard_code: RaceCode::Ebadf, guard_landed: None, vuln_landed: Some(Target::Secret), unauthorized: true },
    PatternSpec { pattern: Pattern::CloseOther, guard_code: RaceCode::Ok, guard_landed: Some(Target::Public), vuln_landed: Some(Target::Public), unauthorized: false },
];

/// 竞态账本：守卫/漏洞双跑分桶，窗口与状态机违例显性计数。
#[derive(Clone, Copy)]
pub struct RaceLedger {
    pub shots: usize,
    pub guarded_writes: usize,
    pub guarded_stale: usize,
    pub guarded_other: usize,
    pub guarded_unauthorized: usize,
    pub vulnerable_writes: usize,
    pub vulnerable_unauthorized: usize,
    pub windows: usize,
    pub window_violations: usize,
    pub setup_failures: usize,
    pub stale_same_target: usize,
}

impl RaceLedger {
    pub fn conserved(&self) -> bool {
        self.shots > 0
            && self.guarded_writes + self.guarded_stale + self.guarded_other == self.shots
            && self.window_violations == 0
            && self.setup_failures == 0
            && self.guarded_unauthorized == 0
    }
}

/// 100 发注入：5 形态 × 20 轮，每发守卫与漏洞各跑一遍。
pub fn run_injection() -> RaceLedger {
    let mut l = RaceLedger {
        shots: 0,
        guarded_writes: 0,
        guarded_stale: 0,
        guarded_other: 0,
        guarded_unauthorized: 0,
        vulnerable_writes: 0,
        vulnerable_unauthorized: 0,
        windows: 0,
        window_violations: 0,
        setup_failures: 0,
        stale_same_target: 0,
    };

    for pat in PATTERNS.iter().copied() {
        let mut round = 0u8;
        while round < ROUNDS {
            let g = run_case(Policy::Guarded, pat, round);
            l.shots += 1;
            if !g.setup_ok {
                l.setup_failures += 1;
            }
            match g.code {
                0 => l.guarded_writes += 1,
                EBADF => l.guarded_stale += 1,
                _ => l.guarded_other += 1,
            }
            if g.unauthorized {
                l.guarded_unauthorized += 1;
            }
            if g.stale && g.pattern == Pattern::CloseReopenSame {
                l.stale_same_target += 1;
            }

            let v = run_case(Policy::Vulnerable, pat, round);
            if v.landed.is_some() {
                l.vulnerable_writes += 1;
            }
            if v.unauthorized {
                l.vulnerable_unauthorized += 1;
            }

            if g.window_ok {
                l.windows += 1;
            } else {
                l.window_violations += 1;
            }

            round += 1;
        }
    }
    l
}

/// 状态机探针：Open→Close→Reopen，旧句柄必须失效。
#[derive(Clone, Copy)]
pub struct Life {
    pub open_ok: bool,
    pub close_ok: bool,
    pub close_again_bad: bool,
    pub reuse_same_fd: bool,
    pub gen_advanced: bool,
    pub stale_blocked: bool,
}

pub fn lifecycle_probe() -> Life {
    let mut t = FdTable::new();
    let (fd, gen0) = match t.open(Target::Public) {
        Ok(pair) => pair,
        Err(_) => {
            return Life {
                open_ok: false,
                close_ok: false,
                close_again_bad: false,
                reuse_same_fd: false,
                gen_advanced: false,
                stale_blocked: false,
            }
        }
    };
    let old = match t.describe(fd) {
        Some(h) => h,
        None => {
            return Life {
                open_ok: false,
                close_ok: false,
                close_again_bad: false,
                reuse_same_fd: false,
                gen_advanced: false,
                stale_blocked: false,
            }
        }
    };
    let close_ok = t.close(fd).is_ok();
    let closed_gone = t.describe(fd).is_none();
    let close_again_bad = t.close(fd) == Err(EBADF) && t.describe(fd).is_none();
    let (fd2, gen1) = match t.open(Target::Secret) {
        Ok(pair) => pair,
        Err(_) => (fd, 0),
    };
    let stale_blocked = commit(Policy::Guarded, &t, Pending { handle: old }).code == EBADF;
    Life {
        open_ok: old.gen == gen0 && old.target == Target::Public,
        close_ok: close_ok && closed_gone,
        close_again_bad,
        reuse_same_fd: fd2 == fd,
        gen_advanced: gen1 == gen0.wrapping_add(1),
        stale_blocked,
    }
}

/// 代际回绕探针：65535 关闭后回绕到 0，旧句柄仍不得放行。
pub fn wrap_probe() -> bool {
    let mut t = FdTable::new();
    let fd = match t.open(Target::Public) {
        Ok(pair) => pair.0,
        Err(_) => return false,
    };
    if let Some(slot) = t.slots.get_mut(0) {
        slot.gen = u16::MAX;
    }
    let old = match t.describe(fd) {
        Some(h) => h,
        None => return false,
    };
    let _ = t.close(fd);
    let (fd2, gen2) = match t.open(Target::Secret) {
        Ok(pair) => pair,
        Err(_) => return false,
    };
    fd2 == fd
        && gen2 == 0
        && commit(Policy::Guarded, &t, Pending { handle: old }).code == EBADF
        && commit(Policy::Guarded, &t, Pending { handle: Handle { fd: fd2, gen: gen2, target: Target::Secret } })
            .code
            == 0
}

pub fn fnv1a(mut h: u32, bytes: &[u8]) -> u32 {
    for b in bytes.iter().copied() {
        h ^= b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    h
}

pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0usize;
    while i < SPECS.len() {
        if let Some(spec) = SPECS.get(i) {
            h = fnv1a(h, spec.pattern.label().as_bytes());
            h = fnv1a(h, &[spec.guard_code.val() as u8]);
            h = fnv1a(h, &[spec.unauthorized as u8]);
        }
        i += 1;
    }
    let l = run_injection();
    let buckets = [
        l.shots as u8,
        l.guarded_writes as u8,
        l.guarded_stale as u8,
        l.vulnerable_unauthorized as u8,
        l.windows as u8,
    ];
    h = fnv1a(h, &buckets);
    h
}

/// 本卡自检入口（≥12 条，动态明细 ≤8 条）。
pub fn run_unx9232_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9232");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add(
        "段位守卫：0x40 对齐且落在 0x7A000..=0x7D1C0",
        seg_ok,
        "段基址唯一且不与他卡重叠，否则拒绝注册",
    );

    let mut code_ok = true;
    let mut i = 0usize;
    while i < RaceCode::ALL.len() {
        if let Some(c) = RaceCode::ALL.get(i) {
            if c.detail().is_empty() || c.label().is_empty() {
                code_ok = false;
            }
        }
        i += 1;
    }
    code_ok = code_ok
        && RaceCode::Ok.val() == 0
        && RaceCode::Ebadf.val() == EBADF
        && RaceCode::Emfile.val() == EMFILE
        && RaceCode::Einval.val() == EINVAL
        && RaceCode::Leak.val() < 0;
    s.add(
        "审计码闭集：5 码标签/detail 非空且 errno 对齐",
        code_ok,
        "Leak 用负值区分「不是系统调用错误，而是越权审计事件」",
    );

    let t = FdTable::new();
    let normal = run_case(Policy::Guarded, Pattern::CloseOther, 0);
    let no_race = commit(
        Policy::Guarded,
        &t,
        Pending { handle: Handle { fd: FIRST_FD, gen: 0, target: Target::Public } },
    );
    let normal_ok = no_race.code == EBADF
        && normal.setup_ok
        && normal.code == 0
        && normal.landed == Some(Target::Public);
    s.add(
        "正常流：无竞态提交落 Public，空表提交按 EBADF",
        normal_ok,
        "先有干净路径，后面的竞态才有对照基线",
    );

    let race = run_case(Policy::Guarded, Pattern::CloseReopenSecret, 0);
    let race_ok = race.code == EBADF
        && race.landed.is_none()
        && race.stale
        && !race.unauthorized
        && race.gen_after == race.gen_before + 1;
    s.add(
        "close-then-use：守卫策略在代际变更后拒绝提交",
        race_ok,
        "拒绝必须是 EBADF 且零落点；只把落点改回 public 也算越权未遂",
    );

    let same = run_case(Policy::Guarded, Pattern::CloseReopenSame, 0);
    s.add(
        "同目标复用仍拒绝：防线是代际不是对象名",
        same.code == EBADF && same.stale && same.landed.is_none(),
        "复用回同名 Public 也必须 EBADF，否则防线可被同名对象绕过",
    );

    let vuln = run_case(Policy::Vulnerable, Pattern::CloseReopenSecret, 0);
    let vuln_ok = vuln.code == 0 && vuln.landed == Some(Target::Secret) && vuln.unauthorized;
    s.add(
        "漏洞基线：同脚本下仅查 fd 号必落 secret（证明注入有杀伤力）",
        vuln_ok,
        "基线恒零说明脚本没打中缝；这种绿灯是假绿",
    );

    let mut specs_ok = SPECS.len() == 5;
    let mut k = 0usize;
    while k < SPECS.len() {
        if let Some(spec) = SPECS.get(k) {
            let g = run_case(Policy::Guarded, spec.pattern, 0);
            let v = run_case(Policy::Vulnerable, spec.pattern, 0);
            if g.code != spec.guard_code.val()
                || g.landed != spec.guard_landed
                || v.landed != spec.vuln_landed
                || v.unauthorized != spec.unauthorized
            {
                specs_ok = false;
            }
        }
        k += 1;
    }
    s.add(
        "形态对账：5 种攻击形态的守卫码/落点/越权位逐行一致",
        specs_ok,
        "期望表人工写死，禁止按实现结果反填",
    );

    let l = run_injection();
    s.add(
        "100 次注入零越权写：守卫策略 unauthorized 恒 0",
        l.shots == 100 && l.guarded_unauthorized == 0 && l.setup_failures == 0,
        "100 = 5 形态 × 20 轮；每轮守卫与漏洞双跑",
    );

    s.add(
        "槽位复用窗口可观测：fd 不变、代际递增、对象换新（100 窗）",
        l.windows == 100 && l.window_violations == 0,
        "窗口三元组任一不成立即不可观测，复用就退化成黑盒",
    );

    let life = lifecycle_probe();
    s.add(
        "状态机：Open→Close→Reopen 合法迁移全绿，二次 close 无副作用",
        life.open_ok
            && life.close_ok
            && life.close_again_bad
            && life.reuse_same_fd
            && life.gen_advanced
            && life.stale_blocked,
        "Free/Open 两态四迁移；非法迁移必须 EBADF 且不改槽",
    );

    s.add(
        "守卫分桶守恒：100 = 20 成功写 + 80 代际拒绝",
        l.guarded_writes == 20 && l.guarded_stale == 80 && l.conserved(),
        "CloseOther 形态 20 发成功，其余 80 发全被代际拦下",
    );

    s.add(
        "漏洞基线分桶：100 发全落点、其中 60 发越权",
        l.vulnerable_writes == 100 && l.vulnerable_unauthorized == 60,
        "三种 secret 形态 ×20；若基线不泄漏，注入器本身失效",
    );

    let mut cap = FdTable::new();
    let mut filled = true;
    for _ in 0..MAX_FDS {
        if cap.open(Target::Dir).is_err() {
            filled = false;
        }
    }
    let cap_ok = filled
        && cap.open(Target::Dir).is_err()
        && cap.open_count() == MAX_FDS
        && cap.describe(FIRST_FD).is_some()
        && cap.close(FIRST_FD).is_ok()
        && cap.describe(FIRST_FD).is_none()
        && cap.close(FIRST_FD) == Err(EBADF);
    s.add(
        "容量与关闭：4 槽满 EMFILE，关闭幂等面 EBADF",
        cap_ok,
        "槽位上限是复用窗口的物理边界，越界必须先拒绝",
    );

    s.add(
        "代际回绕：65535 关闭回绕 0，旧句柄与旧新代际均被区分",
        wrap_probe(),
        "wrapping_add 不得退化成饱和或 panic；回绕点也是 TOCTOU 考点",
    );

    s.add(
        "双跑一致：语料指纹两次相等且同轮结果逐字段一致",
        corpus_fingerprint() == corpus_fingerprint()
            && run_case(Policy::Guarded, Pattern::DoubleReuseSecret, 7).gen_after
                == run_case(Policy::Guarded, Pattern::DoubleReuseSecret, 7).gen_after,
        "隐藏状态会让同轮结果漂移；指纹是确定性的守门员",
    );

    s.add(
        "相邻 fd 不误伤：CloseOther 形态守卫仍写 Public 且零拒绝",
        l.guarded_writes == 20
            && !Pattern::CloseOther.touches_target()
            && run_case(Policy::Guarded, Pattern::CloseOther, 3).code == 0
            && !run_case(Policy::Guarded, Pattern::CloseOther, 3).stale,
        "代际复验必须绑定目标槽，不能把无关槽位变化算成冲突",
    );

    let grid_text = format!(
        "100发 守卫写{}/拒绝{}/越权{}/漏洞越权{}/窗口{}",
        l.guarded_writes, l.guarded_stale, l.guarded_unauthorized,
        l.vulnerable_unauthorized, l.windows
    );
    s.add_dyn("竞态账快照", l.conserved() && l.shots == 100, &grid_text);

    let window_text = format!(
        "复用窗 好{}/坏{}/同目标拒绝{}",
        l.windows, l.window_violations, l.stale_same_target
    );
    s.add_dyn(
        "复用窗快照",
        l.windows == 100 && l.window_violations == 0 && l.stale_same_target == 20,
        &window_text,
    );

    let fp = corpus_fingerprint();
    let fp_text = format!("语料指纹 0x{:08X}", fp);
    s.add_dyn("指纹快照", fp == corpus_fingerprint(), &fp_text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_base_guard() {
        assert_eq!(SEG_BASE, 0x7A780);
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x7A000..=0x7D1C0).contains(&SEG_BASE));
    }

    #[test]
    fn code_closed_set() {
        assert_eq!(RaceCode::ALL.len(), 5);
        for c in RaceCode::ALL.iter().copied() {
            assert!(!c.label().is_empty());
            assert!(!c.detail().is_empty());
        }
        assert_eq!(RaceCode::Ebadf.val(), EBADF);
        assert_eq!(RaceCode::Leak.val(), -13);
    }

    #[test]
    fn normal_flow() {
        let c = run_case(Policy::Guarded, Pattern::CloseOther, 2);
        assert!(c.setup_ok);
        assert_eq!(c.code, 0);
        assert_eq!(c.landed, Some(Target::Public));
        assert!(!c.unauthorized);
        assert!(!c.stale);
        assert!(c.window_ok);
        assert_eq!(c.pattern, Pattern::CloseOther);
        assert!(!Pattern::CloseOther.touches_target());
        assert!(Pattern::CloseReopenSecret.touches_target());
        assert_eq!(Policy::Guarded.label(), "guarded");
        assert_eq!(Target::Public.label(), "public");
        assert_eq!(Target::Secret.label(), "secret");
    }

    #[test]
    fn close_then_use_blocked() {
        let c = run_case(Policy::Guarded, Pattern::CloseReopenSecret, 0);
        assert_eq!(c.code, EBADF);
        assert!(c.stale);
        assert_eq!(c.landed, None);
        assert!(!c.unauthorized);
        assert_eq!(c.gen_after, c.gen_before + 1);
        assert_eq!(c.window_target, Some(Target::Secret));
    }

    #[test]
    fn same_target_still_stale() {
        let c = run_case(Policy::Guarded, Pattern::CloseReopenSame, 0);
        assert_eq!(c.code, EBADF);
        assert!(c.stale);
        assert_eq!(c.window_target, Some(Target::Public));
        assert!(!c.unauthorized);
    }

    #[test]
    fn vulnerable_baseline() {
        let c = run_case(Policy::Vulnerable, Pattern::CloseReopenSecret, 0);
        assert_eq!(c.code, 0);
        assert_eq!(c.landed, Some(Target::Secret));
        assert!(c.unauthorized);
        assert_eq!(Policy::Vulnerable.label(), "vulnerable");
    }

    #[test]
    fn pattern_specs() {
        for spec in SPECS.iter() {
            let g = run_case(Policy::Guarded, spec.pattern, 0);
            let v = run_case(Policy::Vulnerable, spec.pattern, 0);
            assert_eq!(g.code, spec.guard_code.val(), "{:?}", spec.pattern);
            assert_eq!(g.landed, spec.guard_landed, "{:?}", spec.pattern);
            assert_eq!(v.landed, spec.vuln_landed, "{:?}", spec.pattern);
            assert_eq!(v.unauthorized, spec.unauthorized, "{:?}", spec.pattern);
            assert!(!spec.pattern.label().is_empty());
        }
    }

    #[test]
    fn window_observable() {
        let l = run_injection();
        assert_eq!(l.windows, 100);
        assert_eq!(l.window_violations, 0);
        let c = run_case(Policy::Guarded, Pattern::DoubleReuseSecret, 5);
        assert!(c.window_ok);
        assert_eq!(c.gen_after, c.gen_before + 2);
    }

    #[test]
    fn capacity_and_close() {
        let mut t = FdTable::new();
        for _ in 0..MAX_FDS {
            assert!(t.open(Target::Dir).is_ok());
        }
        assert_eq!(t.open(Target::Dir), Err(EMFILE));
        assert_eq!(t.open_count(), MAX_FDS);
        assert!(t.close(FIRST_FD).is_ok());
        assert_eq!(t.close(FIRST_FD), Err(EBADF));
        assert_eq!(t.describe(FIRST_FD), None);
        assert_eq!(t.describe(MAX_FDS as i32 + 100), None);
    }

    #[test]
    fn lifecycle_transitions() {
        let life = lifecycle_probe();
        assert!(life.open_ok);
        assert!(life.close_ok);
        assert!(life.close_again_bad);
        assert!(life.reuse_same_fd);
        assert!(life.gen_advanced);
        assert!(life.stale_blocked);
    }

    #[test]
    fn generation_wrap() {
        assert!(wrap_probe());
    }

    #[test]
    fn ledger_partition() {
        let l = run_injection();
        assert_eq!(l.shots, 100);
        assert_eq!(l.guarded_writes, 20);
        assert_eq!(l.guarded_stale, 80);
        assert_eq!(l.guarded_unauthorized, 0);
        assert_eq!(l.vulnerable_writes, 100);
        assert_eq!(l.vulnerable_unauthorized, 60);
        assert_eq!(l.setup_failures, 0);
        assert!(l.conserved());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(
            run_case(Policy::Guarded, Pattern::CloseReopenSecret, 9).gen_after,
            run_case(Policy::Guarded, Pattern::CloseReopenSecret, 9).gen_after
        );
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9232_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 12);
        assert!(!s.truncated());
    }
}
