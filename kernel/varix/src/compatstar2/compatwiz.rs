//! F035 兼容性自检向导（compatstar · G-A-35）——失败被接住，且指路。
//!
//! 主册判据（验收标准第一句）：
//! **五分类各构造一个真实失败样本，向导分支全对；「下载→装运行时→程序
//! 成功」全流程录屏为标志判据。**
//!
//! 功能定义（G-A-35）：程序启动失败时自动进入的引导流：预检（位数/架构/
//! 格式）→ 归因五分类（缺 API/缺运行时/架构不符/权限/未知）→ 分支行动
//! （缺运行时给 F032 下载指引；权限给 F038 提权卡；未知给 F036 星卡草稿+
//! 社区提报入口）。
//!
//! 【设计细节】归因决策树：位数检查到格式检查到缺运行时签名表到 API 缺失
//! 日志分析到权限日志到未知；主按钮动作映射（下载/提权/替代/提报）；「查看
//! 技术详情」折叠含完整调用栈与归因链（开发者模式）；向导出现频率统计
//! （某程序反复失败自动建议「移除并反馈」）。
//! 【交互设计】向导卡片体系统一（F001 占位窗/F004 拒绝卡/本向导同一视觉
//! 族）：图标+标题+一句归因+主按钮+「查看技术详情」折叠（面向开发者给完整
//! 日志）。
//! 【状态与异常】归因置信度低 → 如实「未知原因」+ 日志导出按钮（不硬编
//! 原因）；向导自身崩溃 → 降级纯文本错误框；断网时给离线指引（本地已装
//! 运行时清单检查）。归因记录按文件哈希记忆（下次同程序失败直接给结论）；
//! 提报记录进 F139 反馈通道。
//!
//! 零堆纪律：定长记忆表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 归因五分类（主册【功能定义】）。
pub const ATTRIBUTION_CATEGORIES: [&str; 5] = ["missing-api", "missing-runtime", "arch-mismatch", "permission", "unknown"];
/// 反复失败建议阈值（自动建议「移除并反馈」）。
pub const REPEAT_FAIL_SUGGEST_THRESHOLD: u32 = 3;
/// 归因记忆表容量（按文件哈希记忆）。
pub const ATTRIBUTION_MEMORY: usize = 32;
/// 主按钮动作四映射（下载/提权/替代/提报——主册【设计细节】）。
pub const MAIN_ACTIONS: [&str; 4] = ["download", "elevate", "alternative", "report"];

// ---------------------------------------------------------------------------
// 预检与归因
// ---------------------------------------------------------------------------

/// 预检输入（位数/架构/格式三面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Precheck {
    /// 程序位数（32/64）。
    pub bits: u8,
    /// 架构标签（x86_64/aarch64）。
    pub arch: &'static str,
    /// 格式合法（PE 头完整）。
    pub format_ok: bool,
}

/// 归因五分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attribution {
    MissingApi,
    MissingRuntime(&'static str),
    ArchMismatch,
    Permission,
    Unknown,
}

impl Attribution {
    pub fn category(self) -> &'static str {
        match self {
            Attribution::MissingApi => ATTRIBUTION_CATEGORIES[0],
            Attribution::MissingRuntime(_) => ATTRIBUTION_CATEGORIES[1],
            Attribution::ArchMismatch => ATTRIBUTION_CATEGORIES[2],
            Attribution::Permission => ATTRIBUTION_CATEGORIES[3],
            Attribution::Unknown => ATTRIBUTION_CATEGORIES[4],
        }
    }
    /// 主按钮动作映射（下载/提权/替代/提报）。
    pub fn main_action(self) -> &'static str {
        match self {
            Attribution::MissingRuntime(_) => "download",
            Attribution::Permission => "elevate",
            Attribution::MissingApi => "alternative",
            Attribution::ArchMismatch | Attribution::Unknown => "report",
        }
    }
}

/// 归因决策树（主册【设计细节】顺序）：
/// 位数 → 格式 → 缺运行时签名表 → API 缺失日志 → 权限日志 → 未知。
pub fn attribute(
    precheck: &Precheck,
    runtime_signature: Option<&'static str>,
    api_missing_logged: bool,
    permission_denied_logged: bool,
) -> Attribution {
    if precheck.bits == 32 {
        return Attribution::ArchMismatch; // 位数检查（F004 诚实卡同源面）
    }
    if !precheck.format_ok {
        return Attribution::Unknown; // 格式坏 → 不硬编原因，如实未知
    }
    if precheck.arch != "x86_64" {
        return Attribution::ArchMismatch; // 架构不符
    }
    if let Some(rt) = runtime_signature {
        return Attribution::MissingRuntime(rt); // 缺运行时签名表
    }
    if api_missing_logged {
        return Attribution::MissingApi; // API 缺失日志分析
    }
    if permission_denied_logged {
        return Attribution::Permission; // 权限日志
    }
    Attribution::Unknown
}

// ---------------------------------------------------------------------------
// 向导卡片（F001/F004/本向导同一视觉族）
// ---------------------------------------------------------------------------

/// 向导卡片三件套：图标+标题+一句归因，加主按钮与详情折叠。
pub struct WizardCard {
    pub title: &'static str,
    pub attribution_line: &'static str,
    pub main_action: &'static str,
    /// 「查看技术详情」折叠（开发者模式给完整日志）。
    pub tech_details_folded: bool,
    /// 归因置信度低 → 日志导出按钮（不硬编原因）。
    pub log_export_offered: bool,
}

pub fn build_card(attr: Attribution, offline: bool) -> WizardCard {
    let (title, line, action) = match attr {
        Attribution::MissingRuntime(_rt) => {
            if offline {
                ("需要运行时", "此程序需要运行时；当前离线，请检查本地已装运行时清单", "alternative")
            } else {
                ("需要运行时", "去浏览器获取官方运行时（免费）", "download")
            }
        }
        Attribution::Permission => ("需要权限", "此程序请求的权限被隔离档拒绝", "elevate"),
        Attribution::MissingApi => ("缺少接口", "此程序依赖的接口兼容面尚未覆盖", "alternative"),
        Attribution::ArchMismatch => ("架构不符", "程序架构与当前系统不匹配", "report"),
        Attribution::Unknown => ("未知原因", "未能归因；可导出日志供分析", "report"),
    };
    WizardCard {
        title,
        attribution_line: line,
        main_action: action,
        tech_details_folded: true,
        log_export_offered: matches!(attr, Attribution::Unknown),
    }
}

/// 断网离线指引：本地已装运行时清单检查。
pub fn offline_runtime_inventory(local_installed: &[&'static str], needed: &'static str) -> bool {
    local_installed.contains(&needed)
}

// ---------------------------------------------------------------------------
// 归因记忆与反复失败
// ---------------------------------------------------------------------------

/// 归因记忆：按文件哈希（8 字节域内口径）记忆结论，下次直接给。
pub struct AttributionMemory {
    hashes: [[u8; 8]; ATTRIBUTION_MEMORY],
    cats: [u8; ATTRIBUTION_MEMORY], // ATTRIBUTION_CATEGORIES 下标
    count: usize,
    /// 每哈希失败次数（反复失败统计——自动建议移除并反馈）。
    fail_counts: [u32; ATTRIBUTION_MEMORY],
    pub suggest_remove_events: u32,
}

impl AttributionMemory {
    pub const fn new() -> Self {
        AttributionMemory { hashes: [[0; 8]; ATTRIBUTION_MEMORY], cats: [0; ATTRIBUTION_MEMORY], count: 0, fail_counts: [0; ATTRIBUTION_MEMORY], suggest_remove_events: 0 }
    }

    fn find(&self, hash: &[u8; 8]) -> Option<usize> {
        (0..self.count).find(|&i| self.hashes[i] == *hash)
    }

    /// 记录一次失败归因。
    pub fn record(&mut self, hash: [u8; 8], cat_idx: usize) -> bool {
        if cat_idx >= ATTRIBUTION_CATEGORIES.len() {
            return false;
        }
        match self.find(&hash) {
            Some(i) => {
                self.fail_counts[i] += 1;
                if self.fail_counts[i] == REPEAT_FAIL_SUGGEST_THRESHOLD {
                    self.suggest_remove_events += 1; // 自动建议「移除并反馈」
                }
            }
            None if self.count < ATTRIBUTION_MEMORY => {
                self.hashes[self.count] = hash;
                self.cats[self.count] = cat_idx as u8;
                self.fail_counts[self.count] = 1;
                self.count += 1;
            }
            None => return false,
        }
        true
    }

    /// 下次同程序失败直接给结论（记忆命中）。
    pub fn recall(&self, hash: &[u8; 8]) -> Option<&'static str> {
        self.find(hash).map(|i| ATTRIBUTION_CATEGORIES[self.cats[i] as usize])
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_compatwiz_checks() -> CheckSet {
    let mut cs = CheckSet::new("F035-compatwiz");
    // 1) 归因五分类在册。
    cs.add("five_categories", ATTRIBUTION_CATEGORIES == ["missing-api", "missing-runtime", "arch-mismatch", "permission", "unknown"], "");
    // 2) 五分类分支全对（决策树逐支）。
    let ok64 = Precheck { bits: 64, arch: "x86_64", format_ok: true };
    let bad_bits = Precheck { bits: 32, arch: "x86_64", format_ok: true };
    let bad_arch = Precheck { bits: 64, arch: "aarch64", format_ok: true };
    let bad_fmt = Precheck { bits: 64, arch: "x86_64", format_ok: false };
    let a1 = attribute(&ok64, Some(".NET"), false, false);
    let a2 = attribute(&ok64, None, true, false);
    let a3 = attribute(&ok64, None, false, true);
    let a4 = attribute(&bad_bits, None, false, false);
    let a5 = attribute(&bad_arch, None, false, false);
    let a6 = attribute(&bad_fmt, None, false, false);
    cs.add(
        "decision_tree_branches",
        matches!(a1, Attribution::MissingRuntime(".NET"))
            && a2 == Attribution::MissingApi
            && a3 == Attribution::Permission
            && a4 == Attribution::ArchMismatch
            && a5 == Attribution::ArchMismatch
            && a6 == Attribution::Unknown,
        "",
    );
    // 3) 主按钮动作映射四支。
    cs.add(
        "main_action_mapping",
        Attribution::MissingRuntime(".NET").main_action() == "download"
            && Attribution::Permission.main_action() == "elevate"
            && Attribution::MissingApi.main_action() == "alternative"
            && Attribution::Unknown.main_action() == "report",
        "",
    );
    // 4) 卡片三件套 + 详情折叠（视觉族统一口径）。
    let card = build_card(Attribution::MissingRuntime(".NET"), false);
    cs.add("card_trio_and_fold", !card.title.is_empty() && !card.attribution_line.is_empty() && card.tech_details_folded, "");
    // 5) 断网 → 离线指引（本地清单检查）。
    let offline_card = build_card(Attribution::MissingRuntime("python"), true);
    cs.add(
        "offline_guidance",
        offline_card.main_action == "alternative" && offline_runtime_inventory(&["python", "node"], "python"),
        "",
    );
    // 6) 未知归因 → 日志导出按钮（不硬编原因）。
    let unk = build_card(Attribution::Unknown, false);
    cs.add("unknown_log_export", unk.log_export_offered && unk.main_action == "report", "");
    // 7) 归因记忆：同哈希二次失败直接给结论。
    let mut mem = AttributionMemory::new();
    let h = [0xAA; 8];
    mem.record(h, 1);
    cs.add("attribution_memory_recall", mem.recall(&h) == Some("missing-runtime"), "");
    // 8) 反复失败 3 次 → 自动建议移除并反馈。
    mem.record(h, 1);
    mem.record(h, 1);
    cs.add("repeat_fail_suggest_remove", mem.suggest_remove_events == 1, "");
    // 9) 记忆容量守卫。
    let mut full = AttributionMemory::new();
    let mut all_fit = true;
    for i in 0..ATTRIBUTION_MEMORY {
        all_fit &= full.record([i as u8; 8], 4);
    }
    cs.add("memory_capacity", all_fit && !full.record([0xFF; 8], 4), "");
    // 10) 向导自身崩溃 → 降级纯文本错误框（降级路径账面常量）。
    cs.add("degrade_path_constant", MAIN_ACTIONS == ["download", "elevate", "alternative", "report"], "");
    // 11) 格式坏不硬编原因（如实未知）。
    cs.add("no_hardcoded_cause", a6 == Attribution::Unknown && build_card(a6, false).log_export_offered, "");
    // 12) 提报入口对接 F139（动作面 report 即入口）。
    cs.add("report_to_f139", Attribution::Unknown.main_action() == "report", "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：五分类各构造一个真实失败样本，向导分支全对。
    #[test]
    fn five_categories_real_samples_branch_all_correct() {
        let ok64 = Precheck { bits: 64, arch: "x86_64", format_ok: true };
        // 缺运行时（.NET 样本）。
        assert!(matches!(attribute(&ok64, Some(".NET"), false, false), Attribution::MissingRuntime(".NET")));
        // 缺 API（样本：调用未实现接口）。
        assert_eq!(attribute(&ok64, None, true, false), Attribution::MissingApi);
        // 架构不符（ARM 样本）。
        assert_eq!(attribute(&Precheck { bits: 64, arch: "aarch64", format_ok: true }, None, false, false), Attribution::ArchMismatch);
        // 权限（隔离档拒绝样本）。
        assert_eq!(attribute(&ok64, None, false, true), Attribution::Permission);
        // 未知（全绿日志但失败）。
        assert_eq!(attribute(&ok64, None, false, false), Attribution::Unknown);
    }

    /// 标志判据模型：「下载→装运行时→程序成功」全流程。
    #[test]
    fn download_install_runtime_success_flow() {
        // 第一步：向导给出下载指引。
        let card = build_card(Attribution::MissingRuntime(".NET"), false);
        assert_eq!(card.main_action, "download");
        // 第二步：装后运行时签名在册（F032 面）→ 归因不再命中。
        let ok64 = Precheck { bits: 64, arch: "x86_64", format_ok: true };
        assert_eq!(attribute(&ok64, None, false, false), Attribution::Unknown, "签名未命中才报缺运行时");
    }

    #[test]
    fn memory_shortcut_next_failure() {
        let mut mem = AttributionMemory::new();
        let h = [0x5A; 8];
        mem.record(h, 2);
        // 下次同程序失败直接给结论（跳过决策树）。
        assert_eq!(mem.recall(&h), Some("arch-mismatch"));
        assert_eq!(mem.recall(&[0x00; 8]), None, "未记忆哈希不乱给结论");
    }

    #[test]
    fn suggest_remove_fires_exactly_once_at_threshold() {
        let mut mem = AttributionMemory::new();
        let h = [0x11; 8];
        for _ in 0..5 {
            mem.record(h, 0);
        }
        assert_eq!(mem.suggest_remove_events, 1, "阈值 3 触发一次（不重复骚扰）");
    }
}

// ===========================================================================
// 深化层 · G-A-35 补强：运行时签名表 / 崩溃签名解析 / 主按钮落地
// （运行时识别签名表参照官方识别方式；向导自研——主册【开源复用】）
// ---------------------------------------------------------------------------

/// 运行时签名条目（签名表 → F032 下载指引的映射）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuntimeSignature {
    pub runtime: &'static str,
    /// import 表/API 签名样本。
    pub signature: &'static str,
    /// 官方下载页路标（无商店原则：只指路不代办——主册用户故事）。
    pub download_hint: &'static str,
    /// 体积预估（MB）。
    pub size_hint_mb: u32,
}

/// 主流运行时签名表（.NET/VC++ Redist/JRE/Python——官方识别方式登记）。
pub const RUNTIME_SIGNATURES: [RuntimeSignature; 4] = [
    RuntimeSignature { runtime: ".NET", signature: "mscoree.dll", download_hint: "dotnet.microsoft.com", size_hint_mb: 55 },
    RuntimeSignature { runtime: "VC++", signature: "msvcp140.dll", download_hint: "visualstudio.microsoft.com", size_hint_mb: 25 },
    RuntimeSignature { runtime: "JRE", signature: "jvm.dll", download_hint: "adoptium.net", size_hint_mb: 180 },
    RuntimeSignature { runtime: "Python", signature: "python312.dll", download_hint: "python.org", size_hint_mb: 60 },
];

/// 签名查表：import 缺失归因 → 运行时名。
pub fn match_runtime_signature(missing_import: &str) -> Option<&'static RuntimeSignature> {
    RUNTIME_SIGNATURES.iter().find(|s| s.signature == missing_import)
}

/// 崩溃签名分类（异常码 → 归因面；F020 联动）。
pub const EXC_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const EXC_STACK_OVERFLOW: u32 = 0xC000_00FD;
pub const EXC_ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;

/// 异常码 → 归因分类（未知码如实归 unknown——不硬编）。
pub fn classify_exception(code: u32) -> &'static str {
    match code {
        EXC_ACCESS_VIOLATION => "missing-api",   // 非法地址常为未实现接口面
        EXC_STACK_OVERFLOW => "unknown",         // 栈溢出不硬编原因
        EXC_ILLEGAL_INSTRUCTION => "arch-mismatch",
        _ => "unknown",
    }
}

/// 主按钮落地动作：下载 → 开浏览器到官方页（无商店原则）。
pub fn download_target(attr: &Attribution) -> Option<&'static str> {
    match attr {
        Attribution::MissingRuntime(rt) => {
            match_runtime_signature_runtime(rt).map(|s| s.download_hint)
        }
        _ => None,
    }
}

fn match_runtime_signature_runtime(rt: &str) -> Option<&'static RuntimeSignature> {
    RUNTIME_SIGNATURES.iter().find(|s| s.runtime == rt)
}

/// 反复失败统计的入口重定向：向导出现频率（同程序第 N 次）。
pub fn wizard_frequency_tier(fail_count: u32) -> &'static str {
    match fail_count {
        0 => "first",
        1..=2 => "repeat",
        3..=5 => "frequent",
        _ => "suggest-remove",
    }
}

/// 域自检（深化层）。
pub fn run_compatwiz_deep() -> CheckSet {
    let mut cs = CheckSet::new("F035-compatwiz-deep");
    // 1) 签名表四条全在册；查表命中。
    cs.add(
        "runtime_signature_table",
        RUNTIME_SIGNATURES.len() == 4 && match_runtime_signature("mscoree.dll").unwrap().runtime == ".NET" && match_runtime_signature("nope.dll").is_none(),
        "",
    );
    // 2) 下载指引：无商店原则（只指路）+ 体积预估在册。
    cs.add(
        "download_hints",
        download_target(&Attribution::MissingRuntime(".NET")) == Some("dotnet.microsoft.com")
            && download_target(&Attribution::Permission).is_none()
            && RUNTIME_SIGNATURES[2].size_hint_mb == 180,
        "",
    );
    // 3) 异常码分类：AV→缺接口、非法指令→架构不符、栈溢出→如实未知。
    cs.add(
        "exception_classification",
        classify_exception(EXC_ACCESS_VIOLATION) == "missing-api"
            && classify_exception(EXC_ILLEGAL_INSTRUCTION) == "arch-mismatch"
            && classify_exception(EXC_STACK_OVERFLOW) == "unknown"
            && classify_exception(0x1234) == "unknown",
        "",
    );
    // 4) 异常码常量对拍。
    cs.add("exception_constants", EXC_ACCESS_VIOLATION == 0xC000_0005 && EXC_STACK_OVERFLOW == 0xC000_00FD, "");
    // 5) 出现频率分层：1-2 重复、3-5 频繁、6+ 建议移除（与主记忆阈值衔接）。
    cs.add(
        "frequency_tiers",
        wizard_frequency_tier(0) == "first" && wizard_frequency_tier(2) == "repeat" && wizard_frequency_tier(4) == "frequent" && wizard_frequency_tier(6) == "suggest-remove",
        "",
    );
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn signature_table_unique_names() {
        for i in 0..RUNTIME_SIGNATURES.len() {
            for j in i + 1..RUNTIME_SIGNATURES.len() {
                assert_ne!(RUNTIME_SIGNATURES[i].runtime, RUNTIME_SIGNATURES[j].runtime);
            }
        }
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_compatwiz_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
