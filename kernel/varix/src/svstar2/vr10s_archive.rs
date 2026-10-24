//! VE-F3799 · R 域归档与证据三件套（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3799`
//!
//! **判据（锚点原文）**：五件包、双签、证据三件套、可复现脚本、判据。
//!
//! **本条是什么**：R 域归档收口——五件包（总账/分账/水位/心流/功耗）；
//! 双签归档；证据三件套（截图/日志/统计报告）；可复现脚本登记。
//!
//! 诊断码段：独占 0xBExx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xBExx）
// ─────────────────────────────────────────────────────────────

/// F3799 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BECode(pub u16);

impl BECode {
    /// 证据缺——补。
    pub const EVIDENCE_MISS: BECode = BECode(0xBE01);
    /// 复现缺——补脚本。
    pub const REPRO_MISS: BECode = BECode(0xBE02);
    /// 签收缺——催办。
    pub const SIGN_MISS: BECode = BECode(0xBE03);
    /// 五件缺件。
    pub const PACK_MISS: BECode = BECode(0xBE04);
    /// 归档未生效。
    pub const ARCHIVE_OPEN: BECode = BECode(0xBE05);
    /// 三件套不齐。
    pub const TRIO_MISS: BECode = BECode(0xBE06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xBE01 => "证据三件套缺失，需补。",
            0xBE02 => "可复现脚本缺失，需补脚本。",
            0xBE03 => "签收缺失，需催办。",
            0xBE04 => "五件包缺件。",
            0xBE05 => "归档未生效。",
            0xBE06 => "证据三件套不齐。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 五件包 + 双签
// ─────────────────────────────────────────────────────────────

/// 归档件（五件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArcItem {
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
}

impl ArcItem {
    /// 全集。
    pub const ALL: [ArcItem; 5] = [
        ArcItem::Master,
        ArcItem::Split,
        ArcItem::Watermark,
        ArcItem::Flow,
        ArcItem::Power,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ArcItem::Master => 0,
            ArcItem::Split => 1,
            ArcItem::Watermark => 2,
            ArcItem::Flow => 3,
            ArcItem::Power => 4,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ArcItem::Master => "总账",
            ArcItem::Split => "分账",
            ArcItem::Watermark => "水位",
            ArcItem::Flow => "心流",
            ArcItem::Power => "功耗",
        }
    }
}

/// 证据三件套。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// 截图。
    Screenshot,
    /// 日志。
    Log,
    /// 统计报告。
    StatReport,
}

impl Evidence {
    /// 全集。
    pub const ALL: [Evidence; 3] = [
        Evidence::Screenshot,
        Evidence::Log,
        Evidence::StatReport,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Evidence::Screenshot => 0,
            Evidence::Log => 1,
            Evidence::StatReport => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Evidence::Screenshot => "截图",
            Evidence::Log => "日志",
            Evidence::StatReport => "统计报告",
        }
    }
}

/// 归档单。
#[derive(Clone, Debug)]
pub struct Archive {
    packed: [bool; 5],
    evidence: [bool; 3],
    /// 双签（归档+核收）。
    pub sign_arc: bool,
    pub sign_recv: bool,
    /// 可复现脚本在册。
    pub repro_script: bool,
    /// 归档生效。
    effective: bool,
}

impl Archive {
    /// 新建。
    pub const fn new() -> Self {
        Archive {
            packed: [false; 5],
            evidence: [false; 3],
            sign_arc: false,
            sign_recv: false,
            repro_script: false,
            effective: false,
        }
    }

    /// 装件。
    pub fn pack(&mut self, a: ArcItem) -> bool {
        let i = a.ordinal();
        if self.packed[i] {
            false
        } else {
            self.packed[i] = true;
            true
        }
    }

    /// 收证据。
    pub fn evidence_take(&mut self, e: Evidence) -> bool {
        let i = e.ordinal();
        if self.evidence[i] {
            false
        } else {
            self.evidence[i] = true;
            true
        }
    }

    /// 缺件数。
    pub fn missing(&self) -> usize {
        self.packed.iter().filter(|p| !**p).count()
    }

    /// 三件套缺数。
    pub fn evidence_missing(&self) -> usize {
        self.evidence.iter().filter(|e| !**e).count()
    }

    /// 双签齐。
    pub fn signed(&self) -> bool {
        self.sign_arc && self.sign_recv
    }

    /// 生效（五件+三证据+双签+脚本）。
    pub fn seal(&mut self) -> bool {
        if self.missing() == 0
            && self.evidence_missing() == 0
            && self.signed()
            && self.repro_script
        {
            self.effective = true;
            true
        } else {
            false
        }
    }

    /// 已生效。
    pub fn effective(&self) -> bool {
        self.effective
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3799 判据集。
pub fn run_vr10s_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3799");

    // 0xBE04 五件包
    {
        let mut a = Archive::new();
        set.add("0xBE04-初始缺五", a.missing() == 5, "");
        for it in ArcItem::ALL.iter() {
            a.pack(*it);
        }
        set.add("0xBE04-五件齐", a.missing() == 0, "");
        set.add("0xBE04-重装拒", !a.pack(ArcItem::Master), "");
    }

    // 0xBE01/0xBE06 三件套
    {
        let mut a = Archive::new();
        set.add("0xBE06-初始缺三", a.evidence_missing() == 3, "");
        for e in Evidence::ALL.iter() {
            a.evidence_take(*e);
        }
        set.add("0xBE06-三件齐", a.evidence_missing() == 0, "");
        set.add("0xBE01-重收拒", !a.evidence_take(Evidence::Log), "");
    }

    // 0xBE03 双签
    {
        let mut a = Archive::new();
        set.add("0xBE03-初始未签", !a.signed(), "");
        a.sign_arc = true;
        set.add("0xBE03-单签不齐", !a.signed(), "");
        a.sign_recv = true;
        set.add("0xBE03-双签齐", a.signed(), "");
    }

    // 0xBE02 复现脚本
    {
        let mut a = Archive::new();
        for it in ArcItem::ALL.iter() {
            a.pack(*it);
        }
        for e in Evidence::ALL.iter() {
            a.evidence_take(*e);
        }
        a.sign_arc = true;
        a.sign_recv = true;
        set.add("0xBE02-无脚本不封", !a.seal() && !a.effective(), "");
        a.repro_script = true;
        set.add("0xBE02-有脚本封", a.seal() && a.effective(), "");
    }

    // 0xBE05 不齐不封
    {
        let mut a = Archive::new();
        a.repro_script = true;
        a.sign_arc = true;
        a.sign_recv = true;
        set.add("0xBE05-缺件不封", !a.seal(), "");
    }

    // 件序自洽
    {
        let mut ok = true;
        for (i, it) in ArcItem::ALL.iter().enumerate() {
            if it.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xBE04-件序自洽", ok, "");
        let mut ok_e = true;
        for (i, e) in Evidence::ALL.iter().enumerate() {
            if e.ordinal() != i {
                ok_e = false;
            }
        }
        set.add("0xBE06-证据序自洽", ok_e, "");
    }

    set
}
