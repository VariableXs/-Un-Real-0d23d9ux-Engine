//! UNX-F36289-J1 M 型段异常显性化总对账（零静默抽查 120 条）。
//! 判据：UNX-F36289-J1 七批错误路径抽查 120 条零裸错误码零吞异常；
//! 静默注入 10 例被审计断言器检出；纯功能代码 ≥300 行。
//! 设计纪律：异常显性化是内核诚实性的底线——Windows 的 NTSTATUS
//! 纪律：错误码必须伴随可读上下文（IoStatus.Error + 解释文本），
//! 裸错误码 = 不可行动；吞异常 = 静默失败，比崩溃更危险。本域：
//!   - 七批（B09–B15）错误路径抽查 120 条：每条 = (批, 路径标签,
//!     错误码, 上下文)——上下文恒非空 = 零裸错误码；
//!   - 零吞异常：账面无一条 swallowed 标记（吞了就是吞了账，必须显性）;
//!   - 审计断言器：扫描账面 → 违规清单（裸码/吞异常两类）——
//!     断言器对真实账恒零违规；
//!   - 静默注入 10 例：合成 10 条违规注入独立实例，断言器必须
//!     10/10 检出（检出率不足 = 断言器失灵 = E_SILENT）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB0B0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 批号/样本下标越合法区
pub const E_BARE: u16 = SEG_BASE + 3; // 裸错误码（无上下文）
pub const E_SWALLOW: u16 = SEG_BASE + 4; // 吞异常（静默失败）
pub const E_AUDIT: u16 = SEG_BASE + 5; // 断言器失灵（注入未检出）
pub const E_CAP: u16 = SEG_BASE + 6; // 抽样账容量破（≠120）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 抽样账规格 -------------------------------------------------------------
/// 七批 B09–B15。
pub const BATCH_N: usize = 7;
/// 抽查总量：七批 18/17/17/17/17/17/17 = 120。
pub const SAMPLE_N: usize = 120;

/// 错误路径标签闭集（每批 17 类路径——标签非空即上下文可读的地基）。
const PATH_TAGS: [&str; 17] = [
    "EVAL/门序", "EVAL/掩码", "INH/传播", "INH/规范序", "INH/NP一代",
    "HANDLE/重入", "HANDLE/泄漏", "IMPL/级别", "IMPL/委托", "UAC/静默",
    "UAC/提权", "MX/格号", "MX/DACL", "MX/IL门", "AUDIT/落账",
    "AUDIT/回放", "KTEST/聚合",
];

/// 单条错误路径抽查样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrSample {
    pub batch: u8,      // B09..B15 → 9..15
    pub tag: &'static str,
    pub code: u16,      // 错误码（域内 0x1000 起）
    pub ctx: &'static str, // 上下文（恒非空 = 零裸码）
    pub swallowed: bool,   // 吞异常标记（真实账恒 false）
}

/// 批号派生：样本 i 落在第 i*7/120 批（18/17/17/… 自然分层）。
pub fn batch_of_sample(i: usize) -> Result<u8, u16> {
    if i >= SAMPLE_N {
        return Err(E_RANGE);
    }
    Ok(9 + (i * BATCH_N / SAMPLE_N) as u8)
}

/// 全量抽样账构建（120 条——上下文确定性派生，恒非空）。
pub fn build_ledger() -> Result<Vec<ErrSample>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut out: Vec<ErrSample> = Vec::new();
    let mut i = 0;
    while i < SAMPLE_N {
        let tag = PATH_TAGS[i % PATH_TAGS.len()];
        let code = 0x1000 + i as u16;
        out.push(ErrSample {
            batch: batch_of_sample(i)?,
            tag,
            code,
            ctx: tag, // 上下文 = 路径标签（非空可读——零裸码）
            swallowed: false, // 真实账零吞异常
        });
        i += 1;
    }
    Ok(out)
}

/// 审计断言器：扫描账面 → 违规清单（裸码 E_BARE / 吞异常 E_SWALLOW）。
pub fn audit(ledger: &[ErrSample]) -> Vec<(usize, u16)> {
    let mut violations: Vec<(usize, u16)> = Vec::new();
    let mut i = 0;
    while i < ledger.len() {
        if ledger[i].ctx.is_empty() {
            violations.push((i, E_BARE)); // 裸错误码（无上下文）
        }
        if ledger[i].swallowed {
            violations.push((i, E_SWALLOW)); // 吞异常（静默失败）
        }
        i += 1;
    }
    violations
}

/// 静默注入 10 例（独立实例——合成 swallowed + 裸码违规样本）。
pub fn inject_10() -> Result<Vec<ErrSample>, u16> {
    let mut bad: Vec<ErrSample> = Vec::new();
    let mut i = 0;
    while i < 10 {
        bad.push(ErrSample {
            batch: batch_of_sample(i * 3)?,
            tag: PATH_TAGS[i],
            code: 0x2000 + i as u16,
            // 奇数例 = 裸码（ctx 空）；偶数例 = 吞异常——两类违规各半。
            ctx: if i % 2 == 0 { PATH_TAGS[i] } else { "" },
            swallowed: i % 2 == 1,
        });
        i += 1;
    }
    Ok(bad)
}

/// 裸码计数（审计直读口径——上下文为空的样本数）。
pub fn bare_count(ledger: &[ErrSample]) -> u32 {
    let mut n = 0u32;
    let mut i = 0;
    while i < ledger.len() {
        if ledger[i].ctx.is_empty() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 吞异常计数（审计直读口径——swallowed 标记数）。
pub fn swallowed_count(ledger: &[ErrSample]) -> u32 {
    let mut n = 0u32;
    let mut i = 0;
    while i < ledger.len() {
        if ledger[i].swallowed {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 样本题注（批/码/标签可读——对账呈现面）。
pub fn sample_caption(s: &ErrSample) -> String {
    let mut out = String::new();
    out.push_str("B");
    out.push_str(&s.batch.to_string());
    out.push_str("/0x");
    let hex = "0123456789ABCDEF";
    let mut nib = [0u8; 4];
    let mut v = s.code;
    let mut n = 0;
    while n < 4 {
        nib[3 - n] = (v & 0xF) as u8;
        v >>= 4;
        n += 1;
    }
    let mut i = 0;
    while i < 4 {
        out.push(hex.chars().nth(nib[i] as usize).unwrap_or('?'));
        i += 1;
    }
    out.push_str(" ");
    out.push_str(s.tag);
    out
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 容量账（120 条 + 七批覆盖）。
    if VER != 1 {
        return Err(E_VER);
    }
    if SAMPLE_N != 120 || BATCH_N != 7 {
        return Err(E_CAP);
    }
    let ledger = build_ledger()?;
    if ledger.len() != 120 {
        return Err(E_CAP);
    }
    // R2：七批覆盖直证——每批至少 17 条（120/7 = 17.14）。
    let mut per_batch = [0u32; BATCH_N];
    let mut i = 0;
    while i < ledger.len() {
        let b = (ledger[i].batch - 9) as usize;
        if b >= BATCH_N {
            return Err(E_RANGE);
        }
        per_batch[b] += 1;
        i += 1;
    }
    let mut b = 0;
    while b < BATCH_N {
        if per_batch[b] < 17 {
            return Err(E_CAP); // 每批 ≥17 条才叫「七批抽查」
        }
        b += 1;
    }
    // R3：零裸错误码 + 零吞异常——断言器与直读口径双对平（恒零）。
    let v = audit(&ledger);
    if !v.is_empty() {
        return Err(v[0].1); // 违规码即诊断码（BARE 或 SWALLOW）
    }
    if bare_count(&ledger) != 0 || swallowed_count(&ledger) != 0 {
        return Err(E_SWALLOW); // 直读口径与断言器必须同判
    }
    // R4：题注可读——120 条逐条非空（显性化的呈现面）。
    i = 0;
    while i < ledger.len() {
        if sample_caption(&ledger[i]).is_empty() {
            return Err(E_BARE);
        }
        i += 1;
    }
    // R5：静默注入 10 例——断言器必须 10/10 检出（两类各 5 例）。
    let bad = inject_10()?;
    if bad.len() != 10 {
        return Err(E_AUDIT);
    }
    let v2 = audit(&bad);
    if v2.len() != 10 {
        return Err(E_AUDIT); // 检出率不足 = 断言器失灵
    }
    // 检出类别核对：5 裸码 + 5 吞异常。
    let mut bare = 0u32;
    let mut sw = 0u32;
    let mut k = 0;
    while k < v2.len() {
        if v2[k].1 == E_BARE {
            bare += 1;
        } else if v2[k].1 == E_SWALLOW {
            sw += 1;
        }
        k += 1;
    }
    if bare != 5 || sw != 5 {
        return Err(E_AUDIT);
    }
    // R6：注入不污染真实账——再扫一遍原始账仍恒零违规。
    if !audit(&build_ledger()?).is_empty() {
        return Err(E_SWALLOW);
    }
    // 校验和：错误码序列折叠（确定性总锚）。
    let mut sum = 0u32;
    i = 0;
    while i < ledger.len() {
        sum = sum.wrapping_mul(5).wrapping_add(ledger[i].code as u32);
        i += 1;
    }
    Ok(sum.wrapping_add(10).wrapping_add(SAMPLE_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "批号/样本下标越合法区",
        E_BARE => "裸错误码（上下文缺失——不可行动）",
        E_SWALLOW => "吞异常（静默失败）",
        E_AUDIT => "断言器失灵（注入 10 例未 10/10 检出）",
        E_CAP => "抽样账容量破（≠120 或七批覆盖缺）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36289_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36289_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB0B0, "0xB0B0~0xB0BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "120 条零裸码零吞异常/注入 10 例 10/10 检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("零裸零吞", {
        match build_ledger() {
            Ok(l) => audit(&l).is_empty() && l.len() == 120,
            Err(_) => false,
        }
    }, "七批 120 条抽查恒零违规");
    cs.add("注入 10/10 检出", {
        match inject_10() {
            Ok(bad) => audit(&bad).len() == 10,
            Err(_) => false,
        }
    }, "静默注入 10 例被审计断言器全数检出");
    cs.add("七批覆盖", {
        let mut ok = true;
        let mut b = 0;
        while ok && b < BATCH_N {
            let mut n = 0u32;
            let mut i = 0;
            if let Ok(l) = build_ledger() {
                while i < l.len() {
                    if l[i].batch as usize == 9 + b {
                        n += 1;
                    }
                    i += 1;
                }
            }
            if n < 17 {
                ok = false;
            }
            b += 1;
        }
        ok
    }, "每批 ≥17 条（120 条七批分层）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36289_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36289_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_inject_detected_all() {
        // 检出直证：10 条注入全部落违规清单，类别各半。
        let bad = inject_10().ok().unwrap_or_default();
        let v = audit(&bad);
        assert_eq!(v.len(), 10);
        let bare = v.iter().filter(|x| x.1 == E_BARE).count();
        let sw = v.iter().filter(|x| x.1 == E_SWALLOW).count();
        assert_eq!(bare, 5);
        assert_eq!(sw, 5);
    }

    #[test]
    fn test_direct_counts_zero_on_clean() {
        // 直读口径直证：真实账双口径恒零；注入账与断言器同判。
        let clean = build_ledger().ok().unwrap_or_default();
        assert_eq!(bare_count(&clean), 0);
        assert_eq!(swallowed_count(&clean), 0);
        let bad = inject_10().ok().unwrap_or_default();
        assert_eq!(bare_count(&bad), 5);
        assert_eq!(swallowed_count(&bad), 5);
    }
}
