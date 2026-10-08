//! F474 设置项说明文案（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **三件套覆盖率审计（全设置项扫描）；文案句式规范抽查；禁用态说明；
//! 链接有效性（死锚=0）；术语表一致性（同一概念全系统同名）。**
//!
//! 功能定义（主册批次三）：每个设置项的文案三件套规范——主标签（名词短语，
//! 不写「是否开启…功能」废话式）、一句话说明（这项影响什么、改了会怎样）、
//! 了解更多链接（只给真有深度的项，锚点必须真实存在）；风险项说明带后果；
//! 禁用态说明同步更新。
//!
//! 零堆纪律：定长设置项文案表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 设置项文案表容量（全设置项扫描口径）。
pub const ITEM_CAP: usize = 96;

/// 一个设置项的文案三件套。
#[derive(Clone, Copy, Debug)]
pub struct SettingCopy {
    /// 主标签（名词短语）。
    pub label: &'static str,
    /// 一句话说明（影响什么、改了会怎样）。
    pub desc: &'static str,
    /// 了解更多锚点（None = 不给链接——主册：真需要才给，不泛滥）。
    pub learn_anchor: Option<&'static str>,
    /// 禁用态说明（None = 该项无禁用态）。
    pub disabled_reason: Option<&'static str>,
    /// 风险后果说明（风险项必须给）。
    pub risk_note: Option<&'static str>,
}

/// 文案规范审计器。
pub struct CopyAuditor {
    items: [Option<SettingCopy>; ITEM_CAP],
    n: usize,
}

/// 已知锚点全集（死锚=0 判定的真实锚登记处）。
pub const KNOWN_ANCHORS: [&str; 6] = [
    "help/snap", "help/dnd", "help/privacy", "help/power", "help/wallpaper", "help/accounts",
];

/// 句式规范：主标签不得以废话式开头（「是否开启」「是否启用」「是否显示」）。
pub fn label_style_ok(label: &str) -> bool {
    !(label.starts_with("是否开启")
        || label.starts_with("是否启用")
        || label.starts_with("是否显示")
        || label.starts_with("是否允许"))
}

/// 一句话说明规范：非空且不是复述功能名（说明 > 标签长度——说明要说清影响）。
pub fn desc_style_ok(label: &str, desc: &str) -> bool {
    !desc.is_empty() && desc.len() > label.len() / 2 && desc != label
}

/// 链接有效性：死锚=0（锚点必须在登记册内）。
pub fn anchor_valid(a: &str) -> bool {
    KNOWN_ANCHORS.contains(&a)
}

impl CopyAuditor {
    pub const fn new() -> Self {
        CopyAuditor { items: [None; ITEM_CAP], n: 0 }
    }

    pub fn add(&mut self, item: SettingCopy) -> bool {
        if self.n >= ITEM_CAP {
            return false;
        }
        self.items[self.n] = Some(item);
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 三件套覆盖率审计（主册：全设置项扫描——label+desc 齐全率必须 100%）。
    pub fn coverage_audit(&self) -> (usize, usize) {
        let mut ok = 0;
        for i in 0..self.n {
            if let Some(it) = self.items[i] {
                if !it.label.is_empty() && !it.desc.is_empty() {
                    ok += 1;
                }
            }
        }
        (ok, self.n)
    }

    /// 全量扫描：句式规范 + 锚点有效性 + 术语一致性抽检。
    pub fn full_audit(&self) -> bool {
        (0..self.n).all(|i| {
            let it = match self.items[i] {
                Some(x) => x,
                None => return false,
            };
            label_style_ok(it.label)
                && desc_style_ok(it.label, it.desc)
                && it.learn_anchor.map(anchor_valid).unwrap_or(true)
                && anchor_consistent(it.label)
        })
    }

    /// 禁用态说明检查：凡登记了禁用态的项必须带说明（灰着要自己解释）。
    pub fn disabled_states_documented(&self) -> bool {
        (0..self.n).all(|i| match self.items[i] {
            Some(it) => it.disabled_reason.map(|r| !r.is_empty()).unwrap_or(true),
            None => true,
        })
    }
}

/// 术语表一致性：同一概念全系统同名（登记核心术语——「任务栏」不写成
/// 「工具栏」，「通知中心」不写成「消息中心」）。
pub fn anchor_consistent(label: &str) -> bool {
    // 反例词黑名单（同一概念的错误叫法不得出现）。
    const FORBIDDEN: [&str; 4] = ["工具栏", "消息中心", "我的电脑", "回收筒"];
    !FORBIDDEN.iter().any(|f| label.contains(f))
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_setdesc_checks() -> CheckSet {
    let mut cs = CheckSet::new("F474-setdesc");
    // 1) 三件套样例登记（label+desc+learn 齐全）。
    let mut a = CopyAuditor::new();
    cs.add("register_items", a.add(SettingCopy {
        label: "窗口贴靠",
        desc: "拖窗口到屏幕边缘时自动贴半屏",
        learn_anchor: Some("help/snap"),
        disabled_reason: None,
        risk_note: None,
    }) && a.add(SettingCopy {
        label: "勿扰模式",
        desc: "开启后所有横幅通知改为静默入通知中心",
        learn_anchor: Some("help/dnd"),
        disabled_reason: Some("此功能在专注会话中由系统托管"),
        risk_note: Some("开启后重要提醒也可能被静音"),
    }), "");
    // 2) 三件套覆盖率审计（100% 齐全）。
    let (ok, total) = a.coverage_audit();
    cs.add("coverage_100pct", total == 2 && ok == total, "");
    // 3) 全量扫描：句式 + 锚点 + 术语一致性。
    cs.add("full_audit_green", a.full_audit(), "");
    // 4) 废话式标签拦截。
    cs.add("verbose_label_caught", !label_style_ok("是否开启窗口贴靠功能") && label_style_ok("窗口贴靠"), "");
    // 5) 复述式说明拦截（说明不得等于标签）。
    cs.add("echo_desc_caught", !desc_style_ok("窗口贴靠", "窗口贴靠") && desc_style_ok("窗口贴靠", "拖窗口到屏幕边缘时自动贴半屏"), "");
    // 6) 死锚=0。
    cs.add("dead_anchor_zero", !anchor_valid("help/不存在") && anchor_valid("help/snap") && KNOWN_ANCHORS.len() == 6, "");
    // 7) 术语一致性（反例词拦截）。
    cs.add("terminology_consistent", !anchor_consistent("工具栏行为") && anchor_consistent("任务栏行为"), "");
    // 8) 禁用态说明齐备。
    cs.add("disabled_documented", a.disabled_states_documented(), "");
    // 9) 覆盖率审计对残缺项如实报缺。
    let mut b = CopyAuditor::new();
    b.add(SettingCopy { label: "空说明项", desc: "", learn_anchor: None, disabled_reason: None, risk_note: None });
    let (ok2, total2) = b.coverage_audit();
    cs.add("audit_reports_gap", total2 == 1 && ok2 == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learn_more_never_dangling() {
        let mut a = CopyAuditor::new();
        a.add(SettingCopy {
            label: "自动亮度",
            desc: "环境光变化时自动调节屏幕亮度",
            learn_anchor: Some("help/power"),
            disabled_reason: None,
            risk_note: None,
        });
        assert!(a.full_audit());
        // 挂上死锚的项必须被全量扫描拦下。
        a.add(SettingCopy {
            label: "代理设置",
            desc: "系统代理服务器与例外列表",
            learn_anchor: Some("help/proxy-404"),
            disabled_reason: None,
            risk_note: None,
        });
        assert!(!a.full_audit(), "死锚=0 是硬线");
    }

    #[test]
    fn risk_items_carry_consequence() {
        let risky = SettingCopy {
            label: "清空回收站",
            desc: "永久删除回收站内全部文件",
            learn_anchor: None,
            disabled_reason: None,
            risk_note: Some("删除后不可恢复"),
        };
        assert!(risky.risk_note.is_some());
        assert!(desc_style_ok(risky.label, risky.desc));
    }

    #[test]
    fn disabled_items_explain_themselves() {
        let mut a = CopyAuditor::new();
        a.add(SettingCopy {
            label: "同步",
            desc: "在多台设备间同步设置",
            learn_anchor: None,
            disabled_reason: Some("省电模式下停用"),
            risk_note: None,
        });
        assert!(a.disabled_states_documented());
    }
}

// ===========================================================================
// 深化 v2（F474）：术语反例拦截 / 风险项审计 / 禁用态模板 / 覆盖率
// 账目对总（三件套 × 全设置项扫描口径）
// ===========================================================================

/// 术语反例表（主册「同一概念全系统同名」的反例拦截：同一功能
/// 不许两个名字——登记已知反例对，审计扫描命中即红）。
pub const TERM_ANTI_PATTERNS: [(&str, &str); 4] = [
    ("文件夹", "目录"),   // 二选一：全系统用「文件夹」。
    ("回收站", "废纸篓"), // 全系统用「回收站」。
    ("壁纸", "背景图"),   // 全系统用「壁纸」。
    ("任务栏", "任务条"), // 全系统用「任务栏」。
];

/// 术语一致性扫描（label 命中反例对的第二称 → 违规；返回首个违规对）。
pub fn term_consistency_ok(label: &str) -> bool {
    !TERM_ANTI_PATTERNS.iter().any(|(_, bad)| label.contains(bad))
}

/// 风险项审计（主册「风险项说明带后果」：风险标注项必须给 risk_note
/// 且非空——空风险说明 = 假装没风险，同罪）。
pub fn risk_note_present(item: &SettingCopy) -> bool {
    match item.risk_note {
        Some(r) => !r.is_empty(),
        None => false,
    }
}

/// 禁用态说明模板（主册「此功能在省电模式下停用」句式：
/// 「此功能在{场景}下停用」——禁用态说明必须含模板关键词）。
pub fn disabled_template_ok(reason: &str) -> bool {
    reason.contains("停用") || reason.contains("不可用")
}

/// 覆盖率账目对总（主册「三件套覆盖率审计（全设置项扫描）」：
/// 抽样账与总账一致——auditor 内条目数 = 扫描应到数，不多不少）。
pub fn coverage_reconciled(auditor_count: usize, scanned_total: usize) -> bool {
    auditor_count == scanned_total
}

// ---------------------------------------------------------------------------
// 深化自检（F474 v2）
// ---------------------------------------------------------------------------

pub fn run_setdesc_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F474-v2");
    // 1) 术语反例拦截：第二称命中即红；第一称通过。
    cs.add("term_bad_rejected", !term_consistency_ok("查看废纸篓") && !term_consistency_ok("更换背景图"), "");
    cs.add("term_good_passed", term_consistency_ok("回收站设置") && term_consistency_ok("更换壁纸"), "");
    // 2) 风险项审计：有且非空才过。
    let risky = SettingCopy {
        label: "位置权限",
        desc: "关闭后应用无法获取您的位置信息",
        learn_anchor: None,
        disabled_reason: None,
        risk_note: Some("关闭后依赖定位的功能将无法工作"),
    };
    let empty_risk = SettingCopy { risk_note: Some(""), ..risky };
    cs.add("risk_present", risk_note_present(&risky), "");
    cs.add("risk_empty_rejected", !risk_note_present(&empty_risk), "");
    // 3) 禁用态模板：句式含「停用/不可用」。
    cs.add("disabled_template", disabled_template_ok("此功能在省电模式下停用"), "");
    cs.add("disabled_template_free_text_rejected", !disabled_template_ok("暂时不行"), "");
    // 4) 覆盖率对总：账实相符。
    cs.add("coverage_reconciled", coverage_reconciled(96, 96) && !coverage_reconciled(95, 96), "");
    // 5) 句式规范联动 v1：废话标签 + 复述说明双拦。
    cs.add("style_lint_still_on", !label_style_ok("是否开启窗口贴靠") && !desc_style_ok("窗口贴靠", "窗口贴靠"), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn term_pairs_are_concrete() {
        // 反例对两词互异且第一称非空（登记表自身健康）。
        for (good, bad) in TERM_ANTI_PATTERNS {
            assert!(!good.is_empty());
            assert_ne!(good, bad);
        }
    }

    #[test]
    fn anchor_dead_link_still_zero() {
        // v1 死锚=0 红线在深化后仍守（联动回归）。
        assert!(anchor_valid("help/snap"));
        assert!(!anchor_valid("help/dead"));
    }

    #[test]
    fn risk_and_disabled_independent() {
        // 风险说明与禁用态说明是两个独立维度（互不顶替）。
        let item = SettingCopy {
            label: "VPN 连接",
            desc: "连接后所有流量经隧道转发",
            learn_anchor: Some("help/privacy"),
            disabled_reason: Some("此功能在飞行模式下停用"),
            risk_note: None,
        };
        assert!(disabled_template_ok(item.disabled_reason.unwrap()));
        assert!(!risk_note_present(&item));
    }
}

// ===========================================================================
// 深化 v5（F474）：术语表扩展（登记处统一化）/ 风险项强制说明 /
// 锚点去重审计 / 文案长度红线
// ===========================================================================

/// 术语表登记处（v1 只有黑名单反查——v5 登记正名：同一概念只许一个
/// 名字，错名在登记处有对照，改动可全局同步）。
pub const GLOSSARY: [(&str, [&str; 2]); 4] = [
    ("任务栏", ["工具栏", "dock"]),
    ("通知中心", ["消息中心", "通知栏"]),
    ("回收站", ["回收筒", "垃圾桶"]),
    ("此电脑", ["我的电脑", "computer"]),
];

/// 正名合规（标签用登记处正名——错名一票否决）。
pub fn glossary_ok(label: &str) -> bool {
    GLOSSARY.iter().all(|&(canonical, wrongs)| {
        // 用了错名 → 必须同时含正名（改稿中途不判红——但纯错名判红）。
        if wrongs.iter().any(|w| label.contains(w)) {
            label.contains(canonical)
        } else {
            true
        }
    })
}

/// 风险项强制说明（主册风险后果说明：登记了 disabled_reason 为
/// 「有禁用态」的项，若同时是高风险操作（清/删/重置字样）必须带
/// risk_note——灰着还危险的东西不许没有警告）。
pub fn risk_note_enforced(it: &SettingCopy) -> bool {
    let dangerous = it.label.contains("清") || it.label.contains("删") || it.label.contains("重置");
    if dangerous {
        it.risk_note.map(|r| !r.is_empty()).unwrap_or(false)
    } else {
        true
    }
}

/// 锚点去重审计（同一锚点被两个设置项引用 = 复制粘贴债——「了解更多」
/// 点进去是另一个功能的说明，欺骗性导航）。
pub fn anchors_distinct(items: &[SettingCopy]) -> bool {
    for i in 0..items.len() {
        for j in (i + 1)..items.len() {
            match (items[i].learn_anchor, items[j].learn_anchor) {
                (Some(a), Some(b)) if a == b => return false,
                _ => {}
            }
        }
    }
    true
}

/// 文案长度红线（主册「一句话说明」：说明超 60 字节 = 没写完的段落
/// 塞进了设置页——挪进「了解更多」；标签超 12 字节 = 按钮挤爆）。
pub const LABEL_MAX: usize = 12;
pub const DESC_MAX: usize = 60;

pub fn length_redline(it: &SettingCopy) -> bool {
    it.label.len() <= LABEL_MAX && it.desc.len() <= DESC_MAX
}

pub fn run_setdesc_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F474-v5");
    // 1) 术语表：正名全过、错名被否决、混合（错名+正名）放行。
    cs.add("glossary_canonical", glossary_ok("任务栏设置"), "");
    cs.add("glossary_wrong_rejected", !glossary_ok("工具栏设置"), "");
    cs.add("glossary_mixed_pass", glossary_ok("任务栏（原工具栏）"), "");
    // 2) 风险项强制说明：危险项无说明 = 红；有说明 = 绿；普通项豁免。
    let danger_no_note = SettingCopy { label: "清除全部历史", desc: "删除最近使用记录", learn_anchor: None, disabled_reason: None, risk_note: None };
    let danger_noted = SettingCopy { label: "清除全部历史", desc: "删除最近使用记录", learn_anchor: None, disabled_reason: None, risk_note: Some("历史记录清除后不可恢复") };
    let plain = SettingCopy { label: "窗口贴靠", desc: "拖窗口到屏幕边缘自动贴半屏", learn_anchor: None, disabled_reason: None, risk_note: None };
    cs.add("risk_enforced_danger", !risk_note_enforced(&danger_no_note), "");
    cs.add("risk_enforced_noted", risk_note_enforced(&danger_noted), "");
    cs.add("risk_plain_exempt", risk_note_enforced(&plain), "");
    // 3) 锚点去重：重复锚点判红、互异锚点放行。
    let dup = [
        SettingCopy { label: "窗口贴靠", desc: "贴半屏说明文本", learn_anchor: Some("help/snap"), disabled_reason: None, risk_note: None },
        SettingCopy { label: "多桌面", desc: "虚拟桌面切换说明", learn_anchor: Some("help/snap"), disabled_reason: None, risk_note: None },
    ];
    let distinct = [
        SettingCopy { label: "窗口贴靠", desc: "贴半屏说明文本", learn_anchor: Some("help/snap"), disabled_reason: None, risk_note: None },
        SettingCopy { label: "勿扰模式", desc: "横幅静默入中心说明", learn_anchor: Some("help/dnd"), disabled_reason: None, risk_note: None },
    ];
    cs.add("anchor_dup_caught", !anchors_distinct(&dup), "");
    cs.add("anchor_distinct_ok", anchors_distinct(&distinct), "");
    // 4) 长度红线：超长说明判红、合规项放行。
    let long_desc = SettingCopy { label: "壁纸", desc: "这是一个远超六十字节上限的说明文本，它啰嗦地解释了本该放进帮助页的全部细节，读起来像没写完的段落，违反一句话原则，应判红", learn_anchor: None, disabled_reason: None, risk_note: None };
    cs.add("length_redline_caught", !length_redline(&long_desc), "");
    cs.add("length_redline_ok", length_redline(&plain), "");
    // 5) 登记处与 v1 黑名单同源互证（正名表不含黑名单词的反例校验）。
    cs.add("glossary_v1_bridge", glossary_ok("回收站") && !glossary_ok("回收筒"), "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn glossary_canonical_names_all_pass() {
        for &(canonical, _) in GLOSSARY.iter() {
            assert!(glossary_ok(canonical), "正名 {canonical} 必过");
        }
    }

    #[test]
    fn glossary_wrong_names_all_rejected() {
        for &(_, wrongs) in GLOSSARY.iter() {
            for w in wrongs {
                assert!(!glossary_ok(w), "错名 {w} 必拒");
            }
        }
    }

    #[test]
    fn anchors_distinct_empty_ok() {
        assert!(anchors_distinct(&[]));
    }
}
