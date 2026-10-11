//! CGPU-F1726 · 异构路线图
//!
//! 判据锚点（施工书原文摘录）：
//! 路线图：二期（异构二期（更多引擎（FPGA 预留/在线学习/自动切分/分布式——路线图
//! （三要素复用；预留位（枚举开放位复用声明——开放复用；标准（行业标准跟踪
//! （SYCL/ONNX 演进——标准跟踪；衔接（并入总路线——衔接复用；测试（路线图/预留/
//! 标准/衔接四组）。判据：路线图、开放位、标准、衔接、四组、判据。
//!
//! 诊断码段 0xA901~0xA905（独占）：
//!   0xA901 路线图缺项（二期四向任一缺席）
//!   0xA902 预留位失范（开放位声明缺/已占位谎称开放）
//!   0xA903 标准跟踪断（SYCL/ONNX 跟踪缺一）
//!   0xA904 衔接断裂（未挂总路线锚）
//!   0xA905 段越界（判码落 0xA900..0xB000 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const ROADMAP_VERSION: &str = "K45-roadmap-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA901~0xA905）
// ---------------------------------------------------------------------------

pub const XC_RM_ITEM_MISSING: u32 = 0xA901;
pub const XC_RM_OPEN_BAD: u32 = 0xA902;
pub const XC_RM_STD_GAP: u32 = 0xA903;
pub const XC_RM_LINK_BROKEN: u32 = 0xA904;
pub const XC_RM_SEG_OOR: u32 = 0xA905;

// ---------------------------------------------------------------------------
// 一、二期路线图（四向闭集）
// ---------------------------------------------------------------------------

/// 二期方向闭集（判据点名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase2Track {
    /// FPGA 引擎预留（不实装，只留位）。
    Fpga,
    /// 在线学习（持续适配工作负载）。
    OnlineLearn,
    /// 自动切分（负载自动划分引擎）。
    AutoSplit,
    /// 分布式（多机协同）。
    Distributed,
}

impl Phase2Track {
    pub const ALL: [Phase2Track; 4] = [
        Phase2Track::Fpga,
        Phase2Track::OnlineLearn,
        Phase2Track::AutoSplit,
        Phase2Track::Distributed,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Phase2Track::Fpga => "fpga",
            Phase2Track::OnlineLearn => "online_learn",
            Phase2Track::AutoSplit => "auto_split",
            Phase2Track::Distributed => "distributed",
        }
    }

    /// 三要素：方向 / 现状声明 / 衔接锚（复用惯例）。
    pub const fn status(self) -> &'static str {
        match self {
            Phase2Track::Fpga => "预留位声明（不实装）",
            Phase2Track::OnlineLearn => "预研（离线优先已定）",
            Phase2Track::AutoSplit => "预研（手动切分已绿）",
            Phase2Track::Distributed => "预研（单机已绿）",
        }
    }

    /// 每向挂既有源单（不新造）。
    pub const fn origin(self) -> &'static str {
        match self {
            Phase2Track::Fpga => "CGPU-F1604",
            Phase2Track::OnlineLearn => "CGPU-F1606",
            Phase2Track::AutoSplit => "CGPU-F1606",
            Phase2Track::Distributed => "CGPU-F1608",
        }
    }
}

/// 路线图总账：四向全在册且状态/源单非空。
pub struct RoadmapLedger;

impl RoadmapLedger {
    pub fn complete() -> Result<(), u32> {
        for t in Phase2Track::ALL {
            if t.status().is_empty() || t.origin().is_empty() {
                return Err(XC_RM_ITEM_MISSING);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、预留位（枚举开放位复用声明）
// ---------------------------------------------------------------------------

/// 开放位声明：引擎枚举的扩展槽（开放=未占；占位=已挂单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenSlot {
    pub slot_id: u16,
    /// 槽名（预留语义显性）。
    pub slot_name: &'static str,
    /// 开放标志（开放位必须真的未占——谎称开放即失范）。
    pub open: bool,
    /// 已占时挂的方向（开放时为空）。
    pub occupied_by: &'static str,
}

impl OpenSlot {
    /// 一致性判：开放则 occupied_by 必空；占位则必非空。
    pub fn sound(&self) -> Result<(), u32> {
        if self.slot_name.is_empty() {
            return Err(XC_RM_OPEN_BAD);
        }
        if self.open && !self.occupied_by.is_empty() {
            return Err(XC_RM_OPEN_BAD);
        }
        if !self.open && self.occupied_by.is_empty() {
            return Err(XC_RM_OPEN_BAD);
        }
        Ok(())
    }
}

/// 在册开放位表：FPGA 槽开放 + 三向占位（显性）。
pub const OPEN_SLOTS: [OpenSlot; 4] = [
    OpenSlot { slot_id: 1, slot_name: "engine_slot_a", open: true, occupied_by: "" },
    OpenSlot { slot_id: 2, slot_name: "engine_slot_b", open: true, occupied_by: "" },
    OpenSlot { slot_id: 3, slot_name: "fpga_track", open: false, occupied_by: "fpga" },
    OpenSlot { slot_id: 4, slot_name: "dist_track", open: false, occupied_by: "distributed" },
];

/// 开放位审计：全部一致 + 至少 1 个真开放。
pub fn open_slots_audit() -> Result<(), u32> {
    let mut any_open = false;
    for s in OPEN_SLOTS.iter() {
        s.sound()?;
        if s.open {
            any_open = true;
        }
    }
    if !any_open {
        return Err(XC_RM_OPEN_BAD);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、标准跟踪（SYCL/ONNX 演进）
// ---------------------------------------------------------------------------

/// 标准跟踪表：跟踪项 / 当前版本锚 / 复评周期（天）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StdTrack {
    pub std_name: &'static str,
    /// 当前锚定版本（演进基线）。
    pub ver_anchor: &'static str,
    /// 复评周期（天，>0）。
    pub review_days: u32,
}

impl StdTrack {
    pub fn sound(&self) -> Result<(), u32> {
        if self.std_name.is_empty() || self.ver_anchor.is_empty() || self.review_days == 0 {
            return Err(XC_RM_STD_GAP);
        }
        Ok(())
    }
}

/// 双标准在册（SYCL/ONNX——判据点名）。
pub const STD_TRACKS: [StdTrack; 2] = [
    StdTrack { std_name: "SYCL", ver_anchor: "2020rev3", review_days: 90 },
    StdTrack { std_name: "ONNX", ver_anchor: "opset-19", review_days: 60 },
];

pub fn std_tracks_audit() -> Result<(), u32> {
    for t in STD_TRACKS.iter() {
        t.sound()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、衔接（并入总路线）
// ---------------------------------------------------------------------------

/// 总路线锚：K 域路线图挂总施工书批次链。
pub struct LinkAnchor {
    /// 总路线锚标识。
    pub anchor: &'static str,
    /// 本域批次区间（K01..=K09——收口前全部在链）。
    pub k_span: [u16; 2],
}

impl LinkAnchor {
    pub fn linked(&self) -> Result<(), u32> {
        if self.anchor.is_empty() {
            return Err(XC_RM_LINK_BROKEN);
        }
        if self.k_span[0] == 0 || self.k_span[0] > self.k_span[1] {
            return Err(XC_RM_LINK_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：路线图/预留/标准/衔接）
// ---------------------------------------------------------------------------

/// 路线图引擎。
pub struct RoadmapEngine;

impl RoadmapEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：四向全在册。
        RoadmapLedger::complete()?;
        // 组二：开放位审计 + 反例（谎称开放拒）。
        open_slots_audit()?;
        let liar = OpenSlot { slot_id: 9, slot_name: "x", open: true, occupied_by: "fpga" };
        if liar.sound().is_ok() {
            return Err(XC_RM_OPEN_BAD);
        }
        let ghost = OpenSlot { slot_id: 9, slot_name: "x", open: false, occupied_by: "" };
        if ghost.sound().is_ok() {
            return Err(XC_RM_OPEN_BAD);
        }
        // 组三：标准双跟踪 + 反例（零周期拒）。
        std_tracks_audit()?;
        let dead = StdTrack { std_name: "Vulkan", ver_anchor: "1.3", review_days: 0 };
        if dead.sound().is_ok() {
            return Err(XC_RM_STD_GAP);
        }
        // 组四：总路线衔接。
        LinkAnchor { anchor: "docs/Varix/VE-STAR-II", k_span: [1, 9] }.linked()?;
        let loose = LinkAnchor { anchor: "", k_span: [1, 9] };
        if loose.linked().is_ok() {
            return Err(XC_RM_LINK_BROKEN);
        }
        let inverted = LinkAnchor { anchor: "x", k_span: [9, 1] };
        if inverted.linked().is_ok() {
            return Err(XC_RM_LINK_BROKEN);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xA900..0xB000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_RM_ITEM_MISSING,
            XC_RM_OPEN_BAD,
            XC_RM_STD_GAP,
            XC_RM_LINK_BROKEN,
            XC_RM_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA900 || codes[i] >= 0xB000 {
                return Err(XC_RM_ITEM_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_RM_ITEM_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
