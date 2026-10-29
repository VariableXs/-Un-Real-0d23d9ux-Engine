//! UNX-F9139 内存族与 Windows VirtualAlloc 对照格（0x63120 段）
//!
//! 锚点：UNX-F9139-J1 对照账（Varix 分配原语 ↔ VirtualAlloc 参数格
//! ——格号 × 双侧原语名 × 双侧参数摘要 × 等价判定逐格落账）；J2 对照
//! 语义（双侧名非空 + 等价档闭集 + 未映射显式 N/A 三性质）；J3 对照
//! 指纹（对照账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（syscall 内存族 Windows 对照面）。
//!
//! 结构：等价档闭集 + 对照格账本 + 对照核（非空闸 + 档闸 + N/A 闸）+
//! 对照指纹 + 注入核（空名/未知档/N/A 带参/重格号/印后落格必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63120 独占 0x20）。
pub const SEG_BASE: u32 = 0x63120;
pub const WA_EMPTY_NAME: u32 = SEG_BASE + 0x01; // 空名
pub const WA_UNK_EQ: u32 = SEG_BASE + 0x02; // 未知等价档
pub const WA_NA_WITH_ARG: u32 = SEG_BASE + 0x03; // N/A 带参
pub const WA_DUP_CELL: u32 = SEG_BASE + 0x04; // 重格号
pub const WA_FROZEN: u32 = SEG_BASE + 0x05; // 印后落格

/// 等价档闭集。
pub const E_EXACT: usize = 0; // 语义等价
pub const E_PARTIAL: usize = 1; // 部分等价（需 wrapper）
pub const E_SUPER: usize = 2; // Windows 侧超集
pub const E_NA: usize = 3; // 无对应
pub const E_COUNT: usize = 4;

pub const EQ_NAMES: [&str; E_COUNT] = ["exact", "partial", "super", "n/a"];

/// 单条对照格。
#[derive(Clone)]
pub struct VirtualAllocCell {
    pub cell_no: u32,
    pub vx_name: String,
    pub win_name: String,
    pub eq: usize,
    /// wrapper 说明（partial 档必填）。
    pub wrapper: String,
}

impl VirtualAllocCell {
    /// 格合法：等价档闭集 + N/A 档不带参 + 非 N/A 双侧名非空 + partial 必带 wrapper。
    pub fn cell_ok(&self) -> bool {
        if self.eq >= E_COUNT {
            return false;
        }
        if self.eq == E_NA {
            return self.win_name.is_empty() && self.wrapper.is_empty() && !self.vx_name.is_empty();
        }
        if self.vx_name.is_empty() || self.win_name.is_empty() {
            return false;
        }
        if self.eq == E_PARTIAL && self.wrapper.is_empty() {
            return false;
        }
        true
    }

    fn render_line(&self) -> String {
        if self.eq == E_NA {
            return format!("WA{}|{}->N/A", self.cell_no, self.vx_name);
        }
        if self.eq == E_PARTIAL {
            return format!(
                "WA{}|{}->{}|{}|w={}",
                self.cell_no, self.vx_name, self.win_name, EQ_NAMES[self.eq], self.wrapper
            );
        }
        format!(
            "WA{}|{}->{}|{}",
            self.cell_no, self.vx_name, self.win_name, EQ_NAMES[self.eq]
        )
    }
}

/// 对照格账本。
pub struct WinAllocLedger {
    cells: Vec<VirtualAllocCell>,
    sealed: bool,
}

impl WinAllocLedger {
    pub fn new() -> Self {
        WinAllocLedger {
            cells: Vec::new(),
            sealed: false,
        }
    }

    /// 落一格：合法性闸 + 重号拒 + 印后拒。
    pub fn put(&mut self, cell: VirtualAllocCell) -> Result<(), u32> {
        if self.sealed {
            return Err(WA_FROZEN);
        }
        if cell.eq >= E_COUNT {
            return Err(WA_UNK_EQ);
        }
        if cell.eq == E_NA && (!cell.win_name.is_empty() || !cell.wrapper.is_empty()) {
            return Err(WA_NA_WITH_ARG);
        }
        if cell.vx_name.is_empty() || (cell.eq != E_NA && cell.win_name.is_empty()) {
            return Err(WA_EMPTY_NAME);
        }
        if cell.eq == E_PARTIAL && cell.wrapper.is_empty() {
            return Err(WA_EMPTY_NAME);
        }
        if self.cells.iter().any(|x| x.cell_no == cell.cell_no) {
            return Err(WA_DUP_CELL);
        }
        self.cells.push(cell);
        Ok(())
    }

    pub fn cell(&self, no: u32) -> Option<&VirtualAllocCell> {
        self.cells.iter().find(|x| x.cell_no == no)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Varix 侧正查。
    pub fn fwd(&self, vx: &str) -> Option<&VirtualAllocCell> {
        self.cells.iter().find(|x| x.vx_name == vx)
    }

    /// 各档计数。
    pub fn eq_counts(&self) -> [usize; E_COUNT] {
        let mut c = [0usize; E_COUNT];
        for x in &self.cells {
            if x.eq < E_COUNT {
                c[x.eq] += 1;
            }
        }
        c
    }

    /// 对照指纹。
    pub fn ledger_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.cells {
            h = h.wrapping_mul(0x0100_0193) ^ x.cell_no;
            for b in x.vx_name.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            for b in x.win_name.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h ^= x.eq as u32;
            for b in x.wrapper.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.cells.is_empty() || !self.cells.iter().all(|x| x.cell_ok()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for x in &self.cells {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("WALLOC-SEAL\n");
        }
        out
    }
}

impl Default for WinAllocLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四格（exact/partial/super/n-a 各一）。
fn standard_ledger() -> WinAllocLedger {
    let mut l = WinAllocLedger::new();
    let _ = l.put(VirtualAllocCell {
        cell_no: 1,
        vx_name: "mmap-anon".to_string(),
        win_name: "VirtualAlloc-MEM_RESERVE-Commit".to_string(),
        eq: E_EXACT,
        wrapper: String::new(),
    });
    let _ = l.put(VirtualAllocCell {
        cell_no: 2,
        vx_name: "brk-extend".to_string(),
        win_name: "VirtualAlloc-adjacent".to_string(),
        eq: E_PARTIAL,
        wrapper: "heap-top-walk".to_string(),
    });
    let _ = l.put(VirtualAllocCell {
        cell_no: 3,
        vx_name: "madvise-dontneed".to_string(),
        win_name: "DiscardVirtualMemory".to_string(),
        eq: E_SUPER,
        wrapper: String::new(),
    });
    let _ = l.put(VirtualAllocCell {
        cell_no: 4,
        vx_name: "mincore".to_string(),
        win_name: String::new(),
        eq: E_NA,
        wrapper: String::new(),
    });
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    let mk = |no: u32, vx: &str, win: &str, eq: usize, w: &str| VirtualAllocCell {
        cell_no: no,
        vx_name: vx.to_string(),
        win_name: win.to_string(),
        eq,
        wrapper: w.to_string(),
    };
    // 注 1：空名
    let mut l = WinAllocLedger::new();
    if l.put(mk(1, "", "X", E_EXACT, "")) == Err(WA_EMPTY_NAME) {
        caught += 1;
    }
    // 注 2：未知等价档
    let mut l = WinAllocLedger::new();
    if l.put(mk(1, "a", "X", 9, "")) == Err(WA_UNK_EQ) {
        caught += 1;
    }
    // 注 3：N/A 带参
    let mut l = WinAllocLedger::new();
    if l.put(mk(1, "a", "X", E_NA, "")) == Err(WA_NA_WITH_ARG) {
        caught += 1;
    }
    // 注 4：partial 缺 wrapper
    let mut l = WinAllocLedger::new();
    if l.put(mk(1, "a", "X", E_PARTIAL, "")) == Err(WA_EMPTY_NAME) {
        caught += 1;
    }
    // 注 5：印后落格
    let mut l = standard_ledger();
    let _ = l.seal();
    if l.put(mk(9, "late", "X", E_EXACT, "")) == Err(WA_FROZEN) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 对照账；J2 对照语义；J3 对照指纹。
pub fn run_unx9139_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9139");
    // J1-1 四格全落账
    let l = standard_ledger();
    let ok = l.len() == 4 && (1..=4).all(|n| l.cell(n).is_some());
    s.add("9139-J1-四格全落账", ok, if ok { "" } else { "对照账不全" });
    // J1-2 渲染导出齐
    let mut l2 = standard_ledger();
    let _ = l2.seal();
    let text = l2.render_ledger();
    let ok = text.contains("WA1|mmap-anon->VirtualAlloc-MEM_RESERVE-Commit|exact")
        && text.contains("WA2|brk-extend->VirtualAlloc-adjacent|partial|w=heap-top-walk")
        && text.contains("WA4|mincore->N/A")
        && text.contains("WALLOC-SEAL");
    s.add("9139-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 格合法性（全部）
    let ok = (1..=4).all(|n| l.cell(n).map(|x| x.cell_ok()).unwrap_or(false));
    s.add("9139-J2-格合法性", ok, if ok { "" } else { "合法性漂移" });
    // J2-2 正查（brk-extend → partial + wrapper）
    let ok = l
        .fwd("brk-extend")
        .map(|x| x.eq == E_PARTIAL && x.wrapper == "heap-top-walk")
        .unwrap_or(false);
    s.add("9139-J2-正查brk", ok, if ok { "" } else { "正查漂移" });
    // J2-3 N/A 显式（mincore 无对应且不带参）
    let ok = l.fwd("mincore").map(|x| x.eq == E_NA && x.win_name.is_empty()).unwrap_or(false);
    s.add("9139-J2-NA显式", ok, if ok { "" } else { "N/A 漂移" });
    // J2-4 档计数（exact1/partial1/super1/na1）
    let ec = l.eq_counts();
    let ok = ec == [1, 1, 1, 1];
    let detail = if ok { String::new() } else { format!("{},{},{},{}", ec[0], ec[1], ec[2], ec[3]) };
    s.add_dyn("9139-J2-档计数", ec == [1, 1, 1, 1], &detail);
    // J2-5 重格号拒
    let mut l3 = standard_ledger();
    let ok = l3
        .put(VirtualAllocCell {
            cell_no: 2,
            vx_name: "dup".to_string(),
            win_name: "X".to_string(),
            eq: E_EXACT,
            wrapper: String::new(),
        })
        == Err(WA_DUP_CELL);
    s.add("9139-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J2-6 partial 缺 wrapper 拒
    let mut l4 = WinAllocLedger::new();
    let ok = l4
        .put(VirtualAllocCell {
            cell_no: 1,
            vx_name: "a".to_string(),
            win_name: "X".to_string(),
            eq: E_PARTIAL,
            wrapper: String::new(),
        })
        == Err(WA_EMPTY_NAME);
    s.add("9139-J2-partial缺wrapper拒", ok, if ok { "" } else { "wrapper 闸失效" });
    // J3-1 对照指纹确定
    let ok = standard_ledger().ledger_fp() == standard_ledger().ledger_fp();
    s.add("9139-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 档变签变
    let mut l5 = standard_ledger();
    let fp0 = l5.ledger_fp();
    if let Some(x) = l5.cells.first_mut() {
        x.eq = E_SUPER;
    }
    let ok = l5.ledger_fp() != fp0;
    s.add("9139-J3-档变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印
    let mut l6 = standard_ledger();
    let ok = l6.seal() && l6.sealed();
    s.add("9139-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒落格
    let ok = l6
        .put(VirtualAllocCell {
            cell_no: 9,
            vx_name: "late".to_string(),
            win_name: "X".to_string(),
            eq: E_EXACT,
            wrapper: String::new(),
        })
        == Err(WA_FROZEN);
    s.add("9139-J3-印后拒落格", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9139-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9139_all_checks_green() {
        let set = run_unx9139_checks();
        assert!(set.all_passed(), "UNX-F9139 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9139_ledger_stable() {
        let mut l = standard_ledger();
        assert!(l.seal());
    }
}
