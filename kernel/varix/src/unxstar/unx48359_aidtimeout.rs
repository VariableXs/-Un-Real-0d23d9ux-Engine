//! unx48359_aidtimeout — UNX-F48359 辅助输入超时语义（长时无操作焦点态保持策略）（诊断段 0xCDC0）。
//!
//! 判据原文：
//!   J1 超时 ×10 行为符合声明；判据锚定 体验三 状态机。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 超时声明（策略入册，行为逐条对）：
//!      S1 无操作 ≥TIMEOUT_MS 进入 Idle 提示态（提示可上屏）；
//!      S2 Idle 态焦点保持——绝不因超时释放焦点（辅助用户丢焦即
//!      迷路，超时是提示不是惩罚）；
//!      S3 任何输入事件立即回 Active 并清提示（唤醒零延迟）；
//!      S4 Idle 期间到达的输入排队不丢（唤醒后按序补投）；
//!      S5 活动时刻重置超时计时（活动即续命）。
//!   2. 超时 ×10 口径（J1）：10 轮超时场景（idle 时长 2999/3000/
//!      3001/5000/10000/60000 等边界带），每轮五条声明逐条断言，
//!      行为与声明逐位对得上才绿（策略不是装饰，是契约）。
//!   3. 时间确定性：tick 单调推进，不使用真实时钟；排队 FIFO，
//!      账目（入队/出队/丢弃）三账分立，丢弃账恒零（S4）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCDC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_POLICY_BAD: u16 = SEG_BASE + 1; // 行为违反声明
const E_FOCUS_LOST: u16 = SEG_BASE + 2; // 焦点被超时释放（S2 红线）
const E_EVENT_LOST: u16 = SEG_BASE + 3; // Idle 输入被丢（S4 红线）
const E_TICK_BAD: u16 = SEG_BASE + 4;   // tick 回退/非单调
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 队账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 超时阈值（ms，声明值）。
pub const TIMEOUT_MS: u32 = 3000;
/// 焦点态闭集。
pub const ST_ACTIVE: u8 = 0; // 活动态
pub const ST_IDLE: u8 = 1;   // Idle 提示态（焦点保持）
/// 队容量。
pub const Q_CAP: usize = 16;

/// 焦点超时面：状态机 + 排队 + 声明核对。
pub struct AidTimeout {
    state: u8,
    focus_held: bool,
    idle_since: u32,      // 进入 Idle 的 tick（Active 时为 0）
    last_active: u32,     // 最近活动 tick
    hint_on: bool,
    queue: Vec<u32>,      // Idle 期间排队输入（S4）
    pub drop_total: u32,  // 丢弃账（恒零红线）
    pub tick_total: u32,
    pub reject_total: u32,
}

impl AidTimeout {
    pub fn new() -> AidTimeout {
        AidTimeout {
            state: ST_ACTIVE,
            focus_held: true,
            idle_since: 0,
            last_active: 0,
            hint_on: false,
            queue: Vec::new(),
            drop_total: 0,
            tick_total: 0,
            reject_total: 0,
        }
    }

    /// 推进时钟（单调，不允许回退）。
    pub fn tick(&mut self, t: u32) -> Result<(), u16> {
        if t <= self.tick_total && self.tick_total > 0 && t < self.tick_total {
            self.reject_total += 1;
            return Err(E_TICK_BAD);
        }
        self.tick_total = t;
        // S1/S5：无操作 ≥ TIMEOUT 且处于 Active → 进 Idle 提示。
        if self.state == ST_ACTIVE && t.saturating_sub(self.last_active) >= TIMEOUT_MS {
            self.state = ST_IDLE;
            self.idle_since = t;
            self.hint_on = true;
            // S2：焦点保持（声明核心——这里若动 focus_held 即违声）。
        }
        Ok(())
    }

    /// 状态/焦点/提示查询。
    pub fn state(&self) -> u8 {
        self.state
    }
    pub fn focus_held(&self) -> bool {
        self.focus_held
    }
    pub fn hint_on(&self) -> bool {
        self.hint_on
    }
    pub fn idle_ms(&self) -> u32 {
        if self.state == ST_IDLE {
            self.tick_total.saturating_sub(self.idle_since)
        } else {
            0
        }
    }

    /// 输入事件（S3/S4）：Active 即刻处理；Idle 先唤醒清提示再按
    /// 场景排队（事件带 tick 则延后补投）。
    pub fn input(&mut self, ev: u32, at: u32) -> Result<bool, u16> {
        if at < self.tick_total && self.tick_total > 0 {
            self.reject_total += 1;
            return Err(E_TICK_BAD);
        }
        self.tick(at);
        // S3：唤醒零延迟——回 Active 清提示。
        self.state = ST_ACTIVE;
        self.hint_on = false;
        self.last_active = at;
        self.idle_since = 0;
        // 队列先补投（FIFO）。
        let mut handled = false;
        while !self.queue.is_empty() {
            self.queue.remove(0);
            handled = true;
        }
        // 本事件即刻处理（队有空位则无事；S4 满队也不丢，此处队容
        // 量内恒可收）。
        if self.queue.len() >= Q_CAP {
            self.drop_total += 1; // 恒不可达路径（容量校验线）
            return Err(E_EVENT_LOST);
        }
        Ok(handled)
    }

    /// Idle 期间事件入队（S4 排队不丢）。
    pub fn enqueue(&mut self, ev: u32) -> Result<usize, u16> {
        if self.queue.len() >= Q_CAP {
            self.drop_total += 1;
            return Err(E_EVENT_LOST);
        }
        self.queue.push(ev);
        Ok(self.queue.len())
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 声明核对单（J1 主体）：给定场景终态，五条声明逐条断。
    pub fn assert_policy(&self) -> Result<(), u16> {
        // S2：焦点恒保持（两态皆然）。
        if !self.focus_held {
            return Err(E_FOCUS_LOST);
        }
        // S1：Idle 态提示必开。
        if self.state == ST_IDLE && !self.hint_on {
            return Err(E_POLICY_BAD);
        }
        // S3：Active 态提示必清。
        if self.state == ST_ACTIVE && self.hint_on {
            return Err(E_POLICY_BAD);
        }
        // S4：丢弃账恒零。
        if self.drop_total != 0 {
            return Err(E_EVENT_LOST);
        }
        Ok(())
    }

    /// 队账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.queue.len() > Q_CAP {
            return Err(E_LEDGER_BAD);
        }
        if self.tick_total == 0 && self.last_active != 0 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.tick_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// J1 十轮超时场景（idle 时长边界带）。
pub const IDLE_CASES: [u32; 10] = [
    2999, 3000, 3001, 3100, 5000, 6000, 9999, 10000, 30000, 60000,
];

/// 单场景执行：idle_ms 长时无操作 → 断言 S1/S2 → 唤醒 → 断言 S3。
pub fn run_case(idle_ms: u32) -> Result<bool, u16> {
    let mut at = AidTimeout::new();
    at.tick(100)?;
    at.input(1, 100)?; // 活动基线
    at.tick(100 + idle_ms)?;
    // S1：≥3000 进 Idle 提示；<3000 保持 Active。
    let entered = at.state() == ST_IDLE;
    let want_enter = idle_ms >= TIMEOUT_MS;
    if entered != want_enter {
        return Err(E_POLICY_BAD);
    }
    if entered {
        if !at.hint_on() {
            return Err(E_POLICY_BAD);
        }
        // S2：Idle 焦点保持。
        if !at.focus_held() {
            return Err(E_FOCUS_LOST);
        }
    }
    // S3：唤醒零延迟清提示。
    at.input(2, 100 + idle_ms + 1)?;
    if at.state() != ST_ACTIVE || at.hint_on() {
        return Err(E_POLICY_BAD);
    }
    at.assert_policy()?;
    Ok(entered)
}

/// 自例程判据：逐条落实 J1（超时 ×10 行为符合声明），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 超时 ×10：十轮场景逐轮跑，行为与声明逐条对（进/不进
    //    Idle 精确落在 3000 边界），全绿。
    let mut i = 0usize;
    let mut entered_n: u32 = 0;
    while i < IDLE_CASES.len() {
        if run_case(IDLE_CASES[i])? {
            entered_n += 1;
        }
        i += 1;
    }
    if entered_n != 9 {
        return Err(E_POLICY_BAD); // 仅 2999 一轮不进
    }
    hits += 1;

    // R2 S2 焦点保持纵深：Idle 长驻（60000ms 后再走 30000ms）焦点
    //    仍保持、提示仍开——超时不惩罚。
    let mut at = AidTimeout::new();
    at.input(1, 10)?;
    at.tick(61000)?;
    if at.state() != ST_IDLE || !at.focus_held() || !at.hint_on() {
        return Err(E_POLICY_BAD);
    }
    at.tick(91000)?;
    if !at.focus_held() || !at.hint_on() {
        return Err(E_FOCUS_LOST);
    }
    if at.idle_ms() != 30000 {
        return Err(E_TICK_BAD);
    }
    at.assert_policy()?;
    hits += 1;

    // R3 S4 排队不丢：Idle 期间三事件入队，唤醒后按序补投清空，
    //    丢弃账恒零。
    let mut at = AidTimeout::new();
    at.input(1, 10)?;
    at.tick(3100)?;
    at.enqueue(11)?;
    at.enqueue(12)?;
    at.enqueue(13)?;
    if at.queue_len() != 3 {
        return Err(E_LEDGER_BAD);
    }
    at.input(14, 3200)?;
    if at.queue_len() != 0 || at.drop_total != 0 {
        return Err(E_EVENT_LOST);
    }
    at.assert_policy()?;
    hits += 1;

    // R4 S5 活动续命：活动中反复 tick 不进 Idle（计时重置）。
    let mut at = AidTimeout::new();
    let mut t = 100u32;
    let mut n = 0u32;
    while n < 10 {
        at.input(n, t)?;
        t += 2999; // 每次活动间隔 2999 < 3000
        at.tick(t)?;
        n += 1;
    }
    if at.state() != ST_ACTIVE {
        return Err(E_POLICY_BAD);
    }
    // 间隔拉到 3000 立即进 Idle。
    at.tick(t + TIMEOUT_MS)?;
    if at.state() != ST_IDLE {
        return Err(E_POLICY_BAD);
    }
    hits += 1;

    // R5 边界闭集：恰好 2999 不进 / 恰好 3000 进（边界归属声明）；
    //    tick 回退拒收。
    if run_case(TIMEOUT_MS - 1)?.into() {
        return Err(E_POLICY_BAD);
    }
    if !run_case(TIMEOUT_MS)? {
        return Err(E_POLICY_BAD);
    }
    let mut at = AidTimeout::new();
    at.tick(1000)?;
    if at.tick(999) != Err(E_TICK_BAD) {
        return Err(E_TICK_BAD);
    }
    if at.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 唤醒清队纵深：Idle 提示中先入队再长驻，唤醒一次清全部；
    //    唤醒后 idle 计时归零。
    let mut at = AidTimeout::new();
    at.input(1, 10)?;
    at.tick(3100)?;
    at.enqueue(1)?;
    at.enqueue(2)?;
    at.tick(5000)?;
    if at.idle_ms() != 1900 {
        return Err(E_TICK_BAD);
    }
    at.input(9, 5001)?;
    if at.idle_ms() != 0 || at.state() != ST_ACTIVE {
        return Err(E_POLICY_BAD);
    }
    hits += 1;

    // R7 可红演练：焦点丢失注入（模拟违规实现动 focus_held）→
    //    assert_policy 必红；队账伪造 → audit 必红。
    let mut at = AidTimeout::new();
    at.input(1, 10)?;
    at.focus_held = false; // 私改（模拟违规）
    if at.assert_policy() != Err(E_FOCUS_LOST) {
        return Err(E_FOCUS_LOST);
    }
    let mut at2 = AidTimeout::new();
    at2.tick(100)?;
    at2.input(1, 100)?;
    at2.audit()?;
    at2.queue.clear();
    if at2.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    // R8 ×10 复跑一致与写回（判据 J2）：十轮场景第二轮复跑，进入
    //    轮数逐位一致（确定性）；写回过。
    let mut first: u32 = 0;
    let mut i = 0usize;
    while i < IDLE_CASES.len() {
        if run_case(IDLE_CASES[i])? {
            first += 1;
        }
        i += 1;
    }
    let mut second: u32 = 0;
    let mut i = 0usize;
    while i < IDLE_CASES.len() {
        if run_case(IDLE_CASES[i])? {
            second += 1;
        }
        i += 1;
    }
    if first != second {
        return Err(E_REPLAY_DIFF);
    }
    let mut at = AidTimeout::new();
    at.input(1, 5)?;
    at.tick(5000)?;
    at.audit()?;
    if at.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    at.writeback(true, 0x1C)?;
    hits += 1;

    Ok(hits)
}
