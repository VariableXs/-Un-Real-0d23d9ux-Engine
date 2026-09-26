//! F027 深化批次二 · 组合属性段压缩与候选排序面（compatstar2/deep · G-A-27）。
//!
//! 批次一深化覆盖 WM_IME_*/HIMC/CANDIDATELIST 翻页；本批补齐：组合属性段
//! 压缩（GCS_COMPATTR 的游程编码——下划线渲染段的最小传输形态）、候选稳定
//! 排序（词频降序、同频保序——「同输入同结果」可复现排序的承载面）、
//! EXFORMINFO 越界钳制（尊重程序位置但不许飞出工作区）、UTF-16 单元记账
//! （表情候选的代理对长度）。
//!
//! 零堆纪律：定长游程表与栈内排序，无 alloc。

use crate::checks::CheckSet;

/// 属性游程表容量。
pub const MAX_ATTR_RUNS: usize = 8;
/// 候选容量（与主面 MAX_CANDIDATES 同口径）。
pub const D2_MAX_CANDIDATES: usize = 9;

/// 一段连续同属性区（GCS_COMPATTR 游程）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrRun {
    pub start: u8,
    pub len: u8,
    pub attr: u8,
}

/// 属性数组 → 游程压缩（下划线渲染段的最小传输形态；段数 ≤ 8）。
pub fn compress_attr_runs(attrs: &[u8], out: &mut [AttrRun; MAX_ATTR_RUNS]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < attrs.len() {
        let attr = attrs[i];
        let mut len = 1usize;
        while i + len < attrs.len() && attrs[i + len] == attr {
            len += 1;
        }
        if n < MAX_ATTR_RUNS {
            out[n] = AttrRun { start: i as u8, len: len as u8, attr };
            n += 1;
        }
        i += len;
    }
    n
}

/// 候选稳定排序：词频降序、同频保序（插入排序——同输入同结果的
/// 可复现排序面；返回重排后的下标序列）。
pub fn stable_rank(freqs: &[u32], out_idx: &mut [usize; D2_MAX_CANDIDATES]) -> usize {
    let n = freqs.len().min(D2_MAX_CANDIDATES);
    for (i, slot) in out_idx.iter_mut().enumerate().take(n) {
        *slot = i;
    }
    for i in 1..n {
        let cur = out_idx[i];
        let mut j = i;
        while j > 0 && freqs[out_idx[j - 1]] < freqs[cur] {
            out_idx[j] = out_idx[j - 1]; // 严格小于才前移 → 同频保序（稳定）
            j -= 1;
        }
        out_idx[j] = cur;
    }
    n
}

/// EXFORMINFO（程序提供的候选窗期望位形）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExFormInfo {
    pub pt_x: i32,
    pub pt_y: i32,
    /// 工作区边界 (x0, y0, x1, y1)——程序窗口客户区。
    pub area: (i32, i32, i32, i32),
}

/// 候选窗落点：EXFORMINFO 在工作区内则尊重，越界钳回边界内
/// （尊重程序但不许飞出——主册【交互设计】的执行面）。
pub fn clamp_exform(ex: &ExFormInfo) -> (i32, i32) {
    let (x0, y0, x1, y1) = ex.area;
    let x = ex.pt_x.clamp(x0, x1);
    let y = ex.pt_y.clamp(y0, y1);
    (x, y)
}

/// UTF-16 单元记账：BMP = 1 单元、代理对 = 2 单元（表情候选的长度账）。
pub fn utf16_units(code_point: u32) -> usize {
    if code_point > 0xFFFF {
        2
    } else {
        1
    }
}

/// 域自检（深化批次二）。
pub fn run_f027d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F027-imm32-d2");
    // 1) 属性段压缩：[0,0,1,1,1,0] → 三段 (0,2,0)/(2,3,1)/(5,1,0)。
    let mut runs = [AttrRun { start: 0, len: 0, attr: 0 }; MAX_ATTR_RUNS];
    let n = compress_attr_runs(&[0, 0, 1, 1, 1, 0], &mut runs);
    cs.add(
        "attr_run_compress",
        n == 3
            && runs[0] == AttrRun { start: 0, len: 2, attr: 0 }
            && runs[1] == AttrRun { start: 2, len: 3, attr: 1 }
            && runs[2] == AttrRun { start: 5, len: 1, attr: 0 },
        "",
    );
    // 2) 候选稳定排序：频 [5,9,9,1] → 序 [1,2,0,3]（同频 9 保 1 先于 2）。
    let mut idx = [0usize; D2_MAX_CANDIDATES];
    let n2 = stable_rank(&[5, 9, 9, 1], &mut idx);
    cs.add("stable_rank", n2 == 4 && idx[0] == 1 && idx[1] == 2 && idx[2] == 0 && idx[3] == 3, "");
    // 3) EXFORMINFO 钳制：区内尊重、越界钳回。
    let ok = ExFormInfo { pt_x: 300, pt_y: 200, area: (0, 0, 800, 600) };
    let out_of_area = ExFormInfo { pt_x: 900, pt_y: -5, area: (0, 0, 800, 600) };
    cs.add(
        "exform_clamp",
        clamp_exform(&ok) == (300, 200) && clamp_exform(&out_of_area) == (800, 0),
        "",
    );
    // 4) UTF-16 记账：你 U+4F60 = 1 单元、😀 U+1F600 = 2 单元。
    cs.add("utf16_units", utf16_units(0x4F60) == 1 && utf16_units(0x1F600) == 2 && utf16_units(0xFFFF) == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attr_single_run_uniform() {
        let mut runs = [AttrRun { start: 0, len: 0, attr: 0 }; MAX_ATTR_RUNS];
        let n = compress_attr_runs(&[1, 1, 1], &mut runs);
        assert_eq!(n, 1);
        assert_eq!(runs[0], AttrRun { start: 0, len: 3, attr: 1 });
    }

    #[test]
    fn rank_deterministic_repeat() {
        // 同输入两次调用同结果（「同输入同结果」排序因子的直接对拍）。
        let mut a = [0usize; D2_MAX_CANDIDATES];
        let mut b = [0usize; D2_MAX_CANDIDATES];
        stable_rank(&[3, 7, 7, 7, 2], &mut a);
        stable_rank(&[3, 7, 7, 7, 2], &mut b);
        assert_eq!(a[..5], b[..5], "稳定排序可复现");
        assert_eq!(&a[..5], &[1, 2, 3, 0, 4]);
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f027d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
