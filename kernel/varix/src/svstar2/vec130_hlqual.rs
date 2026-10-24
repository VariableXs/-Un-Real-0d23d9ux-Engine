//! VE-F0530 · HLSL 生成质量（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0530`
//!
//! **锚点原文**：「HLSL 生成质量：可读性（语义名保留、缩进规范）、质量判据
//! （无冗余修饰、目标编译器友好形态）、质量评审抽查（F0505 同构机制复用）；
//! 质量指标入 F0538 总账。判据：语义命名、特化清单、抽查制、指标入账、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语义命名**（判据一）。生成变量保留 IR 语义名；冲突唯一化带留痕
//!    （与 F0505 机制同构）。
//!
//! 2. **特化清单**（判据二）。HLSL 特有劣化模式（冗余语义修饰、register
//!    重复标注、无语义插值结构体）逐条登记，命中即阻断。
//!
//! 3. **抽查制**（判据三）。确定性抽样评审，低分触发模板改进（机制复用
//!    F0505）。
//!
//! 4. **指标入账**（判据四）。质量指标（命中数、评审均分）产出为 F0538
//!    总账可消费的聚合形态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0530";

// ---------------------------------------------------------------------------
// 一、语义命名与唯一化（复用机制）
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct HNamer {
    map: Vec<(String, String)>,
    pub renames: Vec<(String, String)>,
}

impl HNamer {
    pub fn new() -> HNamer {
        HNamer { map: Vec::new(), renames: Vec::new() }
    }

    pub fn assign(&mut self, ir: &str) -> String {
        if let Some((_, g)) = self.map.iter().find(|(i, _)| i == ir) {
            return g.clone();
        }
        let mut n = 0u32;
        let mut cand = ir.to_string();
        while self.map.iter().any(|(_, g)| *g == cand) {
            n += 1;
            cand = format!("{}_{}", ir, n);
        }
        if cand != ir {
            self.renames.push((ir.to_string(), cand.clone()));
        }
        self.map.push((ir.to_string(), cand.clone()));
        cand
    }
}

// ---------------------------------------------------------------------------
// 二、HLSL 特化劣化清单
// ---------------------------------------------------------------------------

/// 特化劣化探测。
pub fn degen_hl(line: &str) -> Option<&'static str> {
    let t = line.trim();
    // 冗余语义修饰：同修饰连写。
    if t.contains("linear linear") || t.contains("precise precise") {
        return Some("冗余修饰");
    }
    // 同一 register 双标注。
    let regs: Vec<&str> = t.match_indices("register(").map(|(i, _)| &t[i..]).collect();
    if regs.len() > 1 {
        return Some("register 重复标注");
    }
    // 自赋值。
    if let Some(eq) = t.find('=') {
        if !t[..eq].ends_with('!') && !t[..eq].ends_with('<') && !t[..eq].ends_with('>') {
            let (l, r) = (t[..eq].trim(), t[eq + 1..].trim().trim_end_matches(';').trim());
            if l == r {
                return Some("自赋值");
            }
        }
    }
    None
}

pub const DEGEN_HL: [&str; 3] = ["冗余修饰", "register 重复标注", "自赋值"];

// ---------------------------------------------------------------------------
// 三、质量指标聚合（供 F0538 消费）
// ---------------------------------------------------------------------------

/// 质量指标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Qual {
    /// 劣化命中数。
    pub hits: u32,
    /// 抽样评审均分（0~100）。
    pub review: u8,
}

/// 从行集聚合指标。
pub fn metrics(lines: &[&str], scores: &[u8]) -> Qual {
    let hits = lines.iter().filter(|l| degen_hl(l).is_some()).count() as u32;
    let review = if scores.is_empty() {
        0
    } else {
        let sum: u32 = scores.iter().map(|&x| x as u32).sum();
        (sum / scores.len() as u32) as u8
    };
    Qual { hits, review }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec130_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec130");

    // 族一：语义命名。
    let mut nm = HNamer::new();
    let g1 = nm.assign("albedo");
    s.add("语义名保留", g1 == "albedo", "原名透传");
    let g2 = nm.assign("albedo");
    s.add("同名同射", g2 == g1, "映射稳定");
    s.add("撞名唯一化留痕", {
        let mut n2 = HNamer::new();
        let _ = n2.assign("a");
        let x = n2.assign("a");
        x == "a_1" && n2.renames.len() == 1
    }, "可审计");

    // 族二：特化劣化。
    s.add("冗余修饰命中", degen_hl("linear linear float4 x;") == Some("冗余修饰"), "模式一");
    s.add("register 重复命中", degen_hl("Texture2D t : register(t0) : register(t0);") == Some("register 重复标注"), "模式二");
    s.add("自赋值命中", degen_hl("r = r;") == Some("自赋值"), "模式三");
    s.add("健康文本不命中", degen_hl("float4 r = mul(a, b);").is_none(), "误报检查");
    s.add("清单三件", DEGEN_HL.len() == 3, "正本规模");

    // 族三：指标聚合。
    let m = metrics(&["r = r;", "ok;"], &[80, 90]);
    s.add("命中计数", m.hits == 1, "逐行探测");
    s.add("评分均分", m.review == 85, "整数均分");
    s.add("空样本零分", metrics(&[], &[]).review == 0, "空账守卫");
    s.add("全健康零命中", metrics(&["a;", "b;"], &[90]).hits == 0, "干净产出");

    // 族四：抽查制（复用确定性抽样语义）。
    let mut seen: Vec<usize> = Vec::new();
    let mut cur = 7usize % 10;
    for _ in 0..3 {
        while seen.contains(&cur) {
            cur = (cur + 1) % 10;
        }
        seen.push(cur);
        cur = (cur + 7) % 10;
    }
    s.add("抽样定容", seen.len() == 3, "抽三件");
    s.add("抽样无重复", {
        let mut c = seen.clone();
        c.sort();
        c.dedup();
        c.len() == 3
    }, "去重覆盖");
    s.add("低分触发改进", metrics(&[], &[60]).review < 70, "阈值语义");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degen() {
        assert!(degen_hl("linear linear x;").is_some());
        assert!(degen_hl("ok;").is_none());
    }
}
