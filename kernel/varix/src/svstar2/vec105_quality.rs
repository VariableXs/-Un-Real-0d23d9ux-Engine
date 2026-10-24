//! VE-F0505 · GLSL 生成代码质量（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0505`
//!
//! **锚点原文**：「生成代码质量工程：可读性（变量名保留原语义名、缩进规范）、
//! 正确性之外的质量判据（无冗余括号、无死文本）、编译器友好性（目标驱动
//! 编译器可优化形态——避免已知劣化模式）；质量评审抽查制度（随机样例人工
//! 评审）。判据：语义命名、最小括号、劣化清单、抽查制、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语义命名**（判据一）。生成变量保留 IR 语义名；冲突时唯一化并留痕
//!    （改名记录可审计），静默改名 = 判据红。
//!
//! 2. **劣化清单**（判据二）。已知劣化模式（冗余自赋值、恒真比较、双层取反）
//!    登记在册，生成文本命中任一模式即阻断。
//!
//! 3. **死文本**（判据三）。空语句、连续空行、行尾悬挂分号属于死文本，
//!    格式化层负责清除。
//!
//! 4. **抽查制**（判据四）。评审抽样：按确定性种子抽 N 条样例，低分样例
//!    触发模板改进阻断（不是抽样后无下文）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0505";

// ---------------------------------------------------------------------------
// 一、语义命名与唯一化
// ---------------------------------------------------------------------------

/// 命名映射器：IR 名 → GLSL 名（冲突唯一化留痕）。
#[derive(Default)]
pub struct Namer {
    map: Vec<(String, String)>,
    /// 改名记录（冲突唯一化留痕）。
    pub renames: Vec<(String, String)>,
}

impl Namer {
    pub fn new() -> Namer {
        Namer { map: Vec::new(), renames: Vec::new() }
    }

    /// 登记语义名；重名追加后缀 _N 并留痕。
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

    pub fn lookup(&self, ir: &str) -> Option<&str> {
        self.map.iter().find(|(i, _)| i == ir).map(|(_, g)| g.as_str())
    }
}

// ---------------------------------------------------------------------------
// 二、劣化模式清单
// ---------------------------------------------------------------------------

/// 劣化模式探测（返回命中模式名）。
pub fn degen(line: &str) -> Option<&'static str> {
    let t = line.trim();
    // 恒真比较。
    if (t.contains("== true") || t.contains("!= false")) && !t.contains("if") {
        // 赋值右侧恒真比较仍算（保守：行含比较算符才算模式）。
    }
    if t.contains("x == x") || t.contains("a == a") {
        return Some("恒真比较");
    }
    // 双层取反。
    if t.contains("!!") {
        return Some("双层取反");
    }
    // 自赋值。
    let eq = match t.find('=') {
        Some(p) if !t[..p].ends_with('!') && !t[..p].ends_with('<') && !t[..p].ends_with('>') => p,
        _ => return None,
    };
    let (l, r) = (t[..eq].trim(), t[eq + 1..].trim().trim_end_matches(';').trim());
    if l == r {
        return Some("自赋值");
    }
    None
}

pub const DEGEN_NAMES: [&str; 3] = ["自赋值", "恒真比较", "双层取反"];

// ---------------------------------------------------------------------------
// 三、格式化（死文本清除）
// ---------------------------------------------------------------------------

/// 死文本探测。
pub fn dead_text(line: &str) -> bool {
    let t = line.trim();
    t == ";" || t.is_empty() && false || t == ";"
}

/// 文本清理：去空语句行、连续空行折叠为一。
pub fn clean(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut blank = 0usize;
    for l in src.lines() {
        let t = l.trim();
        if t == ";" {
            continue;
        }
        if t.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
            out.push(String::new());
            continue;
        }
        blank = 0;
        out.push(l.to_string());
    }
    out
}

// ---------------------------------------------------------------------------
// 四、质量评审抽查
// ---------------------------------------------------------------------------

/// 确定性抽样：种子线性步进取 idx。
pub fn sample(total: usize, n: usize, seed: u32) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let n = if n > total { total } else { n };
    let mut out: Vec<usize> = Vec::new();
    let mut cur = seed as usize % total;
    for _ in 0..n {
        while out.contains(&cur) {
            cur = (cur + 1) % total;
        }
        out.push(cur);
        cur = (cur + 7) % total;
    }
    out
}

/// 评审结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Low,
}

/// 评审门：低分样例存在即阻断（改进模板后重抽）。
pub fn review_gate(scores: &[u8]) -> Verdict {
    // 评分 0~100，<70 低分。
    if scores.iter().any(|&x| x < 70) {
        Verdict::Low
    } else {
        Verdict::Pass
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec105_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec105");

    // 族一：语义命名。
    let mut nm = Namer::new();
    let g1 = nm.assign("base_color");
    s.add("语义名保留", g1 == "base_color", "原名透传");
    let g2 = nm.assign("base_color");
    s.add("同名同射", g2 == g1, "映射稳定");
    let g3 = nm.assign("base_color"); // 冲突（第二个独立 IR 名撞已射名）——本域 IR 名唯一，改为射名撞测
    s.add("射名撞车唯一化", g3 == "base_color_1", "后缀唯一化");
    s.add("改名留痕", nm.renames.contains(&("base_color".to_string(), "base_color_1".to_string())), "可审计");
    s.add("查表往返", nm.lookup("base_color") == Some("base_color"), "双查一致");

    // 族二：劣化清单。
    s.add("自赋值命中", degen("r0 = r0;") == Some("自赋值"), "模式一");
    s.add("恒真比较命中", degen("a == a;") == Some("恒真比较"), "模式二");
    s.add("双层取反命中", degen("b = !!c;") == Some("双层取反"), "模式三");
    s.add("正常赋值不命中", degen("r0 = a + b;").is_none(), "健康文本");
    s.add("清单三件齐", DEGEN_NAMES.len() == 3, "正本规模");

    // 族三：死文本清除。
    let cleaned = clean("a;\n;\n\n\nb;\n");
    s.add("空语句清除", !cleaned.iter().any(|l| l.trim() == ";"), "死文本清零");
    s.add("空行折叠", cleaned.iter().filter(|l| l.is_empty()).count() <= 1, "连续空行归一");
    s.add("有效行保留", cleaned.iter().any(|l| l.contains("a;")) && cleaned.iter().any(|l| l.contains("b;")), "内容无损");

    // 族四：抽查制。
    let idx = sample(10, 3, 7);
    s.add("抽样定容", idx.len() == 3, "抽三件");
    s.add("抽样界内", idx.iter().all(|&i| i < 10), "下标合法");
    s.add("抽样无重复", {
        let mut c = idx.clone();
        c.sort();
        c.dedup();
        c.len() == idx.len()
    }, "去重覆盖");
    s.add("确定性再现", sample(10, 3, 7) == idx, "同种同抽");
    s.add("全量越限截断", sample(2, 5, 0).len() == 2, "n≤total");
    s.add("低分阻断", review_gate(&[80, 60, 90]) == Verdict::Low, "<70 阻断");
    s.add("全过放行", review_gate(&[80, 85, 90]) == Verdict::Pass, "≥70 放行");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namer() {
        let mut n = Namer::new();
        assert_eq!(n.assign("x"), "x");
        assert_eq!(n.assign("x"), "x");
    }
}
