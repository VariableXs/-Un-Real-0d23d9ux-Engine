//! VE-F0507 · GLSL 后端性能·生成侧（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0507`
//!
//! **锚点原文**：「生成侧性能：文本发射吞吐基准（每千指令毫秒）、内存上界
//! （流式发射不持全文本）、大着色器性能（万指令级模块实测）；性能基准版本
//! 化，退化阻断。判据：流式发射、吞吐基准、内存上界、大模块实测、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **流式发射**（判据一）。发射器逐指令产出文本行，不持有全文本缓冲
//!    ——上界以「在途行数」计（≤ 容量常数），全量缓存即判据红。
//!
//! 2. **吞吐基准**（判据二）。吞吐按「指令数 → 计量步数」确定性模型记录
//!    （本域无时钟，用指令计数近似），基准记录版本化，退化比对可见。
//!
//! 3. **内存上界**（判据三）。发射器容量常数守恒：容量超界即拒（设计
//!    断言形态），不是等爆。
//!
//! 4. **大模块实测**（判据四）。万指令级模块实测记录在册（指令数、行数、
//!    超时判定），超时给拆分建议（F0493 联动）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0507";

/// 发射器容量上限（在途行数）。
pub const CAP: usize = 64;

/// 流式发射器：逐指令入队、满即拒（不持全文本）。
pub struct Emitter {
    buf: Vec<String>,
    /// 累计发射行数（吞吐计量）。
    pub total: usize,
    /// 拒绝次数（容量断言触发）。
    pub rejects: usize,
}

impl Emitter {
    pub fn new() -> Emitter {
        Emitter { buf: Vec::new(), total: 0, rejects: 0 }
    }

    /// 入队一行：容量守恒断言。
    pub fn push(&mut self, line: &str) -> Result<(), u32> {
        if self.buf.len() >= CAP {
            self.rejects += 1;
            return Err(XC_CAP);
        }
        self.buf.push(line.to_string());
        self.total += 1;
        Ok(())
    }

    /// 下游消费（取走一页，缓冲回落）。
    pub fn flush(&mut self) -> Vec<String> {
        let n = self.buf.len().min(16);
        let page: Vec<String> = self.buf.drain(..n).collect();
        page
    }

    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

pub const XC_CAP: u32 = 1;

// ---------------------------------------------------------------------------
// 二、吞吐基准（确定性指令计数模型）
// ---------------------------------------------------------------------------

/// 基准记录（版本化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bench {
    pub ver: u32,
    /// 每千指令的计量步数（越低越快；本域确定性模型）。
    pub steps_per_kilo: u32,
}

/// 基准版本账：退化判定 = 新版步数 > 旧版 × 容差。
pub struct BenchBook {
    pub records: Vec<Bench>,
}

impl BenchBook {
    pub fn new() -> BenchBook {
        BenchBook { records: Vec::new() }
    }

    /// 登记：版本号唯一、步数非零。
    pub fn record(&mut self, ver: u32, steps: u32) -> Result<(), u32> {
        if steps == 0 {
            return Err(XC_ZERO);
        }
        if self.records.iter().any(|b| b.ver == ver) {
            return Err(XC_DUP);
        }
        self.records.push(Bench { ver, steps_per_kilo: steps });
        Ok(())
    }

    /// 退化判定：与上一版比步数涨幅超过容差（%）即退化。
    pub fn regressed(&self, tol_pct: u32) -> Option<(u32, u32)> {
        let n = self.records.len();
        if n < 2 {
            return None;
        }
        let a = self.records[n - 2].steps_per_kilo as u64;
        let b = self.records[n - 1].steps_per_kilo as u64;
        if b * 100 > a * (100 + tol_pct as u64) {
            Some((a as u32, b as u32))
        } else {
            None
        }
    }

    /// 吞吐模拟：指令数 → 计量步数（千指令粒度向上取整）。
    pub fn steps_for(&self, instrs: usize) -> u64 {
        match self.records.last() {
            Some(b) => {
                let kilo = (instrs + 999) / 1000;
                (kilo as u64) * (b.steps_per_kilo as u64)
            }
            None => 0,
        }
    }
}

pub const XC_ZERO: u32 = 2;
pub const XC_DUP: u32 = 3;

// ---------------------------------------------------------------------------
// 三、大模块实测（万指令级）
// ---------------------------------------------------------------------------

/// 大模块实测记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BigMod {
    pub instrs: u32,
    pub lines: u32,
    /// 是否超时（超时给拆分建议）。
    pub overtime: bool,
}

/// 大模块实测：超时判定与拆分建议。
pub fn big_mod(instrs: u32, budget: u32) -> BigMod {
    // 确定性模型：行数 = 指令数（一对一发射），超时 = 步数超预算。
    BigMod { instrs, lines: instrs, overtime: instrs > budget }
}

/// 拆分建议（F0493 联动）：建议块数。
pub fn split_hint(m: &BigMod, per_chunk: u32) -> u32 {
    if !m.overtime {
        return 1;
    }
    (m.instrs + per_chunk - 1) / per_chunk
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec107_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec107");

    // 族一：流式发射。
    let mut e = Emitter::new();
    for i in 0..CAP {
        let _ = e.push("vec4 v;"); // 装满
    }
    s.add("容量守恒", e.pending() == CAP, "满容断言前");
    s.add("超容拒", matches!(e.push("x;"), Err(XC_CAP)), "设计断言形态");
    s.add("拒绝计数", e.rejects == 1, "断言留痕");
    let page = e.flush();
    s.add("页消费回落", e.pending() == CAP - page.len() && !page.is_empty(), "流式不持全文本");
    let _ = e.push("y;");
    s.add("回落后再入队", e.pending() == CAP - page.len() + 1, "容量恢复");

    // 族二：吞吐基准版本化。
    let mut b = BenchBook::new();
    s.add("零步拒", matches!(b.record(1, 0), Err(XC_ZERO)), "计量非零");
    let _ = b.record(1, 100);
    s.add("重复版本拒", matches!(b.record(1, 90), Err(XC_DUP)), "版本唯一");
    let _ = b.record(2, 110);
    s.add("微涨不退化", b.regressed(20).is_none(), "容差内放行");
    let _ = b.record(3, 200);
    s.add("暴涨判退化", b.regressed(20) == Some((110, 200)), "退化可见");
    s.add("单版无退化基线", {
        let mut b2 = BenchBook::new();
        let _ = b2.record(1, 100);
        b2.regressed(20).is_none()
    }, "无对照不判");
    s.add("千粒度取整", b.steps_for(1) == 200 && b.steps_for(1024) == 2 * 200, "向上取整");

    // 族三：大模块实测。
    let ok = big_mod(4_000, 8_000);
    s.add("预算内不超时", !ok.overtime && ok.lines == 4_000, "行数一对一");
    let bad = big_mod(12_000, 10_000);
    s.add("万级超时可见", bad.overtime && bad.instrs == 12_000, "超时判定");
    s.add("超时拆分建议", split_hint(&bad, 4_000) == 3, "三块拆分");
    s.add("未超时单块", split_hint(&ok, 4_000) == 1, "无需拆分");
    s.add("除尽不空块", split_hint(&big_mod(8_000, 4_000), 4_000) == 2, "整除对齐");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn em() {
        let mut e = Emitter::new();
        assert!(e.push("a;").is_ok());
        assert_eq!(e.flush().len(), 1);
    }
}
