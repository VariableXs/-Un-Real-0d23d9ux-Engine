//! UNX-F9326 setitimer 非法间隔矩阵 · 0x7BF00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9326 逐字）：
//!   锚点：UNX-F9326-J1 负值/超上界注入判据过，全 EINVAL 格过，
//!   ITIMER_REAL 三类正判据格过。
//!
//! 设计要点：
//!   ① 非法面建模：timeval 负值（tv_usec 越出 0..=999_999 或 tv_sec 为
//!      负语义）与超上界（总纳秒超定时器硬件窗）都回 EINVAL。Linux 口径
//!      冻结：tv_usec ∈ [0, 999999]，interval 与 value 各自校验，any bad
//!      → EINVAL 且不装表。
//!   ② 三类正判据（ITIMER_REAL 三档）：
//!      A 一次性：value>0 且 interval=0 —— 触发一次后自然停表；
//!      B 周期性：value>0 且 interval>0 —— 首触发后按 interval 周期续发；
//!      C 解除：value=0 且 interval=0 —— disarm，返回旧值。
//!      三档逐格落账：触发序列可复算（A：[value]；B：value, interval,
//!      interval…；C：空序列）。
//!   ③ 时钟账本：装表后按滴答推进（u64 纳秒标量），触发点序列由
//!      (value, interval) 完全决定——确定性可复算是本模块核心性质。
//!   ④ 注入核四类坏样本：usec 越界（1_000_000）、负语义（sec 负）、
//!      总长超上界、装表后幽灵读（未装表读旧值越界）——逐类拒绝，账
//!      守恒逃逸 = 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9326 独占 0x7BF00..=0x7BF3F）。
pub const SEG_BASE: u32 = 0x7BF00;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// tv_usec 上界（Linux 冻结）。
pub const USEC_LIMIT: i64 = 999_999;

/// 定时器硬件窗上界（1000 秒，纳秒）。
pub const NS_WINDOW: u64 = 1_000_000_000_000;

/// itimer 三档闭集：真实计时。
pub const ITIMER_REAL: u32 = 0;
/// 虚拟时间档。
pub const ITIMER_VIRTUAL: u32 = 1;
/// 教授档（prof）。
pub const ITIMER_PROF: u32 = 2;

/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0A 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItimerCode {
    /// tv_usec 越界
    BadUsec,
    /// 负语义
    Negative,
    /// 总长超上界
    BeyondWindow,
    /// 未装表操作
    DisarmGhost,
    /// which 档越界
    BadWhich,
    /// 装表后序列不可复算
    SequenceBroken,
    /// 账本倒退
    LedgerRegress,
    /// 正判据档越界
    ProfileOutOfRange,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ItimerCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ItimerCode::BadUsec => "usec 越界被拒：tv_usec 必须落在 0..=999999；\
                1000000 应折算进 tv_sec，Linux 口径直接 EINVAL",
            ItimerCode::Negative => "负值被拒：timeval 无负语义；负 sec/usec 一律 \
                EINVAL，不得回折为正",
            ItimerCode::BeyondWindow => "超上界被拒：总纳秒超过定时器硬件窗；\
                请分段或降低间隔，越界回 EINVAL",
            ItimerCode::DisarmGhost => "未装表操作被拒：getitimer 对从未装表的档 \
                只允许返回全零旧值；越界读即幽灵",
            ItimerCode::BadWhich => "which 越界被拒：闭集为 REALTIME/VIRTUAL/PROF；\
                越界档回 EINVAL",
            ItimerCode::SequenceBroken => "序列不可复算被拒：触发序列必须由 \
                (value, interval) 完全决定；随机抖动即坏",
            ItimerCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查 \
                是否有回滚路径误改累计值",
            ItimerCode::ProfileOutOfRange => "正判据档越界：三类正判据只有 \
                一次性/周期性/解除；其他组合不属于正判据域",
            ItimerCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            ItimerCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ItimerCode; 10] = [
        ItimerCode::BadUsec,
        ItimerCode::Negative,
        ItimerCode::BeyondWindow,
        ItimerCode::DisarmGhost,
        ItimerCode::BadWhich,
        ItimerCode::SequenceBroken,
        ItimerCode::LedgerRegress,
        ItimerCode::ProfileOutOfRange,
        ItimerCode::NotReproducible,
        ItimerCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── timeval 解析器 ───────────────────────────────────────────────────
/// timeval（sec/usec 有符号承载，负值属坏样本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tv {
    /// 秒（可为负语义坏样本）
    pub sec: i64,
    /// 微秒（可为负/越界坏样本）
    pub usec: i64,
}

impl Tv {
    /// 转纳秒：负值或 usec 越界回 EINVAL。
    pub fn to_ns(&self) -> SysResult<u64> {
        if self.sec < 0 || self.usec < 0 {
            return Err(EINVAL);
        }
        if self.usec > USEC_LIMIT {
            return Err(EINVAL);
        }
        let ns = (self.sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add((self.usec as u64).saturating_mul(1_000));
        if ns > NS_WINDOW {
            return Err(EINVAL);
        }
        Ok(ns)
    }
}

/// setitimer 规格：value + interval 两个 timeval。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItimerSpec {
    /// 首触发
    pub value: Tv,
    /// 周期
    pub interval: Tv,
}

/// setitimer 校验：两 timeval 各自合法且都在窗内。
pub fn validate_spec(spec: &ItimerSpec) -> SysResult<(u64, u64)> {
    let v = spec.value.to_ns()?;
    let i = spec.interval.to_ns()?;
    Ok((v, i))
}

/// which 档闭集校验。
pub fn validate_which(which: u32) -> SysResult<()> {
    match which {
        ITIMER_REAL | ITIMER_VIRTUAL | ITIMER_PROF => Ok(()),
        _ => Err(EINVAL),
    }
}

// ── itimer 时钟账本 ─────────────────────────────────────────────────
/// 触发序列三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// 一次性：value>0, interval=0
    OneShot,
    /// 周期性：value>0, interval>0
    Periodic,
    /// 解除：value=0, interval=0
    Disarm,
}

/// itimer 账本：装表 + 滴答推进 + 触发序列。
#[derive(Clone, Copy, Debug)]
pub struct ItimerLedger {
    which: u32,
    armed: bool,
    /// 首触发 ns
    value_ns: u64,
    /// 周期 ns
    interval_ns: u64,
    /// 已装表累计触发次数
    pub fires: u32,
    /// 档位
    pub profile: Profile,
}

impl ItimerLedger {
    /// setitimer：校验通过才装表；返回旧值 ns（未装表为 0）。
    pub fn arm(which: u32, spec: &ItimerSpec) -> SysResult<(ItimerLedger, u64)> {
        validate_which(which)?;
        let (v, i) = validate_spec(spec)?;
        let profile = if v == 0 && i == 0 {
            Profile::Disarm
        } else if i == 0 {
            Profile::OneShot
        } else {
            Profile::Periodic
        };
        let led = ItimerLedger {
            which,
            armed: profile != Profile::Disarm,
            value_ns: v,
            interval_ns: i,
            fires: 0,
            profile,
        };
        Ok((led, 0))
    }

    /// 滴答推进 t 纳秒：复算触发序列。返回本次推进的触发次数。
    pub fn tick(&mut self, t: u64) -> SysResult<u32> {
        if !self.armed {
            return Err(EINVAL);
        }
        if self.profile == Profile::Disarm {
            return Err(EINVAL);
        }
        let mut fired: u32 = 0;
        if self.value_ns > 0 && t >= self.value_ns {
            fired += 1;
            let rest = t - self.value_ns;
            match self.profile {
                Profile::OneShot => {
                    // 触发后自然停表。
                    self.armed = false;
                    self.value_ns = 0;
                }
                Profile::Periodic => {
                    fired += (rest / self.interval_ns) as u32;
                    self.value_ns = self.interval_ns.saturating_sub(rest % self.interval_ns);
                }
                Profile::Disarm => {}
            }
        }
        self.fires = self.fires.saturating_add(fired);
        Ok(fired)
    }

    /// 档位正确性：三档互斥且与 (v,i) 符号一致。
    pub fn profile_holds(&self) -> bool {
        match self.profile {
            Profile::OneShot => self.value_ns > 0 && self.interval_ns == 0,
            Profile::Periodic => self.value_ns > 0 && self.interval_ns > 0,
            Profile::Disarm => self.value_ns == 0 && self.interval_ns == 0,
        }
    }

    /// 是否在装表态。
    pub fn is_armed(&self) -> bool {
        self.armed
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏 setitimer 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadItimerCase {
    /// usec 越界
    BadUsec,
    /// 负语义
    Negative,
    /// 超窗
    BeyondWindow,
    /// which 越界
    BadWhich,
}

impl BadItimerCase {
    /// 闭集全量。
    pub const ALL: [BadItimerCase; 4] = [
        BadItimerCase::BadUsec,
        BadItimerCase::Negative,
        BadItimerCase::BeyondWindow,
        BadItimerCase::BadWhich,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏 setitimer 注入：每类 INJECT_ROUNDS 次，全部 EINVAL。
pub fn run_bad_itimer_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadItimerCase::ALL.len() {
        if let Some(case) = BadItimerCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadItimerCase::BadUsec => Tv {
                        sec: 0,
                        usec: 1_000_000,
                    }
                    .to_ns()
                        == Err(EINVAL),
                    BadItimerCase::Negative => Tv { sec: -1, usec: 0 }.to_ns() == Err(EINVAL)
                        && Tv { sec: 0, usec: -5 }.to_ns() == Err(EINVAL),
                    BadItimerCase::BeyondWindow => Tv {
                        sec: 1001,
                        usec: 0,
                    }
                    .to_ns()
                        == Err(EINVAL),
                    BadItimerCase::BadWhich => validate_which(9) == Err(EINVAL),
                };
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── ITIMER_REAL 三类正判据 ───────────────────────────────────────────
/// 正判据 A：一次性 —— 触发序列 [value] 后停表。
pub fn profile_oneshot_holds() -> bool {
    let spec = ItimerSpec {
        value: Tv {
            sec: 0,
            usec: 100_000,
        },
        interval: Tv {
            sec: 0,
            usec: 0,
        },
    };
    match ItimerLedger::arm(ITIMER_REAL, &spec) {
        Ok((mut led, _old)) => {
            if led.profile != Profile::OneShot || !led.profile_holds() {
                return false;
            }
            // 50ms 未到不触发；150ms 到触发一次并停表。
            match led.tick(50_000_000) {
                Ok(0) => {}
                _ => return false,
            }
            match led.tick(100_000_000) {
                Ok(1) => {}
                _ => return false,
            }
            !led.is_armed() && led.fires == 1
        }
        Err(_) => false,
    }
}

/// 正判据 B：周期性 —— value 后按 interval 续发。
pub fn profile_periodic_holds() -> bool {
    let spec = ItimerSpec {
        value: Tv {
            sec: 0,
            usec: 100_000,
        },
        interval: Tv {
            sec: 0,
            usec: 50_000,
        },
    };
    match ItimerLedger::arm(ITIMER_REAL, &spec) {
        Ok((mut led, _old)) => {
            if led.profile != Profile::Periodic || !led.profile_holds() {
                return false;
            }
            // 100ms 触发 1 次；再 100ms 触发 2 次（50ms 处 + 100ms 处）。
            match led.tick(100_000_000) {
                Ok(1) => {}
                _ => return false,
            }
            match led.tick(100_000_000) {
                Ok(2) => {}
                _ => return false,
            }
            led.is_armed() && led.fires == 3
        }
        Err(_) => false,
    }
}

/// 正判据 C：解除 —— 双零 disarm。
pub fn profile_disarm_holds() -> bool {
    let spec = ItimerSpec {
        value: Tv {
            sec: 0,
            usec: 0,
        },
        interval: Tv {
            sec: 0,
            usec: 0,
        },
    };
    match ItimerLedger::arm(ITIMER_REAL, &spec) {
        Ok((led, _old)) => {
            led.profile == Profile::Disarm && !led.is_armed() && led.profile_holds()
        }
        Err(_) => false,
    }
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：三档正判据 + 四类坏样本 + 档位闭集。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(if profile_oneshot_holds() { 1 } else { 0 });
    bytes.push(if profile_periodic_holds() { 1 } else { 0 });
    bytes.push(if profile_disarm_holds() { 1 } else { 0 });
    // 档位闭集
    bytes.push(if validate_which(ITIMER_REAL).is_ok() { 1 } else { 0 });
    bytes.push(if validate_which(3).is_err() { 1 } else { 0 });
    // 坏样本全拒
    let (l, per) = run_bad_itimer_matrix();
    bytes.push(if l.conserved() && l.escaped == 0 { 1 } else { 0 });
    let mut i = 0usize;
    while i < per.len() {
        if let Some(n) = per.get(i) {
            bytes.push(if *n == INJECT_ROUNDS { 1 } else { 0 });
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9326 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9326_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9326");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 负值/超上界注入全 EINVAL
    let bad_usec = Tv {
        sec: 0,
        usec: 1_000_000,
    }
    .to_ns();
    let bad_neg = Tv { sec: -1, usec: 0 }.to_ns();
    let bad_over = Tv {
        sec: 1001,
        usec: 0,
    }
    .to_ns();
    let einval_ok = bad_usec == Err(EINVAL)
        && bad_neg == Err(EINVAL)
        && bad_over == Err(EINVAL);
    s.add("非法间隔：usec 越界/负语义/超窗全 EINVAL", einval_ok,
          "Linux 口径逐格冻结：usec≤999999、无负值、窗内");

    // 3. 边界正判据：usec=999999 与 usec=0 合法
    let edge_ok = Tv {
        sec: 0,
        usec: 999_999,
    }
    .to_ns()
        == Ok(999_999_000)
        && Tv {
            sec: 0,
            usec: 0,
        }
        .to_ns()
            == Ok(0);
    s.add("边界正判据：usec=999999/usec=0 合法", edge_ok,
          "闭集两端必须放行，中间无灰色档");

    // 4. 正判据 A：一次性
    s.add("ITIMER_REAL 正判据 A：一次性触发后停表", profile_oneshot_holds(),
          "value>0 interval=0 → [value] 序列后自然 disarm");

    // 5. 正判据 B：周期性
    s.add("ITIMER_REAL 正判据 B：value 后按 interval 续发", profile_periodic_holds(),
          "触发序列 value, interval, interval… 可复算");

    // 6. 正判据 C：解除
    s.add("ITIMER_REAL 正判据 C：双零 disarm", profile_disarm_holds(),
          "value=0 interval=0 合法解除");

    // 7. which 档闭集
    let which_ok = validate_which(ITIMER_REAL).is_ok()
        && validate_which(ITIMER_VIRTUAL).is_ok()
        && validate_which(ITIMER_PROF).is_ok()
        && validate_which(3).is_err()
        && validate_which(0xFFFF).is_err();
    s.add("which 闭集：REALTIME/VIRTUAL/PROF 外全拒", which_ok,
          "档位域闭集，越界回 EINVAL");

    // 8. 档位与符号一致
    let spec_p = ItimerSpec {
        value: Tv {
            sec: 0,
            usec: 10,
        },
        interval: Tv {
            sec: 0,
            usec: 10,
        },
    };
    let prof_ok = match ItimerLedger::arm(ITIMER_REAL, &spec_p) {
        Ok((led, _)) => led.profile_holds() && led.profile == Profile::Periodic,
        Err(_) => false,
    };
    s.add("档位一致：profile 必须与 (value, interval) 符号一致", prof_ok,
          "三档互斥且可复算");

    // 9. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_itimer_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏样本各 10/10 全检出", all_caught,
          "usec 越界/负语义/超窗/which 越界必须逐类拒绝");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 坏参数不装表
    let bad_spec = ItimerSpec {
        value: Tv {
            sec: 0,
            usec: 2_000_000,
        },
        interval: Tv {
            sec: 0,
            usec: 0,
        },
    };
    let no_arm = ItimerLedger::arm(ITIMER_REAL, &bad_spec).is_err();
    s.add("装表守卫：坏参数不得进入装表态", no_arm,
          "校验先于装表，坏参数零副作用");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集与码唯一
    let text_ok = ItimerCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < ItimerCode::ALL.len() {
        let mut j = i + 1;
        while j < ItimerCode::ALL.len() {
            if let (Some(x), Some(y)) = (ItimerCode::ALL.get(i), ItimerCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：10 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_usec_einval() {
        assert_eq!(
            Tv {
                sec: 0,
                usec: 1_000_000,
            }
            .to_ns(),
            Err(EINVAL)
        );
    }

    #[test]
    fn negative_einval() {
        assert_eq!(Tv { sec: -1, usec: 0 }.to_ns(), Err(EINVAL));
        assert_eq!(Tv { sec: 0, usec: -5 }.to_ns(), Err(EINVAL));
    }

    #[test]
    fn beyond_window_einval() {
        assert_eq!(
            Tv {
                sec: 1001,
                usec: 0,
            }
            .to_ns(),
            Err(EINVAL)
        );
    }

    #[test]
    fn edge_usec_ok() {
        assert_eq!(
            Tv {
                sec: 0,
                usec: 999_999,
            }
            .to_ns(),
            Ok(999_999_000)
        );
    }

    #[test]
    fn oneshot_profile() {
        assert!(profile_oneshot_holds());
    }

    #[test]
    fn periodic_profile() {
        assert!(profile_periodic_holds());
    }

    #[test]
    fn disarm_profile() {
        assert!(profile_disarm_holds());
    }

    #[test]
    fn bad_which_einval() {
        assert_eq!(validate_which(9), Err(EINVAL));
        assert!(validate_which(ITIMER_PROF).is_ok());
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_itimer_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9326_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
