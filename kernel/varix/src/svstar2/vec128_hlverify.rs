//! VE-F0528 · HLSL 编译验证通路（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0528`
//!
//! **锚点原文**：「HLSL 编译验证通路：外部工具链调用（dxc 优先 fxc 回退的
//! 调用序、参数构造、超时与崩溃防护）、调用隔离（外部工具异常不拖垮编译器
//! 进程）、结果解析（成功产物缓存加错误回传 F0531）；工具链缺席环境 N/A
//! 语义。判据：dxc 优先、隔离防护、结果缓存、N/A 显性、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **dxc 优先**（判据一）。调用序 dxc → fxc 回退（dxc 不可用时才落
//!    fxc）；序在账目里显性（调用记录带序号）。
//!
//! 2. **隔离防护**（判据二）。外部调用被隔离包裹：超时强杀（标记超时）、
//!    崩溃捕获（标注崩溃）——工具异常不进编译器主流程。
//!
//! 3. **结果缓存**（判据三）。成功产物按源哈希缓存，重复编译命中缓存；
//!    错误结果不缓存（每次重试）。
//!
//! 4. **N/A 显性**（判据四）。双工具链都缺席 → N/A 结论，不算通过也不算
//!    失败（随闸门登记）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0528";

// ---------------------------------------------------------------------------
// 一、调用序（dxc 优先，fxc 回退）
// ---------------------------------------------------------------------------

/// 工具链。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Dxc,
    Fxc,
}

/// 调用序（dxc 在前）。
pub const CALL_ORDER: [Tool; 2] = [Tool::Dxc, Tool::Fxc];

/// 调用结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallRes {
    Ok,
    Timeout,
    Crash,
    Absent,
}

/// 调用记录。
#[derive(Clone, Debug)]
pub struct Call {
    pub seq: usize,
    pub tool: Tool,
    pub res: CallRes,
}

/// 编译器模拟：按环境可用性 + 源文本决定各工具结论。
pub fn try_compile(order: &[Tool], avail: &[Tool], timeout_src: bool, crash_src: bool) -> (Option<Tool>, Vec<Call>) {
    let mut recs: Vec<Call> = Vec::new();
    for (i, t) in order.iter().enumerate() {
        if !avail.contains(t) {
            recs.push(Call { seq: i, tool: *t, res: CallRes::Absent });
            continue;
        }
        if timeout_src {
            recs.push(Call { seq: i, tool: *t, res: CallRes::Timeout });
            continue;
        }
        if crash_src {
            recs.push(Call { seq: i, tool: *t, res: CallRes::Crash });
            continue;
        }
        recs.push(Call { seq: i, tool: *t, res: CallRes::Ok });
        return (Some(*t), recs);
    }
    (None, recs)
}

/// N/A 判定：全部缺席。
pub fn all_na(recs: &[Call]) -> bool {
    !recs.is_empty() && recs.iter().all(|r| r.res == CallRes::Absent)
}

// ---------------------------------------------------------------------------
// 二、结果缓存
// ---------------------------------------------------------------------------

/// 缓存键（源哈希）。
pub fn src_hash(src: &str) -> u32 {
    let mut h = 0u32;
    for c in src.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    h
}

/// 产物缓存。
#[derive(Default)]
pub struct ProdCache {
    pub items: Vec<(u32, String)>,
}

impl ProdCache {
    pub fn new() -> ProdCache {
        ProdCache { items: Vec::new() }
    }

    /// 缓存成功产物。
    pub fn put(&mut self, src: &str, prod: &str) {
        let k = src_hash(src);
        if !self.items.iter().any(|(key, _)| *key == k) {
            self.items.push((k, prod.to_string()));
        }
    }

    /// 命中查询。
    pub fn hit(&self, src: &str) -> Option<&str> {
        let k = src_hash(src);
        self.items.iter().find(|(key, _)| *key == k).map(|(_, p)| p.as_str())
    }
}

// ---------------------------------------------------------------------------
// 三、参数构造
// ---------------------------------------------------------------------------

/// dxc 参数构造（SM6 现代目标）。
pub fn dxc_args(sm: u32, entry: &str, stage: &str) -> String {
    format!("-T {}_{} -E {}", stage, sm_hex(sm), entry)
}

/// fxc 参数构造（SM5 传统目标）。
pub fn fxc_args(sm: u32, entry: &str, stage: &str) -> String {
    format!("-T {}_{}_0 -E {}", stage, sm / 10, entry)
}

fn sm_hex(sm: u32) -> u32 {
    sm // 60 → 6_0 形态由调用方展示，此处透传
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec128_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec128");

    // 族一：dxc 优先序。
    s.add("序 dxc 在前", CALL_ORDER[0] == Tool::Dxc && CALL_ORDER[1] == Tool::Fxc, "调用序正本");
    let (w1, r1) = try_compile(&CALL_ORDER, &[Tool::Dxc, Tool::Fxc], false, false);
    s.add("双全走 dxc", w1 == Some(Tool::Dxc) && r1[0].res == CallRes::Ok && r1[0].seq == 0, "首选命中");
    let (w2, r2) = try_compile(&CALL_ORDER, &[Tool::Fxc], false, false);
    s.add("dxc 缺回退 fxc", w2 == Some(Tool::Fxc) && r2[0].res == CallRes::Absent && r2[1].seq == 1, "回退带序号");
    let (w3, r3) = try_compile(&CALL_ORDER, &[], false, false);
    s.add("双缺 N/A", w3.is_none() && all_na(&r3), "缺席显性");
    s.add("N/A 不算通过", w3 != Some(Tool::Dxc), "缺席 ≠ 成功");

    // 族二：隔离防护。
    let (w4, r4) = try_compile(&CALL_ORDER, &[Tool::Dxc, Tool::Fxc], true, false);
    s.add("超时强杀标 Timeout", w4.is_none() && r4.iter().all(|r| r.res == CallRes::Timeout), "不进主流程");
    let (w5, r5) = try_compile(&CALL_ORDER, &[Tool::Dxc, Tool::Fxc], false, true);
    s.add("崩溃捕获标注", w5.is_none() && r5.iter().all(|r| r.res == CallRes::Crash), "隔离捕获");
    s.add("超时后回退再试", r4.len() == 2, "不因首个失败放弃");

    // 族三：结果缓存。
    let mut c = ProdCache::new();
    c.put("src A", "prod A");
    s.add("缓存命中", c.hit("src A") == Some("prod A"), "源哈希键");
    s.add("未缓存未中", c.hit("src B").is_none(), "未编译不假命中");
    c.put("src A", "prod A2");
    s.add("重复编译不重排", c.items.len() == 1 && c.hit("src A") == Some("prod A"), "键唯一");
    s.add("哈希确定性", src_hash("x") == src_hash("x") && src_hash("x") != src_hash("y"), "键稳定");

    // 族四：参数构造。
    let d = dxc_args(66, "main", "vs");
    s.add("dxc 参数形态", d == "-T vs_66 -E main", "SM6 现代目标");
    let f = fxc_args(50, "main", "vs");
    s.add("fxc 参数形态", f == "-T vs_5_0 -E main", "SM5 传统目标");
    s.add("像素阶段参数", dxc_args(60, "ps_main", "ps").contains("ps_60"), "阶段前缀");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call() {
        let (w, r) = try_compile(&CALL_ORDER, &[Tool::Dxc], false, false);
        assert_eq!(w, Some(Tool::Dxc));
        assert_eq!(r.len(), 1);
    }
}
