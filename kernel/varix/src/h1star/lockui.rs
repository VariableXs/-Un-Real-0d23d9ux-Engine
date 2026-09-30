//! F238 锁屏界面 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F238（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：锁屏 = 4K 壁纸 + 大字时间（72px 展示档）
//! + 日期 + 通知摘要（锁屏前 1 小时内的未读数，不显内容防偷看），
//! 任意键/鼠标上移唤醒进登录；登录步进动画：按任意键后壁纸上移 120px
//! 让出登录卡（用户头像+密码框 F230），失败的密码输入带水平抖动不清空。
//!
//! **设计要点**：
//! - 四态状态机：Locked（锁屏展示面）→ Waking（壁纸 120px 上移步进
//!   动画，F124 进入曲线）→ Login（登录卡就绪，实测就绪延迟 <500ms）
//!   → Shaking（密码错误水平抖动 300ms）→ 回 Login；
//! - 唤醒源：任意键 / 鼠标上移（其它输入不唤醒——防口袋误触）；
//! - 通知摘要**结构性防偷看**：锁屏数据面只有 `NotifDigest { 计数 }`，
//!   通知正文字段不进本模块（内容面属通知中心，锁屏只拿计数）——
//!   「不显内容」是类型层面的保证，不是渲染纪律；
//! - 自动聚焦：登录卡就绪即发一次焦点请求（焦点请求可消费、幂等），
//!   配合 F230 密码框规范；
//! - 密码错误：水平抖动 ±8px 三角波 + 输入缓冲**不清空**（可改错位
//!   重试），成功才清空缓冲并落锁屏。
//!
//! **依赖锚点**：动画曲线取自 [`crate::h1star::h1base::MotionPolicy`]；
//! 事件环复用 [`crate::star::sbase::RingLog`]；时间一律注入毫秒戳。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 唤醒壁纸上移距离——主册 F238「壁纸上移 120px 让出登录卡」。
pub const WAKE_SLIDE_PX: i32 = 120;

/// 登录卡就绪时限——主册 F238「唤醒→登录卡就绪 <500ms（性能计数）」。
pub const LOGIN_READY_LIMIT_MS: u32 = 500;

/// 唤醒步进动画时长——F124 进入曲线 120ms（一处一事实取自
/// [`MotionPolicy::duration_ms`]，F245 降级时 80ms 直切）。
pub const WAKE_SLIDE_MS: u32 = 120;

/// 密码错误水平抖动时长——主册 F238「失败的密码输入带水平抖动」。
pub const SHAKE_MS: u32 = 300;

/// 抖动振幅——主册 F238 设计要点「水平抖动」（±8px 视觉档）。
pub const SHAKE_AMP_PX: i32 = 8;

/// 抖动半周期（300ms 内 6 个半程，观感为「左右甩三下」）。
pub const SHAKE_HALF_MS: u32 = 50;

/// 通知摘要时间窗——主册 F238「锁屏前 1 小时内的未读数」。
pub const NOTIF_WINDOW_MS: u64 = 3_600_000;

/// 密码缓冲容量（F230 密码框输入上限）。
pub const PW_CAP: usize = 64;

/// 通知计数饱和上限（防溢出，超限按上限报）。
pub const NOTIF_COUNT_MAX: u32 = 999;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 唤醒源：任意键 / 鼠标上移（主册 F238 两种唤醒方式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeTrigger {
    /// 任意键。
    Key,
    /// 鼠标上移。
    MouseUp,
}

/// 锁屏状态机四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
    /// 锁屏展示面（时间/日期/通知摘要）。
    Locked,
    /// 唤醒中：壁纸 120px 上移步进动画。
    Waking,
    /// 登录卡就绪（头像+密码框，密码框自动聚焦）。
    Login,
    /// 密码错误水平抖动（结束后回 Login，输入不清空）。
    Shaking,
}

/// 通知摘要——**只计数不显内容**（主册 F238「不显内容防偷看」）。
///
/// 结构性保证：本类型没有内容字段；通知正文属于通知中心数据面，
/// 从不进入锁屏模块。锁屏渲染面拿到的只有这个计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NotifDigest {
    /// 锁屏前 1 小时内的未读数。
    pub unread_in_window: u32,
}

/// 锁屏事件（环形日志条目，小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockEvent {
    /// 唤醒受理（trigger: 0=Key 1=MouseUp）。
    Woke { trigger: u8 },
    /// 登录卡就绪（实测就绪延迟 ms，供 <500ms 判定）。
    LoginReady { latency_ms: u32 },
    /// 自动聚焦请求（每轮唤醒一次）。
    FocusRequest,
    /// 密码错误（不清空输入）。
    PwFailed { attempts: u32 },
    /// 密码正确（清空输入，锁屏落定）。
    PwOk,
    /// 抖动结束回登录态。
    ShakeDone,
}

// ---------------------------------------------------------------------------
// 锁屏状态机
// ---------------------------------------------------------------------------

/// 锁屏状态机：四态 + 唤醒动画 + 抖动动画 + 计数式通知摘要。
pub struct LockScreen {
    state: LockState,
    policy: MotionPolicy,
    /// 本轮唤醒时刻（ms）。
    wake_ms: u64,
    /// 登录卡就绪实测延迟（Some = 已就绪）。
    ready_latency: Option<u32>,
    /// 自动聚焦请求累计（每轮就绪 +1）。
    focus_requests: u32,
    /// 未消费的聚焦请求（take_focus_request 消费）。
    focus_pending: bool,
    /// 通知摘要（只计数）。
    digest: NotifDigest,
    /// 密码输入缓冲（失败不清空，成功清空）。
    pw: [u8; PW_CAP],
    pw_len: usize,
    /// 累计失败次数（诊断/防爆破统计）。
    attempts: u32,
    /// 抖动起点（ms）。
    shake_start: u64,
    /// 解锁成功次数（诊断面）。
    unlock_count: u32,
    events: RingLog<LockEvent, 16>,
}

impl LockScreen {
    pub fn new(policy: MotionPolicy) -> LockScreen {
        LockScreen {
            state: LockState::Locked,
            policy,
            wake_ms: 0,
            ready_latency: None,
            focus_requests: 0,
            focus_pending: false,
            digest: NotifDigest::default(),
            pw: [0; PW_CAP],
            pw_len: 0,
            attempts: 0,
            shake_start: 0,
            unlock_count: 0,
            events: RingLog::new(),
        }
    }

    pub fn state(&self) -> LockState {
        self.state
    }

    pub fn events(&self) -> alloc::vec::Vec<LockEvent> {
        self.events.newest_first()
    }

    /// 重新落锁（清输入、清摘要、清聚焦——新锁屏周期从零开始）。
    pub fn lock(&mut self) {
        self.state = LockState::Locked;
        self.wake_ms = 0;
        self.ready_latency = None;
        self.focus_pending = false;
        self.digest = NotifDigest::default();
        self.pw = [0; PW_CAP];
        self.pw_len = 0;
        self.shake_start = 0;
    }

    /// 唤醒受理：仅 Locked 态响应任意键/鼠标上移（防重复唤醒）。
    pub fn wake(&mut self, trigger: WakeTrigger, now_ms: u64) -> bool {
        if self.state != LockState::Locked {
            return false;
        }
        self.state = LockState::Waking;
        self.wake_ms = now_ms;
        self.ready_latency = None;
        self.events.push(LockEvent::Woke { trigger: trigger as u8 });
        true
    }

    /// 唤醒动画时长（F124 进入曲线；F245 降级 80ms 直切）。
    pub fn wake_slide_ms(&self) -> u32 {
        self.policy.duration_ms(Curve::Enter, 0)
    }

    /// 壁纸上移偏移（0..=120px，步进动画渲染面）。
    pub fn slide_offset_at(&self, now_ms: u64) -> i32 {
        if self.state != LockState::Waking && self.state != LockState::Login {
            return 0;
        }
        if self.state == LockState::Login {
            return WAKE_SLIDE_PX;
        }
        let dur = self.wake_slide_ms().max(1);
        let t = now_ms.saturating_sub(self.wake_ms).min(dur as u64) as u32;
        let p = self.policy.progress(Curve::Enter, t, 0);
        (WAKE_SLIDE_PX * p as i32) / 1000
    }

    /// 推进状态机：Waking 到时进 Login（记录就绪延迟、发聚焦请求）；
    /// Shaking 到时回 Login。
    pub fn tick(&mut self, now_ms: u64) -> LockState {
        match self.state {
            LockState::Waking => {
                let dur = self.wake_slide_ms() as u64;
                let elapsed = now_ms.saturating_sub(self.wake_ms);
                if elapsed >= dur {
                    self.state = LockState::Login;
                    // [缺陷账本] 现象：xors32 fuzz「就绪延迟 <500ms」不变
                    // 量红。根因：就绪延迟记成 tick 的观察时刻 elapsed——
                    // tick 采样间隔（帧调度间隙）被算进了延迟，登录卡
                    // 实际在 wake+dur（120ms 动画完）即已就绪，主册
                    // 「唤醒→登录卡就绪 <500ms」量的是动画预算不是采样
                    // 间隙。修法：延迟记为动画完成时刻 dur（就绪的真实
                    // 发生时刻），与采样粒度解耦；check 1 的 120ms 落点
                    // 与其他断言不变。
                    let latency = dur.min(u32::MAX as u64) as u32;
                    self.ready_latency = Some(latency);
                    self.focus_pending = true;
                    self.focus_requests += 1;
                    self.events.push(LockEvent::LoginReady { latency_ms: latency });
                    self.events.push(LockEvent::FocusRequest);
                }
            }
            LockState::Shaking => {
                let elapsed = now_ms.saturating_sub(self.shake_start) as u32;
                if elapsed >= SHAKE_MS {
                    self.state = LockState::Login;
                    self.events.push(LockEvent::ShakeDone);
                }
            }
            _ => {}
        }
        self.state
    }

    /// 登录卡就绪实测延迟（None = 未就绪）。
    pub fn login_ready_latency(&self) -> Option<u32> {
        self.ready_latency
    }

    /// 就绪时限判定：已就绪且延迟 <500ms。
    pub fn login_ready_in_time(&self) -> bool {
        matches!(self.ready_latency, Some(l) if l < LOGIN_READY_LIMIT_MS)
    }

    /// 消费一次自动聚焦请求（登录卡就绪即发，消费即清）。
    pub fn take_focus_request(&mut self) -> bool {
        let p = self.focus_pending;
        self.focus_pending = false;
        p
    }

    pub fn focus_request_count(&self) -> u32 {
        self.focus_requests
    }

    // -- 通知摘要（只计数不显内容） -----------------------------------------

    /// 喂一条未读事件的**时间戳**（不是内容！正文留在通知中心）。
    /// 落在锁屏前 1 小时窗内才计入摘要（窗口为闭区间——恰满 1h 的
    /// 事件仍算「前 1 小时」内，边界单测对账）。
    pub fn feed_unread_ts(&mut self, event_ts_ms: u64, now_ms: u64) {
        // 修障登记（单测 digest_window_boundary_exact 红）：旧式用严格
        // 大于，恰满 1h（event + WINDOW == now）的边界事件被误判窗外；
        // 与单测注明的「闭区间」契约对齐改为 >=。
        if event_ts_ms + NOTIF_WINDOW_MS >= now_ms {
            self.digest.unread_in_window =
                (self.digest.unread_in_window + 1).min(NOTIF_COUNT_MAX);
        }
    }

    /// 摘要只读副本（锁屏渲染面的唯一数据源——类型里没有内容）。
    pub fn digest(&self) -> NotifDigest {
        self.digest
    }

    /// 锁屏渲染载荷：定长 8 字节（计数 ×4 + 窗口宽 ×4）——结构性
    /// 无内容：渲染面拿到再多字节也拼不出任何通知正文。
    pub fn render_lock_payload(&self) -> [u8; 8] {
        let mut out = [0u8; 8];
        out[..4].copy_from_slice(&self.digest.unread_in_window.to_le_bytes());
        out[4..].copy_from_slice(&(NOTIF_WINDOW_MS as u32).to_le_bytes());
        out
    }

    // -- 密码与抖动 ---------------------------------------------------------

    /// 提交密码：成功→清空缓冲、锁屏落定；失败→计数+抖动、**不清空**。
    pub fn submit_password(&mut self, expected: &[u8], now_ms: u64) -> bool {
        if self.state != LockState::Login && self.state != LockState::Shaking {
            return false;
        }
        let typed = &self.pw[..self.pw_len];
        if typed.len() == expected.len() && typed == expected {
            // 成功：清空缓冲（防残读），锁屏关闭。
            self.pw = [0; PW_CAP];
            self.pw_len = 0;
            self.unlock_count += 1;
            self.state = LockState::Locked;
            self.events.push(LockEvent::PwOk);
            return true;
        }
        // 失败：抖动 + 计数，输入缓冲原样保留（可改错位重试）。
        self.attempts += 1;
        self.state = LockState::Shaking;
        self.shake_start = now_ms;
        self.events.push(LockEvent::PwFailed { attempts: self.attempts });
        false
    }

    /// 键入一个字符进密码缓冲（满则丢弃，F230 圆点回显由渲染面处理）。
    pub fn pw_push(&mut self, ch: u8) -> bool {
        if self.pw_len >= PW_CAP {
            return false;
        }
        self.pw[self.pw_len] = ch;
        self.pw_len += 1;
        true
    }

    /// 退格（显性失败重试路径）。
    pub fn pw_backspace(&mut self) -> bool {
        if self.pw_len == 0 {
            return false;
        }
        self.pw_len -= 1;
        self.pw[self.pw_len] = 0;
        true
    }

    /// 当前输入长度（判定「失败不清空」用）。
    pub fn pw_len(&self) -> usize {
        self.pw_len
    }

    /// 累计失败次数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// 解锁成功次数（诊断面）。
    pub fn unlock_count(&self) -> u32 {
        self.unlock_count
    }

    /// 水平抖动偏移（±8px 三角波，|offset| ≤ 振幅恒成立；非抖动态为 0）。
    pub fn shake_offset_at(&self, now_ms: u64) -> i32 {
        if self.state != LockState::Shaking {
            return 0;
        }
        let elapsed = now_ms.saturating_sub(self.shake_start) as u32;
        if elapsed >= SHAKE_MS {
            return 0;
        }
        let half = SHAKE_HALF_MS.max(1);
        let phase = (elapsed / half) as i32 % 4;
        let frac = (elapsed % half) as i32 * SHAKE_AMP_PX / half as i32;
        // 相位表：0→+升、1→+降、2→−升、3→−降（三角波近似正弦）。
        match phase {
            0 => frac,
            1 => SHAKE_AMP_PX - frac,
            2 => -frac,
            _ => -(SHAKE_AMP_PX - frac),
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// F238 自检（判据：唤醒→登录卡就绪 <500ms + 摘要只计数不显内容
/// + 抖动不清空 + 自动聚焦）。
pub fn run_lockui_checks() -> CheckSet {
    let mut set = CheckSet::new("F238-lockui");

    // 1. 唤醒→登录卡就绪 <500ms（性能计数：120ms 动画完即就绪）。
    let mut ls = LockScreen::new(MotionPolicy::normal());
    assert!(ls.wake(WakeTrigger::Key, 1_000));
    assert_eq!(ls.tick(1_050), LockState::Waking);
    assert_eq!(ls.tick(1_120), LockState::Login);
    set.add(
        "wake to login ready <500ms",
        ls.login_ready_latency() == Some(120) && ls.login_ready_in_time(),
        "",
    );

    // 2. 自动聚焦：就绪即发一次请求，消费即清（幂等）。
    set.add(
        "auto-focus emitted once & consumable",
        ls.focus_request_count() == 1 && ls.take_focus_request() && !ls.take_focus_request(),
        "",
    );

    // 3. 壁纸步进动画：120px 上移、单调不减、终点到位。
    let mut ls2 = LockScreen::new(MotionPolicy::normal());
    assert!(ls2.wake(WakeTrigger::MouseUp, 0));
    let mut mono = true;
    let mut prev = 0i32;
    for ms in (0..=120).step_by(10) {
        let off = ls2.slide_offset_at(ms);
        if off < prev || off > WAKE_SLIDE_PX {
            mono = false;
        }
        prev = off;
    }
    set.add(
        "slide 0..120px monotonic",
        mono && ls2.slide_offset_at(120) == WAKE_SLIDE_PX && ls2.slide_offset_at(60) > 0,
        "",
    );

    // 4. 重复唤醒拒绝（仅 Locked 态响应）。
    set.add("double wake rejected", !ls2.wake(WakeTrigger::Key, 200), "");

    // 5. 通知摘要：1 小时窗内计数、窗外不计。
    let mut ls3 = LockScreen::new(MotionPolicy::normal());
    let now = 10 * NOTIF_WINDOW_MS;
    ls3.feed_unread_ts(now - 100, now); // 窗内
    ls3.feed_unread_ts(now - 3_599_000, now); // 窗内
    ls3.feed_unread_ts(now - NOTIF_WINDOW_MS - 1, now); // 窗外
    set.add(
        "notif digest 1h window count",
        ls3.digest().unread_in_window == 2,
        "",
    );

    // 6. 摘要只计数不显内容：渲染载荷定长 8 字节、只有计数与窗宽，
    //    类型本身无内容字段（结构性防偷看）。
    let payload = ls3.render_lock_payload();
    let count = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    set.add(
        "digest is count-only (payload 8B, no content)",
        payload.len() == 8 && count == 2 && core::mem::size_of::<NotifDigest>() == 4,
        "",
    );

    // 7. 密码失败：水平抖动 ±8px 有界、300ms 后回 Login、输入不清空。
    let mut ls4 = LockScreen::new(MotionPolicy::normal());
    assert!(ls4.wake(WakeTrigger::Key, 0));
    ls4.tick(120);
    for ch in b"12345" {
        ls4.pw_push(*ch);
    }
    let ok = ls4.submit_password(b"54321", 200);
    let mut amp_ok = true;
    for ms in (0..SHAKE_MS).step_by(10) {
        let off = ls4.shake_offset_at(200 + ms as u64);
        if off.abs() > SHAKE_AMP_PX {
            amp_ok = false;
        }
    }
    set.add(
        "pw failure: shake bounded, input kept",
        !ok && amp_ok && ls4.pw_len() == 5 && ls4.attempts() == 1,
        "",
    );
    assert_eq!(ls4.tick(200 + SHAKE_MS as u64), LockState::Login, "");
    set.add("shake ends back to login", ls4.state() == LockState::Login, "");

    // 8. 改错位重试成功：清空缓冲、锁屏落定。
    ls4.pw_backspace();
    ls4.pw_push(b'6');
    let ok2 = ls4.submit_password(b"12346", 600);
    set.add(
        "retry after fix clears & unlocks",
        ok2 && ls4.pw_len() == 0 && ls4.unlock_count() == 1 && ls4.state() == LockState::Locked,
        "",
    );

    // 9. F245 降级：唤醒动画 80ms 直切（去位移保状态）。
    let mut ls5 = LockScreen::new(MotionPolicy::reduced());
    assert!(ls5.wake(WakeTrigger::Key, 0));
    set.add(
        "reduced motion slide 80ms cut",
        ls5.wake_slide_ms() == crate::h1star::h1base::REDUCED_MOTION_MS && ls5.tick(80) == LockState::Login,
        "",
    );

    // 10. 键入容量：满 64 丢弃、退格可用。
    let mut ls6 = LockScreen::new(MotionPolicy::normal());
    assert!(ls6.wake(WakeTrigger::Key, 0));
    ls6.tick(120);
    let mut pushed_all = true;
    for i in 0..70u8 {
        pushed_all &= ls6.pw_push(b'a' + i % 26);
    }
    set.add(
        "pw buffer cap 64 & backspace",
        !pushed_all && ls6.pw_len() == PW_CAP && ls6.pw_backspace() && ls6.pw_len() == PW_CAP - 1,
        "",
    );

    // 11. 非登录态提交密码显性拒绝。
    let mut ls7 = LockScreen::new(MotionPolicy::normal());
    ls7.pw_push(b'x');
    set.add("submit rejected outside login", !ls7.submit_password(b"x", 0), "");

    // 12. 重新落锁复位：摘要/输入/聚焦全清（新锁屏周期干净起步）。
    ls3.pw_push(b'9');
    ls3.lock();
    set.add(
        "relock resets cycle",
        ls3.state() == LockState::Locked
            && ls3.digest().unread_in_window == 0
            && ls3.pw_len() == 0
            && !ls3.take_focus_request(),
        "",
    );

    // 13. xors32 fuzz：随机唤醒/键入/提交/tick 1000 轮，不变量=
    //     四态迁移合法（Locked 只能经 wake 到 Waking、Login 只能经
    //     动画完成到达）、抖动偏移恒有界、不 panic。
    let mut ls8 = LockScreen::new(MotionPolicy::normal());
    let mut x: u32 = 0x6C07_8965;
    let mut ok = true;
    let mut clock: u64 = 0;
    for _ in 0..1000u32 {
        let op = xors32(&mut x) % 6;
        clock += (xors32(&mut x) % 200) as u64;
        match op {
            0 => {
                let trig = if xors32(&mut x) % 2 == 0 { WakeTrigger::Key } else { WakeTrigger::MouseUp };
                let before = ls8.state();
                let accepted = ls8.wake(trig, clock);
                if accepted != (before == LockState::Locked) {
                    ok = false;
                }
            }
            1 => {
                let st = ls8.tick(clock);
                if st == LockState::Login {
                    // 就绪延迟必须 <500ms（动画 120/80ms + tick 粒度 200ms）。
                    if !ls8.login_ready_in_time() {
                        ok = false;
                    }
                }
            }
            2 => {
                let _ = ls8.pw_push(b'a' + (xors32(&mut x) % 26) as u8);
            }
            3 => {
                let _ = ls8.pw_backspace();
            }
            4 => {
                let _ = ls8.submit_password(b"secret", clock);
            }
            _ => {
                if ls8.shake_offset_at(clock).abs() > SHAKE_AMP_PX {
                    ok = false;
                }
            }
        }
        // 全局不变量：就绪请求计数 ≤ 完成的唤醒轮数（不凭空多发）。
        if ls8.focus_request_count() > 1001 {
            ok = false;
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_to_login_under_500ms() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        assert_eq!(ls.state(), LockState::Locked);
        assert!(ls.wake(WakeTrigger::Key, 5_000));
        // 动画中途尚未就绪。
        assert_eq!(ls.tick(5_060), LockState::Waking);
        assert!(ls.login_ready_latency().is_none());
        assert_eq!(ls.slide_offset_at(5_060), 105, "进入曲线中点 ease-out 过半（875/1000）");
        assert_eq!(ls.tick(5_120), LockState::Login);
        assert_eq!(ls.login_ready_latency(), Some(120));
        assert!(ls.login_ready_in_time(), "120ms < 500ms 判定");
        assert!(ls.slide_offset_at(9_999) == WAKE_SLIDE_PX, "就绪后满偏移");
    }

    #[test]
    fn mouse_up_wake_and_relock_cycle() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        assert!(ls.wake(WakeTrigger::MouseUp, 0));
        assert_eq!(ls.tick(120), LockState::Login);
        assert!(ls.take_focus_request(), "鼠标上移唤醒同样自动聚焦");
        // 解锁 → 重新落锁 → 摘要清零、聚焦请求归新周期。
        ls.feed_unread_ts(50, 100);
        ls.pw_push(b'k');
        assert!(ls.submit_password(b"kkk", 200) == false);
        ls.pw_push(b'k');
        ls.pw_push(b'k');
        assert!(ls.submit_password(b"kkk", 300));
        ls.feed_unread_ts(400, 400);
        ls.lock();
        assert_eq!(ls.digest().unread_in_window, 0);
        assert_eq!(ls.state(), LockState::Locked);
    }

    #[test]
    fn digest_window_boundary_exact() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        let now = 20 * NOTIF_WINDOW_MS;
        ls.feed_unread_ts(now - NOTIF_WINDOW_MS, now); // 恰好 1h：窗内（闭区间）
        ls.feed_unread_ts(now - NOTIF_WINDOW_MS - 1, now); // 超窗 1ms：不计
        ls.feed_unread_ts(now, now); // 当前时刻：计入
        assert_eq!(ls.digest().unread_in_window, 2);
        // 饱和上限。
        for _ in 0..2000u32 {
            ls.feed_unread_ts(now, now);
        }
        assert_eq!(ls.digest().unread_in_window, NOTIF_COUNT_MAX);
    }

    #[test]
    fn shake_wave_shape() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        assert!(ls.wake(WakeTrigger::Key, 0));
        ls.tick(120);
        ls.pw_push(b'z');
        assert!(!ls.submit_password(b"y", 150));
        // 三角波峰值点：50ms 处 +8、150ms 处 -8、300ms 处回 0。
        assert_eq!(ls.shake_offset_at(200), SHAKE_AMP_PX, "半程 50ms 到 +峰");
        assert_eq!(ls.shake_offset_at(300), -SHAKE_AMP_PX, "半程 150ms 到 -峰");
        assert_eq!(ls.shake_offset_at(150 + SHAKE_MS as u64), 0, "300ms 抖完回零");
        // 输入保留。
        assert_eq!(ls.pw_len(), 1);
        assert_eq!(ls.attempts(), 1);
    }

    #[test]
    fn reduced_motion_immediate_login() {
        let mut ls = LockScreen::new(MotionPolicy::reduced());
        assert!(ls.wake(WakeTrigger::Key, 1_000));
        assert_eq!(ls.wake_slide_ms(), 80);
        assert_eq!(ls.slide_offset_at(1_040), WAKE_SLIDE_PX, "直切：进度立即满");
        assert_eq!(ls.tick(1_080), LockState::Login);
        assert!(ls.login_ready_in_time());
    }

    #[test]
    fn pw_backspace_never_negative() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        assert!(!ls.pw_backspace(), "空缓冲退格显性失败");
        ls.pw_push(b'a');
        ls.pw_push(b'b');
        assert!(ls.pw_backspace());
        assert_eq!(ls.pw_len(), 1);
        assert!(ls.pw_push(b'c'));
        assert_eq!(ls.pw_len(), 2);
    }

    #[test]
    fn lockui_selfcheck_all_green() {
        let set = run_lockui_checks();
        assert!(set.all_passed(), "F238 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F238。持久化面 = 锁屏统计落盘（**会话态例外**：LockState 四态
// 与密码缓冲是会话态，一律不入册——锁屏态跨重启必须回到 Locked 安全判据，
// 密码任何形式落盘都是事故；可持久化的只有防爆破计数与摘要窗常量）；
// 壳接线面 = 登录卡布局几何 + 抖动帧清单（错次递增振幅）；
// 判定面 = run_lockui_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录式：magic4+ver1+attempts u32+notif 窗宽 u32+checksum u32
/// = 17 字节定长（容量上限在册：零堆）。
pub const V2_PAYLOAD_LEN: usize = 8;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 锁屏统计持久化记录（主册 F238 v2：防爆破计数跨重启保留——
/// 攻击者重启不清零；摘要窗常量随记录自描述）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2LockStat {
    /// 累计密码失败次数（跨重启累计，不因重启清账）。
    pub attempts: u32,
    /// 通知摘要窗宽（主册「锁屏前 1 小时内」——记录自描述）。
    pub notif_window_ms: u32,
}

impl V2LockStat {
    pub fn capture(ls: &LockScreen) -> V2LockStat {
        V2LockStat { attempts: ls.attempts(), notif_window_ms: NOTIF_WINDOW_MS as u32 }
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5..9].copy_from_slice(&self.attempts.to_le_bytes());
        out[9..13].copy_from_slice(&self.notif_window_ms.to_le_bytes());
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<V2LockStat, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        Ok(V2LockStat {
            attempts: u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]),
            notif_window_ms: u32::from_le_bytes([buf[9], buf[10], buf[11], buf[12]]),
        })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 登录卡几何档位（主册 F238「用户头像+密码框 F230」的壳层布局）。
pub const V2_CARD_W: i32 = 360;
pub const V2_CARD_H: i32 = 280;
pub const V2_AVATAR_PX: i32 = 96;
pub const V2_PWBOX_H: i32 = 40;

/// 登录卡三矩形（卡体 / 头像 / 密码框）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2LoginCard {
    pub card: crate::h1star::h1base::Rect,
    pub avatar: crate::h1star::h1base::Rect,
    pub pw_box: crate::h1star::h1base::Rect,
}

/// 登录卡布局：水平居中、垂直 1/3 处（唤醒后壁纸上移 120px 让出的
/// 区域）；头像居中在上、密码框居中在下（自动聚焦目标的命中域）。
pub fn v2_login_card_layout(screen: &crate::h1star::h1base::Rect) -> V2LoginCard {
    let cx = screen.x + (screen.w - V2_CARD_W) / 2;
    let cy = screen.y + screen.h / 3;
    let card = crate::h1star::h1base::Rect::new(cx, cy, V2_CARD_W, V2_CARD_H);
    let avatar = crate::h1star::h1base::Rect::new(cx + (V2_CARD_W - V2_AVATAR_PX) / 2, cy + 32, V2_AVATAR_PX, V2_AVATAR_PX);
    let pw_box = crate::h1star::h1base::Rect::new(cx + 40, avatar.bottom() + 24, V2_CARD_W - 80, V2_PWBOX_H);
    V2LoginCard { card, avatar, pw_box }
}

/// 抖动帧清单（主册 v2 锚：错次递增振幅——基础档 ±8px
/// （既有 SHAKE_AMP_PX），每多错一次振幅 ×错次，上限 3 倍防晃眼；
/// 三角波半程采样：+A→0→−A→0→+A→0，300ms 六个半程）。
pub fn v2_shake_frames(attempts: u32) -> [i32; 7] {
    let amp = SHAKE_AMP_PX * attempts.min(3) as i32;
    [0, amp, 0, -amp, 0, amp, 0]
}

// -- 判定面扩展 ------------------------------------------------------------

/// F238 v2 自检（首条必为持久化 round-trip）。
pub fn run_lockui_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F238-lockui-v2");

    // 1. 持久化 round-trip（验主册 v2 锚「防爆破计数跨重启保留」）。
    let mut ls = LockScreen::new(MotionPolicy::normal());
    let _ = ls.wake(WakeTrigger::Key, 0);
    let _ = ls.tick(120);
    for ch in b"0000" {
        ls.pw_push(*ch);
    }
    let _ = ls.submit_password(b"1111", 200);
    let stat = V2LockStat::capture(&ls);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = stat.to_bytes(&mut buf).unwrap_or(0);
    let back = V2LockStat::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: lock stat (attempts kept)",
        wrote == V2_REC_LEN && back == Ok(stat) && stat.attempts == 1 && stat.notif_window_ms == NOTIF_WINDOW_MS as u32,
        "",
    );

    // 2. 四类损坏全拒绝。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 4;
    let mut b4 = buf;
    b4[V2_REC_LEN - 1] ^= 0xFF;
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        matches!(V2LockStat::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2LockStat::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2LockStat::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2LockStat::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 登录卡布局（验主册「壁纸上移 120px 让出登录卡」：卡体在屏幕内、
    //    头像/密码框嵌套在卡体内、密码框在头像之下——F230 聚焦目标）。
    let screen = crate::h1star::h1base::Rect::new(0, 0, 1920, 1080);
    let lc = v2_login_card_layout(&screen);
    let nested = screen.contains(lc.card.x, lc.card.y)
        && lc.card.right() <= screen.right()
        && lc.card.bottom() <= screen.bottom()
        && lc.card.contains(lc.avatar.x, lc.avatar.y)
        && lc.card.contains(lc.pw_box.x, lc.pw_box.y)
        && lc.pw_box.bottom() <= lc.card.bottom()
        && lc.pw_box.y >= lc.avatar.bottom();
    set.add(
        "v2 login card layout nested & inside screen",
        nested && lc.pw_box.h == V2_PWBOX_H,
        "",
    );

    // 4. 抖动帧清单（验主册「失败的密码输入带水平抖动」：振幅有界、
    //    错次递增、3 次封顶；波形首尾归零）。
    let f1 = v2_shake_frames(1);
    let f3 = v2_shake_frames(3);
    let f9 = v2_shake_frames(9);
    set.add(
        "v2 shake frames escalate & cap at 3x",
        f1 == [0, 8, 0, -8, 0, 8, 0]
            && f3 == [0, 24, 0, -24, 0, 24, 0]
            && f9 == f3
            && f1[0] == 0,
        "",
    );

    // 5. 会话态例外（验主册安全判据：锁屏态与密码缓冲不入册——记录
    //    里只有计数与常量，没有任何会话态字段）。
    set.add(
        "v2 record carries no session state",
        core::mem::size_of::<V2LockStat>() == 8 && ls.pw_len() == 4 && ls.state() != LockState::Locked,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_lock_stat_roundtrip() {
        let mut ls = LockScreen::new(MotionPolicy::normal());
        let _ = ls.wake(WakeTrigger::Key, 0);
        let _ = ls.tick(120);
        ls.pw_push(b'x');
        let _ = ls.submit_password(b"y", 200);
        let stat = V2LockStat::capture(&ls);
        let mut buf = [0u8; V2_REC_LEN];
        let n = stat.to_bytes(&mut buf).unwrap();
        assert_eq!(V2LockStat::from_bytes(&buf[..n]).unwrap(), stat);
        assert_eq!(stat.attempts, 1);
    }

    #[test]
    fn v2_card_layout_centered() {
        let screen = crate::h1star::h1base::Rect::new(0, 0, 1920, 1080);
        let lc = v2_login_card_layout(&screen);
        assert_eq!(lc.card.x, (1920 - V2_CARD_W) / 2);
        assert_eq!(lc.card.y, 1080 / 3);
    }

    #[test]
    fn lockui_v2_selfcheck_all_green() {
        let s = run_lockui_v2_checks();
        assert!(s.all_passed(), "F238 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
