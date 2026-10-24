//! VE-F0520 · GLSL 后端组收口（VE-C 域 · GLSL 后端组 C06 收口）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0520`
//!
//! **锚点原文**：「GLSL 后端组（C 域第六组 F0501-0519）收口：自检入树（架构、
//! 全族生成、控制流重建、版本扩展、质量、对拍、性能、错误回传、套件、限制、
//! 增量、扩展点、单测、调优、格式、一致性维护、fuzz、文档、总账十九件证据
//! 齐备）、缺陷总账清点、双签后移交 HLSL 后端组；经验包：结构化重建标志兜底
//! 方案、双级对拍、驱动怪癖规避方法论。判据：十九件证据、双签、经验包、
//! 缺陷清零、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十九件证据**（判据一）。本组十九件（F0501~F0519 逐件）以静态清单
//!    为正本，逐件收件、缺一阻断。
//!
//! 2. **双签**（判据二）。移交 HLSL 组需产出方与接收方双签（角色互异、
//!    指纹非零）。
//!
//! 3. **经验包**（判据三）。三条方法论（标志兜底、双级对拍、怪癖规避）
//!    带适用域与失效边界登记。
//!
//! 4. **缺陷清零**（判据四）。缺陷三态处置（修复/豁免/转登记）全落账，
//!    未处置残留阻断移交。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0520";

pub const UPSTREAM: &str = "VE-F0501..VE-F0519";
pub const DOWNSTREAM: &str = "VE-F0521";

/// 十九件证据正本（F0501~F0519 逐件）。
pub const EVID_TITLES: [&str; 19] = [
    "架构",
    "全族生成",
    "控制流重建",
    "版本扩展",
    "质量",
    "对拍",
    "性能",
    "错误回传",
    "套件",
    "限制",
    "增量",
    "扩展点",
    "单测",
    "调优",
    "格式",
    "一致性维护",
    "fuzz",
    "文档",
    "总账",
];

/// 证据账。
#[derive(Default)]
pub struct EvidBook {
    items: Vec<&'static str>,
}

impl EvidBook {
    pub fn new() -> EvidBook {
        EvidBook { items: Vec::new() }
    }

    pub fn submit(&mut self, title: &'static str) -> Result<(), u32> {
        if !EVID_TITLES.contains(&title) {
            return Err(XC_UNKNOWN);
        }
        if self.items.contains(&title) {
            return Err(XC_DUP);
        }
        self.items.push(title);
        Ok(())
    }

    pub fn missing(&self) -> Vec<&'static str> {
        EVID_TITLES.iter().filter(|t| !self.items.contains(t)).copied().collect()
    }

    pub fn complete(&self) -> bool {
        self.items.len() == EVID_TITLES.len()
    }
}

pub const XC_UNKNOWN: u32 = 1;
pub const XC_DUP: u32 = 2;

/// 缺陷三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disp {
    Fixed,
    Waived,
    ReReg,
}

/// 缺陷账。
#[derive(Default)]
pub struct DefectBook {
    pub ids: Vec<u32>,
    pub disp: Vec<(u32, Disp, bool)>,
}

impl DefectBook {
    pub fn new() -> DefectBook {
        DefectBook { ids: Vec::new(), disp: Vec::new() }
    }

    pub fn log(&mut self, id: u32) -> Result<(), u32> {
        if id == 0 {
            return Err(XC_ZEROID);
        }
        if self.ids.contains(&id) {
            return Err(XC_DUP);
        }
        self.ids.push(id);
        Ok(())
    }

    /// 处置：豁免/转登记须带理由。
    pub fn dispose(&mut self, id: u32, d: Disp, reason: bool) -> Result<(), u32> {
        if !self.ids.contains(&id) {
            return Err(XC_NOSUCH);
        }
        if (d == Disp::Waived || d == Disp::ReReg) && !reason {
            return Err(XC_NOREASON);
        }
        self.disp.push((id, d, reason));
        Ok(())
    }

    pub fn clear(&self) -> bool {
        self.ids.iter().all(|id| self.disp.iter().any(|(i, _, _)| i == id))
    }
}

pub const XC_ZEROID: u32 = 3;
pub const XC_NOSUCH: u32 = 4;
pub const XC_NOREASON: u32 = 5;

/// 双签。
pub struct Handover {
    sigs: [(bool, u32); 2],
    pub n: usize,
}

impl Handover {
    pub fn new() -> Handover {
        Handover { sigs: [(false, 0); 2], n: 0 }
    }

    pub fn sign(&mut self, producer: bool, fp: u32) -> Result<(), u32> {
        if fp == 0 {
            return Err(XC_ZEROFP);
        }
        if self.sigs[..self.n].iter().any(|(p, _)| *p == producer) {
            return Err(XC_DUPROLE);
        }
        if self.n >= 2 {
            return Err(XC_FULL);
        }
        self.sigs[self.n] = (producer, fp);
        self.n += 1;
        Ok(())
    }

    pub fn both(&self) -> bool {
        self.n == 2
    }
}

pub const XC_ZEROFP: u32 = 6;
pub const XC_DUPROLE: u32 = 7;
pub const XC_FULL: u32 = 8;

/// 经验包三条（带边界）。
pub struct Lesson {
    pub title: &'static str,
    pub scope: &'static str,
    pub limit: &'static str,
}

pub const LESSONS: [Lesson; 3] = [
    Lesson { title: "结构化重建标志兜底", scope: "多重跳出场景", limit: "旗数随跳出点数线性涨，深度嵌套需先迭化" },
    Lesson { title: "双级对拍", scope: "后端产物验证", limit: "语义级依赖环境，无环境时只做语法级并显性标注" },
    Lesson { title: "驱动怪癖规避", scope: "跨驱动产物兼容", limit: "怪癖表须逐驱动实测；凭论坛传闻登记无效" },
];

/// 三门收口。
pub fn gate(e: &EvidBook, d: &DefectBook, h: &Handover) -> bool {
    e.complete() && d.clear() && h.both()
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec120_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec120");

    // 族一：十九件证据。
    let mut eb = EvidBook::new();
    for t in EVID_TITLES.iter() {
        match eb.submit(t) {
            Ok(_) => {}
            Err(_) => { s.add("逐件收件", false, "正本内被拒"); return s; }
        }
    }
    s.add("十九件齐", eb.complete(), "F0501~F0519 逐件");
    s.add("清单外拒", matches!(eb.submit("件外"), Err(XC_UNKNOWN)), "伪造拒");
    s.add("重复件拒", matches!(eb.submit(EVID_TITLES[0]), Err(XC_DUP)), "一票一件");
    s.add("缺件可见", {
        let mut e2 = EvidBook::new();
        for t in EVID_TITLES.iter().take(19 - 1) {
            let _ = e2.submit(t);
        }
        e2.missing().len() == 1 && !e2.complete()
    }, "缺一即红");

    // 族二：缺陷清零。
    let mut db = DefectBook::new();
    let _ = db.log(1);
    let _ = db.log(2);
    s.add("三态处置过", {
        let _ = db.dispose(1, Disp::Fixed, false);
        let _ = db.dispose(2, Disp::Waived, true);
        db.clear()
    }, "全部落账");
    s.add("豁免无理由拒", {
        let mut d2 = DefectBook::new();
        let _ = d2.log(9);
        matches!(d2.dispose(9, Disp::Waived, false), Err(XC_NOREASON))
    }, "诚实处置");
    s.add("处置未知 id 拒", matches!(db.dispose(42, Disp::Fixed, false), Err(XC_NOSUCH)), "无此账");
    s.add("零 id 拒", matches!(db.log(0), Err(XC_ZEROID)), "正编号");

    // 族三：双签。
    let mut h = Handover::new();
    s.add("零指纹拒", matches!(h.sign(true, 0), Err(XC_ZEROFP)), "指纹非零");
    let _ = h.sign(true, 0xA1);
    s.add("同角色拒", matches!(h.sign(true, 0xA2), Err(XC_DUPROLE)), "角色互异");
    let _ = h.sign(false, 0xB1);
    s.add("三签满拒", matches!(h.sign(false, 0xB2), Err(XC_FULL)), "双签封顶");
    s.add("双签成立", h.both(), "移交条件");

    // 族四：经验包与三门。
    s.add("经验三条带边界", LESSONS.len() == 3 && LESSONS.iter().all(|l| !l.limit.is_empty()), "方法论非格言");
    s.add("三门放行", gate(&eb, &db, &h), "收口 Ready");
    s.add("缺证据阻断", {
        let mut e3 = EvidBook::new();
        for t in EVID_TITLES.iter().take(18) {
            let _ = e3.submit(t);
        }
        !gate(&e3, &db, &h)
    }, "十九件前置");
    s.add("批次衔接", UPSTREAM.contains("F0501") && DOWNSTREAM == "VE-F0521", "HLSL 组接续");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_ok() {
        let mut eb = EvidBook::new();
        for t in EVID_TITLES.iter() {
            let _ = eb.submit(t);
        }
        let mut h = Handover::new();
        let _ = h.sign(true, 1);
        let _ = h.sign(false, 2);
        assert!(gate(&eb, &DefectBook::new(), &h));
    }
}
