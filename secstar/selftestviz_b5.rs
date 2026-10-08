//! F172 引导自检可视化 · 批次五深化（secstar · G-G-02）。
//!
//! 批次五功能面（达成率 43%——主攻批次。与 b3「账与闪」、b4「跑与环」
//! 互补，本批管「目录与分诊」）：
//! - [`ITEM_CATALOG`]：36 项全目录——名称/套件/序号三元组定长表
//!   （名册的代码面：可视化层的每一格都有名字有出处）；
//! - [`FailureDiagnostics`]：失败分诊——失败项 → 可能原因 → 建议动作
//!   三要素（第 9 章错误三要素的自检面落地）；
//! - [`RetestFlow`]：重测流——失败项单独重测，通过后清失败账
//!   （重测不是重跑全部——精确到项的恢复路径）；
//! - [`CatalogConsistency`]：目录-套件项数一致性（名册-预期计数对拍
//!   的目录版：36 项与四套件定义逐项等值）。
//!
//! 零堆纪律：定长目录 + 定长诊断，无 alloc。

use super::selftestviz::{SUITE_MEM_ITEMS, SUITE_PROC_ITEMS, SUITE_STORE_ITEMS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 36 项全目录
// ---------------------------------------------------------------------------

/// 总项数（与 b4 TOTAL_ITEMS 同源）。
pub const CATALOG_N: usize = SUITE_MEM_ITEMS + SUITE_PROC_ITEMS + SUITE_STORE_ITEMS + 6;

/// 套件序（0-3——与 b4 Suite 同值不同处，本地枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatSuite {
    Mem,
    Proc,
    Store,
    Input,
}

impl CatSuite {
    pub fn items(self) -> usize {
        match self {
            CatSuite::Mem => SUITE_MEM_ITEMS,
            CatSuite::Proc => SUITE_PROC_ITEMS,
            CatSuite::Store => SUITE_STORE_ITEMS,
            CatSuite::Input => 6,
        }
    }
}

/// 目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogItem {
    pub suite: CatSuite,
    pub seq: usize,
    pub name: &'static str,
}

/// 内存套件 9 项（主册 kinfo 名册逐名）。
pub const MEM_NAMES: [&str; 9] = [
    "phys-map", "pgtables", "kheap", "zombie-ok", "frame-cache", "page-tables", "kstacks", "vm-layout", "guard-pages",
];
/// 处理器套件 11 项。
pub const PROC_NAMES: [&str; 11] = [
    "sched-ready", "ctx-switch", "syscall-tbl", "ipc-ports", "irq-map", "timer-cal", "prio-inherit", "affinity", "stack-guard", "fpu-save", "zombie-ok",
];
/// 存储套件 10 项。
pub const STORE_NAMES: [&str; 10] = [
    "vfs-root", "fat-mount", "ext4-mount", "journal", "cache-hash", "dirty-flush", "path-resolve", "handle-table", "fs-lock", "quota",
];
/// 输入套件 6 项。
pub const INPUT_NAMES: [&str; 6] = ["kbd-map", "ptr-bounds", "wheel-axis", "touch-cal", "ime-hook", "hotkey"];

/// 全目录（36 项定长——名称逐项有出处）。
pub const ITEM_CATALOG: [CatalogItem; CATALOG_N] = make_catalog();

const fn make_catalog() -> [CatalogItem; CATALOG_N] {
    let mut out = [CatalogItem { suite: CatSuite::Mem, seq: 0, name: "" }; CATALOG_N];
    let mut i = 0;
    let mut s = 0;
    while s < MEM_NAMES.len() {
        out[i] = CatalogItem { suite: CatSuite::Mem, seq: s, name: MEM_NAMES[s] };
        i += 1;
        s += 1;
    }
    s = 0;
    while s < PROC_NAMES.len() {
        out[i] = CatalogItem { suite: CatSuite::Proc, seq: s, name: PROC_NAMES[s] };
        i += 1;
        s += 1;
    }
    s = 0;
    while s < STORE_NAMES.len() {
        out[i] = CatalogItem { suite: CatSuite::Store, seq: s, name: STORE_NAMES[s] };
        i += 1;
        s += 1;
    }
    s = 0;
    while s < INPUT_NAMES.len() {
        out[i] = CatalogItem { suite: CatSuite::Input, seq: s, name: INPUT_NAMES[s] };
        i += 1;
        s += 1;
    }
    out
}

/// 目录-套件项数一致性：各套件目录项数 == 套件定义项数（逐项等值）。
pub fn catalog_consistent() -> bool {
    let count = |s: CatSuite| ITEM_CATALOG.iter().filter(|it| it.suite == s).count();
    count(CatSuite::Mem) == SUITE_MEM_ITEMS
        && count(CatSuite::Proc) == SUITE_PROC_ITEMS
        && count(CatSuite::Store) == SUITE_STORE_ITEMS
        && count(CatSuite::Input) == 6
}

/// 按名查项（诊断/重测的入口——名字是键）。
pub fn find_item(name: &str) -> Option<CatalogItem> {
    ITEM_CATALOG.iter().copied().find(|it| it.name == name)
}

// ---------------------------------------------------------------------------
// 失败分诊（三要素）
// ---------------------------------------------------------------------------

/// 分诊卡（哪条坏/可能原因/建议动作）。
pub fn diagnose(item: &CatalogItem) -> (&'static str, &'static str, &'static str) {
    match item.suite {
        CatSuite::Mem => (
            "内存自检项失败",
            "页表/堆/帧缓存初始化异常，或物理内存映射与预期不符",
            "进入安全模式查看内存诊断详情，必要时运行内存全检",
        ),
        CatSuite::Proc => (
            "处理器自检项失败",
            "调度器/中断/系统调用表初始化异常",
            "以最近一次成功配置启动，或运行引导修复",
        ),
        CatSuite::Store => (
            "存储自检项失败",
            "文件系统挂载/日志/缓存初始化异常，卷可能受损",
            "运行「检查此卷」修复，或从还原点回滚",
        ),
        CatSuite::Input => (
            "输入自检项失败",
            "键表/指针/触摸校准数据异常（不影响系统核心）",
            "进入系统后在输入设置中重新校准",
        ),
    }
}

/// 分诊分级：存储/内存失败为阻断级，输入失败为可带病运行级。
pub fn blocking(item: &CatalogItem) -> bool {
    !matches!(item.suite, CatSuite::Input)
}

// ---------------------------------------------------------------------------
// 重测流
// ---------------------------------------------------------------------------

/// 重测账：失败集合 + 重测通过后清除。
#[derive(Clone, Copy)]
pub struct RetestFlow {
    failed: [bool; CATALOG_N],
    pub failed_n: usize,
    pub retests: u32,
}

impl RetestFlow {
    pub const fn new() -> RetestFlow {
        RetestFlow { failed: [false; CATALOG_N], failed_n: 0, retests: 0 }
    }

    pub fn mark_failed(&mut self, idx: usize) -> bool {
        if idx >= CATALOG_N || self.failed[idx] {
            return false;
        }
        self.failed[idx] = true;
        self.failed_n += 1;
        true
    }

    pub fn is_failed(&self, idx: usize) -> bool {
        self.failed.get(idx).copied().unwrap_or(false)
    }

    /// 重测一项：通过 → 清失败账；仍败 → 账保留（重测有诚实的两种结局）。
    pub fn retest(&mut self, idx: usize, passed: bool) -> bool {
        if idx >= CATALOG_N || !self.failed[idx] {
            return false;
        }
        self.retests += 1;
        if passed {
            self.failed[idx] = false;
            self.failed_n -= 1;
        }
        true
    }

    /// 零失败判定（重测全部通过 → 收口）。
    pub fn all_cleared(&self) -> bool {
        self.failed_n == 0
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_selftestviz_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b5");

    // 1) 目录-套件一致性：四套件项数逐项等值（36 分母的目录版对拍）。
    cs.add("catalog_consistent", catalog_consistent() && ITEM_CATALOG.len() == CATALOG_N, "");

    // 2) 目录键唯一：套件内名唯一（kinfo 名册里 zombie-ok 跨套件同名为
    //     主册事实——键是 (套件, seq)，诊断引用带套件前缀不混淆）。
    let mut unique = true;
    for s in [CatSuite::Mem, CatSuite::Proc, CatSuite::Store, CatSuite::Input] {
        // 定长收集（零堆——每套件 ≤11 名）。
        let mut names = [""; 16];
        let mut n = 0;
        for it in ITEM_CATALOG.iter().filter(|it| it.suite == s) {
            names[n] = it.name;
            n += 1;
        }
        for i in 0..n {
            for j in i + 1..n {
                unique &= names[i] != names[j];
            }
        }
    }
    cs.add("catalog_names_unique", unique, "");

    // 3) 按名查项：命中返回套件+序号、未登记名 None（键查两面）。
    let hit = find_item("phys-map").unwrap();
    let miss = find_item("no-such-item");
    cs.add(
        "catalog_lookup",
        hit.suite == CatSuite::Mem && hit.seq == 0 && miss.is_none(),
        "",
    );

    // 4) 分诊三要素：四套件各有三段非空文案（三要素逐套件在岗）。
    let all = ITEM_CATALOG.iter().all(|it| {
        let (what, why, next) = diagnose(it);
        !what.is_empty() && !why.is_empty() && !next.is_empty()
    });
    cs.add("diagnose_three_parts", all, "");

    // 5) 分级：存储/内存阻断、输入可带病（分级不是一刀切）。
    let store = find_item("fat-mount").unwrap();
    let input = find_item("kbd-map").unwrap();
    cs.add("diagnose_blocking", blocking(&store) && !blocking(&input), "");

    // 6) 重测账：标记/去重/重测通过清账（精确到项的恢复路径）。
    let mut r = RetestFlow::new();
    r.mark_failed(3);
    let dup = r.mark_failed(3);
    r.mark_failed(5);
    let clear = r.retest(3, true);
    cs.add(
        "retest_clears",
        !dup && r.failed_n == 1 && clear && !r.is_failed(3) && r.retests == 1,
        "",
    );

    // 7) 重测仍败：账保留（重测的两种结局都诚实）。
    let mut r2 = RetestFlow::new();
    r2.mark_failed(7);
    r2.retest(7, false);
    cs.add("retest_failure_kept", r2.is_failed(7) && r2.failed_n == 1 && r2.retests == 1, "");

    // 8) 全清判定：零失败 → 收口（重测全部通过）。
    let mut r3 = RetestFlow::new();
    r3.mark_failed(1);
    r3.mark_failed(2);
    r3.retest(1, true);
    r3.retest(2, true);
    cs.add("retest_all_cleared", r3.all_cleared() && r3.failed_n == 0, "");

    // 9) 重测未失败项拒收：没失败的不存在重测（账面纪律）。
    let mut r4 = RetestFlow::new();
    cs.add("retest_only_failed", !r4.retest(0, true), "");

    // 10) 名册常量贯通：9/11/10 项数与主层一处一事实。
    cs.add(
        "roster_consts",
        MEM_NAMES.len() == SUITE_MEM_ITEMS && PROC_NAMES.len() == SUITE_PROC_ITEMS && STORE_NAMES.len() == SUITE_STORE_ITEMS,
        "",
    );

    // 11) 目录序号连续性：各套件内 seq 0..n 连续（无跳号）。
    let mut seq_ok = true;
    for s in [CatSuite::Mem, CatSuite::Proc, CatSuite::Store, CatSuite::Input] {
        let mut seen = [false; 16];
        for it in ITEM_CATALOG.iter().filter(|it| it.suite == s) {
            if it.seq < 16 {
                seen[it.seq] = true;
            }
        }
        seq_ok &= seen[..s.items()].iter().all(|x| *x);
    }
    cs.add("catalog_seq_continuous", seq_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn catalog_total_matches() {
        // 36 = 9+11+10+6（目录分母与 b4 TOTAL_ITEMS 同源对拍）。
        assert_eq!(ITEM_CATALOG.len(), 36);
        assert_eq!(ITEM_CATALOG[0].name, "phys-map");
        assert_eq!(ITEM_CATALOG[35].name, "hotkey");
    }

    #[test]
    fn retest_interleaved() {
        // 交错重测：多项失败各自独立清账（互不串扰）。
        let mut r = RetestFlow::new();
        for i in [0usize, 4, 10, 20, 35] {
            r.mark_failed(i);
        }
        assert_eq!(r.failed_n, 5);
        r.retest(10, true);
        r.retest(4, false);
        assert_eq!(r.failed_n, 4);
        assert!(!r.is_failed(10));
        assert!(r.is_failed(4));
    }

    #[test]
    fn diagnose_per_suite_distinct() {
        // 四套件分诊文案互不相同（一个模子套四话 = 没分诊）。
        let sets: [(&str, CatSuite); 4] = [
            ("phys-map", CatSuite::Mem),
            ("sched-ready", CatSuite::Proc),
            ("fat-mount", CatSuite::Store),
            ("kbd-map", CatSuite::Input),
        ];
        let msgs: Vec<&'static str> = sets.iter().map(|(n, _)| diagnose(&find_item(n).unwrap()).0).collect();
        assert_ne!(msgs[0], msgs[1]);
        assert_ne!(msgs[1], msgs[2]);
        assert_ne!(msgs[2], msgs[3]);
    }
}
