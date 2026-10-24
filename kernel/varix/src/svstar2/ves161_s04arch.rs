//! VE-F3861 S04 开工与动效无障碍总纲（SEG 0xCCxx）。
//!
//! 分工声明（S01 建管线、S04 装内容）；复制红线（复用组不得复制前组实现）；
//! 指针红线（每个复用项必须有引用指针）；十项映射。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// 诊断码（SEG 0xCCxx）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CCCode {
    /// 复制红线
    CopyRedline = 0xCC00,
    /// 指针缺失
    PointerMiss = 0xCC01,
    /// 映射缺失
    MapMiss = 0xCC02,
    /// 分工越界
    DutyOver = 0xCC03,
    /// 复用项冲突
    ReuseConflict = 0xCC04,
    /// 项索引越界
    ItemOver = 0xCC05,
    /// 声明缺失
    DeclMiss = 0xCC06,
    /// 重叠超限
    OverlapOver = 0xCC07,
    /// 引用悬空
    RefDangling = 0xCC08,
    /// 十项不齐
    TenIncomplete = 0xCC09,
    /// 来源伪造
    SourceFake = 0xCC0A,
    /// 审计失败
    AuditFail = 0xCC0B,
}

impl CCCode {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn explain(self) -> &'static str {
        match self {
            CCCode::CopyRedline => "复制红线",
            CCCode::PointerMiss => "复用指针缺失",
            CCCode::MapMiss => "映射缺失",
            CCCode::DutyOver => "分工越界",
            CCCode::ReuseConflict => "复用项冲突",
            CCCode::ItemOver => "项索引越界",
            CCCode::DeclMiss => "声明缺失",
            CCCode::OverlapOver => "重叠率超限",
            CCCode::RefDangling => "引用悬空",
            CCCode::TenIncomplete => "十项映射不齐",
            CCCode::SourceFake => "来源伪造",
            CCCode::AuditFail => "审计失败",
        }
    }
}

/// S04 十项映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum S04Item {
    /// 动效替代引擎
    MotionAlt,
    /// 视频替代
    VideoAlt,
    /// 音频替代
    AudioAlt,
    /// 实时媒体
    LiveMedia,
    /// 时长个性化
    DurationPref,
    /// 前庭友好
    Vestibular,
    /// 光敏安全
    Photosafe,
    /// 媒体控制
    MediaCtrl,
    /// API 冻结
    ApiFreeze,
    /// 组收口
    GroupClose,
}

impl S04Item {
    pub const ALL: [S04Item; 10] = [
        S04Item::MotionAlt,
        S04Item::VideoAlt,
        S04Item::AudioAlt,
        S04Item::LiveMedia,
        S04Item::DurationPref,
        S04Item::Vestibular,
        S04Item::Photosafe,
        S04Item::MediaCtrl,
        S04Item::ApiFreeze,
        S04Item::GroupClose,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            S04Item::MotionAlt => "动效替代",
            S04Item::VideoAlt => "视频替代",
            S04Item::AudioAlt => "音频替代",
            S04Item::LiveMedia => "实时媒体",
            S04Item::DurationPref => "时长个性化",
            S04Item::Vestibular => "前庭",
            S04Item::Photosafe => "光敏",
            S04Item::MediaCtrl => "媒体控制",
            S04Item::ApiFreeze => "API",
            S04Item::GroupClose => "收口",
        }
    }
}

/// 复用来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReuseSource {
    /// P08 五段
    P08,
    /// S01 管线
    S01,
    /// F3806 替代渲染
    F3806,
    /// F3145 渲染清单
    F3145,
}

impl ReuseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ReuseSource::P08 => "P08",
            ReuseSource::S01 => "S01",
            ReuseSource::F3806 => "F3806",
            ReuseSource::F3145 => "F3145",
        }
    }
}

/// 复用条目（指针红线载体）。
#[derive(Clone, Debug)]
pub struct ReuseEntry {
    pub item: S04Item,
    pub source: ReuseSource,
    /// 引用指针（单源地址/编号）
    pub pointer: u32,
    /// 本组是否复制了实现（必须 false）
    pub copied_impl: bool,
    /// 与既有实现重叠率（0-100）
    pub overlap_pct: u8,
}

/// S04 总纲。
pub struct S04Arch {
    entries: Vec<ReuseEntry>,
    /// 十项映射是否齐
    mapped: [bool; 10],
}

impl S04Arch {
    pub fn new() -> Self {
        S04Arch {
            entries: Vec::new(),
            mapped: [false; 10],
        }
    }

    fn item_index(item: S04Item) -> usize {
        S04Item::ALL.iter().position(|x| *x == item).unwrap_or(10)
    }

    /// 登记复用项。复制实现或缺指针即拒。
    pub fn register(&mut self, e: ReuseEntry) -> Result<(), CCCode> {
        if e.copied_impl {
            return Err(CCCode::CopyRedline);
        }
        if e.pointer == 0 {
            return Err(CCCode::PointerMiss);
        }
        if e.overlap_pct > 30 {
            return Err(CCCode::OverlapOver);
        }
        if self.entries.iter().any(|x| x.item == e.item) {
            return Err(CCCode::ReuseConflict);
        }
        let i = Self::item_index(e.item);
        if i >= 10 {
            return Err(CCCode::ItemOver);
        }
        self.mapped[i] = true;
        self.entries.push(e);
        Ok(())
    }

    pub fn mapped_count(&self) -> usize {
        self.mapped.iter().filter(|x| **x).count()
    }

    pub fn all_mapped(&self) -> bool {
        self.mapped_count() == 10
    }

    pub fn pointer_missing(&self) -> usize {
        self.entries.iter().filter(|x| x.pointer == 0).count()
    }

    pub fn copy_violation(&self) -> usize {
        self.entries.iter().filter(|x| x.copied_impl).count()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 审计：十项齐 + 无复制 + 指针全。
    pub fn audit(&self) -> Result<(), CCCode> {
        if !self.all_mapped() {
            return Err(CCCode::TenIncomplete);
        }
        if self.copy_violation() > 0 {
            return Err(CCCode::CopyRedline);
        }
        if self.pointer_missing() > 0 {
            return Err(CCCode::PointerMiss);
        }
        Ok(())
    }
}

pub fn run_ves161_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves161_s04arch");
    {
        let a = S04Arch::new();
        chk(&mut set, "C3861-01", a.mapped_count() == 0, "初始零映射");
    }
    {
        let mut a = S04Arch::new();
        let r = a.register(ReuseEntry {
            item: S04Item::MotionAlt,
            source: ReuseSource::F3806,
            pointer: 0,
            copied_impl: false,
            overlap_pct: 5,
        });
        chk(
            &mut set,
            "C3861-02",
            r == Err(CCCode::PointerMiss),
            "缺指针拒登记",
        );
    }
    {
        let mut a = S04Arch::new();
        let r = a.register(ReuseEntry {
            item: S04Item::VideoAlt,
            source: ReuseSource::P08,
            pointer: 100,
            copied_impl: true,
            overlap_pct: 5,
        });
        chk(
            &mut set,
            "C3861-03",
            r == Err(CCCode::CopyRedline),
            "复制红线",
        );
    }
    {
        let mut a = S04Arch::new();
        let r = a.register(ReuseEntry {
            item: S04Item::AudioAlt,
            source: ReuseSource::S01,
            pointer: 200,
            copied_impl: false,
            overlap_pct: 50,
        });
        chk(
            &mut set,
            "C3861-04",
            r == Err(CCCode::OverlapOver),
            "重叠超限拒",
        );
    }
    {
        let mut a = S04Arch::new();
        let r = a.register(ReuseEntry {
            item: S04Item::LiveMedia,
            source: ReuseSource::F3145,
            pointer: 300,
            copied_impl: false,
            overlap_pct: 10,
        });
        chk(&mut set, "C3861-05", r.is_ok(), "合法登记成功");
    }
    {
        let mut a = S04Arch::new();
        let _ = a.register(ReuseEntry {
            item: S04Item::MotionAlt,
            source: ReuseSource::P08,
            pointer: 1,
            copied_impl: false,
            overlap_pct: 0,
        });
        let r = a.register(ReuseEntry {
            item: S04Item::MotionAlt,
            source: ReuseSource::S01,
            pointer: 2,
            copied_impl: false,
            overlap_pct: 0,
        });
        chk(
            &mut set,
            "C3861-06",
            r == Err(CCCode::ReuseConflict),
            "重复项冲突",
        );
    }
    {
        let mut a = S04Arch::new();
        for (i, item) in S04Item::ALL.iter().enumerate() {
            let _ = a.register(ReuseEntry {
                item: *item,
                source: ReuseSource::P08,
                pointer: (i as u32) + 1,
                copied_impl: false,
                overlap_pct: 0,
            });
        }
        chk(&mut set, "C3861-07", a.all_mapped(), "十项全齐");
    }
    {
        let mut a = S04Arch::new();
        for (i, item) in S04Item::ALL.iter().enumerate() {
            let _ = a.register(ReuseEntry {
                item: *item,
                source: ReuseSource::S01,
                pointer: (i as u32) + 10,
                copied_impl: false,
                overlap_pct: 3,
            });
        }
        chk(&mut set, "C3861-08", a.audit().is_ok(), "审计通过");
    }
    {
        let mut a = S04Arch::new();
        let e = a.register(ReuseEntry {
            item: S04Item::DurationPref,
            source: ReuseSource::F3806,
            pointer: 55,
            copied_impl: false,
            overlap_pct: 1,
        });
        chk(&mut set, "C3861-09", e.is_ok() && a.entry_count() == 1, "条目计数");
    }
    {
        let mut a = S04Arch::new();
        let r = a.audit();
        chk(
            &mut set,
            "C3861-10",
            r == Err(CCCode::TenIncomplete),
            "不齐不可审计过",
        );
    }
    {
        let mut a = S04Arch::new();
        let _ = a.register(ReuseEntry {
            item: S04Item::Vestibular,
            source: ReuseSource::P08,
            pointer: 9,
            copied_impl: false,
            overlap_pct: 0,
        });
        chk(&mut set, "C3861-11", a.mapped_count() == 1, "映射计数");
    }
    {
        let a = S04Arch::new();
        chk(&mut set, "C3861-12", a.copy_violation() == 0, "零复制");
    }
    set
}
