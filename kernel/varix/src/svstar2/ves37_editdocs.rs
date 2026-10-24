//! VE-F3637 · 编辑器文档（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3637`
//!
//! **判据**：三册、映射公开、FAQ 驱动、2 跳复用、家族管道、判据。
//! **码段**：独占 **0x55xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// F3637 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V27Code(pub u16);

impl V27Code {
    /// 三册缺一。
    pub const VOL_MISS: V27Code = V27Code(0x5501);
    /// 映射缺。
    pub const MAP_MISS: V27Code = V27Code(0x5502);
    /// FAQ 过时。
    pub const FAQ_STALE: V27Code = V27Code(0x5503);
    /// 导航迷宫。
    pub const NAV_MAZE: V27Code = V27Code(0x5504);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V27Code::VOL_MISS => "三册缺一。建议：补册。",
            V27Code::MAP_MISS => "映射缺。建议：补控件-令牌映射。",
            V27Code::FAQ_STALE => "FAQ 过时。建议：复审。",
            V27Code::NAV_MAZE => "导航迷宫。建议：2 跳压缩。",
            _ => "未知诊断码。",
        }
    }
}

/// 三册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdVol {
    /// 用户手册。
    Manual,
    /// 参数指南。
    Params,
    /// FAQ。
    Faq,
}

impl EdVol {
    /// 全集。
    pub fn all() -> [EdVol; 3] {
        [EdVol::Manual, EdVol::Params, EdVol::Faq]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            EdVol::Manual => "用户手册",
            EdVol::Params => "参数指南",
            EdVol::Faq => "FAQ",
        }
    }
}

/// 三册文档。
#[derive(Clone, Debug, Default)]
pub struct EdDocs {
    vols: Vec<EdVol>,
    /// 控件-令牌映射条数。
    pub map_entries: usize,
    /// FAQ 日期戳。
    pub faq_stamp: u32,
    /// 当前日期戳。
    pub now_stamp: u32,
}

impl EdDocs {
    /// 新建。
    pub fn new() -> EdDocs {
        EdDocs {
            vols: Vec::new(),
            map_entries: 0,
            faq_stamp: 0,
            now_stamp: 0,
        }
    }

    /// 交付一册。
    pub fn deliver(&mut self, v: EdVol) {
        if !self.vols.contains(&v) {
            self.vols.push(v);
        }
    }

    /// 三册审计。
    pub fn audit_volumes(&self) -> Result<(), V27Code> {
        for v in EdVol::all() {
            if !self.vols.contains(&v) {
                return Err(V27Code::VOL_MISS);
            }
        }
        Ok(())
    }

    /// 映射公开审计。
    pub fn audit_mapping(&self) -> Result<(), V27Code> {
        if self.map_entries > 0 {
            Ok(())
        } else {
            Err(V27Code::MAP_MISS)
        }
    }

    /// FAQ 复审审计。
    pub fn audit_faq(&self) -> Result<(), V27Code> {
        if self.faq_stamp >= self.now_stamp {
            Ok(())
        } else {
            Err(V27Code::FAQ_STALE)
        }
    }
}

/// 家族管道索引（九用）。
pub const FAMILY_MEMBER_INDEX: u16 = 9;

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F2879", "VE-F3356", "CI-HOOK"];

/// 自检清单。
pub fn run_ves37_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3637");

    let mut d = EdDocs::new();
    for v in EdVol::all() {
        d.deliver(v);
    }
    d.map_entries = 10;
    d.faq_stamp = 5;
    d.now_stamp = 5;

    out.add("0x5501-三册齐", d.audit_volumes().is_ok(), "");
    out.add("0x5502-映射公开", d.audit_mapping().is_ok(), "");
    out.add("0x5503-FAQ新鲜", d.audit_faq().is_ok(), "");

    let d2 = EdDocs::new();
    out.add("0x5504-缺册", d2.audit_volumes().is_err(), "");

    let mut d3 = EdDocs::new();
    for v in EdVol::all() {
        d3.deliver(v);
    }
    d3.faq_stamp = 1;
    d3.now_stamp = 9;
    out.add("0x5505-FAQ过时", d3.audit_faq().is_err(), "");

    out
}
