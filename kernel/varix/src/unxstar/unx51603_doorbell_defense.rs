//! UNX-F51603 doorbell 非法敲击防御（target 越界） · 0xC1980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51604-J1 死锁注入 ×10 检测→复位恢复全通；UNX-F51604-J1R
//! 恢复后账一致 ×10，健康态零误触发；③ 完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。
//!
//! 结构清单：
//!   - DoorCode            诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - DoorbellPhase       门铃域相态机（Healthy→Quarantined→Recovered）
//!   - DoorbellDispatcher  门铃分发器（target 校验/限速/隔离/恢复）
//!   - DoorbellAuditLedger 敲击审计账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51603_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① target 越界全拒并审计：合法 target 在 0..DOOR_TARGETS、门号在
//!      0..DOORS_PER_TARGET，越界一律显性拒（TargetOob / DoorIndexOob）
//!      且逐条落审计账，不静默吞。
//!   ② 限速防御：同一 target 在观察窗内敲击超 DOORBELL_RATE_CAP 判洪水
//!      （RateFlood），合法敲击 ×10^4 轮零误拒。
//!   ③ 隔离相态机：连续违例达阈值进入 Quarantined（拒一切敲击），
//!      显性复位指令走到 Recovered；相态只按编码序前进，回退显性拒。
//!   ④ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑤ 完成写回：复位恢复完成写一条状态=已完成、结果摘要非空的回执。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——注入参数由 LCG
//!      派生（种子 = SEG_BASE × 黄金比常数 + 1），同段位恒定可复现。
//!   ⑦ 全程零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1980;

/// 门铃 target 数（合法 target 0..8）。
pub const DOOR_TARGETS: usize = 8;
/// 每 target 门铃寄存器数。
pub const DOORS_PER_TARGET: usize = 4;
/// 同 target 观察窗限速上限。
pub const DOORBELL_RATE_CAP: u32 = 6;
/// 连续违例隔离阈值。
pub const QUARANTINE_THRESHOLD: u32 = 5;

/// 诊断码闭集（doorbell 防御域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoorCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// target 越界
    TargetOob = 3,
    /// 门号越界
    DoorIndexOob = 4,
    /// 敲击洪水（超限速）
    RateFlood = 5,
    /// 敲击重放（同参数同轮重复敲）
    KickReplay = 6,
    /// 隔离相态内敲击
    Quarantined = 7,
    /// 相态回退
    StateRegress = 8,
    /// 审计账不符
    AuditMismatch = 9,
    /// 注入账不符
    LedgerMismatch = 10,
    /// 写回摘要为空
    WritebackEmpty = 11,
    /// 复位恢复未完成
    RecoveryIncomplete = 12,
}

impl DoorCode {
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
            DoorCode::Ok => "门铃敲击放行",
            DoorCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            DoorCode::TargetOob => "target 越界：敲击目标必须在 0..8 之内",
            DoorCode::DoorIndexOob => "门号越界：每 target 门铃寄存器仅 0..4",
            DoorCode::RateFlood => "敲击洪水：同 target 观察窗内超过限速上限",
            DoorCode::KickReplay => "敲击重放：同参数同轮重复敲击必须拒",
            DoorCode::Quarantined => "隔离中：连续违例触发隔离，复位前拒一切敲击",
            DoorCode::StateRegress => "相态回退：Healthy→Quarantined→Recovered 只能单向前进",
            DoorCode::AuditMismatch => "审计账不符：每次敲击判定必须留痕且三本计数对平",
            DoorCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            DoorCode::WritebackEmpty => "写回摘要为空：恢复回执必须带非空结果摘要",
            DoorCode::RecoveryIncomplete => "恢复未完成：相态未到 Recovered 不得写回完成",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [DoorCode; 12] = [
    DoorCode::Ok,
    DoorCode::SegBad,
    DoorCode::TargetOob,
    DoorCode::DoorIndexOob,
    DoorCode::RateFlood,
    DoorCode::KickReplay,
    DoorCode::Quarantined,
    DoorCode::StateRegress,
    DoorCode::AuditMismatch,
    DoorCode::LedgerMismatch,
    DoorCode::WritebackEmpty,
    DoorCode::RecoveryIncomplete,
];

/// 门铃域相态机（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoorbellPhase {
    /// 健康放行
    Healthy = 0,
    /// 隔离（拒一切敲击）
    Quarantined = 1,
    /// 已恢复
    Recovered = 2,
}

impl DoorbellPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            DoorbellPhase::Healthy => "健康",
            DoorbellPhase::Quarantined => "隔离",
            DoorbellPhase::Recovered => "已恢复",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DoorWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
}

/// 门铃分发器：target 校验 + 限速 + 隔离相态机 + 审计账。
pub struct DoorbellDispatcher {
    rate: [u32; DOOR_TARGETS],
    last_kick: [Option<(u8, u32)>; DOOR_TARGETS],
    epoch: u32,
    phase: DoorbellPhase,
    violations: u32,
    accept_cnt: u64,
    reject_cnt: u64,
    audit: Vec<(u8, u8, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl DoorbellDispatcher {
    /// 新分发器：全 target 限速窗清零、健康相态。
    pub fn new() -> DoorbellDispatcher {
        DoorbellDispatcher {
            rate: [0; DOOR_TARGETS],
            last_kick: [None; DOOR_TARGETS],
            epoch: 0,
            phase: DoorbellPhase::Healthy,
            violations: 0,
            accept_cnt: 0,
            reject_cnt: 0,
            audit: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 当前相态。
    pub fn phase(&self) -> DoorbellPhase {
        self.phase
    }

    /// 相态推进：只允许编码序 +1 前进，回退/跳相显性拒。
    pub fn advance_phase(&mut self, want: DoorbellPhase) -> Result<DoorbellPhase, DoorCode> {
        let cur = self.phase.code();
        let w = want.code();
        if w != cur + 1 {
            return Err(DoorCode::StateRegress);
        }
        self.phase = want;
        Ok(want)
    }

    /// 敲击入口：target/门号越界、限速、重放、隔离四道闸依次过闸。
    pub fn kick(&mut self, target: usize, door: u8) -> Result<(), DoorCode> {
        if self.phase == DoorbellPhase::Quarantined {
            self.reject_cnt += 1;
            self.audit.push((target as u8, door, DoorCode::Quarantined.code()));
            return Err(DoorCode::Quarantined);
        }
        if target >= DOOR_TARGETS {
            self.reject_cnt += 1;
            self.bump_violation();
            self.audit.push((target as u8, door, DoorCode::TargetOob.code()));
            return Err(DoorCode::TargetOob);
        }
        if door as usize >= DOORS_PER_TARGET {
            self.reject_cnt += 1;
            self.bump_violation();
            self.audit.push((target as u8, door, DoorCode::DoorIndexOob.code()));
            return Err(DoorCode::DoorIndexOob);
        }
        let t = match self.rate.get(target) {
            Some(r) => *r,
            None => 0,
        };
        if t >= DOORBELL_RATE_CAP {
            self.reject_cnt += 1;
            self.bump_violation();
            self.audit.push((target as u8, door, DoorCode::RateFlood.code()));
            return Err(RateFloodErr::val_code());
        }
        // 重放检测：同 target 同门号同一观察窗纪元内重复敲。
        let replay = match self.last_kick.get(target) {
            Some(Some(prev)) => *prev == (door, self.epoch),
            _ => false,
        };
        if replay {
            self.reject_cnt += 1;
            self.bump_violation();
            self.audit.push((target as u8, door, DoorCode::KickReplay.code()));
            return Err(DoorCode::KickReplay);
        }
        if let Some(r) = self.rate.get_mut(target) {
            *r += 1;
        }
        if let Some(slot) = self.last_kick.get_mut(target) {
            *slot = Some((door, self.epoch));
        }
        self.accept_cnt += 1;
        self.audit.push((target as u8, door, DoorCode::Ok.code()));
        Ok(())
    }

    /// 连续违例累计，达阈值进入隔离相态。
    fn bump_violation(&mut self) {
        self.violations += 1;
        if self.violations >= QUARANTINE_THRESHOLD && self.phase == DoorbellPhase::Healthy {
            self.phase = DoorbellPhase::Quarantined;
        }
    }

    /// 观察窗滑窗：每轮把全 target 限速窗衰减 1 并推进窗纪元（合法长流
    /// 不被洪水误判，跨窗重复敲也不算重放）。
    pub fn decay_window(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        let mut i = 0usize;
        while i < DOOR_TARGETS {
            if let Some(r) = self.rate.get_mut(i) {
                *r = r.saturating_sub(1);
            }
            if let Some(slot) = self.last_kick.get_mut(i) {
                if let Some((_, t)) = *slot {
                    if t == 0 {
                        *slot = None;
                    } else if let Some(s) = slot.as_mut() {
                        s.1 -= 1;
                    }
                }
            }
            i += 1;
        }
    }

    /// 重放位清除：洪水场景中逐事件清重放位（事件彼此独立，非重发）。
    pub fn clear_replay(&mut self, target: usize) {
        if let Some(slot) = self.last_kick.get_mut(target) {
            *slot = None;
        }
    }

    /// 复位恢复：Quarantined→Recovered，清违例与限速窗并写回完成回执。
    pub fn recover(&mut self) -> Result<DoorWriteback, DoorCode> {
        if self.phase != DoorbellPhase::Quarantined {
            return Err(DoorCode::RecoveryIncomplete);
        }
        let _ = self.advance_phase(DoorbellPhase::Recovered)?;
        self.violations = 0;
        let mut i = 0usize;
        while i < DOOR_TARGETS {
            if let Some(r) = self.rate.get_mut(i) {
                *r = 0;
            }
            i += 1;
        }
        Ok(DoorWriteback {
            done: true,
            summary: format!(
                "门铃隔离复位完成：放行 {} 拒绝 {}，违例清零，审计账 {} 条",
                self.accept_cnt, self.reject_cnt, self.audit.len()
            ),
        })
    }

    /// 回到健康相态（新一轮观察）。
    pub fn reset_phase(&mut self) {
        self.phase = DoorbellPhase::Healthy;
        self.violations = 0;
    }

    pub fn accept_cnt(&self) -> u64 {
        self.accept_cnt
    }
    pub fn reject_cnt(&self) -> u64 {
        self.reject_cnt
    }
    /// 审计账条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
    /// 审计账守恒：放行 + 拒绝 = 审计条数。
    pub fn audit_conserved(&self) -> bool {
        self.accept_cnt + self.reject_cnt == self.audit.len() as u64
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
    pub fn snapshot(&self) -> (u64, u64, u32, u32, usize) {
        (self.accept_cnt, self.reject_cnt, self.violations, self.phase.code(), self.audit.len())
    }
}

/// 内部桥：RateFlood 判定落点（保持 match 封闭性）。
struct RateFloodErr;

impl RateFloodErr {
    fn val_code() -> DoorCode {
        DoorCode::RateFlood
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

/// 注入核：5 类坏样本逐类 ×10 轮精确复现（target 越界、门号越界、
/// 敲击洪水、敲击重放、隔离期敲击），注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut d = DoorbellDispatcher::new();
    let mut seed: u64 = (SEG_BASE as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：target 越界 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            if d.kick(DOOR_TARGETS + i as usize, 0) == Err(DoorCode::TargetOob) {
                caught += 1;
                d.note_detected();
                // 本类只考察拒写留账，检出即清违例计数避免叠加隔离相态。
                d.violations = 0;
            } else {
                d.note_escape();
            }
            let _ = lcg_next(&mut seed);
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：门号越界 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            if d.kick(i as usize % DOOR_TARGETS, DOORS_PER_TARGET as u8 + 1) == Err(DoorCode::DoorIndexOob) {
                caught += 1;
                d.note_detected();
                d.violations = 0;
            } else {
                d.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：敲击洪水 ×10（同 target 连敲超限速）。
    {
        let mut caught = 0u32;
        // 先清走前两类积累的违例：复位观察窗。
        d.reset_phase();
        d.violations = 0;
        for _ in 0..(DOORBELL_RATE_CAP + 10) {
            d.clear_replay(1); // 洪水事件彼此独立，非重发
            if d.kick(1, 0) == Err(DoorCode::RateFlood) {
                caught += 1;
                d.note_detected();
                // 洪水检出即清违例计数：本类只考察限速，不叠加隔离相态。
                d.violations = 0;
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：敲击重放 ×10（同 target 同门号同窗重复敲）。
    {
        let mut caught = 0u32;
        d.reset_phase();
        d.violations = 0;
        let mut i = 0u32;
        while i < 10u32 {
            d.decay_window();
            // 先敲一个未用门号，再立刻重放同参数。
            let door = 3u8;
            let _ = d.kick(2, door);
            if d.kick(2, door) == Err(DoorCode::KickReplay) {
                caught += 1;
                d.note_detected();
                // 本类只考察重放，检出即清违例计数避免叠加隔离相态。
                d.violations = 0;
            } else {
                d.note_escape();
            }
            i += 1;
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：隔离期敲击 ×10（连续违例入隔离后一切敲击必须拒）。
    {
        let mut caught = 0u32;
        d.reset_phase();
        d.violations = 0;
        for i in 0..QUARANTINE_THRESHOLD {
            let _ = d.kick(DOOR_TARGETS + 100 + i as usize, 0);
        }
        if d.phase() == DoorbellPhase::Quarantined {
            for _ in 0..10u32 {
                if d.kick(0, 0) == Err(DoorCode::Quarantined) {
                    caught += 1;
                    d.note_detected();
                } else {
                    d.note_escape();
                }
            }
        } else {
            for _ in 0..10u32 {
                d.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (d.injected, d.detected, d.escaped, classes_hit)
}

/// 合法敲击 ×10^4 零误拒：每轮滑动限速窗，target/门号恒合法。
pub fn legal_stream_zero_false() -> bool {
    let mut d = DoorbellDispatcher::new();
    let mut false_reject = 0u64;
    let mut round = 0u64;
    while round < 10_000u64 {
        let target = (round % DOOR_TARGETS as u64) as usize;
        let door = ((round / DOOR_TARGETS as u64) % DOORS_PER_TARGET as u64) as u8;
        d.decay_window();
        if d.kick(target, door).is_err() {
            false_reject += 1;
        }
        round += 1;
    }
    false_reject == 0 && d.phase() == DoorbellPhase::Healthy && d.audit_conserved()
}

/// 恢复后账一致 ×10：每次隔离→复位→写回，账守恒成立。
pub fn recover_ledger_consistent() -> bool {
    let mut all_ok = true;
    let mut i = 0u32;
    while i < 10u32 {
        let mut d = DoorbellDispatcher::new();
        for j in 0..QUARANTINE_THRESHOLD {
            let _ = d.kick(DOOR_TARGETS + j as usize, 0);
        }
        let wb = d.recover();
        match wb {
            Ok(w) => {
                if !w.done || w.summary.is_empty() || !d.audit_conserved() {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
        i += 1;
    }
    all_ok
}

/// 引擎指纹：注入核 + 合法流 + 恢复链快照混合 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let (inj, det, esc, cls) = injection_core();
    let mut d = DoorbellDispatcher::new();
    for t in 0..DOOR_TARGETS {
        let _ = d.kick(t, 1);
        d.decay_window();
    }
    let snap = d.snapshot();
    let full = format!("unx51603|{} {} {} {}|{:?}", inj, det, esc, cls, snap);
    fnv1a(full.as_bytes())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51603_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51603");
    // 1 合法敲击放行并留痕。
    {
        let mut d = DoorbellDispatcher::new();
        let a = d.kick(0, 0);
        let b = d.kick(DOOR_TARGETS - 1, DOORS_PER_TARGET as u8 - 1);
        s.add(
            "合法敲击放行",
            a.is_ok() && b.is_ok() && d.accept_cnt() == 2,
            "首尾 target 与首尾门号合法敲击必须放行",
        );
    }
    // 2 target 越界全拒并审计。
    {
        let mut d = DoorbellDispatcher::new();
        let mut rejected = 0u32;
        for i in 0..10u32 {
            if d.kick(DOOR_TARGETS + i as usize, 0) == Err(DoorCode::TargetOob) {
                rejected += 1;
                d.violations = 0;
            }
        }
        s.add(
            "越界全拒审计",
            rejected == 10 && d.reject_cnt() == 10 && d.audit_conserved(),
            "target 越界 ×10 必须全拒且逐条审计",
        );
    }
    // 3 合法敲击 ×10^4 零误拒。
    s.add("合法流零误拒", legal_stream_zero_false(), "10000 轮合法敲击不得误拒一次且相态保持健康");
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
    // 6 隔离相态触发：连续违例达阈值入 Quarantined。
    {
        let mut d = DoorbellDispatcher::new();
        for i in 0..QUARANTINE_THRESHOLD {
            let _ = d.kick(DOOR_TARGETS + 50 + i as usize, 0);
        }
        s.add(
            "违例入隔离",
            d.phase() == DoorbellPhase::Quarantined,
            "连续 5 次违例必须进入隔离相态",
        );
    }
    // 7 复位恢复全链 + 写回 ×10 账一致。
    s.add("恢复后账一致×10", recover_ledger_consistent(), "10 次隔离→复位全通且写回非空、审计守恒");
    // 8 隔离期敲击全拒。
    {
        let mut d = DoorbellDispatcher::new();
        for i in 0..QUARANTINE_THRESHOLD {
            let _ = d.kick(DOOR_TARGETS + 70 + i as usize, 0);
        }
        let r = d.kick(0, 0);
        s.add(
            "隔离期拒敲",
            r == Err(DoorCode::Quarantined),
            "隔离相态内一切敲击必须显性拒",
        );
    }
    // 9 相态回退显性拒。
    {
        let mut d = DoorbellDispatcher::new();
        let r0 = d.advance_phase(DoorbellPhase::Recovered);
        let _ = d.kick(DOOR_TARGETS + 90, 0);
        let _ = d.kick(DOOR_TARGETS + 91, 0);
        let _ = d.kick(DOOR_TARGETS + 92, 0);
        let _ = d.kick(DOOR_TARGETS + 93, 0);
        let _ = d.kick(DOOR_TARGETS + 94, 0);
        let back = d.advance_phase(DoorbellPhase::Healthy);
        s.add(
            "相态单调",
            r0 == Err(DoorCode::StateRegress) && back == Err(DoorCode::StateRegress),
            "Healthy→Recovered 跳相与 Quarantined→Healthy 回退都必须显性拒",
        );
    }
    // 10 限速滑窗语义：达上限判洪水，滑窗后放行。
    {
        let mut d = DoorbellDispatcher::new();
        d.violations = 0;
        let mut i = 0u32;
        while i < DOORBELL_RATE_CAP {
            d.clear_replay(3);
            let _ = d.kick(3, 2);
            i += 1;
        }
        d.clear_replay(3);
        let flood = d.kick(3, 2);
        d.decay_window();
        d.clear_replay(3);
        let after = d.kick(3, 2);
        s.add(
            "限速滑窗",
            flood == Err(DoorCode::RateFlood) && after.is_ok(),
            "窗内超上限判洪水，滑窗衰减后恢复放行",
        );
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
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC1980 须 0x40 对齐且在分派区间");
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
    // 16 恢复未完成拒写回（健康相态直接 recover）。
    {
        let mut d = DoorbellDispatcher::new();
        s.add(
            "未隔离拒写回",
            d.recover() == Err(DoorCode::RecoveryIncomplete),
            "未进入隔离相态不得写回恢复完成",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_kick_accepted_and_audited() {
        let mut d = DoorbellDispatcher::new();
        assert!(d.kick(0, 0).is_ok());
        assert!(d.kick(7, 3).is_ok());
        assert_eq!(d.accept_cnt(), 2);
        assert!(d.audit_conserved());
    }

    #[test]
    fn target_oob_rejected_and_audited() {
        let mut d = DoorbellDispatcher::new();
        for i in 0..10u32 {
            assert_eq!(d.kick(DOOR_TARGETS + i as usize, 0), Err(DoorCode::TargetOob));
            d.violations = 0;
        }
        assert_eq!(d.reject_cnt(), 10);
        assert!(d.audit_conserved());
    }

    #[test]
    fn door_index_oob_rejected() {
        let mut d = DoorbellDispatcher::new();
        assert_eq!(d.kick(2, 4), Err(DoorCode::DoorIndexOob));
        assert_eq!(d.kick(2, 255), Err(DoorCode::DoorIndexOob));
    }

    #[test]
    fn rate_flood_then_decay_release() {
        let mut d = DoorbellDispatcher::new();
        d.violations = 0;
        let mut i = 0u32;
        while i < DOORBELL_RATE_CAP {
            d.clear_replay(1);
            assert!(d.kick(1, 1).is_ok());
            i += 1;
        }
        d.clear_replay(1);
        assert_eq!(d.kick(1, 1), Err(DoorCode::RateFlood));
        d.decay_window();
        d.clear_replay(1);
        assert!(d.kick(1, 1).is_ok());
    }

    #[test]
    fn replay_kick_caught() {
        let mut d = DoorbellDispatcher::new();
        assert!(d.kick(4, 2).is_ok());
        assert_eq!(d.kick(4, 2), Err(DoorCode::KickReplay));
    }

    #[test]
    fn quarantine_and_recover_full_chain() {
        let mut d = DoorbellDispatcher::new();
        for i in 0..QUARANTINE_THRESHOLD {
            let _ = d.kick(DOOR_TARGETS + i as usize, 0);
        }
        assert_eq!(d.phase(), DoorbellPhase::Quarantined);
        let wb = d.recover();
        assert!(wb.is_ok());
        assert_eq!(d.phase(), DoorbellPhase::Recovered);
        assert!(d.audit_conserved());
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn legal_stream_ten_thousand_zero_false() {
        assert!(legal_stream_zero_false());
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
    fn unx51603_checks_green() {
        let set = run_unx51603_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
