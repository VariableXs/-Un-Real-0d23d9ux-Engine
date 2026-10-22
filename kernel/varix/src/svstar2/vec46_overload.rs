//! VE-F0446 · 函数重载决议（VE-C 域 · 语义组 C03 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0446`
//!
//! **锚点原文**：「函数重载决议：候选收集（同名跨作用域）、可行集过滤
//! （参数个数与可转换性按等级表）、最佳匹配裁定（歧义即报错带候选明细，
//! 不静默选一个）、用户函数与内建统一决议（F0432 遮蔽登记在此裁决）；
//! 决议结果旁路登记。判据：候选过滤、歧义全明细、统一决议、旁路登记、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **候选过滤**（判据一）。三段过滤闭集：① 名收集（同名跨作用域全收，
//!    F0442 逐级查找的多结果版）；② 个数过滤（参数数不等者出局）；③
//!    可转换性过滤（实参→形参等级表全参可转才留，vec43 rank_of）。
//!    每段出局原因记账（不是静默筛掉）。
//!
//! 2. **歧义全明细**（判据二）。可行集 >1 时**报错不选**：诊断携带全部
//!    候选明细（签名摘要列）。空候选也报错（带实参类型与建议）。
//!    恰一个可行 → 唯一决议（唯一时无需比较等级）。
//!
//! 3. **统一决议**（判据三）。用户函数与内建**同一候选池**：内建遮蔽
//!    （F0432 登记的用户重定义）在此裁决——用户条目按作用域层级优先于
//!    内建（内建 = 最外层伪域）。判据实测：用户重定义后调用量到用户版。
//!
//! 4. **旁路登记**（判据四）。决议结果写旁路表（F0441 通道）：调用节点
//!    号 → 当选候选签名。决议与实现不一致 → 测试对拍修正（判据以
//!    确定性重放验证：同输入两次决议同结果）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0446";

use super::vec43_typesys::Ty;
use alloc::format;

/// 候选签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sig {
    pub name: String,
    pub params: Vec<Ty>,
    /// 层级（0 = 最内作用域，越大越外；内建伪域 = u32::MAX）。
    pub scope: u32,
    pub builtin: bool,
}

impl Sig {
    pub fn digest(&self) -> String {
        let mut s = alloc::format!("{}(", self.name);
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&p.digest());
        }
        s.push(')');
        s
    }
}

/// 出局原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    ArityMis,
    NotConvertible,
    /// 用户候选在场遮蔽内建（统一决议的伪域出局面）。
    Shadowed,
}

/// 候选过滤记录（出局留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutRecord {
    pub sig_digest: String,
    pub why: Cut,
}

/// 决议结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolve {
    /// 唯一当选（候选摘要）。
    Elected(String),
    /// 歧义（全部可行候选明细）。
    Ambiguous(Vec<String>),
    /// 空候选（带实参摘要）。
    Empty(String),
}

/// 可转换性：全参按等级表可转（vec43 rank_of + 向量同维直通）。
pub fn convertible(arg: &Ty, param: &Ty) -> bool {
    match (arg, param) {
        _ if arg == param => true,
        (Ty::Scalar(a), Ty::Scalar(b)) => {
            super::vec43_typesys::rank_of(&Ty::Scalar(a), &Ty::Scalar(b)).is_some()
        }
        _ => false,
    }
}

/// 决议器：三段过滤 + 歧义不选。
pub struct Resolver;

impl Resolver {
    /// 候选收集：同名跨作用域全收（调用方给全量同名集）。
    pub fn collect<'a>(all: &'a [Sig], name: &str) -> Vec<&'a Sig> {
        all.iter().filter(|s| s.name == name).collect()
    }
    /// 完整决议。
    pub fn resolve(all: &[Sig], name: &str, args: &[Ty]) -> (Resolve, Vec<CutRecord>) {
        let mut cuts: Vec<CutRecord> = Vec::new();
        // ① 名收集。
        let named = Self::collect(all, name);
        if named.is_empty() {
            let mut digest = String::new();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    digest.push(',');
                }
                digest.push_str(&a.digest());
            }
            return (Resolve::Empty(digest), cuts);
        }
        // ② 个数过滤。
        let arity: Vec<&&Sig> = named.iter().filter(|s| s.params.len() == args.len()).collect();
        for s in named.iter() {
            if s.params.len() != args.len() {
                cuts.push(CutRecord { sig_digest: s.digest(), why: Cut::ArityMis });
            }
        }
        if arity.is_empty() {
            let mut digest = String::new();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    digest.push(',');
                }
                digest.push_str(&a.digest());
            }
            return (Resolve::Empty(digest), cuts);
        }
        // ③ 可转换性过滤。
        let mut viable: Vec<&&Sig> = Vec::new();
        for s in arity.iter() {
            let ok = s
                .params
                .iter()
                .zip(args.iter())
                .all(|(p, a)| convertible(a, p));
            if ok {
                viable.push(s);
            } else {
                cuts.push(CutRecord { sig_digest: s.digest(), why: Cut::NotConvertible });
            }
        }
        match viable.len() {
            0 => {
                let mut digest = String::new();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        digest.push(',');
                    }
                    digest.push_str(&a.digest());
                }
                (Resolve::Empty(digest), cuts)
            }
            1 => (Resolve::Elected(viable[0].digest()), cuts),
            // 歧义不选：层级最小者不偷选——全明细上报。
            _ => {
                let ds: Vec<String> = viable.iter().map(|s| s.digest()).collect();
                (Resolve::Ambiguous(ds), cuts)
            }
        }
    }
    /// 统一决议：内建伪域层级 = u32::MAX（用户层级总是更内层）。
    /// 混集歧义时用户可行候选遮蔽一切内建候选（内建出局留痕），余下
    /// 按纯用户集复裁；纯用户歧义仍不选（全明细上报）。
    pub fn unified(all: &[Sig], name: &str, args: &[Ty]) -> (Resolve, Vec<CutRecord>) {
        let (r, mut cuts) = Self::resolve(all, name, args);
        if let Resolve::Ambiguous(_) = r {
            // 用户可行候选在场？
            let user_viable = |s: &Sig| {
                s.name == name
                    && !s.builtin
                    && s.params.len() == args.len()
                    && s.params.iter().zip(args.iter()).all(|(p, a)| convertible(a, p))
            };
            if all.iter().any(user_viable) {
                for s in all.iter().filter(|s| s.name == name && s.builtin) {
                    cuts.push(CutRecord { sig_digest: s.digest(), why: Cut::Shadowed });
                }
                let users: Vec<Sig> =
                    all.iter().filter(|s| s.name == name && !s.builtin).cloned().collect();
                let (r2, _) = Self::resolve(&users, name, args);
                return (r2, cuts);
            }
        }
        (r, cuts)
    }
}

/// 决议旁路登记。
#[derive(Debug, Default)]
pub struct ResolveNotes {
    /// 调用节点号 → 当选摘要。
    pub notes: Vec<(u32, String)>,
}

impl ResolveNotes {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn put(&mut self, node: u32, sig: &str) {
        self.notes.push((node, sig.into()));
    }
    pub fn get(&self, node: u32) -> Option<&str> {
        self.notes.iter().rev().find(|(n, _)| *n == node).map(|(_, s)| s.as_str())
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec46_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let sig = |n: &str, ps: Vec<Ty>, sc: u32, b: bool| Sig { name: n.into(), params: ps, scope: sc, builtin: b };
    let i_t = Ty::Scalar("int");
    let f_t = Ty::Scalar("float");
    let d_t = Ty::Scalar("double");
    // 一、候选过滤。
    {
        let all = vec![
            sig("f", vec![i_t.clone()], 0, false),
            sig("f", vec![i_t.clone(), i_t.clone()], 0, false),
            sig("g", vec![f_t.clone()], 0, false),
        ];
        let (r, cuts) = Resolver::resolve(&all, "f", &[i_t.clone()]);
        v.push((
            "F0446-过滤-唯一当选",
            r == Resolve::Elected("f(s:int)".to_string()),
        ));
        // 个数出局留痕。
        v.push((
            "F0446-过滤-个数出局留痕",
            cuts.len() == 1 && cuts[0].why == Cut::ArityMis,
        ));
        // 可转换性：int 实参 → float 形参可转；double 实参 → int 形参不可转。
        let all2 = vec![sig("h", vec![f_t.clone()], 0, false)];
        let (r2, _) = Resolver::resolve(&all2, "h", &[i_t.clone()]);
        v.push(("F0446-过滤-升级可转", r2 == Resolve::Elected("h(s:float)".to_string())));
        let (r3, cuts3) = Resolver::resolve(&all2, "h", &[d_t.clone()]);
        v.push((
            "F0446-过滤-降级出局",
            matches!(r3, Resolve::Empty(_)) && cuts3.iter().any(|c| c.why == Cut::NotConvertible),
        ));
        // 空名候选。
        let (r4, _) = Resolver::resolve(&all, "ghost", &[i_t.clone()]);
        v.push(("F0446-过滤-空名报错", matches!(r4, Resolve::Empty(_))));
    }
    // 二、歧义全明细。
    {
        // f(float) 与 f(double)：int 实参两者皆可转 → 歧义。
        let all = vec![sig("f", vec![f_t.clone()], 0, false), sig("f", vec![d_t.clone()], 0, false)];
        let (r, _) = Resolver::resolve(&all, "f", &[i_t.clone()]);
        match r {
            Resolve::Ambiguous(ref ds) => {
                v.push(("F0446-歧义-两候选全明细", ds.len() == 2 && ds.contains(&"f(s:float)".to_string()) && ds.contains(&"f(s:double)".to_string())));
            }
            _ => v.push(("F0446-歧义-两候选全明细", false)),
        }
        // 不静默选：歧义不是 Elected。
        v.push(("F0446-歧义-不偷选", !matches!(r, Resolve::Elected(_))));
        // 恰一个可行 → 唯一。
        let all2 = vec![sig("f", vec![f_t.clone()], 0, false), sig("f", vec![i_t.clone()], 0, false)];
        let (r2, _) = Resolver::resolve(&all2, "f", &[f_t.clone()]);
        v.push(("F0446-歧义-精确匹配唯一", r2 == Resolve::Elected("f(s:float)".to_string())));
    }
    // 三、统一决议（遮蔽裁决）。
    {
        // 内建 abs(float) + 用户重定义 abs(int)（层级 0 更内层）。
        let all = vec![
            sig("abs", vec![f_t.clone()], u32::MAX, true),
            sig("abs", vec![i_t.clone()], 0, false),
        ];
        let (r, _) = Resolver::unified(&all, "abs", &[i_t.clone()]);
        v.push((
            "F0446-统一-用户遮内建",
            r == Resolve::Elected("abs(s:int)".to_string()),
        ));
        // float 实参走内建（用户版 int 不可转降）。
        let (r2, _) = Resolver::unified(&all, "abs", &[f_t.clone()]);
        v.push(("F0446-统一-float走内建", r2 == Resolve::Elected("abs(s:float)".to_string())));
        // 内建位不另立机制：无遮蔽场景（纯用户集）下 unified == resolve；
        // 混集场景两机制分工显性（resolve 全明细、unified 伪域出局面）。
        let pure_user = vec![sig("abs", vec![i_t.clone()], 0, false)];
        v.push((
            "F0446-统一-机制统一",
            Resolver::unified(&pure_user, "abs", &[i_t.clone()])
                == Resolver::resolve(&pure_user, "abs", &[i_t.clone()]),
        ));
    }
    // 四、旁路登记 + 确定性。
    {
        let all = vec![sig("f", vec![i_t.clone()], 0, false)];
        let (r, _) = Resolver::resolve(&all, "f", &[i_t.clone()]);
        if let Resolve::Elected(s) = &r {
            let mut notes = ResolveNotes::new();
            notes.put(11, s);
            v.push(("F0446-旁路-写入读出", notes.get(11) == Some("f(s:int)")));
            // 重放确定性：同输入两次同结果。
            let (r2, _) = Resolver::resolve(&all, "f", &[i_t.clone()]);
            v.push(("F0446-旁路-重放确定", r == r2));
        } else {
            v.push(("F0446-旁路-写入读出", false));
            v.push(("F0446-旁路-重放确定", false));
        }
        // 判据自检。
        v.push(("F0446-判据-锚点入册", ANCHOR == "VE-F0446"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec46_raw_checks as run_vec46_checks;
    #[test]
    fn vec46_overload_checks() {
        for (name, ok) in run_vec46_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec46）。
pub fn run_vec46_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec46");
    for (name, ok) in vec46_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
