//! F448 麦克风测试向导 · 完整设计（STAR I 主册 G-I-48）。
//!
//! **判据（主册）**：电平实时性（<100ms）；回放保真（原样录制回放）；
//! 建议触发阈值（电平分档）；设备切换；与 F322 隐私指示联动（测试中
//! 指示亮）。＋通12。
//!
//! 设计：麦克风测试核——电平表（采样 → RMS 电平，入表延迟账 <100ms
//! ——「它在听」肉眼可见）；5s 录音回放（录到的字节原样回放——保真
//! 对拍）；电平分档 → 人话建议（过低「离麦克风近一点」/ 过高「输入
//! 音量降到 NN」/ 正常「就用这个设置」）；多输入设备切换即测（切换
//! 不断线——设备表热切换）；F322 隐私指示联动（测试中指示亮——
//! 测试自己也在被监视的自觉）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 电平表入表判线（ms）。
pub const METER_LATENCY_BUDGET_MS: u64 = 100;
/// 测试录音时长（ms）。
pub const TEST_CLIP_MS: u64 = 5_000;

/// 电平分档 → 建议文案（阈值唯一登记点）。
pub fn level_advice(level_permille: u16, input_volume: u8) -> &'static str {
    if level_permille < 200 {
        "电平过低——离麦克风近一点，或调高输入音量"
    } else if level_permille > 950 {
        if input_volume > 50 {
            "电平过高——把输入音量降到 60 以内再试"
        } else {
            "电平过高——离麦克风远一点"
        }
    } else {
        "电平正常——就用这个设置"
    }
}

/// 麦克风测试核。
pub struct MicTest {
    /// 当前输入设备（设备表热切换）。
    pub devices: Vec<&'static str>,
    pub active_device: usize,
    /// 电平表延迟账。
    pub meter_latencies_ms: Vec<u64>,
    pub over_budget_meters: u64,
    /// 5s 录音缓冲（字节）。
    pub clip: Vec<u8>,
    pub clip_ms: u64,
    /// F322 隐私指示（测试中 = 亮）。
    pub privacy_indicator_on: bool,
}

impl MicTest {
    pub fn new(devices: Vec<&'static str>) -> MicTest {
        MicTest {
            devices,
            active_device: 0,
            meter_latencies_ms: Vec::new(),
            over_budget_meters: 0,
            clip: Vec::new(),
            clip_ms: 0,
            privacy_indicator_on: false,
        }
    }

    /// 设备切换即测：切换不中断测试会话（指示保持亮）。
    pub fn switch_device(&mut self, idx: usize) -> bool {
        if idx >= self.devices.len() {
            return false;
        }
        self.active_device = idx;
        true
    }

    /// 电平表更新：一次采样入表，延迟记账（<100ms 判据）。
    pub fn meter_tick(&mut self, sample: i16, latency_ms: u64) -> u16 {
        if latency_ms > METER_LATENCY_BUDGET_MS {
            self.over_budget_meters += 1;
        }
        self.meter_latencies_ms.push(latency_ms);
        // RMS 近似：|sample| 映射到千分比。
        ((sample.unsigned_abs() as u32) * 1_000 / i16::MAX as u32) as u16
    }

    /// 录 5 秒：字节入缓冲（原样——回放保真的结构性来源）。
    pub fn record_bytes(&mut self, bytes: &[u8], duration_ms: u64) -> bool {
        if self.clip_ms + duration_ms > TEST_CLIP_MS {
            return false; // 测试片段限 5 秒
        }
        self.privacy_indicator_on = true; // 录制中指示亮（F322 联动）
        self.clip.extend_from_slice(bytes);
        self.clip_ms += duration_ms;
        true
    }

    /// 回放：返回录制字节原样（保真 = 逐字节相同）。
    pub fn playback(&self) -> &[u8] {
        &self.clip
    }

    /// 回放保真对拍：回放字节与录制时字节完全一致。
    pub fn fidelity_ok(&self, recorded: &[u8]) -> bool {
        self.clip == recorded
    }

    /// 结束测试：释放指示（测试完指示灭——不常驻监听）。
    pub fn finish(&mut self) {
        self.privacy_indicator_on = false;
        self.clip.clear();
        self.clip_ms = 0;
    }
}

pub fn run_mictest_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F448");
    let mut m = MicTest::new(alloc::vec!["内置麦克风", "USB 话筒"]);
    // 电平表实时性：<100ms 入表；超线记账。
    let lvl = m.meter_tick(16_384, 40); // 半幅 → 500‰
    set.add(
        "f448-meter-latency",
        m.meter_latencies_ms.len() == 1 && m.over_budget_meters == 0,
        "",
    );
    let _ = m.meter_tick(32_000, 250);
    set.add("f448-over-budget-logged", m.over_budget_meters == 1, "");
    // 建议触发阈值（电平分档 → 人话）。
    set.add(
        "f448-advice-tiers",
        level_advice(100, 50).contains("近一点")
            && level_advice(980, 80).contains("降到 60")
            && level_advice(980, 30).contains("远一点")
            && level_advice(500, 50).contains("正常")
            && lvl == 500,
        "",
    );
    // 5s 录音回放保真（字节级对拍）。
    let payload: Vec<u8> = (0..160u16).map(|i| (i % 251) as u8).collect();
    set.add("f448-record-5s", m.record_bytes(&payload, 5_000) && m.clip_ms == 5_000, "");
    set.add("f448-fidelity-byte-exact", m.fidelity_ok(&payload) && m.playback().len() == 160, "");
    // 超长拒绝（诚实——测试片段限 5 秒）。
    set.add("f448-overlong-rejected", !m.record_bytes(b"x", 100), "");
    // 设备切换即测：切换不断线（指示保持亮、缓冲保留）。
    set.add(
        "f448-device-switch-live",
        m.switch_device(1) && m.devices[m.active_device] == "USB 话筒" && m.privacy_indicator_on && m.clip_ms == 5_000,
        "",
    );
    set.add("f448-device-switch-bounds", !m.switch_device(9), "");
    // F322 指示联动：结束测试 → 指示灭 + 缓冲清。
    m.finish();
    set.add("f448-indicator-released", !m.privacy_indicator_on && m.clip.is_empty(), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_scale_extremes() {
        let mut m = MicTest::new(alloc::vec!["mic"]);
        assert_eq!(m.meter_tick(0, 10), 0, "静音采样 → 0‰");
        assert_eq!(m.meter_tick(i16::MIN, 10), 1_000, "满幅负采样 → 1000‰（绝对值）");
        assert_eq!(m.meter_tick(i16::MAX, 10), 1_000);
    }

    #[test]
    fn advice_thresholds_at_boundaries() {
        // 阈值边界：200 与 950 的分档归属唯一。
        assert!(level_advice(199, 50).contains("低"));
        assert!(level_advice(200, 50).contains("正常"));
        assert!(level_advice(950, 50).contains("正常"));
        assert!(level_advice(951, 50).contains("高"));
    }
}
