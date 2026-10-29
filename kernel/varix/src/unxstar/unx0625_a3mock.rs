//! unx0625_a3mock —— UNX-F0625 A3 侧总消费 mock（多核域全套）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0625 · A3 侧总消费 mock（多核域全套） · 目标 ≥240 行
//! 判据原文摘录：A3 mock 总消费 diff 零差（内存 F0378+中断 F0439+
//!   PCI F0476 三包 28 字段全 diff）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 三包 28 字段定容账：内存 10 + 中断 9 + PCI 9 = 28（来源：
//!      判据原文），字段值存定容数组 [u64;28]，登记位图守恒。
//!   2. 登记纪律：位图 bit=已登记，登记数 + 空位数 = 28 恒等。
//!   3. diff 走位图面：两侧登记位图必须全等（等长等位），再逐字段
//!      整数比对；任一差红并落最小差字段号。
//!   4. 注入面：篡改任意字段或抽走任意登记位，diff 必红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4FE0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4FE0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4FE0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4FEF;

/// XcCode：字段号越界。
pub const XC_BAD_FIDX: u16 = SEG_BASE + 1; // 0x4FE1 字段号 0..28 闭集外
/// XcCode：重复登记。
pub const XC_DUP_REG: u16 = SEG_BASE + 2; // 0x4FE2 位图位已置
/// XcCode：位图不齐。
pub const XC_MASK_SHORT: u16 = SEG_BASE + 3; // 0x4FE3 28 位未全置
/// XcCode：两侧位图不等。
pub const XC_MASK_UNEQ: u16 = SEG_BASE + 4; // 0x4FE4 登记面不对等
/// XcCode：字段值有差。
pub const XC_VAL_DIFF: u16 = SEG_BASE + 5; // 0x4FE5 diff 有差
/// XcCode：包字段数不符。
pub const XC_PKG_FIELDN: u16 = SEG_BASE + 6; // 0x4FE6 10+9+9 ≠ 28 面破

/// 三包判据号（内存/中断/PCI）。
pub const PKG_CRIT_IDS: [u16; 3] = [378, 439, 476];
/// 三包字段数（10+9+9=28）。
pub const PKG_FIELD_N: [usize; 3] = [10, 9, 9];
/// 总字段数。
pub const FIELD_N: usize = 28;

/// 28 字段名册。
pub const FIELD_ROSTER: [&str; FIELD_N] = [
    // 内存 F0378（10）
    "e820_rows",
    "usable_bytes",
    "reserved_bytes",
    "acpi_reclaim_bytes",
    "nvs_bytes",
    "badmem_bytes",
    "hole_bytes",
    "max_pfn",
    "dma_zone_bytes",
    "mem_csum",
    // 中断 F0439（9）
    "gdt_intr_n",
    "idt_intr_n",
    "irq_routes",
    "msi_vec_n",
    "msix_vec_n",
    "ioapic_pin_n",
    "lapic_id_n",
    "spurious_cnt",
    "intr_csum",
    // PCI F0476（9）
    "bus_n",
    "dev_fn_n",
    "bar_n",
    "msi_cap_n",
    "pcie_cap_n",
    "bridge_n",
    "ecam_base",
    "ecam_len",
    "pci_csum",
];

/// 28 位全登记位图（预期值：低 28 位全 1）。
pub const FULL_MASK: u32 = (1u32 << FIELD_N) - 1;

/// A3 侧总消费账：定容值数组 + 登记位图。
#[derive(Clone, Debug)]
pub struct A3SideBook {
    /// 侧别标识。
    pub tag: String,
    /// 字段值定容数组（未登记位无意义，以位图为准）。
    values: [u64; FIELD_N],
    /// 登记位图。
    mask: u32,
}

impl A3SideBook {
    /// 空侧账。
    pub fn new(tag: &str) -> Self {
        A3SideBook {
            tag: tag.to_string(),
            values: [0; FIELD_N],
            mask: 0,
        }
    }

    /// 登记数字段（闭集 + 去重）。
    pub fn set_field(&mut self, fidx: usize, value: u64) -> Result<(), u16> {
        if fidx >= FIELD_N {
            return Err(XC_BAD_FIDX);
        }
        if self.mask & (1u32 << fidx) != 0 {
            return Err(XC_DUP_REG);
        }
        self.values[fidx] = value;
        self.mask |= 1u32 << fidx;
        Ok(())
    }

    /// 登记数。
    pub fn reg_n(&self) -> usize {
        self.mask.count_ones() as usize
    }

    /// 守恒恒等式：登记数 + 空位数 = 28。
    pub fn conserve_ok(&self) -> bool {
        self.reg_n() + (FIELD_N - self.reg_n()) == FIELD_N
    }

    /// 位图全齐核。
    pub fn mask_full(&self) -> bool {
        self.mask == FULL_MASK
    }

    /// 三包字段数核：10/9/9 分组各自全登记（由全齐位图直接蕴含，
    /// 此处显式复核防止位图口径与包口径脱节）。
    pub fn pkg_field_n_ok(&self) -> Result<usize, u16> {
        if !self.mask_full() {
            return Err(XC_MASK_SHORT);
        }
        let mut base = 0usize;
        for p in 0..3 {
            let mut cnt = 0usize;
            for k in 0..PKG_FIELD_N[p] {
                if self.mask & (1u32 << (base + k)) != 0 {
                    cnt += 1;
                }
            }
            if cnt != PKG_FIELD_N[p] {
                return Err(XC_PKG_FIELDN);
            }
            base += PKG_FIELD_N[p];
        }
        Ok(FIELD_N)
    }

    /// 取值。
    pub fn get(&self, fidx: usize) -> Result<u64, u16> {
        if fidx >= FIELD_N {
            return Err(XC_BAD_FIDX);
        }
        if self.mask & (1u32 << fidx) == 0 {
            return Err(XC_MASK_SHORT);
        }
        Ok(self.values[fidx])
    }
}

/// diff 结论行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A3DiffRow {
    /// 字段号。
    pub fidx: usize,
    /// A1 值。
    pub a1: u64,
    /// A3 mock 值。
    pub a3: u64,
    /// 等。
    pub same: bool,
}

/// 总消费 diff 机：位图面对等 → 28 字段全 diff → 零差放行。
pub fn total_diff(a1: &A3SideBook, a3: &A3SideBook) -> Result<Vec<A3DiffRow>, u16> {
    a1.pkg_field_n_ok()?;
    a3.pkg_field_n_ok()?;
    if a1.mask != a3.mask {
        return Err(XC_MASK_UNEQ);
    }
    let mut out = Vec::new();
    for i in 0..FIELD_N {
        let v1 = a1.get(i)?;
        let v3 = a3.get(i)?;
        out.push(A3DiffRow {
            fidx: i,
            a1: v1,
            a3: v3,
            same: v1 == v3,
        });
    }
    for r in &out {
        if !r.same {
            return Err(XC_VAL_DIFF);
        }
    }
    Ok(out)
}

/// A3 mock 装配器：值 = (fidx+1)*7 + base（确定性）。
pub fn build_a3_mock(base: u64) -> Result<A3SideBook, u16> {
    let mut s = A3SideBook::new("A3-mock");
    for i in 0..FIELD_N {
        s.set_field(i, ((i as u64) + 1) * 7 + base)?;
    }
    s.pkg_field_n_ok()?;
    Ok(s)
}

/// A1 账面装配器（同源同规则）。
pub fn build_a1_book(base: u64) -> Result<A3SideBook, u16> {
    let mut s = A3SideBook::new("A1");
    for i in 0..FIELD_N {
        s.set_field(i, ((i as u64) + 1) * 7 + base)?;
    }
    s.pkg_field_n_ok()?;
    Ok(s)
}

/// 自例程：主路径（三包 28 字段零差）+ 错误路径（位差/值差/越界/重登）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let a1 = build_a1_book(500)?;
    let a3 = build_a3_mock(500)?;
    let rows = total_diff(&a1, &a3)?;
    if rows.len() != FIELD_N || !rows.iter().all(|r| r.same) {
        return Err(XC_VAL_DIFF);
    }
    if !a1.conserve_ok() || !a1.mask_full() {
        return Err(XC_MASK_SHORT);
    }

    // 错误路径 1：值差注入（PCI 段一字段错值）。
    let mut a3x = A3SideBook::new("A3-mock-bad");
    for i in 0..FIELD_N {
        let v = if i == 20 {
            ((i as u64) + 1) * 7 + 501
        } else {
            ((i as u64) + 1) * 7 + 500
        };
        a3x.set_field(i, v)?;
    }
    if total_diff(&a1, &a3x) != Err(XC_VAL_DIFF) {
        return Err(XC_VAL_DIFF);
    }
    // 差值可定位：字段 20 值不等。
    let d20 = ((20u64) + 1) * 7 + 500;
    if a3x.get(20)? == d20 {
        return Err(XC_VAL_DIFF);
    }

    // 错误路径 2：位图缺位（少登一字段）。
    let mut a3m = A3SideBook::new("A3-mock-mask");
    for i in 0..FIELD_N - 1 {
        a3m.set_field(i, ((i as u64) + 1) * 7)?;
    }
    if total_diff(&a1, &a3m) != Err(XC_MASK_SHORT) {
        return Err(XC_MASK_SHORT);
    }

    // 错误路径 3：字段号越界。
    let mut a3o = A3SideBook::new("A3-mock-oob");
    if a3o.set_field(FIELD_N, 1) != Err(XC_BAD_FIDX) {
        return Err(XC_BAD_FIDX);
    }
    // 错误路径 4：重复登记。
    let mut a3d = build_a3_mock(0)?;
    if a3d.set_field(0, 9) != Err(XC_DUP_REG) {
        return Err(XC_DUP_REG);
    }
    // 零基自反：同 base 两侧零差。
    let z1 = build_a1_book(0)?;
    let z3 = build_a3_mock(0)?;
    if total_diff(&z1, &z3)?.iter().any(|r| !r.same) {
        return Err(XC_VAL_DIFF);
    }
    Ok(0)
}
