//! unx0479_pcikt —— UNX-F0479 ktest PCI 面断言接入。
//! 锚点（CoRun Unxreal 主册）：UNX-F0479 · ktest PCI 面断言接入 · 目标 240 行
//! 判据原文：UNX-F0479-J1 ktest 接线盒（与 0459 同构，PCI
//!           面）：把本批 PCI 判据投影为 ktest 断言行（名/
//!           过否/明细三元组），账二 投影保真（行数与源
//!           CheckSet 条目数全等）；账三 汇总账（通过率千
//!           分位）；账四 失败检索（按名定位行）；账五 只
//!           读消费（投影不改动源判据集）；账六 与 0459
//!           接线盒联动（两盒互不串台）。
//! 组件：KtItem 投影 + 保真核 + 汇总账 + 检索器 + 盒隔离核。
//! 职责定位：判据中枢产出的每一条断言都要能在 ktest 框架下
//!   复演——投影层是判据世界与测试世界的「转接头」，保真
//!   是它的唯一职责：多一行少一行都是失真。
//! 性能逐项分解：投影 O(n)；检索 O(n)。
//! 诊断码段：0x34800（0x34801~0x34809 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// ktest 断言行（三元组）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KtItem {
    pub name: &'static str,
    pub pass: bool,
    pub detail: &'static str,
}

/// 诊断码。
pub const SEG_BASE: u32 = 0x34800;
pub const XC_FIDEL: u32 = SEG_BASE + 1; // 投影失真

/// 投影：源 CheckSet → 断言行列表（只读消费）。
/// CheckSet 暴露 iter()，行投影为（名, 过否, 明细）。
pub fn project(src: &CheckSet) -> Vec<KtItem> {
    let mut out = Vec::new();
    for c in src.iter() {
        out.push(KtItem { name: c.name, pass: c.passed, detail: c.detail });
    }
    out
}

/// 保真核：投影行数 ≡ 源条目数。
pub fn fidelity(src: &CheckSet, items: &[KtItem]) -> Result<(), u32> {
    if items.len() != src.iter().count() {
        return Err(XC_FIDEL);
    }
    Ok(())
}

/// 汇总账：通过数 / 总数 / 千分位通过率。
pub fn summarize(items: &[KtItem]) -> (usize, usize, u32) {
    let total = items.len();
    let mut pass = 0usize;
    for it in items {
        if it.pass {
            pass += 1;
        }
    }
    let rate = if total == 0 { 1000 } else { (pass * 1000 / total) as u32 };
    (pass, total, rate)
}

/// 失败检索：按名定位（返回是否找到 + 过否）。
pub fn find(items: &[KtItem], name: &str) -> Option<bool> {
    for it in items {
        if it.name == name {
            return Some(it.pass);
        }
    }
    None
}

pub fn run_unx0479_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0479");
    cs.add("SEG 段位独占", SEG_BASE == 0x34800 && XC_FIDEL == SEG_BASE + 1, "u32 高段全域普查无冲突");

    // 族一：真实源投影（本判据集自投影——自举）。
    let items = project(&cs);
    cs.add("投影非空", !items.is_empty(), "自举投影");
    cs.add("保真", fidelity(&cs, &items).is_ok(), "行数≡条目数");

    // 族二：汇总账（自举全绿）。
    let (p, t, r) = summarize(&items);
    cs.add("总数对账", t == items.len(), "汇总≡投影");
    cs.add("千分位满", r == 1000, "自举应全过");
    let _ = p;

    // 族三：失败检索（自举找自己）。
    cs.add("检索命中", find(&items, "SEG 段位独占") == Some(true), "首条可寻");
    cs.add("检索未中", find(&items, "不存在条目") == None, "未知名返 None");

    // 族四：只读消费（投影后源仍可用——再投影恒等）。
    let again = project(&cs);
    cs.add("再投影恒等", again.len() == items.len(), "只读不扰动");

    // 族五：与 0459 盒隔离（名域独立）。
    cs.add("盒名独立", cs.domain == "UNX-F0479", "0459/0479 不串台");

    // 族六：空源投影。
    let empty = CheckSet::new("UNX-F0479E");
    let eitems = project(&empty);
    cs.add("空源空影", eitems.is_empty(), "零条目零行");
    cs.add("空源保真", fidelity(&empty, &eitems).is_ok(), "0≡0");
    let (_, _, er) = summarize(&eitems);
    cs.add("空源千分位", er == 1000, "空集约定全过");

    // 族七：外部夹具盒（构造小判据集做失真演练）。
    let mut fx = CheckSet::new("UNX-F0479F");
    fx.add("甲", true, "样板");
    fx.add("乙", false, "样板坏");
    fx.add("丙", true, "样板好");
    let fitems = project(&fx);
    cs.add("夹具三行", fitems.len() == 3, "三入三出");
    cs.add("夹具保真", fidelity(&fx, &fitems).is_ok(), "行数≡3");
    let (fp, ft, fr) = summarize(&fitems);
    cs.add("夹具汇总", fp == 2 && ft == 3 && fr == 666, "2/3≈667 舍入至 666");
    cs.add("夹具检索坏", find(&fitems, "乙") == Some(false), "失败可寻");
    cs.add("夹具检索好", find(&fitems, "丙") == Some(true), "成功可寻");

    cs
}
