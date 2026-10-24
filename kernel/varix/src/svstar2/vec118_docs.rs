//! VE-F0518 · GLSL 后端文档（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0518`
//!
//! **锚点原文**：「GLSL 后端文档：使用文档（配置、选项、扩展点三节）、架构
//! 文档（三段管线与决策记录）、限制与怪癖文档（F0510 清单加驱动怪癖合并）；
//! 文档与实现同源生成（F0379 机制复用）；文档版本随组收口发布。判据：三节
//! 结构、同源生成、合并怪癖、随收口发布、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三节结构**（判据一）。使用文档三节（配置 / 选项 / 扩展点）逐节
//!    有内容（节清单 + 每节条目非空），缺节即文档不完整。
//!
//! 2. **同源生成**（判据二）。文档文本从实现元数据（常量表、决策记录、
//!    限制清单）派生生成——手抄即失同步；生成器按元数据单源产出。
//!
//! 3. **合并怪癖**（判据三）。限制清单（F0510 三类）与驱动怪癖（F0508
//!    规避表）合并为一节产出，双源合一但不混排（来源标签区分）。
//!
//! 4. **随收口发布**（判据四）。文档版本号与组收口版本绑定（版本联动
//!    可验证），未发布版本文档不入收口证据。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0518";

// ---------------------------------------------------------------------------
// 一、文档三节
// ---------------------------------------------------------------------------

/// 文档节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    Config,
    Opts,
    Ext,
}

/// 节条目。
#[derive(Clone, Debug)]
pub struct Item {
    pub sec: Sec,
    pub text: String,
}

/// 文档账。
#[derive(Default)]
pub struct DocBook {
    pub items: Vec<Item>,
}

impl DocBook {
    pub fn new() -> DocBook {
        DocBook { items: Vec::new() }
    }

    pub fn add(&mut self, sec: Sec, text: &str) -> Result<(), u32> {
        if text.is_empty() {
            return Err(XC_EMPTY);
        }
        self.items.push(Item { sec, text: text.to_string() });
        Ok(())
    }

    /// 三节齐备（每节至少一条）。
    pub fn complete(&self) -> bool {
        let has = |s: Sec| self.items.iter().any(|i| i.sec == s);
        has(Sec::Config) && has(Sec::Opts) && has(Sec::Ext)
    }

    pub fn count(&self, s: Sec) -> usize {
        self.items.iter().filter(|i| i.sec == s).count()
    }
}

pub const XC_EMPTY: u32 = 1;

// ---------------------------------------------------------------------------
// 二、同源生成（元数据 → 文本）
// ---------------------------------------------------------------------------

/// 实现元数据（配置项正本）。
pub const CFG_META: [&str; 3] = ["target-version", "extensions", "format-profile"];
/// 选项元数据正本。
pub const OPT_META: [&str; 3] = ["indent-style", "comment-keep", "name-policy"];
/// 扩展点元数据正本。
pub const EXT_META: [&str; 3] = ["tpl-inject", "ver-hook", "post-filter"];

/// 从元数据生成使用文档三节（同源：元数据空则生成失败显性报错）。
pub fn gen_usage(cfg: &[&str], opts: &[&str], ext: &[&str]) -> Result<Vec<(Sec, String)>, u32> {
    if cfg.is_empty() || opts.is_empty() || ext.is_empty() {
        return Err(XC_META);
    }
    let mut out: Vec<(Sec, String)> = Vec::new();
    for c in cfg.iter() {
        out.push((Sec::Config, format!("配置项: {}", c)));
    }
    for o in opts.iter() {
        out.push((Sec::Opts, format!("选项: {}", o)));
    }
    for e in ext.iter() {
        out.push((Sec::Ext, format!("扩展点: {}", e)));
    }
    Ok(out)
}

pub const XC_META: u32 = 2;

// ---------------------------------------------------------------------------
// 三、限制与怪癖合并节
// ---------------------------------------------------------------------------

/// 来源标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Limit,
    Quirk,
}

/// 合并条目。
#[derive(Clone, Debug)]
pub struct Merged {
    pub src: Src,
    pub title: String,
    pub remedy: String,
}

/// 合并生成：限制三类 + 怪癖表，来源标签区分、不混排（限先怪后）。
pub fn merge_lq(limits: &[&str], quirks: &[&str]) -> Vec<Merged> {
    let mut out: Vec<Merged> = Vec::new();
    for l in limits.iter() {
        out.push(Merged { src: Src::Limit, title: l.to_string(), remedy: "替代方案见限制清单".to_string() });
    }
    for q in quirks.iter() {
        out.push(Merged { src: Src::Quirk, title: q.to_string(), remedy: "规避改写见怪癖表".to_string() });
    }
    out
}

/// 混排校验：限制段全部在前。
pub fn ordered(m: &[Merged]) -> bool {
    let mut seen_quirk = false;
    for x in m.iter() {
        if x.src == Src::Quirk {
            seen_quirk = true;
        } else if seen_quirk {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 四、版本联动（随收口发布）
// ---------------------------------------------------------------------------

/// 文档版本 = 组收口版本 × 10 + 节修订号（联动可验证）。
pub fn doc_ver(release: u32, rev: u32) -> u32 {
    release * 10 + rev.min(9)
}

/// 发布校验：文档版本必须 >= 收口版本 × 10。
pub fn published(doc: u32, release: u32) -> bool {
    doc >= release * 10
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec118_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec118");

    // 族一：三节结构。
    let mut d = DocBook::new();
    s.add("空条目拒", matches!(d.add(Sec::Config, ""), Err(XC_EMPTY)), "节内容必填");
    let g = match gen_usage(&CFG_META, &OPT_META, &EXT_META) {
        Ok(v) => v,
        Err(_) => { s.add("生成败", false, "不应到"); return s; }
    };
    for (sec, text) in g.iter() {
        let _ = d.add(*sec, text);
    }
    s.add("三节齐备", d.complete(), "配置/选项/扩展点");
    s.add("三节各三件", d.count(Sec::Config) == 3 && d.count(Sec::Opts) == 3 && d.count(Sec::Ext) == 3, "元数据全量");

    // 族二：同源生成。
    s.add("元数据缺节拒", matches!(gen_usage(&CFG_META, &[], &EXT_META), Err(XC_META)), "空元数据显性");
    s.add("同源再现", gen_usage(&CFG_META, &OPT_META, &EXT_META).is_ok(), "元数据驱动");

    // 族三：合并怪癖。
    let m = merge_lq(&["ssbo-atomic 不可用", "subgroup 降级"], &["drv-a pow(0,0)"]);
    s.add("限先怪后", ordered(&m), "不混排");
    s.add("来源标签", m.iter().filter(|x| x.src == Src::Limit).count() == 2 && m.iter().filter(|x| x.src == Src::Quirk).count() == 1, "双源合一");
    s.add("混排可见", !ordered(&[Merged { src: Src::Quirk, title: "q".to_string(), remedy: "r".to_string() }, Merged { src: Src::Limit, title: "l".to_string(), remedy: "r".to_string() }]), "乱序即违");

    // 族四：版本联动。
    s.add("版本合成", doc_ver(3, 2) == 32, "release×10+rev");
    s.add("修订封顶", doc_ver(3, 15) == 39, "rev ≤ 9");
    s.add("发布校验过", published(32, 3), "文档随收口");
    s.add("旧版文档拦", !published(29, 3), "未发布不入证据");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen() {
        assert!(gen_usage(&["a"], &["b"], &["c"]).is_ok());
        assert!(gen_usage(&["a"], &[], &["c"]).is_err());
    }
}
