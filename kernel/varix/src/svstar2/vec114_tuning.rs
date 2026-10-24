//! VE-F0514 · GLSL 生成器性能调优（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0514`
//!
//! **锚点原文**：「生成器性能调优：热点分析（发射路径剖析）、调优项（字符串
//! 构建优化、查表预编译、缓冲复用）、调优后基准对比（F0507 基准复测）；调优
//! 历史记录（改了什么、收益多少诚实入账）。判据：热点剖析、诚实回滚、对拍
//! 保护、基准裁决、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **热点剖析**（判据一）。发射路径按段计量（确定性步数模型），热点 =
//!    步数占比超过阈值的段；剖析先于调优（无剖析不调优）。
//!
//! 2. **诚实回滚**（判据二）。调优项收益 <= 0 即回滚并如实记录（不保留
//!    复杂度）；收益争议由基准裁决，不由感觉裁决。
//!
//! 3. **对拍保护**（判据三）。调优后必须过黄金对拍才可入账；破坏正确性
//!    的调优无条件回滚。
//!
//! 4. **基准裁决**（判据四）。调优前后的基准记录对比（F0507 结构复用），
//!    收益按步数差诚实入账（可为负）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0514";

// ---------------------------------------------------------------------------
// 一、热点剖析（发射路径分段计量）
// ---------------------------------------------------------------------------

/// 发射段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Lookup,
    Build,
    Fmt,
}

/// 剖析记录：段 → 步数。
#[derive(Default)]
pub struct Profile {
    pub steps: Vec<(Seg, u64)>,
}

impl Profile {
    pub fn new() -> Profile {
        Profile { steps: Vec::new() }
    }

    pub fn add(&mut self, s: Seg, n: u64) {
        if let Some(e) = self.steps.iter_mut().find(|(x, _)| *x == s) {
            e.1 += n;
        } else {
            self.steps.push((s, n));
        }
    }

    pub fn total(&self) -> u64 {
        self.steps.iter().map(|(_, n)| *n).sum()
    }

    /// 热点段：占比 >= 阈值（千分比）且步数最大者。
    pub fn hot(&self, thr_per_mille: u64) -> Option<Seg> {
        let t = self.total();
        if t == 0 {
            return None;
        }
        let mut best: Option<(Seg, u64)> = None;
        for (s, n) in self.steps.iter() {
            if n * 1000 >= thr_per_mille * t {
                match best {
                    Some((_, bn)) if *n <= bn => {}
                    _ => best = Some((*s, *n)),
                }
            }
        }
        best.map(|(s, _)| s)
    }
}

// ---------------------------------------------------------------------------
// 二、调优项账（诚实入账，可为负收益）
// ---------------------------------------------------------------------------

/// 调优项。
#[derive(Clone, Debug)]
pub struct Tune {
    pub name: String,
    /// 前后基准步数（每千指令）。
    pub before: u64,
    pub after: u64,
    /// 是否保留（收益 > 0 且对拍过）。
    pub kept: bool,
    /// 回滚理由（未保留时必填）。
    pub rollback: String,
}

/// 调优历史。
#[derive(Default)]
pub struct TuneBook {
    pub items: Vec<Tune>,
}

impl TuneBook {
    pub fn new() -> TuneBook {
        TuneBook { items: Vec::new() }
    }

    /// 入账：收益 = before - after（正收益为快）。
    pub fn record(&mut self, name: &str, before: u64, after: u64, gold_ok: bool) -> Result<(), u32> {
        if name.is_empty() {
            return Err(XC_NONAME);
        }
        if !gold_ok {
            // 对拍失败：无条件回滚。
            self.items.push(Tune {
                name: name.to_string(),
                before,
                after,
                kept: false,
                rollback: "对拍拦截".to_string(),
            });
            return Ok(());
        }
        if after < before {
            self.items.push(Tune { name: name.to_string(), before, after, kept: true, rollback: String::new() });
        } else {
            let why = if after == before { "零收益" } else { "负收益" };
            self.items.push(Tune { name: name.to_string(), before, after, kept: false, rollback: why.to_string() });
        }
        Ok(())
    }

    /// 收益合计（只算保留项）。
    pub fn gain(&self) -> i64 {
        self.items.iter().filter(|t| t.kept).map(|t| t.before as i64 - t.after as i64).sum()
    }

    /// 诚实性：未保留项全部带理由。
    pub fn honest(&self) -> bool {
        self.items.iter().all(|t| t.kept || !t.rollback.is_empty())
    }
}

pub const XC_NONAME: u32 = 1;

// ---------------------------------------------------------------------------
// 三、查表预编译与缓冲复用（两个具体调优模型）
// ---------------------------------------------------------------------------

/// 预编译查表：把线性扫描转哈希索引（确定性简易哈希）。
pub fn precompile(ops: &[&str]) -> Vec<(u32, usize)> {
    ops.iter().enumerate().map(|(i, o)| {
        let mut h = 0u32;
        for c in o.chars() {
            h = h.wrapping_mul(33).wrapping_add(c as u32);
        }
        (h, i)
    }).collect()
}

/// 预编译查表命中。
pub fn lookup_fast(table: &[(u32, usize)], op: &str) -> Option<usize> {
    let mut h = 0u32;
    for c in op.chars() {
        h = h.wrapping_mul(33).wrapping_add(c as u32);
    }
    table.iter().find(|(k, _)| *k == h).map(|(_, i)| *i)
}

/// 缓冲复用：行缓冲池（取还）。
pub struct Pool {
    free: Vec<String>,
    pub reused: usize,
    pub fresh: usize,
}

impl Pool {
    pub fn new(n: usize) -> Pool {
        Pool { free: (0..n).map(|_| String::new()).collect(), reused: 0, fresh: 0 }
    }

    /// 取一缓冲：池空才新分配。
    pub fn take(&mut self) -> String {
        match self.free.pop() {
            Some(mut s) => {
                s.clear();
                self.reused += 1;
                s
            }
            None => {
                self.fresh += 1;
                String::new()
            }
        }
    }

    pub fn give(&mut self, s: String) {
        self.free.push(s);
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec114_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec114");

    // 族一：热点剖析。
    let mut p = Profile::new();
    p.add(Seg::Lookup, 100);
    p.add(Seg::Build, 700);
    p.add(Seg::Fmt, 200);
    s.add("总量守恒", p.total() == 1000, "三段合计");
    s.add("热点识别", p.hot(500) == Some(Seg::Build), "Build 占 70%");
    s.add("高阈值无热点", p.hot(900).is_none(), "阈值上不冤枉");
    s.add("空剖析无热点", Profile::new().hot(1).is_none(), "零步数守卫");

    // 族二：诚实入账。
    let mut b = TuneBook::new();
    s.add("空名拒", matches!(b.record("", 100, 90, true), Err(XC_NONAME)), "调优必实名");
    let _ = b.record("预编译查表", 100, 60, true);
    let _ = b.record("缓冲复用", 100, 100, true);
    let _ = b.record("内联展开", 100, 120, true);
    s.add("正收益保留", b.items[0].kept, "60 < 100");
    s.add("零收益回滚", !b.items[1].kept && b.items[1].rollback == "零收益", "不保留复杂度");
    s.add("负收益回滚", !b.items[2].kept && b.items[2].rollback == "负收益", "诚实入账");
    s.add("收益合计 40", b.gain() == 40, "只算保留项");
    s.add("全部带理由或保留", b.honest(), "诚实性校验");

    // 族三：对拍保护。
    let mut b2 = TuneBook::new();
    let _ = b2.record("激进改写", 100, 10, false);
    s.add("对拍拦截即回滚", !b2.items[0].kept && b2.items[0].rollback.contains("对拍"), "正确性优先");

    // 族四：两个具体调优模型。
    let ops = ["iadd", "isub", "imul", "tex2d"];
    let tb = precompile(&ops);
    s.add("预编译命中", lookup_fast(&tb, "imul") == Some(2), "哈希索引");
    s.add("预编译未命中", lookup_fast(&tb, "rsq").is_none(), "表外拒绝");
    let mut pool = Pool::new(2);
    let _a = pool.take();
    let _b3 = pool.take();
    let _c = pool.take();
    s.add("池耗尽才新分配", pool.reused == 2 && pool.fresh == 1, "复用优先");
    pool.give(_c);
    let _d = pool.take();
    s.add("归还后再复用", pool.reused == 3 && pool.fresh == 1, "缓冲循环");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tune() {
        let mut b = TuneBook::new();
        let _ = b.record("x", 10, 5, true);
        assert_eq!(b.gain(), 5);
    }
}
