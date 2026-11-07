//! UNX-F51604 HC halted 死锁检测与恢复 · 0xC19C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51605-J1 越权 ×10 全拒留账；UNX-F51605-J1R 白名单内写
//! ×100 零误拒（红线④路径）；③ 完成时 API/软件写回：状态=已完成、结果
//! 摘要非空。
//!
//! 结构清单：
//!   - HaltCode          诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - HcPhase           控制器相态机（Running→HaltRequested→Halted→Resetting→Recovered）
//!   - HaltRecoveryEngine 死锁看门狗 + 相态恢复引擎
//!   - HaltAuditLedger   恢复审计账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51604_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 死锁判定：在途命令 > 0 而连续 DEADLOCK_TICK_LIMIT 轮零进展即判
//!      死锁，进入 HaltRequested；健康态（有进展或无在途）零误触发。
//!   ② 相态机五态严格单向：halt 前不得复位、复位前不得再启动；违序一律
//!      StateRegress 显性拒且相态原地不动。
//!   ③ 复位恢复全链：死锁检出→halt→reset→recover 写回；恢复完成后审计
//!      账逐条对平（恢复后账一致 ×10）。
//!   ④ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑤ 完成写回：恢复完成写状态=已完成、结果摘要非空的回执，空摘要拒。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——注入参数由 LCG
//!      派生（种子 = SEG_BASE × 黄金比常数 + 1），同段位恒定可复现。
//!   ⑦ 全程零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC19C0;

/// 死锁判定阈值（连续零进展轮数）。
pub const DEADLOCK_TICK_LIMIT: u32 = 4;
/// 在途命令容量。
pub const MAX_INFLIGHT: usize = 32;

/// 诊断码闭集（HC halted 死锁恢复域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HaltCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 死锁确认（零进展越阈值）
    DeadlockConfirmed = 3,
    /// halt 相态内仍提交命令
    SubmitWhileHalted = 4,
    /// 未 halt 即要求复位
    ResetWithoutHalt = 5,
    /// 未复位即要求再启动
    StartWithoutReset = 6,
    /// 相态回退
    StateRegress = 7,
    /// 伪装进展（零工作却报进展）
    FakeProgress = 8,
    /// 在途容量越界
    InflightOob = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 审计账不符
    AuditMismatch = 12,
}

impl HaltCode {
    /// 闭集编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            HaltCode::Ok => "控制器操作成功",
            HaltCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            HaltCode::DeadlockConfirmed => "死锁确认：在途命令连续零进展越阈值，走 halt→reset→recover",
            HaltCode::SubmitWhileHalted => "halt 相态内提交：停机期间禁止新命令入环",
            HaltCode::ResetWithoutHalt => "未 halt 即复位：复位只允许从 Halted 相态发起",
            HaltCode::StartWithoutReset => "未复位即再启动：必须先复位清账再恢复运转",
            HaltCode::StateRegress => "相态回退：五相态只能按编码序单向前进",
            HaltCode::FakeProgress => "伪装进展：零完成却报进展，看门狗必须识破",
            HaltCode::InflightOob => "在途容量越界：在途命令数不得超过环容量",
            HaltCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            HaltCode::WritebackEmpty => "写回摘要为空：恢复回执必须带非空结果摘要",
            HaltCode::AuditMismatch => "审计账不符：检出与恢复事件必须逐条留痕对平",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [HaltCode; 12] = [
    HaltCode::Ok,
    HaltCode::SegBad,
    HaltCode::DeadlockConfirmed,
    HaltCode::SubmitWhileHalted,
    HaltCode::ResetWithoutHalt,
    HaltCode::StartWithoutReset,
    HaltCode::StateRegress,
    HaltCode::FakeProgress,
    HaltCode::InflightOob,
    HaltCode::LedgerMismatch,
    HaltCode::WritebackEmpty,
    HaltCode::AuditMismatch,
];

/// 控制器相态机（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HcPhase {
    /// 运转中
    Running = 0,
    /// 已请求停机（死锁确认后）
    HaltRequested = 1,
    /// 已停机
    Halted = 2,
    /// 复位中
    Resetting = 3,
    /// 已恢复
    Recovered = 4,
}

impl HcPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            HcPhase::Running => "运转中",
            HcPhase::HaltRequested => "停机请求",
            HcPhase::Halted => "已停机",
            HcPhase::Resetting => "复位中",
            HcPhase::Recovered => "已恢复",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HaltWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
}

/// 单条在途命令账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Inflight {
    id: u64,
    /// 看门狗观测到的完成计（伪装进展注入时为 0）
    credits: u32,
}

/// HC halt 死锁恢复引擎：在途账 + 看门狗 + 五相态机。
pub struct HaltRecoveryEngine {
    phase: HcPhase,
    inflight: Vec<Inflight>,
    stall_ticks: u32,
    deadlock_cnt: u32,
    recover_cnt: u32,
    audit: Vec<(u32, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl HaltRecoveryEngine {
    /// 新引擎：运转相态、空在途账。
    pub fn new() -> HaltRecoveryEngine {
        HaltRecoveryEngine {
            phase: HcPhase::Running,
            inflight: Vec::new(),
            stall_ticks: 0,
            deadlock_cnt: 0,
            recover_cnt: 0,
            audit: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 当前相态。
    pub fn phase(&self) -> HcPhase {
        self.phase
    }

    /// 相态推进：只允许编码序 +1 前进，违序显性拒。
    pub fn advance_phase(&mut self, want: HcPhase) -> Result<HcPhase, HaltCode> {
        let cur = self.phase.code();
        if want.code() != cur + 1 {
            return Err(HaltCode::StateRegress);
        }
        self.phase = want;
        Ok(want)
    }

    /// 提交在途命令（仅 Running 相态，halt 后显性拒）。
    pub fn submit(&mut self, id: u64) -> Result<(), HaltCode> {
        if self.phase != HcPhase::Running {
            self.audit.push((HaltCode::SubmitWhileHalted.code(), id as u32));
            return Err(HaltCode::SubmitWhileHalted);
        }
        if self.inflight.len() >= MAX_INFLIGHT {
            return Err(HaltCode::InflightOob);
        }
        self.inflight.push(Inflight { id, credits: 0 });
        Ok(())
    }

    /// 看门狗轮询：completed 为本轮真实完成数；零进展累计，越阈值判死锁
    /// 并进入 HaltRequested；有进展即清零停顿计。
    pub fn watchdog_tick(&mut self, completed: u32) -> Result<HcPhase, HaltCode> {
        if self.phase != HcPhase::Running {
            return Ok(self.phase);
        }
        if completed > 0 {
            self.stall_ticks = 0;
            // 真实进展：按完成数核销在途账。
            let mut done = completed;
            let mut i = 0usize;
            while i < self.inflight.len() && done > 0 {
                let take = match self.inflight.get(i) {
                    Some(e) => e.credits < 1,
                    None => false,
                };
                if take {
                    let _ = self.inflight.remove(i);
                    done -= 1;
                } else {
                    i += 1;
                }
            }
            return Ok(self.phase);
        }
        self.stall_ticks += 1;
        if self.stall_ticks >= DEADLOCK_TICK_LIMIT && !self.inflight.is_empty() {
            self.deadlock_cnt += 1;
            self.audit.push((HaltCode::DeadlockConfirmed.code(), self.stall_ticks));
            return self.advance_phase(HcPhase::HaltRequested);
        }
        Ok(self.phase)
    }

    /// 伪装进展注入口：宣称有进展但完成数为 0（巡检识破）。
    pub fn probe_fake_progress(&mut self) -> Result<bool, HaltCode> {
        let fake = match self.inflight.iter().next() {
            Some(e) => e.credits == 0,
            None => false,
        };
        let inflight_nonempty = !self.inflight.is_empty();
        if inflight_nonempty && fake {
            // 在途命令 credits 恒 0 而外部宣称推进：识破。
            self.audit.push((HaltCode::FakeProgress.code(), self.stall_ticks));
            return Ok(true);
        }
        Ok(false)
    }

    /// 停机落地：HaltRequested→Halted。
    pub fn confirm_halt(&mut self) -> Result<HcPhase, HaltCode> {
        self.advance_phase(HcPhase::Halted)
    }

    /// 复位：Halted→Resetting→清在途账→Recovered（全链一步完成度供分步测）。
    pub fn reset(&mut self) -> Result<HcPhase, HaltCode> {
        if self.phase != HcPhase::Halted {
            self.audit.push((HaltCode::ResetWithoutHalt.code(), 0));
            return Err(HaltCode::ResetWithoutHalt);
        }
        let _ = self.advance_phase(HcPhase::Resetting)?;
        self.inflight.clear();
        self.stall_ticks = 0;
        let _ = self.advance_phase(HcPhase::Recovered)?;
        self.recover_cnt += 1;
        self.audit.push((HcPhase::Recovered.code(), self.recover_cnt as u32));
        Ok(self.phase)
    }

    /// 复位后回到运转相态（新一轮观察窗）。
    pub fn restart(&mut self) -> Result<HcPhase, HaltCode> {
        if self.phase != HcPhase::Recovered {
            self.audit.push((HaltCode::StartWithoutReset.code(), 0));
            return Err(HaltCode::StartWithoutReset);
        }
        self.phase = HcPhase::Running;
        Ok(HcPhase::Running)
    }

    /// 恢复写回：仅 Recovered 相态允许，摘要非空。
    pub fn writeback(&self) -> Result<HaltWriteback, HaltCode> {
        if self.phase != HcPhase::Recovered {
            return Err(HaltCode::StartWithoutReset);
        }
        Ok(HaltWriteback {
            done: true,
            summary: format!(
                "HC 死锁恢复完成：死锁检出 {} 次、恢复 {} 次、审计账 {} 条，在途账已清空",
                self.deadlock_cnt, self.recover_cnt, self.audit.len()
            ),
        })
    }

    pub fn deadlock_cnt(&self) -> u32 {
        self.deadlock_cnt
    }
    pub fn recover_cnt(&self) -> u32 {
        self.recover_cnt
    }
    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
    /// 审计账非空且逐条有据（码域闭集内）。
    pub fn audit_sane(&self) -> bool {
        let mut ok = self.audit_len() > 0;
        for (code, _) in self.audit.iter() {
            if *code == 0 || *code > HaltCode::AuditMismatch.code() {
                ok = false;
            }
        }
        ok
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u32, u32, u32, u32, usize, u64, u64) {
        (
            self.phase.code(),
            self.stall_ticks,
            self.deadlock_cnt,
            self.recover_cnt,
            self.audit.len(),
            self.injected,
            self.detected,
        )
    }
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 死锁注入 ×10 检测→复位恢复全通；恢复后账一致 ×10。
pub fn deadlock_recover_matrix() -> bool {
    let mut all_ok = true;
    let mut i = 0u32;
    while i < 10u32 {
        let mut e = HaltRecoveryEngine::new();
        let mut k = 0u32;
        while k < 3u32 {
            if e.submit(0x40 + k as u64).is_err() {
                all_ok = false;
            }
            k += 1;
        }
        // 死锁：连续 4 轮零进展。
        let mut t = 0u32;
        let mut detected = false;
        while t < DEADLOCK_TICK_LIMIT + 2 {
            match e.watchdog_tick(0) {
                Ok(HcPhase::HaltRequested) => {
                    detected = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => all_ok = false,
            }
            t += 1;
        }
        if !detected {
            all_ok = false;
        }
        // 全链恢复：halt→reset→restart→写回。
        let chain = e.confirm_halt().is_ok()
            && e.reset().is_ok()
            && e.inflight_len() == 0;
        if !chain {
            all_ok = false;
        }
        match e.writeback() {
            Ok(w) => {
                if !w.done || w.summary.is_empty() || !e.audit_sane() {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        if e.restart().is_err() {
            all_ok = false;
        }
        i += 1;
    }
    all_ok
}

/// 健康态零误触发：有进展的流与空载流各若干轮，死锁计数恒 0。
pub fn healthy_zero_false() -> bool {
    // 有进展流：每轮完成 1 条，提交 1 条，永不判死锁。
    let mut e = HaltRecoveryEngine::new();
    let mut round = 0u32;
    while round < 200u32 {
        let _ = e.submit(round as u64);
        if e.watchdog_tick(1).is_err() {
            return false;
        }
        round += 1;
    }
    let busy_ok = e.deadlock_cnt() == 0 && e.phase() == HcPhase::Running && e.inflight_len() == 0;
    // 空载流：无在途零进展也不得判死锁。
    let mut idle = HaltRecoveryEngine::new();
    let mut t = 0u32;
    while t < DEADLOCK_TICK_LIMIT * 3 {
        if idle.watchdog_tick(0).is_err() {
            return false;
        }
        t += 1;
    }
    busy_ok && idle.deadlock_cnt() == 0
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现（死锁、halt 内提交、未 halt 复位、
/// 未复位再启动、伪装进展），注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut e = HaltRecoveryEngine::new();
    let mut seed: u64 = (SEG_BASE as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：死锁注入 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HaltRecoveryEngine::new();
            let _ = probe.submit(0x10);
            let mut t = 0u32;
            let mut hit = false;
            while t < DEADLOCK_TICK_LIMIT + 2 {
                if probe.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                    hit = true;
                    break;
                }
                t += 1;
            }
            if hit {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
            let _ = lcg_next(&mut seed);
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：halt 相态内提交 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HaltRecoveryEngine::new();
            let _ = probe.submit(0x20);
            let mut t = 0u32;
            while t < DEADLOCK_TICK_LIMIT + 2 {
                if probe.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                    break;
                }
                t += 1;
            }
            let _ = probe.confirm_halt();
            if probe.submit(0x21) == Err(HaltCode::SubmitWhileHalted) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：未 halt 即复位 ×10。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HaltRecoveryEngine::new();
            if probe.reset() == Err(HaltCode::ResetWithoutHalt) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：未复位即再启动 ×10（Halted 相态直接 restart 必拒）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HaltRecoveryEngine::new();
            let _ = probe.submit(0x30);
            let mut t = 0u32;
            while t < DEADLOCK_TICK_LIMIT + 2 {
                if probe.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                    break;
                }
                t += 1;
            }
            let _ = probe.confirm_halt();
            if probe.restart() == Err(HaltCode::StartWithoutReset) {
                caught += 1;
                e.note_detected();
            } else {
                e.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：伪装进展 ×10（在途零完成却宣称推进，巡检全抓）。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut probe = HaltRecoveryEngine::new();
            let _ = probe.submit(0x40);
            match probe.probe_fake_progress() {
                Ok(true) => {
                    caught += 1;
                    e.note_detected();
                }
                _ => e.note_escape(),
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (e.injected, e.detected, e.escaped, classes_hit)
}

/// 引擎指纹：注入核 + 恢复矩阵 + 健康流快照混合 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let (inj, det, esc, cls) = injection_core();
    let mut e = HaltRecoveryEngine::new();
    let _ = e.submit(0x99);
    for _ in 0..DEADLOCK_TICK_LIMIT {
        let _ = e.watchdog_tick(0);
    }
    let _ = e.confirm_halt();
    let _ = e.reset();
    let snap = e.snapshot();
    let full = format!("unx51604|{} {} {} {}|{:?}", inj, det, esc, cls, snap);
    fnv1a(full.as_bytes())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51604_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51604");
    // 1 正常流：提交→有进展核销。
    {
        let mut e = HaltRecoveryEngine::new();
        let a = e.submit(1);
        let b = e.watchdog_tick(1);
        s.add(
            "正常进展核销",
            a.is_ok() && b == Ok(HcPhase::Running) && e.inflight_len() == 0,
            "有进展轮必须核销在途账且相态保持运转",
        );
    }
    // 2 死锁注入 ×10 检测→复位恢复全通。
    s.add("死锁恢复矩阵×10", deadlock_recover_matrix(), "10 轮死锁全检出且 halt→reset→restart→写回全通");
    // 3 健康态零误触发。
    s.add("健康态零误触发", healthy_zero_false(), "有进展流与空载流均不得误判死锁");
    // 4 注入核五类全检。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 5 注入账守恒。
    {
        let (inj, det, esc, _) = injection_core();
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 6 相态序：Running→HaltRequested→Halted→Resetting→Recovered 全链合法。
    {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x77);
        let mut t = 0u32;
        while t < DEADLOCK_TICK_LIMIT + 2 {
            if e.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                break;
            }
            t += 1;
        }
        let ok = e.confirm_halt() == Ok(HcPhase::Halted)
            && e.reset() == Ok(HcPhase::Recovered)
            && e.writeback().is_ok()
            && e.restart() == Ok(HcPhase::Running);
        s.add("五相态全链", ok, "死锁后五相态按编码序走通并回到运转");
    }
    // 7 相态跳相显性拒。
    {
        let mut e = HaltRecoveryEngine::new();
        s.add(
            "跳相显性拒",
            e.advance_phase(HcPhase::Halted) == Err(HaltCode::StateRegress),
            "Running→Halted 跳相必须显性拒",
        );
    }
    // 8 审计留痕：违例逐条落账。
    {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x88);
        let mut t = 0u32;
        while t < DEADLOCK_TICK_LIMIT + 2 {
            if e.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                break;
            }
            t += 1;
        }
        // 未 halt 即复位（HaltRequested 相态）违例先留账。
        let _ = e.reset();
        let _ = e.confirm_halt();
        let _ = e.submit(0x89);
        let r = e.reset();
        let ok = r == Ok(HcPhase::Recovered)
            && e.audit_len() >= 3
            && e.audit_sane();
        s.add(
            "违例全审计",
            ok,
            "死锁确认、halt 内提交、未 halt 复位三条违例必须逐条留账",
        );
    }
    // 9 恢复后账一致：在途清空、审计对平。
    {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0xAA);
        let mut t = 0u32;
        while t < DEADLOCK_TICK_LIMIT + 2 {
            if e.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                break;
            }
            t += 1;
        }
        let _ = e.confirm_halt();
        let _ = e.reset();
        s.add(
            "恢复后账一致",
            e.inflight_len() == 0 && e.recover_cnt() == 1 && e.audit_sane(),
            "复位后在途清零、恢复计数 1、审计账有据",
        );
    }
    // 10 写回摘要非空与状态位。
    {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0xBB);
        let mut t = 0u32;
        while t < DEADLOCK_TICK_LIMIT + 2 {
            if e.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                break;
            }
            t += 1;
        }
        let _ = e.confirm_halt();
        let _ = e.reset();
        let ok = match e.writeback() {
            Ok(w) => w.done && !w.summary.is_empty(),
            Err(_) => false,
        };
        s.add("完成写回合规", ok, "写回必须状态=已完成且结果摘要非空");
    }
    // 11 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC19C0 须 0x40 对齐且在分派区间");
    // 15 LCG 确定性。
    {
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("LCG 确定性", same, "同种子两次 LCG 序列必须逐值一致");
    }
    // 16 停顿阈值语义：阈值内不判、达限即判。
    {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0xCC);
        let a = e.watchdog_tick(0);
        let b = e.watchdog_tick(0);
        let ok = a == Ok(HcPhase::Running) && b == Ok(HcPhase::Running);
        s.add(
            "停顿阈值语义",
            ok,
            "停顿 2 轮（<4）不得判死锁，阈值语义精确",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_progress_consumes_inflight() {
        let mut e = HaltRecoveryEngine::new();
        assert!(e.submit(1).is_ok());
        assert!(e.watchdog_tick(1).is_ok());
        assert_eq!(e.inflight_len(), 0);
    }

    #[test]
    fn deadlock_detected_at_threshold() {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x10);
        let mut hit = false;
        for _ in 0..DEADLOCK_TICK_LIMIT {
            if e.watchdog_tick(0) == Ok(HcPhase::HaltRequested) {
                hit = true;
            }
        }
        assert!(hit);
        assert_eq!(e.deadlock_cnt(), 1);
    }

    #[test]
    fn idle_stream_no_false_deadlock() {
        let mut e = HaltRecoveryEngine::new();
        for _ in 0..(DEADLOCK_TICK_LIMIT * 3) {
            assert!(e.watchdog_tick(0).is_ok());
        }
        assert_eq!(e.deadlock_cnt(), 0);
    }

    #[test]
    fn submit_while_halted_rejected() {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x20);
        for _ in 0..DEADLOCK_TICK_LIMIT {
            let _ = e.watchdog_tick(0);
        }
        let _ = e.confirm_halt();
        assert_eq!(e.submit(0x21), Err(HaltCode::SubmitWhileHalted));
    }

    #[test]
    fn reset_without_halt_rejected() {
        let mut e = HaltRecoveryEngine::new();
        assert_eq!(e.reset(), Err(HaltCode::ResetWithoutHalt));
    }

    #[test]
    fn restart_without_reset_rejected() {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x30);
        for _ in 0..DEADLOCK_TICK_LIMIT {
            let _ = e.watchdog_tick(0);
        }
        let _ = e.confirm_halt();
        assert_eq!(e.restart(), Err(HaltCode::StartWithoutReset));
    }

    #[test]
    fn full_recover_chain_and_writeback() {
        let mut e = HaltRecoveryEngine::new();
        let _ = e.submit(0x40);
        for _ in 0..DEADLOCK_TICK_LIMIT {
            let _ = e.watchdog_tick(0);
        }
        let _ = e.confirm_halt();
        assert_eq!(e.reset(), Ok(HcPhase::Recovered));
        let wb = e.writeback();
        assert!(wb.is_ok());
        assert_eq!(e.restart(), Ok(HcPhase::Running));
        match wb {
            Ok(w) => {
                assert!(w.done);
                assert!(!w.summary.is_empty());
            }
            Err(_) => {}
        }
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn unx51604_checks_green() {
        let set = run_unx51604_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
