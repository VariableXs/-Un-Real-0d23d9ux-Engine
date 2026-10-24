//! VE-F0510 · GLSL 后端已知限制清单（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0510`
//!
//! **锚点原文**：「GLSL 后端已知限制清单：限制项登记（不支持构造、降级语义、
//! 精度差异三类，每项含影响与替代方案）、限制与 F0438 拒绝清单分工（后端
//! 限制 vs 语法兼容限制）；清单版本化随宣告发布；受限项不承诺。判据：三类
//! 限制、分工边界、不承诺、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三类限制**（判据一）。不支持构造 / 降级语义 / 精度差异三类逐项
//!    登记，每项必须带影响面与替代方案（两字段空 = 登记不完整）。
//!
//! 2. **分工边界**（判据二）。本清单只收后端限制；语法兼容类归 F0438
//!    拒绝清单——同一条目重复登记两处即分工违规。
//!
//! 3. **不承诺**（判据三）。受限项查询返回「无承诺」结论而不是默认可用
//!    ——缺省放行即判据红。
//!
//! 4. **版本化**（判据四）。清单带版本号，条目增删必须升版（版本与条目
//!    数联动可验证）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0510";

// ---------------------------------------------------------------------------
// 一、限制模型（三类）
// ---------------------------------------------------------------------------

/// 限制类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    NoCtor,
    Degrade,
    Precision,
}

/// 限制条目。
#[derive(Clone, Debug)]
pub struct Limit {
    pub kind: LimitKind,
    /// 受限构造（如 "ssbo-atomic"）。
    pub ctor: String,
    /// 影响面。
    pub impact: String,
    /// 替代方案。
    pub alt: String,
}

/// 限制清单（版本化）。
pub struct LimitBook {
    pub ver: u32,
    items: Vec<Limit>,
}

impl LimitBook {
    pub fn new(ver: u32) -> LimitBook {
        LimitBook { ver, items: Vec::new() }
    }

    /// 登记：影响与替代必填；同构造重复登记拒。
    pub fn reg(&mut self, kind: LimitKind, ctor: &str, impact: &str, alt: &str) -> Result<(), u32> {
        if ctor.is_empty() {
            return Err(XC_NOCTOR);
        }
        if impact.is_empty() || alt.is_empty() {
            return Err(XC_INCOMPLETE);
        }
        if self.items.iter().any(|l| l.ctor == ctor) {
            return Err(XC_DUP);
        }
        self.items.push(Limit { kind, ctor: ctor.to_string(), impact: impact.to_string(), alt: alt.to_string() });
        self.ver += 1;
        Ok(())
    }

    /// 受限查询：在册 = 无承诺；不在册 = 可用。
    pub fn promise(&self, ctor: &str) -> Result<(), u32> {
        if self.items.iter().any(|l| l.ctor == ctor) {
            Err(XC_NOPROMISE)
        } else {
            Ok(())
        }
    }

    pub fn count(&self, k: LimitKind) -> usize {
        self.items.iter().filter(|l| l.kind == k).count()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn ver(&self) -> u32 {
        self.ver
    }
}

pub const XC_NOCTOR: u32 = 1;
pub const XC_INCOMPLETE: u32 = 2;
pub const XC_DUP: u32 = 3;
pub const XC_NOPROMISE: u32 = 4;

// ---------------------------------------------------------------------------
// 二、分工边界（与 F0438 拒绝清单）
// ---------------------------------------------------------------------------

/// 条目是否属于语法兼容域（应归 F0438 而非本清单）。
pub fn syntax_domain(ctor: &str) -> bool {
    // 保留字冲突、语法歧义类是语法兼容域。
    ctor.starts_with("kw-") || ctor.starts_with("amb-")
}

/// 分工校验：本清单内不允许语法域条目。
pub fn division_ok(book: &LimitBook) -> bool {
    book.items.iter().all(|l| !syntax_domain(&l.ctor))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec110_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec110");

    // 族一：三类登记。
    let mut b = LimitBook::new(1);
    s.add("空构造拒", matches!(b.reg(LimitKind::NoCtor, "", "影响", "替代"), Err(XC_NOCTOR)), "条目必实名");
    s.add("缺替代拒", matches!(b.reg(LimitKind::NoCtor, "ssbo-atomic", "影响", ""), Err(XC_INCOMPLETE)), "两字段必填");
    let _ = b.reg(LimitKind::NoCtor, "ssbo-atomic", "原子操作不可用", "临界区软件模拟");
    let _ = b.reg(LimitKind::Degrade, "subgroup-vote", "降级为手写归约", "共享内存归约树");
    let _ = b.reg(LimitKind::Precision, "highp-mod", "mod 精度损失", "x - y*floor(x/y)");
    s.add("三类各一件", b.count(LimitKind::NoCtor) == 1 && b.count(LimitKind::Degrade) == 1 && b.count(LimitKind::Precision) == 1, "三清单齐");
    s.add("重复构造拒", matches!(b.reg(LimitKind::NoCtor, "ssbo-atomic", "i", "a"), Err(XC_DUP)), "一构造一登记");

    // 族二：版本化联动。
    s.add("登记自动升版", b.ver() == 4, "三次登记三次升");
    s.add("初始版本正确", LimitBook::new(7).ver() == 7, "版本号自持");

    // 族三：不承诺语义。
    s.add("受限项无承诺", matches!(b.promise("ssbo-atomic"), Err(XC_NOPROMISE)), "缺省不放行");
    s.add("非受限可用", b.promise("普通texture").is_ok(), "清单外可用");

    // 族四：分工边界。
    s.add("语法域条目识别", syntax_domain("kw-reserved"), "kw- 前缀");
    s.add("歧义域条目识别", syntax_domain("amb-shift"), "amb- 前缀");
    s.add("分工校验通过", division_ok(&b), "无越界条目");
    let mut bad = LimitBook::new(1);
    let _ = bad.reg(LimitKind::NoCtor, "kw-inline", "影响", "替代");
    s.add("越界条目拦截", !division_ok(&bad), "语法域归 F0438");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promise() {
        let mut b = LimitBook::new(1);
        let _ = b.reg(LimitKind::Degrade, "x", "i", "a");
        assert!(matches!(b.promise("x"), Err(XC_NOPROMISE)));
    }
}
