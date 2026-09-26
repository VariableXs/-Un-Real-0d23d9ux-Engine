//! F443 蓝牙配对流程 · 完整设计（STAR I 主册 G-I-43）。
//!
//! **判据（主册）**：发现/配对/确认全链；确认码核对判据（不跳过）；
//! 失败归因映射表；自动重连时长；改名持久化。＋通12。
//!
//! 设计：蓝牙配对状态机——发现（信号强度 + 设备类型）；配对请求 →
//! **确认码核对**（两端码一致才完成——未核对/不一致都到不了已配对，
//! 结构性跳不过）；失败归因映射表（错误码 → 人话 + 下一步动作）；
//! 已配对设备操作（连接/断开/删除/改名——改名持久化登记）；自动重连
//! 计时（信号恢复 → 重连完成 <3s 预算账）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 自动重连判线（ms）。
pub const RECONNECT_BUDGET_MS: u64 = 3_000;

/// 设备类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtKind {
    Headset,
    Keyboard,
    Mouse,
    Phone,
    Unknown,
}

impl BtKind {
    pub fn label(self) -> &'static str {
        match self {
            BtKind::Headset => "音频设备",
            BtKind::Keyboard => "键盘",
            BtKind::Mouse => "鼠标",
            BtKind::Phone => "手机",
            BtKind::Unknown => "未知设备",
        }
    }
}

/// 配对状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    Discovered,
    AwaitingConfirm,
    Paired,
    Failed,
}

/// 发现列表条目。
#[derive(Clone, Debug)]
pub struct BtDevice {
    pub id: u64,
    pub name: String,
    pub kind: BtKind,
    /// 信号强度（0-100，越近越高）。
    pub rssi_pct: u8,
    pub state: PairState,
    /// 本端期望的确认码（配对时生成；None = 未进入配对）。
    pub expect_code: Option<u32>,
    /// 用户自定义名（改名持久化）。
    pub custom_name: Option<String>,
}

/// 失败归因映射表：错误码 → (人话, 下一步)。
pub fn failure_cause(code: u32) -> (&'static str, &'static str) {
    match code {
        1 => ("设备未进入配对模式", "查看设备说明书长按配对键后重试"),
        2 => ("确认码不匹配", "重新配对并核对两端显示的数字"),
        3 => ("设备已连接到其他主机", "在原主机上断开连接后再配对"),
        4 => ("超出有效范围", "把设备靠近本机（10 米内）再试"),
        _ => ("配对失败", "关闭设备电源重开再试"),
    }
}

/// 蓝牙配对核。
pub struct BtPairing {
    pub devices: Vec<BtDevice>,
    /// 重连耗时账（最近一次重连 ms；超线计数）。
    pub last_reconnect_ms: Option<u64>,
    pub reconnects_over_budget: u64,
}

impl BtPairing {
    pub fn new() -> BtPairing {
        BtPairing { devices: Vec::new(), last_reconnect_ms: None, reconnects_over_budget: 0 }
    }

    /// 发现：信号强度 + 类型入列。
    pub fn discover(&mut self, id: u64, name: &str, kind: BtKind, rssi_pct: u8) {
        if !self.devices.iter().any(|d| d.id == id) {
            self.devices.push(BtDevice {
                id,
                name: String::from(name),
                kind,
                rssi_pct,
                state: PairState::Discovered,
                expect_code: None,
                custom_name: None,
            });
        }
    }

    /// 发起配对：生成两端展示的确认码。
    pub fn begin_pair(&mut self, id: u64, code: u32) -> bool {
        match self.devices.iter_mut().find(|d| d.id == id) {
            Some(d) if d.state == PairState::Discovered || d.state == PairState::Failed => {
                d.expect_code = Some(code);
                d.state = PairState::AwaitingConfirm;
                true
            }
            _ => false,
        }
    }

    /// 确认码核对：两端码一致才完成（不跳过——None 也到不了 Paired）。
    pub fn confirm_pair(&mut self, id: u64, user_input: u32) -> bool {
        let Some(d) = self.devices.iter_mut().find(|d| d.id == id) else { return false };
        let ok = d.expect_code == Some(user_input);
        if ok {
            d.state = PairState::Paired;
            d.expect_code = None;
        } else {
            d.state = PairState::Failed;
        }
        ok
    }

    /// 已配对设备改名（持久化登记——custom_name 字段）。
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        match self.devices.iter_mut().find(|d| d.id == id) {
            Some(d) if d.state == PairState::Paired => {
                d.custom_name = Some(String::from(name));
                true
            }
            _ => false,
        }
    }

    /// 自动重连：信号恢复 → 计时；超 3s 记账（诚实）。
    pub fn reconnect_tick(&mut self, elapsed_ms: u64) {
        self.last_reconnect_ms = Some(elapsed_ms);
        if elapsed_ms > RECONNECT_BUDGET_MS {
            self.reconnects_over_budget += 1;
        }
    }
}

pub fn run_btpair_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F443");
    let mut b = BtPairing::new();
    // 发现：信号强度 + 类型。
    b.discover(1, "StarBuds", BtKind::Headset, 82);
    b.discover(2, "StarKeys", BtKind::Keyboard, 45);
    set.add(
        "f443-discovered-with-rssi",
        b.devices.len() == 2 && b.devices[0].kind.label() == "音频设备" && b.devices[0].rssi_pct == 82,
        "",
    );
    // 配对：确认码核对（不跳过）。
    set.add("f443-awaiting-confirm", b.begin_pair(1, 482_913) && b.devices[0].state == PairState::AwaitingConfirm, "");
    set.add(
        "f443-wrong-code-rejected",
        !b.confirm_pair(1, 111_111) && b.devices[0].state == PairState::Failed,
        "",
    );
    // 失败后可重试：重发码 → 对上 → Paired。
    set.add("f443-retry-allowed", b.begin_pair(1, 482_913), "");
    set.add(
        "f443-confirm-paired",
        b.confirm_pair(1, 482_913) && b.devices[0].state == PairState::Paired && b.devices[0].expect_code.is_none(),
        "",
    );
    // 未进入配对的设备无法凭空确认。
    set.add("f443-cannot-skip-confirm", !b.confirm_pair(2, 0), "");
    // 失败归因映射表（人话 + 下一步）。
    let (what, next) = failure_cause(2);
    set.add(
        "f443-failure-mapped",
        what == "确认码不匹配" && next.contains("重新配对"),
        "",
    );
    // 改名持久化（仅已配对设备）。
    set.add("f443-rename-paired", b.rename(1, "我的耳机") && b.devices[0].custom_name.as_deref() == Some("我的耳机"), "");
    set.add("f443-rename-unpaired-refused", !b.rename(2, "x"), "");
    // 自动重连预算 <3s。
    b.reconnect_tick(1_800);
    set.add("f443-reconnect-under-3s", b.last_reconnect_ms == Some(1_800) && b.reconnects_over_budget == 0, "");
    b.reconnect_tick(4_000);
    set.add("f443-reconnect-over-logged", b.reconnects_over_budget == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_discovery_deduped() {
        let mut b = BtPairing::new();
        b.discover(7, "Mouse", BtKind::Mouse, 60);
        b.discover(7, "Mouse", BtKind::Mouse, 65); // 同 id 再广播：去重
        assert_eq!(b.devices.len(), 1);
        assert_eq!(b.devices[0].rssi_pct, 60, "首见为准，不被重复广播覆盖");
    }

    #[test]
    fn all_failure_codes_have_human_reasons() {
        for code in [1u32, 2, 3, 4, 99] {
            let (what, next) = failure_cause(code);
            assert!(!what.is_empty() && !next.is_empty(), "错误码 {} 必须有人话归因", code);
        }
    }
}
