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
