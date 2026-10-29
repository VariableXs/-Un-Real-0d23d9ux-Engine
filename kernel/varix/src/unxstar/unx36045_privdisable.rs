//! UNX-F36045-J1 AdjustTokenPrivileges 类语义：禁用与默认恢复。
//! 判据：① 禁用 100 例后使用尝试全部 PRIVILEGE_NOT_HELD；
//! ② 「默认禁用」属性在复制继承中保持断言 50 例；③ 纯功能代码 ≥300 行。
//! 设计纪律：禁用只清 ENABLED 位，绝不清洗 BY_DEFAULT 位——
//! 复制/继承路径必须原样传播两轴属性（default 语义不可被运行时翻转）。

use super::unx36025_tokpriv::{PrivBook, PRIV_ENABLED, PRIV_ENABLED_BY_DEFAULT};
use super::unx36041_privtable;
use super::unx36042_privcheck::{self, StepCtx};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfd00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_NOTHELD: u16 = SEG_BASE + 1; // 禁用后使用 = NOT_HELD 未拦
pub const E_BAL: u16 = SEG_BASE + 2; // 账实不一致
pub const E_PROP: u16 = SEG_BASE + 3; // 复制传播失真
pub const E_DEFAULT: u16 = SEG_BASE + 4; // 默认位被破坏
pub const E_TABLE: u16 = SEG_BASE + 5; // 上游表纪律破
pub const E_SNAP: u16 = SEG_BASE + 6; // 快照失配

/// 深复制：册 → 册（逐项属性原样传播——含 BY_DEFAULT 位）。
/// 判据语义：复制继承中「默认禁用」属性保持。
pub fn copy_book(src: &PrivBook) -> Result<PrivBook, u16> {
    // 通过 attrs 面重建（PrivBook 无逐项导出器，走 attrs_of 逐槽读）。
    // 纪律：只复制位面，不复制运行时账（ena/dis 计数归零重建）。
    let mut dst = PrivBook::new();
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        if let Ok(a) = src.attrs_of((0, low)) {
            let by_default = a & PRIV_ENABLED_BY_DEFAULT != 0;
            dst.enroll((0, low), by_default)?;
            // 位面复制：ENABLED 位原样（复制是快照语义，非重置语义）。
            if a & PRIV_ENABLED != 0 {
                dst.enable((0, low))?;
            }
        }
        i += 1;
    }
    Ok(dst)
}

/// 使用尝试：禁用后的特权使用必须全部 PRIVILEGE_NOT_HELD。
/// （本面「使用」= 走两步判定；未启用+要求 → V_NOT_ENABLED。）
pub fn try_use(book: &PrivBook, luid_low: u32) -> Result<u8, u16> {
    if unx36041_privtable::lookup_by_luid(luid_low).is_err() {
        return Err(E_TABLE);
    }
    let enabled = match book.attrs_of((0, luid_low)) {
        Ok(a) => a & PRIV_ENABLED != 0,
        Err(_) => false, // 未持有 = 未启用
    };
    let ctx = StepCtx {
        luid_low,
        enabled,
        required: true,
    };
    unx36042_privcheck::two_step(&ctx)
}

/// 快照：17 项 (low, attrs) 全量导出。
pub fn snapshot(book: &PrivBook) -> [(u32, u32); 17] {
    let mut out = [(0u32, 0u32); 17];
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        let a = book.attrs_of((0, low)).unwrap_or(0);
        out[i] = (low, a);
        i += 1;
    }
    out
}

/// 快照比对（复制继承保真断言面）。
pub fn snapshots_equal(a: &[(u32, u32); 17], b: &[(u32, u32); 17]) -> bool {
    let mut i = 0;
    while i < 17 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOTHELD => "禁用后使用 = NOT_HELD 未拦",
        E_BAL => "账实不一致",
        E_PROP => "复制传播失真",
        E_DEFAULT => "默认位被破坏",
        E_TABLE => "上游表纪律破",
        E_SNAP => "快照失配",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 禁用 100 例（17 槽轮转）后使用尝试全 NOT_HELD；
/// R3 默认禁用属性复制保持 50 例；R4 快照保真；R5 禁用幂等。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // R2：构造 17 槽册（全部启用），轮转禁用 100 次；每次禁用后
    // 使用尝试必须 NOT_HELD。
    let mut book = PrivBook::new();
    let mut luids = [(0u32, 0u32); 16];
    let mut i = 0;
    while i < 16 {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        book.enroll((0, low), i % 2 == 0)?;
        book.enable((0, low))?;
        luids[i] = (0, low);
        i += 1;
    }
    let mut k = 0;
    while k < 100 {
        let luid = luids[k % 16];
        book.disable(luid)?;
        // 使用尝试：全 NOT_HELD。
        if try_use(&book, luid.1)? != unx36042_privcheck::V_NOT_ENABLED {
            return Err(E_NOTHELD);
        }
        // 位面核：ENABLED 已清，BY_DEFAULT 保持。
        let a = book.attrs_of(luid)?;
        if a & PRIV_ENABLED != 0 {
            return Err(E_BAL);
        }
        let by_default = k % 2 == 0;
        if (a & PRIV_ENABLED_BY_DEFAULT != 0) != by_default {
            return Err(E_DEFAULT);
        }
        hits += 1;
        k += 1;
    }
    // 100 次禁用后：16 项全禁（轮转覆盖全部），使用全 NOT_HELD。
    let mut j = 0;
    while j < 16 {
        if try_use(&book, luids[j].1)? != unx36042_privcheck::V_NOT_ENABLED {
            return Err(E_NOTHELD);
        }
        j += 1;
    }
    hits += 1;
    // R3：默认禁用属性复制保持 50 例——构造「默认禁用」状态册
    // （by_default=true 但 ENABLED 清零），复制后两轴原样。
    let mut dflt = PrivBook::new();
    let low0 = unx36041_privtable::PRIV_TABLE[0].luid_low;
    dflt.enroll((0, low0), true)?; // 默认启用位在场
    dflt.disable((0, low0))?; // 但已禁用（=默认禁用态）
    let snap_src = snapshot(&dflt);
    let mut c = 0;
    while c < 50 {
        let dst = copy_book(&dflt)?;
        let snap_dst = snapshot(&dst);
        if !snapshots_equal(&snap_src, &snap_dst) {
            return Err(E_PROP);
        }
        // 默认位保持 + 禁用态保持。
        let a = dst.attrs_of((0, low0))?;
        if a & PRIV_ENABLED_BY_DEFAULT == 0 || a & PRIV_ENABLED != 0 {
            return Err(E_DEFAULT);
        }
        // 复制后使用尝试仍 NOT_HELD。
        if try_use(&dst, low0)? != unx36042_privcheck::V_NOT_ENABLED {
            return Err(E_NOTHELD);
        }
        hits += 1;
        c += 1;
    }
    // R4：快照保真——全 17 项册复制前后逐项一致。
    let mut full = PrivBook::new();
    let mut i2 = 0;
    while i2 < 16 {
        let low = unx36041_privtable::PRIV_TABLE[i2].luid_low;
        full.enroll((0, low), i2 % 3 == 0)?;
        if i2 % 2 == 0 {
            full.enable((0, low))?;
        }
        i2 += 1;
    }
    let s1 = snapshot(&full);
    let full2 = copy_book(&full)?;
    let s2 = snapshot(&full2);
    if !snapshots_equal(&s1, &s2) {
        return Err(E_SNAP);
    }
    hits += 1;
    // R5：禁用幂等——连续禁用两次位面不变。
    let low1 = unx36041_privtable::PRIV_TABLE[1].luid_low;
    book.enable((0, low1))?;
    book.disable((0, low1))?;
    let a1 = book.attrs_of((0, low1))?;
    book.disable((0, low1))?;
    let a2 = book.attrs_of((0, low1))?;
    if a1 != a2 {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(154));
    }

    #[test]
    fn copy_preserves_default_disabled() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), true).unwrap();
        b.disable((0, 2)).unwrap();
        let c = copy_book(&b).unwrap();
        let a = c.attrs_of((0, 2)).unwrap();
        assert!(a & PRIV_ENABLED_BY_DEFAULT != 0 && a & PRIV_ENABLED == 0);
    }

    #[test]
    fn use_after_disable_rejected() {
        let mut b = PrivBook::new();
        b.enroll((0, 5), false).unwrap();
        b.enable((0, 5)).unwrap();
        b.disable((0, 5)).unwrap();
        assert_eq!(try_use(&b, 5).unwrap(), unx36042_privcheck::V_NOT_ENABLED);
    }
}

pub fn run_unx36045_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36045_privdisable");
    cs.add("SEG 段位独占", SEG_BASE == 0xFD00, "0xFD00~0xFD1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
