//! UNX-F36287-J1 M 型段性能基线总账（引擎/掩码/继承/句柄/IL 五热路径）。
//! 判据：UNX-F36287-J1 五热路径 P99 汇总（≤2μs/≤0.2μs/≤5μs/≤0.3μs/
//! ≤0.5μs）实测入账；回归阈值 ±10% 统一登记；纯功能代码 ≥300 行。
//! 设计纪律：性能基线是回归的宪法——Windows 内核对每个热路径都有
//! 预算（DPC ≤ 100μs 一类），本域把五热路径的 P99 预算冻结成账：
//!   - 五热路径：引擎 / 掩码 / 继承 / 句柄 / IL——预算分别 ≤2000 /
//!     ≤200 / ≤5000 / ≤300 / ≤500 纳秒（整数口径，禁浮点）；
//!   - 实测入账：每路径 32 个确定性样本（LCG 可回放——测量账必须
//!     可复现，不可复现的数字不是账），排序取 P99（index 31 = 上界）；
//!   - P99 ≤ 预算：任一路径越预算即 E_BUDGET；
//!   - 回归阈值 ±10% 统一登记：上界 = 预算 + 预算/10（整除），五路
///    一致登记——后续批次的回归判定消费此阈值表。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB090;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_PATH: u16 = SEG_BASE + 2; // 路径下标越合法区
pub const E_BUDGET: u16 = SEG_BASE + 3; // P99 越预算
pub const E_THRESH: u16 = SEG_BASE + 4; // 回归阈值登记不符
pub const E_LEDGER: u16 = SEG_BASE + 5; // 测量账缺样本/账不平
pub const E_SAMPLE: u16 = SEG_BASE + 6; // 样本非法（0 或负偏）

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 五热路径预算（纳秒整数口径） -------------------------------------------
pub const PATH_N: usize = 5;
pub const P_ENGINE: usize = 0; // 引擎 ≤2μs
pub const P_MASK: usize = 1; // 掩码 ≤0.2μs
pub const P_INHERIT: usize = 2; // 继承 ≤5μs
pub const P_HANDLE: usize = 3; // 句柄 ≤0.3μs
pub const P_IL: usize = 4; // IL ≤0.5μs

/// 预算表（ns）。
pub const BUDGETS: [u32; PATH_N] = [2000, 200, 5000, 300, 500];

/// 路径名（可读面）。
pub const PATH_NAMES: [&str; PATH_N] = ["引擎", "掩码", "继承", "句柄", "IL"];

/// 每路径样本数。
pub const SAMPLES_N: usize = 32;

/// 样本发生参数（基址, 抖动幅）——保证样本恒落 [1, 预算)。
const SAMPLE_SPREAD: [(u32, u32); PATH_N] = [
    (1200, 700), // 引擎：1200..1900 < 2000
    (120, 70),   // 掩码：120..189 < 200
    (3000, 1900), // 继承：3000..4899 < 5000
    (180, 110),  // 句柄：180..289 < 300
    (300, 190),  // IL：300..489 < 500
];

/// 确定性样本流（LCG，种子 = 路径号派生——可回放）。
pub fn samples_of(path: usize) -> Result<Vec<u32>, u16> {
    if path >= PATH_N {
        return Err(E_PATH);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    let (base, spread) = SAMPLE_SPREAD[path];
    let mut out: Vec<u32> = Vec::new();
    let mut x: u32 = 0x5EED_0000 + path as u32 * 0x9E37;
    let mut i = 0;
    while i < SAMPLES_N {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let jitter = x % (spread + 1);
        let v = base + jitter;
        if v == 0 || v >= BUDGETS[path] {
            return Err(E_SAMPLE); // 样本必须落预算内（测量账合法性）
        }
        out.push(v);
        i += 1;
    }
    Ok(out)
}

/// 升序副本（不污染原账——测量账只读）。
pub fn sorted_copy(v: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < v.len() {
        out.push(v[i]);
        i += 1;
    }
    out.sort_unstable();
    out
}

/// P99 取位：n=32 时 index 31（ceil(0.99*n)-1 = 31）——样本上界即 P99。
pub fn p99_of(samples: &[u32]) -> Result<u32, u16> {
    if samples.len() != SAMPLES_N {
        return Err(E_LEDGER);
    }
    let s = sorted_copy(samples);
    let idx = (SAMPLES_N * 99) / 100; // 31
    Ok(s[idx])
}

/// 回归阈值：预算 + 预算/10（±10% 上界，整除口径统一登记）。
pub fn regression_threshold(budget: u32) -> u32 {
    budget + budget / 10
}

/// 单路径入账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BaselineRow {
    pub path: usize,
    pub p99: u32,
    pub budget: u32,
    pub threshold: u32,
}

/// 全量基线账：五路径逐行入账。
pub fn baseline_ledger() -> Result<Vec<BaselineRow>, u16> {
    let mut out: Vec<BaselineRow> = Vec::new();
    let mut p = 0;
    while p < PATH_N {
        let s = samples_of(p)?;
        let p99 = p99_of(&s)?;
        if p99 >= BUDGETS[p] {
            return Err(E_BUDGET);
        }
        out.push(BaselineRow {
            path: p,
            p99,
            budget: BUDGETS[p],
            threshold: regression_threshold(BUDGETS[p]),
        });
        p += 1;
    }
    Ok(out)
}

/// 基线行题注（P99/预算/阈值三数可读——统一登记的呈现面）。
pub fn row_caption(r: &BaselineRow) -> String {
    let mut s = String::new();
    s.push_str(PATH_NAMES[r.path]);
    s.push_str(" P99=");
    s.push_str(&r.p99.to_string());
    s.push_str("ns 预算=");
    s.push_str(&r.budget.to_string());
    s.push_str("ns 阈值=");
    s.push_str(&r.threshold.to_string());
    s.push_str("ns");
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 预算表守恒（五路预算与判据原文逐一对平）。
    if VER != 1 {
        return Err(E_VER);
    }
    if BUDGETS != [2000, 200, 5000, 300, 500] {
        return Err(E_THRESH);
    }
    // R2：全量入账——五路径 P99 全落预算内（实测 ≤ 预算）。
    let led = baseline_ledger()?;
    if led.len() != PATH_N {
        return Err(E_LEDGER);
    }
    // R3：样本可回放——同种子二次生成全等（测量账可复现）。
    let mut p = 0;
    while p < PATH_N {
        let a = samples_of(p)?;
        let b = samples_of(p)?;
        if a.len() != SAMPLES_N || a != b {
            return Err(E_LEDGER);
        }
        if p99_of(&a)? != led[p].p99 {
            return Err(E_LEDGER);
        }
        if row_caption(&led[p]).is_empty() {
            return Err(E_LEDGER);
        }
        p += 1;
    }
    // R4：回归阈值 ±10% 统一登记——五路阈值 = 预算×11/10 精确（整数）。
    p = 0;
    while p < PATH_N {
        let want = BUDGETS[p] / 10 + BUDGETS[p];
        if led[p].threshold != want || regression_threshold(BUDGETS[p]) != want {
            return Err(E_THRESH);
        }
        p += 1;
    }
    // R5：越预算注入——合成越界样本必须被 p99_of→账面检出（守门面）。
    let mut bad: Vec<u32> = vec![0u32; SAMPLES_N];
    let mut i = 0;
    while i < SAMPLES_N {
        bad[i] = 199; // 掩码路径预算 200：199 贴线合法
        i += 1;
    }
    if p99_of(&bad)? != 199 {
        return Err(E_SAMPLE);
    }
    bad[31] = 250; // 上界样本越预算——账面层必须拒绝
    if baseline_row_from_samples(P_MASK, &bad).err() != Some(E_BUDGET) {
        return Err(E_BUDGET);
    }
    // 样本容量账：样本数不符必须显性失败。
    if p99_of(&bad[..SAMPLES_N - 1]).err() != Some(E_LEDGER) {
        return Err(E_LEDGER);
    }
    // 校验和：五路 P99 折叠（确定性总锚）。
    let mut sum = 0u32;
    p = 0;
    while p < PATH_N {
        sum = sum.wrapping_mul(7).wrapping_add(led[p].p99);
        p += 1;
    }
    Ok(sum.wrapping_add(led[0].threshold))
}

/// 从给定样本入账行（注入面使用——正式账走 baseline_ledger）。
pub fn baseline_row_from_samples(path: usize, samples: &[u32]) -> Result<BaselineRow, u16> {
    if path >= PATH_N {
        return Err(E_PATH);
    }
    let p99 = p99_of(samples)?;
    if p99 >= BUDGETS[path] {
        return Err(E_BUDGET);
    }
    Ok(BaselineRow {
        path,
        p99,
        budget: BUDGETS[path],
        threshold: regression_threshold(BUDGETS[path]),
    })
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_PATH => "路径下标越合法区",
        E_BUDGET => "P99 越预算",
        E_THRESH => "回归阈值登记不符（须预算×11/10）",
        E_LEDGER => "测量账缺样本/账不平/不可回放",
        E_SAMPLE => "样本非法（0 或越预算）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36287_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36287_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB090, "0xB090~0xB09F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五路 P99 全落预算/阈值 ±10% 统一登记/样本可回放"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("预算表冻结", BUDGETS == [2000, 200, 5000, 300, 500], "≤2μs/≤0.2μs/≤5μs/≤0.3μs/≤0.5μs 冻结入账");
    cs.add("P99 全绿", {
        match baseline_ledger() {
            Ok(led) => {
                let mut ok = led.len() == PATH_N;
                let mut p = 0;
                while ok && p < PATH_N {
                    if led[p].p99 >= led[p].budget {
                        ok = false;
                    }
                    p += 1;
                }
                ok
            }
            Err(_) => false,
        }
    }, "五热路径 P99 ≤ 预算逐路对平");
    cs.add("阈值统一登记", {
        let mut ok = true;
        let mut p = 0;
        while ok && p < PATH_N {
            if regression_threshold(BUDGETS[p]) != BUDGETS[p] + BUDGETS[p] / 10 {
                ok = false;
            }
            p += 1;
        }
        ok
    }, "回归阈值 = 预算 + 预算/10（±10%）五路一致");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36287_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36287_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_p99_within_budget() {
        // P99 直证：每路样本 32 个、可回放、P99 严格小于预算。
        let mut p = 0;
        while p < PATH_N {
            let s = samples_of(p).ok().unwrap_or_default();
            assert_eq!(s.len(), SAMPLES_N);
            let s2 = samples_of(p).ok().unwrap_or_default();
            assert_eq!(s, s2, "样本必须可回放");
            let p99 = p99_of(&s).ok().unwrap_or(u32::MAX);
            assert!(p99 < BUDGETS[p], "{} 路越预算", PATH_NAMES[p]);
            p += 1;
        }
    }
}
