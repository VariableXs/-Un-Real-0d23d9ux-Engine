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
