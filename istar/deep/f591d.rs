//! 深化层 · F591 输出设备切换热键（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深五条（判据唯一源：主册 F591 节）：
//! ① **循环覆盖账**——一整圈对在线设备恰好各访一次、离线设备零访问
//!   （鲁棒循环覆盖面），且圈序逐圈复现（确定性）；
//! ② **OSD 显示账**——内容（前缀+设备名）与时序（[shown, shown+OSD_MS)
//!   闭开窗）的纯计算，无渲染依赖；
//! ③ **防爆音三步过渡状态机**——静音→切→恢复强制按序，不许瞬切、
//!   不许跳步、恢复须满过渡窗；
//! ④ **默认无绑定零副作用审计**——未绑定按键按下后切换数/OSD/过渡窗
//!   全部纹丝不动；
//! ⑤ **绑定持久化 round-trip**——绑定值编码为注册表记录再解码不失真。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::outdevkey::{OutDevHotkey, ANTI_POP_FADE_MS, OSD_MS};

use alloc::string::String;
use alloc::vec::Vec;

// --- ① 循环覆盖账 -----------------------------------------------------------

/// 一段循环行程的访问记录（覆盖审计的账面）。
pub struct CycleLedger {
    visited: Vec<String>,
}

impl CycleLedger {
    pub fn new() -> CycleLedger {
        CycleLedger { visited: Vec::new() }
    }

    pub fn note(&mut self, name: &str) {
        self.visited.push(String::from(name));
    }

    /// 覆盖审计：每个在线设备恰好访问一次、离线设备零访问。
    pub fn covers_exactly_once(&self, online: &[&str], offline: &[&str]) -> bool {
        for o in online {
            if self.visited.iter().filter(|v| v == o).count() != 1 {
                return false;
            }
        }
        for o in offline {
            if self.visited.iter().any(|v| v == o) {
                return false;
            }
        }
        true
    }

    /// 圈序复现：两段行程完全一致（循环确定性——同态不漂移）。
    pub fn cycles_repeat(&self, cycle_len: usize) -> bool {
        if self.visited.len() < cycle_len * 2 || cycle_len == 0 {
            return false;
        }
        let (a, b) = self.visited.split_at(cycle_len);
        a == b
    }

    pub fn len(&self) -> usize {
        self.visited.len()
    }
}

impl Default for CycleLedger {
    fn default() -> Self {
        Self::new()
    }
}

// --- ② OSD 显示账 -----------------------------------------------------------

/// OSD 卡（F240 形制的纯计算——内容 + 时序，渲染归宿主 UI 层）。
pub struct OsdCard {
    /// 前缀文案（形制唯一源）。
    pub prefix: &'static str,
    device: String,
    shown_at_ms: u64,
}

impl OsdCard {
    /// 切到某设备的 OSD 卡：内容定形、时刻定窗。
    pub fn new(device: &str, shown_at_ms: u64) -> OsdCard {
        OsdCard { prefix: "输出:", device: String::from(device), shown_at_ms }
    }

    /// 在显判定：[shown, shown+OSD_MS) 闭开窗内可见（到期自收）。
    pub fn visible_at(&self, now_ms: u64) -> bool {
        now_ms >= self.shown_at_ms && now_ms < self.shown_at_ms + OSD_MS
    }

    pub fn device(&self) -> &str {
        &self.device
    }
}

// --- ③ 防爆音三步过渡状态机 ---------------------------------------------------

/// 过渡步（Idle→Muted→Switched→Restored，强制按序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeStep {
    Idle,
    Muted,
    Switched,
    Restored,
}

/// 三步过渡账：静音 → 切 → 恢复（不许瞬切——每步都要走、不许多走）。
pub struct AntiPopSeq {
    step: FadeStep,
    since_ms: u64,
}

impl AntiPopSeq {
    pub fn new() -> AntiPopSeq {
        AntiPopSeq { step: FadeStep::Idle, since_ms: 0 }
    }

    pub fn step(&self) -> FadeStep {
        self.step
    }

    /// 第一步静音（仅从 Idle 进入）。
    pub fn mute(&mut self, ms: u64) -> bool {
        if self.step != FadeStep::Idle {
            return false;
        }
        self.step = FadeStep::Muted;
        self.since_ms = ms;
        true
    }

    /// 第二步切换（仅从 Muted 进入——没静音就切 = 瞬切，拒绝）。
    pub fn switch(&mut self, ms: u64) -> bool {
        if self.step != FadeStep::Muted {
            return false;
        }
        self.step = FadeStep::Switched;
        self.since_ms = ms;
        true
    }

    /// 第三步恢复（仅从 Switched 且距切换满 ANTI_POP_FADE_MS 过渡窗）。
    pub fn restore(&mut self, ms: u64) -> bool {
        if self.step != FadeStep::Switched {
            return false;
        }
        if ms.saturating_sub(self.since_ms) < ANTI_POP_FADE_MS as u64 {
            return false;
        }
        self.step = FadeStep::Restored;
        true
    }
}

impl Default for AntiPopSeq {
    fn default() -> Self {
        Self::new()
    }
}

// --- ⑤ 绑定持久化 round-trip --------------------------------------------------

/// 绑定值 → 注册表记录（F244 承接面：绑定与否 + 键位原样存档）。
pub fn binding_to_reg<'a>(key: Option<&'a str>) -> (bool, &'a str) {
    match key {
        Some(k) => (true, k),
        None => (false, ""),
    }
}

/// 注册表记录 → 绑定值（bound 且键位非空才成绑定——坏档诚实 None）。
pub fn binding_from_reg(bound: bool, key: &str) -> Option<String> {
    if bound && !key.is_empty() {
        Some(String::from(key))
    } else {
        None
    }
}

// --- 深化自检 ---------------------------------------------------------------

pub fn run_f591_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 循环覆盖：蓝牙离线，一整圈（2 按）在线设备恰好各一次、离线零访。
    let mut h = OutDevHotkey::new();
    h.sync_devices(&["耳机", "扬声器", "蓝牙耳机"]);
    h.set_online("蓝牙耳机", false);
    h.bind("O");
    let mut c1 = CycleLedger::new();
    for _ in 0..2 {
        if let Some(n) = h.hotkey_pressed() {
            c1.note(&n);
        }
    }
    cs.add(
        "cycle covers online exactly once",
        c1.len() == 2 && c1.covers_exactly_once(&["耳机", "扬声器"], &["蓝牙耳机"]),
        "",
    );

    // 2) 圈序复现：全员在线连按两整圈，两圈序列逐位一致（确定性）。
    let mut h2 = OutDevHotkey::new();
    h2.sync_devices(&["耳机", "扬声器", "蓝牙耳机"]);
    h2.bind("O");
    let mut c2 = CycleLedger::new();
    for _ in 0..6 {
        if let Some(n) = h2.hotkey_pressed() {
            c2.note(&n);
        }
    }
    cs.add("cycle order repeats", c2.len() == 6 && c2.cycles_repeat(3), "");

    // 3) OSD 显示账：内容「输出:+设备」；窗口 [shown, shown+1500) 内显。
    let card = OsdCard::new("耳机", 10_000);
    cs.add(
        "osd content and timing",
        card.prefix == "输出:" && card.device() == "耳机"
            && card.visible_at(10_000) && card.visible_at(10_100)
            && !card.visible_at(9_999) && !card.visible_at(10_000 + OSD_MS),
        "",
    );

    // 4) 三步过渡正序：静音→切→恢复（过渡窗满）全通，终态 Restored。
    let mut seq = AntiPopSeq::new();
    let m = seq.mute(0);
    let s = seq.switch(10);
    let r = seq.restore(10 + ANTI_POP_FADE_MS as u64);
    cs.add(
        "anti pop ordered sequence",
        m && s && r && seq.step() == FadeStep::Restored,
        "",
    );

    // 5) 状态机纪律：没静音就切 = 瞬切，拒绝（跳步诚实 false）。
    let mut seq2 = AntiPopSeq::new();
    let skipped = seq2.switch(0);
    cs.add(
        "no instant switch skip mute",
        !skipped && seq2.step() == FadeStep::Idle,
        "",
    );

    // 6) 恢复过早：距切换不足 40ms 过渡窗 → 拒绝（防爆音窗必须走满）。
    let mut seq3 = AntiPopSeq::new();
    let _ = seq3.mute(0);
    let _ = seq3.switch(100);
    let too_soon = seq3.restore(100 + ANTI_POP_FADE_MS as u64 - 1);
    let in_time = seq3.restore(100 + ANTI_POP_FADE_MS as u64);
    cs.add(
        "restore waits fade window",
        !too_soon && in_time && seq3.step() == FadeStep::Restored,
        "",
    );

    // 7) 默认无绑定零副作用：按下=无切换、OSD 不显、过渡窗不启。
    let mut h7 = OutDevHotkey::new();
    h7.sync_devices(&["耳机", "扬声器"]);
    let pressed = h7.hotkey_pressed();
    cs.add(
        "unbound press zero side effects",
        pressed.is_none() && h7.switch_count() == 0
            && !h7.osd_visible() && !h7.anti_pop_active(),
        "",
    );

    // 8) 绑定持久化 round-trip：bind→注册表→回读同值；unbound 档诚实 None。
    let mut h8 = OutDevHotkey::new();
    h8.bind("F9");
    let (bound, key) = binding_to_reg(h8.binding());
    let round_trip = binding_from_reg(bound, key);
    h8.unbind();
    let (bound2, key2) = binding_to_reg(h8.binding());
    cs.add(
        "binding registry round trip",
        round_trip.as_deref() == Some("F9") && bound
            && !bound2 && binding_from_reg(bound2, key2).is_none(),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_fails_when_offline_visited() {
        let mut c = CycleLedger::new();
        c.note("耳机");
        c.note("蓝牙耳机"); // 离线设备被访——覆盖审计应红
        assert!(!c.covers_exactly_once(&["耳机"], &["蓝牙耳机"]));
    }

    #[test]
    fn osd_boundary_inclusive_start() {
        let card = OsdCard::new("扬声器", 0);
        assert!(card.visible_at(0));
        assert!(card.visible_at(OSD_MS - 1));
        assert!(!card.visible_at(OSD_MS));
    }

    #[test]
    fn mute_twice_rejected() {
        let mut seq = AntiPopSeq::new();
        assert!(seq.mute(0));
        assert!(!seq.mute(1)); // Muted 态不能再静音
    }
}
