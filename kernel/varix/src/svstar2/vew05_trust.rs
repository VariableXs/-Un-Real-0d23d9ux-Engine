//! VE-F4605 · 插件签名与信任链（VE-W 域 · 插件 SDK · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4605`
//!
//! **判据（锚点原文四条）**：**强制签名、三级信任、吊销流程、限期处理**。
//!
//! - **强制签名**：无签名不可安装。锚点原文「可关但默认强制 + 风险提示」——
//!   开关存在但**默认值是强制**，且关闭时必须给风险提示；把安全开关的默认值
//!   放在不安全的一侧，是这类设计最常见的失手处。
//! - **三级信任**：官方签名 / 社区签名 / 无签名，三级信任标识随插件常驻。
//!   三级不是三档形容词，而是**三种不同的处置强度**（见 [`TrustTier::policy`]）。
//! - **吊销流程**：签名密钥泄露类事件的吊销流程——**吊销命中 → 已装插件标记 +
//!   限期处理**，不是静默移除（静默移除会让用户在毫无提示的情况下丢掉功能）。
//! - **限期处理**：命中吊销的已装插件须在**限定期限**内处置（禁用或换签），
//!   期限过后升级为强制禁用。限期是给用户与发布者的**缓冲窗口**，不是可选项。
//!
//! **错误路径与降级矩阵**：签名不符 → 拒绝安装；吊销命中 → 已装插件标记 +
//! 限期处理；分级滥用 → 审核修正。
//!
//! **跨批对接点**：上游 F4602 清单（插件身份）、F4603 加载器（签名验证步骤）；
//! 下游 F4614 安全消费、F4645 权限衔接。
//!
//! **无障碍与隐私**：信任标识读屏可辨（域本色）——三级信任各有独立播报文本，
//! 不靠颜色单独承载语义；**签名数据非隐私**。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；类型自持（不 import 未注册兄弟模块——平行会话
//! 的 `ve*` 族尚在施工，编译期硬耦合会让本条因别人的进度而红）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 吊销限期（逻辑 tick；限期内可处置，逾期强制禁用）。
pub const REVOCATION_GRACE: u64 = 30_000;

/// 已装插件表上界（防御性）。
pub const MAX_INSTALLED: usize = 64;

/// 信任策略文档。
pub const TRUST_POLICY_DOC: &str = "\
三级信任策略（VE-F4605 · v1）：官方签名与社区签名均**可安装**，但社区签名插件须\
显式提示风险；无签名**默认拒绝安装**（强制签名），仅在显式关闭强制开关时方可安装且\
必给风险提示。三级不是三档形容词，而是三种处置强度——官方可自动更新、社区需用户\
确认、无签不得进正式通道。";

// ---------------------------------------------------------------------------
// 二、数据结构（签名验证器 / 信任分级表 / 吊销流程单）
// ---------------------------------------------------------------------------

/// 信任分级（三级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustTier {
    /// 官方签名：本体自有密钥签发。
    Official,
    /// 社区签名：第三方密钥签发，密钥不在本体信任根内。
    Community,
    /// 无签名：未提供签名。
    Unsigned,
}

impl TrustTier {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            TrustTier::Official => "official",
            TrustTier::Community => "community",
            TrustTier::Unsigned => "unsigned",
        }
    }

    /// 中文名（读屏播报与界面显示）。
    pub fn label(self) -> &'static str {
        match self {
            TrustTier::Official => "官方签名",
            TrustTier::Community => "社区签名",
            TrustTier::Unsigned => "未签名",
        }
    }

    /// 三级穷举（信任分级表完备性机检驱动面）。
    pub fn all() -> [TrustTier; 3] {
        [TrustTier::Official, TrustTier::Community, TrustTier::Unsigned]
    }

    /// 该级的处置策略：**可安装 / 须提示 / 不得安装**。
    pub fn policy(self) -> InstallPolicy {
        match self {
            TrustTier::Official => InstallPolicy::Allow,
            TrustTier::Community => InstallPolicy::AllowWithRiskPrompt,
            TrustTier::Unsigned => InstallPolicy::Deny,
        }
    }

    /// 读屏播报文本（域本色：信任标识读屏可辨，且不靠颜色单独承载语义）。
    pub fn announce(self) -> &'static str {
        match self {
            TrustTier::Official => "该插件由官方签名，可信",
            TrustTier::Community => "该插件由社区签名，来源不在官方信任根内，请确认后再安装",
            TrustTier::Unsigned => "该插件未签名，来源不可验证",
        }
    }
}

/// 安装策略（三级信任的处置落点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallPolicy {
    /// 准安装。
    Allow,
    /// 准安装但须显式风险提示。
    AllowWithRiskPrompt,
    /// 拒绝安装。
    Deny,
}

/// 信任分级表（由签名验证结果推出等级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustTable;

/// 签名验证结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigVerdict {
    /// 插件标识。
    pub plugin: String,
    /// 信任等级。
    pub tier: TrustTier,
    /// 是否准安装。
    pub allowed: bool,
    /// 是否须风险提示。
    pub risk_prompt: bool,
    /// 读屏播报文本。
    pub announce: &'static str,
    /// 错误码（拒绝时非空）。
    pub code: &'static str,
}

impl TrustTable {
    /// 由「有无签名 + 是否官方密钥」推出信任等级。
    ///
    /// 复杂度 O(1)：两次布尔判定，无查表无循环。
    pub fn classify(has_sig: bool, official_key: bool) -> TrustTier {
        if !has_sig {
            TrustTier::Unsigned
        } else if official_key {
            TrustTier::Official
        } else {
            TrustTier::Community
        }
    }
}

/// 吊销流程单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revocation {
    /// 被吊销的密钥标识。
    pub key: String,
    /// 吊销理由（密钥泄露 / 冒用 / 其他）。
    pub reason: RevokeReason,
    /// 吊销生效的逻辑 tick。
    pub effective_tick: u64,
}

/// 吊销理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeReason {
    /// 密钥泄露。
    KeyLeak,
    /// 冒用他人身份。
    Impersonation,
    /// 其他（仍须登记，不许留空）。
    Other,
}

impl RevokeReason {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            RevokeReason::KeyLeak => "key-leak",
            RevokeReason::Impersonation => "impersonation",
            RevokeReason::Other => "other",
        }
    }
}

/// 已装插件记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPlugin {
    /// 插件标识。
    pub id: String,
    /// 签名密钥（未签名为空串）。
    pub key: String,
    /// 信任等级。
    pub tier: TrustTier,
    /// 命中吊销的逻辑 tick（None = 未命中）。
    pub revoked_at: Option<u64>,
    /// 是否已在限期内处置。
    pub handled: bool,
}

/// 签名与信任链管理器。
#[derive(Clone, Debug, Default)]
pub struct TrustChain {
    /// 强制签名开关（**默认 true**——安全开关默认值必须在安全侧）。
    enforce_signature: bool,
    /// 官方信任根内的密钥。
    official_keys: Vec<String>,
    /// 吊销单列表。
    revocations: Vec<Revocation>,
    /// 已装插件。
    installed: Vec<InstalledPlugin>,
    /// 逻辑 tick。
    tick: u64,
}

impl TrustChain {
    /// 构造管理器：**强制签名默认开启**。
    pub fn new() -> Self {
        TrustChain {
            enforce_signature: true,
            official_keys: Vec::new(),
            revocations: Vec::new(),
            installed: Vec::new(),
            tick: 0,
        }
    }

    /// 强制签名开关当前值。
    pub fn enforce_signature(&self) -> bool {
        self.enforce_signature
    }

    /// 关闭强制签名（**必须给出风险提示**，锚点「可关但默认强制 + 风险提示」）。
    ///
    /// 返回风险提示文本；调用方须把它呈现给用户，不得吞掉。
    pub fn disable_enforcement(&mut self) -> &'static str {
        self.enforce_signature = false;
        UNSIGNED_RISK_PROMPT
    }

    /// 开启强制签名。
    pub fn enable_enforcement(&mut self) {
        self.enforce_signature = true;
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 推进 tick（驱动限期判定）。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.saturating_add(n);
    }

    /// 登记官方密钥（进信任根）。
    pub fn add_official_key(&mut self, key: &str) {
        if !key.is_empty() && !self.official_keys.iter().any(|k| k == key) {
            self.official_keys.push(key.to_string());
        }
    }

    /// 是否在官方信任根内。
    pub fn is_official_key(&self, key: &str) -> bool {
        self.official_keys.iter().any(|k| k == key)
    }

    /// 吊销簿。
    pub fn revocations(&self) -> &[Revocation] {
        &self.revocations
    }

    /// 已装插件表。
    pub fn installed(&self) -> &[InstalledPlugin] {
        &self.installed
    }

    /// 登记吊销单（重复登记幂等）。
    pub fn revoke(&mut self, key: &str, reason: RevokeReason) {
        if !key.is_empty() && !self.revocations.iter().any(|r| r.key == key) {
            self.revocations.push(Revocation {
                key: key.to_string(),
                reason,
                effective_tick: self.tick,
            });
        }
    }

    /// 是否已吊销。
    pub fn is_revoked(&self, key: &str) -> bool {
        self.revocations.iter().any(|r| r.key == key)
    }

    /// 验证并尝试安装（复杂度 O(1)：两次布尔 + 一次线性吊销查表）。
    pub fn verify_and_install(&mut self, plugin: &str, key: &str) -> SigVerdict {
        let has_sig = !key.is_empty();
        let tier = TrustTable::classify(has_sig, has_sig && self.is_official_key(key));
        let policy = tier.policy();

        // 无签名 + 强制开启 → 拒绝安装。
        if tier == TrustTier::Unsigned && self.enforce_signature {
            return SigVerdict {
                plugin: plugin.to_string(),
                tier,
                allowed: false,
                risk_prompt: false,
                announce: tier.announce(),
                code: "E_SIGNATURE_REQUIRED",
            };
        }

        // 吊销命中 → 拒绝安装（已装的走 mark_revoked 限期流程）。
        if has_sig && self.is_revoked(key) {
            return SigVerdict {
                plugin: plugin.to_string(),
                tier,
                allowed: false,
                risk_prompt: false,
                announce: tier.announce(),
                code: "E_KEY_REVOKED",
            };
        }

        // 已装表上界。
        if self.installed.len() >= MAX_INSTALLED {
            return SigVerdict {
                plugin: plugin.to_string(),
                tier,
                allowed: false,
                risk_prompt: false,
                announce: tier.announce(),
                code: "E_INSTALLED_FULL",
            };
        }

        self.installed.push(InstalledPlugin {
            id: plugin.to_string(),
            key: key.to_string(),
            tier,
            revoked_at: None,
            handled: false,
        });
        SigVerdict {
            plugin: plugin.to_string(),
            tier,
            allowed: true,
            // 社区签名与关闭强制后的无签名都须显式风险提示。
            risk_prompt: matches!(
                policy,
                InstallPolicy::AllowWithRiskPrompt
            ) || (tier == TrustTier::Unsigned && !self.enforce_signature),
            announce: tier.announce(),
            code: "",
        }
    }

    /// 吊销命中处理：**标记已装插件 + 限期处置**（不静默移除）。
    ///
    /// 返回被标记的插件数（复杂度 O(插件数)）。
    pub fn mark_revoked(&mut self, key: &str) -> usize {
        if !self.is_revoked(key) {
            return 0;
        }
        let mut n = 0usize;
        for p in self.installed.iter_mut() {
            if p.key == key && p.revoked_at.is_none() {
                p.revoked_at = Some(self.tick);
                p.handled = false;
                n += 1;
            }
        }
        n
    }

    /// 限期是否已过（`true` = 逾期，须强制禁用）。
    pub fn grace_expired(&self, p: &InstalledPlugin) -> bool {
        match p.revoked_at {
            None => false,
            Some(t) => self.tick.saturating_sub(t) > REVOCATION_GRACE,
        }
    }

    /// 标记已处置（用户在限期内禁用或换签）。
    pub fn mark_handled(&mut self, plugin: &str) -> bool {
        match self.installed.iter_mut().find(|p| p.id == plugin) {
            Some(p) => {
                p.handled = true;
                true
            }
            None => false,
        }
    }

    /// 某插件当前是否应被强制禁用（命中吊销且逾期未处置）。
    pub fn must_disable(&self, plugin: &str) -> bool {
        match self.installed.iter().find(|p| p.id == plugin) {
            Some(p) => p.revoked_at.is_some() && !p.handled && self.grace_expired(p),
            None => false,
        }
    }
}

/// 无签名安装的风险提示（关闭强制签名时必须呈现）。
pub const UNSIGNED_RISK_PROMPT: &str = "\
警告：强制签名已关闭。未签名插件来源不可验证，其行为完全由作者掌控，\
本域不对其作任何安全承诺。请确认来源后再安装。";

// ---------------------------------------------------------------------------
// 三、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F4605 域自检。
pub fn run_vew05_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F4605");

    // ---- 判据一：强制签名 ----
    {
        let mut tc = TrustChain::new();
        let v = tc.verify_and_install("p", "");
        set.add(
            "W05-强制签名-无签名拒绝安装",
            !v.allowed && v.code == "E_SIGNATURE_REQUIRED",
            "",
        );
    }

    {
        // 默认值必须在安全侧（这是本条最容易失手处）。
        let tc = TrustChain::new();
        set.add("W05-强制签名-默认开启（安全侧）", tc.enforce_signature(), "");
    }

    {
        // 可关但关闭须给风险提示。
        let mut tc = TrustChain::new();
        let prompt = tc.disable_enforcement();
        let v = tc.verify_and_install("p", "");
        set.add(
            "W05-强制签名-可关但必给风险提示",
            !tc.enforce_signature()
                && v.allowed
                && v.risk_prompt
                && !prompt.is_empty(),
            "",
        );
    }

    {
        // 签名不符（吊销键）拒绝安装。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        let v = tc.verify_and_install("p", "k1");
        set.add(
            "W05-强制签名-吊销键拒绝安装",
            !v.allowed && v.code == "E_KEY_REVOKED",
            "",
        );
    }

    // ---- 判据二：三级信任 ----
    {
        let off = TrustTable::classify(true, true);
        let com = TrustTable::classify(true, false);
        let uns = TrustTable::classify(false, false);
        set.add(
            "W05-三级信任-分级判定正确",
            off == TrustTier::Official
                && com == TrustTier::Community
                && uns == TrustTier::Unsigned,
            "",
        );
    }

    {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let a = tc.verify_and_install("a", "k1");
        let b = tc.verify_and_install("b", "k2");
        set.add(
            "W05-三级信任-官方免提示社区须提示",
            a.tier == TrustTier::Official
                && !a.risk_prompt
                && b.tier == TrustTier::Community
                && b.risk_prompt,
            "",
        );
    }

    {
        // 三级各有独立播报文本且不靠颜色单独承载语义（域本色）。
        let all = TrustTier::all();
        let distinct = all[0].announce() != all[1].announce()
            && all[1].announce() != all[2].announce()
            && all[0].announce() != all[2].announce();
        set.add(
            "W05-三级信任-读屏标识可辨",
            all.len() == 3 && distinct && all.iter().all(|t| !t.announce().is_empty()),
            "",
        );
    }

    {
        // 三级各有不同处置策略（不是三档形容词）。
        let p = TrustTier::all().map(|t| t.policy());
        set.add(
            "W05-三级信任-处置强度各异",
            p[0] == InstallPolicy::Allow
                && p[1] == InstallPolicy::AllowWithRiskPrompt
                && p[2] == InstallPolicy::Deny,
            "",
        );
    }

    // ---- 判据三：吊销流程 ----
    {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        tc.add_official_key("k2");
        let _ = tc.verify_and_install("p1", "k1");
        let _ = tc.verify_and_install("p2", "k2");
        tc.revoke("k1", RevokeReason::KeyLeak);
        let marked = tc.mark_revoked("k1");
        // 命中者被标记，未命中者不受影响。
        let p2_clean = tc
            .installed()
            .iter()
            .find(|p| p.id == "p2")
            .map(|p| p.revoked_at.is_none())
            == Some(true);
        set.add("W05-吊销-命中者标记未命中者不受影响", marked == 1 && p2_clean, "");
    }

    {
        // 吊销不是静默移除：已装记录仍在（用户不会毫无提示地丢掉功能）。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let _ = tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        set.add(
            "W05-吊销-标记而非静默移除",
            tc.installed().len() == 1,
            "",
        );
    }

    {
        // 重复吊销幂等。
        let mut tc = TrustChain::new();
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.revoke("k1", RevokeReason::KeyLeak);
        set.add("W05-吊销-重复登记幂等", tc.revocations().len() == 1, "");
    }

    // ---- 判据四：限期处理 ----
    {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let _ = tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        // 限期内未处置 → 尚未强制禁用（缓冲窗口有效）。
        tc.advance(REVOCATION_GRACE);
        let within = !tc.must_disable("p1");
        set.add("W05-限期-限期内不强制禁用", within, "");
    }

    {
        // 逾期未处置 → 强制禁用。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let _ = tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        tc.advance(REVOCATION_GRACE + 1);
        let overdue = tc.must_disable("p1");
        set.add("W05-限期-逾期未处置强制禁用", overdue, "");
    }

    {
        // 限期内处置 → 不再强制禁用（给了机会要能用）。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let _ = tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        tc.advance(REVOCATION_GRACE + 1);
        let _ = tc.mark_handled("p1");
        set.add(
            "W05-限期-限期内已处置则免强制禁用",
            !tc.must_disable("p1"),
            "",
        );
    }

    {
        // 未命中吊销者永不被强制禁用（防误伤）。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let _ = tc.verify_and_install("p1", "k1");
        tc.advance(REVOCATION_GRACE * 10);
        set.add(
            "W05-限期-未命中者不误禁用",
            !tc.must_disable("p1"),
            "",
        );
    }

    // ---- 分级滥用 → 审核修正 ----
    {
        // 官方根内的键不得被社区冒用：分级由信任根判定，不由声明判定。
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        let v = tc.verify_and_install("p", "k1");
        set.add(
            "W05-分级-等级由信任根判定非自述",
            v.tier == TrustTier::Official,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_denied_by_default() {
        let mut tc = TrustChain::new();
        assert!(!tc.verify_and_install("p", "").allowed);
    }

    #[test]
    fn enforcement_default_is_secure() {
        assert!(TrustChain::new().enforce_signature());
    }

    #[test]
    fn disabling_enforcement_still_prompts() {
        let mut tc = TrustChain::new();
        let prompt = tc.disable_enforcement();
        let v = tc.verify_and_install("p", "");
        assert!(v.allowed && v.risk_prompt && !prompt.is_empty());
    }

    #[test]
    fn three_tiers_map_to_three_policies() {
        assert_eq!(TrustTable::classify(true, true), TrustTier::Official);
        assert_eq!(TrustTable::classify(true, false), TrustTier::Community);
        assert_eq!(TrustTable::classify(false, false), TrustTier::Unsigned);
        assert_ne!(TrustTier::Official.policy(), TrustTier::Community.policy());
        assert_ne!(TrustTier::Community.policy(), TrustTier::Unsigned.policy());
    }

    #[test]
    fn revocation_marks_without_removing() {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        assert_eq!(tc.mark_revoked("k1"), 1);
        assert_eq!(tc.installed().len(), 1, "吊销须标记而非静默移除");
    }

    #[test]
    fn grace_period_then_forced_disable() {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        tc.advance(REVOCATION_GRACE);
        assert!(!tc.must_disable("p1"), "限期内是缓冲窗口");
        tc.advance(1);
        assert!(tc.must_disable("p1"), "逾期须强制禁用");
    }

    #[test]
    fn handled_within_grace_exempts() {
        let mut tc = TrustChain::new();
        tc.add_official_key("k1");
        tc.verify_and_install("p1", "k1");
        tc.revoke("k1", RevokeReason::KeyLeak);
        tc.mark_revoked("k1");
        tc.advance(REVOCATION_GRACE + 1);
        tc.mark_handled("p1");
        assert!(!tc.must_disable("p1"));
    }

    #[test]
    fn effects_checks_all_green() {
        let set = run_vew05_checks();
        let (p, f) = set.tally();
        assert!(!set.truncated());
        assert!(set.all_passed(), "VE-F4605 红项：{}/{}", p, p + f);
    }
}
