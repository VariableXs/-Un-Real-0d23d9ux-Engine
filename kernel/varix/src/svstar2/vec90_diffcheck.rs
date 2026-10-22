//! VE-F0490 · 优化正确性对拍框架（VE-C 域 · 优化组终件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0490`
//!
//! **锚点原文**：「优化正确性对拍框架：对拍基准（优化前 IR 与优化后 IR 同
//! 输入执行对比——软渲解释器为参考执行器）、抽样策略（每 pass 后抽样加全
//! 量关键场景）、失败定位（差异定位到首个分歧指令）；对拍结果入优化质量账。
//! 判据：参考执行器、双层抽样、首违定位、N/A 显性、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **参考执行器**（判据一）。参考执行面（软渲解释器替身：同输入双跑
//!    优化前后 IR——终值面投影对比）。
//!
//! 2. **双层抽样**（判据二）。抽样两层：每 pass 后抽样（批面）+ 全量关键
//!    场景（关键面全跑——双层覆盖账）。
//!
//! 3. **首违定位**（判据三）。差异定位到首个分歧指令（投影序内第一个不同
//!    值即定位——序号即指令位）。
//!
//! 4. **N/A 显性**（判据四）。执行器不可用（软渲缺席）→ N/A 显性（不算通
//!    过——N/A ≠ pass），对拍结果入优化质量账。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0490";

// ---------------------------------------------------------------------------
// 参考执行器（软渲解释器替身）
// ---------------------------------------------------------------------------

/// 参考执行器可用性（软渲缺席 → N/A 显性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecAvail {
    Available,
    Unavailable,
}

/// IR 步进指令（解释器面：op + 操作数值 → 结果值）。
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Const(i64),
    Add(i64, i64),
    Sub(i64, i64),
    Mul(i64, i64),
    /// 不可解释指令（执行器能力外——N/A 触发面）。
    Unsupported,
}

/// 参考执行：指令序 → 终值投影（Ok(投影) / Err(N/A)）。
pub fn reference_exec(steps: &[Step]) -> Result<Vec<i64>, &'static str> {
    let mut out: Vec<i64> = Vec::new();
    for s in steps.iter() {
        match s {
            Step::Const(v) => out.push(*v),
            Step::Add(a, b) => out.push(a.wrapping_add(*b)),
            Step::Sub(a, b) => out.push(a.wrapping_sub(*b)),
            Step::Mul(a, b) => out.push(a.wrapping_mul(*b)),
            Step::Unsupported => return Err("执行器不可用：N/A 显性（不算通过）"),
        }
    }
    Ok(out)
}

/// 对拍：同输入双跑 → 一致 / 首违定位（序号）。
pub fn diff_of(before: &[i64], after: &[i64]) -> Result<(), (usize, i64, i64)> {
    let n = before.len().min(after.len());
    for i in 0..n {
        if before[i] != after[i] {
            return Err((i, before[i], after[i]));
        }
    }
    if before.len() != after.len() {
        return Err((n, before.len() as i64, after.len() as i64));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 对拍框架（双层抽样 + 质量账）
// ---------------------------------------------------------------------------

/// 对拍质量账。
#[derive(Debug, Default)]
pub struct DiffBook {
    /// 批面抽样计数（每 pass 后）。
    pub batch_samples: usize,
    /// 关键场景全量计数。
    pub key_scenes: usize,
    pub passed: usize,
    pub failed: Vec<String>,
    /// N/A 计数（显性——不算通过）。
    pub na_count: usize,
}

impl DiffBook {
    pub fn new() -> Self {
        Self::default()
    }
    /// 单次对拍（可用性驱动：N/A 入账不算通过）。
    pub fn compare(
        &mut self,
        name: &str,
        before: &[Step],
        after: &[Step],
        avail: ExecAvail,
    ) -> bool {
        if avail == ExecAvail::Unavailable {
            self.na_count += 1;
            return false; // N/A ≠ 通过
        }
        let b = match reference_exec(before) {
            Ok(x) => x,
            Err(_) => {
                self.na_count += 1;
                return false;
            }
        };
        let a = match reference_exec(after) {
            Ok(x) => x,
            Err(_) => {
                self.na_count += 1;
                return false;
            }
        };
        match diff_of(&b, &a) {
            Ok(()) => {
                self.passed += 1;
                true
            }
            Err((i, vb, va)) => {
                self.failed.push(format!(
                    "{}：首违指令 #{}（前 {} ≠ 后 {}）",
                    name, i, vb, va
                ));
                false
            }
        }
    }
    /// 双层覆盖判定：批抽样 ≥ 门槛 且 关键场景全量（key ≥ 1）。
    pub fn covered(&self, min_batch: usize) -> bool {
        self.batch_samples >= min_batch && self.key_scenes >= 1
    }
    /// 质量结论：覆盖达标 + 零失败 + 零 N/A（N/A 显性阻断）。
    pub fn verdict(&self, min_batch: usize) -> Result<(), String> {
        if !self.covered(min_batch) {
            return Err(format!(
                "覆盖不足：批抽样 {} < {} 或关键场景 {} 缺",
                self.batch_samples, min_batch, self.key_scenes
            ));
        }
        if !self.failed.is_empty() {
            return Err(self.failed[0].clone());
        }
        if self.na_count > 0 {
            return Err(format!("N/A 显性 {} 条（不算通过——阻断）", self.na_count));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec90_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、参考执行器。
    {
        // 基本执行：const/add/mul 投影。
        let steps = vec![Step::Const(2), Step::Const(3), Step::Add(2, 3), Step::Mul(4, 5)];
        let r = reference_exec(&steps);
        v.push((
            "F0490-执行器-基本投影",
            r == Ok(vec![2, 3, 5, 20]),
        ));
        // 能力外指令 → N/A Err。
        let bad = vec![Step::Const(1), Step::Unsupported];
        v.push((
            "F0490-执行器-能力外N/A",
            reference_exec(&bad).is_err(),
        ));
    }

    // 二、对拍 + 首违定位。
    {
        // 一致：双跑同投影。
        let same = diff_of(&[1, 2, 3], &[1, 2, 3]);
        v.push(("F0490-对拍-一致", same.is_ok()));
        // 分歧：定位首个不同序号。
        let d = diff_of(&[1, 2, 3], &[1, 9, 3]);
        v.push((
            "F0490-对拍-首违定位",
            d == Err((1, 2, 9)),
        ));
        // 长度差：定位到溢出点。
        let d2 = diff_of(&[1, 2], &[1, 2, 3]);
        v.push(("F0490-对拍-长度差定位", d2 == Err((2, 2, 3))));
    }

    // 三、双层抽样。
    {
        let mut book = DiffBook::new();
        // 批面抽样 8 次（全过）。
        let steps = vec![Step::Const(2), Step::Add(2, 3)];
        for i in 0..8 {
            book.batch_samples += 1;
            let _ = book.compare(&format!("s{}", i), &steps, &steps, ExecAvail::Available);
        }
        // 关键场景 2 个全量。
        book.key_scenes += 2;
        let _ = book.compare("key1", &steps, &steps, ExecAvail::Available);
        let _ = book.compare("key2", &steps, &steps, ExecAvail::Available);
        v.push((
            "F0490-抽样-双层覆盖",
            book.covered(8) && book.passed == 10 && book.verdict(8).is_ok(),
        ));
        // 覆盖不足：关键场景缺 → 阻断。
        let mut b2 = DiffBook::new();
        b2.batch_samples = 8;
        v.push((
            "F0490-抽样-关键缺阻断",
            !b2.covered(8) && b2.verdict(8).is_err(),
        ));
    }

    // 四、N/A 显性。
    {
        let mut book = DiffBook::new();
        book.batch_samples = 4;
        book.key_scenes = 1;
        // 执行器不可用 → N/A 入账且不算通过。
        let steps = vec![Step::Const(1)];
        let r = book.compare("na_case", &steps, &steps, ExecAvail::Unavailable);
        v.push((
            "F0490-NA-显性不算通过",
            !r && book.na_count == 1 && book.passed == 0,
        ));
        // verdict：N/A 阻断（覆盖足也阻）。
        let v1 = book.verdict(4);
        v.push((
            "F0490-NA-verdict阻断",
            matches!(&v1, Err(msg) if msg.contains("N/A")),
        ));
        // 能力外指令同 N/A 面。
        let mut book2 = DiffBook::new();
        book2.batch_samples = 4;
        book2.key_scenes = 1;
        let sup = vec![Step::Unsupported];
        let r2 = book2.compare("sup_case", &sup, &sup, ExecAvail::Available);
        v.push(("F0490-NA-能力外同面", !r2 && book2.na_count == 1));
    }

    // 五、失败入账 + 判据自检。
    {
        let mut book = DiffBook::new();
        book.batch_samples = 4;
        book.key_scenes = 1;
        // 分歧样例：优化前后不等价 → 失败账带首违定位。
        let before = vec![Step::Const(2), Step::Mul(2, 3)];
        let after = vec![Step::Const(2), Step::Mul(2, 4)];
        let r = book.compare("fold_bug", &before, &after, ExecAvail::Available);
        v.push((
            "F0490-失败-账带定位",
            !r && book.failed.len() == 1 && book.failed[0].contains("fold_bug") && book.failed[0].contains("#1"),
        ));
        v.push(("F0490-判据-锚点入册", ANCHOR == "VE-F0490"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec90）。
pub fn run_vec90_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec90");
    for (name, ok) in vec90_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec90_raw_checks as run_vec90_checks;
    #[test]
    fn vec90_diffcheck_checks() {
        for (name, ok) in run_vec90_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
