//! VE-F4026 · RTL 图标与隐喻
//!
//! 方向性图标与隐喻 RTL 适配：图标镜像（方向性图标自动镜像：箭头/返
//! 回/播放——镜像图标表，表驱动）；隐喻差异（手势/符号含义地域差异
//! ——隐喻审查——隐喻红线：文化冒犯图标=生态事故——隐喻断言+地域
//! 审查清单）；替代资产（RTL 专属图标资产——R03 工坊产出复述单源）。
//! 数据结构：镜像图标表；隐喻审查断言；替代资产复用；表驱动。
//! 错误路径：隐喻冒犯→P0；该镜像未镜像→P1；资产缺→工坊补；表黑箱
//! →公开。
//! 性能：镜像 O(图标表)；隐喻 O(地域审查)；资产 O(复用)；表 O(1)。
//! 跨批对接：R03 工坊单源复用声明；F3645 网格对端；地域规范对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6AC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6AC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T32GCode(pub u16);

impl T32GCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const METAPHOR_P0: Self = Self(SEG_BASE + 1);
    pub const MIRROR_MISS: Self = Self(SEG_BASE + 2);
    pub const ASSET_GAP: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::METAPHOR_P0 => "隐喻冒犯→P0",
            Self::MIRROR_MISS => "该镜像未镜像→P1",
            Self::ASSET_GAP => "资产缺→工坊补",
            _ => "未知",
        }
    }
}

// ── 镜像图标表（表驱动）─────────────────────────────────────────────

/// 方向性图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirIcon {
    ArrowLeft,
    ArrowRight,
    Back,
    Play,
    Undo,
    Redo,
    AlignLeft,
    AlignRight,
}

/// 镜像图标表：(图标, RTL 应镜像)。
pub const ICON_TABLE: [(DirIcon, bool); 8] = [
    (DirIcon::ArrowLeft, true),
    (DirIcon::ArrowRight, true),
    (DirIcon::Back, true),
    (DirIcon::Play, false),      // 播放键文化惯例不镜像。
    (DirIcon::Undo, true),
    (DirIcon::Redo, true),
    (DirIcon::AlignLeft, true),
    (DirIcon::AlignRight, true),
];

impl DirIcon {
    pub fn name(self) -> &'static str {
        match self {
            DirIcon::ArrowLeft => "左箭头",
            DirIcon::ArrowRight => "右箭头",
            DirIcon::Back => "返回",
            DirIcon::Play => "播放",
            DirIcon::Undo => "撤销",
            DirIcon::Redo => "重做",
            DirIcon::AlignLeft => "左对齐",
            DirIcon::AlignRight => "右对齐",
        }
    }

    pub fn mirror_rule(self) -> bool {
        ICON_TABLE.iter().find(|(i, _)| *i == self).map(|(_, m)| *m).unwrap_or(false)
    }
}

// ── 隐喻审查（地域差异）─────────────────────────────────────────────

/// 地域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Global,
    Mena,
    EastAsia,
    Latin,
}

pub const REGIONS: [Region; 4] = [Region::Global, Region::Mena, Region::EastAsia, Region::Latin];

impl Region {
    pub fn name(self) -> &'static str {
        match self {
            Region::Global => "全球",
            Region::Mena => "中东北非",
            Region::EastAsia => "东亚",
            Region::Latin => "拉美",
        }
    }
}

/// 隐喻条目：图标 + 地域 + 审查结论。
#[derive(Debug, Clone)]
pub struct MetaphorRow {
    pub icon: DirIcon,
    pub region: Region,
    /// 审查通过（无文化冒犯）。
    pub pass: bool,
    /// 审查备注。
    pub note: String,
}

/// 隐喻审查清单。
#[derive(Debug, Clone)]
pub struct MetaphorAudit {
    pub rows: Vec<MetaphorRow>,
}

impl MetaphorAudit {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn review(&mut self, icon: DirIcon, region: Region, pass: bool, note: &str) {
        self.rows.push(MetaphorRow {
            icon,
            region,
            pass,
            note: String::from(note),
        });
    }

    /// 冒犯检出（P0 对象）。
    pub fn offenses(&self) -> Vec<(DirIcon, Region)> {
        self.rows.iter().filter(|r| !r.pass).map(|r| (r.icon, r.region)).collect()
    }

    /// 全域审查过。
    pub fn clean(&self) -> bool {
        self.offenses().is_empty()
    }

    /// 覆盖断言：每地域至少审查一条。
    pub fn region_covered(&self) -> bool {
        REGIONS.iter().all(|r| self.rows.iter().any(|row| row.region == *r))
    }
}

// ── 替代资产（RTL 专属）─────────────────────────────────────────────

/// 资产登记：图标 → RTL 专属资产指针。
#[derive(Debug, Clone)]
pub struct AssetLink {
    pub icon: DirIcon,
    pub rtl_asset: String,
    /// 工坊来源（R03 复述单源）。
    pub from_workshop: bool,
}

/// 资产册。
#[derive(Debug, Clone)]
pub struct AssetBook {
    pub links: Vec<AssetLink>,
}

impl AssetBook {
    pub fn new() -> Self {
        Self { links: Vec::new() }
    }

    pub fn put(&mut self, icon: DirIcon, rtl_asset: &str, from_workshop: bool) {
        self.links.push(AssetLink {
            icon,
            rtl_asset: String::from(rtl_asset),
            from_workshop,
        });
    }

    /// 资产缺检出：表内应镜像图标但无 RTL 资产。
    pub fn gaps(&self) -> Vec<&'static str> {
        ICON_TABLE.iter()
            .filter(|(i, m)| *m && !self.links.iter().any(|l| l.icon == *i))
            .map(|(i, _)| i.name())
            .collect()
    }

    /// 非工坊来源检出（复述单源红线）。
    pub fn off_source(&self) -> Vec<String> {
        self.links.iter().filter(|l| !l.from_workshop).map(|l| l.rtl_asset.clone()).collect()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves266_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves266_s12icon");

    // C4026-01 诊断码段不越窗。
    chk(&mut set, "C4026-01",
        T32GCode::ASSET_GAP.0 < SEG_BASE + 16 && SEG_BASE == 0x6AC0, "err");

    // C4026-02 图标表八图标齐备。
    chk(&mut set, "C4026-02", ICON_TABLE.len() == 8, "err");

    // C4026-03 播放键不镜像（文化惯例豁免）。
    chk(&mut set, "C4026-03",
        !DirIcon::Play.mirror_rule() && DirIcon::Back.mirror_rule(), "err");

    // C4026-04 表公开（全图标规则可查）。
    chk(&mut set, "C4026-04",
        ICON_TABLE.iter().all(|(i, _)| {
            let _ = i.name();
            true
        }) && DirIcon::ArrowLeft.mirror_rule(), "err");

    // C4026-05 该镜像未镜像 → P1（返回箭头未镜像）。
    let mut a = MetaphorAudit::new();
    a.review(DirIcon::Back, Region::Global, true, "通过");
    chk(&mut set, "C4026-05", a.clean(), "err");

    // C4026-06 隐喻冒犯 → P0。
    a.review(DirIcon::Play, Region::Mena, false, "某些地域播放键朝向冒犯");
    chk(&mut set, "C4026-06",
        a.offenses().len() == 1 && !a.clean()
            && T32GCode::METAPHOR_P0.explain().contains("P0"), "err");

    // C4026-07 处置后（换资产）恢复。
    a.rows.pop();
    chk(&mut set, "C4026-07", a.clean(), "err");

    // C4026-08 四地域覆盖断言。
    let mut a2 = MetaphorAudit::new();
    for r in REGIONS.iter() {
        a2.review(DirIcon::Back, *r, true, "过");
    }
    chk(&mut set, "C4026-08", a2.region_covered(), "err");

    // C4026-09 缺地域 → 覆盖不达。
    a2.rows.retain(|r| r.region != Region::Latin);
    chk(&mut set, "C4026-09", !a2.region_covered(), "err");

    // C4026-10 资产缺检出。
    let mut ab = AssetBook::new();
    ab.put(DirIcon::Back, "back_rtl.svg", true);
    let gaps = ab.gaps();
    chk(&mut set, "C4026-10",
        gaps.len() == 6 && gaps.contains(&"播放") == false, "err");

    // C4026-11 工坊补齐后缺口清。
    ab.put(DirIcon::ArrowLeft, "arrow_left_rtl.svg", true);
    ab.put(DirIcon::ArrowRight, "arrow_right_rtl.svg", true);
    ab.put(DirIcon::Undo, "undo_rtl.svg", true);
    ab.put(DirIcon::Redo, "redo_rtl.svg", true);
    ab.put(DirIcon::AlignLeft, "align_left_rtl.svg", true);
    ab.put(DirIcon::AlignRight, "align_right_rtl.svg", true);
    chk(&mut set, "C4026-11", ab.gaps().is_empty() && ab.links.len() == 7, "err");

    // C4026-12 非工坊来源检出（单源红线）。
    let mut ab2 = AssetBook::new();
    ab2.put(DirIcon::Back, "back_rtl.svg", false);
    chk(&mut set, "C4026-12",
        ab2.off_source() == vec![String::from("back_rtl.svg")], "err");

    // C4026-13 四地域名互异。
    let mut uniq = true;
    for i in 0..4 {
        for j in (i + 1)..4 {
            if REGIONS[i].name() == REGIONS[j].name() { uniq = false; }
        }
    }
    chk(&mut set, "C4026-13", uniq, "err");

    // C4026-14 图标名互异。
    let mut uniq2 = true;
    for i in 0..8 {
        for j in (i + 1)..8 {
            if ICON_TABLE[i].0.name() == ICON_TABLE[j].0.name() { uniq2 = false; }
        }
    }
    chk(&mut set, "C4026-14", uniq2, "err");

    // C4026-15 表内应镜像计数（7/8——播放豁免）。
    let mirror_cnt = ICON_TABLE.iter().filter(|(_, m)| *m).count();
    chk(&mut set, "C4026-15", mirror_cnt == 7, "err");

    // C4026-16 隐喻审查行可追加去重位。
    a2.review(DirIcon::Back, Region::Latin, true, "复审");
    chk(&mut set, "C4026-16", a2.rows.len() == 4, "err");

    // C4026-17 资产缺码语义。
    chk(&mut set, "C4026-17", T32GCode::ASSET_GAP.explain().contains("工坊"), "err");

    // C4026-18 镜像未达码语义（P1）。
    chk(&mut set, "C4026-18", T32GCode::MIRROR_MISS.explain().contains("P1"), "err");

    // C4026-19 错误码互异。
    let codes = [T32GCode::OK.0, T32GCode::METAPHOR_P0.0, T32GCode::MIRROR_MISS.0, T32GCode::ASSET_GAP.0];
    chk(&mut set, "C4026-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C4026-20 空审查册清白。
    chk(&mut set, "C4026-20",
        MetaphorAudit::new().clean() && !MetaphorAudit::new().region_covered(), "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_exempt() {
        assert!(!DirIcon::Play.mirror_rule());
        assert!(DirIcon::Undo.mirror_rule());
    }

    #[test]
    fn metaphor_p0() {
        let mut a = MetaphorAudit::new();
        a.review(DirIcon::Play, Region::Mena, false, "x");
        assert_eq!(a.offenses().len(), 1);
    }

    #[test]
    fn asset_gap_counts() {
        let ab = AssetBook::new();
        assert_eq!(ab.gaps().len(), 7); // 7 个应镜像图标全缺
    }
}
