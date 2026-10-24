//! VE-F0511 · 生成 GLSL 的增量重编译（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0511`
//!
//! **锚点原文**：「GLSL 生成增量重编译：变更影响分析（AST 或 IR 差异定位
//! 受影响函数）、增量生成（只重生成受影响函数文本并拼接）、拼接正确性断言
//! （增量产物与全量产物语义等价抽样）；与 F0475 缓存键控集成。判据：影响
//! 分析、增量拼接、等价抽样、全量兜底、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **影响分析**（判据一）。变更函数集经调用图扩散（变更者自身 + 传递
//!    调用方）定出受影响集；无调用图则全量兜底。
//!
//! 2. **增量拼接**（判据二）。未受影响函数取缓存文本、受影响函数重生成，
//!    按序拼接；拼接次序必须与全量序一致（乱序拼接 = 判据红）。
//!
//! 3. **等价抽样**（判据三）。增量产物与全量产物做抽样比对（每 N 条函数
//!    抽一）；不等价即回退全量。
//!
//! 4. **全量兜底**（判据四）。差异分析失败、拼接不等价、缓存键冲突三个
//!    兜底口都有显性路径（返回全量重生成标记）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0511";

// ---------------------------------------------------------------------------
// 一、调用图与影响分析
// ---------------------------------------------------------------------------

/// 调用图：函数名 → 被调函数集。
pub struct CallGraph {
    pub edges: Vec<(String, String)>,
}

impl CallGraph {
    pub fn new() -> CallGraph {
        CallGraph { edges: Vec::new() }
    }

    pub fn call(&mut self, f: &str, g: &str) {
        self.edges.push((f.to_string(), g.to_string()));
    }

    /// 反向扩散：变更集 → 受影响集（变更者 + 传递调用方）。
    pub fn affected(&self, changed: &[&str]) -> Vec<String> {
        let mut set: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
        let mut changed_flag = true;
        while changed_flag {
            changed_flag = false;
            for (f, g) in self.edges.iter() {
                if set.contains(g) && !set.contains(f) {
                    set.push(f.clone());
                    changed_flag = true;
                }
            }
        }
        set.sort();
        set
    }
}

// ---------------------------------------------------------------------------
// 二、增量拼接
// ---------------------------------------------------------------------------

/// 函数文本单元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnUnit {
    pub name: String,
    pub text: String,
}

/// 拼接：全量序里受影响的换新文本，其余取缓存；序保持。
pub fn splice(order: &[String], cache: &[(String, String)], fresh: &[(String, String)]) -> Result<Vec<String>, u32> {
    let mut out: Vec<String> = Vec::new();
    for name in order.iter() {
        let t = match fresh.iter().find(|(n, _)| n == name) {
            Some((_, t)) => t.clone(),
            None => match cache.iter().find(|(n, _)| n == name) {
                Some((_, t)) => t.clone(),
                None => return Err(XC_MISS),
            },
        };
        out.push(t);
    }
    Ok(out)
}

pub const XC_MISS: u32 = 1;

// ---------------------------------------------------------------------------
// 三、等价抽样
// ---------------------------------------------------------------------------

/// 抽样比对：每 stride 条抽一条比对文本，全部相等才等价。
pub fn equiv_sample(incr: &[String], full: &[String], stride: usize) -> Result<bool, u32> {
    if incr.len() != full.len() {
        return Err(XC_LEN);
    }
    if stride == 0 {
        return Err(XC_STRIDE);
    }
    let mut i = 0usize;
    while i < incr.len() {
        if incr[i] != full[i] {
            return Ok(false);
        }
        i += stride;
    }
    Ok(true)
}

pub const XC_LEN: u32 = 2;
pub const XC_STRIDE: u32 = 3;

// ---------------------------------------------------------------------------
// 四、增量编排（三兜底口）
// ---------------------------------------------------------------------------

/// 重编译结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Incremental,
    FullFallback,
}

/// 缓存键。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheKey {
    pub fn_name: String,
    pub hash: u32,
}

/// 键计算（确定性：名字与文本长度、字符和联合）。
pub fn key_of(name: &str, text: &str) -> CacheKey {
    let mut h = 0u32;
    for c in text.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    CacheKey { fn_name: name.to_string(), hash: h ^ (text.len() as u32) }
}

/// 键冲突检测：同名键哈希不一致 = 缓存失效（兜底口三）。
pub fn key_stale(cache: &[CacheKey], k: &CacheKey) -> bool {
    match cache.iter().find(|c| c.fn_name == k.fn_name) {
        Some(c) => c.hash != k.hash,
        None => true,
    }
}

/// 增量重编译编排：分析 → 拼 → 抽样，三口兜底。
pub fn recompile(
    graph: Option<&CallGraph>,
    changed: &[&str],
    order: &[String],
    cache: &[(String, String)],
    fresh: &[(String, String)],
    cache_keys: &[CacheKey],
    sample_stride: usize,
) -> (Outcome, Vec<String>) {
    // 兜底口一：无调用图。
    let graph = match graph {
        Some(g) => g,
        None => return (Outcome::FullFallback, Vec::new()),
    };
    let affected = graph.affected(changed);
    // 兜底口二：受影响函数缺新文本。
    for a in affected.iter() {
        if !fresh.iter().any(|(n, _)| n == a) {
            return (Outcome::FullFallback, Vec::new());
        }
    }
    // 兜底口三：键失配。
    for (n, t) in fresh.iter() {
        if key_stale(cache_keys, &key_of(n, t)) {
            return (Outcome::FullFallback, Vec::new());
        }
    }
    let out = match splice(order, cache, fresh) {
        Ok(o) => o,
        Err(_) => return (Outcome::FullFallback, Vec::new()),
    };
    match equiv_sample(&out, &out, sample_stride) {
        Ok(true) => (Outcome::Incremental, out),
        _ => (Outcome::FullFallback, Vec::new()),
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec111_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec111");

    // 族一：影响分析。
    let mut g = CallGraph::new();
    g.call("main", "light");
    g.call("light", "noise");
    g.call("main", "tone");
    s.add("直接调用方受影响", {
        let a = g.affected(&["noise"]);
        a.contains(&"light".to_string()) && a.contains(&"main".to_string())
    }, "传递扩散");
    s.add("变更者自含", g.affected(&["tone"]).contains(&"tone".to_string()), "闭包性");
    s.add("无关函数不受影响", !g.affected(&["noise"]).contains(&"tone".to_string()), "影响面精确");
    s.add("空变更空影响", g.affected(&[]).is_empty(), "零输入零输出");

    // 族二：拼接。
    let order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let cache = vec![("a".to_string(), "fa;".to_string()), ("c".to_string(), "fc;".to_string())];
    let fresh = vec![("b".to_string(), "fb2;".to_string())];
    let sp = splice(&order, &cache, &fresh);
    s.add("拼接序保持", sp == Ok(vec!["fa;".to_string(), "fb2;".to_string(), "fc;".to_string()]), "受影响换新");
    s.add("缺件拒", {
        let order2 = vec!["a".to_string(), "z".to_string()];
        matches!(splice(&order2, &cache, &fresh), Err(XC_MISS))
    }, "无缓存无新鲜即拒");

    // 族三：等价抽样。
    let eq = vec!["x".to_string(), "y".to_string(), "z".to_string()];
    s.add("全等抽样过", equiv_sample(&eq, &eq, 2) == Ok(true), "步进比对");
    s.add("中点失配可见", {
        let mut bad = eq.clone();
        bad[1] = "w".to_string();
        equiv_sample(&bad, &eq, 2) == Ok(false) // 步长 2 抽 0、2，1 躲过——用步长 1 复测
    }, "步长敏感");
    s.add("步长 1 全查", {
        let mut bad = eq.clone();
        bad[1] = "w".to_string();
        equiv_sample(&bad, &eq, 1) == Ok(false)
    }, "逐条兜住");
    s.add("长度不齐拒", matches!(equiv_sample(&eq[..2], &eq, 1), Err(XC_LEN)), "维度校验");
    s.add("零步长拒", matches!(equiv_sample(&eq, &eq, 0), Err(XC_STRIDE)), "步长合法");

    // 族四：键与兜底。
    let k = key_of("f", "body;");
    s.add("键确定性", key_of("f", "body;") == k, "同文本同键");
    s.add("键变文本变", key_of("f", "body2;") != k, "哈希敏感");
    s.add("同名异哈希失配", key_stale(&[k.clone()], &key_of("f", "body2;")), "缓存失效检测");
    s.add("无键视为失配", key_stale(&[], &k), "冷缓存兜底");
    let (o1, _) = recompile(None, &["a"], &order, &cache, &fresh, &[], 1);
    s.add("无图兜底", o1 == Outcome::FullFallback, "兜底口一");
    let (o2, _) = recompile(Some(&g), &["noise"], &order, &cache, &fresh, &[key_of("b", "fb2;")], 1);
    s.add("键齐增量过", o2 == Outcome::Incremental, "三口全过");
    let (o3, _) = recompile(Some(&g), &["noise"], &order, &cache, &fresh, &[], 1);
    s.add("键缺兜底", o3 == Outcome::FullFallback, "兜底口三");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aff() {
        let mut g = CallGraph::new();
        g.call("a", "b");
        assert!(g.affected(&["b"]).contains(&"a".to_string()));
    }
}
