//! VE-F0537 · HLSL 后端文档（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0537`
//!
//! **锚点原文**：「HLSL 后端文档：使用文档（配置、双工具链、选项）、架构
//! 文档（管线与决策记录）、限制与工具链怪癖文档（F0534 加 F0531 怪癖合并）；
//! 同源生成机制复用（F0379）；随组收口发布。判据：三节结构、同源生成、怪癖
//! 合并、随收口发布、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三节结构**（判据一）。使用文档三节（配置 / 双工具链 / 选项）逐节
//!    有条目，缺节即不完整。
//!
//! 2. **同源生成**（判据二）。文档从实现元数据派生（F0379 机制），元数据
//!    缺失显性报错。
//!
//! 3. **怪癖合并**（判据三）。F0534 限制与 F0531 工具链怪癖合并一节，来源
//!    标签区分不混排（限先怪后）。
//!
//! 4. **随收口发布**（判据四）。文档版本与组收口版本联动（版本合成可验证）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0537";

/// 文档节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    Config,
    Tool,
    Opts,
}

/// 条目。
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

    pub fn complete(&self) -> bool {
        let has = |s: Sec| self.items.iter().any(|i| i.sec == s);
        has(Sec::Config) && has(Sec::Tool) && has(Sec::Opts)
    }

    pub fn count(&self, s: Sec) -> usize {
        self.items.iter().filter(|i| i.sec == s).count()
    }
}

pub const XC_EMPTY: u32 = 1;

// ---------------------------------------------------------------------------
// 二、同源生成（元数据 → 三节）
// ---------------------------------------------------------------------------

/// 元数据正本。
pub const CFG_META: [&str; 2] = ["target-sm", "toolchain-prefer"];
pub const TOOL_META: [&str; 2] = ["dxc-SM6", "fxc-SM5"];
pub const OPT_META: [&str; 2] = ["indent-style", "attr-emit"];

/// 同源生成：任一元数据空即显性失败。
pub fn gen(cfg: &[&str], tool: &[&str], opt: &[&str]) -> Result<Vec<(Sec, String)>, u32> {
    if cfg.is_empty() || tool.is_empty() || opt.is_empty() {
        return Err(XC_META);
    }
    let mut out: Vec<(Sec, String)> = Vec::new();
    for c in cfg.iter() {
        out.push((Sec::Config, format!("配置项: {}", c)));
    }
    for t in tool.iter() {
        out.push((Sec::Tool, format!("工具链: {}", t)));
    }
    for o in opt.iter() {
        out.push((Sec::Opts, format!("选项: {}", o)));
    }
    Ok(out)
}

pub const XC_META: u32 = 2;

// ---------------------------------------------------------------------------
// 三、限制与怪癖合并（限先怪后）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Src {
    Limit,
    Quirk,
}

#[derive(Clone, Debug)]
pub struct Merged {
    pub src: Src,
    pub title: String,
}

/// 合并：限制在前、怪癖在后。
pub fn merge(limits: &[&str], quirks: &[&str]) -> Vec<Merged> {
    let mut out: Vec<Merged> = Vec::new();
    for l in limits.iter() {
        out.push(Merged { src: Src::Limit, title: l.to_string() });
    }
    for q in quirks.iter() {
        out.push(Merged { src: Src::Quirk, title: q.to_string() });
    }
    out
}

/// 不混排校验。
pub fn ordered(m: &[Merged]) -> bool {
    let mut seen_q = false;
    for x in m.iter() {
        if x.src == Src::Quirk {
            seen_q = true;
        } else if seen_q {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 四、版本联动
// ---------------------------------------------------------------------------

/// 文档版本 = 收口版 × 10 + 修订（修订封顶 9）。
pub fn doc_ver(release: u32, rev: u32) -> u32 {
    release * 10 + rev.min(9)
}

pub fn published(doc: u32, release: u32) -> bool {
    doc >= release * 10
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec137_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec137");

    // 族一：三节结构。
    let mut d = DocBook::new();
    s.add("空条目拒", matches!(d.add(Sec::Config, ""), Err(XC_EMPTY)), "内容必填");
    let g = match gen(&CFG_META, &TOOL_META, &OPT_META) {
        Ok(v) => v,
        Err(_) => { s.add("生成败", false, "不应到"); return s; }
    };
    for (sec, text) in g.iter() {
        let _ = d.add(*sec, text);
    }
    s.add("三节齐", d.complete(), "配置/工具/选项");
    s.add("三节各二件", d.count(Sec::Config) == 2 && d.count(Sec::Tool) == 2 && d.count(Sec::Opts) == 2, "元数据全量");

    // 族二：同源生成。
    s.add("元数据缺节拒", matches!(gen(&CFG_META, &[], &OPT_META), Err(XC_META)), "空元数据显性");
    s.add("同源可再现", gen(&CFG_META, &TOOL_META, &OPT_META).is_ok(), "元数据驱动");

    // 族三：怪癖合并。
    let m = merge(&["mesh-shader 限制"], &["dxc 2023 switch fallthrough"]);
    s.add("限先怪后", ordered(&m), "不混排");
    s.add("来源标签", m[0].src == Src::Limit && m[1].src == Src::Quirk, "双源合一");
    s.add("混排可见", !ordered(&[Merged { src: Src::Quirk, title: "q".to_string() }, Merged { src: Src::Limit, title: "l".to_string() }]), "乱序即违");

    // 族四：版本联动。
    s.add("版本合成", doc_ver(7, 3) == 73, "release×10+rev");
    s.add("修订封顶", doc_ver(7, 20) == 79, "rev ≤ 9");
    s.add("发布校验", published(73, 7), "随收口");
    s.add("旧版拦截", !published(69, 7), "未发布不入证据");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_minimal() {
        assert!(gen(&["a"], &["b"], &["c"]).is_ok());
        assert!(gen(&[], &["b"], &["c"]).is_err());
    }
}
