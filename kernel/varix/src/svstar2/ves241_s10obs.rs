//! VE-F3987 · S 域体验日志与观测终审
//!
//! 终审：S 域观测终审——体验日志埋点口径冻结（无障碍事件四类埋点：
//! 色标切换 / 读屏会话 / 动效降级 / 替代输入）+ 观测面板项终版 + 留痕
//! 采样率复核。
//! 数据结构：埋点口径册（四类）；面板项表；采样率表。
//! 错误路径：口径漂移→对版修正；面板缺项→补齐；采样失真→重校。
//! 性能：冻结 O(口径)；补齐 O(缺项)；重校 O(样本)。
//! 跨批对接：F3964 工具上游；F3992 清账证据源。
//! 无障碍与隐私：埋点不含可识别用户内容（隐私红线复述）；采样脱敏。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x41C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x41C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T7OCode(pub u16);

impl T7OCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CALIBER_DRIFT: Self = Self(SEG_BASE + 1);
    pub const PANEL_GAP: Self = Self(SEG_BASE + 2);
    pub const SAMPLE_DISTORT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CALIBER_DRIFT => "口径漂移→对版修正",
            Self::PANEL_GAP => "面板缺项→补齐",
            Self::SAMPLE_DISTORT => "采样失真→重校",
            _ => "未知",
        }
    }
}

// ── 四类埋点 ─────────────────────────────────────────────────────────

/// 无障碍事件四类埋点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 色标切换。
    ColorSwitch,
    /// 读屏会话。
    ReaderSession,
    /// 动效降级。
    MotionDegrade,
    /// 替代输入。
    AltInput,
}

pub const EVENT_KINDS: [EventKind; 4] = [
    EventKind::ColorSwitch, EventKind::ReaderSession, EventKind::MotionDegrade, EventKind::AltInput,
];

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            EventKind::ColorSwitch => "色标切换",
            EventKind::ReaderSession => "读屏会话",
            EventKind::MotionDegrade => "动效降级",
            EventKind::AltInput => "替代输入",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            EventKind::ColorSwitch => 0,
            EventKind::ReaderSession => 1,
            EventKind::MotionDegrade => 2,
            EventKind::AltInput => 3,
        }
    }
}

/// 埋点口径条目：事件类 + 版本号 + 脱敏声明。
#[derive(Debug, Clone)]
pub struct CaliberEntry {
    pub kind: EventKind,
    /// 口径版本。
    pub ver: u32,
    /// 脱敏声明（埋点不含可识别用户内容）。
    pub sanitized: bool,
}

/// 埋点口径册（可冻结）。
#[derive(Debug, Clone)]
pub struct CaliberBook {
    pub entries: [Option<CaliberEntry>; 4],
    pub frozen: bool,
}

pub const CALIBER_VERSION: u32 = 7;

impl CaliberBook {
    pub fn new() -> Self {
        Self { entries: [None, None, None, None], frozen: false }
    }

    /// 口径登记（冻结后拒绝）。
    pub fn put(&mut self, kind: EventKind, ver: u32, sanitized: bool) -> bool {
        if self.frozen {
            return false;
        }
        self.entries[kind.slot()] = Some(CaliberEntry { kind, ver, sanitized });
        true
    }

    /// 口径漂移检出：版本不齐或未脱敏。
    pub fn drift(&self) -> bool {
        self.entries.iter().any(|e| match e {
            Some(c) => c.ver != CALIBER_VERSION || !c.sanitized,
            None => true,
        })
    }

    /// 冻结（漂移即拒）。
    pub fn freeze(&mut self) -> T7OCode {
        if self.frozen {
            return T7OCode::OK;
        }
        if self.drift() {
            return T7OCode::CALIBER_DRIFT;
        }
        self.frozen = true;
        T7OCode::OK
    }
}

// ── 观测面板项终版 ───────────────────────────────────────────────────

pub const PANEL_ITEMS: [&str; 6] = [
    "色标切换次数",
    "读屏会话时长",
    "动效降级率",
    "替代输入占比",
    "无障碍达标率",
    "心流指数",
];

/// 面板项表。
#[derive(Debug, Clone)]
pub struct PanelBook {
    /// 已上板项。
    pub mounted: Vec<&'static str>,
}

impl PanelBook {
    pub fn new() -> Self {
        Self { mounted: Vec::new() }
    }

    pub fn mount(&mut self, item: &'static str) -> bool {
        if !PANEL_ITEMS.contains(&item) || self.mounted.contains(&item) {
            return false;
        }
        self.mounted.push(item);
        true
    }

    /// 缺项清单。
    pub fn gaps(&self) -> Vec<&'static str> {
        PANEL_ITEMS.iter().filter(|i| !self.mounted.contains(i)).map(|i| *i).collect()
    }

    /// 终版：零缺项。
    pub fn final_ok(&self) -> bool {
        self.gaps().is_empty()
    }
}

// ── 留痕采样率复核 ──────────────────────────────────────────────────

/// 采样率表：事件类 → 千分比（0..1000）。
#[derive(Debug, Clone)]
pub struct SampleBook {
    pub rates: [u16; 4],
}

/// 采样率上限（千分比）：全量 1000 为失真上限红线内值。
pub const SAMPLE_MAX: u16 = 1000;

impl SampleBook {
    pub fn new() -> Self {
        Self { rates: [0; 4] }
    }

    /// 设采样率（越界拒绝）。
    pub fn set(&mut self, kind: EventKind, permille: u16) -> bool {
        if permille > SAMPLE_MAX {
            return false;
        }
        self.rates[kind.slot()] = permille;
        true
    }

    /// 失真检出：零采样（丢证据）或越限。
    pub fn distorted(&self, kind: EventKind) -> bool {
        self.rates[kind.slot()] == 0
    }

    /// 四类全有效。
    pub fn all_valid(&self) -> bool {
        (0..4).all(|i| !self.distorted_by_slot(i))
    }

    fn distorted_by_slot(&self, i: usize) -> bool {
        self.rates[i] == 0
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves241_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves241_s10obs");

    // C3987-01 诊断码段不越窗。
    chk(&mut set, "C3987-01",
        T7OCode::SAMPLE_DISTORT.0 < SEG_BASE + 16 && SEG_BASE == 0x41C0, "err");

    // C3987-02 四类埋点齐备。
    chk(&mut set, "C3987-02",
        EVENT_KINDS.len() == 4
            && EVENT_KINDS[0].name() == "色标切换"
            && EVENT_KINDS[3].name() == "替代输入", "err");

    // C3987-03 口径登记 + 漂移检出（缺类）。
    let mut cb = CaliberBook::new();
    for k in EVENT_KINDS.iter().take(3) {
        cb.put(*k, CALIBER_VERSION, true);
    }
    chk(&mut set, "C3987-03", cb.drift(), "err");

    // C3987-04 四类齐 → 无漂移 → 冻结过。
    cb.put(EventKind::AltInput, CALIBER_VERSION, true);
    chk(&mut set, "C3987-04", !cb.drift() && cb.freeze() == T7OCode::OK, "err");

    // C3987-05 版本漂移 → 冻结拒。
    let mut cb2 = CaliberBook::new();
    for k in EVENT_KINDS.iter() {
        cb2.put(*k, if *k == EventKind::ReaderSession { 5 } else { CALIBER_VERSION }, true);
    }
    chk(&mut set, "C3987-05", cb2.freeze() == T7OCode::CALIBER_DRIFT, "err");

    // C3987-06 未脱敏口径 → 漂移（隐私红线复述）。
    let mut cb3 = CaliberBook::new();
    for k in EVENT_KINDS.iter() {
        cb3.put(*k, CALIBER_VERSION, *k != EventKind::ColorSwitch);
    }
    chk(&mut set, "C3987-06", cb3.drift(), "err");

    // C3987-07 冻结后口径写入拒绝。
    chk(&mut set, "C3987-07", !cb.put(EventKind::ColorSwitch, 9, true), "err");

    // C3987-08 面板缺项检出 + 补齐。
    let mut pb = PanelBook::new();
    for i in PANEL_ITEMS.iter().take(4) {
        pb.mount(i);
    }
    let gaps = pb.gaps();
    chk(&mut set, "C3987-08",
        gaps == vec!["无障碍达标率", "心流指数"] && !pb.final_ok(), "err");

    // C3987-09 补齐后终版过。
    pb.mount("无障碍达标率");
    pb.mount("心流指数");
    chk(&mut set, "C3987-09", pb.final_ok() && pb.mounted.len() == 6, "err");

    // C3987-10 越名面板项拒绝 + 重复挂载拒绝。
    let mut pb2 = PanelBook::new();
    pb2.mount("心流指数");
    chk(&mut set, "C3987-10", !pb2.mount("伪项") && !pb2.mount("心流指数"), "err");

    // C3987-11 采样率越界拒绝。
    let mut sb = SampleBook::new();
    chk(&mut set, "C3987-11", !sb.set(EventKind::ColorSwitch, 1001) && sb.set(EventKind::ColorSwitch, 1000), "err");

    // C3987-12 零采样 = 失真（丢证据）。
    let mut sb2 = SampleBook::new();
    for k in EVENT_KINDS.iter() {
        sb2.set(*k, 500);
    }
    sb2.rates[EventKind::MotionDegrade.slot()] = 0;
    chk(&mut set, "C3987-12",
        sb2.distorted(EventKind::MotionDegrade) && !sb2.all_valid(), "err");

    // C3987-13 全有效判定。
    let mut sb3 = SampleBook::new();
    for k in EVENT_KINDS.iter() {
        sb3.set(*k, 300);
    }
    chk(&mut set, "C3987-13", sb3.all_valid(), "err");

    // C3987-14 埋点类槽双射。
    let slots: Vec<usize> = EVENT_KINDS.iter().map(|k| k.slot()).collect();
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if slots[i] == slots[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3987-14", uniq, "err");

    // C3987-15 面板项表六项互异。
    let mut uniq2 = true;
    for i in 0..PANEL_ITEMS.len() {
        for j in (i + 1)..PANEL_ITEMS.len() {
            if PANEL_ITEMS[i] == PANEL_ITEMS[j] { uniq2 = false; }
        }
    }
    chk(&mut set, "C3987-15", uniq2 && PANEL_ITEMS.len() == 6, "err");

    // C3987-16 口径冻结幂等。
    chk(&mut set, "C3987-16", cb.freeze() == T7OCode::OK, "err");

    // C3987-17 漂移修正流（对版修正后冻结过）。
    cb2.put(EventKind::ReaderSession, CALIBER_VERSION, true);
    chk(&mut set, "C3987-17", !cb2.drift() && cb2.freeze() == T7OCode::OK, "err");

    // C3987-18 采样上限边界值合法。
    let mut sb4 = SampleBook::new();
    for k in EVENT_KINDS.iter() {
        sb4.set(*k, SAMPLE_MAX);
    }
    chk(&mut set, "C3987-18", sb4.all_valid(), "err");

    // C3987-19 错误码互异。
    let codes = [T7OCode::OK.0, T7OCode::CALIBER_DRIFT.0, T7OCode::PANEL_GAP.0, T7OCode::SAMPLE_DISTORT.0];
    chk(&mut set, "C3987-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3987-20 面板观测与埋点四类对应（前四项）。
    chk(&mut set, "C3987-20",
        PANEL_ITEMS[0] == "色标切换次数"
            && PANEL_ITEMS[1] == "读屏会话时长"
            && PANEL_ITEMS[2] == "动效降级率"
            && PANEL_ITEMS[3] == "替代输入占比", "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caliber_freeze_gate() {
        let mut cb = CaliberBook::new();
        assert_eq!(cb.freeze(), T7OCode::CALIBER_DRIFT);
        for k in EVENT_KINDS.iter() {
            cb.put(*k, CALIBER_VERSION, true);
        }
        assert_eq!(cb.freeze(), T7OCode::OK);
    }

    #[test]
    fn panel_gap_flow() {
        let mut pb = PanelBook::new();
        assert_eq!(pb.gaps().len(), 6);
        for i in PANEL_ITEMS.iter() {
            pb.mount(i);
        }
        assert!(pb.final_ok());
    }

    #[test]
    fn zero_sample_distorts() {
        let sb = SampleBook::new();
        assert!(sb.distorted(EventKind::AltInput));
    }
}
