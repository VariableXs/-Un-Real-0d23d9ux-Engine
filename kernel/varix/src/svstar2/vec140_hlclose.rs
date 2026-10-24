//! VE-F0540 · HLSL 组收口（VE-C 域 · HLSL 后端组 C07 收口）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0540`
//!
//! **锚点原文**：「HLSL 后端组（C 域第七组 F0521-0539）收口：自检入树（架构、
//! 类型映射、资源声明、分配器、指令族、控制流复用、系统值、验证通路、对拍、
//! 质量、错误回传、现代路径、套件、限制、增量、fuzz、文档、总账、协作协议
//! 十九件证据齐备）、缺陷总账清点、双签后移交 WGSL 后端组；经验包：register
//! 显式优先、主序语义显性化、外部工具链隔离防护方法论。判据：十九件证据、
//! 双签、经验包、缺陷清零、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十九件证据**（判据一）。F0521~F0539 逐件登记，缺一阻断。
//! 2. **双签**（判据二）。移交 WGSL 组双签（角色互异、指纹非零）。
//! 3. **经验包**（判据三）。三条方法论带适用域与失效边界。
//! 4. **缺陷清零**（判据四）。三态处置全落账。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0540";

pub const UPSTREAM: &str = "VE-F0521..VE-F0539";
pub const DOWNSTREAM: &str = "VE-F0541";

/// 十九件证据正本。
pub const EVID_TITLES: [&str; 19] = [
    "架构",
    "类型映射",
    "资源声明",
    "分配器",
    "指令族",
    "控制流复用",
    "系统值",
    "验证通路",
    "对拍",
    "质量",
    "错误回传",
    "现代路径",
    "套件",
    "限制",
    "增量",
    "fuzz",
    "文档",
    "总账",
    "协作协议",
];

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

#[derive(Default)]
pub struct DefectBook {
    pub ids: Vec<u32>,
    pub done: Vec<(u32, Disp, bool)>,
}

impl DefectBook {
    pub fn new() -> DefectBook {
        DefectBook { ids: Vec::new(), done: Vec::new() }
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

    pub fn dispose(&mut self, id: u32, d: Disp, reason: bool) -> Result<(), u32> {
        if !self.ids.contains(&id) {
            return Err(XC_NOSUCH);
        }
        if (d == Disp::Waived || d == Disp::ReReg) && !reason {
            return Err(XC_NOREASON);
        }
        self.done.push((id, d, reason));
        Ok(())
    }

    pub fn clear(&self) -> bool {
        self.ids.iter().all(|id| self.done.iter().any(|(i, _, _)| i == id))
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

/// 经验包三条。
pub struct Lesson {
    pub title: &'static str,
    pub scope: &'static str,
    pub limit: &'static str,
}

pub const LESSONS: [Lesson; 3] = [
    Lesson { title: "register 显式优先", scope: "HLSL 资源分配", limit: "显式撞自动须报冲突；静默重排即违约" },
    Lesson { title: "主序语义显性化", scope: "矩阵类映射", limit: "缺省请求不裁定；缺请求即拒" },
    Lesson { title: "外部工具链隔离防护", scope: "dxc/fxc 调用", limit: "超时崩溃都标注后回退；不进编译器主流程" },
];

/// 三门收口。
pub fn gate(e: &EvidBook, d: &DefectBook, h: &Handover) -> bool {
    e.complete() && d.clear() && h.both()
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec140_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec140");

    // 族一：十九件。
    let mut eb = EvidBook::new();
    for t in EVID_TITLES.iter() {
        match eb.submit(t) {
            Ok(_) => {}
            Err(_) => { s.add("逐件收件", false, "不应到"); return s; }
        }
    }
    s.add("十九件齐", eb.complete(), "F0521~F0539");
    s.add("清单外拒", matches!(eb.submit("件外"), Err(XC_UNKNOWN)), "伪造拒");
    s.add("重复拒", matches!(eb.submit(EVID_TITLES[0]), Err(XC_DUP)), "一票一件");
    s.add("缺件可见", {
        let mut e2 = EvidBook::new();
        for t in EVID_TITLES.iter().take(18) {
            let _ = e2.submit(t);
        }
        e2.missing().len() == 1
    }, "缺一即红");

    // 族二：缺陷清零。
    let mut db = DefectBook::new();
    let _ = db.log(1);
    let _ = db.log(2);
    s.add("三态处置", {
        let _ = db.dispose(1, Disp::Fixed, false);
        let _ = db.dispose(2, Disp::ReReg, true);
        db.clear()
    }, "全落账");
    s.add("无理由豁免拒", {
        let mut d2 = DefectBook::new();
        let _ = d2.log(9);
        matches!(d2.dispose(9, Disp::Waived, false), Err(XC_NOREASON))
    }, "诚实处置");
    s.add("未知 id 拒", matches!(db.dispose(42, Disp::Fixed, false), Err(XC_NOSUCH)), "无此账");

    // 族三：双签。
    let mut h = Handover::new();
    s.add("零指纹拒", matches!(h.sign(true, 0), Err(XC_ZEROFP)), "指纹非零");
    let _ = h.sign(true, 0xC1);
    s.add("同角色拒", matches!(h.sign(true, 0xC2), Err(XC_DUPROLE)), "角色互异");
    let _ = h.sign(false, 0xD1);
    s.add("三签满拒", matches!(h.sign(false, 0xD2), Err(XC_FULL)), "双签封顶");
    s.add("双签成立", h.both(), "移交条件");

    // 族四：经验包与三门。
    s.add("经验三条带边界", LESSONS.len() == 3 && LESSONS.iter().all(|l| !l.limit.is_empty()), "方法论非格言");
    s.add("三门放行", gate(&eb, &db, &h), "收口 Ready");
    s.add("缺证据阻断", {
        let mut e3 = EvidBook::new();
        for t in EVID_TITLES.iter().take(19 - 1) {
            let _ = e3.submit(t);
        }
        !gate(&e3, &db, &h)
    }, "十九件前置");
    s.add("批次衔接", UPSTREAM.contains("F0521") && DOWNSTREAM == "VE-F0541", "WGSL 组接续");

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
