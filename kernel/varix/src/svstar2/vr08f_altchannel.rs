//! VE-F3746 · 创作替代通道
//!
//! 锚点：创作替代通道（复用 F3145 模式跨域复刻：创作信息替代（创作过程信息
//! 非视觉替代（验证结果文本/状态播报/进度语音——复述完备断言创作化；创作反馈
//! 替代（操作反馈非视觉（编辑成功的音频/触觉反馈（复用 F3053/F3054 联动复述；
//! 通道质量（替代通道语义准确（复述一致；主通道切换（辅具使用时替代通道为主
//! 通道（复述切换断言。
//!
//! 数据结构：完备断言复用；反馈联动复述；一致复述；切换复用。
//! 错误路径与降级矩阵：通道缺失→P1（复述红线实测全域）；语义不一致→P1（复述）；
//! 切换失效→修正（复述）；反馈缺失→补（复述）。
//! 跨批对接点：F3145/F3053/F3054 单源复用声明；R01-R03 反馈对端；N05 语义对端。
//!
//! 诊断码段：独占 0x89xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x89xx）
// ─────────────────────────────────────────────────────────────

/// F3746 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A7Code(pub u16);

impl A7Code {
    /// 信息替代通道缺失——完备断言全域。
    pub const INFO_CHANNEL_MISS: A7Code = A7Code(0x8901);
    /// 反馈替代缺失——联动复述。
    pub const FEEDBACK_MISS: A7Code = A7Code(0x8902);
    /// 语义不一致——一致复述 P1。
    pub const SEMANTIC_MISMATCH: A7Code = A7Code(0x8903);
    /// 主通道切换失效——切换断言。
    pub const SWITCH_FAIL: A7Code = A7Code(0x8904);
    /// 进度语音缺失——复述项。
    pub const PROGRESS_VOICE_MISS: A7Code = A7Code(0x8905);
    /// 触觉反馈缺失——F3053/F3054 联动。
    pub const HAPTIC_MISS: A7Code = A7Code(0x8906);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8901 => "创作信息非视觉替代通道缺失，完备断言全域不通过。",
            0x8902 => "操作反馈非视觉替代缺失，联动复述不完整。",
            0x8903 => "替代通道语义与主通道不一致，P1 对账。",
            0x8904 => "辅具状态下主通道切换失效，切换断言不通过。",
            0x8905 => "进度语音缺失，复述项不完整。",
            0x8906 => "触觉反馈缺失，F3053/F3054 联动复述不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 创作信息替代（完备断言创作化）
// ─────────────────────────────────────────────────────────────

/// 信息项类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InfoSlot {
    /// 验证结果文本。
    VerifyText,
    /// 状态播报。
    StatusAnnounce,
    /// 进度语音。
    ProgressVoice,
    /// 错误提示文本。
    ErrorText,
}

impl InfoSlot {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            InfoSlot::VerifyText => 0,
            InfoSlot::StatusAnnounce => 1,
            InfoSlot::ProgressVoice => 2,
            InfoSlot::ErrorText => 3,
        }
    }

    /// 全集。
    pub fn all() -> [InfoSlot; 4] {
        [
            InfoSlot::VerifyText,
            InfoSlot::StatusAnnounce,
            InfoSlot::ProgressVoice,
            InfoSlot::ErrorText,
        ]
    }
}

/// 信息替代通道完备表。
#[derive(Clone, Debug, Default)]
pub struct InfoAltTable {
    /// 每槽是否有非视觉替代。
    slots: [bool; 4],
}

impl InfoAltTable {
    /// 全缺。
    pub fn new() -> InfoAltTable {
        InfoAltTable { slots: [false; 4] }
    }

    /// 打开某槽替代。
    pub fn open(&mut self, slot: InfoSlot) {
        self.slots[slot.index() as usize] = true;
    }

    /// 关闭（演练注入）。
    pub fn close(&mut self, slot: InfoSlot) {
        self.slots[slot.index() as usize] = false;
    }

    /// 槽是否已替代。
    pub fn covered(&self, slot: InfoSlot) -> bool {
        self.slots[slot.index() as usize]
    }

    /// 完备断言：全槽已替代。
    pub fn complete(&self) -> bool {
        self.slots.iter().all(|b| *b)
    }

    /// 缺失槽序号。
    pub fn missing(&self) -> Vec<u8> {
        let mut m = Vec::new();
        for s in InfoSlot::all() {
            if !self.covered(s) {
                m.push(s.index());
            }
        }
        m
    }
}

// ─────────────────────────────────────────────────────────────
// 创作反馈替代（F3053/F3054 联动复述）
// ─────────────────────────────────────────────────────────────

/// 反馈通道。
#[derive(Clone, Debug, Default)]
pub struct FeedbackAlt {
    /// 编辑成功音频。
    pub audio_ok: bool,
    /// 编辑成功触觉。
    pub haptic_ok: bool,
    /// 误操作触觉。
    pub haptic_err: bool,
}

impl FeedbackAlt {
    /// 全缺。
    pub fn new() -> FeedbackAlt {
        FeedbackAlt {
            audio_ok: false,
            haptic_ok: false,
            haptic_err: false,
        }
    }

    /// 联动完整：音频 + 双触觉。
    pub fn linked(&self) -> bool {
        self.audio_ok && self.haptic_ok && self.haptic_err
    }

    /// 缺失名清单。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if !self.audio_ok {
            m.push("audio_ok");
        }
        if !self.haptic_ok {
            m.push("haptic_ok");
        }
        if !self.haptic_err {
            m.push("haptic_err");
        }
        m
    }
}

// ─────────────────────────────────────────────────────────────
// 语义一致（复述一致）
// ─────────────────────────────────────────────────────────────

/// 语义对拍对。
#[derive(Clone, Debug)]
pub struct SemPair {
    /// 主通道语义 id。
    pub primary: u32,
    /// 替代通道语义 id（0=缺失）。
    pub alt: u32,
}

impl SemPair {
    /// 构造。
    pub fn new(primary: u32, alt: u32) -> SemPair {
        SemPair { primary, alt }
    }

    /// 一致。
    pub fn match_ok(&self) -> bool {
        self.alt != 0 && self.primary == self.alt
    }
}

/// 语义对拍表。
#[derive(Clone, Debug, Default)]
pub struct SemLedger {
    pairs: Vec<SemPair>,
}

impl SemLedger {
    /// 空表。
    pub fn new() -> SemLedger {
        SemLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, p: SemPair) {
        self.pairs.push(p);
    }

    /// 扫描不一致。
    pub fn scan_mismatch(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.pairs.iter() {
            if !p.match_ok() {
                bad.push(p.primary);
            }
        }
        bad
    }

    /// 对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 主通道切换（辅具使用时替代通道为主通道）
// ─────────────────────────────────────────────────────────────

/// 通道角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelRole {
    /// 主通道。
    Primary,
    /// 替代通道。
    Alt,
}

/// 切换器。
#[derive(Clone, Debug)]
pub struct ChannelSwitcher {
    /// 辅具是否激活。
    assist_active: bool,
    /// 当前主通道角色。
    primary_role: ChannelRole,
}

impl ChannelSwitcher {
    /// 构造（默认主通道为主）。
    pub fn new() -> ChannelSwitcher {
        ChannelSwitcher {
            assist_active: false,
            primary_role: ChannelRole::Primary,
        }
    }

    /// 激活辅具并切换。
    pub fn engage_assist(&mut self) -> bool {
        self.assist_active = true;
        self.primary_role = ChannelRole::Alt;
        true
    }

    /// 释放辅具。
    pub fn release_assist(&mut self) -> bool {
        self.assist_active = false;
        self.primary_role = ChannelRole::Primary;
        true
    }

    /// 切换断言：辅具激活时主通道必须是 Alt。
    pub fn switch_ok(&self) -> bool {
        if self.assist_active {
            self.primary_role == ChannelRole::Alt
        } else {
            self.primary_role == ChannelRole::Primary
        }
    }

    /// 注入切换失效（演练）。
    pub fn inject_fail(&mut self) {
        self.primary_role = ChannelRole::Primary;
    }

    /// 修正切换。
    pub fn fix_switch(&mut self) {
        if self.assist_active {
            self.primary_role = ChannelRole::Alt;
        }
    }

    /// 是否辅具激活。
    pub fn assist_on(&self) -> bool {
        self.assist_active
    }
}

impl Default for ChannelSwitcher {
    fn default() -> ChannelSwitcher {
        ChannelSwitcher::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作替代通道审查器。
#[derive(Clone, Debug, Default)]
pub struct AltChannelAuditor {
    info: InfoAltTable,
    feedback: FeedbackAlt,
    sem: SemLedger,
    switcher: ChannelSwitcher,
}

impl AltChannelAuditor {
    /// 构造。
    pub fn new() -> AltChannelAuditor {
        AltChannelAuditor {
            info: InfoAltTable::new(),
            feedback: FeedbackAlt::new(),
            sem: SemLedger::new(),
            switcher: ChannelSwitcher::new(),
        }
    }

    /// 取信息表（可变）。
    pub fn info_mut(&mut self) -> &mut InfoAltTable {
        &mut self.info
    }

    /// 取反馈（可变）。
    pub fn feedback_mut(&mut self) -> &mut FeedbackAlt {
        &mut self.feedback
    }

    /// 取语义表（可变）。
    pub fn sem_mut(&mut self) -> &mut SemLedger {
        &mut self.sem
    }

    /// 取切换器（可变）。
    pub fn switcher_mut(&mut self) -> &mut ChannelSwitcher {
        &mut self.switcher
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<A7Code> {
        let mut bad = Vec::new();
        if !self.info.complete() {
            bad.push(A7Code::INFO_CHANNEL_MISS);
        }
        if !self.feedback.linked() {
            bad.push(A7Code::FEEDBACK_MISS);
        }
        if !self.sem.scan_mismatch().is_empty() {
            bad.push(A7Code::SEMANTIC_MISMATCH);
        }
        if !self.switcher.switch_ok() {
            bad.push(A7Code::SWITCH_FAIL);
        }
        if !self.info.covered(InfoSlot::ProgressVoice) {
            bad.push(A7Code::PROGRESS_VOICE_MISS);
        }
        if !self.feedback.haptic_ok || !self.feedback.haptic_err {
            bad.push(A7Code::HAPTIC_MISS);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3746 判据集。
pub fn run_vr08f_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3746");

    // 0x8901 完备断言
    {
        let mut t = InfoAltTable::new();
        out.add("0x8901-全缺检出", !t.complete() && t.missing().len() == 4, "");
        for s in InfoSlot::all() {
            t.open(s);
        }
        out.add("0x8901-全开完备", t.complete() && t.missing().is_empty(), "");
        t.close(InfoSlot::VerifyText);
        out.add("0x8901-缺一槽检出", t.missing().len() == 1, "");
    }

    // 0x8902 反馈联动
    {
        let mut f = FeedbackAlt::new();
        out.add("0x8902-全缺拦", !f.linked() && f.missing().len() == 3, "");
        f.audio_ok = true;
        f.haptic_ok = true;
        f.haptic_err = true;
        out.add("0x8902-联动齐过", f.linked(), "");
    }

    // 0x8903 语义一致
    {
        let mut s = SemLedger::new();
        s.push(SemPair::new(10, 10));
        out.add("0x8903-一致过", s.scan_mismatch().is_empty(), "");
        s.push(SemPair::new(11, 12));
        out.add("0x8903-不一致检出", s.scan_mismatch().len() == 1, "");
    }

    // 0x8904 切换断言
    {
        let mut sw = ChannelSwitcher::new();
        out.add("0x8904-默认过", sw.switch_ok(), "");
        sw.engage_assist();
        out.add("0x8904-辅具后主切换", sw.switch_ok() && sw.assist_on(), "");
        sw.inject_fail();
        out.add("0x8904-失效检出", !sw.switch_ok(), "");
        sw.fix_switch();
        out.add("0x8904-修正后过", sw.switch_ok(), "");
    }

    // 判据
    {
        let mut a = AltChannelAuditor::new();
        let bad = a.audit();
        out.add(
            "0x8905-进度语音缺失",
            bad.iter().any(|c| c.raw() == 0x8905),
            "",
        );
        out.add("0x8906-触觉缺失", bad.iter().any(|c| c.raw() == 0x8906), "");
    }

    out
}
