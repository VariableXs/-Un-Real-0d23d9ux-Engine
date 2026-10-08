//! H2 打印作业参数模型 · 深化批次六（F289 深化——份数/页码范围/
//! 双面的参数校验与页数核算，队列中心的作业描述单）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F289 打印队列中心**：作业描述单的字段级校验（页码范围
//!   「1-3,5」解析、份数钳制、双面折算）——参数错在提交点拒绝
//!   （人话原因），不进队列（队列状态机只见过合法作业）；
//! - **八章「诚实进度」**：页数核算是进度的分母——范围解析错了
//!   进度条就撒谎，所以解析与核算在提交前机判。
//!
//! 纯函数：解析与核算无状态。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 页码范围解析
// ---------------------------------------------------------------------------

/// 页码范围解析：`"1-3,5"` → 有序去重页表；`"all"` → 全量。
/// 非法段（倒序/越界/空）→ Err（人话原因——提交点拒绝）。
pub fn parse_pages(spec: &str, doc_pages: u32) -> Result<Vec<u32>, &'static str> {
    if doc_pages == 0 {
        return Err("文档没有可打印的页");
    }
    let spec = spec.trim();
    if spec.is_empty() || spec.eq_ignore_ascii_case("all") {
        return Ok((1..=doc_pages).collect());
    }
    let mut pages = Vec::new();
    for seg in spec.split(',') {
        let seg = seg.trim();
        if seg.is_empty() {
            return Err("页码范围里有空段——检查逗号");
        }
        if let Some((a, b)) = seg.split_once('-') {
            let (a, b) = (a.trim(), b.trim());
            let (pa, pb) = (a.parse::<u32>(), b.parse::<u32>());
            match (pa, pb) {
                (Ok(pa), Ok(pb)) => {
                    if pa == 0 || pb == 0 {
                        return Err("页码从 1 开始——没有第 0 页");
                    }
                    if pa > pb {
                        return Err("范围倒序（如 5-2）——请从小到大写");
                    }
                    if pb > doc_pages {
                        return Err("页码超出文档页数——请检查范围");
                    }
                    pages.extend(pa..=pb);
                }
                _ => return Err("页码范围格式不对——示例：1-3,5"),
            }
        } else {
            match seg.parse::<u32>() {
                Ok(0) => return Err("页码从 1 开始——没有第 0 页"),
                Ok(p) if p <= doc_pages => pages.push(p),
                Ok(_) => return Err("页码超出文档页数——请检查范围"),
                Err(_) => return Err("页码不是数字——示例：1-3,5"),
            }
        }
    }
    pages.sort_unstable();
    pages.dedup();
    if pages.is_empty() {
        return Err("没有选出任何页");
    }
    Ok(pages)
}

/// 双面折算：N 页单面 = ⌈N/2⌉ 张纸（双面打印的纸张核算）。
pub fn sheets_needed(pages: usize, duplex: bool) -> usize {
    if duplex {
        (pages + 1) / 2
    } else {
        pages
    }
}

/// 总产出页数（份数 × 页数——进度分母）。
pub fn total_output(pages: usize, copies: u32) -> usize {
    pages * copies.max(1) as usize
}

/// 份数钳制（F289 队列口径：1..=999——0 与 4 位数拒绝）。
pub fn clamp_copies(copies: u32) -> Result<u32, &'static str> {
    if copies == 0 {
        Err("份数至少 1 份")
    } else if copies > 999 {
        Err("份数最多 999 份——大量打印请分批")
    } else {
        Ok(copies)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2prnparams_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2prnparams");
    // 范围解析：all / 单页 / 区间 / 混合 / 去重排序。
    set.add(
        "h2prnparams parse all",
        parse_pages("all", 5) == Ok(vec![1, 2, 3, 4, 5])
            && parse_pages("", 3) == Ok(vec![1, 2, 3]),
        "all & empty",
    );
    set.add(
        "h2prnparams parse mixed",
        parse_pages("5,1-3,1", 10) == Ok(vec![1, 2, 3, 5]),
        "dedup + sort",
    );
    // 非法族：倒序/越界/0/非数字/空段——逐条人话拒绝。
    set.add(
        "h2prnparams rejects",
        parse_pages("5-2", 10).unwrap_err().contains("倒序")
            && parse_pages("9-12", 10).unwrap_err().contains("超出")
            && parse_pages("0", 10).unwrap_err().contains("从 1 开始")
            && parse_pages("abc", 10).unwrap_err().contains("不是数字")
            && parse_pages("1,,2", 10).unwrap_err().contains("空段"),
        "human reasons",
    );
    // 零页文档拒绝。
    set.add("h2prnparams zero doc", parse_pages("all", 0).is_err(), "no pages no job");
    // 双面折算：奇偶两判例。
    set.add(
        "h2prnparams duplex",
        sheets_needed(10, true) == 5 && sheets_needed(7, true) == 4 && sheets_needed(7, false) == 7,
        "ceil half",
    );
    // 产出核算：份数乘法；0 份钳 1。
    set.add(
        "h2prnparams output",
        total_output(10, 3) == 30 && total_output(10, 0) == 10,
        "copies floor 1",
    );
    // 份数钳制边界。
    set.add(
        "h2prnparams copies clamp",
        clamp_copies(0).is_err()
            && clamp_copies(1000).is_err()
            && clamp_copies(1) == Ok(1)
            && clamp_copies(999) == Ok(999),
        "1..=999",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2prnparams_all_green() {
        let set = run_h2prnparams_checks();
        assert!(set.all_passed(), "h2prnparams 自检有红项");
        assert!(!set.truncated(), "h2prnparams 自检溢出");
    }

    #[test]
    fn parse_never_exceeds_doc() {
        // 疯狂范围串 fuzz：合法解析结果的页码全部落在文档内（越界
        // 页码进队列=进度撒谎——结构上不可能）。
        let specs = ["all", "1-100", "50-60", "1,1,1,1", "7", "1-5,5-9"];
        for s in specs {
            if let Ok(pages) = parse_pages(s, 10) {
                assert!(pages.iter().all(|p| (1..=10).contains(p)));
                pages.windows(2).for_each(|w| assert!(w[0] < w[1]));
            }
        }
    }
}
