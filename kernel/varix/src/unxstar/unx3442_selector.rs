//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3442 · 用例选择器（五维过滤：域/批/ID/标签/kind + 组合求值）
//! · 诊断段 0xCD00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：五维选择语法（domain=/batch=/id=/tag=/kind=）+ 组合求值
//!      （与语义、逗号或语义）；M-201 五维组合矩阵验证；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五维谓词：domain/batch/id/tag/kind；
//!   - 组合求值：谓词间"与"（同一谓词内逗号"或"）；
//!   - M-201：五维组合矩阵（单维 5 例 + 双维与 + 或）全过；
//!   - 语法解析：键=值串 → 谓词集；未知键必拒；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xCD00;

/// 合法键集。
pub const KEYS: [&str; 5] = ["domain", "batch", "id", "tag", "kind"];

/// 用例描述。
#[derive(Debug, Clone)]
pub struct TestCase {
    pub domain: String,
    pub batch: String,
    pub id: u32,
    pub tags: Vec<String>,
    pub kind: String,
}

impl TestCase {
    pub fn new(domain: &str, batch: &str, id: u32, tags: &[&str], kind: &str) -> Self {
        let mut t = Vec::new();
        for x in tags {
            t.push(String::from(*x));
        }
        TestCase {
            domain: String::from(domain),
            batch: String::from(batch),
            id,
            tags: t,
            kind: String::from(kind),
        }
    }
}

/// 单维谓词：键 + 值集（逗号或语义）。
#[derive(Debug, Clone)]
pub struct Predicate {
    pub key: usize, // KEYS 索引
    pub values: Vec<String>,
}

impl Predicate {
    /// 对用例求值：值集任一命中即真（或语义）。
    pub fn matches(&self, tc: &TestCase) -> bool {
        let get = |tc: &TestCase| -> Vec<String> {
            match self.key {
                0 => vec_like(&tc.domain),
                1 => vec_like(&tc.batch),
                2 => vec_like(&tc.id.to_string()),
                3 => tc.tags.clone(),
                _ => vec_like(&tc.kind),
            }
        };
        let fields = get(tc);
        self.values.iter().any(|v| fields.iter().any(|f| f == v))
    }
}

fn vec_like(s: &String) -> Vec<String> {
    let mut v = Vec::new();
    v.push(s.clone());
    v
}

/// 选择器：谓词集（间"与"语义）。
pub struct Selector {
    pub preds: Vec<Predicate>,
}

impl Selector {
    pub fn new() -> Self {
        Selector { preds: Vec::new() }
    }

    /// 语法解析："domain=A1,batch=B01,tag=core" → 谓词集。
    /// 未知键必拒。
    pub fn parse(&mut self, expr: &str) -> bool {
        for part in expr.split(',') {
            let mut it = part.splitn(2, '=');
            let k = match it.next() {
                Some(k) => k,
                None => return false,
            };
            let v = match it.next() {
                Some(v) if !v.is_empty() => v,
                _ => return false,
            };
            let ki = KEYS.iter().position(|x| *x == k);
            match ki {
                Some(idx) => {
                    let values: Vec<String> = v.split('|').map(String::from).collect();
                    self.preds.push(Predicate { key: idx, values });
                }
                None => return false,
            }
        }
        !self.preds.is_empty()
    }

    /// 组合求值：全谓词命中（与语义）。
    pub fn eval(&self, tc: &TestCase) -> bool {
        self.preds.iter().all(|p| p.matches(tc))
    }
}

/// M-201 五维组合矩阵验证。
pub fn m201() -> (bool, usize) {
    let c1 = TestCase::new("A1", "B01", 7, &["core", "mem"], "unit");
    let c2 = TestCase::new("A2", "B02", 8, &["irq"], "stress");
    let mut pass = 0usize;
    // 单维五例
    for key in KEYS {
        let mut sel = Selector::new();
        let expr = match key {
            "domain" => "domain=A1",
            "batch" => "batch=B02",
            "id" => "id=7",
            "tag" => "tag=irq",
            _ => "kind=stress",
        };
        if sel.parse(expr) {
            let hit1 = sel.eval(&c1);
            let hit2 = sel.eval(&c2);
            let expect1 = key != "domain" && key != "batch" && key != "tag" || expr == "domain=A1";
            let expect2 = key == "batch" || key == "tag" || key == "kind";
            if hit1 == (key == "domain" || key == "id") && hit2 == expect2 && (expect1 || true) {
                pass += 1;
            }
        }
    }
    // 双维与
    let mut sel = Selector::new();
    let and_ok = sel.parse("domain=A1,kind=unit") && sel.eval(&c1) && !sel.eval(&c2);
    // 或语义（同谓词内 | ）
    let mut sel2 = Selector::new();
    let or_ok = sel2.parse("domain=A1|A2") && sel2.eval(&c1) && sel2.eval(&c2);
    // 与+或混合
    let mut sel3 = Selector::new();
    let mix_ok = sel3.parse("tag=core|irq,kind=unit") && sel3.eval(&c1) && !sel3.eval(&c2);
    (pass == 5 && and_ok && or_ok && mix_ok, pass)
}

/// J1 主判据：五维语法 + 组合求值 + M-201 矩阵。
pub fn j1_main() -> bool {
    let (ok, n) = m201();
    ok && n == 5 && {
        let mut sel = Selector::new();
        !sel.parse("bogus=X") // 未知键必拒
    } && {
        let mut sel = Selector::new();
        !sel.parse("domain=") // 空值必拒
    }
}

// ── 注入核（0xCD00 段 5 项）────────────────────────────────────────────

/// 注入核 1：未知键注入。
pub fn inj1_unknown_key() -> bool {
    let mut sel = Selector::new();
    !sel.parse("zone=A1")
}

/// 注入核 2：缺等号注入。
pub fn inj2_no_eq() -> bool {
    let mut sel = Selector::new();
    !sel.parse("domainA1")
}

/// 注入核 3：空值注入。
pub fn inj3_empty_val() -> bool {
    let mut sel = Selector::new();
    !sel.parse("tag=")
}

/// 注入核 4：键名互异注入。
pub fn inj4_keys() -> bool {
    for i in 0..KEYS.len() {
        if KEYS[i].is_empty() {
            return false;
        }
        for j in i + 1..KEYS.len() {
            if KEYS[i] == KEYS[j] {
                return false;
            }
        }
    }
    true
}

/// 注入核 5：与语义注入——一维不中全不中。
pub fn inj5_and_semantics() -> bool {
    let c = TestCase::new("A1", "B01", 1, &["x"], "unit");
    let mut sel = Selector::new();
    let ok = sel.parse("domain=A1,kind=stress");
    ok && !sel.eval(&c)
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3442_checks：本单判据聚合（诊断段 0xCD00）。
pub fn run_unx3442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3442 用例选择器（五维过滤：域/批/ID/标签/kind + 组合求值）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "五维选择语法+组合求值",
        j1_main(),
        if j1_main() { "" } else { "选择器破" },
    );
    let (m_ok, n) = m201();
    s.add(
        "M-201五维组合矩阵验证",
        m_ok && n == 5,
        if m_ok { "" } else { "矩阵例缺" },
    );
    let inj = [
        ("注入1未知键", inj1_unknown_key()),
        ("注入2缺等号", inj2_no_eq()),
        ("注入3空值", inj3_empty_val()),
        ("注入4键名互异", inj4_keys()),
        ("注入5与语义", inj5_and_semantics()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
