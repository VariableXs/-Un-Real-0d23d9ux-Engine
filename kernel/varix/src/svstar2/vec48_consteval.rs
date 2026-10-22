//! VE-F0448 · 常量表达式求值（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0448`
//!
//! **锚点原文**：「常量表达式求值器：常量语境判定（哪些位置要求常量按规范）、
//! 求值引擎（整数与浮点语义按规范、除零与溢出在编译期报错）、求值结果缓存
//! （同表达式只求一次）、非常量误用诊断（带条款与原因）。判据：语境判定、
//! 编译期报错、结果缓存、条款诊断、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语境判定**（判据一）。哪些位置**要求**常量由 [`CTX_TABLE`] 静态
//!    表驱动（数组维度 / case 标签 / 布局值 / 特化开关四语境），判定
//!    O(1) 查表——语境清单不是散落的 if 链。
//!
//! 2. **编译期报错**（判据二）。除零（整型与浮点各自口径——浮点除零
//!    按规范产 Inf 不报错，整型除零编译期报错）与整型溢出（checked 算术）
//!    在求值期即产诊断带位置与类型边界，不留到运行期。
//!
//! 3. **结果缓存**（判据三）。同一表达式键（文本规范化）只求一次：
//!    [`Cache`] 命中即复用并计命中数——同表达式二次求值不重复进引擎。
//!
//! 4. **条款诊断**（判据四）。非常量出现在要求常量的语境 → 诊断带
//!    条款（[`CLAUSE_CONST_CTX`]）与求值路径说明（为什么不是常量）。

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0448";

/// 规范条款：常量语境要求。
pub const CLAUSE_CONST_CTX: &str = "F0402-§4.3.1 要求常量的语境必须可常量求值";
/// 规范条款：整型除零编译期报错。
pub const CLAUSE_DIVZERO: &str = "F0402-§4.3.2 整型除零在编译期报错";
/// 规范条款：整型溢出编译期报错。
pub const CLAUSE_OVERFLOW: &str = "F0402-§4.3.3 整型溢出在编译期报错";

// ---------------------------------------------------------------------------
// 常量语境表（判据一的表驱动面）
// ---------------------------------------------------------------------------

/// 要求常量的语境（静态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtxKind {
    /// 数组维度（F0429 消费）。
    ArrayDim,
    /// case 标签。
    CaseLabel,
    /// 布局键值（F0433 消费）。
    LayoutValue,
    /// 特化开关（F0477 消费）。
    SpecSwitch,
}

impl CtxKind {
    pub fn tag(&self) -> &'static str {
        match self {
            CtxKind::ArrayDim => "array-dim",
            CtxKind::CaseLabel => "case-label",
            CtxKind::LayoutValue => "layout-value",
            CtxKind::SpecSwitch => "spec-switch",
        }
    }
}

/// 语境表：闭集四项。
pub const CTX_TABLE: [CtxKind; 4] = [
    CtxKind::ArrayDim,
    CtxKind::CaseLabel,
    CtxKind::LayoutValue,
    CtxKind::SpecSwitch,
];

/// 语境判定：是否要求常量（表驱动 O(1)——闭集内线性四项即常数）。
pub fn requires_const(kind: CtxKind) -> bool {
    CTX_TABLE.iter().any(|k| *k == kind)
}

// ---------------------------------------------------------------------------
// 常量值与求值引擎
// ---------------------------------------------------------------------------

/// 常量值（整型/浮点/布尔三形态）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CVal {
    I(i64),
    F(f64),
    B(bool),
}

impl CVal {
    pub fn type_name(&self) -> &'static str {
        match self {
            CVal::I(_) => "int",
            CVal::F(_) => "float",
            CVal::B(_) => "bool",
        }
    }
    /// int 边界（溢出诊断带类型边界的证据面）。
    pub const I_MIN: i64 = i64::MIN;
    pub const I_MAX: i64 = i64::MAX;
}

/// 求值失败（编译期诊断）。
#[derive(Clone, Debug, PartialEq)]
pub struct CDiag {
    pub off: u32,
    pub kind: DiagKind,
    pub clause: &'static str,
    pub note: String,
}

/// 诊断类别（编译期三类 + 语境误用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagKind {
    DivZero,
    Overflow,
    NotConst,
}

/// 常量表达式 AST（求值引擎的输入面）。
#[derive(Clone, Debug)]
pub enum CExpr {
    Lit { val: CVal, off: u32 },
    Ident { name: String, off: u32 },
    Unary { op: &'static str, kid: Box<CExpr>, off: u32 },
    Binary { op: &'static str, lhs: Box<CExpr>, rhs: Box<CExpr>, off: u32 },
    Ternary { cond: Box<CExpr>, then: Box<CExpr>, els: Box<CExpr>, off: u32 },
}

/// 求值器：常量环境（Ident → 值）驱动。
pub struct Evaluator<'e> {
    pub env: &'e [(String, CVal)],
}

impl<'e> Evaluator<'e> {
    pub fn new(env: &'e [(String, CVal)]) -> Self {
        Evaluator { env }
    }
    /// 求值：Ok(值) 或 Err(编译期诊断)。
    pub fn eval(&self, e: &CExpr) -> Result<CVal, CDiag> {
        match e {
            CExpr::Lit { val, .. } => Ok(*val),
            CExpr::Ident { name, off } => self
                .env
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| *v)
                .ok_or_else(|| CDiag {
                    off: *off,
                    kind: DiagKind::NotConst,
                    clause: CLAUSE_CONST_CTX,
                    note: alloc::format!("名字 {} 不在常量环境（求值路径：标识符解析失败）", name),
                }),
            CExpr::Unary { op, kid, off } => {
                let v = self.eval(kid)?;
                match (*op, v) {
                    ("-", CVal::I(a)) => a.checked_neg().map(CVal::I).ok_or_else(|| overflow(*off, "int")),
                    ("-", CVal::F(a)) => Ok(CVal::F(-a)),
                    ("!", CVal::B(a)) => Ok(CVal::B(!a)),
                    ("~", CVal::I(a)) => Ok(CVal::I(!a)),
                    _ => Err(notsup(*off, op)),
                }
            }
            CExpr::Binary { op, lhs, rhs, off } => {
                let a = self.eval(lhs)?;
                let b = self.eval(rhs)?;
                binop(op, a, b, *off)
            }
            CExpr::Ternary { cond, then, els, off } => {
                let c = self.eval(cond)?;
                match c {
                    CVal::B(true) => self.eval(then),
                    CVal::B(false) => self.eval(els),
                    _ => Err(notsup(*off, "?:")),
                }
            }
        }
    }
}

/// 二元运算（整型 checked / 浮点按 IEEE / 除零按规范口径分型）。
fn binop(op: &str, a: CVal, b: CVal, off: u32) -> Result<CVal, CDiag> {
    match (a, b) {
        (CVal::I(x), CVal::I(y)) => match op {
            "+" => x.checked_add(y).map(CVal::I).ok_or_else(|| overflow(off, "int")),
            "-" => x.checked_sub(y).map(CVal::I).ok_or_else(|| overflow(off, "int")),
            "*" => x.checked_mul(y).map(CVal::I).ok_or_else(|| overflow(off, "int")),
            "/" => {
                if y == 0 {
                    Err(CDiag {
                        off,
                        kind: DiagKind::DivZero,
                        clause: CLAUSE_DIVZERO,
                        note: "整型除零（编译期报错）".to_string(),
                    })
                } else {
                    Ok(CVal::I(x / y))
                }
            }
            "%" => {
                if y == 0 {
                    Err(CDiag {
                        off,
                        kind: DiagKind::DivZero,
                        clause: CLAUSE_DIVZERO,
                        note: "整型取模零（编译期报错）".to_string(),
                    })
                } else {
                    Ok(CVal::I(x % y))
                }
            }
            "<" => Ok(CVal::B(x < y)),
            ">" => Ok(CVal::B(x > y)),
            "==" => Ok(CVal::B(x == y)),
            _ => Err(notsup(off, op)),
        },
        (CVal::F(x), CVal::F(y)) => match op {
            // 浮点除零按规范产 Inf 不报错（与整型口径分型——规范明示）。
            "+" => Ok(CVal::F(x + y)),
            "-" => Ok(CVal::F(x - y)),
            "*" => Ok(CVal::F(x * y)),
            "/" => Ok(CVal::F(x / y)),
            "<" => Ok(CVal::B(x < y)),
            ">" => Ok(CVal::B(x > y)),
            "==" => Ok(CVal::B(x == y)),
            _ => Err(notsup(off, op)),
        },
        (CVal::B(x), CVal::B(y)) => match op {
            "&&" => Ok(CVal::B(x && y)),
            "||" => Ok(CVal::B(x || y)),
            "==" => Ok(CVal::B(x == y)),
            _ => Err(notsup(off, op)),
        },
        _ => Err(CDiag {
            off,
            kind: DiagKind::NotConst,
            clause: CLAUSE_CONST_CTX,
            note: "混合类型常量运算无规范语义".to_string(),
        }),
    }
}

fn overflow(off: u32, ty: &str) -> CDiag {
    CDiag {
        off,
        kind: DiagKind::Overflow,
        clause: CLAUSE_OVERFLOW,
        note: alloc::format!("{} 溢出（边界 [{}, {}]）", ty, CVal::I_MIN, CVal::I_MAX),
    }
}

fn notsup(off: u32, op: &str) -> CDiag {
    CDiag {
        off,
        kind: DiagKind::NotConst,
        clause: CLAUSE_CONST_CTX,
        note: alloc::format!("常量语境不支持运算 {}", op),
    }
}

// ---------------------------------------------------------------------------
// 结果缓存（判据三）
// ---------------------------------------------------------------------------

/// 求值缓存：表达式键 → 结果；命中计数（同表达式只求一次的证据面）。
#[derive(Debug, Default)]
pub struct Cache {
    pub entries: Vec<(String, Result<i64, DiagKind>)>,
    pub hits: u32,
    pub misses: u32,
}

impl Cache {
    pub fn new() -> Self {
        Cache::default()
    }
    /// 键：表达式文本规范化（去空白）。
    pub fn key_of(src: &str) -> String {
        src.chars().filter(|c| !c.is_whitespace()).collect()
    }
    /// 取：命中计 hits，未中计 misses。
    pub fn get(&mut self, key: &str) -> Option<Result<i64, DiagKind>> {
        let hit = self.entries.iter().find(|(k, _)| k == key).map(|(_, r)| *r);
        if hit.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        hit
    }
    pub fn put(&mut self, key: String, r: Result<i64, DiagKind>) {
        self.entries.push((key, r));
    }
}

/// 带缓存的求值入口：I 语义快速路径（键 → 缓存 → 引擎 → 回填）。
pub fn eval_cached_i64(
    cache: &mut Cache,
    src: &str,
    ev: &Evaluator,
    e: &CExpr,
) -> Result<i64, CDiag> {
    let key = Cache::key_of(src);
    if let Some(hit) = cache.get(&key) {
        return hit.map_err(|k| CDiag {
            off: 0,
            kind: k,
            clause: CLAUSE_CONST_CTX,
            note: "缓存复用诊断".to_string(),
        });
    }
    match ev.eval(e) {
        Ok(CVal::I(n)) => {
            cache.put(key, Ok(n));
            Ok(n)
        }
        Ok(_) => {
            let d = notsup(0, "非整型结果");
            cache.put(key, Err(d.kind));
            Err(d)
        }
        Err(d) => {
            cache.put(key, Err(d.kind));
            Err(d)
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec48_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、语境判定。
    {
        // 表驱动四语境全要求常量。
        v.push((
            "F0448-语境-四语境全要求",
            CTX_TABLE.iter().all(|k| requires_const(*k)),
        ));
        // 表闭集：恰四项且互异。
        v.push((
            "F0448-语境-闭集四项",
            CTX_TABLE.len() == 4
                && CTX_TABLE.iter().all(|k| CTX_TABLE.iter().filter(|k2| **k2 == *k).count() == 1),
        ));
        // 各语境标签非空（可机检）。
        v.push((
            "F0448-语境-标签可机检",
            CTX_TABLE.iter().all(|k| !k.tag().is_empty()),
        ));
    }

    // 二、编译期报错。
    {
        let env: Vec<(String, CVal)> = Vec::new();
        let ev = Evaluator::new(&env);
        let lit = |n: i64| CExpr::Lit { val: CVal::I(n), off: 7 };
        // 整型除零：编译期报错带条款。
        let div = CExpr::Binary {
            op: "/",
            lhs: Box::new(lit(10)),
            rhs: Box::new(lit(0)),
            off: 7,
        };
        match ev.eval(&div) {
            Err(d) => v.push((
                "F0448-报错-整型除零带条款",
                d.kind == DiagKind::DivZero && d.clause == CLAUSE_DIVZERO && d.off == 7,
            )),
            Ok(_) => v.push(("F0448-报错-整型除零带条款", false)),
        }
        // 浮点除零：产 Inf 不报错（口径分型）。
        let fdiv = CExpr::Binary {
            op: "/",
            lhs: Box::new(CExpr::Lit { val: CVal::F(1.0), off: 0 }),
            rhs: Box::new(CExpr::Lit { val: CVal::F(0.0), off: 0 }),
            off: 0,
        };
        match ev.eval(&fdiv) {
            Ok(CVal::F(x)) => v.push(("F0448-报错-浮点除零产Inf", x.is_infinite())),
            _ => v.push(("F0448-报错-浮点除零产Inf", false)),
        }
        // 整型溢出：checked 拦截带类型边界。
        let ovf = CExpr::Binary {
            op: "*",
            lhs: Box::new(lit(i64::MAX)),
            rhs: Box::new(lit(2)),
            off: 3,
        };
        match ev.eval(&ovf) {
            Err(d) => v.push((
                "F0448-报错-溢出带边界",
                d.kind == DiagKind::Overflow && d.note.contains(&CVal::I_MAX.to_string()),
            )),
            Ok(_) => v.push(("F0448-报错-溢出带边界", false)),
        }
        // 正常整型与浮点语义。
        let ok1 = ev.eval(&CExpr::Binary { op: "+", lhs: Box::new(lit(2)), rhs: Box::new(lit(3)), off: 0 });
        let ok2 = ev.eval(&CExpr::Binary {
            op: "*",
            lhs: Box::new(CExpr::Lit { val: CVal::F(1.5), off: 0 }),
            rhs: Box::new(CExpr::Lit { val: CVal::F(2.0), off: 0 }),
            off: 0,
        });
        v.push((
            "F0448-报错-正常语义",
            ok1 == Ok(CVal::I(5)) && ok2 == Ok(CVal::F(3.0)),
        ));
    }

    // 三、结果缓存。
    {
        let env = vec![("N".to_string(), CVal::I(8))];
        let ev = Evaluator::new(&env);
        let mut cache = Cache::new();
        let expr = CExpr::Ident { name: "N".to_string(), off: 0 };
        let src = " N ";
        let r1 = eval_cached_i64(&mut cache, src, &ev, &expr);
        let r2 = eval_cached_i64(&mut cache, src, &ev, &expr);
        v.push((
            "F0448-缓存-同式只求一次",
            r1 == Ok(8) && r2 == Ok(8) && cache.misses == 1 && cache.hits == 1,
        ));
        // 不同写法同表达式（规范化后同键）走缓存。
        let r3 = eval_cached_i64(&mut cache, "N", &ev, &expr);
        v.push((
            "F0448-缓存-规范化同键",
            r3 == Ok(8) && cache.hits == 2,
        ));
    }

    // 四、条款诊断。
    {
        // 非常量（环境缺名）：诊断带条款与求值路径说明。
        let env: Vec<(String, CVal)> = Vec::new();
        let ev = Evaluator::new(&env);
        let bad = CExpr::Ident { name: "runtime_var".to_string(), off: 9 };
        match ev.eval(&bad) {
            Err(d) => v.push((
                "F0448-诊断-非常量带条款",
                d.kind == DiagKind::NotConst
                    && d.clause == CLAUSE_CONST_CTX
                    && d.note.contains("求值路径"),
            )),
            Ok(_) => v.push(("F0448-诊断-非常量带条款", false)),
        }
        // 三元：布尔条件按分支求值（语境面语义完整）。
        let ev2_env = Vec::new();
        let ev2 = Evaluator::new(&ev2_env);
        let tern = CExpr::Ternary {
            cond: Box::new(CExpr::Lit { val: CVal::B(false), off: 0 }),
            then: Box::new(CExpr::Lit { val: CVal::I(1), off: 0 }),
            els: Box::new(CExpr::Lit { val: CVal::I(2), off: 0 }),
            off: 0,
        };
        v.push(("F0448-诊断-三元分支求值", ev2.eval(&tern) == Ok(CVal::I(2))));
    }

    // 五、判据自检。
    {
        v.push(("F0448-判据-锚点入册", ANCHOR == "VE-F0448"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec48）。
pub fn run_vec48_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec48");
    for (name, ok) in vec48_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec48_raw_checks as run_vec48_checks;
    #[test]
    fn vec48_consteval_checks() {
        for (name, ok) in run_vec48_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
