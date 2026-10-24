//! VE-F3795 · R 域移交包（S 域 F3801）（R10 收口组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3795`
//!
//! **判据（锚点原文）**：八件包、双签、退回路线、跨域对拍、移交书、判据。
//!
//! **本条是什么**：向 S 域（F3801）的正式移交——八件包（R 版七件 +
//! 双维册扩展：总账/分账/水位/心流/功耗/汇总/例外 + 双维册）双签移交；
//! 跨域对拍；移交书；分歧退回。
//!
//! 诊断码段：独占 0xBAxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBAxx）
// ─────────────────────────────────────────────────────────────

/// F3795 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BACode(pub u16);

impl BACode {
    /// 材料缺——补。
    pub const MAT_MISS: BACode = BACode(0xBA01);
    /// 签收缺——催办。
    pub const SIGN_MISS: BACode = BACode(0xBA02);
    /// 分歧——退回（P1）。
    pub const REJECT: BACode = BACode(0xBA03);
    /// 对拍不一——立案。
    pub const COMPARE_FAIL: BACode = BACode(0xBA04);
    /// 八件缺件。
    pub const PACK_MISS: BACode = BACode(0xBA05);
    /// 移交书缺。
    pub const LETTER_MISS: BACode = BACode(0xBA06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBA01 => "移交材料缺失，需补齐。",
            0xBA02 => "签收缺失，需催办。",
            0xBA03 => "移交分歧，退回处理。",
            0xBA04 => "跨域对拍不一致，需立案。",
            0xBA05 => "八件包缺件。",
            0xBA06 => "移交书缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 八件包
// ─────────────────────────────────────────────────────────────

/// 移交件（八件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pack {
    /// 总账。
    Master,
    /// 分账。
    Split,
    /// 水位。
    Watermark,
    /// 心流。
    Flow,
    /// 功耗。
    Power,
    /// 汇总。
    Summary,
    /// 例外。
    Exception,
    /// 双维册（R 扩展）。
    DualBook,
}

impl Pack {
    /// 全集。
    pub const ALL: [Pack; 8] = [
        Pack::Master,
        Pack::Split,
        Pack::Watermark,
        Pack::Flow,
        Pack::Power,
        Pack::Summary,
        Pack::Exception,
        Pack::DualBook,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Pack::Master => 0,
            Pack::Split => 1,
            Pack::Watermark => 2,
            Pack::Flow => 3,
            Pack::Power => 4,
            Pack::Summary => 5,
            Pack::Exception => 6,
            Pack::DualBook => 7,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Pack::Master => "总账",
            Pack::Split => "分账",
            Pack::Watermark => "水位",
            Pack::Flow => "心流",
            Pack::Power => "功耗",
            Pack::Summary => "汇总",
            Pack::Exception => "例外",
            Pack::DualBook => "双维册",
        }
    }

    /// 是否 R 扩展件。
    pub fn is_r_ext(self) -> bool {
        self == Pack::DualBook
    }
}

/// 移交状态。
#[derive(Clone, Debug)]
pub struct HandoverPack {
    packed: [bool; 8],
    /// 双签（交出+签收）。
    pub sign_out: bool,
    pub sign_in: bool,
    /// 移交书在册。
    pub letter: bool,
    /// 已移交。
    delivered: bool,
}

impl HandoverPack {
    /// 新建。
    pub const fn new() -> Self {
        HandoverPack {
            packed: [false; 8],
            sign_out: false,
            sign_in: false,
            letter: false,
            delivered: false,
        }
    }

    /// 装件。
    pub fn pack(&mut self, p: Pack) -> bool {
        let i = p.ordinal();
        if self.packed[i] {
            false
        } else {
            self.packed[i] = true;
            true
        }
    }

    /// 缺件数。
    pub fn missing(&self) -> usize {
        self.packed.iter().filter(|p| !**p).count()
    }

    /// 八件齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 双签齐。
    pub fn signed(&self) -> bool {
        self.sign_out && self.sign_in
    }

    /// 执行移交（八件齐+双签+移交书）。
    pub fn deliver(&mut self) -> bool {
        if self.complete() && self.signed() && self.letter {
            self.delivered = true;
            true
        } else {
            false
        }
    }

    /// 已移交。
    pub fn delivered(&self) -> bool {
        self.delivered
    }

    /// 退回（分歧后）。
    pub fn reject(&mut self) -> bool {
        if self.delivered {
            self.delivered = false;
            true
        } else {
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 跨域对拍
// ─────────────────────────────────────────────────────────────

/// 对拍器。
#[derive(Clone, Debug)]
pub struct CrossCompare {
    /// R 侧指纹。
    pub r_print: u16,
    /// 对端侧指纹。
    pub peer_print: u16,
    /// 立案数（分歧）。
    pub cases: u32,
}

impl CrossCompare {
    /// 新建。
    pub const fn new() -> Self {
        CrossCompare {
            r_print: 0,
            peer_print: 0,
            cases: 0,
        }
    }

    /// 执行对拍（不一致立案）。
    pub fn compare(&mut self) -> bool {
        if self.r_print == self.peer_print {
            true
        } else {
            self.cases += 1;
            false
        }
    }

    /// 立案数。
    pub fn case_count(&self) -> u32 {
        self.cases
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3795 判据集。
pub fn run_vr10o_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3795");

    // 0xBA05 八件包
    {
        let mut h = HandoverPack::new();
        set.add("0xBA05-初始缺八", h.missing() == 8, "");
        for p in Pack::ALL.iter() {
            h.pack(*p);
        }
        set.add("0xBA05-八件齐", h.complete(), "");
        set.add("0xBA05-重装拒", !h.pack(Pack::Master), "");
    }

    // R 扩展件
    {
        set.add("0xBA05-双维是扩展", Pack::DualBook.is_r_ext(), "");
        set.add("0xBA05-总账非扩展", !Pack::Master.is_r_ext(), "");
    }

    // 0xBA02 双签移交
    {
        let mut h = HandoverPack::new();
        for p in Pack::ALL.iter() {
            h.pack(*p);
        }
        h.sign_out = true;
        set.add("0xBA02-单签不交", !h.deliver() && !h.signed(), "");
        h.sign_in = true;
        h.letter = true;
        set.add("0xBA02-齐备可交", h.deliver() && h.delivered(), "");
        set.add("0xBA02-双签齐", h.signed(), "");
    }

    // 0xBA03 退回
    {
        let mut h = HandoverPack::new();
        for p in Pack::ALL.iter() {
            h.pack(*p);
        }
        h.sign_out = true;
        h.sign_in = true;
        h.letter = true;
        h.deliver();
        set.add("0xBA03-退回进", h.reject(), "");
        set.add("0xBA03-退回后未交", !h.delivered(), "");
        set.add("0xBA03-未交拒退", !h.reject(), "");
    }

    // 0xBA01 材料缺不交
    {
        let mut h = HandoverPack::new();
        h.sign_out = true;
        h.sign_in = true;
        h.letter = true;
        set.add("0xBA01-缺件不交", !h.deliver(), "");
    }

    // 0xBA04 跨域对拍
    {
        let mut c = CrossCompare::new();
        c.r_print = 42;
        c.peer_print = 42;
        set.add("0xBA04-同指纹过", c.compare(), "");
        set.add("0xBA04-零立案", c.case_count() == 0, "");
        c.peer_print = 43;
        set.add("0xBA04-异指纹不过", !c.compare(), "");
        set.add("0xBA04-立案计数", c.case_count() == 1, "");
    }

    // 件序自洽
    {
        let mut ok = true;
        for (i, p) in Pack::ALL.iter().enumerate() {
            if p.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBA05-件序自洽", ok, "");
    }

    set
}

