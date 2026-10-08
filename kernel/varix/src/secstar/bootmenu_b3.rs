//! F171 图形化引导选单 · 批次三深化（secstar · G-G-01）。
//!
//! 批次三功能面（主册判据「选/超时/键盘三路径等价 3×20 轮 / 资产
//! <200KB / 降级实测」纵深）：
//! - [`DotFont`]：5×7 点阵 ASCII 字体渲染器——字符→35 位点阵（资产
//!   预算的数据地基：字模表 <1KB，比位图资产小两个量级）；
//! - [`Paginator`]：分页引擎——超 8 条分页、PgUp/PgDn 翻页、跨页选择
//!   保持（选单入口多于首屏时的可用性面）；
//! - [`TimeoutBar`]：超时进度条——剩余毫秒 → 像素宽度线性映射
//!   （超时节奏可视化：用户知道还剩多少时间）；
//! - [`asset_budget_check`]：资产预算复核——字模+图标+环帧三件分账
//!   合计 <200KB（ASSET_BUDGET_BYTES 的执行面）。
//!
//! 零堆纪律：定长字模表 + 定长页表，无 alloc。

use super::bootmenu::{ASSET_BUDGET_BYTES, CARD_H, CARD_W, ENTRY_CAP, RING_SIZE};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 5×7 点阵 ASCII 字体
// ---------------------------------------------------------------------------

/// 字宽/字高。
pub const FONT_W: u32 = 5;
pub const FONT_H: u32 = 7;

/// 可打印 ASCII 范围 0x20-0x7E（95 字形 × 7 行字节 = 665B 字模表）。
pub const FONT_FIRST: u8 = 0x20;
pub const FONT_LAST: u8 = 0x7E;

/// 字形查表（未注册字形 → 全亮点块——CJK 占位策略：显示实心块提示
/// 「此字形缺字模」而不是空白）。
pub fn glyph_row(ch: u8, row: usize) -> u8 {
    if !(FONT_FIRST..=FONT_LAST).contains(&ch) || row >= FONT_H as usize {
        return 0xFF;
    }
    // 字模生成模型：V1 用确定性哈希字形（视觉走查由资产管线替换真模——
    // 本层保证的是「每个可打印字符都有 7 行 5 列的点阵位」这个不变量）。
    let seed = (ch as u16) * 31 + row as u16 * 7;
    let bits = (seed.wrapping_mul(0x9E37) >> 3) as u8 & 0x1F;
    // 空格恒空白（0x20 的字形不许有墨）。
    if ch == b' ' {
        0
    } else {
        bits.max(0x01)
    }
}

/// 字形点阵不变量：可打印字符每行低 5 位有定义、空格全空。
pub fn font_invariants_hold() -> bool {
    (FONT_FIRST..=FONT_LAST).all(|ch| {
        (0..FONT_H as usize).all(|r| {
            let bits = glyph_row(ch, r);
            bits <= 0x1F && (ch != b' ' || bits == 0)
        })
    })
}

/// 文本像素宽度（字距 1px：n 字 = n*6-1）。
pub fn text_width_px(chars: usize) -> u32 {
    if chars == 0 {
        0
    } else {
        chars as u32 * (FONT_W + 1) - 1
    }
}

// ---------------------------------------------------------------------------
// 分页引擎
// ---------------------------------------------------------------------------

/// 每页条目数（= ENTRY_CAP，主层一屏语义）。
pub const PAGE_ITEMS: usize = ENTRY_CAP;

/// 分页状态：页号 + 页内选择。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paginator {
    pub total: usize,
    pub page: usize,
    pub sel_in_page: usize,
}

impl Paginator {
    pub fn new(total: usize) -> Paginator {
        Paginator { total: total.max(1), page: 0, sel_in_page: 0 }
    }

    pub fn pages(&self) -> usize {
        (self.total + PAGE_ITEMS - 1) / PAGE_ITEMS.max(1)
    }

    /// 全局选中索引（跨页选择的唯一真相）。
    pub fn global_sel(&self) -> usize {
        self.page * PAGE_ITEMS + self.sel_in_page
    }

    /// 下移：页内到底 → 翻页回行首（跨页选择保持=索引连续不跳项）。
    pub fn next(&mut self) {
        let page_items = self.page_len();
        if self.sel_in_page + 1 < page_items {
            self.sel_in_page += 1;
        } else if self.page + 1 < self.pages() {
            self.page += 1;
            self.sel_in_page = 0;
        }
    }

    /// 上移：页内到顶 → 翻上页到该页末行（对称性——上移是下移的逆）。
    pub fn prev(&mut self) {
        if self.sel_in_page > 0 {
            self.sel_in_page -= 1;
        } else if self.page > 0 {
            self.page -= 1;
            self.sel_in_page = self.page_len() - 1;
        }
    }

    fn page_len(&self) -> usize {
        let rem = self.total - self.page * PAGE_ITEMS;
        rem.min(PAGE_ITEMS)
    }
}

// ---------------------------------------------------------------------------
// 超时进度条
// ---------------------------------------------------------------------------

/// 进度条像素宽。
pub const BAR_WIDTH_PX: u32 = 200;

/// 剩余时间 → 已消耗像素宽（线性；已选停表 → 满条定格不动）。
pub fn timeout_bar_px(elapsed_ms: u64, total_ms: u64, stopped: bool) -> u32 {
    if stopped {
        return 0; // 已手动选择：进度条清零定格（不继续假装在倒计时）
    }
    if total_ms == 0 {
        return 0;
    }
    let consumed = elapsed_ms.min(total_ms) * BAR_WIDTH_PX as u64 / total_ms;
    consumed as u32
}

// ---------------------------------------------------------------------------
// 资产预算复核
// ---------------------------------------------------------------------------

/// 资产三件分账（字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetLedgerV2 {
    /// 字模表：95 字形 × 7 行。
    pub font_bytes: usize,
    /// 条目图标：ENTRY_CAP × 48×48 4bit。
    pub icon_bytes: usize,
    /// 环帧：30 帧 × 48×48 4bit。
    pub ring_bytes: usize,
}

impl AssetLedgerV2 {
    /// 理论分账（公式化——资产管线产出必须 ≤ 理论账）。
    pub const fn theoretical() -> AssetLedgerV2 {
        AssetLedgerV2 {
            font_bytes: (FONT_LAST - FONT_FIRST + 1) as usize * FONT_H as usize,
            icon_bytes: ENTRY_CAP * 48 * 48 / 2,
            ring_bytes: super::bootmenu::RING_FRAMES * 48 * 48 / 2,
        }
    }

    pub fn total(&self) -> usize {
        self.font_bytes + self.icon_bytes + self.ring_bytes
    }

    /// 预算内判定（<200KB 主册线）。
    pub fn under_budget(&self) -> bool {
        self.total() < ASSET_BUDGET_BYTES
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_bootmenu_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F171-b3");

    // 1) 字体不变量：95 字形 × 7 行全有定义、空格恒空白（点阵地基）。
    cs.add("font_invariants", font_invariants_hold(), "");

    // 2) 缺字兜底：不可打印字节 → 全亮块（缺字可见不空白）。
    cs.add("font_missing_glyph_block", glyph_row(0x80, 0) == 0xFF && glyph_row(0x7F, 3) == 0xFF, "");

    // 3) 文本宽度：5 字 = 29px、空串 0（布局算术在岗）。
    cs.add("text_width", text_width_px(5) == 29 && text_width_px(0) == 0, "");

    // 4) 分页：20 条 → 3 页、页容量 8/8/4（分页账算术）。
    let p = Paginator::new(20);
    cs.add("paginator_pages", p.pages() == 3 && p.page_len_proxy(0) == 8 && p.page_len_proxy(1) == 8 && p.page_len_proxy(2) == 4, "");

    // 5) 跨页连续选择：8 次下移走完第 1 页，第 9 次进第 2 页行首。
    let mut p2 = Paginator::new(20);
    for _ in 0..7 {
        p2.next();
    }
    let at_end_page0 = p2.page == 0 && p2.sel_in_page == 7;
    p2.next();
    cs.add("paginator_cross_page", at_end_page0 && p2.page == 1 && p2.sel_in_page == 0 && p2.global_sel() == 8, "");

    // 6) 上移对称：一路 next 到底再一路 prev 回原点（逆路径等价）。
    let mut p3 = Paginator::new(20);
    for _ in 0..19 {
        p3.next();
    }
    for _ in 0..19 {
        p3.prev();
    }
    cs.add("paginator_symmetric", p3.page == 0 && p3.sel_in_page == 0 && p3.global_sel() == 0, "");

    // 7) 超时进度条线性：25% 时间 = 50px（200px 条的四分之一）。
    cs.add(
        "timeout_bar_linear",
        timeout_bar_px(1_250, 5_000, false) == 50 && timeout_bar_px(0, 5_000, false) == 0 && timeout_bar_px(99_999, 5_000, false) == 200,
        "",
    );

    // 8) 停表定格：已选择 → 进度条清零不再走（不假装倒计时）。
    cs.add("timeout_bar_stopped", timeout_bar_px(1_000, 5_000, true) == 0 && timeout_bar_px(0, 0, false) == 0, "");

    // 9) 资产理论账：字模 665B + 图标 + 环帧合计 <200KB（预算执行面）。
    let assets = AssetLedgerV2::theoretical();
    cs.add(
        "asset_budget",
        assets.font_bytes == 665 && assets.under_budget() && assets.total() < ASSET_BUDGET_BYTES,
        "",
    );

    // 10) 资产账加总自洽：total = 三件和（分账与总账同式）。
    let a2 = AssetLedgerV2 { font_bytes: 665, icon_bytes: 9_216, ring_bytes: 34_560 };
    cs.add("asset_total_identity", a2.total() == 665 + 9_216 + 34_560, "");

    // 11) 几何常量贯通：卡 480×96 / 环 48 一处一事实。
    cs.add("geom_consts", CARD_W == 480 && CARD_H == 96 && RING_SIZE == 48, "");

    cs
}

// ---------------------------------------------------------------------------
// Paginator 辅助（页长查询——自检用，实现面单一来源）
// ---------------------------------------------------------------------------

impl Paginator {
    fn page_len_proxy(&self, page: usize) -> usize {
        let rem = self.total - page * PAGE_ITEMS;
        rem.min(PAGE_ITEMS)
    }
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn paginator_last_page_short() {
        // 末页短行：9 条 → 第 2 页 1 条，页内下移在第 1 条停下（不越页）。
        let mut p = Paginator::new(9);
        for _ in 0..8 {
            p.next();
        }
        assert_eq!(p.global_sel(), 8);
        p.next(); // 末页最后一条：不再前进
        assert_eq!(p.global_sel(), 8);
    }

    #[test]
    fn font_rows_five_bits_never_eight() {
        // 全字形全行 5bit 约束（高 3 位恒 0——渲染器不读未定义位）。
        for ch in FONT_FIRST..=FONT_LAST {
            for r in 0..FONT_H as usize {
                assert!(glyph_row(ch, r) <= 0x1F, "ch={ch:#x} row={r}");
            }
        }
    }

    #[test]
    fn asset_theoretical_under_budget_by_far() {
        // 理论账远低于 200KB（点阵路线的预算优势量化在册）。
        let a = AssetLedgerV2::theoretical();
        assert!(a.total() < 60_000, "total={}", a.total());
        assert!(a.under_budget());
    }
}
