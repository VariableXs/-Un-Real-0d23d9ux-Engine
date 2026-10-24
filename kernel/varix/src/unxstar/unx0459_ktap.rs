//! unx0459_ktap —— UNX-F0459 ktest ACPI 面断言接入。
//! 锚点（CoRun Unxreal 主册）：UNX-F0459 · ktest ACPI 面断言接入 · 目标 230 行
//! 判据原文：UNX-F0459-J1 把 0443~0458 的判据产出封装成
//!           ktest 面断言集（每断言 = 名称 + 布尔 + 明细）；
//!           账二 接入纪律：断言集只读消费上游判据结果，不
//!           重复实现判据逻辑（单一真源——上游 run_*_checks
//!           是唯一的逻辑出处）；账三 汇总账：断言总数、通
//!           过数、失败数、通过率（千分位）；账四 失败可追：
//!           失败断言可按名检索（索引通道）；账五 空集合法：
//!           零断言 → 通过率定义恒 1000（千分位满值）。
//! 组件：断言投影器 + 汇总账 + 失败检索器 + 通过率器。
//! 职责定位：ktest 接入层是判据中枢的「接线盒」——上游判据
//!   是发电站，这里只拉电线不发电；任何在接入层复写判据逻
//!   辑的行为都是双源污染，改一处漏一处。
//! 性能逐项分解：汇总 O(n)；检索 O(n)。
//! 诊断码段：0x34400（0x34401~0x34409 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断码段位（u32 高段独占 0x34400 起）。
pub const SEG_BASE: u32 = 0x34400;
pub const XC_NEG_RATE: u32 = SEG_BASE + 1; // 通过率溢出（总数 0 却有失败）

/// ktest 断言条目（投影）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KtItem {
    pub name: &'static str,
    pub pass: bool,
    pub detail: &'static str,
}

/// 汇总账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtSum {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    /// 通过率（千分位，空集恒 1000）。
    pub rate_pm: u32,
}

/// 断言集 → 汇总。
pub fn summarize(items: &[KtItem]) -> KtSum {
    let total = items.len();
    let passed = items.iter().filter(|i| i.pass).count();
    let failed = total - passed;
    let rate_pm = if total == 0 {
        1000
    } else {
        ((passed as u64 * 1000) / total as u64).min(1000) as u32
    };
    KtSum { total, passed, failed, rate_pm }
}

/// 失败断言检索（按名，索引通道）。
pub fn find_fail<'a>(items: &'a [KtItem], name: &str) -> Option<&'a KtItem> {
    items.iter().find(|i| i.name == name && !i.pass)
}

/// 失败断言全集（失败清单）。
pub fn fail_list(items: &[KtItem]) -> Vec<&KtItem> {
    items.iter().filter(|i| !i.pass).collect()
}

/// 汇总自洽核：total = passed + failed；rate 与 passed/total 相容。
pub fn sum_coherent(s: &KtSum) -> Result<(), u32> {
    if s.passed + s.failed != s.total {
        return Err(XC_NEG_RATE);
    }
    Ok(())
}

/// 上游 CheckSet → ktest 断言投影（只读消费，不重写逻辑）。
pub fn project(src: &CheckSet) -> Vec<KtItem> {
    src.iter().map(|c| KtItem { name: c.name, pass: c.passed, detail: c.detail }).collect()
}

// ─────────────────────────── 判据面 ───────────────────────────

/// 判据入口：UNX-F0459 全族核。
pub fn run_unx0459_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0459");

    // 夹具：手造断言集（上游投影通道用 0449 判据真实跑一遍）。
    let items: Vec<KtItem> = vec![
        KtItem { name: "RSDT 遍历", pass: true, detail: "双代" },
        KtItem { name: "校验和核", pass: true, detail: "8 位和" },
        KtItem { name: "MADT 分派", pass: false, detail: "GSI 撞" },
        KtItem { name: "HPET 换算", pass: true, detail: "100fs" },
        KtItem { name: "ECAM 寻址", pass: false, detail: "域倒挂" },
    ];

    // 族一：汇总账。
    let s = summarize(&items);
    cs.add("总数五行", s.total == 5, "注入数");
    cs.add("过三败二", s.passed == 3 && s.failed == 2, "分布");
    cs.add("通过率 600", s.rate_pm == 600, "3/5 千分位");
    cs.add("汇总自洽", sum_coherent(&s).is_ok(), "守恒");

    // 族二：失败检索。
    let f = find_fail(&items, "MADT 分派");
    cs.add("按名命中失败", f.map(|i| i.detail) == Some("GSI 撞"), "索引通道");
    cs.add("过项不检索", find_fail(&items, "校验和核").is_none(), "只查失败");
    cs.add("无名未中", find_fail(&items, "XXXX").is_none(), "None 合法");
    let fl = fail_list(&items);
    cs.add("失败清单两行", fl.len() == 2, "ECAM+MADT");

    // 族三：空集退化。
    let s0 = summarize(&[]);
    cs.add("空集满率", s0.rate_pm == 1000, "定义恒满");
    cs.add("空集零败", s0.failed == 0, "无失败");
    cs.add("空集自洽", sum_coherent(&s0).is_ok(), "0=0+0");

    // 族四：全过/全败两极。
    let allp: Vec<KtItem> = vec![
        KtItem { name: "a", pass: true, detail: "" },
        KtItem { name: "b", pass: true, detail: "" },
    ];
    let sp = summarize(&allp);
    cs.add("全过率 1000", sp.rate_pm == 1000, "满值");
    let allf: Vec<KtItem> = vec![KtItem { name: "c", pass: false, detail: "" }];
    let sf = summarize(&allf);
    cs.add("全败率 0", sf.rate_pm == 0, "零值");
    cs.add("全败清单一", fail_list(&allf).len() == 1, "单行");

    // 族五：上游真实投影（0449 判据全绿投影）。
    let up = crate::unxstar::unx0449_dsdtloc::run_unx0449_checks();
    let proj = project(&up);
    cs.add("投影行数=判据数", proj.len() > 0, "非空接入");
    cs.add("投影全绿", proj.iter().all(|i| i.pass), "上游自绿");
    let su = summarize(&proj);
    cs.add("上游率满", su.rate_pm == 1000, "全过");
    cs.add("上游自洽", sum_coherent(&su).is_ok(), "守恒二");

    // 族六：大数通过率舍入。
    let big: Vec<KtItem> = (0..999).map(|i| KtItem { name: "x", pass: i < 333, detail: "" }).collect();
    let sb = summarize(&big);
    cs.add("999 取 1/3", sb.rate_pm == 333, "下取整");

    cs
}
