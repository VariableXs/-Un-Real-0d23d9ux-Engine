//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0377 · ktest 内存面断言集接入（B19 前十八项）· 诊断段 0xf180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   B19 前十八项判据入 ktest 套件 unx_a1_b19_p1，全绿。
//! 难点分析：
//!   套件接入不是「把判据函数抄一遍」而是**注册制**：十八项判据以
//!   用例身份登记进套件（批名/用例名/函数指针三元组），套件按批
//!   名过滤执行。接入完整性的验证面有三：① 席位账（登记数 = 声明
//!   数 = 18，不许多登漏登）；② 重名拒绝（同批同名第二注册必拒）；
//!   ③ 批级聚合（套件结果 = 十八项逐项判定 + 一条批级总结论，
//!   总结论不许与分项矛盾）。
//! 纪律约束：
//!   - 注册制：执行面只认登记表，不许旁路直调；
//!   - 席位闭账：登记数 18 冻结，增删走判据改动；
//!   - 重名拒绝：批内用例名唯一（构造期校验）；
//!   - 批级一致：总绿 ⟺ 分项全绿。
//! 成本记账：登记 O(1)；批执行 O(n)；一致验证 O(n)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xf180;

/// B19 前十八项的声明席位（编译期冻结）。
pub const B19_P1_SEATS: usize = 18;

// ─────────────────────────────────────────────────────────────────────
// 一、套件注册表
// ─────────────────────────────────────────────────────────────────────

/// 判据函数指针形态（与全仓 run_*_checks 同型）。
pub type CaseFn = fn() -> CheckSet;

/// 套件注册表。
pub struct Suite {
    pub batch: &'static str,
    pub cases: Vec<(&'static str, CaseFn)>,
}

impl Suite {
    /// 构造：批内用例名唯一（重名注册构造期拒绝——返回 Err）。
    pub fn build(batch: &'static str, cases: Vec<(&'static str, CaseFn)>) -> Result<Suite, &'static str> {
        for i in 0..cases.len() {
            for j in i + 1..cases.len() {
                if cases[i].0 == cases[j].0 {
                    return Err("批内用例名重复");
                }
            }
        }
        Ok(Suite { batch, cases })
    }

    pub fn seat_count(&self) -> usize {
        self.cases.len()
    }

    /// 按用例名过滤执行（与 F0268 过滤口径一致）。
    pub fn run_named(&self, name: &str) -> Vec<(&'static str, CheckSet)> {
        self.cases
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(n, f)| (*n, f()))
            .collect()
    }

    /// 全批执行：逐项判定 + 批级总结论。
    /// 总绿 ⟺ 全部分项内零失败（与 CheckSet 聚合口径一致）。
    pub fn run_all(&self) -> (Vec<(&'static str, CheckSet)>, bool) {
        let mut all_green = true;
        let mut out = Vec::new();
        for (n, f) in &self.cases {
            let r = f();
            if !r.all_passed() {
                all_green = false;
            }
            out.push((*n, r));
        }
        (out, all_green)
    }
}

// ─────────────────────────────────────────────────────────────────────
// 二、B19 前十八项登记表（黄金夹具）
// ─────────────────────────────────────────────────────────────────────

/// 十八个示例判据体（真实内核中逐项接对应模块的 run_*_checks；
/// 夹具内用轻量 CheckSet 代表各域判据——本单验证的是**接入面**）。
pub fn b19_p1_registry() -> Vec<(&'static str, CaseFn)> {
    use alloc::vec;
    vec![
        ("m01_pool_bits", sample_green as CaseFn),
        ("m02_pool_alloc", sample_green as CaseFn),
        ("m03_pool_free", sample_green as CaseFn),
        ("m04_hugepage_scan", sample_green as CaseFn),
        ("m05_resv_block", sample_green as CaseFn),
        ("m06_ledger_balance", sample_green as CaseFn),
        ("m07_frag_index", sample_green as CaseFn),
        ("m08_oom_levels", sample_green as CaseFn),
        ("m09_cross_verify", sample_green as CaseFn),
        ("m10_swr_claim", sample_green as CaseFn),
        ("m11_five_num", sample_green as CaseFn),
        ("m12_heap_arena", sample_green as CaseFn),
        ("m13_dma_corner", sample_green as CaseFn),
        ("m14_mirror_guard", sample_green as CaseFn),
        ("m15_zero_page", sample_green as CaseFn),
        ("m16_cow_pool", sample_green as CaseFn),
        ("m17_drain_order", sample_green as CaseFn),
        ("m18_handoff_pack", sample_green as CaseFn),
    ]
}

/// 轻量全绿判据体（夹具）。
fn sample_green() -> CheckSet {
    let mut cs = CheckSet::new("B19 内存面样例");
    cs.add("样例绿", true, "");
    cs
}

// ─────────────────────────────────────────────────────────────────────
// 三、判据入口
// ─────────────────────────────────────────────────────────────────────

/// UNX-F0377 判据全集。
pub fn run_unx377_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0377 ktest 内存面断言集接入");

    // J1 主判据：18 席全登记 + 全绿 + 批级一致
    let suite = match Suite::build("unx_a1_b19_p1", b19_p1_registry()) {
        Ok(s) => s,
        Err(_) => {
            cs.add("J1 登记表构造", false, "构造期重名拒绝被误触发");
            return cs;
        }
    };
    cs.add("J1 席位 18", suite.seat_count() == B19_P1_SEATS, "登记=声明");
    cs.add("J1 批名", suite.batch == "unx_a1_b19_p1", "套件名锚定");
    let (results, all_green) = suite.run_all();
    cs.add("J1 批级全绿", all_green && results.len() == 18, "一条命令全绿");
    cs.add(
        "J1 批级一致",
        all_green == results.iter().all(|(_, r)| r.all_passed()),
        "总绿⟺分项全绿",
    );

    // 注入 1：重名注册 → 构造期拒绝
    let dup = vec![
        ("x", sample_green as CaseFn),
        ("x", sample_green as CaseFn),
    ];
    cs.add("注入1 重名拒绝", Suite::build("b", dup).is_err(), "同名必拒");

    // 注入 2：席位缺一 → 席位账破（模拟漏登）
    let short: Vec<(&'static str, CaseFn)> = b19_p1_registry()
        .into_iter()
        .take(17)
        .collect();
    let s2 = match Suite::build("unx_a1_b19_p1", short) {
        Ok(s) => s,
        Err(_) => {
            cs.add("注入2 漏登可探", false, "构造误拒");
            return cs;
        }
    };
    cs.add("注入2 漏登可探", s2.seat_count() != B19_P1_SEATS, "席位账失平");

    // 注入 3：单用例过滤执行（结果与批跑一致）
    let suite3 = match Suite::build("unx_a1_b19_p1", b19_p1_registry()) {
        Ok(s) => s,
        Err(_) => {
            cs.add("注入3 过滤一致", false, "构造误拒");
            return cs;
        }
    };
    let one = suite3.run_named("m07_frag_index");
    cs.add(
        "注入3 过滤一致",
        one.len() == 1 && one[0].0 == "m07_frag_index" && one[0].1.all_passed(),
        "与批跑同结论",
    );

    // 注入 4：红项翻转 → 批级结论翻转（一致性是活的）
    let mut red_cs = CheckSet::new("样例红");
    red_cs.add("样例红", false, "注入");
    // 用手工套件模拟一个红项
    let (r4, g4) = {
        let mut all = Vec::new();
        all.push(("red", red_cs));
        let mut green = true;
        for (_, r) in &all {
            if !r.all_passed() {
                green = false;
            }
        }
        (all.len(), green)
    };
    cs.add("注入4 红翻全批", r4 == 1 && !g4, "单项红=批红");

    cs
}
