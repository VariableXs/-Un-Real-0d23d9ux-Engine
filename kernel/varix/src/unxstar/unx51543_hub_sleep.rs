//! UNX-F51543 HUB 自身休眠（空闲超时休眠与唤醒边界） · 0xC0A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51544-J1 过流恢复后预算重算 ×10 一致
//!
//! 设计要点：
//!   ① 休眠域按「逻辑拍空闲计数」建模：不引墙钟，只用业务拍 tick 推进空
//!      闲计数；空闲计满 SLEEP_AFTER_IDLE=64 拍才允许休眠，边界值逐拍
//!      校验（63 拍拒、64 拍收）。
//!   ② 单调状态机：清醒→休眠→清醒 只有一条合法路径；休眠后再休眠显性拒
//!      （AlreadyAsleep），清醒时唤醒显性拒（AlreadyAwake）；每完成一次
//!      转移纪元 epoch 单调 +1，纪元只增不减。
//!   ③ 空闲计数的单调面：清醒期内快进探针 advance_idle_to 只许前进，倒
//!      退显性拒（IdleRewind）；唤醒是合法清零点（域语义），清零走唤醒
//!      路径并落流水。
//!   ④ 边界注入器：早睡（阈值前）、重复休眠、空转唤醒、陈旧/超前纪元四
//!      类注入各 ×10 轮全抓；守恒账 注入=检出+逃逸，逃逸恒 0。
//!   ⑤ 判定次序：状态先行 → 阈值次之 → 落账；休眠/唤醒事件流水只增不
//!      减，休眠量与唤醒量逐条对平。
//!   ⑥ 全程零浮点、零 IO、零墙钟、零随机；拍计数用 saturating/checked。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0A80;

/// 空闲休眠阈值（逻辑拍）。
pub const SLEEP_AFTER_IDLE: u32 = 64;
/// 边界探针冻结表（休眠边界扫描用）。
pub const BOUNDARY_PROBES: [u32; 5] = [0, 1, 31, 63, 64];

/// HUB 睡眠状态机状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubState {
    /// 清醒
    Awake,
    /// 休眠
    Asleep,
}

/// 睡眠事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepEventKind {
    /// 休眠事件
    Sleep,
    /// 唤醒事件
    Wake,
}

/// 单条睡眠事件（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SleepEvent {
    pub seq: u32,
    pub kind: SleepEventKind,
    pub idle_at: u32,
}

/// 诊断码闭集（HUB 休眠域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 已在休眠（禁重复休眠）
    AlreadyAsleep,
    /// 已在清醒（禁空转唤醒）
    AlreadyAwake,
    /// 空闲未满阈值（禁早睡）
    BelowThreshold,
    /// 纪元读数落后（陈旧观测）
    StaleEpoch,
    /// 纪元读数超前（异常观测）
    EpochAhead,
    /// 空闲计数倒退（清醒期内只许前进）
    IdleRewind,
    /// 账面不符
    LedgerMismatch,
}

impl SleepCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SleepCode::Ok => "睡眠转移成功",
            SleepCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            SleepCode::AlreadyAsleep => "重复休眠：HUB 已在休眠态",
            SleepCode::AlreadyAwake => "空转唤醒：HUB 已在清醒态",
            SleepCode::BelowThreshold => "早睡拒绝：空闲拍数未满休眠阈值",
            SleepCode::StaleEpoch => "纪元落后：观测读数旧于内核纪元",
            SleepCode::EpochAhead => "纪元超前：观测读数新于内核纪元",
            SleepCode::IdleRewind => "空闲倒退：清醒期内空闲计数只许前进",
            SleepCode::LedgerMismatch => "账面不符：休眠量与唤醒量必须逐条对平",
        }
    }
}

/// HUB 睡眠核心：状态机 + 纪元 + 事件流水 + 守恒账。
pub struct HubSleepCore {
    state: HubState,
    idle_ticks: u32,
    epoch: u64,
    seq: u32,
    journal: Vec<SleepEvent>,
    sleep_cnt: u64,
    wake_cnt: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl HubSleepCore {
    pub fn new() -> HubSleepCore {
        HubSleepCore {
            state: HubState::Awake,
            idle_ticks: 0,
            epoch: 0,
            seq: 0,
            journal: Vec::new(),
            sleep_cnt: 0,
            wake_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 业务拍：清醒时空闲计数 +1（饱和）；休眠态拍不累计。
    pub fn tick(&mut self) {
        if self.state == HubState::Awake {
            self.idle_ticks = self.idle_ticks.saturating_add(1);
        }
    }

    /// 快进探针：清醒期内空闲计数只许前进，倒退显性拒。
    pub fn advance_idle_to(&mut self, v: u32) -> Result<(), SleepCode> {
        if self.state != HubState::Awake {
            return Err(SleepCode::AlreadyAsleep);
        }
        if v < self.idle_ticks {
            return Err(SleepCode::IdleRewind);
        }
        self.idle_ticks = v;
        Ok(())
    }

    /// 业务活动：清醒态清零空闲计数；休眠态视作唤醒源。
    pub fn activity(&mut self) {
        if self.state == HubState::Asleep {
            self.wake_transition();
        } else {
            self.idle_ticks = 0;
        }
    }

    /// 请求休眠：状态先行 → 阈值次之 → 落账。
    pub fn try_sleep(&mut self) -> Result<(), SleepCode> {
        if self.state == HubState::Asleep {
            return Err(SleepCode::AlreadyAsleep);
        }
        if self.idle_ticks < SLEEP_AFTER_IDLE {
            return Err(SleepCode::BelowThreshold);
        }
        self.state = HubState::Asleep;
        self.epoch = self.epoch.saturating_add(1);
        self.seq = self.seq.saturating_add(1);
        self.journal.push(SleepEvent {
            seq: self.seq,
            kind: SleepEventKind::Sleep,
            idle_at: self.idle_ticks,
        });
        // 休眠落账后空闲计数清零（唤醒从零重新累计，饱和拍不跨周期残留）。
        self.idle_ticks = 0;
        self.sleep_cnt = self.sleep_cnt.saturating_add(1);
        Ok(())
    }

    /// 请求唤醒：清醒态显性拒。
    pub fn wake(&mut self) -> Result<(), SleepCode> {
        if self.state == HubState::Awake {
            return Err(SleepCode::AlreadyAwake);
        }
        self.wake_transition();
        Ok(())
    }

    /// 唤醒转移（内部）：纪元 +1、清零空闲、落唤醒事件。
    fn wake_transition(&mut self) {
        self.state = HubState::Awake;
        self.epoch = self.epoch.saturating_add(1);
        self.seq = self.seq.saturating_add(1);
        self.journal.push(SleepEvent {
            seq: self.seq,
            kind: SleepEventKind::Wake,
            idle_at: self.idle_ticks,
        });
        self.idle_ticks = 0;
        self.wake_cnt = self.wake_cnt.saturating_add(1);
    }

    /// 纪元观测一致性：落后/超前都显性拒。
    pub fn claim_epoch(&self, v: u64) -> Result<(), SleepCode> {
        if v < self.epoch {
            return Err(SleepCode::StaleEpoch);
        }
        if v > self.epoch {
            return Err(SleepCode::EpochAhead);
        }
        Ok(())
    }

    pub fn state(&self) -> HubState {
        self.state
    }

    pub fn idle(&self) -> u32 {
        self.idle_ticks
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    pub fn tally(&self) -> (u64, u64) {
        (self.sleep_cnt, self.wake_cnt)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0 且检出非零。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }

    /// 流水一致：休眠量+唤醒量 = 流水长度 = 序号。
    pub fn journal_consistent(&self) -> bool {
        self.sleep_cnt.saturating_add(self.wake_cnt) == self.journal.len() as u64
            && self.seq as u64 == self.journal.len() as u64
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (HubState, u32, u64, u32, usize) {
        (self.state, self.idle_ticks, self.epoch, self.seq, self.journal.len())
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一拍脚本两次建核，快照与流水逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> ((HubState, u32, u64, u32, usize), Vec<SleepEvent>) {
        let mut c = HubSleepCore::new();
        for _ in 0..3u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
            let _ = c.try_sleep();
            let _ = c.wake();
        }
        (c.snapshot(), c.journal.clone())
    };
    build() == build()
}

/// 边界扫描：探针值低于阈值拒睡、等于阈值收睡。
pub fn boundary_probe_ok(v: u32) -> bool {
    let mut c = HubSleepCore::new();
    let _ = c.advance_idle_to(v);
    let low = c.try_sleep();
    if v < SLEEP_AFTER_IDLE {
        low == Err(SleepCode::BelowThreshold)
    } else {
        low.is_ok()
    }
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51543_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51543");
    // 1 正常流：满阈休眠、唤醒、纪元推进。
    {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let sl = c.try_sleep();
        let wk = c.wake();
        s.add_dyn(
            "满阈休眠唤醒",
            sl.is_ok() && wk.is_ok() && c.epoch() == 2 && c.state() == HubState::Awake,
            "阈值 64 拍应可休眠并唤醒，纪元 +2",
        );
    }
    // 2 边界表扫描：0/1/31/63 拒、64 收。
    {
        let mut all = true;
        for v in BOUNDARY_PROBES {
            if !boundary_probe_ok(v) {
                all = false;
            }
        }
        s.add_dyn("边界表扫描", all, "阈值前显性拒、阈值处放行");
    }
    // 3 边界 ×10：63 拍拒、64 拍收，逐轮复验。
    {
        let mut c = HubSleepCore::new();
        let mut all = true;
        for _ in 0..10u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE - 1);
            if c.try_sleep() != Err(SleepCode::BelowThreshold) {
                all = false;
            }
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
            if c.try_sleep().is_err() {
                all = false;
            }
            let _ = c.wake();
        }
        s.add_dyn("休眠边界 ×10", all, "63 拍必须拒、64 拍必须收，×10 轮一致");
    }
    // 4 上限边界：空闲饱和后仍可休眠。
    {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(u32::MAX);
        c.tick();
        let r = c.try_sleep();
        s.add_dyn(
            "空闲饱和上限",
            r.is_ok() && c.idle() == 0 && c.state() == HubState::Asleep,
            "饱和拍计数不得溢出，休眠后清零",
        );
    }
    // 5 注入必抓：早睡 ×10 轮。
    {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = c.advance_idle_to(i);
            if c.try_sleep() == Err(SleepCode::BelowThreshold) {
                caught += 1;
                c.note_detected();
            }
        }
        s.add_dyn("早睡注入 10/10", caught == 10, "阈值前休眠必须全抓");
    }
    // 6 注入必抓：重复休眠 ×10 轮。
    {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE + i);
            let _ = c.try_sleep();
            if c.try_sleep() == Err(SleepCode::AlreadyAsleep) {
                caught += 1;
                c.note_detected();
            }
            let _ = c.wake();
        }
        s.add_dyn("重复休眠注入 10/10", caught == 10, "休眠态再休眠必须全抓");
    }
    // 7 注入必抓：空转唤醒 ×10 轮。
    {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if c.wake() == Err(SleepCode::AlreadyAwake) {
                caught += 1;
                c.note_detected();
            }
        }
        s.add_dyn("空转唤醒注入 10/10", caught == 10, "清醒态唤醒必须全抓");
    }
    // 8 注入必抓：陈旧/超前纪元 ×10 轮。
    {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let _ = c.try_sleep();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let stale = c.epoch().saturating_sub(1);
            let ahead = c.epoch().saturating_add(1 + i as u64);
            let r1 = c.claim_epoch(stale);
            let r2 = c.claim_epoch(ahead);
            if r1 == Err(SleepCode::StaleEpoch) && r2 == Err(SleepCode::EpochAhead) {
                caught += 1;
                c.note_detected();
            }
        }
        s.add_dyn("纪元观测注入 10/10", caught == 10, "陈旧与超前纪元必须全抓");
    }
    // 9 倒退显性拒：空闲计数只许前进。
    {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(40);
        let r = c.advance_idle_to(39);
        let r2 = c.advance_idle_to(0);
        s.add_dyn(
            "空闲倒退显性拒",
            r == Err(SleepCode::IdleRewind)
                && r2 == Err(SleepCode::IdleRewind)
                && c.idle() == 40,
            "倒退探针显性拒且计数不动",
        );
    }
    // 10 判定次序：状态先行于阈值。
    {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let _ = c.try_sleep(); // 休眠态，空闲已入账
        let r = c.try_sleep();
        s.add_dyn(
            "判序状态先行",
            r == Err(SleepCode::AlreadyAsleep),
            "状态判据先于阈值判据",
        );
    }
    // 11 拍守恒：休眠量=唤醒量，流水长度=序号。
    {
        let mut c = HubSleepCore::new();
        for _ in 0..10u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
            let _ = c.try_sleep();
            let _ = c.wake();
        }
        let (sl, wk) = c.tally();
        s.add_dyn(
            "休眠唤醒量守恒",
            sl == wk && sl == 10 && c.journal_consistent(),
            "休眠量=唤醒量=10，流水逐条对平",
        );
    }
    // 12 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut c = HubSleepCore::new();
        for _ in 0..10u32 {
            let _ = c.wake();
            c.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            c.conserved() && c.snapshot() == (HubState::Awake, 0, 0, 0, 0),
            "注入=检出+逃逸，逃逸恒 0，被拒转移不落账",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一拍脚本两次建核快照与流水相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 15 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [SleepCode; 9] = [
    SleepCode::Ok,
    SleepCode::SegBad,
    SleepCode::AlreadyAsleep,
    SleepCode::AlreadyAwake,
    SleepCode::BelowThreshold,
    SleepCode::StaleEpoch,
    SleepCode::EpochAhead,
    SleepCode::IdleRewind,
    SleepCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_wake_full_cycle() {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        assert!(c.try_sleep().is_ok());
        assert_eq!(c.state(), HubState::Asleep);
        assert!(c.wake().is_ok());
        assert_eq!(c.state(), HubState::Awake);
        assert_eq!(c.idle(), 0);
        assert_eq!(c.epoch(), 2);
    }

    #[test]
    fn boundary_exact_threshold() {
        for v in BOUNDARY_PROBES {
            assert!(boundary_probe_ok(v), "探针 {} 边界判定不符", v);
        }
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE - 1);
        assert_eq!(c.try_sleep(), Err(SleepCode::BelowThreshold));
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        assert!(c.try_sleep().is_ok());
    }

    #[test]
    fn early_sleep_injection_all_caught() {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let _ = c.advance_idle_to(i);
            if c.try_sleep() == Err(SleepCode::BelowThreshold) {
                caught += 1;
                c.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(c.conserved());
    }

    #[test]
    fn double_sleep_injection_all_caught() {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
            let _ = c.try_sleep();
            assert_eq!(c.try_sleep(), Err(SleepCode::AlreadyAsleep));
            let _ = c.wake();
            caught += 1;
            c.note_detected();
        }
        assert_eq!(caught, 10);
    }

    #[test]
    fn idle_wake_injection_all_caught() {
        let mut c = HubSleepCore::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if c.wake() == Err(SleepCode::AlreadyAwake) {
                caught += 1;
                c.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert!(c.conserved());
    }

    #[test]
    fn epoch_claim_rejects_stale_and_ahead() {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let _ = c.try_sleep();
        assert_eq!(c.claim_epoch(0), Err(SleepCode::StaleEpoch));
        assert_eq!(c.claim_epoch(2), Err(SleepCode::EpochAhead));
        assert!(c.claim_epoch(1).is_ok());
    }

    #[test]
    fn idle_rewind_explicit_reject() {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(40);
        assert_eq!(c.advance_idle_to(39), Err(SleepCode::IdleRewind));
        assert_eq!(c.advance_idle_to(0), Err(SleepCode::IdleRewind));
        assert_eq!(c.idle(), 40);
        assert!(c.advance_idle_to(41).is_ok());
    }

    #[test]
    fn state_before_threshold_order() {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let _ = c.try_sleep();
        assert_eq!(c.try_sleep(), Err(SleepCode::AlreadyAsleep));
    }

    #[test]
    fn activity_wakes_from_sleep() {
        let mut c = HubSleepCore::new();
        let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
        let _ = c.try_sleep();
        c.activity();
        assert_eq!(c.state(), HubState::Awake);
        assert_eq!(c.idle(), 0);
        let _ = c.advance_idle_to(5);
        c.activity();
        assert_eq!(c.idle(), 0);
    }

    #[test]
    fn tally_conserves_after_cycles() {
        let mut c = HubSleepCore::new();
        for _ in 0..10u32 {
            let _ = c.advance_idle_to(SLEEP_AFTER_IDLE);
            let _ = c.try_sleep();
            let _ = c.wake();
        }
        let (sl, wk) = c.tally();
        assert_eq!(sl, wk);
        assert_eq!(sl, 10);
        assert!(c.journal_consistent());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx51543_checks_green() {
        let set = run_unx51543_checks();
        assert!(set.all_passed(), "UNX-F51543 red: {}", set.red_items_text());
    }
}
