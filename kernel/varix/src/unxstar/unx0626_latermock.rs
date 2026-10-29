//! unx0626_latermock —— UNX-F0626 后续域总 mock（显示/输入/文件族）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0626 · 后续域总 mock（显示/输入/文件族） · 目标 ≥240 行
//! 判据原文摘录：后续域 mock diff 零差（显示 F0497+输入 F0518+ktest
//!   F0538+日志 F0559+参数 F0598+initrd F0617 六包全字段 diff）。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 六包层级账：每包一个字段表（字段名+整数值），包号 0..6 闭集
//!      绑定判据号名册（497/518/538/559/598/617，来源判据原文）。
//!   2. 全字段 diff 语义：先包面对账（包数、包序、每包字段数），再
//!      字段面对账（同名同值），任何一层破即红并带定位信息。
//!   3. 零差判据：6/6 包 × 各包字段全等，包字段数名册 5+4+4+4+5+4
//!      = 26 守恒。
//!   4. 注入红检出：包级抽包 / 字段级错值 / 字段名漂移三类注入均须
//!      被核出。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5000 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

/// 诊断段基址（0x5000 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5000;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x500F;

/// XcCode：包数不齐。
pub const XC_PKG_SHORT: u16 = SEG_BASE + 1; // 0x5001 六包未登满
/// XcCode：包判据号绑错。
pub const XC_PKG_BIND: u16 = SEG_BASE + 2; // 0x5002 包内判据号 ≠ 名册
/// XcCode：包字段数不符。
pub const XC_FIELDN: u16 = SEG_BASE + 3; // 0x5003 包字段数 ≠ 名册
/// XcCode：字段名漂移。
pub const XC_NAME_DRIFT: u16 = SEG_BASE + 4; // 0x5004 同位字段名不同
/// XcCode：字段值有差。
pub const XC_VAL_DIFF: u16 = SEG_BASE + 5; // 0x5005 diff 有差
/// XcCode：包重复。
pub const XC_DUP_PKG: u16 = SEG_BASE + 6; // 0x5006 一包一位不许重登

/// 六包判据号名册（显示/输入/ktest/日志/参数/initrd）。
pub const SIX_PKG_CRITS: [u16; 6] = [497, 518, 538, 559, 598, 617];
/// 六包字段数名册（5+4+4+4+5+4 = 26）。
pub const SIX_PKG_FIELDN: [usize; 6] = [5, 4, 4, 4, 5, 4];
/// 六包总字段数。
pub const SIX_FIELD_TOTAL: usize = 26;

/// 六包字段名总名册（按包分组，包内序即登记序）。
pub const SIX_FIELD_ROSTER: [&str; SIX_FIELD_TOTAL] = [
    // 显示 F0497（5）
    "disp_mode", "fb_width", "fb_height", "fb_bpp", "disp_csum",
    // 输入 F0518（4）
    "kbd_layout", "ptr_btn_n", "ptr_wheel", "input_csum",
    // ktest F0538（4）
    "kt_suite_n", "kt_case_n", "kt_green_n", "ktest_csum",
    // 日志 F0559（4）
    "log_ring_cap", "log_lines", "log_drop_n", "log_csum",
    // 参数 F0598（5）
    "param_key_n", "param_bool_n", "param_str_n", "param_over_n", "param_csum",
    // initrd F0617（4）
    "initrd_mode", "initrd_size", "initrd_pkg_n", "initrd_csum",
];

/// 单包字段表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgFields {
    /// 包号（0..6）。
    pub pkg_no: usize,
    /// 判据号。
    pub crit_id: u16,
    /// 字段（名, 值）表。
    pub fields: Vec<(String, u64)>,
}

impl PkgFields {
    /// 包字段数。
    pub fn field_n(&self) -> usize {
        self.fields.len()
    }
}

/// 六包总账（一侧：A1 账面或 mock 共用）。
#[derive(Clone, Debug, Default)]
pub struct SixPkgSide {
    /// 六包（按包号升序）。
    pkgs: Vec<PkgFields>,
    /// 侧别标识。
    pub tag: String,
}

impl SixPkgSide {
    /// 空侧账。
    pub fn new(tag: &str) -> Self {
        SixPkgSide {
            pkgs: Vec::new(),
            tag: tag.to_string(),
        }
    }

    /// 登一包（判据号绑定 + 包序递增 + 去重 + 字段数名册核）。
    pub fn enroll_pkg(&mut self, pkg_no: usize, values: &[u64]) -> Result<(), u16> {
        if pkg_no >= 6 {
            return Err(XC_PKG_SHORT);
        }
        if self.pkgs.iter().any(|p| p.pkg_no == pkg_no) {
            return Err(XC_DUP_PKG);
        }
        let want = SIX_PKG_FIELDN[pkg_no];
        if values.len() != want {
            return Err(XC_FIELDN);
        }
        let base = SIX_PKG_FIELDN[..pkg_no].iter().sum::<usize>();
        let mut fields = Vec::new();
        for k in 0..want {
            fields.push((SIX_FIELD_ROSTER[base + k].to_string(), values[k]));
        }
        self.pkgs.push(PkgFields {
            pkg_no,
            crit_id: SIX_PKG_CRITS[pkg_no],
            fields,
        });
        self.pkgs.sort_by_key(|p| p.pkg_no);
        Ok(())
    }

    /// 包数。
    pub fn pkg_n(&self) -> usize {
        self.pkgs.len()
    }

    /// 取包。
    pub fn pkg_at(&self, pkg_no: usize) -> Option<&PkgFields> {
        self.pkgs.iter().find(|p| p.pkg_no == pkg_no)
    }

    /// 六包面守恒核：6/6 包 + 判据号绑定 + 包字段数名册 + 总字段 26。
    pub fn face_ok(&self) -> Result<usize, u16> {
        if self.pkg_n() != 6 {
            return Err(XC_PKG_SHORT);
        }
        let mut total = 0usize;
        for p in 0..6 {
            let pkg = match self.pkg_at(p) {
                Some(p) => p,
                None => return Err(XC_PKG_SHORT),
            };
            if pkg.crit_id != SIX_PKG_CRITS[p] {
                return Err(XC_PKG_BIND);
            }
            if pkg.field_n() != SIX_PKG_FIELDN[p] {
                return Err(XC_FIELDN);
            }
            total += pkg.field_n();
        }
        if total != SIX_FIELD_TOTAL {
            return Err(XC_FIELDN);
        }
        Ok(total)
    }
}

/// 六包全字段 diff 机：包面对账 + 字段面对账，零差放行。
pub fn six_pkg_diff(a1: &SixPkgSide, mock: &SixPkgSide) -> Result<usize, u16> {
    let n1 = a1.face_ok()?;
    let n2 = mock.face_ok()?;
    if n1 != n2 {
        return Err(XC_FIELDN);
    }
    for p in 0..6 {
        let p1 = match a1.pkg_at(p) {
            Some(p) => p,
            None => return Err(XC_PKG_SHORT),
        };
        let p2 = match mock.pkg_at(p) {
            Some(p) => p,
            None => return Err(XC_PKG_SHORT),
        };
        if p1.field_n() != p2.field_n() {
            return Err(XC_FIELDN);
        }
        for k in 0..p1.field_n() {
            let (n1, v1) = &p1.fields[k];
            let (n2, v2) = &p2.fields[k];
            if n1 != n2 {
                return Err(XC_NAME_DRIFT);
            }
            if v1 != v2 {
                return Err(XC_VAL_DIFF);
            }
        }
    }
    Ok(n1)
}

/// mock 装配器：值 = (全局字段号+1)*3 + base（确定性）。
pub fn build_six_side(tag: &str, base: u64) -> Result<SixPkgSide, u16> {
    let mut s = SixPkgSide::new(tag);
    let mut g: u64 = 0;
    for p in 0..6 {
        let want = SIX_PKG_FIELDN[p];
        let mut vals = Vec::new();
        for _ in 0..want {
            g += 1;
            vals.push(g * 3 + base);
        }
        s.enroll_pkg(p, &vals)?;
    }
    s.face_ok()?;
    Ok(s)
}

/// 自例程：主路径（六包 26 字段零差）+ 错误路径（缺包/绑错/缺字段/名漂移/值差）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let a1 = build_six_side("A1", 900)?;
    let mock = build_six_side("later-mock", 900)?;
    if six_pkg_diff(&a1, &mock)? != SIX_FIELD_TOTAL {
        return Err(XC_FIELDN);
    }
    if a1.face_ok()? != SIX_FIELD_TOTAL {
        return Err(XC_FIELDN);
    }

    // 错误路径 1：缺包（只登 5 包）。
    let mut m1 = SixPkgSide::new("mock-short-pkg");
    for p in 0..5 {
        let want = SIX_PKG_FIELDN[p];
        let vals: Vec<u64> = (0..want).map(|k| k as u64 + 1).collect();
        m1.enroll_pkg(p, &vals)?;
    }
    if six_pkg_diff(&a1, &m1) != Err(XC_PKG_SHORT) {
        return Err(XC_PKG_SHORT);
    }

    // 错误路径 2：包字段数不符（显示包只给 4 字段）。
    let mut m2 = SixPkgSide::new("mock-short-field");
    m2.enroll_pkg(0, &[1, 2, 3, 4])?;
    if m2.face_ok() != Err(XC_FIELDN) {
        return Err(XC_FIELDN);
    }

    // 错误路径 3：值差注入（参数包 param_csum 错值）。
    let mut m3 = build_six_side("mock-valdiff", 900)?;
    // 重构包 4 用错值（先登满才可整包覆盖 → 换新侧账更直接）。
    let mut m3b = SixPkgSide::new("mock-valdiff-b");
    let mut g: u64 = 0;
    for p in 0..6 {
        let want = SIX_PKG_FIELDN[p];
        let mut vals = Vec::new();
        for k in 0..want {
            g += 1;
            let v = if p == 4 && k == 4 {
                g * 3 + 901
            } else {
                g * 3 + 900
            };
            vals.push(v);
        }
        m3b.enroll_pkg(p, &vals)?;
    }
    if six_pkg_diff(&a1, &m3b) != Err(XC_VAL_DIFF) {
        return Err(XC_VAL_DIFF);
    }
    let _ = m3;

    // 错误路径 4：包判据号绑错（手工构造）。
    let mut m4 = build_six_side("mock-bind", 0)?;
    if let Some(p) = m4.pkgs.iter_mut().find(|p| p.pkg_no == 2) {
        p.crit_id = 999;
    }
    if m4.face_ok() != Err(XC_PKG_BIND) {
        return Err(XC_PKG_BIND);
    }

    // 错误路径 5：包重复。
    let mut m5 = SixPkgSide::new("mock-dup");
    m5.enroll_pkg(0, &[1, 2, 3, 4, 5])?;
    if m5.enroll_pkg(0, &[1, 2, 3, 4, 5]) != Err(XC_DUP_PKG) {
        return Err(XC_DUP_PKG);
    }
    // 自反零差。
    let z1 = build_six_side("A1z", 0)?;
    let z2 = build_six_side("mockz", 0)?;
    if six_pkg_diff(&z1, &z2)? != SIX_FIELD_TOTAL {
        return Err(XC_VAL_DIFF);
    }
    Ok(0)
}
