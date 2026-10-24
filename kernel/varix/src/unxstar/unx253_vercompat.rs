//! UNX-F0253 早传版本兼容：消费方老版本（0x3CF0 段）
//!
//! 锚点：UNX-F0253-J1 版本字段声明 + 消费方支持矩阵，老版本
//! 消费方读新 Manifest 零崩溃。
//! 域：CoRun·UNX-A1-B13（版本兼容面）。
//!
//! 结构：版本三元组（主.次.修，主版本不兼容 / 次版本向后兼容 /
//! 修订纯修复）+ 消费方支持矩阵（每域声明支持版本区间）+ 兼容
//! 判定器（老读新：次版本差 = 兼容，主版本差 = 拒绝但不崩溃）+
//! 降级读核（不认识的字段跳过、认识的字段照读、零 panic 路径）+
//! 注入核（越集版本 / 主版本跨代 / 空矩阵必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3CF0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3CF0;
pub const VC_MAJ_BREAK: u32 = SEG_BASE + 0x01; // 主版本跨代不兼容
pub const VC_VER_OOB: u32 = SEG_BASE + 0x02; // 版本分量越界（>255）
pub const VC_DOM_OOB: u32 = SEG_BASE + 0x03; // 消费方域越集
pub const VC_NOTDECL: u32 = SEG_BASE + 0x04; // 域未声明支持版本
pub const VC_FIELD_SKIP: u32 = SEG_BASE + 0x05; // 未知字段（降级读跳过，非错误）

/// 版本三元组（各分量 0..=255）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ver {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl Ver {
    pub fn new(major: u8, minor: u8, patch: u8) -> Self {
        Ver { major, minor, patch }
    }

    /// 兼容判定：消费方支持版本 v_sup 读产物版本 v_doc。
    /// 主版本必须相等；产物次版本 ≥ 支持声明次版本（向后兼容）。
    pub fn readable_by(self, v_sup: Ver) -> Result<(), u32> {
        if self.major != v_sup.major {
            return Err(VC_MAJ_BREAK);
        }
        if self.minor < v_sup.minor {
            return Err(VC_MAJ_BREAK); // 产物比声明还老（前向不保证）
        }
        Ok(())
    }

    /// 渲染（vMAJ.MIN.PATCH 稳定序）。
    pub fn render(self) -> String {
        format!("v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 未知字段条目（降级读登记）。
#[derive(Clone)]
pub struct SkipRecord {
    pub field_id: u16,
    pub dom: usize,
}

/// 消费方支持矩阵行：域 + 支持版本声明。
#[derive(Clone)]
pub struct SupportRow {
    pub dom: usize,
    pub sup: Ver,
    pub skips: Vec<SkipRecord>,
}

/// 兼容矩阵。
pub struct CompatMatrix {
    pub rows: Vec<SupportRow>,
    pub doc_ver: Ver,
}

impl CompatMatrix {
    /// 以产物版本开矩阵。
    pub fn new(doc_ver: Ver) -> Self {
        CompatMatrix { rows: Vec::new(), doc_ver }
    }

    /// 声明消费方支持版本（重复域覆盖旧声明）。
    pub fn declare(&mut self, dom: usize, sup: Ver) -> Result<(), u32> {
        if dom > 9 {
            return Err(VC_DOM_OOB);
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.dom == dom) {
            r.sup = sup;
        } else {
            self.rows.push(SupportRow { dom, sup, skips: Vec::new() });
        }
        Ok(())
    }

    /// 读判定：域支持版本 × 产物版本。
    /// 不兼容返回 Err（调用方降级，不崩溃）；兼容返回 Ok。
    pub fn read(&self, dom: usize) -> Result<(), u32> {
        if dom > 9 {
            return Err(VC_DOM_OOB);
        }
        let row = match self.rows.iter().find(|r| r.dom == dom) {
            Some(r) => r,
            None => return Err(VC_NOTDECL),
        };
        self.doc_ver.readable_by(row.sup)
    }

    /// 降级读核：老版本消费方遇到未知字段 → 登记跳过（零崩溃路径）。
    /// 返回本域累计跳过数。
    pub fn degrade_read(&mut self, dom: usize, field_id: u16) -> Result<usize, u32> {
        // 读判定先行：主版本跨代仍拒（拒 ≠ 崩溃，是受控错误返回）
        self.read(dom)?;
        let row = match self.rows.iter_mut().find(|r| r.dom == dom) {
            Some(r) => r,
            None => return Err(VC_NOTDECL),
        };
        if !row.skips.iter().any(|s| s.field_id == field_id) {
            row.skips.push(SkipRecord { field_id, dom });
        }
        Ok(row.skips.len())
    }

    /// 矩阵渲染。
    pub fn render(&self) -> String {
        let mut out = format!("兼容矩阵 产物={}\n", self.doc_ver.render());
        for r in &self.rows {
            let line = format!(
                "  域{} 支持{} 跳过字段={}个\n",
                r.dom,
                r.sup.render(),
                r.skips.len()
            );
            out.push_str(&line);
        }
        out
    }
}

/// 标准夹具：产物 v2.7.1；四域分别声明 v2.1/v2.5/v2.7/v1.9。
pub fn standard_matrix() -> CompatMatrix {
    let mut m = CompatMatrix::new(Ver::new(2, 7, 1));
    let _ = m.declare(2, Ver::new(2, 1, 0)); // 老消费方（内存域）
    let _ = m.declare(3, Ver::new(2, 5, 0)); // 次老（CPU 域）
    let _ = m.declare(4, Ver::new(2, 7, 0)); // 同步（驱动域）
    let _ = m.declare(5, Ver::new(1, 9, 0)); // 跨代老（报告域，必拒）
    m
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut m = CompatMatrix::new(Ver::new(3, 0, 0));
    // 1) 主版本跨代拒
    let _ = m.declare(2, Ver::new(2, 5, 0));
    if m.read(2) == Err(VC_MAJ_BREAK) {
        caught += 1;
    }
    // 2) 产物比声明还老拒（次版本前向不保证）
    let mut m2 = CompatMatrix::new(Ver::new(2, 3, 0));
    let _ = m2.declare(2, Ver::new(2, 5, 0));
    if m2.read(2) == Err(VC_MAJ_BREAK) {
        caught += 1;
    }
    // 3) 未声明域读拒
    if m2.read(7) == Err(VC_NOTDECL) {
        caught += 1;
    }
    // 4) 域越集拒
    if m2.declare(10, Ver::new(1, 0, 0)) == Err(VC_DOM_OOB) {
        caught += 1;
    }
    if m2.read(10) == Err(VC_DOM_OOB) {
        caught += 1;
    }
    // 5) 降级读：兼容域未知字段 → 跳过登记（Ok 路径）
    let mut m3 = standard_matrix();
    if m3.degrade_read(2, 0x0041) == Ok(1) {
        caught += 1;
    }
    // 6) 跨代域降级读仍受控拒（不崩溃，Err 返回）
    if m3.degrade_read(5, 0x0042) == Err(VC_MAJ_BREAK) {
        caught += 1;
    }
    // 7) 老版本读新 Manifest 零崩溃：v2.1 域读 v2.7 产物 = Ok + 幂等跳过
    if m3.read(2).is_ok() && m3.degrade_read(2, 0x0041) == Ok(1) {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx253_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0253");
    // J1-1 版本字段声明 + 支持矩阵建立
    let m = standard_matrix();
    let ok = m.rows.len() == 4 && !m.render().is_empty() && m.doc_ver.render() == "v2.7.1";
    s.add("B13-34-版本矩阵建立", ok, if ok { "" } else { "矩阵缺行" });
    // J1-2 老版本消费方读新 Manifest 零崩溃（v2.1 读 v2.7 = Ok）
    let m = standard_matrix();
    let ok = m.read(2).is_ok() && m.read(3).is_ok() && m.read(4).is_ok();
    s.add("B13-35-老读新零崩溃", ok, if ok { "" } else { "老读新误拒" });
    // J1-3 跨代拒（受控 Err，非崩溃）
    let m = standard_matrix();
    let ok = m.read(5) == Err(VC_MAJ_BREAK);
    s.add("B13-36-跨代受控拒", ok, if ok { "" } else { "跨代放行" });
    // J1-4 降级读跳过登记（认识照读 + 不认识跳过 + 幂等）
    let mut m = standard_matrix();
    let ok = m.degrade_read(2, 0x10) == Ok(1)
        && m.degrade_read(2, 0x11) == Ok(2)
        && m.degrade_read(2, 0x10) == Ok(2);
    s.add("B13-37-降级读跳过账", ok, if ok { "" } else { "跳过账漂移" });
    // J1-5 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-38-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx253_all_checks_green() {
        let set = run_unx253_checks();
        assert!(set.all_passed(), "UNX-F0253 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx253_ver_ordering() {
        assert!(Ver::new(2, 7, 1) > Ver::new(2, 7, 0));
        assert!(Ver::new(2, 7, 0) > Ver::new(2, 6, 9));
    }
}
