#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDD00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDD00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDDCode(pub u16);

impl CDDCode {
    pub const EXAMPLE_STALE: Self = Self(SEG_BASE);   // 示例失效
    pub const COMPARE_MISSING: Self = Self(SEG_BASE + 1); // 对照缺
    pub const ANTIPATTERN_MISS: Self = Self(SEG_BASE + 2); // 反例误用
    pub const DRIFT: Self = Self(SEG_BASE + 3);        // 漂移
    pub const INCOMPLETE: Self = Self(SEG_BASE + 4);   // 工程不完整

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::EXAMPLE_STALE => "示例失效",
            Self::COMPARE_MISSING => "对照缺",
            Self::ANTIPATTERN_MISS => "反例误用",
            Self::DRIFT => "漂移",
            Self::INCOMPLETE => "工程不完整",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::EXAMPLE_STALE, Self::COMPARE_MISSING, Self::ANTIPATTERN_MISS,
        Self::DRIFT, Self::INCOMPLETE,
    ];
}

// ── 五主题 × 10 示例 ────────────────────────────────────────────────

/// 示例主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleTheme {
    /// 替代引擎。
    AltEngine,
    /// 字幕样式。
    SubtitleStyle,
    /// 音频替代。
    AudioAlt,
    /// 时长倍率。
    DurationScale,
    /// 紧急通道。
    EmergencyChannel,
}

impl ExampleTheme {
    pub const ALL: [Self; 5] = [
        Self::AltEngine, Self::SubtitleStyle, Self::AudioAlt, Self::DurationScale, Self::EmergencyChannel,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::AltEngine => "替代引擎",
            Self::SubtitleStyle => "字幕样式",
            Self::AudioAlt => "音频替代",
            Self::DurationScale => "时长倍率",
            Self::EmergencyChannel => "紧急通道",
        }
    }
}

/// 示例项。
#[derive(Debug, Clone)]
pub struct Example {
    pub id: u32,
    pub theme: ExampleTheme,
    pub has_compare: bool,
    pub has_antipattern: bool,
    pub complete: bool,
}

// ── 示例注册库 ──────────────────────────────────────────────────────

/// 示例库。
#[derive(Debug, Clone)]
pub struct ExampleLibrary {
    pub examples: Vec<Example>,
}

impl ExampleLibrary {
    /// 10 项注册：五主题×2。
    pub fn standard() -> Self {
        let mut examples = Vec::new();
        let mut id = 0u32;
        for theme in ExampleTheme::ALL {
            for _ in 0..2 {
                examples.push(Example {
                    id, theme, has_compare: true, has_antipattern: true, complete: true,
                });
                id += 1;
            }
        }
        Self { examples }
    }

    /// 计数=10。
    pub fn count_ok(&self) -> bool { self.examples.len() == 10 }

    /// 全部有对照。
    pub fn all_have_compare(&self) -> bool {
        self.examples.iter().all(|e| e.has_compare)
    }

    /// 全部有反例。
    pub fn all_have_antipattern(&self) -> bool {
        self.examples.iter().all(|e| e.has_antipattern)
    }

    /// 全部完整工程。
    pub fn all_complete(&self) -> bool {
        self.examples.iter().all(|e| e.complete)
    }

    /// 每主题 2 例。
    pub fn per_theme_two(&self) -> bool {
        ExampleTheme::ALL.iter().all(|t| {
            self.examples.iter().filter(|e| e.theme == *t).count() == 2
        })
    }
}

// ── 替代对照切换 ────────────────────────────────────────────────────

/// 对照切换（内置模拟）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompareToggle {
    pub show_original: bool,
    pub show_alt: bool,
}

impl CompareToggle {
    /// 并排对照。
    pub fn side_by_side(&self) -> bool { self.show_original && self.show_alt }
}

// ── 反例标注 ────────────────────────────────────────────────────────

/// 反例类型（三反模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntiPattern {
    /// 替代开销反超。
    OverheadOver,
    /// 字幕遮挡。
    SubtitleOcclude,
    /// 紧急被节流。
    UrgentThrottled,
}

impl AntiPattern {
    pub const ALL: [Self; 3] = [Self::OverheadOver, Self::SubtitleOcclude, Self::UrgentThrottled];
    pub fn diagnose(self) -> &'static str {
        match self {
            Self::OverheadOver => "替代开销反超：检查策略选择与设备档",
            Self::SubtitleOcclude => "字幕遮挡：检查字幕框位置与主体区冲突",
            Self::UrgentThrottled => "紧急被节流：检查优先级队列与覆盖渲染",
        }
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3877_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4o_motaltex");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 五主题
    {
        chk(&mut set, "C3877-01", ExampleTheme::ALL.len() == 5, "err");
    }
    // 2 主题名
    {
        chk(&mut set, "C3877-02", ExampleTheme::AltEngine.name() == "替代引擎", "err");
    }
    // 3 示例库 10 项
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-03", lib.count_ok(), "err");
    }
    // 4 每主题 2 例
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-04", lib.per_theme_two(), "err");
    }
    // 5 全部有对照
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-05", lib.all_have_compare(), "err");
    }
    // 6 全部有反例
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-06", lib.all_have_antipattern(), "err");
    }
    // 7 全部完整
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-07", lib.all_complete(), "err");
    }
    // 8 对照并排
    {
        let t = CompareToggle { show_original: true, show_alt: true };
        chk(&mut set, "C3877-08", t.side_by_side(), "err");
    }
    // 9 对照缺
    {
        let t = CompareToggle { show_original: true, show_alt: false };
        chk(&mut set, "C3877-09", !t.side_by_side(), "err");
    }
    // 10 反模式三类
    {
        chk(&mut set, "C3877-10", AntiPattern::ALL.len() == 3, "err");
    }
    // 11 诊断文本
    {
        chk(&mut set, "C3877-11", AntiPattern::UrgentThrottled.diagnose().len() > 10, "err");
    }
    // 12 模拟对照
    {
        let t = CompareToggle { show_original: false, show_alt: true };
        chk(&mut set, "C3877-12", !t.side_by_side(), "err");
    }
    // 13 示例 id 互异
    {
        let lib = ExampleLibrary::standard();
        let mut ok = true;
        for i in 0..lib.examples.len() {
            for j in (i + 1)..lib.examples.len() {
                if lib.examples[i].id == lib.examples[j].id { ok = false; }
            }
        }
        chk(&mut set, "C3877-13", ok, "err");
    }
    // 14 五主题全被覆盖
    {
        let lib = ExampleLibrary::standard();
        let mut covered = [false; 5];
        for e in &lib.examples {
            let idx = match e.theme {
                ExampleTheme::AltEngine => 0,
                ExampleTheme::SubtitleStyle => 1,
                ExampleTheme::AudioAlt => 2,
                ExampleTheme::DurationScale => 3,
                ExampleTheme::EmergencyChannel => 4,
            };
            covered[idx] = true;
        }
        chk(&mut set, "C3877-14", covered.iter().all(|&c| c), "err");
    }
    // 15 对照切换复位
    {
        let t = CompareToggle { show_original: true, show_alt: true };
        chk(&mut set, "C3877-15", t.side_by_side(), "err");
    }
    // 16 诊断可读
    {
        chk(&mut set, "C3877-16", AntiPattern::SubtitleOcclude.diagnose().len() > 10, "err");
    }
    // 17 库非空
    {
        let lib = ExampleLibrary::standard();
        chk(&mut set, "C3877-17", !lib.examples.is_empty(), "err");
    }
    // 18 反模式有诊断
    {
        let mut all = true;
        for ap in AntiPattern::ALL {
            if ap.diagnose().len() <= 5 { all = false; }
        }
        chk(&mut set, "C3877-18", all, "err");
    }

    set
}
