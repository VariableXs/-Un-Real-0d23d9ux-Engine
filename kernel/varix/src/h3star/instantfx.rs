//! F303 即时生效哲学 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：即时生效项比例（≥95%）；五类例外白名单审计；
//! 「重启后生效」徽标五处齐；就地反馈延迟 <100ms。
//!
//! **设计要点（主册）**：
//! - 设置改动即时生效并就地反馈（开关拨动即变、滑杆拖动实时预览）——
//!   「确定/应用/取消」三按钮制在设置中心废除（对话框类操作除外）；
//! - 仅五类例外允许延迟生效且明标「重启后生效」徽标（分辨率/缩放/
//!   默认主题/引导项/语言）；改坏了随时可退；
//! - 无感标准：改设置像调音量——拨了就变，从不出现「点了确定但什么
//!   都没发生」或「忘了点应用白改」；例外项提前告知不惊吓。
//!
//! 实现形态：比例审计器 + 徽标账（五类各一徽标、缺一即红）+ 即时改值
//! 通路（直通 [`super::hbase::SettingRegistry::set_value`] 唯一改值口）。

use crate::checks::CheckSet;

use super::hbase::{ControlKind, DeferredKind, EffectKind, SettingItem, SettingRegistry, INSTANT_FEEDBACK_MS};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 即时生效项比例判线（‰——950 = 95%）。
pub const INSTANT_RATIO_MIN_PERMILLE: u32 = 950;

/// 「重启后生效」徽标统一文案（一处一事实——五处共用）。
pub const DEFERRED_BADGE: &str = "重启后生效";

// ---------------------------------------------------------------------------
// 徽标账（五类例外 → 徽标登记）
// ---------------------------------------------------------------------------

/// 徽标账：五类例外各登记一处徽标（缺一 = 审计红）。
#[derive(Clone, Debug, Default)]
pub struct BadgeLedger {
    registered: Vec<(DeferredKind, &'static str)>,
}

impl BadgeLedger {
    pub fn new() -> BadgeLedger {
        BadgeLedger { registered: Vec::new() }
    }

    /// 登记某类例外的徽标（重复登记拒绝——一处一事实）。
    pub fn register(&mut self, kind: DeferredKind) -> bool {
        if self.registered.iter().any(|(k, _)| *k == kind) {
            return false;
        }
        self.registered.push((kind, DEFERRED_BADGE));
        true
    }

    /// 五处齐（判据：徽标五处齐）。
    pub fn all_five(&self) -> bool {
        DeferredKind::ALL.iter().all(|k| self.registered.iter().any(|(rk, _)| rk == k))
    }

    /// 查某类的徽标文案。
    pub fn badge_of(&self, kind: DeferredKind) -> Option<&'static str> {
        self.registered.iter().find(|(k, _)| *k == kind).map(|(_, t)| *t)
    }

    pub fn len(&self) -> usize {
        self.registered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 审计器
// ---------------------------------------------------------------------------

/// 即时生效审计结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstantAudit {
    /// 即时项比例（‰）。
    pub instant_ratio_permille: u32,
    /// 白名单外延迟项（枚举化后恒 0——防御性保留账面）。
    pub off_whitelist_deferred: usize,
    /// 徽标五处齐。
    pub badges_complete: bool,
}

/// 全表审计（比例 + 白名单 + 徽标三面合账）。
pub fn audit(reg: &SettingRegistry, badges: &BadgeLedger) -> InstantAudit {
    let total = reg.items().len();
    let instant = reg
        .items()
        .iter()
        .filter(|i| i.effect == EffectKind::Instant)
        .count();
    let ratio = if total == 0 { 1000 } else { (instant * 1000 / total) as u32 };
    // 白名单审计：枚举化的 DeferredKind 只有五类值（构造面即白名单），
    // 此处对延迟项做「类别可解释」复核（label 非空）。
    let off_whitelist_deferred = reg
        .items()
        .iter()
        .filter(|i| match i.effect {
            EffectKind::Deferred(k) => k.label().is_empty(),
            EffectKind::Instant => false,
        })
        .count();
    InstantAudit {
        instant_ratio_permille: ratio,
        off_whitelist_deferred,
        badges_complete: badges.all_five(),
    }
}

/// 审计绿（三项判据合账）。
pub fn audit_green(a: &InstantAudit) -> bool {
    a.instant_ratio_permille >= INSTANT_RATIO_MIN_PERMILLE
        && a.off_whitelist_deferred == 0
        && a.badges_complete
}

/// 就地反馈预算（即时项）：<100ms 判线直通底盘常量。
pub const fn instant_feedback_budget() -> u64 {
    INSTANT_FEEDBACK_MS
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F303 自检（判据：≥95%；白名单五类；徽标五处；<100ms）。
pub fn run_instantfx_checks() -> CheckSet {
    let mut set = CheckSet::new("F303-instantfx");

    let mut reg = SettingRegistry::new();
    let mut badges = BadgeLedger::new();
    let mut all_registered = true;
    for k in DeferredKind::ALL {
        all_registered = all_registered && badges.register(k);
    }

    // 1. 徽标五处齐（判据载体）。
    set.add("badges all five", all_registered && badges.all_five() && badges.len() == 5, "");

    // 2. 重复登记拒绝（一处一事实）。
    set.add("badge duplicate rejected", !badges.register(DeferredKind::Scaling), "");

    // 3. 比例达标基线：95 即时 + 5 延迟（每类一）= 95%。
    let mk = |name: &'static str, effect: EffectKind| SettingItem {
        name,
        page: "系统/显示",
        synonyms: &["s1", "s2", "s3"],
        effect,
        default: 0,
        value: 0,
        control: ControlKind::Toggle,
    };
    for i in 0..95u8 {
        let name: &'static str = match i {
            0 => "即零", 1 => "即一", 2 => "即二", 3 => "即三", 4 => "即四",
            5 => "即五", 6 => "即六", 7 => "即七", 8 => "即八", 9 => "即九",
            10 => "即十", 11 => "即十一", 12 => "即十二", 13 => "即十三", 14 => "即十四",
            15 => "即十五", 16 => "即十六", 17 => "即十七", 18 => "即十八", 19 => "即十九",
            20 => "即二十", 21 => "即二一", 22 => "即二二", 23 => "即二三", 24 => "即二四",
            25 => "即二五", 26 => "即二六", 27 => "即二七", 28 => "即二八", 29 => "即二九",
            30 => "即三十", 31 => "即三一", 32 => "即三二", 33 => "即三三", 34 => "即三四",
            35 => "即三五", 36 => "即三六", 37 => "即三七", 38 => "即三八", 39 => "即三九",
            40 => "即四十", 41 => "即四一", 42 => "即四二", 43 => "即四三", 44 => "即四四",
            45 => "即四五", 46 => "即四六", 47 => "即四七", 48 => "即四八", 49 => "即四九",
            50 => "即五十", 51 => "即五一", 52 => "即五二", 53 => "即五三", 54 => "即五四",
            55 => "即五五", 56 => "即五六", 57 => "即五七", 58 => "即五八", 59 => "即五九",
            60 => "即六十", 61 => "即六一", 62 => "即六二", 63 => "即六三", 64 => "即六四",
            65 => "即六五", 66 => "即六六", 67 => "即六七", 68 => "即六八", 69 => "即六九",
            70 => "即七十", 71 => "即七一", 72 => "即七二", 73 => "即七三", 74 => "即七四",
            75 => "即七五", 76 => "即七六", 77 => "即七七", 78 => "即七八", 79 => "即七九",
            80 => "即八十", 81 => "即八一", 82 => "即八二", 83 => "即八三", 84 => "即八四",
            85 => "即八五", 86 => "即八六", 87 => "即八七", 88 => "即八八", 89 => "即八九",
            90 => "即九十", 91 => "即九一", 92 => "即九二", 93 => "即九三", _ => "即九四",
        };
        reg.add_item(mk(name, EffectKind::Instant));
    }
    let deferred_names = ["分辨率项", "缩放项", "主题项", "引导项", "语言项"];
    for (i, k) in DeferredKind::ALL.iter().enumerate() {
        reg.add_item(mk(deferred_names[i], EffectKind::Deferred(*k)));
    }
    let a = audit(&reg, &badges);
    set.add(
        "ratio 95 percent green",
        a.instant_ratio_permille == INSTANT_RATIO_MIN_PERMILLE
            && a.off_whitelist_deferred == 0
            && audit_green(&a),
        "",
    );

    // 4. 比例破线审计红（96 项里 10 项延迟 = 89.6% < 95%）。
    let mut bad_reg = reg.clone();
    // 把五个延迟项换成第 6 个（同类别重复——不新增类别仍 5 处徽标）。
    for name in deferred_names {
        let _ = bad_reg.item_mut(name).map(|it| it.effect = EffectKind::Instant);
    }
    for name in ["即零", "即一", "即二", "即三", "即四", "即五", "即六", "即七", "即八", "即九"] {
        let _ = bad_reg.item_mut(name).map(|it| it.effect = EffectKind::Deferred(DeferredKind::Scaling));
    }
    let a = audit(&bad_reg, &badges);
    set.add(
        "ratio below line red",
        a.instant_ratio_permille < INSTANT_RATIO_MIN_PERMILLE && !audit_green(&a),
        "",
    );

    // 5. 徽标缺失审计红（撤一处徽标）。
    let mut thin = BadgeLedger::new();
    for k in DeferredKind::ALL.iter().skip(1) {
        thin.register(*k);
    }
    let a = audit(&reg, &thin);
    set.add("badge missing red", !audit_green(&a) && !a.badges_complete, "");

    // 6. 徽标文案统一（五处共用「重启后生效」——一处一事实）。
    set.add(
        "badge text uniform",
        DeferredKind::ALL.iter().all(|k| badges.badge_of(*k) == Some(DEFERRED_BADGE)),
        "",
    );

    // 7. 即时改值通路：返回反馈预算 <100ms；延迟项返回 None（徽标面）。
    set.add(
        "instant path budget",
        instant_feedback_budget() == 100
            && reg.set_value("即零", 1) == Some(100)
            && reg.set_value("分辨率项", 1).is_none(),
        "",
    );

    // 8. 白名单类别可解释（五类 label 非空——防御面）。
    set.add(
        "whitelist labels explainable",
        DeferredKind::ALL.iter().all(|k| !k.label().is_empty()),
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
    fn empty_registry_ratio_is_green() {
        let a = audit(&SettingRegistry::new(), &BadgeLedger::new());
        assert_eq!(a.instant_ratio_permille, 1000);
        assert!(!audit_green(&a), "徽标未登记时不绿");
    }

    #[test]
    fn badge_text_is_uniform_constant() {
        let mut b = BadgeLedger::new();
        assert!(b.register(DeferredKind::Language));
        assert_eq!(b.badge_of(DeferredKind::Language), Some("重启后生效"));
    }

    #[test]
    fn ratio_rounds_down() {
        // 9 即时 + 1 延迟 = 90%。
        let mut reg = SettingRegistry::new();
        for n in ["a", "b", "c", "d", "e", "f", "g", "h", "i"] {
            reg.add_item(SettingItem {
                name: n,
                page: "x/y",
                synonyms: &["1", "2", "3"],
                effect: EffectKind::Instant,
                default: 0,
                value: 0,
                control: ControlKind::Toggle,
            });
        }
        reg.add_item(SettingItem {
            name: "z",
            page: "x/y",
            synonyms: &["1", "2", "3"],
            effect: EffectKind::Deferred(DeferredKind::Resolution),
            default: 0,
            value: 0,
            control: ControlKind::Toggle,
        });
        assert_eq!(audit(&reg, &BadgeLedger::new()).instant_ratio_permille, 900);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 延迟项徽标覆盖审计 + pending 应用队列（重启前可反悔）
// ---------------------------------------------------------------------------

/// 延迟项徽标覆盖审计（「重启后生效」徽标五处齐判据的机器面）：每个
/// 延迟生效条目的例外类别必须在徽标账有登记——缺口清单直出（哪一项
/// 没挂徽标），不许静默漏挂。
pub struct BadgeCoverage;

impl BadgeCoverage {
    /// 条目例外类别（延迟条目 → DeferredKind）。
    fn kind_of(item: &SettingItem) -> Option<DeferredKind> {
        match item.effect {
            EffectKind::Deferred(k) => Some(k),
            EffectKind::Instant => None,
        }
    }

    /// 审计：返回缺徽标的延迟条目名清单（空 = 五处齐）。
    pub fn missing_badges(reg: &SettingRegistry, badges: &BadgeLedger) -> Vec<&'static str> {
        reg.items()
            .iter()
            .filter_map(|i| Self::kind_of(i).map(|k| (i.name, k)))
            .filter(|(_, k)| badges.badge_of(*k).is_none())
            .map(|(name, _)| name)
            .collect()
    }

    /// 覆盖率‰（延迟条目口径——分母只数延迟项）。
    pub fn coverage_permille(reg: &SettingRegistry, badges: &BadgeLedger) -> u32 {
        let deferred: Vec<Option<DeferredKind>> =
            reg.items().iter().map(|i| Self::kind_of(i)).collect();
        let kinds: Vec<DeferredKind> = deferred.into_iter().flatten().collect();
        if kinds.is_empty() {
            return 1000;
        }
        let covered = kinds.iter().filter(|k| badges.badge_of(**k).is_some()).count();
        (covered * 1000 / kinds.len()) as u32
    }
}

/// pending 应用队列（延迟生效项的改动先进 pending——重启时统一落
/// 地；用户可「立即应用」（触发重启流程）或「放弃」（零残留回滚——
/// 改了又后悔不留半截状态））。
#[derive(Default)]
pub struct ApplyPendingQueue {
    /// (条目名, 期望值)。
    pub pending: Vec<(&'static str, i64)>,
    /// 反悔留痕（放弃的条目——异常显性化）。
    pub discarded: Vec<&'static str>,
    /// 落地留痕（随重启应用的条目）。
    pub applied: Vec<&'static str>,
}

impl ApplyPendingQueue {
    /// 延迟条目改值 → 入 pending（唯一入口——即时条目不进这里）。
    pub fn enqueue(&mut self, name: &'static str, value: i64) -> bool {
        if self.pending.iter().any(|(n, _)| *n == name) {
            match self.pending.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = value,
                None => return false,
            }
        } else {
            self.pending.push((name, value));
        }
        true
    }

    /// 放弃：清 pending + 留痕（条目值回滚由登记表 restore 承担）。
    pub fn discard_all(&mut self) -> usize {
        let n = self.pending.len();
        self.discarded.extend(self.pending.drain(..).map(|(name, _)| name));
        n
    }

    /// 随重启落地：pending → applied（重启流程的模拟面）。
    pub fn apply_on_reboot(&mut self) -> usize {
        let n = self.pending.len();
        self.applied.extend(self.pending.drain(..).map(|(name, _)| name));
        n
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// 深化层二自检（徽标覆盖 / pending 队列）。
pub fn run_instantfx_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F303-deep2");

    // 布景：一即时 + 两延迟（一挂徽标一漏挂）。
    let mut reg = SettingRegistry::new();
    let _ = reg.add_item(SettingItem {
        name: "音量",
        page: "系统/声音",
        synonyms: &["声音大小", "volume", "响度"],
        effect: EffectKind::Instant,
        default: 50,
        value: 50,
        control: ControlKind::Slider,
    });
    let _ = reg.add_item(SettingItem {
        name: "缩放",
        page: "系统/显示",
        synonyms: &["显示缩放", "dpi", "缩放比"],
        effect: EffectKind::Deferred(DeferredKind::Scaling),
        default: 100,
        value: 100,
        control: ControlKind::Dropdown,
    });
    let _ = reg.add_item(SettingItem {
        name: "分辨率",
        page: "系统/显示",
        synonyms: &["屏幕分辨率", "resolution", "清晰度"],
        effect: EffectKind::Deferred(DeferredKind::Resolution),
        default: 0,
        value: 0,
        control: ControlKind::Dropdown,
    });
    let mut badges = BadgeLedger::new();
    let _ = badges.register(DeferredKind::Scaling);

    // 1. 缺口直出：分辨率漏挂徽标被点名。
    let missing = BadgeCoverage::missing_badges(&reg, &badges);
    set.add("badge gap surfaced", missing == alloc::vec!["分辨率"], "");

    // 2. 覆盖率 500‰（两延迟项挂一）→ 补挂后 1000‰。
    set.add("coverage permille", BadgeCoverage::coverage_permille(&reg, &badges) == 500, "");
    let _ = badges.register(DeferredKind::Resolution);
    set.add(
        "coverage full after fix",
        BadgeCoverage::coverage_permille(&reg, &badges) == 1000
            && BadgeCoverage::missing_badges(&reg, &badges).is_empty(),
        "",
    );

    // 3. pending 队列：改延迟项入队（同条目覆盖不重复）→ 随重启落地。
    let mut q = ApplyPendingQueue::default();
    let _ = q.enqueue("缩放", 150);
    let _ = q.enqueue("缩放", 200);
    set.add(
        "pending enqueue overwrite",
        q.pending_len() == 1 && q.pending[0].1 == 200,
        "",
    );
    set.add("apply on reboot", q.apply_on_reboot() == 1 && q.pending_len() == 0, "");

    // 4. 反悔路径：放弃零残留 + 留痕。
    let mut q2 = ApplyPendingQueue::default();
    let _ = q2.enqueue("分辨率", 1);
    let n = q2.discard_all();
    set.add(
        "discard leaves no residue",
        n == 1 && q2.pending_len() == 0 && q2.discarded == alloc::vec!["分辨率"],
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn coverage_no_deferred_is_trivially_full() {
        let reg = SettingRegistry::new();
        let badges = BadgeLedger::new();
        assert_eq!(BadgeCoverage::coverage_permille(&reg, &badges), 1000);
    }

    #[test]
    fn discard_then_reenqueue_works() {
        let mut q = ApplyPendingQueue::default();
        let _ = q.enqueue("语言", 2);
        let _ = q.discard_all();
        assert!(q.enqueue("语言", 3), "放弃后可重新入队");
        assert_eq!(q.pending[0].1, 3);
    }
}
