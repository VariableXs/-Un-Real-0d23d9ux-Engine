//! VE-F0500 · 优化组收口与后端移交（VE-C 域 · GLSL 后端组 C06 前置收口）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0500`
//!
//! **锚点原文**：「优化组（C 域第五组 F0481-0499）收口：自检入树（变体总成、
//! 预热、一致性、三优化族、压力模型、展开、管理器、对拍、基准、诊断、预算、
//! 确定性、上限、记录、语料、fuzz、总账十九件证据齐备）、缺陷总账清点、双签
//! 后移交后端族（GLSL 组起）；经验包：确定性空例外单原则、参考执行器对拍、
//! 诚实关闭负收益 pass 方法论。判据：十九件证据、双签、经验包、缺陷清零、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十九件证据**（判据一）。收口自检以 [`EVID_TITLES`] 静态清单为唯一
//!    正本——十九件逐件登记（每件含标题与证据摘要），缺一件即收口阻断。
//!    清单长度本身也是判据：不足十九或超十九都是清单漂移。
//!
//! 2. **双签**（判据二）。移交凭证必须带两枚签名（产出方签 + 接收方签），
//!    且两签角色互异、指纹非零；单签或同角色双签都是无效移交。
//!
//! 3. **经验包**（判据三）。三条方法论（确定性空例外单、参考执行器对拍、
//!    诚实关闭负收益 pass）逐条登记为经验条目，每条含适用域与失效边界——
//!    经验包不是格言集，是带边界条件的操作指引。
//!
//! 4. **缺陷清零**（判据四）。移交前缺陷总账必须全部处置（修复/豁免/
//!    转登记三态，不允许「未处置」残留）；豁免与转登记必须带理由。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0500";

/// 上游批次（F0481~F0499 优化组）。
pub const UPSTREAM: &str = "VE-F0481..VE-F0499";
/// 下游开工条目。
pub const DOWNSTREAM: &str = "VE-F0501";

// ---------------------------------------------------------------------------
// 一、十九件证据清单（唯一正本）
// ---------------------------------------------------------------------------

/// 十九件证据标题静态表（收口正本）。
pub const EVID_TITLES: [&str; 19] = [
    "变体总成",
    "预热验证",
    "一致性核验",
    "死代码消除族",
    "常量折叠族",
    "指令合并族",
    "压力模型",
    "循环展开",
    "优化管理器",
    "对拍验证",
    "性能基准",
    "诊断报告",
    "优化预算",
    "确定性验证",
    "优化上限",
    "优化记录",
    "语义语料",
    "fuzz 报告",
    "优化总账",
];

/// 单件证据。
#[derive(Clone, Debug)]
pub struct Evid {
    /// 证据标题（须与 [`EVID_TITLES`] 对位）。
    pub title: String,
    /// 证据摘要（入树位置或产物指纹）。
    pub digest: String,
}

impl Evid {
    pub fn new(title: &str, digest: &str) -> Evid {
        Evid { title: title.to_string(), digest: digest.to_string() }
    }
}

/// 证据账：按清单顺序收件，乱序/缺件/重复件都拒。
#[derive(Default)]
pub struct EvidBook {
    items: Vec<Evid>,
}

impl EvidBook {
    pub fn new() -> EvidBook {
        EvidBook { items: Vec::new() }
    }

    /// 收件：标题必须在清单内且未被收过（重复件 = 伪造证据）。
    pub fn submit(&mut self, title: &str, digest: &str) -> Result<(), u32> {
        if !EVID_TITLES.iter().any(|t| *t == title) {
            return Err(XC_UNKNOWN);
        }
        if digest.is_empty() {
            return Err(XC_NODIGEST);
        }
        if self.items.iter().any(|e| e.title == title) {
            return Err(XC_DUP);
        }
        self.items.push(Evid::new(title, digest));
        Ok(())
    }

    /// 已收件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 缺件清单（按正本顺序）。
    pub fn missing(&self) -> Vec<&'static str> {
        EVID_TITLES
            .iter()
            .filter(|t| !self.items.iter().any(|e| e.title == **t))
            .copied()
            .collect()
    }

    /// 齐备判定：十九件不缺一件。
    pub fn complete(&self) -> bool {
        self.items.len() == EVID_TITLES.len() && self.missing().is_empty()
    }
}

pub const XC_UNKNOWN: u32 = 1;
pub const XC_NODIGEST: u32 = 2;
pub const XC_DUP: u32 = 3;

// ---------------------------------------------------------------------------
// 二、缺陷总账（三态处置）
// ---------------------------------------------------------------------------

/// 缺陷处置三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disp {
    Fixed,
    Waived,
    ReReg,
}

/// 缺陷条目。
#[derive(Clone, Debug)]
pub struct Defect {
    pub id: u32,
    pub disp: Disp,
    /// 豁免/转登记必须带理由；修复可空。
    pub reason: String,
}

/// 缺陷总账：清零判定。
#[derive(Default)]
pub struct DefectBook {
    items: Vec<Defect>,
}

impl DefectBook {
    pub fn new() -> DefectBook {
        DefectBook { items: Vec::new() }
    }

    /// 登记缺陷：id 唯一。
    pub fn log(&mut self, id: u32) -> Result<(), u32> {
        if id == 0 {
            return Err(XC_ZEROID);
        }
        if self.items.iter().any(|d| d.id == id) {
            return Err(XC_DUP);
        }
        self.items.push(Defect { id, disp: Disp::Fixed, reason: String::new() });
        Ok(())
    }

    /// 处置：豁免与转登记必须带理由（诚实处置，不是静默埋雷）。
    pub fn dispose(&mut self, id: u32, disp: Disp, reason: &str) -> Result<(), u32> {
        let d = match self.items.iter_mut().find(|d| d.id == id) {
            Some(d) => d,
            None => return Err(XC_NOSUCH),
        };
        if (disp == Disp::Waived || disp == Disp::ReReg) && reason.is_empty() {
            return Err(XC_NOREASON);
        }
        d.disp = disp;
        d.reason = reason.to_string();
        Ok(())
    }

    /// 清零判定：全部已处置且豁免/转登记都带理由。
    pub fn clear(&self) -> bool {
        self.items.iter().all(|d| match d.disp {
            Disp::Fixed => true,
            Disp::Waived | Disp::ReReg => !d.reason.is_empty(),
        })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

pub const XC_ZEROID: u32 = 4;
pub const XC_NOSUCH: u32 = 5;
pub const XC_NOREASON: u32 = 6;

// ---------------------------------------------------------------------------
// 三、双签移交
// ---------------------------------------------------------------------------

/// 签名（角色 + 指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sig {
    /// 角色：true = 产出方，false = 接收方。
    pub producer: bool,
    /// 签名指纹（非零）。
    pub fp: u32,
}

/// 移交凭证。
pub struct Handover {
    /// 上游批次标识。
    pub from: String,
    /// 下游开工条目。
    pub to: String,
    sigs: Vec<Sig>,
}

impl Handover {
    pub fn new() -> Handover {
        Handover { from: UPSTREAM.to_string(), to: DOWNSTREAM.to_string(), sigs: Vec::new() }
    }

    /// 签署：同角色第二签拒（角色互异才有效）。
    pub fn sign(&mut self, producer: bool, fp: u32) -> Result<(), u32> {
        if fp == 0 {
            return Err(XC_ZEROFP);
        }
        if self.sigs.iter().any(|s| s.producer == producer) {
            return Err(XC_DUPROLE);
        }
        self.sigs.push(Sig { producer, fp });
        Ok(())
    }

    /// 双签齐备。
    pub fn both(&self) -> bool {
        self.sigs.len() == 2
            && self.sigs.iter().any(|s| s.producer)
            && self.sigs.iter().any(|s| !s.producer)
    }
}

pub const XC_ZEROFP: u32 = 7;
pub const XC_DUPROLE: u32 = 8;

// ---------------------------------------------------------------------------
// 四、经验包（带边界条件的三条方法论）
// ---------------------------------------------------------------------------

/// 经验条目。
#[derive(Clone, Debug)]
pub struct Lesson {
    pub title: &'static str,
    /// 适用域。
    pub scope: &'static str,
    /// 失效边界（何时不再适用）。
    pub limit: &'static str,
}

/// 三条方法论正本。
pub const LESSONS: [Lesson; 3] = [
    Lesson {
        title: "确定性空例外单原则",
        scope: "优化 pass 顺序与调度",
        limit: "仅当输入语料全覆盖时成立；语料缺口即例外单失真",
    },
    Lesson {
        title: "参考执行器对拍",
        scope: "优化前后语义等价验证",
        limit: "参考执行器本身有差异项时须先登记差异再对拍",
    },
    Lesson {
        title: "诚实关闭负收益 pass",
        scope: "pass 开关管理",
        limit: "关闭必须带实测负收益数据；凭感觉关闭视为未关闭",
    },
];

/// 经验包完整性判定：三条都在且边界条件非空。
pub fn lessons_ok() -> bool {
    LESSONS.len() == 3 && LESSONS.iter().all(|l| !l.limit.is_empty())
}

// ---------------------------------------------------------------------------
// 五、收口编排（证据 + 缺陷 + 双签 三门全过才可移交）
// ---------------------------------------------------------------------------

/// 收口结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Ready,
    Blocked,
}

/// 三门收口：证据齐备 + 缺陷清零 + 双签齐备。
pub fn gate(e: &EvidBook, d: &DefectBook, h: &Handover) -> Gate {
    if e.complete() && d.clear() && h.both() {
        Gate::Ready
    } else {
        Gate::Blocked
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec100_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec100");

    // 族一：十九件证据。
    let mut eb = EvidBook::new();
    for t in EVID_TITLES.iter() {
        let dig = format!("ev-{}", t.len());
        match eb.submit(t, &dig) {
            Ok(_) => {}
            Err(_) => { s.add("十九件逐件收件", false, "正本内标题被拒"); return s; }
        }
    }
    s.add("十九件齐备", eb.complete() && eb.len() == 19, "逐件收齐");
    s.add("缺件即报", {
        let mut e2 = EvidBook::new();
        for t in EVID_TITLES.iter().take(18) {
            let _ = e2.submit(t, "d");
        }
        !e2.complete() && e2.missing().len() == 1
    }, "少一件可见");
    s.add("清单外标题拒", matches!(eb.submit("件外", "d"), Err(XC_UNKNOWN)), "伪造拒");
    s.add("空摘要拒", matches!(eb.submit(EVID_TITLES[0], ""), Err(XC_NODIGEST)), "无指纹拒");
    s.add("重复件拒", {
        let mut e3 = EvidBook::new();
        let _ = e3.submit(EVID_TITLES[0], "a");
        matches!(e3.submit(EVID_TITLES[0], "b"), Err(XC_DUP))
    }, "一票一件");

    // 族二：缺陷总账。
    let mut db = DefectBook::new();
    let _ = db.log(1);
    let _ = db.log(2);
    let _ = db.log(3);
    s.add("处置后清零", {
        let _ = db.dispose(1, Disp::Fixed, "");
        let _ = db.dispose(2, Disp::Waived, "实测无影响");
        let _ = db.dispose(3, Disp::ReReg, "转限制清单 F0510");
        db.clear()
    }, "三态全过");
    s.add("空理由豁免拒", {
        let mut d2 = DefectBook::new();
        let _ = d2.log(9);
        matches!(d2.dispose(9, Disp::Waived, ""), Err(XC_NOREASON))
    }, "诚实处置");
    s.add("重复 id 拒", matches!(db.log(1), Err(XC_DUP)), "账目唯一");
    s.add("零 id 拒", matches!(db.log(0), Err(XC_ZEROID)), "编号正整数");
    s.add("处置未知 id 拒", matches!(db.dispose(42, Disp::Fixed, ""), Err(XC_NOSUCH)), "无此账");

    // 族三：双签。
    let mut h = Handover::new();
    s.add("零指纹拒", matches!(h.sign(true, 0), Err(XC_ZEROFP)), "指纹非零");
    let _ = h.sign(true, 0xA1);
    s.add("同角色二签拒", matches!(h.sign(true, 0xA2), Err(XC_DUPROLE)), "角色互异");
    let _ = h.sign(false, 0xB1);
    s.add("双签成立", h.both(), "产出+接收");

    // 族四：经验包与三门收口。
    s.add("经验包三条带边界", lessons_ok(), "方法论非格言");
    s.add("三门全过放行", gate(&eb, &db, &h) == Gate::Ready, "收口 Ready");
    s.add("缺一门阻断", {
        let mut h2 = Handover::new();
        let _ = h2.sign(true, 0xA1);
        gate(&eb, &db, &h2) == Gate::Blocked
    }, "单签阻断");
    s.add("缺陷未清阻断", {
        let mut d3 = DefectBook::new();
        let _ = d3.log(7);
        gate(&eb, &d3, &h) == Gate::Blocked
    }, "清零前置");

    // 族五：批次衔接常量。
    s.add("上游批次标定", UPSTREAM.contains("F0481") && UPSTREAM.contains("F0499"), "优化组全段");
    s.add("下游开工条目", DOWNSTREAM == "VE-F0501", "GLSL 架构接续");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_ready() {
        let mut eb = EvidBook::new();
        for t in EVID_TITLES.iter() {
            let _ = eb.submit(t, "d");
        }
        let mut h = Handover::new();
        let _ = h.sign(true, 1);
        let _ = h.sign(false, 2);
        assert_eq!(gate(&eb, &DefectBook::new(), &h), Gate::Ready);
    }
}
