//! F428 Ctrl+P 打印链路 · 完整设计（STAR I 主册 G-I-28）。
//!
//! **判据（主册）**：四常用项功能；分页预览准确性（与实际输出一致）；
//! PDF 产物落位；无打印机引导链；对话框键位（F434）兼容。＋通12。
//!
//! 设计：打印对话框语义核——打印机选择（已装/虚拟 PDF/无打印机→F444
//! 引导）；四常用项（页码范围/份数/单双面/方向）；分页预览（页码范围
//! 解析与页数核算一致性）；PDF 产物落文档目录；Ctrl+P 注册。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_CTRL};

use alloc::vec::Vec;

/// PDF 产物落位目录。
pub const PDF_TARGET_DIR: &str = "文档";

/// 份数上限（打印对话框通用判线——防手滑 999 份）。
pub const COPIES_MAX: u32 = 99;

/// 四常用项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintOpts {
    /// 页码范围（1 起，含端点）。
    pub from_page: u32,
    pub to_page: u32,
    pub copies: u32,
    pub duplex: bool,
    /// 方向：false 纵向 / true 横向。
    pub landscape: bool,
    /// 每纸张数（v6：1/2/4 版——N 上打印）。
    pub pages_per_sheet: u32,
}

impl PrintOpts {
    fn defaults(total: u32) -> PrintOpts {
        PrintOpts {
            from_page: 1,
            to_page: total,
            copies: 1,
            duplex: false,
            landscape: false,
            pages_per_sheet: 1,
        }
    }
}

/// 打印对话框核。
pub struct PrintDialog {
    pub hotkeys: HotkeyTable,
    pub printers: Vec<&'static str>,
    pub selected: Option<usize>,
    pub opts: PrintOpts,
    /// 文档总页数（预览准确性基准）。
    pub total_pages: u32,
    pub pdf_jobs: u64,
    /// 任务取消账（v6：执行前撤销——取消不虚计产物）。
    pub jobs_cancelled: u64,
}

impl PrintDialog {
    pub fn new(total_pages: u32) -> PrintDialog {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f428.print", Chord::new(MOD_CTRL, b'P'));
        PrintDialog {
            hotkeys,
            printers: Vec::new(),
            selected: None,
            opts: PrintOpts::defaults(total_pages),
            total_pages,
            pdf_jobs: 0,
            jobs_cancelled: 0,
        }
    }

    /// 页码范围校验（越界收敛 + 逆序翻转——不产生非法范围）。
    /// 零页文档：范围归 0（诚实空态，不造出「第 1 页」）。
    pub fn normalize_range(&mut self, from: u32, to: u32) -> (u32, u32) {
        if self.total_pages == 0 {
            self.opts.from_page = 0;
            self.opts.to_page = 0;
            return (0, 0);
        }
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        self.opts.from_page = a.max(1).min(self.total_pages);
        self.opts.to_page = b.max(1).min(self.total_pages);
        (self.opts.from_page, self.opts.to_page)
    }

    /// 份数设定（v6）：1..=99 钳制（防手滑，超限收敛到界）。
    pub fn set_copies(&mut self, copies: u32) -> u32 {
        self.opts.copies = copies.clamp(1, COPIES_MAX);
        self.opts.copies
    }

    /// 分页预览：实际将打印的页清单（与输出一致的核算基准）。
    pub fn preview_pages(&self) -> Vec<u32> {
        match self.selected {
            Some(_) if self.opts.to_page >= self.opts.from_page && self.opts.to_page > 0 => {
                (self.opts.from_page..=self.opts.to_page).collect()
            }
            _ => Vec::new(),
        }
    }

    /// 总输出张数（份数 × 张数；双面按张纸两面计；N 上按每张 N 页计）。
    pub fn sheets(&self) -> u32 {
        if self.total_pages == 0 || self.opts.to_page < self.opts.from_page || self.opts.to_page == 0 {
            return 0; // 零页文档/非法范围：0 张——诚实，不虚报。
        }
        let pages = self.opts.to_page.saturating_sub(self.opts.from_page) + 1;
        let slots = self.opts.pages_per_sheet.max(1) * if self.opts.duplex { 2 } else { 1 };
        let per_copy = pages.div_ceil(slots);
        per_copy * self.opts.copies.max(1)
    }

    /// 选打印机（清单外拒绝）。
    pub fn select(&mut self, idx: usize) -> bool {
        if idx < self.printers.len() {
            self.selected = Some(idx);
            true
        } else {
            false
        }
    }

    /// 无打印机引导链：清单空 → 返回 F444 添加向导动作。
    pub fn no_printer_guide(&self) -> Option<&'static str> {
        if self.printers.is_empty() {
            Some("f444.add-printer")
        } else {
            None
        }
    }

    /// 打印执行：虚拟 PDF 打印产文件到文档目录（产物账）。
    pub fn print_to_pdf(&mut self) -> Option<(&'static str, u32)> {
        match self.selected {
            Some(i) if self.printers[i] == "虚拟 PDF" => {
                self.pdf_jobs += 1;
                Some((PDF_TARGET_DIR, self.sheets()))
            }
            Some(_) => Some((PDF_TARGET_DIR, self.sheets())), // 物理打印机同样产计数
            None => None,
        }
    }

    /// 执行前取消（v6）：未产文件不虚计产物，取消如实入账。
    pub fn cancel(&mut self) -> bool {
        if self.pdf_jobs > 0 {
            return false; // 已执行的产物没有「取消」——撤销是执行前的权利。
        }
        self.jobs_cancelled += 1;
        true
    }
}

pub fn run_printkey_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F428");
    let mut p = PrintDialog::new(10);
    set.add(
        "f428-ctrl-p-registered",
        p.hotkeys.lookup(Chord::new(MOD_CTRL, b'P')) == Some("f428.print"),
        "",
    );
    // 无打印机引导链。
    set.add(
        "f428-no-printer-guide",
        p.no_printer_guide() == Some("f444.add-printer") && p.preview_pages().is_empty(),
        "",
    );
    // 装打印机（含虚拟 PDF）后选择。
    p.printers = alloc::vec!["虚拟 PDF", "HP LaserJet"];
    set.add(
        "f428-select-printer",
        p.select(0) && !p.select(5),
        "",
    );
    // 四常用项：范围归一化 + 逆序翻转。
    set.add(
        "f428-range-normalize",
        p.normalize_range(3, 6) == (3, 6) && p.normalize_range(8, 5) == (5, 8),
        "",
    );
    set.add("f428-range-clamped", p.normalize_range(0, 99) == (1, 10), "");
    // 分页预览准确性。
    p.normalize_range(2, 4);
    set.add(
        "f428-preview-accurate",
        p.preview_pages() == alloc::vec![2, 3, 4],
        "",
    );
    // 张数核算：3 页 ×2 份单面 = 6 张；双面 = 4 张。
    p.opts.copies = 2;
    set.add("f428-sheets-simplex", p.sheets() == 6, "");
    p.opts.duplex = true;
    set.add("f428-sheets-duplex", p.sheets() == 4, "");
    // PDF 产物落位。
    p.opts.copies = 1;
    p.opts.duplex = false;
    p.normalize_range(1, 10);
    set.add(
        "f428-pdf-target",
        p.print_to_pdf() == Some(("文档", 10)) && p.pdf_jobs == 1,
        "",
    );
    // 未选打印机不执行。
    let mut q = PrintDialog::new(5);
    set.add("f428-no-selection-no-print", q.print_to_pdf().is_none(), "");
    // 份数钳制（v6）：0 → 1、200 → 99（防手滑 999 份）。
    set.add(
        "f428-copies-clamp",
        q.set_copies(0) == 1 && q.set_copies(200) == COPIES_MAX && q.opts.copies == 99,
        "",
    );
    // N 上打印（v6）：10 页 2 上 → 5 张；4 上双面 → ceil(10/8)=2 张。
    let mut n = PrintDialog::new(10);
    n.printers = alloc::vec!["虚拟 PDF"];
    let _ = n.select(0);
    let _ = n.normalize_range(1, 10);
    n.opts.pages_per_sheet = 2;
    set.add("f428-2up-sheets", n.sheets() == 5, "");
    n.opts.pages_per_sheet = 4;
    n.opts.duplex = true;
    set.add("f428-4up-duplex-sheets", n.sheets() == 2, "");
    // N 上不改默认行为（v6 回归锚）：pps=1 时老算式不变。
    n.opts.pages_per_sheet = 1;
    n.opts.duplex = false;
    set.add("f428-1up-default", n.sheets() == 10, "");
    // 执行前取消（v6）：不产文件、账分记；执行后无取消。
    set.add("f428-cancel-before-print", n.cancel() && n.jobs_cancelled == 1, "");
    let _ = n.print_to_pdf();
    set.add(
        "f428-no-cancel-after-print",
        !n.cancel() && n.jobs_cancelled == 1 && n.pdf_jobs == 1,
        "",
    );
    // 零页文档诚实门（v6）：范围归 0、预览空、张数 0——不造「第 1 页」。
    let mut z = PrintDialog::new(0);
    z.printers = alloc::vec!["虚拟 PDF"];
    let _ = z.select(0);
    set.add(
        "f428-zero-page-doc",
        z.normalize_range(1, 5) == (0, 0)
            && z.preview_pages().is_empty()
            && z.sheets() == 0
            && z.print_to_pdf() == Some(("文档", 0)),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_math_matrix() {
        let mut p = PrintDialog::new(7);
        p.printers = alloc::vec!["虚拟 PDF"];
        let _ = p.select(0);
        let _ = p.normalize_range(1, 7);
        // 单面 1 份。
        p.opts = PrintOpts { from_page: 1, to_page: 7, copies: 1, duplex: false, landscape: false, pages_per_sheet: 1 };
        assert_eq!(p.sheets(), 7);
        // 双面 1 份（7 页 → 4 张）。
        p.opts.duplex = true;
        assert_eq!(p.sheets(), 4);
        // 双面 3 份（12 张）。
        p.opts.copies = 3;
        assert_eq!(p.sheets(), 12);
    }

    #[test]
    fn physical_printer_also_prints() {
        let mut p = PrintDialog::new(3);
        p.printers = alloc::vec!["HP"];
        let _ = p.select(0);
        assert!(p.print_to_pdf().is_some());
        assert!(p.no_printer_guide().is_none(), "有打印机不触发引导");
    }
}
