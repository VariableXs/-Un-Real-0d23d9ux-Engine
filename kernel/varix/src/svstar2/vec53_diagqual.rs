//! VE-F0453 · 语义错误报告质量（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0453`
//!
//! **锚点原文**：「语义错误报告质量工程：类型不匹配类错误附两侧类型推导路径、
//! 作用域类错误附作用域链可视化文本、重载歧义附候选明细（F0446 语义复用）；
//! 错误文案三要素化统一（现状、原因引用条款、修正建议带示例）；报告抽查制度。
//! 判据：推导路径、条款引用、三要素统一、抽查制、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **推导路径**（判据一）。类型不匹配报告附两侧推导路径（每步
//!    「来源 → 结论」），路径回溯 O(推导深度)——报告说的是「为什么推出
//!    这个类型」，不是只报「类型不匹配」。
//!
//! 2. **条款引用**（判据二）。原因一律引用规范条款（条款号随文案）；
//!    条款引用失效检测（引用表外的条款号 = 失效）随规范版本同步。
//!
//! 3. **三要素统一**（判据三）。所有文案经模板生成：现状（S）+ 原因
//!    引条款（R）+ 修正建议带示例（A）三要素齐备才可出报告——模板
//!    缺口显性登记（降级最小告知也要登记模板缺口）。
//!
//! 4. **抽查制**（判据四）。报告质量抽查账：抽样评审打分（0-100），
//!    低分（< 70）阻断发布（改写后重报）；抽样限频（同模板一次）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0453";

/// 推导步：来源 → 结论。
#[derive(Clone, Debug, PartialEq)]
pub struct InferStep {
    pub from: String,
    pub to: String,
}

/// 推导路径（两侧各一条）。
#[derive(Clone, Debug, Default)]
pub struct InferPath {
    pub lhs: Vec<InferStep>,
    pub rhs: Vec<InferStep>,
}

impl InferPath {
    /// 两侧路径可视化（为什么推出这个类型的报告面）。
    pub fn render(&self) -> String {
        let side = |name: &str, steps: &Vec<InferStep>| {
            let mut s = alloc::format!("{}:", name);
            for (i, st) in steps.iter().enumerate() {
                if i > 0 {
                    s.push_str(" -> ");
                }
                s.push_str(&alloc::format!("{}=>{}", st.from, st.to));
            }
            s
        };
        alloc::format!("{}; {}", side("lhs", &self.lhs), side("rhs", &self.rhs))
    }
}

/// 作用域链可视化文本（scope 类错误附品）。
pub fn render_scope_chain(chain: &[&str]) -> String {
    let mut s = String::from("作用域链:");
    for (i, n) in chain.iter().enumerate() {
        if i > 0 {
            s.push_str(" <- ");
        }
        s.push_str(n);
    }
    s
}

/// 报告三要素。
#[derive(Clone, Debug)]
pub struct Report3 {
    /// 现状。
    pub situation: String,
    /// 原因（引条款）。
    pub reason_clause: &'static str,
    /// 修正建议带示例。
    pub fix_example: String,
}

impl Report3 {
    /// 三要素齐备判定（出报告的闸）。
    pub fn complete(&self) -> bool {
        !self.situation.is_empty()
            && self.reason_clause.starts_with("F0402-§")
            && !self.fix_example.is_empty()
            && self.fix_example.contains("示例")
    }
    /// 渲染统一文案。
    pub fn render(&self) -> String {
        alloc::format!("现状：{}；原因：{}；修正：{}", self.situation, self.reason_clause, self.fix_example)
    }
}

/// 报告模板注册表：错误类 × 三要素模板（缺口显性登记）。
#[derive(Debug, Default)]
pub struct TemplateBook {
    /// 错误类名 → 模板是否在册（在册 = 三要素都有现成骨架）。
    pub registered: Vec<(&'static str, bool)>,
    /// 模板缺口（降级最小告知时登记）。
    pub gaps: Vec<&'static str>,
}

impl TemplateBook {
    pub fn new() -> Self {
        TemplateBook::default()
    }
    pub fn register(&mut self, class: &'static str) {
        if !self.registered.iter().any(|(c, _)| *c == class) {
            self.registered.push((class, true));
        }
    }
    /// 取模板：在册返回 Ok，否则登记缺口并 Err（降级最小告知）。
    pub fn take(&mut self, class: &'static str) -> Result<(), &'static str> {
        if self.registered.iter().any(|(c, on)| *c == class && *on) {
            Ok(())
        } else {
            if !self.gaps.contains(&class) {
                self.gaps.push(class);
            }
            Err("模板缺口：降级最小告知")
        }
    }
}

/// 抽查账：抽样评审分（0-100），低分阻断。
#[derive(Debug, Default)]
pub struct AuditBook {
    pub samples: Vec<(&'static str, u8)>,
    pub blocked: Vec<&'static str>,
}

/// 抽查通过线。
pub const AUDIT_PASS: u8 = 70;

impl AuditBook {
    pub fn sample(&mut self, template: &'static str, score: u8) {
        // 限频：同模板只抽一次。
        if !self.samples.iter().any(|(t, _)| *t == template) {
            self.samples.push((template, score));
            if score < AUDIT_PASS {
                self.blocked.push(template);
            }
        }
    }
    /// 发布闸：存在被阻断模板即不可发布。
    pub fn publishable(&self) -> bool {
        self.blocked.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec53_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、推导路径。
    {
        let mut p = InferPath::default();
        p.lhs.push(InferStep { from: "字面量 1".to_string(), to: "s:int".to_string() });
        p.lhs.push(InferStep { from: "s:int".to_string(), to: "s:float（等级提升）".to_string() });
        p.rhs.push(InferStep { from: "声明 v: bool".to_string(), to: "s:bool".to_string() });
        let text = p.render();
        v.push((
            "F0453-推导-两侧路径可视化",
            text.contains("lhs:") && text.contains("rhs:") && text.contains("=>") && text.contains("等级提升"),
        ));
        // 路径为空仍可渲染（降级形态不崩）。
        v.push(("F0453-推导-空路径可渲染", InferPath::default().render().contains("lhs:;")));
    }

    // 二、作用域链 + 条款引用。
    {
        let chain = render_scope_chain(&["fn-scope", "block-2", "block-1"]);
        v.push((
            "F0453-作用域-链可视化",
            chain.contains("fn-scope <- block-2 <- block-1"),
        ));
        // 三要素：原因引条款。
        let r = Report3 {
            situation: "v + w 类型不可组合".to_string(),
            reason_clause: "F0402-§5.4 运算符无可用类型组合",
            fix_example: "示例：改为 v + vec3(w)。".to_string(),
        };
        v.push((
            "F0453-条款-三要素齐备",
            r.complete() && r.render().contains("现状：") && r.render().contains("原因："),
        ));
        // 条款号格式错 → 不齐备（引用有牙）。
        let bad = Report3 {
            situation: "x".to_string(),
            reason_clause: "no-clause",
            fix_example: "示例：y".to_string(),
        };
        v.push(("F0453-条款-格式有牙", !bad.complete()));
    }

    // 三、模板缺口。
    {
        let mut book = TemplateBook::new();
        book.register("type-mismatch");
        book.register("scope-miss");
        book.register("overload-ambig");
        v.push((
            "F0453-模板-三类在册",
            book.registered.len() == 3,
        ));
        // 缺口模板：take 报错并登记缺口。
        let miss = book.take("nonsuch-class");
        v.push((
            "F0453-模板-缺口显性",
            miss.is_err() && book.gaps.contains(&"nonsuch-class"),
        ));
        // 在册模板取用成功。
        v.push(("F0453-模板-在册取用", book.take("type-mismatch").is_ok()));
    }

    // 四、抽查制。
    {
        let mut audit = AuditBook::default();
        audit.sample("type-mismatch", 85);
        audit.sample("scope-miss", 55);
        audit.sample("overload-ambig", 40);
        // 低分（<70）阻断发布。
        v.push((
            "F0453-抽查-低分阻断",
            !audit.publishable() && audit.blocked.len() == 2,
        ));
        // 限频：同模板二次抽样不改记录。
        audit.sample("type-mismatch", 10);
        v.push((
            "F0453-抽查-限频一次",
            audit.samples.len() == 3 && audit.samples[0].1 == 85,
        ));
        // 全过线可发布。
        let mut good = AuditBook::default();
        good.sample("t", 70);
        good.sample("s", 90);
        v.push(("F0453-抽查-过线可发布", good.publishable()));
    }

    // 五、判据自检。
    {
        v.push(("F0453-判据-锚点入册", ANCHOR == "VE-F0453"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec53）。
pub fn run_vec53_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec53");
    for (name, ok) in vec53_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec53_raw_checks as run_vec53_checks;
    #[test]
    fn vec53_diagqual_checks() {
        for (name, ok) in run_vec53_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
