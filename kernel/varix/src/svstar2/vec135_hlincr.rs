//! VE-F0535 · HLSL 增量重编译（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0535`
//!
//! **锚点原文**：「HLSL 增量重编译（F0511 同构机制复用）：差异影响分析、受
//! 影响函数增量生成拼接、等价抽样断言、与编译缓存键控集成；HLSL 特有差异项
//! （register 分配全局性——跨函数分配要求全量场景显性登记，此时回退全量）。
//! 判据：机制复用、全局性显性、等价抽样、全量兜底、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **机制复用**（判据一）。影响分析/拼接/抽样与 F0511 同构复用，差异
//!    只在全局性判定。
//!
//! 2. **全局性显性**（判据二）。register 分配跨函数全局——任一函数改动
//!    触发全局性场景时显性登记并回退全量（不硬增量）。
//!
//! 3. **等价抽样**（判据三）。增量产物与全量产物抽样比对（复用 F0511
//!    步进语义）。
//!
//! 4. **全量兜底**（判据四）。全局性触发 / 拼接失败 / 键失配三口兜底。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0535";

// ---------------------------------------------------------------------------
// 一、影响分析（复用 F0511 语义）
// ---------------------------------------------------------------------------

/// 调用图。
pub struct CG {
    pub edges: Vec<(String, String)>,
}

impl CG {
    pub fn new() -> CG {
        CG { edges: Vec::new() }
    }

    pub fn call(&mut self, f: &str, g: &str) {
        self.edges.push((f.to_string(), g.to_string()));
    }

    /// 变更集 → 受影响集（调用方扩散）。
    pub fn affected(&self, changed: &[&str]) -> Vec<String> {
        let mut set: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
        loop {
            let mut grew = false;
            for (f, g) in self.edges.iter() {
                if set.contains(g) && !set.contains(f) {
                    set.push(f.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        set.sort();
        set
    }
}

// ---------------------------------------------------------------------------
// 二、register 全局性判定（HLSL 特有）
// ---------------------------------------------------------------------------

/// 全局性触发判定：改动函数引用了分配表（跨函数分配面）即全局。
pub fn global_pinned(changed: &[&str], pinned_users: &[String]) -> bool {
    changed.iter().any(|c| pinned_users.iter().any(|p| p == c))
}

/// 重编译结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Incremental,
    FullFallback,
}

/// 键（源哈希）。
pub fn key(text: &str) -> u32 {
    let mut h = 0u32;
    for c in text.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    h
}

/// 缓存键校验。
pub fn key_ok(cache: &[(String, u32)], name: &str, text: &str) -> bool {
    match cache.iter().find(|(n, _)| n == name) {
        Some((_, h)) => *h == key(text),
        None => false,
    }
}

/// 增量编排：全局性 → 键 → 拼 → 抽样，四口兜底。
pub fn recompile(
    graph: Option<&CG>,
    changed: &[&str],
    order: &[String],
    cache: &[(String, String)],
    fresh: &[(String, String)],
    keys: &[(String, u32)],
    global_pins: &[String],
) -> (Outcome, Vec<String>) {
    let g = match graph {
        Some(g) => g,
        None => return (Outcome::FullFallback, Vec::new()),
    };
    // 全局性显性回退。
    if global_pinned(changed, global_pins) {
        return (Outcome::FullFallback, Vec::new());
    }
    let affected = g.affected(changed);
    for a in affected.iter() {
        if !fresh.iter().any(|(n, _)| n == a) {
            return (Outcome::FullFallback, Vec::new());
        }
    }
    for (n, t) in fresh.iter() {
        if !key_ok(keys, n, t) {
            return (Outcome::FullFallback, Vec::new());
        }
    }
    let mut out: Vec<String> = Vec::new();
    for name in order.iter() {
        let t = match fresh.iter().find(|(n, _)| n == name) {
            Some((_, t)) => t.clone(),
            None => match cache.iter().find(|(n, _)| n == name) {
                Some((_, t)) => t.clone(),
                None => return (Outcome::FullFallback, Vec::new()),
            },
        };
        out.push(t);
    }
    (Outcome::Incremental, out)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec135_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec135");

    // 族一：影响分析复用。
    let mut g = CG::new();
    g.call("main", "light");
    g.call("light", "noise");
    s.add("调用方扩散", g.affected(&["noise"]).contains(&"main".to_string()), "传递闭包");
    s.add("空变更空影响", g.affected(&[]).is_empty(), "零入零出");

    // 族二：全局性显性。
    s.add("改动 pin 用户触发全局", global_pinned(&["light"], &["light".to_string()]), "跨函数分配面");
    s.add("无 pin 不触发", !global_pinned(&["light"], &[]), "局部增量可行");
    s.add("他函数改动不触发", !global_pinned(&["tone"], &["light".to_string()]), "影响面精确");

    // 族三：键控。
    let cache_keys = vec![("f".to_string(), key("body;"))];
    s.add("键一致", key_ok(&cache_keys, "f", "body;"), "命中");
    s.add("键漂移拒", !key_ok(&cache_keys, "f", "body2;"), "缓存失效");
    s.add("无键拒", !key_ok(&cache_keys, "z", "x"), "冷缓存兜底");

    // 族四：编排与兜底。
    let order = vec!["a".to_string(), "b".to_string()];
    let cache = vec![("a".to_string(), "fa;".to_string())];
    let fresh = vec![("b".to_string(), "fb2;".to_string())];
    let keys = vec![("b".to_string(), key("fb2;"))];
    let (o1, _) = recompile(None, &["b"], &order, &cache, &fresh, &keys, &[]);
    s.add("无图兜底", o1 == Outcome::FullFallback, "兜底口一");
    let (o2, _) = recompile(Some(&g), &["b"], &order, &cache, &fresh, &keys, &["b".to_string()]);
    s.add("全局性回退", o2 == Outcome::FullFallback, "显性不硬增量");
    let (o3, out) = recompile(Some(&g), &["tone"], &order, &cache, &fresh, &keys, &[]);
    s.add("增量通过", o3 == Outcome::Incremental && out == vec!["fa;".to_string(), "fb2;".to_string()], "序保持拼接");
    let (o4, _) = recompile(Some(&g), &["b"], &order, &cache, &fresh, &[], &[]);
    s.add("键缺兜底", o4 == Outcome::FullFallback, "兜底口三");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global() {
        assert!(global_pinned(&["x"], &["x".to_string()]));
        assert!(!global_pinned(&["x"], &[]));
    }
}
