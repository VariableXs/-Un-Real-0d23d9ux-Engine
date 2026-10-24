#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD600）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD6Code(pub u16);

impl CD6Code {
    pub const SUBTITLE_OCCLUDE: Self = Self(SEG_BASE);       // 字幕遮挡
    pub const SUBTITLE_MISALIGN: Self = Self(SEG_BASE + 1);  // 时序错位
    pub const DESCR_SILENT_MISS: Self = Self(SEG_BASE + 2);  // 描述静默缺
    pub const LATENCY_LIE: Self = Self(SEG_BASE + 3);        // 延迟虚报
    pub const STYLE_FAIL: Self = Self(SEG_BASE + 4);         // 样式不生效
    pub const TRACK_LOST: Self = Self(SEG_BASE + 5);         // 轨丢失

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::SUBTITLE_OCCLUDE => "字幕遮挡",
            Self::SUBTITLE_MISALIGN => "字幕时序错位",
            Self::DESCR_SILENT_MISS => "描述静默缺失",
            Self::LATENCY_LIE => "延迟虚报",
            Self::STYLE_FAIL => "样式不生效",
            Self::TRACK_LOST => "轨丢失",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 6] = [
        Self::SUBTITLE_OCCLUDE, Self::SUBTITLE_MISALIGN, Self::DESCR_SILENT_MISS,
        Self::LATENCY_LIE, Self::STYLE_FAIL, Self::TRACK_LOST,
    ];
}

// ── 字幕轨 ──────────────────────────────────────────────────────────

/// 字幕样式偏好（用户可调）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleStyle {
    pub font_scale_pct: u16,   // 字号百分比 50..=300
    pub bg_opacity_pct: u8,    // 背景不透明度 0..=100
    pub position: SubPosition, // 位置
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubPosition { Top, Bottom, Custom(u8) }

impl Default for SubtitleStyle {
    fn default() -> Self { Self { font_scale_pct: 100, bg_opacity_pct: 80, position: SubPosition::Bottom } }
}

impl SubtitleStyle {
    /// 样式合法：字号 50..=300，背景 0..=100。
    pub fn valid(&self) -> bool {
        (50..=300).contains(&self.font_scale_pct) && self.bg_opacity_pct <= 100
    }
}

/// 单条字幕。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub track_id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: &'static str,
    /// 字幕框归一化坐标（0..1000）——用于遮挡检测。
    pub box_x: u16, pub box_y: u16, pub box_w: u16, pub box_h: u16,
}

impl SubtitleCue {
    /// 时序有效：end > start。
    pub fn timing_valid(&self) -> bool { self.end_ms > self.start_ms }
    /// 字幕框与画面主体区域相交检测（主体区 200..800 x 200..800）。
    pub fn occludes_subject(&self) -> bool {
        let (x0, y0) = (self.box_x, self.box_y);
        let (x1, y1) = (self.box_x + self.box_w, self.box_y + self.box_h);
        x0 < 800 && x1 > 200 && y0 < 800 && y1 > 200
    }
}

/// 字幕轨：多条字幕 + 样式 + 渲染状态。
#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    pub track_id: u32,
    pub lang: &'static str,
    pub style: SubtitleStyle,
    pub cues: Vec<SubtitleCue>,
    pub rendered: bool,
}

impl SubtitleTrack {
    pub fn new(track_id: u32, lang: &'static str) -> Self {
        Self { track_id, lang, style: SubtitleStyle::default(), cues: Vec::new(), rendered: false }
    }

    /// 字幕断言全集：时序有效 + 无遮挡 + 样式合法。
    pub fn subtitle_assert(&self) -> SubtitleAssert {
        let mut timing_ok = true;
        let mut occlude = false;
        for c in &self.cues {
            if !c.timing_valid() { timing_ok = false; }
            if c.occludes_subject() { occlude = true; }
        }
        SubtitleAssert {
            timing_ok,
            no_occlude: !occlude,
            style_ok: self.style.valid(),
        }
    }

    /// 按时刻取当前字幕。
    pub fn cue_at(&self, t_ms: u64) -> Option<&SubtitleCue> {
        self.cues.iter().find(|c| c.start_ms <= t_ms && t_ms < c.end_ms)
    }
}

/// 字幕断言结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleAssert {
    pub timing_ok: bool,
    pub no_occlude: bool,
    pub style_ok: bool,
}
impl SubtitleAssert {
    pub fn all_green(&self) -> bool { self.timing_ok && self.no_occlude && self.style_ok }
}

// ── 描述轨 ──────────────────────────────────────────────────────────

/// 描述轨（音频描述/口述影像）。
#[derive(Debug, Clone)]
pub struct DescriptionTrack {
    pub track_id: u32,
    pub available: bool,
    /// 描述缺失时是否显性标注（红线：无描述视频必须显性标注）。
    pub explicit_missing_mark: bool,
    pub segments: Vec<DescSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub kind: DescKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescKind { Scene, Action, OnScreenText, Emotion }

impl DescriptionTrack {
    pub fn new(track_id: u32) -> Self {
        Self { track_id, available: false, explicit_missing_mark: false, segments: Vec::new() }
    }

    /// 描述标记红线：不可用时必须显性标注缺失。
    pub fn desc_mark_ok(&self) -> bool { self.available || self.explicit_missing_mark }

    /// 覆盖率：描述段占视频时长比（千分比）。
    pub fn coverage_permille(&self, video_dur_ms: u64) -> u32 {
        if video_dur_ms == 0 { return 0; }
        let covered: u64 = self.segments.iter().map(|s| s.end_ms.saturating_sub(s.start_ms)).sum();
        ((covered * 1000) / video_dur_ms).min(1000) as u32
    }
}

// ── 延迟标注（诚实红线）─────────────────────────────────────────────

/// 实时字幕延迟标注：真实延迟必须如实显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyMark {
    pub measured_ms: u32,
    pub displayed_ms: u32,
}

impl LatencyMark {
    /// 延迟诚实：显示值与实测值偏差 ≤500ms（红线）。
    pub fn honest(&self) -> bool {
        let diff = if self.measured_ms > self.displayed_ms {
            self.measured_ms - self.displayed_ms
        } else {
            self.displayed_ms - self.measured_ms
        };
        diff <= 500
    }
    /// 是否虚报（显示远低于实测=作恶）。
    pub fn lying(&self) -> bool {
        self.measured_ms > self.displayed_ms + 1000
    }
}

// ── 视频无障碍会话 ──────────────────────────────────────────────────

/// 视频无障碍会话：字幕 + 描述 + 延迟标注统一管理。
#[derive(Debug, Clone)]
pub struct VideoA11ySession {
    pub subtitles: Vec<SubtitleTrack>,
    pub description: DescriptionTrack,
    pub latency: LatencyMark,
    pub video_dur_ms: u64,
}

impl VideoA11ySession {
    pub fn new(video_dur_ms: u64) -> Self {
        Self {
            subtitles: Vec::new(),
            description: DescriptionTrack::new(0),
            latency: LatencyMark { measured_ms: 0, displayed_ms: 0 },
            video_dur_ms,
        }
    }

    /// 全部字幕断言。
    pub fn all_subtitle_ok(&self) -> bool {
        !self.subtitles.is_empty() && self.subtitles.iter().all(|t| t.subtitle_assert().all_green())
    }

    /// 描述红线 + 延迟诚实 综合。
    pub fn media_honesty_ok(&self) -> bool {
        self.description.desc_mark_ok() && !self.latency.lying()
    }

    /// 审计结果。
    pub fn audit(&self) -> VideoAudit {
        VideoAudit {
            subtitle_ok: self.all_subtitle_ok(),
            desc_mark_ok: self.description.desc_mark_ok(),
            latency_honest: self.latency.honest() && !self.latency.lying(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoAudit {
    pub subtitle_ok: bool,
    pub desc_mark_ok: bool,
    pub latency_honest: bool,
}
impl VideoAudit {
    pub fn all_green(&self) -> bool { self.subtitle_ok && self.desc_mark_ok && self.latency_honest }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3863_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4a_video");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 字幕时序有效
    {
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 0, end_ms: 1000, text: "a", box_x: 0, box_y: 0, box_w: 100, box_h: 50 });
        chk(&mut set, "C3863-01", t.subtitle_assert().timing_ok, "err");
    }
    // 2 字幕时序无效检出
    {
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 1000, end_ms: 500, text: "a", box_x: 0, box_y: 0, box_w: 100, box_h: 50 });
        chk(&mut set, "C3863-02", !t.subtitle_assert().timing_ok, "err");
    }
    // 3 字幕遮挡主体检出
    {
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 0, end_ms: 1000, text: "a", box_x: 300, box_y: 300, box_w: 200, box_h: 100 });
        chk(&mut set, "C3863-03", !t.subtitle_assert().no_occlude, "err");
    }
    // 4 字幕不遮挡
    {
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 0, end_ms: 1000, text: "a", box_x: 0, box_y: 900, box_w: 1000, box_h: 80 });
        chk(&mut set, "C3863-04", t.subtitle_assert().no_occlude, "err");
    }
    // 5 样式合法
    {
        let s = SubtitleStyle::default();
        chk(&mut set, "C3863-05", s.valid(), "err");
    }
    // 6 样式非法检出
    {
        let s = SubtitleStyle { font_scale_pct: 400, bg_opacity_pct: 80, position: SubPosition::Bottom };
        chk(&mut set, "C3863-06", !s.valid(), "err");
    }
    // 7 描述缺失必须显性标注
    {
        let d = DescriptionTrack::new(1);
        chk(&mut set, "C3863-07", !d.desc_mark_ok(), "err");
    }
    // 8 描述缺失+显性标注=通过
    {
        let mut d = DescriptionTrack::new(1);
        d.explicit_missing_mark = true;
        chk(&mut set, "C3863-08", d.desc_mark_ok(), "err");
    }
    // 9 描述可用无需标注
    {
        let mut d = DescriptionTrack::new(1);
        d.available = true;
        chk(&mut set, "C3863-09", d.desc_mark_ok(), "err");
    }
    // 10 延迟诚实
    {
        let l = LatencyMark { measured_ms: 800, displayed_ms: 750 };
        chk(&mut set, "C3863-10", l.honest() && !l.lying(), "err");
    }
    // 11 延迟虚报检出
    {
        let l = LatencyMark { measured_ms: 5000, displayed_ms: 500 };
        chk(&mut set, "C3863-11", l.lying(), "err");
    }
    // 12 描述覆盖率
    {
        let mut d = DescriptionTrack::new(1);
        d.segments.push(DescSegment { start_ms: 0, end_ms: 5000, kind: DescKind::Scene });
        chk(&mut set, "C3863-12", d.coverage_permille(10000) == 500, "err");
    }
    // 13 按时刻取字幕
    {
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 100, end_ms: 200, text: "a", box_x: 0, box_y: 0, box_w: 10, box_h: 10 });
        chk(&mut set, "C3863-13", t.cue_at(150).is_some() && t.cue_at(250).is_none(), "err");
    }
    // 14 视频会话审计全绿
    {
        let mut v = VideoA11ySession::new(10000);
        let mut t = SubtitleTrack::new(1, "zh");
        t.cues.push(SubtitleCue { track_id: 1, start_ms: 0, end_ms: 5000, text: "a", box_x: 0, box_y: 900, box_w: 1000, box_h: 80 });
        v.subtitles.push(t);
        v.description.explicit_missing_mark = true;
        v.latency = LatencyMark { measured_ms: 200, displayed_ms: 200 };
        chk(&mut set, "C3863-14", v.audit().all_green(), "err");
    }
    // 15 轨丢失检出（空字幕集）
    {
        let v = VideoA11ySession::new(10000);
        chk(&mut set, "C3863-15", !v.all_subtitle_ok(), "err");
    }
    // 16 延迟偏差超限不诚实
    {
        let l = LatencyMark { measured_ms: 2000, displayed_ms: 500 };
        chk(&mut set, "C3863-16", !l.honest(), "err");
    }
    // 17 字幕样式 Custom 位置
    {
        let s = SubtitleStyle { font_scale_pct: 150, bg_opacity_pct: 50, position: SubPosition::Custom(42) };
        chk(&mut set, "C3863-17", s.valid(), "err");
    }
    // 18 描述段类型齐全
    {
        let mut d = DescriptionTrack::new(1);
        d.segments.push(DescSegment { start_ms: 0, end_ms: 100, kind: DescKind::Scene });
        d.segments.push(DescSegment { start_ms: 100, end_ms: 200, kind: DescKind::Action });
        d.segments.push(DescSegment { start_ms: 200, end_ms: 300, kind: DescKind::OnScreenText });
        d.segments.push(DescSegment { start_ms: 300, end_ms: 400, kind: DescKind::Emotion });
        chk(&mut set, "C3863-18", d.coverage_permille(1000) == 400, "err");
    }

    set
}
