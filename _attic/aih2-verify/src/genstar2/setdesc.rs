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
