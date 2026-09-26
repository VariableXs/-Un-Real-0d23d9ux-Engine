
// ---------------------------------------------------------------------------
// F008 · 深化批次八：页范围解析（打印对话框「页码范围」语法——
// `1-3,5,8-` 三段式：区间/单页/开区间到末页；空 = 全部）。
// ---------------------------------------------------------------------------

/// 解析结果（PrintDlg PAGESETUP 语义面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageRange {
    pub all: bool,
    pub min: u32,
    pub max: u32,
    /// 选中页数（开区间按 max 封顶计；all 时 0 表示未用此字段）。
    pub count: u32,
}

/// 解析页范围串：`1-3,5,8-`（末尾开区间 = 到 max）；非法段整串拒绝
/// （不静默跳过——用户输入的字面责任）。空串 = 全部。
pub fn parse_page_range(s: &str, doc_max: u32) -> Option<PageRange> {
    let t = s.trim();
    if t.is_empty() {
        return Some(PageRange { all: true, min: 1, max: doc_max, count: 0 });
    }
    let mut min = u32::MAX;
    let mut max_seen = 0u32;
    let mut count = 0u32;
    for seg in t.split(',') {
        let seg = seg.trim();
        let (a, b) = match seg.split_once('-') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (seg, None),
        };
        let start: u32 = a.parse().ok()?;
        if start == 0 {
            return None; // 页码从 1 起
        }
        let end = match b {
            None => start,
            Some(x) if x.is_empty() => doc_max, // 开区间
            Some(x) => {
                let e: u32 = x.parse().ok()?;
                if e < start {
                    return None; // 倒序区间拒绝
                }
                e
            }
        };
        if end > doc_max {
            return None; // 超文档页数如实拒（不静默钳制——打印的是用户的纸）
        }
        min = min.min(start);
        max_seen = max_seen.max(end);
        count += end - start + 1;
    }
    Some(PageRange { all: false, min, max: max_seen, count })
}

/// F008 深化批次八自检。
fn run_comdlg_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F008-comdlg-deep7");
    // 1) 三段式：1-3,5,8- (doc_max=10) → min1 max10 count 3+1+3=7。
    let r = parse_page_range("1-3,5,8-", 10);
    cs.add(
        "page_range_three_segments",
        matches!(r, Some(p) if p.min == 1 && p.max == 10 && p.count == 7 && !p.all),
        "",
    );
    // 2) 空串 = 全部；单页 = count 1。
    let all = parse_page_range("", 10);
    let one = parse_page_range("7", 10);
    cs.add(
        "page_range_all_and_single",
        matches!(all, Some(p) if p.all)
            && matches!(one, Some(p) if p.min == 7 && p.max == 7 && p.count == 1),
        "",
    );
    // 3) 非法四向：倒序区间 / 0 页 / 超文档 / 非数字——全拒绝。
    let bad1 = parse_page_range("5-3", 10);
    let bad2 = parse_page_range("0", 10);
    let bad3 = parse_page_range("11", 10);
    let bad4 = parse_page_range("1,x", 10);
    cs.add(
        "page_range_rejects_invalid",
        bad1.is_none() && bad2.is_none() && bad3.is_none() && bad4.is_none(),
        "",
    );
    cs
}
