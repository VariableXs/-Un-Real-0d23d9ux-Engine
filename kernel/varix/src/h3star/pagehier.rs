//! F302 设置页层级规范 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：层级深度审计（三级=缺陷）；每页 ≤15 条扫描；副标题
//! 覆盖率 100%；交叉链接有效性（死链=0 走查）。
//!
//! **设计要点（主册）**：
//! - 设置中心两级封顶：主分类页（一屏九宫格）+设置页（一页一主题、
//!   条目不超过 15 条）——需要更深的内容改用「高级」折叠区收在本页
//!   底部，不开第三层；
//! - 每页顶部一句人话副标题说明本页管什么（「这里调屏幕怎么显示」）；
//! - 相关设置交叉引用用链接跳转不复制条目；
//! - 无感标准：任何设置三次点击内必达，层级从不迷宫化。
//!
//! 实现形态（规范/走查/登记类）：登记门（违规页/条目拒绝入表）+ 走查
//! 审计器（产出逐页违规清单——「哪里违、违了什么、怎么改」）。登记表
//! 数据面在 [`super::hbase::SettingRegistry`]（一处一事实，本模块不复制
//! 数据结构）。

use crate::checks::CheckSet;

use super::hbase::SettingRegistry;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 单页条目上限（第 16 条起拒绝登记——「高级」折叠区归页内组件，不占新页）。
pub const MAX_ITEMS_PER_PAGE: usize = 15;

/// 合法层级深度（两级 = 主分类/页）。
pub const LEGAL_DEPTH: usize = 2;

// ---------------------------------------------------------------------------
// 违规清单
// ---------------------------------------------------------------------------

/// 一条违规（人话三要素：哪里/什么/怎么改）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub path: String,
    pub kind: ViolationKind,
    pub remedy: &'static str,
}

/// 违规类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// 三级及更深路径。
    TooDeep,
    /// 条目数超限。
    Overfull,
    /// 副标题缺失。
    NoSubtitle,
    /// 死链。
    DeadLink,
}

impl ViolationKind {
    pub fn label(self) -> &'static str {
        match self {
            ViolationKind::TooDeep => "层级超两级",
            ViolationKind::Overfull => "页内条目超限",
            ViolationKind::NoSubtitle => "副标题缺失",
            ViolationKind::DeadLink => "交叉链接死链",
        }
    }
}

/// 登记门：页路径两级 + 副标题非空 + 条目数不超限，否则拒绝。
pub fn gate_add_page(reg: &mut SettingRegistry, path: &'static str, subtitle: &'static str) -> bool {
    if path.matches('/').count() + 1 != LEGAL_DEPTH || subtitle.trim().is_empty() {
        return false;
    }
    reg.add_page(super::hbase::SettingPage { path, subtitle, cross_links: &[] })
}

/// 登记门（条目）：所属页必须已登记且未超限。
pub fn gate_add_item(
    reg: &mut SettingRegistry,
    item: super::hbase::SettingItem,
) -> bool {
    if item.page.matches('/').count() + 1 != LEGAL_DEPTH {
        return false;
    }
    let page_known = reg.pages().iter().any(|p| p.path == item.page);
    if !page_known {
        return false;
    }
    let n = reg.items().iter().filter(|i| i.page == item.page).count();
    if n >= MAX_ITEMS_PER_PAGE {
        return false;
    }
    reg.add_item(item)
}

/// 全表走查审计：产出逐条违规清单（空清单 = 走查绿）。
pub fn walkthrough(reg: &SettingRegistry) -> Vec<Violation> {
    let mut out: Vec<Violation> = Vec::new();
    let page_paths: Vec<&str> = reg.pages().iter().map(|p| p.path).collect();
    for p in reg.pages() {
        if p.path.matches('/').count() + 1 != LEGAL_DEPTH {
            out.push(Violation {
                path: String::from(p.path),
                kind: ViolationKind::TooDeep,
                remedy: "改用页内「高级」折叠区，不开第三层",
            });
        }
        if p.subtitle.trim().is_empty() {
            out.push(Violation {
                path: String::from(p.path),
                kind: ViolationKind::NoSubtitle,
                remedy: "补一句人话副标题（本页管什么）",
            });
        }
        for t in p.cross_links {
            if !page_paths.contains(t) {
                out.push(Violation {
                    path: String::from(p.path),
                    kind: ViolationKind::DeadLink,
                    remedy: "改链到已登记页或删链（不复制条目）",
                });
            }
        }
        let n = reg.items().iter().filter(|i| i.page == p.path).count();
        if n > MAX_ITEMS_PER_PAGE {
            out.push(Violation {
                path: String::from(p.path),
                kind: ViolationKind::Overfull,
                remedy: "低频条目移入「高级」折叠区或拆主题页",
            });
        }
    }
    for i in reg.items() {
        if i.page.matches('/').count() + 1 != LEGAL_DEPTH {
            out.push(Violation {
                path: String::from(i.page),
                kind: ViolationKind::TooDeep,
                remedy: "条目归位到两级页",
            });
        }
    }
    out
}

/// 走查绿（零违规）。
pub fn walkthrough_green(reg: &SettingRegistry) -> bool {
    walkthrough(reg).is_empty()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F302 自检（判据：三级=缺陷；≤15 条；副标题 100%；死链=0）。
pub fn run_pagehier_checks() -> CheckSet {
    let mut set = CheckSet::new("F302-pagehier");

    // 1. 登记门：三级路径拒绝。
    let mut reg = SettingRegistry::new();
    set.add(
        "gate rejects third level page",
        !gate_add_page(&mut reg, "系统/显示/缩放", "副标题"),
        "",
    );

    // 2. 登记门：副标题缺失拒绝（覆盖率 100% 的源头保证）。
    set.add(
        "gate rejects empty subtitle",
        !gate_add_page(&mut reg, "系统/显示", "  "),
        "",
    );

    // 3. 登记门：合法页通过；条目页未登记拒绝；超限第 16 条拒绝。
    set.add("gate accepts legal page", gate_add_page(&mut reg, "系统/显示", "这里调屏幕怎么显示"), "");
    let mk = |name: &'static str, page: &'static str| super::hbase::SettingItem {
        name,
        page,
        synonyms: &["a", "b", "c"],
        effect: super::hbase::EffectKind::Instant,
        default: 0,
        value: 0,
        control: super::hbase::ControlKind::Toggle,
    };
    set.add("gate rejects item on unknown page", !gate_add_item(&mut reg, mk("孤儿条目", "孤/页")), "");
    let mut full = true;
    for i in 0..MAX_ITEMS_PER_PAGE {
        // 用编号名造 15 条不同名条目（静态名面走 match 拼装）。
        let name: &'static str = match i {
            0 => "条目零",
            1 => "条目一",
            2 => "条目二",
            3 => "条目三",
            4 => "条目四",
            5 => "条目五",
            6 => "条目六",
            7 => "条目七",
            8 => "条目八",
            9 => "条目九",
            10 => "条目十",
            11 => "条目十一",
            12 => "条目十二",
            13 => "条目十三",
            14 => "条目十四",
            _ => "条目溢出",
        };
        full = full && gate_add_item(&mut reg, mk(name, "系统/显示"));
    }
    set.add("gate fills to cap", full && reg.items().len() == MAX_ITEMS_PER_PAGE, "");
    set.add("gate rejects overfull", !gate_add_item(&mut reg, mk("第十六条", "系统/显示")), "");

    // 4. 走查绿基线：门内登记的表零违规。
    set.add("walkthrough green on gated registry", walkthrough_green(&reg), "");

    // 5. 走查抓缺陷：死链 + 三级 + 副标题缺失逐一报出且含修法。
    let mut bad = SettingRegistry::new();
    bad.add_page(super::hbase::SettingPage { path: "个性化/主题", subtitle: "这里调主题", cross_links: &["不存在页"] });
    bad.add_page(super::hbase::SettingPage { path: "系统/显示/高级", subtitle: "x", cross_links: &[] });
    bad.add_page(super::hbase::SettingPage { path: "系统/声音", subtitle: "", cross_links: &[] });
    let v = walkthrough(&bad);
    set.add(
        "walkthrough catches violations",
        v.len() == 3
            && v.iter().any(|x| x.kind == ViolationKind::DeadLink)
            && v.iter().any(|x| x.kind == ViolationKind::TooDeep)
            && v.iter().any(|x| x.kind == ViolationKind::NoSubtitle)
            && v.iter().all(|x| !x.remedy.is_empty()),
        "",
    );

    // 6. 违规类别文案完备（四类全可解释——人话映射表）。
    set.add(
        "violation labels complete",
        [ViolationKind::TooDeep, ViolationKind::Overfull, ViolationKind::NoSubtitle, ViolationKind::DeadLink]
            .iter()
            .all(|k| !k.label().is_empty()),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_counting() {
        assert_eq!("a/b".matches('/').count() + 1, LEGAL_DEPTH);
        assert_eq!("a/b/c".matches('/').count() + 1, 3);
    }

    #[test]
    fn overfull_is_reported_not_panicked() {
        // 直灌超限表（绕过门）走查仍稳——审计不 panic 不误判。
        let mut reg = SettingRegistry::new();
        reg.add_page(crate::h3star::hbase::SettingPage { path: "a/b", subtitle: "s", cross_links: &[] });
        for i in 0..20 {
            let name: &'static str = match i {
                0 => "n0", 1 => "n1", 2 => "n2", 3 => "n3", 4 => "n4",
                5 => "n5", 6 => "n6", 7 => "n7", 8 => "n8", 9 => "n9",
                10 => "n10", 11 => "n11", 12 => "n12", 13 => "n13", 14 => "n14",
                15 => "n15", 16 => "n16", 17 => "n17", 18 => "n18", _ => "n19",
            };
            reg.add_item(crate::h3star::hbase::SettingItem {
                name,
                page: "a/b",
                synonyms: &["x", "y", "z"],
                effect: crate::h3star::hbase::EffectKind::Instant,
                default: 0,
                value: 0,
                control: crate::h3star::hbase::ControlKind::Toggle,
            });
        }
        let v = walkthrough(&reg);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].kind, ViolationKind::Overfull);
    }
}
