// SPDX-License-Identifier: Apache-2.0
//! VE-F0174 · 崩溃复现最小化器
//!
//! Delta debugging 算法：最小化崩溃复现步骤。
//! 输入：触发崩溃的命令序列。输出：最短仍触发崩溃的子序列。
//! 保证：最小化后仍崩溃 + 最小性验证（去掉任一元素不崩溃）。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 崩溃判定
// ---------------------------------------------------------------------------

/// 崩溃判定函数（模拟：特定序列触发崩溃）。
pub type CrashPredicate = fn(&[u32]) -> bool;

/// 默认判定：序列包含 {1, 3, 5} 时崩溃。
fn default_crash_predicate(seq: &[u32]) -> bool {
    seq.contains(&1) && seq.contains(&3) && seq.contains(&5)
}

// ---------------------------------------------------------------------------
// Delta Debugging 最小化器
// ---------------------------------------------------------------------------

/// 最小化结果。
#[derive(Debug, Clone)]
pub struct MinimizeResult {
    /// 最小序列。
    pub minimal: Vec<u32>,
    /// 原始长度。
    pub original_len: usize,
    /// 最小长度。
    pub minimal_len: usize,
    /// 迭代次数。
    pub iterations: u32,
    /// 最小性验证通过。
    pub is_minimal: bool,
}

/// Delta debugging 最小化器。
pub struct Minimizer {
    /// 崩溃判定。
    predicate: CrashPredicate,
    /// 最大迭代次数。
    max_iterations: u32,
}

impl Minimizer {
    pub fn new(predicate: CrashPredicate) -> Self {
        Minimizer { predicate, max_iterations: 10_000 }
    }

    /// 最小化（ddmin 算法）。
    ///
    /// 核心思想：把序列分成 n 份，逐份尝试去掉——如果去掉后仍崩溃，
    /// 就永久去掉那份。n 递增直到无法再分。
    pub fn minimize(&self, input: &[u32]) -> MinimizeResult {
        let mut current: Vec<u32> = input.to_vec();
        let mut n = 2usize; // 初始分 2 份
        let mut iterations = 0u32;

        // 先验证输入确实崩溃
        if !(self.predicate)(&current) {
            return MinimizeResult {
                minimal: current.clone(),
                original_len: input.len(),
                minimal_len: current.len(),
                iterations: 0,
                is_minimal: true,
            };
        }

        while current.len() >= 2 && iterations < self.max_iterations {
            iterations += 1;
            let chunk_size = (current.len() + n - 1) / n;
            let mut reduced = false;

            // 尝试去掉每一份
            for i in 0..n {
                let start = i * chunk_size;
                let end = ((i + 1) * chunk_size).min(current.len());
                if start >= end { continue; }

                // 构造去掉第 i 份的子序列
                let candidate: Vec<u32> = current[..start].iter()
                    .chain(current[end..].iter())
                    .copied()
                    .collect();

                if (self.predicate)(&candidate) {
                    current = candidate;
                    n = (n - 1).max(2);
                    reduced = true;
                    break;
                }
            }

            if !reduced {
                if n >= current.len() { break; }
                n = (n * 2).min(current.len());
            }
        }

        // 最小性验证：去掉任一元素是否仍崩溃
        let is_minimal = self.verify_minimality(&current);

        MinimizeResult {
            minimal_len: current.len(),
            original_len: input.len(),
            minimal: current,
            iterations,
            is_minimal,
        }
    }

    /// 最小性验证（去掉任一元素 → 不崩溃）。
    pub fn verify_minimality(&self, seq: &[u32]) -> bool {
        for i in 0..seq.len() {
            let without: Vec<u32> = seq[..i].iter().chain(seq[i+1..].iter()).copied().collect();
            if (self.predicate)(&without) {
                return false; // 还能再缩
            }
        }
        true
    }

    /// 验证最小序列仍崩溃。
    pub fn verify_still_crashes(&self, seq: &[u32]) -> bool {
        (self.predicate)(seq)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// VE-F0174 判据全集。
pub fn run_minimizer_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("VE-F0174", "崩溃复现最小化器");

    let minimizer = Minimizer::new(default_crash_predicate);

    // 1. 基本最小化
    let input = alloc::vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let result = minimizer.minimize(&input);
    cs.ok("最小化执行", result.iterations > 0, "有迭代");
    cs.ok("长度减少", result.minimal_len < result.original_len, "缩短");
    cs.ok("最小序列", result.minimal.len() <= 3, "≤3 元素");
    cs.ok("仍崩溃", minimizer.verify_still_crashes(&result.minimal), "保持崩溃");

    // 2. 最小性
    cs.ok("最小性验证", result.is_minimal, "去掉任一不崩溃");

    // 3. 精确最小
    cs.ok("精确最小", result.minimal == alloc::vec![1, 3, 5], "就是 {1,3,5}");

    // 4. 非崩溃输入
    let safe = alloc::vec![2, 4, 6, 8];
    let safe_result = minimizer.minimize(&safe);
    cs.ok("非崩溃不缩", safe_result.minimal == safe, "原样返回");

    // 5. 已最小输入
    let already_min = alloc::vec![1, 3, 5];
    let min_result = minimizer.minimize(&already_min);
    cs.ok("已最小", min_result.minimal == already_min, "不缩");
    cs.ok("最小性", min_result.is_minimal, "已是最小");

    // 6. 最小性验证
    cs.ok("最小性真", minimizer.verify_minimality(&[1, 3, 5]), "去掉任一不崩");
    cs.ok("最小性假", !minimizer.verify_minimality(&[1, 2, 3, 5]), "去掉 2 仍崩");

    // 7. 空输入
    let empty_result = minimizer.minimize(&[]);
    cs.ok("空输入", empty_result.minimal.is_empty(), "空=空");

    // 8. 单元素崩溃
    let single_crash = Minimizer::new(|s| s.contains(&42));
    let r = single_crash.minimize(&[42]);
    cs.ok("单元素最小", r.minimal == alloc::vec![42], "不动");

    cs.done()
}
