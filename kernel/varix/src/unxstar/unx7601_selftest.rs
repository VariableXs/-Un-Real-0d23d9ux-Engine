//! UNX-F7601 · SMART self-test 三型（offline/short/long）与状态机（idle/running/completed）· 诊断段 0xAF00
//! 验收锚点：三型自检落账——offline（离线，时长编码 0x00）、short（短测，0x01）、
//! long（长测，0x02）；测试状态机 idle→running→completed 合法迁移三边 + abort 边
//! （running→idle，记中止）；已完成测试留结果码，禁止 running 中重复启动。


use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xAF00;

/// 三型编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestKind {
    Offline = 0x00,
    Short = 0x01,
    Long = 0x02,
}

impl TestKind {
    pub fn code(self) -> u8 {
        self as u8
    }
    /// 约定轮数（演示负载）：offline 8 / short 16 / long 32。
    pub fn laps(self) -> u32 {
        match self {
            TestKind::Offline => 8,
            TestKind::Short => 16,
            TestKind::Long => 32,
        }
    }
    pub fn from_code(c: u8) -> Option<TestKind> {
        match c {
            0x00 => Some(TestKind::Offline),
            0x01 => Some(TestKind::Short),
            0x02 => Some(TestKind::Long),
            _ => None,
        }
    }
}

/// 状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestState {
    Idle,
    Running,
    Completed,
}

/// 完成结果码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestResult {
    Pass,
    WarnSector(u16),
    HardFail(u16),
    Aborted,
}

/// 自检控制器。
pub struct SelfTest {
    pub state: TestState,
    pub kind: Option<TestKind>,
    pub lap: u32,
    pub last: Option<TestResult>,
    pub aborted_total: u32,
    pub completed_total: u32,
}

impl SelfTest {
    pub fn new() -> Self {
        SelfTest { state: TestState::Idle, kind: None, lap: 0, last: None, aborted_total: 0, completed_total: 0 }
    }

    /// 启动：仅 Idle 可启动；Running 中重复启动拒绝。
    pub fn start(&mut self, k: TestKind) -> bool {
        if self.state != TestState::Idle {
            return false;
        }
        self.state = TestState::Running;
        self.kind = Some(k);
        self.lap = 0;
        true
    }

    /// 推进一轮；轮满进入 Completed 并出结果码。
    pub fn tick(&mut self, bad_at: Option<u32>) -> Option<TestResult> {
        if self.state != TestState::Running {
            return None;
        }
        let need = self.kind.map(|k| k.laps()).unwrap_or(0);
        self.lap += 1;
        if Some(self.lap) == bad_at {
            self.state = TestState::Completed;
            self.completed_total += 1;
            let r = TestResult::WarnSector(self.lap as u16);
            self.last = Some(r);
            return Some(r);
        }
        if self.lap >= need {
            self.state = TestState::Completed;
            self.completed_total += 1;
            let r = TestResult::Pass;
            self.last = Some(r);
            return Some(r);
        }
        None
    }

    /// 中止：仅 Running 可中止，回到 Idle 记 Aborted。
    pub fn abort(&mut self) -> bool {
        if self.state != TestState::Running {
            return false;
        }
        self.state = TestState::Idle;
        self.kind = None;
        self.lap = 0;
        self.aborted_total += 1;
        self.last = Some(TestResult::Aborted);
        true
    }

    /// 复位到 Idle（Completed → Idle，允许下一轮）。
    pub fn reset(&mut self) -> bool {
        if self.state != TestState::Completed {
            return false;
        }
        self.state = TestState::Idle;
        self.kind = None;
        self.lap = 0;
        true
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xAF00 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x7601_AF00;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("三型-编码回读", TestKind::from_code(TestKind::Long.code()) == Some(TestKind::Long)));
    out.push(("三型-非法码拒绝", TestKind::from_code(0x03).is_none()));
    out.push(("状态机-Idle中止拒绝", !SelfTest::new().abort()));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx7601_checks：本单判据聚合（诊断段 0xAF00）。
pub fn run_unx7601_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F7601 SMART self-test 三型与状态机");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 三型时长编码
    let codes_ok = TestKind::Offline.code() == 0x00 && TestKind::Short.code() == 0x01 && TestKind::Long.code() == 0x02;
    s.add("三型-时长编码0/1/2", codes_ok, if codes_ok { "" } else { "编码漂移" });
    let laps_ok = TestKind::Offline.laps() == 8 && TestKind::Short.laps() == 16 && TestKind::Long.laps() == 32;
    s.add("三型-约定轮数8/16/32", laps_ok, if laps_ok { "" } else { "轮数漂移" });

    // 状态机：idle→running→completed 三边（long 完整走完）
    let mut t = SelfTest::new();
    let ok_start = t.start(TestKind::Long);
    s.add("状态机-Idle启动Running", ok_start, if ok_start { "" } else { "启动失败" });
    let ok_dup = !t.start(TestKind::Short);
    s.add("状态机-Running重复启动拒绝", ok_dup, if ok_dup { "" } else { "重复启动未拒" });
    let mut result: Option<TestResult> = None;
    for _ in 0..32 {
        result = t.tick(None);
    }
    let ok_done = t.state == TestState::Completed && result == Some(TestResult::Pass);
    s.add("状态机-32轮满Completed出Pass", ok_done, if ok_done { "" } else { "完成路径漂移" });

    // 中止边：running→idle 记 Aborted
    let mut t2 = SelfTest::new();
    t2.start(TestKind::Short);
    t2.tick(None);
    let ok_abort = t2.abort();
    s.add("状态机-Running中止回Idle", ok_abort, if ok_abort { "" } else { "中止失败" });
    let ok_abort_rec = t2.last == Some(TestResult::Aborted) && t2.aborted_total == 1;
    s.add("状态机-中止留Aborted账", ok_abort_rec, if ok_abort_rec { "" } else { "中止账缺失" });

    // Completed→Idle 复位后再启动
    let ok_reset = t.reset();
    s.add("状态机-Completed复位Idle", ok_reset, if ok_reset { "" } else { "复位失败" });
    let ok_restart = t.start(TestKind::Offline);
    s.add("状态机-复位后可再启动", ok_restart, if ok_restart { "" } else { "再启动失败" });

    // 坏扇区路径：short 第 9 轮遇坏 → WarnSector(9)
    let mut t3 = SelfTest::new();
    t3.start(TestKind::Short);
    let mut r3: Option<TestResult> = None;
    for _ in 0..9 {
        r3 = t3.tick(Some(9));
    }
    let ok_warn = r3 == Some(TestResult::WarnSector(9)) && t3.state == TestState::Completed;
    s.add("坏扇区-第9轮WarnSector(9)停测", ok_warn, if ok_warn { "" } else { "坏扇区路径漂移" });

    // Idle 下 tick 无效
    let mut t4 = SelfTest::new();
    s.add("状态机-Idle下tick无效", t4.tick(None).is_none(), if t4.tick(None).is_none() { "" } else { "Idle tick 泄漏" });

    // 40 轮随机型混跑（LCG 决定型与中止概率）
    let mut seed: u64 = 0x7601_0040;
    let mut passes = 0u32;
    let mut aborts = 0u32;
    for i in 0..40u32 {
        let mut t5 = SelfTest::new();
        let k = match lcg_next(&mut seed) % 3 {
            0 => TestKind::Offline,
            1 => TestKind::Short,
            _ => TestKind::Long,
        };
        t5.start(k);
        let mut done = false;
        for _ in 0..k.laps() {
            if t5.tick(None).is_some() {
                done = true;
                break;
            }
            if i % 5 == 4 && t5.lap == 1 {
                t5.abort();
                break;
            }
        }
        if done && t5.state == TestState::Completed {
            passes += 1;
        } else if t5.aborted_total == 1 {
            aborts += 1;
        }
    }
    let ok_mix = passes + aborts == 40;
    s.add("混跑-40轮全程有账(pass+abort=40)", ok_mix, if ok_mix { "" } else { "混跑账目缺口" });
    let ok_abort_n = aborts == 8;
    s.add("混跑-中止恰8轮", ok_abort_n, if ok_abort_n { "" } else { "中止计数漂移" });

    // FNV 结果码稳定
    let h1 = fnv1a(b"short warn @9");
    let h2 = fnv1a(b"short warn @9");
    s.add("FNV-结果码哈希稳定", h1 == h2 && h1 != 0, if h1 == h2 && h1 != 0 { "" } else { "哈希不稳" });

    s
}
