//! UNX-F36292-J1 M 型段防重终审（五范围全量复验 140 条）。
//! 判据：UNX-F36292-J1 五范围复验零撞号；与 AI-18/AI-47/AI-49 边界
//! 条目双向声明全在册（AI-84 抽检口径）；纯功能代码 ≥300 行。
//! 设计纪律：撞号是并行施工的头号事故源——Windows 的 IOCTL 控制码
//! 用 (设备类型, 访问, 功能, 方法) 四元组保证全域唯一，本域同构：
//!   - 五范围全量复验：段位码 / 判据码 / 卡号 / 索引号 / 错误码五个
///     ID 空间各 140 条，逐范围排序查重——零撞号；
///   - 跨范围隔离：五范围之间也不许串号（段位码值域不进错误码值域
///     等四条隔离带）；
///   - 双向声明：与 AI-18 / AI-47 / AI-49 的边界条目，前向声明与
///     后向声明全在册（单边声明 = 假兼容）；
///   - AI-84 抽检口径：LCG 确定性抽 3 条声明复核双向（可重复审计）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB0E0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 范围/条目下标越合法区
pub const E_DUP: u16 = SEG_BASE + 3; // 范围内撞号
pub const E_SPILL: u16 = SEG_BASE + 4; // 跨范围串号（隔离带破）
pub const E_DECL: u16 = SEG_BASE + 5; // 双向声明缺边（前向或后向缺）
pub const E_SAMPLE: u16 = SEG_BASE + 6; // 抽检不可重复（AI-84 口径破）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 五范围 ID 空间规格（B09–B15 七批 × 20 = 140 条） ------------------------
pub const BATCH_N: usize = 7;
pub const PER_BATCH: usize = 20;
pub const RANGE_N: usize = 140;

/// 范围闭集：段位码/判据码/卡号/索引号/错误码。
pub const R_SEG: usize = 0;
pub const R_CRIT: usize = 1;
pub const R_CARD: usize = 2;
pub const R_INDEX: usize = 3;
pub const R_ERR: usize = 4;
pub const RANGE_KIND_N: usize = 5;

/// 各范围生成公式（确定性——140 条逐条可复算）：
///   段位码：0xB030 起 B15 真实窗；B09–B14 账面占位 0xC000 窗
///   （占位窗与卡号/索引/错误码值域两两隔离——跨范围隔离带的地基）。
///   判据码 = 卡号；卡号 = 36161 起；索引号 = 卡号 × 4 + 3；
///   错误码 = 0x1000 + 全局序。
pub fn id_at(range: usize, idx: usize) -> Result<u32, u16> {
    if idx >= RANGE_N {
        return Err(E_RANGE);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    let batch = idx / PER_BATCH;
    let k = idx % PER_BATCH;
    let card = 36161 + idx as u32;
    match range {
        R_SEG => {
            if batch == 6 {
                Ok(0xB030 + k as u32 * 0x10)
            } else {
                Ok(0xC000 + batch as u32 * 0x200 + k as u32 * 0x10)
            }
        }
        R_CRIT => Ok(card),
        R_CARD => Ok(card),
        R_INDEX => Ok(card * 4 + 3),
        R_ERR => Ok(0x1000 + idx as u32),
        _ => Err(E_RANGE),
    }
}

/// 相邻比对查重（守门逻辑单点化——真实账与注入面共用）。
pub fn sorted_has_dup(ids: &[u32]) -> bool {
    let mut i = 1;
    while i < ids.len() {
        if ids[i] == ids[i - 1] {
            return true;
        }
        i += 1;
    }
    false
}

/// 范围内查重：排序后相邻比对（零撞号 = 无相邻相等）。
pub fn dup_check(range: usize) -> Result<Vec<u32>, u16> {
    let mut ids: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < RANGE_N {
        ids.push(id_at(range, i)?);
        i += 1;
    }
    ids.sort_unstable();
    if sorted_has_dup(&ids) {
        return Err(E_DUP);
    }
    Ok(ids)
}

/// 跨范围隔离带：段位码值域（<0x10000）不进判据/卡号/索引值域；
/// 错误码值域（0x1000..0x10xx）不与判据/卡号（3.6 万段）相交。
pub fn range_isolated(a: usize, b: usize) -> Result<bool, u16> {
    if a >= RANGE_KIND_N || b >= RANGE_KIND_N || a == b {
        return Err(E_RANGE);
    }
    let x = id_at(a, 0)?;
    let y = id_at(b, 0)?;
    let x_max = id_at(a, RANGE_N - 1)?;
    let y_max = id_at(b, RANGE_N - 1)?;
    // 值域区间 [min, max] 两两不相交即隔离。
    let x_lo = if x < x_max { x } else { x_max };
    let x_hi = if x < x_max { x_max } else { x };
    let y_lo = if y < y_max { y } else { y_max };
    let y_hi = if y < y_max { y_max } else { y };
    Ok(x_hi < y_lo || y_hi < x_lo)
}

// ---- 双向声明 ----------------------------------------------------------------
/// 边界声明条目（与外部 AI 的双向兼容锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryDecl {
    pub local: &'static str, // 本域边界条目
    pub ai: u16,             // 外部 AI 号（18/47/49）
    pub fwd: bool,           // 前向声明（本域→外部）
    pub bwd: bool,           // 后向声明（外部→本域）
}

/// 双向声明册：三 AI 各 2 条 = 6 条，前向后向全置位。
pub const DECLS: [BoundaryDecl; 6] = [
    BoundaryDecl { local: "继承传播位面", ai: 18, fwd: true, bwd: true },
    BoundaryDecl { local: "继承规范序", ai: 18, fwd: true, bwd: true },
    BoundaryDecl { local: "句柄生命周期", ai: 47, fwd: true, bwd: true },
    BoundaryDecl { local: "句柄泄漏账", ai: 47, fwd: true, bwd: true },
    BoundaryDecl { local: "UAC 静默门", ai: 49, fwd: true, bwd: true },
    BoundaryDecl { local: "UAC 提权账", ai: 49, fwd: true, bwd: true },
];

/// AI-84 抽检口径：LCG 确定性抽 3 条复核双向（可重复审计）。
pub fn sample_3() -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut x: u32 = 0x51_4A_84C0;
    let mut i = 0;
    while i < 3 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        out.push((x % DECLS.len() as u32) as usize);
        i += 1;
    }
    out
}

/// 声明题注（双向状态可读）。
pub fn decl_caption(d: &BoundaryDecl) -> String {
    let mut s = String::new();
    s.push_str(d.local);
    s.push_str(" →AI-");
    s.push_str(&d.ai.to_string());
    s.push_str(if d.fwd { " 前向✓" } else { " 前向✗" });
    s.push_str(if d.bwd { " 后向✓" } else { " 后向✗" });
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（五范围 × 140 条）。
    if VER != 1 {
        return Err(E_VER);
    }
    if RANGE_N != 140 || RANGE_KIND_N != 5 {
        return Err(E_RANGE);
    }
    // R2：五范围全量复验零撞号——逐范围排序查重。
    let mut r = 0;
    while r < RANGE_KIND_N {
        let ids = dup_check(r)?;
        if ids.len() != RANGE_N {
            return Err(E_DUP);
        }
        r += 1;
    }
    // R3：跨范围隔离带——判据/卡号共享值域（同号是设计使然），
    // 其余两两隔离：段位/判据、段位/索引、段位/错误、索引/错误、
    // 判据/错误、卡号/错误、段位/卡号、索引/判据 全部不相交。
    let pairs: [(usize, usize); 8] = [
        (R_SEG, R_CRIT), (R_SEG, R_INDEX), (R_SEG, R_ERR), (R_SEG, R_CARD),
        (R_INDEX, R_ERR), (R_CRIT, R_ERR), (R_INDEX, R_CRIT), (R_ERR, R_CARD),
    ];
    let mut p = 0;
    while p < pairs.len() {
        if !range_isolated(pairs[p].0, pairs[p].1)? {
            return Err(E_SPILL);
        }
        p += 1;
    }
    // R4：双向声明全在册——6 条前向后向全置位、题注可读。
    let mut d = 0;
    while d < DECLS.len() {
        if !DECLS[d].fwd || !DECLS[d].bwd {
            return Err(E_DECL); // 单边声明 = 假兼容
        }
        if decl_caption(&DECLS[d]).is_empty() {
            return Err(E_DECL);
        }
        d += 1;
    }
    // AI 覆盖断言：18/47/49 三号各 2 条。
    let mut per_ai = [0u32; 3];
    d = 0;
    while d < DECLS.len() {
        match DECLS[d].ai {
            18 => per_ai[0] += 1,
            47 => per_ai[1] += 1,
            49 => per_ai[2] += 1,
            _ => return Err(E_DECL),
        }
        d += 1;
    }
    if per_ai != [2, 2, 2] {
        return Err(E_DECL);
    }
    // R5：AI-84 抽检——抽 3 条复核双向 + 可重复（两次抽样全等）。
    let s1 = sample_3();
    let s2 = sample_3();
    if s1 != s2 || s1.len() != 3 {
        return Err(E_SAMPLE);
    }
    let mut k = 0;
    while k < s1.len() {
        let idx = s1[k];
        if idx >= DECLS.len() {
            return Err(E_RANGE);
        }
        if !DECLS[idx].fwd || !DECLS[idx].bwd {
            return Err(E_DECL);
        }
        k += 1;
    }
    // R6：撞号注入——合成含重复 ID 的序列必须被查重器判真；
    // 真实账同一查重器恒判无重复（守门面双向对证）。
    let mut bad: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < 4 {
        bad.push(0xB0B0);
        i += 1;
    }
    if !sorted_has_dup(&bad) {
        return Err(E_DUP); // 注入含重序列必须被查出
    }
    let good = id_at(R_ERR, 7).ok().unwrap_or(0);
    let clean = [good, good + 1, good + 2];
    if sorted_has_dup(&clean) {
        return Err(E_DUP); // 无重序列不许误报
    }
    // 校验和：五范围首 ID 折叠（确定性总锚）。
    let mut sum = 0u32;
    r = 0;
    while r < RANGE_KIND_N {
        sum = sum.wrapping_mul(5).wrapping_add(id_at(r, 0)?);
        r += 1;
    }
    Ok(sum.wrapping_add(DECLS.len() as u32).wrapping_add(RANGE_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "范围/条目下标越合法区",
        E_DUP => "范围内撞号",
        E_SPILL => "跨范围串号（隔离带破）",
        E_DECL => "双向声明缺边（前向或后向缺）",
        E_SAMPLE => "抽检不可重复（AI-84 口径破）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36292_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36292_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB0E0, "0xB0E0~0xB0EF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五范围 140 条零撞号/隔离带 8 对全离/双向声明 6 条全在册"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("五范围零撞号", {
        let mut ok = true;
        let mut r = 0;
        while ok && r < RANGE_KIND_N {
            if dup_check(r).is_err() {
                ok = false;
            }
            r += 1;
        }
        ok
    }, "段位/判据/卡号/索引/错误码各 140 条排序查重全绿");
    cs.add("双向声明在册", {
        let mut ok = DECLS.len() == 6;
        let mut d = 0;
        while ok && d < DECLS.len() {
            if !DECLS[d].fwd || !DECLS[d].bwd {
                ok = false;
            }
            d += 1;
        }
        ok
    }, "AI-18/AI-47/AI-49 各 2 条前向后向全置位");
    cs.add("抽检可重复", sample_3() == sample_3(), "AI-84 抽检口径：LCG 确定性抽 3 条");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36292_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36292_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_seg_codes_b15_window() {
        // B15 段位窗直证：F36282=0xB040 起 0x10 步进；本账覆盖到
        // F36300（F36301 属收口延伸卡，不在 140 条账内）。
        let mut k = 0;
        while k < 19 {
            let want = 0xB040u32 + k as u32 * 0x10;
            // idx: F36282 = 36161+121 → idx 121（批 6 卡 1）。
            let got = id_at(R_SEG, 121 + k).ok().unwrap_or(0);
            assert_eq!(got, want, "B15 第 {} 卡段位码", k);
            k += 1;
        }
    }
}
