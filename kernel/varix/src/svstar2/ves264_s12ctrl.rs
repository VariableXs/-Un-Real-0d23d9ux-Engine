//! VE-F4024 · RTL 控件镜像
//!
//! 控件级 RTL 镜像：方向性控件（进度条/滑块/步骤条/返回箭头）镜像规
//! 则表——控件镜像表（表驱动）；方向性语义（时间轴/进度语义方向——
//! 时间方向不减红线：进度条 RTL 不反转时间语义——语义方向断言）；
//! 图标方向（返回/前进镜像——F4026 联动前向）；控件覆盖扫描。
//! 数据结构：控件镜像表；语义方向断言；图标联动；覆盖断言。
//! 错误路径：语义反转检出→P1；控件漏镜像→P1；表黑箱→公开；图标
//! 失联→联动。
//! 性能：镜像 O(控件)；语义 O(断言)；图标 O(联动)；覆盖 O(扫描)。
//! 跨批对接：N 域控件对端；F4022 布局单源复用；F4026 前向。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x69C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x69C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T30KCode(pub u16);

impl T30KCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const SEMANTIC_FLIP: Self = Self(SEG_BASE + 1);
    pub const CTRL_MISS: Self = Self(SEG_BASE + 2);
    pub const ICON_BROKEN: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SEMANTIC_FLIP => "语义反转→P1",
            Self::CTRL_MISS => "控件漏镜像→P1",
            Self::ICON_BROKEN => "图标失联→联动",
            _ => "未知",
        }
    }
}

// ── 控件镜像表（表驱动）─────────────────────────────────────────────

/// 方向性控件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirControl {
    /// 进度条（时间语义不减）。
    Progress,
    /// 滑块。
    Slider,
    /// 步骤条。
    Stepper,
    /// 返回箭头。
    BackArrow,
    /// 前进箭头。
    FwdArrow,
    /// 时间轴。
    Timeline,
}

pub const CONTROL_TABLE: [(DirControl, bool); 6] = [
    // (控件, RTL 是否镜像)。
    (DirControl::Progress, true),
    (DirControl::Slider, true),
    (DirControl::Stepper, true),
    (DirControl::BackArrow, true),
    (DirControl::FwdArrow, true),
    (DirControl::Timeline, true),
];

impl DirControl {
    pub fn name(self) -> &'static str {
        match self {
            DirControl::Progress => "进度条",
            DirControl::Slider => "滑块",
            DirControl::Stepper => "步骤条",
            DirControl::BackArrow => "返回箭头",
            DirControl::FwdArrow => "前进箭头",
            DirControl::Timeline => "时间轴",
        }
    }

    /// 表查镜像规则（公开）。
    pub fn mirror_rule(self) -> bool {
        CONTROL_TABLE.iter().find(|(c, _)| *c == self).map(|(_, m)| *m).unwrap_or(false)
    }
}

// ── 语义方向断言（时间方向不减）─────────────────────────────────────

/// 进度语义：视觉从右向左填充（RTL），但语义时间序不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressSemantics {
    /// 视觉起点侧（true = 右起）。
    pub visual_right_start: bool,
    /// 语义时间序保持（值增 = 时间前进）。
    pub time_monotonic: bool,
}

/// 语义方向断言：视觉可镜像，语义序不可反转。
pub fn semantic_assert(sem: ProgressSemantics) -> T30KCode {
    if sem.time_monotonic {
        T30KCode::OK
    } else {
        T30KCode::SEMANTIC_FLIP
    }
}

// ── 图标联动 ────────────────────────────────────────────────────────

/// 图标方向对：控件 ↔ 图标镜像联动。
#[derive(Debug, Clone)]
pub struct IconLink {
    /// 控件。
    pub control: DirControl,
    /// 控件已镜像。
    pub control_mirrored: bool,
    /// 图标已镜像。
    pub icon_mirrored: bool,
}

/// 图标联动断言：镜像态必须同步。
pub fn icon_link_assert(links: &[IconLink]) -> T30KCode {
    if links.iter().any(|l| l.control_mirrored != l.icon_mirrored) {
        T30KCode::ICON_BROKEN
    } else {
        T30KCode::OK
    }
}

// ── 控件覆盖扫描 ────────────────────────────────────────────────────

/// 控件实例。
#[derive(Debug, Clone)]
pub struct ControlInstance {
    pub kind: DirControl,
    pub name: String,
    /// RTL 态已镜像。
    pub mirrored: bool,
}

/// 覆盖扫描器。
#[derive(Debug, Clone)]
pub struct ControlScanner {
    pub items: Vec<ControlInstance>,
}

impl ControlScanner {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn put(&mut self, kind: DirControl, name: &str, mirrored: bool) {
        self.items.push(ControlInstance {
            kind,
            name: String::from(name),
            mirrored,
        });
    }

    /// 漏镜像检出（表要求镜像但未镜像）。
    pub fn missing(&self) -> Vec<String> {
        self.items.iter()
            .filter(|i| i.kind.mirror_rule() && !i.mirrored)
            .map(|i| i.name.clone())
            .collect()
    }

    pub fn coverage_ok(&self) -> bool {
        self.missing().is_empty()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves264_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves264_s12ctrl");

    // C4024-01 诊断码段不越窗。
    chk(&mut set, "C4024-01",
        T30KCode::ICON_BROKEN.0 < SEG_BASE + 16 && SEG_BASE == 0x69C0, "err");

    // C4024-02 控件表六控件齐备。
    chk(&mut set, "C4024-02",
        CONTROL_TABLE.len() == 6
            && CONTROL_TABLE.iter().all(|(_, m)| *m), "err");

    // C4024-03 表公开（全部控件规则可查）。
    chk(&mut set, "C4024-03",
        DirControl::Progress.mirror_rule()
            && DirControl::BackArrow.mirror_rule()
            && DirControl::Timeline.mirror_rule(), "err");

    // C4024-04 语义方向断言：视觉镜像 + 时间不减 → 过。
    let sem = ProgressSemantics { visual_right_start: true, time_monotonic: true };
    chk(&mut set, "C4024-04", semantic_assert(sem) == T30KCode::OK, "err");

    // C4024-05 语义反转 → P1（时间方向减）。
    let sem2 = ProgressSemantics { visual_right_start: true, time_monotonic: false };
    chk(&mut set, "C4024-05", semantic_assert(sem2) == T30KCode::SEMANTIC_FLIP, "err");

    // C4024-06 LTR 语义基线：右不起 + 单调 → 过。
    let sem3 = ProgressSemantics { visual_right_start: false, time_monotonic: true };
    chk(&mut set, "C4024-06", semantic_assert(sem3) == T30KCode::OK, "err");

    // C4024-07 图标联动：同步镜像过。
    let links = [
        IconLink { control: DirControl::BackArrow, control_mirrored: true, icon_mirrored: true },
        IconLink { control: DirControl::FwdArrow, control_mirrored: false, icon_mirrored: false },
    ];
    chk(&mut set, "C4024-07", icon_link_assert(&links) == T30KCode::OK, "err");

    // C4024-08 图标失联：控件镜像图标未镜像。
    let links2 = [
        IconLink { control: DirControl::BackArrow, control_mirrored: true, icon_mirrored: false },
    ];
    chk(&mut set, "C4024-08", icon_link_assert(&links2) == T30KCode::ICON_BROKEN, "err");

    // C4024-09 覆盖扫描：全镜像过。
    let mut s = ControlScanner::new();
    for c in CONTROL_TABLE.iter() {
        s.put(c.0, c.0.name(), true);
    }
    chk(&mut set, "C4024-09", s.coverage_ok() && s.items.len() == 6, "err");

    // C4024-10 漏镜像检出 → P1。
    let mut s2 = ControlScanner::new();
    s2.put(DirControl::Slider, "音量滑块", false);
    s2.put(DirControl::Progress, "下载进度", true);
    chk(&mut set, "C4024-10",
        s2.missing() == vec![String::from("音量滑块")] && !s2.coverage_ok(), "err");

    // C4024-11 补镜像后过。
    let mut s3 = ControlScanner::new();
    s3.put(DirControl::Slider, "音量滑块", true);
    chk(&mut set, "C4024-11", s3.coverage_ok(), "err");

    // C4024-12 空扫描过（零常态）。
    chk(&mut set, "C4024-12", ControlScanner::new().coverage_ok(), "err");

    // C4024-13 控件名互异。
    let mut uniq = true;
    for i in 0..6 {
        for j in (i + 1)..6 {
            if CONTROL_TABLE[i].0.name() == CONTROL_TABLE[j].0.name() { uniq = false; }
        }
    }
    chk(&mut set, "C4024-13", uniq, "err");

    // C4024-14 枚举双射（六控件互异）。
    let kinds = [DirControl::Progress, DirControl::Slider, DirControl::Stepper,
        DirControl::BackArrow, DirControl::FwdArrow, DirControl::Timeline];
    let mut uniq2 = true;
    for i in 0..6 {
        for j in (i + 1)..6 {
            if kinds[i] == kinds[j] { uniq2 = false; }
        }
    }
    chk(&mut set, "C4024-14", uniq2, "err");

    // C4024-15 语义断言与镜像规则解耦（镜像不改语义断言输入）。
    let sem4 = ProgressSemantics { visual_right_start: false, time_monotonic: true };
    chk(&mut set, "C4024-15",
        DirControl::Progress.mirror_rule() && semantic_assert(sem4) == T30KCode::OK, "err");

    // C4024-16 步骤条在表内。
    chk(&mut set, "C4024-16",
        CONTROL_TABLE.iter().any(|(c, _)| *c == DirControl::Stepper), "err");

    // C4024-17 时间轴语义保护（表内 + 断言组合）。
    chk(&mut set, "C4024-17",
        DirControl::Timeline.mirror_rule()
            && semantic_assert(ProgressSemantics { visual_right_start: true, time_monotonic: true })
                == T30KCode::OK, "err");

    // C4024-18 多实例扫描归并。
    let mut s4 = ControlScanner::new();
    s4.put(DirControl::Progress, "p1", true);
    s4.put(DirControl::Progress, "p2", false);
    chk(&mut set, "C4024-18", s4.missing() == vec![String::from("p2")], "err");

    // C4024-19 失联码语义。
    chk(&mut set, "C4024-19", T30KCode::ICON_BROKEN.explain().contains("联动"), "err");

    // C4024-20 错误码互异。
    let codes = [T30KCode::OK.0, T30KCode::SEMANTIC_FLIP.0, T30KCode::CTRL_MISS.0, T30KCode::ICON_BROKEN.0];
    chk(&mut set, "C4024-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_monotonic_gate() {
        assert_eq!(
            semantic_assert(ProgressSemantics { visual_right_start: true, time_monotonic: true }),
            T30KCode::OK
        );
        assert_eq!(
            semantic_assert(ProgressSemantics { visual_right_start: true, time_monotonic: false }),
            T30KCode::SEMANTIC_FLIP
        );
    }

    #[test]
    fn icon_sync() {
        let links = [IconLink { control: DirControl::BackArrow, control_mirrored: true, icon_mirrored: false }];
        assert_eq!(icon_link_assert(&links), T30KCode::ICON_BROKEN);
    }

    #[test]
    fn table_public() {
        assert!(DirControl::Slider.mirror_rule());
    }
}
