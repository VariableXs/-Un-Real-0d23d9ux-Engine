#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD680）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD680;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD6BCode(pub u16);

impl CD6BCode {
    pub const EQUIV_MISSING: Self = Self(SEG_BASE);       // 等价缺失
    pub const ALT_BREAK_GATE: Self = Self(SEG_BASE + 1);  // 替代破门
    pub const TRANSCRIPT_LIE: Self = Self(SEG_BASE + 2);  // 转写虚延迟
    pub const SONAR_DISTORT: Self = Self(SEG_BASE + 3);   // 声呐失真
    pub const PHOTOSAFE_FAIL: Self = Self(SEG_BASE + 4);  // 光敏门失败

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::EQUIV_MISSING => "等价缺失",
            Self::ALT_BREAK_GATE => "替代破门",
            Self::TRANSCRIPT_LIE => "转写虚延迟",
            Self::SONAR_DISTORT => "声呐失真",
            Self::PHOTOSAFE_FAIL => "光敏门失败",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::EQUIV_MISSING, Self::ALT_BREAK_GATE, Self::TRANSCRIPT_LIE,
        Self::SONAR_DISTORT, Self::PHOTOSAFE_FAIL,
    ];
}

// ── 转写渲染 ────────────────────────────────────────────────────────

/// 音频转写文本段（含延迟标注）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: &'static str,
    pub latency_ms: u32,
}

impl TranscriptSegment {
    /// 转写延迟诚实：延迟 ≤3000ms 且如实标注。
    pub fn honest_latency(&self) -> bool { self.latency_ms <= 3000 }
}

/// 转写流：多段 + 延迟标注。
#[derive(Debug, Clone)]
pub struct TranscriptStream {
    pub segments: Vec<TranscriptSegment>,
    pub delay_marked: bool,
}

impl TranscriptStream {
    pub fn new() -> Self { Self { segments: Vec::new(), delay_marked: false } }

    /// 转写延迟红线：全部段延迟诚实 + 延迟有标注。
    pub fn transcript_ok(&self) -> bool {
        self.delay_marked && self.segments.iter().all(|s| s.honest_latency())
    }
}

// ── 声源可视化（视觉声呐）───────────────────────────────────────────

/// 声源事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSource {
    pub id: u32,
    /// 方位角 0..360。
    pub azimuth_deg: u16,
    /// 距离档：0=近，1=中，2=远。
    pub distance_tier: u8,
    pub kind: SoundKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind { Speech, Alert, Ambient, Music, Footstep }

/// 视觉声呐：声源方向/类型可视化。
#[derive(Debug, Clone)]
pub struct VisualSonar {
    pub sources: Vec<SoundSource>,
    pub rendered: bool,
}

impl VisualSonar {
    pub fn new() -> Self { Self { sources: Vec::new(), rendered: false } }

    /// 声呐红线：所有声源均已渲染可视化（仅靠声音提示必须有视觉等价）。
    pub fn equiv_complete(&self) -> bool {
        self.rendered && !self.sources.is_empty()
    }

    /// 声呐数据有效：方位 0..360，距离 0..2。
    pub fn data_valid(&self) -> bool {
        self.sources.iter().all(|s| s.azimuth_deg < 360 && s.distance_tier <= 2)
    }

    /// 按类型过滤。
    pub fn by_kind(&self, kind: SoundKind) -> Vec<&SoundSource> {
        self.sources.iter().filter(|s| s.kind == kind).collect()
    }
}

// ── 音量事件视觉替代 ────────────────────────────────────────────────

/// 音量事件→视觉闪烁替代（须过光敏门）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeFlashAlt {
    /// 闪烁频率 Hz（光敏红线：3..=30 Hz 危险）。
    pub flash_freq_hz: u16,
    /// 是否用光敏安全替代（恒亮/渐变）。
    pub photosafe_mode: bool,
}

impl VolumeFlashAlt {
    /// 替代自检：光敏安全模式下不得使用危险闪烁频率。
    pub fn alt_selfcheck(&self) -> bool {
        if self.photosafe_mode {
            // 光敏安全替代不允许 3..=30 Hz 闪烁
            !(3..=30).contains(&self.flash_freq_hz)
        } else {
            true // 非光敏安全模式由独立光敏门管
        }
    }

    /// 光敏门（复用 F3809 正交：闪烁频率检查）。
    pub fn photosafe_gate(&self) -> bool {
        self.photosafe_mode || !(3..=30).contains(&self.flash_freq_hz)
    }
}

// ── 静音可视化 ──────────────────────────────────────────────────────

/// 静音态渲染标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuteIndicator {
    pub muted: bool,
    pub rendered: bool,
}

impl MuteIndicator {
    /// 状态可辨：静音态必须渲染标识。
    pub fn discernible(&self) -> bool { !self.muted || self.rendered }
}

// ── 音频替代会话 ────────────────────────────────────────────────────

/// 音频替代会话：转写 + 声呐 + 替代 + 静音。
#[derive(Debug, Clone)]
pub struct AudioAltSession {
    pub transcript: TranscriptStream,
    pub sonar: VisualSonar,
    pub flash_alt: VolumeFlashAlt,
    pub mute: MuteIndicator,
}

impl AudioAltSession {
    pub fn new() -> Self {
        Self {
            transcript: TranscriptStream::new(),
            sonar: VisualSonar::new(),
            flash_alt: VolumeFlashAlt { flash_freq_hz: 1, photosafe_mode: true },
            mute: MuteIndicator { muted: false, rendered: true },
        }
    }

    /// 替代完备审计。
    pub fn audit(&self) -> AudioAltAudit {
        AudioAltAudit {
            transcript_ok: self.transcript.transcript_ok(),
            sonar_equiv: self.sonar.equiv_complete() && self.sonar.data_valid(),
            alt_gate_ok: self.flash_alt.alt_selfcheck() && self.flash_alt.photosafe_gate(),
            mute_ok: self.mute.discernible(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioAltAudit {
    pub transcript_ok: bool,
    pub sonar_equiv: bool,
    pub alt_gate_ok: bool,
    pub mute_ok: bool,
}
impl AudioAltAudit {
    pub fn all_green(&self) -> bool { self.transcript_ok && self.sonar_equiv && self.alt_gate_ok && self.mute_ok }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3864_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4b_audioalt");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 转写延迟诚实
    {
        let s = TranscriptSegment { start_ms: 0, end_ms: 1000, text: "hi", latency_ms: 500 };
        chk(&mut set, "C3864-01", s.honest_latency(), "err");
    }
    // 2 转写延迟超标
    {
        let s = TranscriptSegment { start_ms: 0, end_ms: 1000, text: "hi", latency_ms: 5000 };
        chk(&mut set, "C3864-02", !s.honest_latency(), "err");
    }
    // 3 转写流红线（无标注）
    {
        let t = TranscriptStream::new();
        chk(&mut set, "C3864-03", !t.transcript_ok(), "err");
    }
    // 4 转写流红线（有标注+诚实）
    {
        let mut t = TranscriptStream::new();
        t.delay_marked = true;
        t.segments.push(TranscriptSegment { start_ms: 0, end_ms: 100, text: "a", latency_ms: 100 });
        chk(&mut set, "C3864-04", t.transcript_ok(), "err");
    }
    // 5 声呐等价完备
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 90, distance_tier: 1, kind: SoundKind::Alert });
        s.rendered = true;
        chk(&mut set, "C3864-05", s.equiv_complete(), "err");
    }
    // 6 声呐未渲染=等价缺失
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 90, distance_tier: 1, kind: SoundKind::Alert });
        chk(&mut set, "C3864-06", !s.equiv_complete(), "err");
    }
    // 7 声呐数据有效
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 359, distance_tier: 2, kind: SoundKind::Speech });
        chk(&mut set, "C3864-07", s.data_valid(), "err");
    }
    // 8 声呐数据非法（方位越界）
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 400, distance_tier: 1, kind: SoundKind::Speech });
        chk(&mut set, "C3864-08", !s.data_valid(), "err");
    }
    // 9 替代自检（光敏安全+低频）
    {
        let a = VolumeFlashAlt { flash_freq_hz: 1, photosafe_mode: true };
        chk(&mut set, "C3864-09", a.alt_selfcheck(), "err");
    }
    // 10 替代破门（光敏安全+危险频率）
    {
        let a = VolumeFlashAlt { flash_freq_hz: 10, photosafe_mode: true };
        chk(&mut set, "C3864-10", !a.alt_selfcheck(), "err");
    }
    // 11 光敏门
    {
        let a = VolumeFlashAlt { flash_freq_hz: 60, photosafe_mode: false };
        chk(&mut set, "C3864-11", a.photosafe_gate(), "err");
    }
    // 12 光敏门失败（非安全模式+危险频率）
    {
        let a = VolumeFlashAlt { flash_freq_hz: 15, photosafe_mode: false };
        chk(&mut set, "C3864-12", !a.photosafe_gate(), "err");
    }
    // 13 静音可辨
    {
        let m = MuteIndicator { muted: true, rendered: true };
        chk(&mut set, "C3864-13", m.discernible(), "err");
    }
    // 14 静音不可辨
    {
        let m = MuteIndicator { muted: true, rendered: false };
        chk(&mut set, "C3864-14", !m.discernible(), "err");
    }
    // 15 声呐按类型过滤
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 0, distance_tier: 0, kind: SoundKind::Alert });
        s.sources.push(SoundSource { id: 2, azimuth_deg: 180, distance_tier: 1, kind: SoundKind::Speech });
        chk(&mut set, "C3864-15", s.by_kind(SoundKind::Alert).len() == 1, "err");
    }
    // 16 会话审计全绿
    {
        let mut a = AudioAltSession::new();
        a.transcript.delay_marked = true;
        a.transcript.segments.push(TranscriptSegment { start_ms: 0, end_ms: 100, text: "x", latency_ms: 50 });
        a.sonar.sources.push(SoundSource { id: 1, azimuth_deg: 45, distance_tier: 0, kind: SoundKind::Alert });
        a.sonar.rendered = true;
        chk(&mut set, "C3864-16", a.audit().all_green(), "err");
    }
    // 17 距离档越界
    {
        let mut s = VisualSonar::new();
        s.sources.push(SoundSource { id: 1, azimuth_deg: 0, distance_tier: 5, kind: SoundKind::Ambient });
        chk(&mut set, "C3864-17", !s.data_valid(), "err");
    }
    // 18 零声源不等价
    {
        let mut s = VisualSonar::new();
        s.rendered = true;
        chk(&mut set, "C3864-18", !s.equiv_complete(), "err");
    }

    set
}
