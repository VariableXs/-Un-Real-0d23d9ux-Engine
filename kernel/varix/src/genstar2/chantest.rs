//! F480 扬声器声道测试（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **左右独立测试与屏幕高亮同步；默认 40% 音量；设备路由正确性；无声诊断
//! 提示（「左声道无声——检查接口或换设备」）。**
//!
//! 功能定义（主册批次三）：左/右声道独立发声测试（点击哪个响哪个——屏幕
//! 同步高亮对应侧）、双声道同时测试、音量归零保护（测试音量默认 40% 不炸
//! 耳）；测试不依赖网络；每声道路由验证（当前输出设备 F241 真实发声）。
//!
//! 零堆纪律：定长测试账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 测试音量默认值（主册：默认 40% 不炸耳）。
pub const TEST_VOLUME_PERMILLE: u16 = 400;
/// 声道路由验证超时（诊断「无声」的等待上限）。
pub const ROUTE_VERIFY_TIMEOUT_MS: u64 = 500;

/// 声道（主册：左/右独立 + 双声道同时）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Left,
    Right,
    Both,
}

impl Channel {
    /// 屏幕高亮侧（主册：屏幕同步高亮对应侧——双声道双侧同亮）。
    pub fn highlight(self) -> (&'static str, &'static str) {
        match self {
            Channel::Left => ("left-glow", ""),
            Channel::Right => ("", "right-glow"),
            Channel::Both => ("left-glow", "right-glow"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Channel::Left => "left",
            Channel::Right => "right",
            Channel::Both => "both",
        }
    }
}

/// 声道测试会话（路由正确性 + 无声诊断）。
pub struct ChannelTest {
    pub volume_permille: u16,
    /// 当前输出设备路由（F241 同源设备 id）。
    pub routed_device: u32,
    /// 测试账（每声道路由验证结论）。
    results: [Option<(Channel, bool)>; 8],
    n: usize,
}

impl ChannelTest {
    pub const fn new(routed_device: u32) -> Self {
        ChannelTest {
            volume_permille: TEST_VOLUME_PERMILLE,
            routed_device,
            results: [None; 8],
            n: 0,
        }
    }

    /// 音量归零保护：外部传入音量越界（>600 测试档）钳回 400（不炸耳红线）。
    pub fn set_test_volume(&mut self, permille: u16) -> u16 {
        self.volume_permille = if permille > 600 { TEST_VOLUME_PERMILLE } else { permille };
        self.volume_permille
    }

    /// 声道路由验证（主册：测试的就是当前在用的那个设备）。
    /// `device_ack`：声卡回报的发声设备 id（None = 超时无声）。
    pub fn verify_channel(&mut self, ch: Channel, device_ack: Option<u32>) -> bool {
        let ok = device_ack == Some(self.routed_device);
        if self.n < 8 {
            self.results[self.n] = Some((ch, ok));
            self.n += 1;
        }
        ok
    }

    /// 无声诊断提示（主册原文口径：「左声道无声——检查接口或换设备」）。
    pub fn silent_diagnosis(ch: Channel) -> &'static str {
        match ch {
            Channel::Left => "左声道无声——检查接口或换设备",
            Channel::Right => "右声道无声——检查接口或换设备",
            Channel::Both => "双声道无声——检查音量、接口或输出设备",
        }
    }

    /// 全声道验证完成审计（左右 both 三路都验过）。
    pub fn all_channels_verified(&self) -> bool {
        let want = [Channel::Left, Channel::Right, Channel::Both];
        want.iter().all(|w| {
            (0..self.n).any(|i| matches!(self.results[i], Some((c, ok)) if c == *w && ok))
        })
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_chantest_checks() -> CheckSet {
    let mut cs = CheckSet::new("F480-chantest");
    // 1) 默认 40% 音量。
    let mut t = ChannelTest::new(0x41);
    cs.add("default_40pct", t.volume_permille == 400, "");
    // 2) 音量归零保护（越界钳回 400）。
    cs.add("volume_clamped", t.set_test_volume(1_000) == 400 && t.set_test_volume(500) == 500, "");
    // 3) 左右独立测试与屏幕高亮同步。
    cs.add("left_highlight", Channel::Left.highlight() == ("left-glow", ""), "");
    cs.add("right_highlight", Channel::Right.highlight() == ("", "right-glow"), "");
    cs.add("both_highlight", Channel::Both.highlight() == ("left-glow", "right-glow"), "");
    // 4) 设备路由正确性（当前设备 ack 才算通过）。
    cs.add("route_ok", t.verify_channel(Channel::Left, Some(0x41)), "");
    cs.add("route_wrong_device_fails", !t.verify_channel(Channel::Right, Some(0x42)), "");
    cs.add("route_timeout_fails", !t.verify_channel(Channel::Both, None), "");
    // 5) 无声诊断提示（主册原文口径）。
    cs.add("silent_diag_left", ChannelTest::silent_diagnosis(Channel::Left) == "左声道无声——检查接口或换设备", "");
    cs.add("silent_diag_right", ChannelTest::silent_diagnosis(Channel::Right).contains("右声道"), "");
    // 6) 全声道验证审计（失败后重验可过）。
    cs.add("retry_then_all_pass", t.verify_channel(Channel::Right, Some(0x41)) && t.verify_channel(Channel::Both, Some(0x41)) && t.all_channels_verified(), "");
    // 7) 路由验证超时常量在册。
    cs.add("timeout_const", ROUTE_VERIFY_TIMEOUT_MS == 500, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_tests_loud() {
        let mut t = ChannelTest::new(7);
        // 测试档永远温和（>600 钳回 400）。
        for v in [700u16, 800, 1_000, u16::MAX] {
            t.set_test_volume(v);
            assert!(t.volume_permille <= 600);
        }
    }

    #[test]
    fn routing_must_match_current_device() {
        let mut t = ChannelTest::new(0xAA);
        assert!(!t.verify_channel(Channel::Left, Some(0xBB)), "测试了个寂寞 = 失败");
        assert!(t.verify_channel(Channel::Left, Some(0xAA)));
    }

    #[test]
    fn silent_side_gets_actionable_hint() {
        let msg = ChannelTest::silent_diagnosis(Channel::Right);
        assert!(msg.contains("右声道") && (msg.contains("接口") || msg.contains("换设备")));
    }
}

// ===========================================================================
// 深化 v2（F480）：测试音序列发生器 / 声道路由账 / 无声判定窗 / 结果报告
// ===========================================================================

/// 测试音配置（440Hz 标准音 + 500ms 时长——左右独立发声的声学参数）。
pub const TEST_TONE_HZ: u16 = 440;
pub const TEST_TONE_MS: u64 = 500;

/// 音序列发生器（左右交替节拍——双声道同时测试的时序面）。
pub struct ToneSeq {
    step: u8,
    steps_left: u8,
}

impl ToneSeq {
    pub const fn new(steps: u8) -> Self {
        ToneSeq { step: 0, steps_left: steps }
    }

    /// 下一发声声道（Left→Right→Both 循环；步尽 → None——测试有终点）。
    pub fn next_channel(&mut self) -> Option<Channel> {
        if self.steps_left == 0 {
            return None;
        }
        self.steps_left -= 1;
        let ch = match self.step % 3 {
            0 => Channel::Left,
            1 => Channel::Right,
            _ => Channel::Both,
        };
        self.step += 1;
        Some(ch)
    }

    pub fn remaining(&self) -> u8 {
        self.steps_left
    }
}

/// 声道路由账（每声道一次验证记录——「测试的就是当前在用的那个设备」
/// 的可审计面）。
pub struct RouteLog {
    entries: [(u8, u32, bool); 8], // (声道 id, 设备 ack, 是否匹配)
    n: usize,
}

impl RouteLog {
    pub const fn new() -> Self {
        RouteLog { entries: [(0, 0, false); 8], n: 0 }
    }

    pub fn record(&mut self, ch: Channel, ack_device: Option<u32>, expect_device: u32) {
        if self.n >= 8 {
            return;
        }
        let ch_id = match ch {
            Channel::Left => 0,
            Channel::Right => 1,
            Channel::Both => 2,
        };
        self.entries[self.n] = (ch_id, ack_device.unwrap_or(0), ack_device == Some(expect_device));
        self.n += 1;
    }

    pub fn all_matched(&self) -> bool {
        self.n >= 3 && (0..self.n).all(|i| self.entries[i].2)
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 无声判定窗（发声后 500ms 内无 ack = 无声——诊断提示的触发时钟）。
pub const SILENCE_WINDOW_MS: u64 = 500;

pub fn is_silent(played_at_ms: u64, ack_at_ms: Option<u64>) -> bool {
    match ack_at_ms {
        Some(t) => t.saturating_sub(played_at_ms) > SILENCE_WINDOW_MS,
        None => true,
    }
}

/// 结果报告结构（测试完成后的可读结论——三声道各一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TestReport {
    pub left_ok: bool,
    pub right_ok: bool,
    pub both_ok: bool,
    pub volume_permille: u16,
}

impl TestReport {
    pub fn all_pass(&self) -> bool {
        self.left_ok && self.right_ok && self.both_ok
    }

    /// 人话结论（全过/哪边无声——三要素口径）。
    pub fn conclusion(&self) -> &'static str {
        if self.all_pass() {
            "左右声道测试通过"
        } else if !self.left_ok {
            ChannelTest::silent_diagnosis(Channel::Left)
        } else if !self.right_ok {
            ChannelTest::silent_diagnosis(Channel::Right)
        } else {
            ChannelTest::silent_diagnosis(Channel::Both)
        }
    }
}

pub fn run_chantest_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F480-deep");
    // 音序列（左右双循环——步尽诚实终止）。
    cs.add("tone_seq_cycle", {
        let mut s = ToneSeq::new(6);
        let seq = [s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel()];
        seq == [Some(Channel::Left), Some(Channel::Right), Some(Channel::Both), Some(Channel::Left), Some(Channel::Right), Some(Channel::Both)]
            && s.next_channel().is_none()
            && s.remaining() == 0
    }, "");
    // 音参数（440Hz/500ms 在册——测试音不炸耳的声学锚）。
    cs.add("tone_params", TEST_TONE_HZ == 440 && TEST_TONE_MS == 500, "");
    // 路由账（三声道全匹配才算过——单边通不算通）。
    cs.add("route_log_all_pass", {
        let mut r = RouteLog::new();
        r.record(Channel::Left, Some(7), 7);
        r.record(Channel::Right, Some(7), 7);
        r.record(Channel::Both, Some(7), 7);
        r.all_matched()
    }, "");
    cs.add("route_log_one_fail", {
        let mut r = RouteLog::new();
        r.record(Channel::Left, Some(7), 7);
        r.record(Channel::Right, Some(9), 7); // ack 设备不对
        r.record(Channel::Both, Some(7), 7);
        !r.all_matched()
    }, "");
    // 无声判定窗（500ms 无 ack = 无声——诊断触发时钟）。
    cs.add("silence_window", is_silent(0, None) && !is_silent(0, Some(499)) && is_silent(0, Some(501)), "");
    // 结果报告（全过人话/左无声人话——三要素结论）。
    cs.add("report_all_pass", {
        TestReport { left_ok: true, right_ok: true, both_ok: true, volume_permille: 400 }.conclusion() == "左右声道测试通过"
    }, "");
    cs.add("report_left_silent", {
        TestReport { left_ok: false, right_ok: true, both_ok: true, volume_permille: 400 }.conclusion() == "左声道无声——检查接口或换设备"
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn seq_partial_run_stops_cleanly() {
        let mut s = ToneSeq::new(2);
        assert_eq!(s.next_channel(), Some(Channel::Left));
        assert_eq!(s.next_channel(), Some(Channel::Right));
        assert_eq!(s.next_channel(), None);
        assert_eq!(s.remaining(), 0);
    }

    #[test]
    fn route_log_caps_at_8() {
        let mut r = RouteLog::new();
        for _ in 0..10 {
            r.record(Channel::Left, Some(1), 1);
        }
        assert_eq!(r.count(), 8);
    }

    #[test]
    fn silence_boundary_exact() {
        assert!(!is_silent(1_000, Some(1_000 + SILENCE_WINDOW_MS)));
        assert!(is_silent(1_000, Some(1_000 + SILENCE_WINDOW_MS + 1)));
    }

    #[test]
    fn report_both_silent_falls_to_both_message() {
        let r = TestReport { left_ok: true, right_ok: true, both_ok: false, volume_permille: 400 };
        assert_eq!(r.conclusion(), "双声道无声——检查音量、接口或输出设备");
    }
}

// ===========================================================================
// 深化 v3（F480）：左右声道轮测序 / 静默诊断决策树深化 / 设备切换
// 中断账 / 音量档位表 / 测试会话报告（人话小结）
// ===========================================================================

/// 声道轮测序（主册「逐个测」：左 → 右 → 双 的固定序——顺序错乱
/// 会让用户漏测；轮测游标按此序推进）。
pub const CHANNEL_ORDER: [Channel; 3] = [Channel::Left, Channel::Right, Channel::Both];

impl ChannelTest {
    /// 下一个该测的声道（结果簿里没有的第一个——按 CHANNEL_ORDER 序）。
    pub fn next_pending(&self) -> Option<Channel> {
        CHANNEL_ORDER.iter().copied().find(|ch| {
            !(0..self.n).any(|i| matches!(self.results[i], Some((c, true)) if c == *ch))
        })
    }

    /// 轮测进度（已通过数 / 3，permille——「测到哪了」）。
    pub fn progress_permille(&self) -> u16 {
        let done = CHANNEL_ORDER.iter().filter(|ch| {
            (0..self.n).any(|i| matches!(self.results[i], Some((c, true)) if c == **ch))
        }).count();
        (done * 1_000 / 3) as u16
    }
}

/// 静默诊断决策树深化（v1 silent_diagnosis 的补全：无声 → 三问
/// （设备在吗/音量对吗/路由对吗）逐层归因——先查最便宜的）。
pub fn silent_decision_tree(device_alive: bool, volume_permille: u16, routed: bool) -> &'static str {
    if !device_alive {
        return "设备未接入——检查插孔或蓝牙连接";
    }
    if volume_permille == 0 {
        return "测试音量为零——拖动音量滑块后重测";
    }
    if !routed {
        return "声音走了别的设备——在音量面板切换输出设备";
    }
    "硬件层无声——建议用系统自带声音设置再验证"
}

/// 设备切换中断账（测试中途拔设备：会话作废 + 已测结果保留待
/// 新设备重测——不静默清账，也不带病继续）。
pub struct DeviceSwitchAudit {
    pub interrupted: bool,
    pub old_device: Option<u32>,
}

pub fn on_device_switch(old_device: Option<u32>) -> DeviceSwitchAudit {
    DeviceSwitchAudit { interrupted: old_device.is_some(), old_device }
}

/// 音量档位表（主册「测试音量」的档位化：0/25/40(默认)/60/100——
/// 滑块落点吸附到档位；400 档即 v1 默认）。
pub const VOLUME_STOPS: [u16; 5] = [0, 250, 400, 600, 1_000];

pub fn volume_nearest_stop(permille: u16) -> u16 {
    let mut best = VOLUME_STOPS[0];
    let mut best_d = 1_001u32;
    for &s in VOLUME_STOPS.iter() {
        let d = (s as i32 - permille as i32).unsigned_abs();
        if d < best_d {
            best_d = d;
            best = s;
        }
    }
    best
}

/// 测试会话报告（主册「测试完成有人话小结」：全过 = 设备正常；
/// 有未过 = 指名哪只声道——小结即账面，不是口号）。
pub fn session_summary(verified: [bool; 3]) -> &'static str {
    if verified == [true, true, true] {
        return "三个声道全部正常——设备路由没问题";
    }
    if !verified[0] {
        return "左声道未通过——检查左耳单元或平衡设置";
    }
    if !verified[1] {
        return "右声道未通过——检查右耳单元或平衡设置";
    }
    "双声道正常但立体声混音异常——检查应用音量混合器"
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F480-v3）
// ---------------------------------------------------------------------------

pub fn run_chantest_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F480-v3");
    // 1) 轮测序：先左、再右、再双；进度随通过推进。
    let mut t = ChannelTest::new(7);
    cs.add("order_first_left", t.next_pending() == Some(Channel::Left), "");
    let _ = t.verify_channel(Channel::Left, Some(7));
    cs.add("order_then_right", t.next_pending() == Some(Channel::Right), "");
    let _ = t.verify_channel(Channel::Right, Some(7));
    cs.add("order_then_both", t.next_pending() == Some(Channel::Both), "");
    let _ = t.verify_channel(Channel::Both, Some(7));
    cs.add("order_done_none", t.next_pending().is_none(), "");
    cs.add("progress_full", t.progress_permille() == 1_000, "");
    // 2) 诊断决策树：逐层归因（设备→音量→路由→硬件）。
    cs.add("tree_device", silent_decision_tree(false, 400, true).contains("设备未接入"), "");
    cs.add("tree_volume", silent_decision_tree(true, 0, true).contains("音量为零"), "");
    cs.add("tree_route", silent_decision_tree(true, 400, false).contains("别的设备"), "");
    cs.add("tree_hw_last", silent_decision_tree(true, 400, true).contains("硬件层"), "");
    // 3) 设备切换：中断账保留旧设备号。
    let sw = on_device_switch(Some(7));
    cs.add("switch_interrupt", sw.interrupted && sw.old_device == Some(7), "");
    cs.add("switch_cold_start", !on_device_switch(None).interrupted, "");
    // 4) 音量档位吸附：400 默认档、越界就近。
    cs.add("volume_stops", VOLUME_STOPS.contains(&400), "");
    cs.add("volume_snap_400", volume_nearest_stop(380) == 400, "");
    cs.add("volume_snap_0", volume_nearest_stop(100) == 0, "");
    cs.add("volume_snap_1000", volume_nearest_stop(900) == 1_000, "");
    // 5) 会话小结：全过/左坏/右坏/混音，四话各归其位。
    cs.add("summary_all_ok", session_summary([true, true, true]).contains("全部正常"), "");
    cs.add("summary_left", session_summary([false, true, true]).contains("左声道"), "");
    cs.add("summary_right", session_summary([true, false, true]).contains("右声道"), "");
    cs.add("summary_mixed", session_summary([true, true, false]).contains("立体声"), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn progress_partial_steps() {
        let mut t = ChannelTest::new(1);
        assert_eq!(t.progress_permille(), 0);
        let _ = t.verify_channel(Channel::Left, Some(1));
        assert_eq!(t.progress_permille(), 333);
        let _ = t.verify_channel(Channel::Right, Some(1));
        assert_eq!(t.progress_permille(), 666);
    }

    #[test]
    fn verify_wrong_device_is_failure() {
        let mut t = ChannelTest::new(7);
        // 路由到别的设备 = 该声道未通过（结果簿记 false）。
        assert!(!t.verify_channel(Channel::Left, Some(9)));
        assert_eq!(t.next_pending(), Some(Channel::Left), "未通过仍待测");
    }

    #[test]
    fn volume_stops_strictly_ordered() {
        for i in 1..VOLUME_STOPS.len() {
            assert!(VOLUME_STOPS[i] > VOLUME_STOPS[i - 1]);
        }
    }

    #[test]
    fn summary_never_empty() {
        for v in [[true, true, true], [false, false, false], [true, false, false]] {
            assert!(!session_summary(v).is_empty());
        }
    }
}

// ===========================================================================
// 深化 v7（F480）：会话账（连败告警）/ 路由时延账 / 声道隔离矩阵 /
// 偏好持久化 v7（W7H1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 会话账——每次声道测试记账（过/败）：时钟单调守卫 + 通过率 +
//   连败告警（3 连败 = 声道硬件可疑，显性化不让用户盲目重试）。
// 2. 路由时延账——「声道高亮 ↔ 声音到位」的时延量化：500ms 预算
//   （ROUTE_VERIFY_TIMEOUT_MS 同源锚）+ 峰值现形。
// 3. 声道隔离矩阵——左声道发声时右声道必须静默（串音检测的判据面：
//   立体声的意义就在分离）。
// 4. 偏好持久化——测试音量 + 上次设备：W7H1 通道（FNV 尾）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 会话账（连败告警）
// ---------------------------------------------------------------------------

/// 会话账容量。
pub const SESSION_LEDGER_CAP: usize = 16;
/// 连败告警阈值。
pub const CONSECUTIVE_FAIL_ALERT: usize = 3;

pub struct SessionLedger {
    ring: [(u64, bool); SESSION_LEDGER_CAP], // (时刻, 是否通过)
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl SessionLedger {
    pub const fn new() -> Self {
        SessionLedger { ring: [(0, false); SESSION_LEDGER_CAP], head: 0, n: 0, out_of_order_rejected: 0 }
    }

    pub fn push(&mut self, at_ms: u64, passed: bool) -> bool {
        if self.n > 0 {
            let last = (self.head + SESSION_LEDGER_CAP - 1) % SESSION_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, passed);
        self.head = (self.head + 1) % SESSION_LEDGER_CAP;
        self.n = (self.n + 1).min(SESSION_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 通过率 permille（空账诚实 None）。
    pub fn pass_rate_permille(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let passes = (0..self.n)
            .filter(|&i| {
                let idx = (self.head + SESSION_LEDGER_CAP - self.n + i) % SESSION_LEDGER_CAP;
                self.ring[idx].1
            })
            .count();
        Some((passes as u32 * 1_000 / self.n as u32) as u32)
    }

    /// 连败告警（最新连续失败 ≥ 阈值——中间夹一次通过就断链）。
    pub fn consecutive_failing(&self) -> bool {
        let mut streak = 0;
        for i in 0..self.n {
            let idx = (self.head + SESSION_LEDGER_CAP - 1 - i) % SESSION_LEDGER_CAP;
            if self.ring[idx].1 {
                break;
            }
            streak += 1;
            if streak >= CONSECUTIVE_FAIL_ALERT {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 路由时延账（高亮 ↔ 声音到位）
// ---------------------------------------------------------------------------

/// 时延账容量。
pub const ROUTE_LEDGER_CAP: usize = 12;

pub struct RouteLatencyLedger {
    ring: [(u64, u64); ROUTE_LEDGER_CAP], // (时钟, 时延 ms)
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl RouteLatencyLedger {
    pub const fn new() -> Self {
        RouteLatencyLedger {
            ring: [(0, 0); ROUTE_LEDGER_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
        }
    }

    pub fn push(&mut self, at_ms: u64, latency_ms: u64) -> bool {
        if self.n > 0 {
            let last = (self.head + ROUTE_LEDGER_CAP - 1) % ROUTE_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, latency_ms);
        self.head = (self.head + 1) % ROUTE_LEDGER_CAP;
        self.n = (self.n + 1).min(ROUTE_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn max_latency(&self) -> u64 {
        (0..self.n)
            .map(|i| {
                let idx = (self.head + ROUTE_LEDGER_CAP - self.n + i) % ROUTE_LEDGER_CAP;
                self.ring[idx].1
            })
            .max()
            .unwrap_or(0)
    }

    /// 预算达成率（permille，预算 = ROUTE_VERIFY_TIMEOUT_MS）。
    pub fn budget_hit_permille(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let hit = (0..self.n)
            .filter(|&i| {
                let idx = (self.head + ROUTE_LEDGER_CAP - self.n + i) % ROUTE_LEDGER_CAP;
                self.ring[idx].1 <= ROUTE_VERIFY_TIMEOUT_MS
            })
            .count();
        (hit as u32 * 1_000 / self.n as u32) as u32
    }
}

// ---------------------------------------------------------------------------
// 声道隔离矩阵（立体声分离判据）
// ---------------------------------------------------------------------------

/// 隔离裁决：发左声道时右声道应答 = 串音（隔离破缺）。
/// ack_channels: bit0 = 左有应答、bit1 = 右有应答。
pub fn isolation_ok(played: Channel, ack_bits: u8) -> bool {
    match played {
        Channel::Left => ack_bits & 0b10 == 0, // 左发 → 右必须静默
        Channel::Right => ack_bits & 0b01 == 0, // 右发 → 左必须静默
        Channel::Both => true,                  // 双声道全响 = 正常
    }
}

/// 全矩阵审计：三发声情形全过（左右互不串、双声道全响合法）。
pub fn isolation_matrix_audit() -> bool {
    isolation_ok(Channel::Left, 0b01)      // 只有左应答 = ✓
        && !isolation_ok(Channel::Left, 0b11) // 右也响 = 串音 ✗
        && isolation_ok(Channel::Right, 0b10)
        && !isolation_ok(Channel::Right, 0b11)
        && isolation_ok(Channel::Both, 0b11)
}

// ---------------------------------------------------------------------------
// 偏好持久化 v7（W7H1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7H 族）。
pub const CHANTEST_V7_MAGIC: [u8; 4] = *b"W7H1";
/// 长度：魔标(4) + 版本(1) + 保留(1) + 音量(2, LE) + 设备(4, LE) + FNV(4) = 16。
pub const CHANTEST_V7_LEN: usize = 16;
pub const CHANTEST_V7_VERSION: u8 = 1;

/// 序列化（音量 permille ≤1000；设备 0 = 未登记）。
pub fn save_prefs_v7(volume_permille: u16, last_device: u32, out: &mut [u8]) -> Option<usize> {
    if out.len() < CHANTEST_V7_LEN || volume_permille > 1_000 {
        return None;
    }
    out[..4].copy_from_slice(&CHANTEST_V7_MAGIC);
    out[4] = CHANTEST_V7_VERSION;
    out[5] = 0;
    out[6..8].copy_from_slice(&volume_permille.to_le_bytes());
    out[8..12].copy_from_slice(&last_device.to_le_bytes());
    let h = fnv1a(&out[..12]);
    out[12] = (h & 0xff) as u8;
    out[13] = ((h >> 8) & 0xff) as u8;
    out[14] = ((h >> 16) & 0xff) as u8;
    out[15] = ((h >> 24) & 0xff) as u8;
    Some(CHANTEST_V7_LEN)
}

/// 反序列化（版本/保留位/音量值域/FNV 四重守卫）。
pub fn load_prefs_v7(buf: &[u8]) -> Option<(u16, u32)> {
    if buf.len() < CHANTEST_V7_LEN || buf[..4] != CHANTEST_V7_MAGIC {
        return None;
    }
    if buf[4] != CHANTEST_V7_VERSION || buf[5] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..12]);
    let got = buf[12] as u32
        | ((buf[13] as u32) << 8)
        | ((buf[14] as u32) << 16)
        | ((buf[15] as u32) << 24);
    if expect != got {
        return None;
    }
    let mut v = [0u8; 2];
    v.copy_from_slice(&buf[6..8]);
    let volume = u16::from_le_bytes(v);
    if volume > 1_000 {
        return None;
    }
    let mut d = [0u8; 4];
    d.copy_from_slice(&buf[8..12]);
    Some((volume, u32::from_le_bytes(d)))
}

// ---------------------------------------------------------------------------
// 域自检（F480 v7）
// ---------------------------------------------------------------------------

pub fn run_chantest_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F480-v7");
    // 1) 会话账：通过率 + 单调守卫 + 环上限。
    cs.add("session_pass_rate", {
        let mut led = SessionLedger::new();
        for i in 0..8u64 {
            let _ = led.push(i * 1_000, i % 4 != 3); // 6 过 2 败
        }
        led.pass_rate_permille() == Some(750)
    }, "");
    cs.add("session_monotonic", {
        let mut led = SessionLedger::new();
        let _ = led.push(1_000, true);
        !led.push(500, false) && led.out_of_order_rejected == 1
    }, "");
    cs.add("session_ring_cap", {
        let mut led = SessionLedger::new();
        for i in 0..(SESSION_LEDGER_CAP * 2) {
            let _ = led.push(i as u64 * 100, true);
        }
        led.count() == SESSION_LEDGER_CAP
    }, "");
    cs.add("session_empty_honest", SessionLedger::new().pass_rate_permille().is_none(), "");
    // 2) 连败告警：3 连败触发、夹一次通过断链。
    cs.add("consecutive_fail_alert", {
        let mut led = SessionLedger::new();
        for i in 0..3u64 {
            let _ = led.push(i * 1_000, false);
        }
        led.consecutive_failing()
    }, "");
    cs.add("pass_breaks_streak", {
        let mut led = SessionLedger::new();
        let _ = led.push(1_000, false);
        let _ = led.push(2_000, false);
        let _ = led.push(3_000, true);
        let _ = led.push(4_000, false);
        let _ = led.push(5_000, false);
        !led.consecutive_failing() // 连败被通过打断（仅 2 连）
    }, "");
    // 3) 路由时延账：预算达成 + 峰值现形 + 单调守卫。
    cs.add("route_budget_hit", {
        let mut led = RouteLatencyLedger::new();
        for i in 0..10u64 {
            let _ = led.push(i * 1_000, 400);
        }
        led.budget_hit_permille() == 1_000 && led.max_latency() == 400
    }, "");
    cs.add("route_tail_visible", {
        let mut led = RouteLatencyLedger::new();
        for i in 0..9u64 {
            let _ = led.push(i * 1_000, 300);
        }
        let _ = led.push(9_000, 900); // 超预算一笔现形
        led.max_latency() == 900 && led.budget_hit_permille() == 900
    }, "");
    cs.add("route_monotonic", {
        let mut led = RouteLatencyLedger::new();
        let _ = led.push(1_000, 100);
        !led.push(500, 100) && led.out_of_order_rejected == 1
    }, "");
    // 4) 声道隔离矩阵：左右互不串、双声道全响。
    cs.add("isolation_matrix", isolation_matrix_audit(), "");
    // 5) 偏好持久化：round-trip + 篡改 + 音量值域。
    let mut buf = [0u8; CHANTEST_V7_LEN];
    cs.add("prefs_roundtrip", {
        let n = save_prefs_v7(TEST_VOLUME_PERMILLE, 0xDEAD, &mut buf).unwrap_or(0);
        load_prefs_v7(&buf[..n]) == Some((TEST_VOLUME_PERMILLE, 0xDEAD))
    }, "");
    cs.add("prefs_tamper", {
        let n = save_prefs_v7(400, 7, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[8] ^= 0x01;
        load_prefs_v7(&bad[..n]).is_none()
    }, "");
    cs.add("prefs_bad_volume", save_prefs_v7(1_001, 1, &mut buf).is_none(), "");
    // 6) v1 回归锚：测试音量常量 + 三声道序（v7 面不许伤 v1 语义）。
    cs.add("v1_volume_regression", TEST_VOLUME_PERMILLE == 400, "");
    cs.add("v1_channel_order_regression", {
        CHANNEL_ORDER == [Channel::Left, Channel::Right, Channel::Both]
    }, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn pass_rate_full_and_zero() {
        let mut all_pass = SessionLedger::new();
        let mut all_fail = SessionLedger::new();
        for i in 0..4u64 {
            let _ = all_pass.push(i * 100, true);
            let _ = all_fail.push(i * 100, false);
        }
        assert_eq!(all_pass.pass_rate_permille(), Some(1_000));
        assert_eq!(all_fail.pass_rate_permille(), Some(0));
        assert!(all_fail.consecutive_failing());
    }

    #[test]
    fn route_empty_honest() {
        let led = RouteLatencyLedger::new();
        assert_eq!(led.max_latency(), 0);
        assert_eq!(led.budget_hit_permille(), 0);
    }

    #[test]
    fn isolation_both_channels_silent_is_ok_for_both() {
        // 双声道测试时全静默是另一个问题（无声），但隔离判据不拦——
        // 静默诊断走 silent_decision_tree（v3 面）。
        assert!(isolation_ok(Channel::Both, 0b00));
    }
}
