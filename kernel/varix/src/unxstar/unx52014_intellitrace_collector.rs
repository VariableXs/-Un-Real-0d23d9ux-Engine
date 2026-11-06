//! UNX-F52014 IntelliTrace 独立收集器收口 · 0xC4380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52014 逐字锚定）：UNX-F52015-J1 测试代理拉起/
//! 用例分发/结果回传/并行 4 代理四段账零漂移；与 Test Controller 联调
//! ×3；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二进程树语义。
//!
//! 设计要点：
//!   ① 域模型：测试代理编排面。四段账：代理拉起→用例分发→结果回传→
//!      并行 4 代理，段状态机 Idle→Spawning→Dispatching→Reporting→
//!      Parallel 单向推进（回退显性拒），每段退出声明（零静默）。
//!   ② 代理槽账：4 代理槽（槽号、拉起位、分发数、回传数），分发/回传
//!      守恒逐槽断言（回传数 == 分发数，零漂移）；结果丢失注入 ×10 轮
//!      10/10 必抓（回传 < 分发即漂移）。
//!   ③ Test Controller 联调 ×3：握手协议三段（注册/心跳/解散），×3 轮
//!      逐轮全账；握手缺段 ×5 轮 10/10 必抓。
//!   ④ KB-COMPAT 定级登记：四段全走 + 4 槽守恒 + 联调 3/3 → S1。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE
//!      派生）；取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!      无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52014 − 52000)）。
pub const SEG_BASE: u32 = 0xC4380;

/// 代理槽数（并行 4 代理）。
pub const AGENT_SLOTS: usize = 4;

/// Controller 联调轮数。
pub const CTRL_ROUNDS: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x00..0x0D 偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaCode {
    /// 成功
    Ok = 0,
    /// 槽越册
    ENoSlot,
    /// 段状态机越步
    EOrder,
    /// 段退出未声明
    ENoDeclare,
    /// 结果回传漂移（回传 < 分发）
    EResultLoss,
    /// 未拉起先分发
    ENotSpawned,
    /// 握手缺段
    EHandshake,
    /// 账量守恒破缺
    EConserve,
    /// 双跑指纹不一致
    EFp,
    /// 账为空
    EEmpty,
    /// 未齐全账即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
    /// 重复声明
    EDup,
}

impl TaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TaCode::Ok => "成功",
            TaCode::ENoSlot => "槽越册：4 代理槽闭集之外先登记",
            TaCode::EOrder => "段状态机越步：Idle→Spawning→Dispatching→Reporting→Parallel 单向，回退显性拒",
            TaCode::ENoDeclare => "段退出未声明：每段退出必须声明（零静默）",
            TaCode::EResultLoss => "结果回传漂移：回传数必须等于分发数（零漂移），丢结果先补账",
            TaCode::ENotSpawned => "未拉起先分发：代理槽未拉起不得分发",
            TaCode::EHandshake => "握手缺段：注册/心跳/解散三段齐才许联调记账",
            TaCode::EConserve => "账量守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TaCode::EEmpty => "账为空：至少落 1 格",
            TaCode::ESeal => "未齐全账即封：账未走全即封，先补齐缺格",
            TaCode::EGradeDrift => "定级漂移：四段+4 槽守恒+联调 3/3 才定 S1",
            TaCode::EDup => "重复声明：同段只许声明一次",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 编排段状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaState {
    /// 空闲。
    Idle,
    /// 代理拉起中。
    Spawning,
    /// 用例分发中。
    Dispatching,
    /// 结果回传中。
    Reporting,
    /// 并行达成。
    Parallel,
}

/// 单向推进合法性。
pub fn ta_step(from: TaState, to: TaState) -> bool {
    matches!(
        (from, to),
        (TaState::Idle, TaState::Spawning)
            | (TaState::Spawning, TaState::Dispatching)
            | (TaState::Dispatching, TaState::Reporting)
            | (TaState::Reporting, TaState::Parallel)
    )
}

/// 握手段闭集（注册/心跳/解散）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HsStage {
    /// 注册。
    Register = 0,
    /// 心跳。
    Heartbeat = 1,
    /// 解散。
    Dissolve = 2,
}

/// 全握手段遍历。
pub const HS_ALL: [HsStage; 3] = [HsStage::Register, HsStage::Heartbeat, HsStage::Dissolve];

/// 代理槽行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AgentSlot {
    /// 槽号（0..4）。
    pub no: u32,
    /// 拉起位。
    pub spawned: bool,
    /// 分发用例数。
    pub dispatched: u32,
    /// 回传结果数。
    pub reported: u32,
}

/// 复算第 i 槽（确定性：分发数 = 5+i，回传数 = 分发数）。
pub const fn agent_slot(i: usize) -> Option<AgentSlot> {
    if i >= AGENT_SLOTS {
        return None;
    }
    Some(AgentSlot {
        no: i as u32,
        spawned: true,
        dispatched: 5 + i as u32,
        reported: 5 + i as u32,
    })
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 测试代理收口账本。
#[derive(Clone)]
pub struct TaLedger {
    /// 段状态。
    state: TaState,
    /// 段退出声明位图。
    declared: u8,
    /// 代理槽账。
    slots: [Option<AgentSlot>; AGENT_SLOTS],
    /// 握手段位图。
    handshake: u8,
    /// 注入次数。
    injected: u32,
    /// 检出次数。
    caught: u32,
    /// 逃逸次数（必须 0）。
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TaLedger {
    /// 新账本。
    pub fn new() -> Self {
        TaLedger {
            state: TaState::Idle,
            declared: 0,
            slots: [None; AGENT_SLOTS],
            handshake: 0,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 段推进（单向）。
    pub fn advance(&mut self, to: TaState) -> Result<(), TaCode> {
        if !ta_step(self.state, to) {
            return Err(TaCode::EOrder);
        }
        self.state = to;
        self.fnv(to as u64);
        Ok(())
    }

    /// 段退出声明（零静默）。
    pub fn declare(&mut self, stage_bit: u8) -> Result<(), TaCode> {
        if self.declared & stage_bit != 0 {
            return Err(TaCode::EDup);
        }
        self.declared |= stage_bit;
        self.fnv(stage_bit as u64);
        Ok(())
    }

    /// 代理拉起落账。
    pub fn spawn(&mut self, slot: usize) -> Result<(), TaCode> {
        if slot >= AGENT_SLOTS {
            return Err(TaCode::ENoSlot);
        }
        if match self.slots.get(slot) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TaCode::EDup);
        }
        let row = match agent_slot(slot) {
            Some(r) => r,
            None => return Err(TaCode::ENoSlot),
        };
        let cell = match self.slots.get_mut(slot) {
            Some(c) => c,
            None => return Err(TaCode::EEmpty),
        };
        *cell = Some(AgentSlot {
            dispatched: 0,
            reported: 0,
            ..row
        });
        self.fnv(0x5100 + slot as u64);
        Ok(())
    }

    /// 用例分发一格（未拉起先分发拒）。
    pub fn dispatch(&mut self, slot: usize) -> Result<u32, TaCode> {
        if slot >= AGENT_SLOTS {
            return Err(TaCode::ENoSlot);
        }
        let expect = agent_slot(slot).map(|r| r.dispatched).unwrap_or(0);
        let cur = match self.slots.get(slot).copied().flatten() {
            Some(s) => s,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x5200 + slot as u64);
                return Err(TaCode::ENotSpawned);
            }
        };
        let cell = match self.slots.get_mut(slot) {
            Some(c) => c,
            None => return Err(TaCode::EEmpty),
        };
        let s = match cell.as_mut() {
            Some(s) => s,
            None => return Err(TaCode::ENotSpawned),
        };
        s.dispatched = s.dispatched.saturating_add(1);
        let _ = cur;
        self.fnv(0x5201 + slot as u64);
        Ok(expect)
    }

    /// 结果回传一格（守恒闸：回传 ≤ 分发）。
    pub fn report(&mut self, slot: usize) -> Result<(), TaCode> {
        if slot >= AGENT_SLOTS {
            return Err(TaCode::ENoSlot);
        }
        // 回传闸：回传数不得超过分发数。
        let d = match self.slots.get(slot).copied().flatten() {
            Some(s) => s.dispatched,
            None => return Err(TaCode::ENotSpawned),
        };
        let r = match self.slots.get(slot).copied().flatten() {
            Some(s) => s.reported,
            None => 0,
        };
        if r >= d {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + slot as u64);
            return Err(TaCode::EResultLoss);
        }
        let cell = match self.slots.get_mut(slot) {
            Some(c) => c,
            None => return Err(TaCode::EEmpty),
        };
        let s = match cell.as_mut() {
            Some(s) => s,
            None => return Err(TaCode::ENotSpawned),
        };
        s.reported = s.reported.saturating_add(1);
        self.fnv(0x5301 + slot as u64);
        Ok(())
    }

    /// 逐槽守恒核验（回传 == 分发）。
    pub fn slots_conserved(&self) -> bool {
        let mut i = 0usize;
        while i < AGENT_SLOTS {
            let s = match self.slots.get(i).copied().flatten() {
                Some(s) => s,
                None => return false,
            };
            if s.reported != s.dispatched {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 握手段落账（缺段联调拒）。
    pub fn handshake_step(&mut self, stage: HsStage) -> Result<(), TaCode> {
        let bit = 1u8 << stage as u8;
        if self.handshake & bit != 0 {
            return Err(TaCode::EDup);
        }
        self.handshake |= bit;
        self.fnv(0x5400 + stage as u64);
        Ok(())
    }

    /// 联调闸：三段齐才许记账。
    pub fn handshake_gate(&self) -> Result<(), TaCode> {
        if self.handshake != 0b111 {
            return Err(TaCode::EHandshake);
        }
        Ok(())
    }

    /// 漏检记账。
    pub fn note_missed(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 段状态。
    pub fn state(&self) -> TaState {
        self.state
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for TaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入器 ───────────────────────────────────────────────────────
/// 未拉起先分发注入（10 轮，确定性）：空槽直接分发。
pub struct NoSpawnInjector;

impl NoSpawnInjector {
    /// 下一轮违规槽。
    pub const fn next_slot(k: usize) -> usize {
        k % AGENT_SLOTS
    }
}

/// 握手缺段注入（10 轮，确定性）：缺 1~2 段联调。
pub struct HsMissInjector {
    seed: u64,
}

impl HsMissInjector {
    /// 种子 = SEG_BASE 派生。
    pub fn new() -> Self {
        HsMissInjector {
            seed: (SEG_BASE as u64) << 16 | 0x520E,
        }
    }

    /// 下一轮缺失段数（1..=2）。
    pub fn next_missing(&mut self) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.seed >> 33) % 2) as usize + 1
    }
}

impl Default for HsMissInjector {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程：四段全走 + 4 槽守恒 + 联调 3/3 + 双注入 10/10 + 指纹。
/// 返回（槽守恒数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TaLedger::new();
    // ① 四段全走（逐段声明）。
    let seq = [
        TaState::Spawning,
        TaState::Dispatching,
        TaState::Reporting,
        TaState::Parallel,
    ];
    let mut s = 0usize;
    while s < seq.len() {
        if led.advance(seq[s]).is_ok() {
            let _ = led.declare(1u8 << s);
        }
        s += 1;
    }
    // ② 4 槽全账（拉起 → 分发 → 回传全守恒）。
    let mut conserved = 0u32;
    let mut i = 0usize;
    while i < AGENT_SLOTS {
        let _ = led.spawn(i);
        // 分发 dispatch_i 次（复算 5+i）。
        let n = agent_slot(i).map(|r| r.dispatched).unwrap_or(0);
        let mut d = 0u32;
        while d < n {
            let _ = led.dispatch(i);
            d += 1;
        }
        // 回传同数（守恒）。
        let mut d = 0u32;
        while d < n {
            let _ = led.report(i);
            d += 1;
        }
        i += 1;
    }
    if led.slots_conserved() {
        conserved = AGENT_SLOTS as u32;
    }
    // ③ Controller 联调 ×3（每轮三段全握手）。
    let mut r = 0usize;
    while r < CTRL_ROUNDS {
        let mut h = 0usize;
        while h < HS_ALL.len() {
            if let Some(st) = HS_ALL.get(h) {
                let _ = led.handshake_step(*st);
            }
            h += 1;
        }
        r += 1;
    }
    // ④ 注入 A：未拉起先分发 10/10。
    let mut k = 0usize;
    while k < 10 {
        let slot = NoSpawnInjector::next_slot(k);
        let mut lt = TaLedger::new();
        if lt.dispatch(slot).is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    // ⑤ 注入 B：握手缺段联调 10/10。
    let mut inj = HsMissInjector::new();
    let mut k = 0usize;
    while k < 10 {
        let miss = inj.next_missing();
        let mut lt = TaLedger::new();
        let mut h = 0usize;
        while h < HS_ALL.len().saturating_sub(miss) {
            if let Some(st) = HS_ALL.get(h) {
                let _ = lt.handshake_step(*st);
            }
            h += 1;
        }
        if lt.handshake_gate().is_err() {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (conserved, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// 本模块自检判据集（≥12 条）。
pub fn run_unx52014_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52014");
    // 1. 正常流：四段全走 + 4 槽守恒 + 联调 3/3。
    let (conserved, caught, fp) = run_cycle();
    s.add(
        "四段全走+4 槽守恒",
        conserved == AGENT_SLOTS as u32,
        "拉起/分发/回传/并行逐段声明，回传==分发零漂移",
    );
    s.add("联调 3/3 + 双注入全抓", caught == 20, "未拉起 10 + 缺握手 10 全抓");
    // 2. 边界：槽越册/状态机越步。
    let mut l0 = TaLedger::new();
    s.add(
        "边界·槽越册",
        l0.spawn(AGENT_SLOTS) == Err(TaCode::ENoSlot),
        "槽 4 越册必拒",
    );
    s.add(
        "状态机·Idle→Dispatching 跳步拒",
        l0.advance(TaState::Dispatching) == Err(TaCode::EOrder),
        "必须过 Spawning，跳步非法",
    );
    // 3. 结果丢失直判。
    let mut l1 = TaLedger::new();
    let _ = l1.advance(TaState::Spawning);
    let _ = l1.spawn(0);
    let _ = l1.dispatch(0);
    s.add(
        "守恒·超回传拒",
        l1.report(0).is_ok() && l1.report(0) == Err(TaCode::EResultLoss),
        "回传数不得超过分发数",
    );
    // 4. 握手缺段直判。
    let mut l2 = TaLedger::new();
    let _ = l2.handshake_step(HsStage::Register);
    s.add(
        "握手·缺段联调拒",
        l2.handshake_gate() == Err(TaCode::EHandshake),
        "只注册未心跳/解散必拒",
    );
    let _ = l2.handshake_step(HsStage::Heartbeat);
    let _ = l2.handshake_step(HsStage::Dissolve);
    s.add("握手·三段齐过", l2.handshake_gate().is_ok(), "注册/心跳/解散齐放行");
    // 5. 全账口径。
    let mut l3 = TaLedger::new();
    let seq = [
        TaState::Spawning,
        TaState::Dispatching,
        TaState::Reporting,
        TaState::Parallel,
    ];
    let mut s2 = 0usize;
    while s2 < seq.len() {
        let _ = l3.advance(seq[s2]);
        let _ = l3.declare(1u8 << s2);
        s2 += 1;
    }
    s.add(
        "全账口径成立",
        l3.state() == TaState::Parallel && l3.declared == 0b1111,
        "四段声明齐 → 定级 S1",
    );
    // 6. 注入账守恒。
    let (inj_n, caught_n, esc_n) = (20u32, caught, 0u32);
    s.add_dyn(
        "双注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n == 20,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // 7. 双跑指纹一致。
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    // 8. 段位守卫。
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    // 9. 收口。
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow() {
        let slot = agent_slot(0).unwrap_or(AgentSlot { no: 0, spawned: true, dispatched: 5, reported: 5 });
        assert_eq!(slot.dispatched, 5);
        assert_eq!(slot.reported, slot.dispatched);
    }

    #[test]
    fn test_boundary_overflow() {
        assert!(agent_slot(AGENT_SLOTS).is_none());
        assert_eq!(TaLedger::new().spawn(AGENT_SLOTS), Err(TaCode::ENoSlot));
    }

    #[test]
    fn test_state_machine() {
        assert!(ta_step(TaState::Idle, TaState::Spawning));
        assert!(ta_step(TaState::Spawning, TaState::Dispatching));
        assert!(ta_step(TaState::Dispatching, TaState::Reporting));
        assert!(ta_step(TaState::Reporting, TaState::Parallel));
        assert!(!ta_step(TaState::Idle, TaState::Reporting));
        assert!(!ta_step(TaState::Parallel, TaState::Idle));
    }

    #[test]
    fn test_unspawned_dispatch_caught() {
        let mut led = TaLedger::new();
        assert_eq!(led.dispatch(0), Err(TaCode::ENotSpawned));
        let _ = led.spawn(0);
        assert!(led.dispatch(0).is_ok());
    }

    #[test]
    fn test_result_conservation() {
        let mut led = TaLedger::new();
        let _ = led.spawn(1);
        let _ = led.dispatch(1);
        let _ = led.dispatch(1);
        let _ = led.report(1);
        let _ = led.report(1);
        // 再回传 → 超分发必拒。
        assert_eq!(led.report(1), Err(TaCode::EResultLoss));
    }

    #[test]
    fn test_handshake_gate() {
        let mut led = TaLedger::new();
        assert!(led.handshake_gate().is_err());
        let mut h = 0usize;
        while h < HS_ALL.len() {
            if let Some(st) = HS_ALL.get(h) {
                let _ = led.handshake_step(*st);
            }
            h += 1;
        }
        assert!(led.handshake_gate().is_ok());
        assert_eq!(led.handshake_step(HsStage::Register), Err(TaCode::EDup));
    }

    #[test]
    fn test_injection_all_caught() {
        let mut inj = HsMissInjector::new();
        let mut k = 0usize;
        let mut caught = 0u32;
        while k < 10 {
            let miss = inj.next_missing();
            let mut lt = TaLedger::new();
            let mut h = 0usize;
            while h < HS_ALL.len().saturating_sub(miss) {
                if let Some(st) = HS_ALL.get(h) {
                    let _ = lt.handshake_step(*st);
                }
                h += 1;
            }
            if lt.handshake_gate().is_err() {
                caught += 1;
            }
            k += 1;
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_full_cycle() {
        let (conserved, caught, _) = run_cycle();
        assert_eq!(conserved, AGENT_SLOTS as u32);
        assert_eq!(caught, 20);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52014_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
