//! UNX-F0128 · NMI 备份看门狗 · 诊断段 0x2460
//! 验收锚点：UNX-F0128-J1 喂狗窗口闭集内必喂、超窗 NMI 告警、
//! 误报/漏报双判据。零浮点、零 unwrap、零 expect。


use alloc::vec::Vec;
use alloc::format;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x2460;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_WINDOW: u16 = SEG_BASE + 1; // 喂狗窗口非法（零或超上限）
const E_LATE: u16 = SEG_BASE + 2;   // 超窗喂狗（NMI 已告警后的迟到喂）
const E_ARMED: u16 = SEG_BASE + 3;  // 未启动即喂狗
const E_TWICE: u16 = SEG_BASE + 4;  // 同窗口双告警（一轮一告警）
const E_RDONLY: u16 = SEG_BASE + 5; // 看门狗已停，再操作被拒
const E_BAL: u16 = SEG_BASE + 6;    // 喂狗账不平

pub fn detail(code: u16) -> &'static str {
    match code {
        E_WINDOW => "窗口非法：喂狗周期须为正且不超 60 秒",
        E_LATE => "超窗喂狗：NMI 已告警后喂狗属迟到",
        E_ARMED => "未启动喂狗：看门狗未armed",
        E_TWICE => "同窗口双告警：一超窗一告警",
        E_RDONLY => "看门狗已停：操作被拒",
        E_BAL => "喂狗账不平：喂入数 ≠ 准时数 + 迟到数",
        _ => "未知诊断码",
    }
}

pub const WINDOW_MAX_MS: u64 = 60_000;

/// 喂狗事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FeedVerdict {
    OnTime,
    Late { alarm_fired: bool },
}

/// NMI 备份看门狗：软喂狗 + 超窗告警账。
pub struct NmiWatch {
    pub window_ms: u64,
    pub armed: bool,
    pub stopped: bool,
    pub last_feed_ms: u64,
    pub feeds_on_time: u64,
    pub feeds_late: u64,
    pub alarms: u64,
    pub in_stall: bool, // 停摆 episode 中（连续迟到不重复告警）
    pub total_feeds: u64, // 总喂入（守恒基准）
}

impl NmiWatch {
    pub fn new(window_ms: u64) -> Result<NmiWatch, u16> {
        if window_ms == 0 || window_ms > WINDOW_MAX_MS {
            return Err(E_WINDOW);
        }
        Ok(NmiWatch {
            window_ms,
            armed: false,
            stopped: false,
            last_feed_ms: 0,
            feeds_on_time: 0,
            feeds_late: 0,
            alarms: 0,
            in_stall: false,
            total_feeds: 0,
        })
    }

    /// 启动：首次喂狗即 armed。
    pub fn start(&mut self, now_ms: u64) -> Result<(), u16> {
        if self.stopped {
            return Err(E_RDONLY);
        }
        if self.armed {
            return Err(E_TWICE);
        }
        self.armed = true;
        self.last_feed_ms = now_ms;
        Ok(())
    }

    /// 喂狗：距上次 ≤ 窗口为准时；超窗则触发 NMI 告警并记迟到。
    pub fn feed(&mut self, now_ms: u64) -> Result<FeedVerdict, u16> {
        if self.stopped {
            return Err(E_RDONLY);
        }
        if !self.armed {
            return Err(E_ARMED);
        }
        let delta = now_ms.saturating_sub(self.last_feed_ms);
        if delta <= self.window_ms {
            self.feeds_on_time += 1;
            self.total_feeds += 1;
            self.in_stall = false;
            self.last_feed_ms = now_ms;
            return Ok(FeedVerdict::OnTime);
        }
        // 超窗：NMI 告警按停摆 episode 去重（连续迟到只告警一次）。
        if !self.in_stall {
            self.alarms += 1;
            self.in_stall = true;
        }
        self.feeds_late += 1;
        self.total_feeds += 1;
        self.last_feed_ms = now_ms;
        Ok(FeedVerdict::Late { alarm_fired: true })
    }

    /// 停狗：终态。
    pub fn stop(&mut self) -> Result<(), u16> {
        if self.stopped {
            return Err(E_RDONLY);
        }
        self.stopped = true;
        Ok(())
    }

    /// 喂狗账守恒：准时 + 迟到 == 总喂。
    pub fn audit(&self) -> Result<(), u16> {
        if self.feeds_on_time + self.feeds_late != self.total_feeds {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 告警合理性：告警数 ≤ 迟到数（一轮一告警）。
    pub fn alarm_sane(&self) -> Result<(), u16> {
        if self.alarms > self.feeds_late {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if !self.armed {
            return Err(E_ARMED);
        }
        Ok(format!(
            "看门狗 {}ms 窗：准时 {} 迟到 {} NMI告警 {}",
            self.window_ms, self.feeds_on_time, self.feeds_late, self.alarms
        ))
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：窗口 100ms，准时节拍 50ms 喂 6 次 → 全准时零告警。
    let mut w = NmiWatch::new(100)?;
    w.start(0)?;
    for i in 1..=6u64 {
        let v = w.feed(i * 50)?;
        if v != FeedVerdict::OnTime {
            return Err(E_BAL);
        }
    }
    if w.alarms != 0 || w.feeds_late != 0 {
        return Err(E_BAL);
    }
    w.audit()?;
    w.alarm_sane()?;
    hits += 1;

    // R2：窗口 100ms，200ms 处喂 → 超窗告警 1 次并记迟到。
    let mut w2 = NmiWatch::new(100)?;
    w2.start(0)?;
    w2.feed(80)?;
    let v = w2.feed(280)?;
    match v {
        FeedVerdict::Late { alarm_fired } => {
            if !alarm_fired {
                return Err(E_BAL);
            }
        }
        _ => return Err(E_BAL),
    }
    if w2.alarms != 1 || w2.feeds_late != 1 {
        return Err(E_BAL);
    }
    w2.alarm_sane()?;
    hits += 1;

    // R3：同 episode 二次迟到不重复告警；准时喂恢复后新 episode 再告警。
    let mut w3 = NmiWatch::new(100)?;
    w3.start(0)?;
    w3.feed(250)?;  // 迟到 → 告警 1（进停摆）
    w3.feed(360)?;  // 连续迟到（110ms）→ 不重复告警
    if w3.alarms != 1 || w3.feeds_late != 2 {
        return Err(E_BAL);
    }
    w3.feed(400)?;  // 准时（40ms）→ 出停摆
    w3.feed(700)?;  // 再迟到 → 新 episode 告警 2
    if w3.alarms != 2 || w3.feeds_late != 3 || w3.feeds_on_time != 2 {
        return Err(E_BAL);
    }
    w3.alarm_sane()?;
    w3.audit()?;
    hits += 1;

    // R4：渲染账含三计数。
    let rep = w2.render()?;
    if !rep.contains("准时 1") || !rep.contains("迟到 1") || !rep.contains("NMI告警 1") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：窗口非法（零 / 超 60 秒）→ E_WINDOW。
    if NmiWatch::new(0).is_ok() {
        return Err(E_WINDOW);
    }
    if NmiWatch::new(WINDOW_MAX_MS + 1).is_ok() {
        return Err(E_WINDOW);
    }
    hits += 1;

    // I2：未启动喂狗 → E_ARMED。
    let mut i2 = NmiWatch::new(100)?;
    if i2.feed(10) != Err(E_ARMED) {
        return Err(E_ARMED);
    }
    if i2.render() != Err(E_ARMED) {
        return Err(E_ARMED);
    }
    hits += 1;

    // I3：重复启动 → E_TWICE。
    let mut i3 = NmiWatch::new(100)?;
    i3.start(0)?;
    if i3.start(10) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I4：停狗后再喂/再停 → E_RDONLY。
    i3.stop()?;
    if i3.feed(20) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    if i3.stop() != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I5：告警合理性破注入（告警数 > 迟到数）→ E_BAL。
    let mut i5 = NmiWatch::new(100)?;
    i5.start(0)?;
    i5.alarms = 5; // 人为多记告警
    if i5.alarm_sane() != Err(E_BAL) {
        return Err(E_BAL);
    }
    i5.feed(50)?;
    i5.total_feeds = 99; // 人为破守恒账
    if i5.audit() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
