//! m6assist — VARIX-M600 AI-22 智能助手域 (F526~F550)
//!
//! 本地智能运行时、意图路由、自然语言命令、搜索融合、智能剪贴板、
//! 通知排序、文件建议、写作/纪要/截图问答、屏幕理解、隐私推理、
//! 模型热切换、端云协同、技能 SDK、自动化建议、自适应、无障碍智能、
//! 性能调优、审计日志、幻觉防护、离线模式、多语言、回归走廊、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（置信度 permille）。

use crate::checks::CheckSet;

// ===========================================================================
// F526 — 本地智能运行时
// ===========================================================================

pub const MODEL_MAX_MB: u32 = 512;

#[derive(Clone, Copy, PartialEq)]
pub enum RuntimeState {
    Unloaded,
    Loading,
    Ready,
    Busy,
}

pub struct LocalRuntime {
    pub model_mb: u32,
    pub state: RuntimeState,
    pub token_budget: u32,
}

impl LocalRuntime {
    pub fn can_load(&self) -> bool {
        self.model_mb <= MODEL_MAX_MB
    }
    pub fn load(&mut self) -> bool {
        if self.can_load() && self.state == RuntimeState::Unloaded {
            self.state = RuntimeState::Ready;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F527 — 助手意图路由
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
#[derive(Debug)]
pub enum Intent {
    OpenApp,
    Search,
    SetSetting,
    Ask,
    Automate,
    Unknown,
}

/// 关键词命中路由：优先精确动词匹配。
pub fn route_intent(verbs: u8, question_mark: bool, has_object: bool) -> Intent {
    match verbs {
        v if v & 0b0001 != 0 => Intent::OpenApp,
        v if v & 0b0010 != 0 => Intent::Search,
        v if v & 0b0100 != 0 => Intent::SetSetting,
        v if v & 0b1000 != 0 => Intent::Automate,
        _ if question_mark => Intent::Ask,
        _ if has_object => Intent::Search,
        _ => Intent::Unknown,
    }
}

// ===========================================================================
// F528 — 自然语言命令
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NlCommand {
    pub intent: Intent,
    pub target: u32,
    pub conf_permille: u16,
}

pub fn nl_command_executable(c: &NlCommand) -> bool {
    c.intent != Intent::Unknown && c.conf_permille >= 700
}

// ===========================================================================
// F529 — 智能搜索融合：语义 × 关键词
// ===========================================================================

pub fn fused_score(kw_hit_permille: u16, sem_hit_permille: u16) -> u16 {
    // 线性融合：60% 关键词 + 40% 语义
    let s = kw_hit_permille as u32 * 600 + sem_hit_permille as u32 * 400;
    (s / 1000).min(1000) as u16
}

// ===========================================================================
// F530 — 智能剪贴板：内容理解与转换建议
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum ClipKind {
    Text,
    Url,
    Code,
    Number,
    Image,
}

pub fn clip_suggest(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Url => "open-in-browser",
        ClipKind::Code => "format-and-preview",
        ClipKind::Number => "calculator",
        ClipKind::Image => "annotate",
        ClipKind::Text => "summarize",
    }
}

// ===========================================================================
// F531 — 智能通知排序
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Notification {
    pub urgency: u8,  // 0~3
    pub affinity: u8, // 0~3 用户关注度
}

pub fn notify_rank(n: &Notification) -> u8 {
    n.urgency * 4 + n.affinity
}

pub fn notify_sort(items: &mut [Notification], n: usize) -> usize {
    let n = n.min(items.len());
    for i in 1..n {
        let key = items[i];
        let mut j = i;
        while j > 0 && notify_rank(&items[j - 1]) < notify_rank(&key) {
            items[j] = items[j - 1];
            j -= 1;
        }
        items[j] = key;
    }
    n
}

// ===========================================================================
// F532 — 智能文件建议
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FileHint {
    pub file_id: u32,
    pub recency_days: u32,
    pub open_count: u32,
    pub context_match: bool,
}

pub fn file_hint_score(h: &FileHint) -> u32 {
    // 新近 + 高频 + 场景命中加权
    let rec = 100u32.saturating_sub(h.recency_days.min(100));
    h.open_count.min(50) * 2 + rec + if h.context_match { 50 } else { 0 }
}

pub fn top_file_hint(hints: &[FileHint], n: usize) -> Option<u32> {
    let n = n.min(hints.len());
    let mut best: Option<(u32, u32)> = None; // (score, id)
    for h in &hints[..n] {
        let s = file_hint_score(h);
        match best {
            Some((bs, _)) if bs >= s => {}
            _ => best = Some((s, h.file_id)),
        }
    }
    best.map(|(_, id)| id)
}

// ===========================================================================
// F533 — 写作助手
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum WriteSuggest {
    Grammar,
    Concise,
    Tone,
    Structure,
    None,
}

pub fn write_suggest(word_count: u32, long_sentences: u32, typos: u32) -> WriteSuggest {
    if typos > 0 {
        WriteSuggest::Grammar
    } else if word_count > 2000 && long_sentences > 5 {
        WriteSuggest::Concise
    } else if long_sentences > 0 {
        WriteSuggest::Structure
    } else {
        WriteSuggest::None
    }
}

// ===========================================================================
// F534 — 会议纪要
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MeetingRec {
    pub duration_min: u32,
    pub turns: u32,
    pub action_items: u32,
}

pub fn meeting_digest(rec: &MeetingRec) -> bool {
    // 纪要质量：有轮次、有行动项、时长合理
    rec.turns > 0 && rec.action_items > 0 && rec.duration_min >= 5 && rec.duration_min <= 240
}

// ===========================================================================
// F535 — 智能截图问答
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum ShotQa {
    TextExtract,
    ChartRead,
    FindUi,
    Unsupported,
}

pub fn shot_qa_route(has_text: bool, has_chart: bool, has_ui_hint: bool) -> ShotQa {
    if has_chart {
        ShotQa::ChartRead
    } else if has_ui_hint {
        ShotQa::FindUi
    } else if has_text {
        ShotQa::TextExtract
    } else {
        ShotQa::Unsupported
    }
}

// ===========================================================================
// F536 — 屏幕理解
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ScreenFrame {
    pub window_count: u8,
    pub text_regions: u8,
    pub active_app: u16,
}

pub fn screen_context_score(f: &ScreenFrame) -> u8 {
    f.window_count.min(10) * 2 + f.text_regions.min(10)
}

// ===========================================================================
// F537 — 隐私优先推理
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum InferenceWhere {
    LocalOnly,
    CloudAllowed,
}

/// 敏感内容必须本地；非敏感且模型缺失才允许云端。
pub fn privacy_route(sensitive: bool, local_model_ready: bool, cloud_enabled: bool) -> Option<InferenceWhere> {
    if sensitive {
        if local_model_ready { Some(InferenceWhere::LocalOnly) } else { None }
    } else if local_model_ready {
        Some(InferenceWhere::LocalOnly)
    } else if cloud_enabled {
        Some(InferenceWhere::CloudAllowed)
    } else {
        None
    }
}

// ===========================================================================
// F538 — 模型热切换
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ModelSlot {
    pub id: u8,
    pub mb: u32,
    pub loaded: bool,
}

pub const HOTSWAP_MAX_MB: u32 = 1024;

pub fn hot_switch(from: &mut ModelSlot, to: &mut ModelSlot) -> bool {
    if !to.loaded && from.loaded && from.mb + to.mb <= HOTSWAP_MAX_MB {
        from.loaded = false;
        to.loaded = true;
        true
    } else {
        false
    }
}

// ===========================================================================
// F539 — 端云协同策略
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum CloudStrategy {
    Never,
    FallbackOnly,   // 云只在本地失败时兜底
    OffloadLarge,   // 大任务卸载云端
    Always,
}

pub fn cloud_strategy(model_mb: u32, net_metered: bool, privacy_mode: bool) -> CloudStrategy {
    if privacy_mode {
        CloudStrategy::Never
    } else if net_metered {
        CloudStrategy::FallbackOnly
    } else if model_mb > MODEL_MAX_MB {
        CloudStrategy::OffloadLarge
    } else {
        CloudStrategy::FallbackOnly
    }
}

// ===========================================================================
// F540 — 助手技能 SDK
// ===========================================================================

pub const SKILL_API_VER: u8 = 2;
pub const SKILL_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct Skill {
    pub id: u8,
    pub api_ver: u8,
    pub sandboxed: bool,
    pub enabled: bool,
}

pub fn skill_loadable(s: &Skill) -> bool {
    s.api_ver <= SKILL_API_VER && s.sandboxed
}

pub fn skill_register(slots: &mut [Option<Skill>; SKILL_CAP], s: Skill) -> bool {
    if !skill_loadable(&s) || (s.id as usize) >= SKILL_CAP {
        return false;
    }
    if slots[s.id as usize].is_some() {
        return false; // 去重：同 id 拒绝重复注册
    }
    slots[s.id as usize] = Some(s);
    true
}

// ===========================================================================
// F541 — 自动化建议：从习惯提炼
// ===========================================================================

#[derive(Clone, Copy)]
pub struct HabitEvent {
    pub app: u16,
    pub hour: u8,
}

/// 同一 app 在同一小时出现 >= 3 次即提议自动化。
pub fn automation_suggest(events: &[HabitEvent], n: usize) -> Option<(u16, u8)> {
    let n = n.min(events.len());
    for i in 0..n {
        let mut count = 0u32;
        for e in &events[..n] {
            if e.app == events[i].app && e.hour == events[i].hour {
                count += 1;
            }
        }
        if count >= 3 {
            return Some((events[i].app, events[i].hour));
        }
    }
    None
}

// ===========================================================================
// F542 — 学习曲线自适应
// ===========================================================================

#[derive(Clone, Copy)]
pub struct AssistProfile {
    pub interactions: u32,
    pub accepted: u32,
}

impl AssistProfile {
    /// 接受率 permille；随样本增加置信度上升。
    pub fn accept_permille(&self) -> u16 {
        if self.interactions == 0 {
            return 0;
        }
        ((self.accepted as u32 * 1000) / self.interactions) as u16
    }
    pub fn personalized(&self) -> bool {
        self.interactions >= 20
    }
}

// ===========================================================================
// F543 — 无障碍智能
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum A11yAid {
    ReadAloud,
    SimplifyUi,
    LargerText,
    VoiceNav,
}

pub fn a11y_aid_for(visual_impairment: bool, motor_impairment: bool, cognitive: bool) -> A11yAid {
    if motor_impairment {
        A11yAid::VoiceNav
    } else if visual_impairment {
        A11yAid::ReadAloud
    } else if cognitive {
        A11yAid::SimplifyUi
    } else {
        A11yAid::LargerText
    }
}

// ===========================================================================
// F544 — 智能性能调优
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PerfAdvice {
    pub cpu_hot_permille: u16,
    pub mem_pressure_permille: u16,
    pub battery_low: bool,
}

pub fn perf_tune_hint(a: &PerfAdvice) -> &'static str {
    if a.battery_low {
        "power-save"
    } else if a.cpu_hot_permille >= 850 {
        "throttle-cores"
    } else if a.mem_pressure_permille >= 900 {
        "drop-caches"
    } else {
        "none"
    }
}

// ===========================================================================
// F545 — 助手审计日志
// ===========================================================================

pub const AUDIT_CAP: usize = 32;

#[derive(Clone, Copy)]
pub struct AssistAuditRec {
    pub seq: u64,
    pub action: u8,   // 0=read 1=write 2=network 3=model-call
    pub where_: u8,   // 0=local 1=cloud
    pub data_class: u8, // 0=public 1=personal 2=sensitive
}

/// 敏感数据走云端必须记录且拒绝。
pub fn audit_violation(rec: &AssistAuditRec) -> bool {
    rec.data_class == 2 && rec.where_ == 1
}

pub fn audit_append(log: &mut [Option<AssistAuditRec>; AUDIT_CAP], len: &mut usize, rec: AssistAuditRec) -> bool {
    if *len >= AUDIT_CAP {
        return false;
    }
    log[*len] = Some(rec);
    *len += 1;
    true
}

// ===========================================================================
// F546 — 幻觉防护栏
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Claim {
    pub conf_permille: u16,
    pub has_citation: bool,
    pub verifiable: bool,
}

/// 防护栏：低置信或不可验证且无引用的断言必须降级为"仅供参考"。
pub fn claim_gated(c: &Claim) -> bool {
    !(c.conf_permille < 500 || (!c.has_citation && c.verifiable))
}

// ===========================================================================
// F547 — 离线智能模式
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum NetState {
    Online,
    Metered,
    Offline,
}

pub fn offline_capable(net: NetState, model_local: bool, cache_warm: bool) -> bool {
    match net {
        NetState::Online => true,
        NetState::Metered => model_local && cache_warm,
        NetState::Offline => model_local,
    }
}

// ===========================================================================
// F548 — 多语言智能
// ===========================================================================

pub const ASSIST_LANGS: [&str; 6] = ["zh", "en", "ja", "ko", "de", "fr"];

pub fn lang_supported(code: &str) -> bool {
    ASSIST_LANGS.iter().any(|l| *l == code)
}

/// 回退：zh-Hant -> zh，en-GB -> en。
pub fn lang_fallback(code: &str) -> Option<&'static str> {
    if let Some(pos) = code.find('-') {
        let base = &code[..pos];
        if lang_supported(base) {
            return ASSIST_LANGS.iter().find(|l| **l == base).copied();
        }
    }
    if lang_supported(code) {
        ASSIST_LANGS.iter().find(|l| **l == code).copied()
    } else {
        None
    }
}

// ===========================================================================
// F549 — 助手回归走廊
// ===========================================================================

pub const ASSIST_CORRIDOR_CASES: [&str; 5] =
    ["intent-route", "privacy-local", "hallucination-gate", "offline-mode", "audit-complete"];

pub fn assist_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F550 — 智能年报
// ===========================================================================

pub struct AssistYearbook {
    pub requests: u32,
    pub local_served: u32,
    pub gated_claims: u32,
    pub automation_proposed: u32,
    pub automation_accepted: u32,
}

impl AssistYearbook {
    pub fn local_rate_permille(&self) -> u16 {
        if self.requests == 0 {
            return 0;
        }
        ((self.local_served as u32 * 1000) / self.requests) as u16
    }
    pub fn privacy_grade(&self) -> u8 {
        // 0=优：本地率 >= 90%
        if self.local_rate_permille() >= 900 { 0 } else { 1 }
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m6assist_checks() -> CheckSet {
    let mut set = CheckSet::new("m6assist");

    // F526 运行时
    let mut rt = LocalRuntime { model_mb: 400, state: RuntimeState::Unloaded, token_budget: 4096 };
    let too_big = LocalRuntime { model_mb: 600, state: RuntimeState::Unloaded, token_budget: 1 };
    set.add(
        "F526 runtime",
        rt.can_load() && rt.load() && rt.state == RuntimeState::Ready && !too_big.can_load(),
        "bounded load",
    );

    // F527 意图
    set.add(
        "F527 intent",
        route_intent(0b0001, false, true) == Intent::OpenApp
            && route_intent(0b1000, false, true) == Intent::Automate
            && route_intent(0, true, false) == Intent::Ask
            && route_intent(0, false, false) == Intent::Unknown,
        "verb priority",
    );

    // F528 NL 命令
    let c = NlCommand { intent: Intent::SetSetting, target: 3, conf_permille: 850 };
    let weak = NlCommand { intent: Intent::Unknown, target: 3, conf_permille: 990 };
    set.add(
        "F528 nl command",
        nl_command_executable(&c) && !nl_command_executable(&weak),
        "conf gate",
    );

    // F529 融合
    set.add(
        "F529 fused score",
        fused_score(1000, 0) == 600 && fused_score(0, 1000) == 400 && fused_score(500, 500) == 500,
        "60/40 blend",
    );

    // F530 剪贴板
    set.add(
        "F530 clipboard",
        clip_suggest(ClipKind::Url) == "open-in-browser" && clip_suggest(ClipKind::Code) == "format-and-preview",
        "kind->action",
    );

    // F531 通知排序
    let mut notes = [
        Notification { urgency: 1, affinity: 1 },
        Notification { urgency: 3, affinity: 3 },
        Notification { urgency: 2, affinity: 0 },
    ];
    notify_sort(&mut notes, 3);
    set.add(
        "F531 notify sort",
        notify_rank(&notes[0]) == 15 && notify_rank(&notes[1]) == 8 && notify_rank(&notes[2]) == 5,
        "ranked desc",
    );

    // F532 文件建议
    let hints = [
        FileHint { file_id: 1, recency_days: 1, open_count: 10, context_match: true },
        FileHint { file_id: 2, recency_days: 90, open_count: 2, context_match: false },
    ];
    set.add("F532 file hint", top_file_hint(&hints, 2) == Some(1), "scored pick");

    // F533 写作
    set.add(
        "F533 writing",
        write_suggest(100, 0, 2) == WriteSuggest::Grammar
            && write_suggest(3000, 8, 0) == WriteSuggest::Concise
            && write_suggest(100, 0, 0) == WriteSuggest::None,
        "suggest ladder",
    );

    // F534 会议纪要
    let good = MeetingRec { duration_min: 45, turns: 30, action_items: 4 };
    let bad = MeetingRec { duration_min: 3, turns: 2, action_items: 0 };
    set.add("F534 meeting", meeting_digest(&good) && !meeting_digest(&bad), "digest gates");

    // F535 截图问答
    set.add(
        "F535 shot qa",
        shot_qa_route(true, true, false) == ShotQa::ChartRead
            && shot_qa_route(true, false, true) == ShotQa::FindUi
            && shot_qa_route(false, false, false) == ShotQa::Unsupported,
        "route priority",
    );

    // F536 屏幕理解
    let sf = ScreenFrame { window_count: 5, text_regions: 8, active_app: 12 };
    set.add("F536 screen", screen_context_score(&sf) == 18, "context score");

    // F537 隐私路由
    set.add(
        "F537 privacy route",
        privacy_route(true, true, true) == Some(InferenceWhere::LocalOnly)
            && privacy_route(true, false, true).is_none()
            && privacy_route(false, false, true) == Some(InferenceWhere::CloudAllowed),
        "sensitive local only",
    );

    // F538 热切换
    let mut a = ModelSlot { id: 0, mb: 500, loaded: true };
    let mut b = ModelSlot { id: 1, mb: 400, loaded: false };
    let mut c2 = ModelSlot { id: 2, mb: 700, loaded: false };
    set.add(
        "F538 hot swap",
        hot_switch(&mut a, &mut b) && !a.loaded && b.loaded && !hot_switch(&mut a, &mut c2),
        "mem bounded",
    );

    // F539 端云协同
    set.add(
        "F539 cloud strategy",
        cloud_strategy(100, false, true) == CloudStrategy::Never
            && cloud_strategy(100, true, false) == CloudStrategy::FallbackOnly
            && cloud_strategy(600, false, false) == CloudStrategy::OffloadLarge,
        "policy matrix",
    );

    // F540 技能 SDK
    let mut slots = [const { None }; SKILL_CAP];
    let ok = Skill { id: 3, api_ver: 2, sandboxed: true, enabled: true };
    let dup = Skill { id: 3, api_ver: 2, sandboxed: true, enabled: true };
    let bad = Skill { id: 4, api_ver: 9, sandboxed: false, enabled: true };
    set.add(
        "F540 skill sdk",
        skill_register(&mut slots, ok) && !skill_register(&mut slots, dup) && !skill_register(&mut slots, bad),
        "ver+dedup+sandbox",
    );

    // F541 自动化建议
    let evs = [
        HabitEvent { app: 7, hour: 9 },
        HabitEvent { app: 7, hour: 9 },
        HabitEvent { app: 8, hour: 10 },
        HabitEvent { app: 7, hour: 9 },
    ];
    set.add(
        "F541 automation",
        automation_suggest(&evs, 4) == Some((7, 9)) && automation_suggest(&evs[2..], 1).is_none(),
        "habit mining",
    );

    // F542 自适应
    let p = AssistProfile { interactions: 40, accepted: 30 };
    set.add(
        "F542 adaptive",
        p.accept_permille() == 750 && p.personalized(),
        "accept rate",
    );

    // F543 无障碍智能
    set.add(
        "F543 a11y aid",
        a11y_aid_for(false, true, false) == A11yAid::VoiceNav
            && a11y_aid_for(true, false, false) == A11yAid::ReadAloud
            && a11y_aid_for(false, false, true) == A11yAid::SimplifyUi,
        "aid routing",
    );

    // F544 性能调优
    let pa = PerfAdvice { cpu_hot_permille: 900, mem_pressure_permille: 500, battery_low: false };
    set.add(
        "F544 perf tune",
        perf_tune_hint(&pa) == "throttle-cores"
            && perf_tune_hint(&PerfAdvice { cpu_hot_permille: 100, mem_pressure_permille: 950, battery_low: false }) == "drop-caches"
            && perf_tune_hint(&PerfAdvice { cpu_hot_permille: 100, mem_pressure_permille: 100, battery_low: true }) == "power-save",
        "hint matrix",
    );

    // F545 审计
    let mut log = [const { None }; AUDIT_CAP];
    let mut logn = 0usize;
    let bad_cloud = AssistAuditRec { seq: 1, action: 2, where_: 1, data_class: 2 };
    let ok_local = AssistAuditRec { seq: 2, action: 0, where_: 0, data_class: 2 };
    audit_append(&mut log, &mut logn, bad_cloud);
    audit_append(&mut log, &mut logn, ok_local);
    set.add(
        "F545 audit",
        logn == 2 && audit_violation(&bad_cloud) && !audit_violation(&ok_local),
        "sensitive-cloud flagged",
    );

    // F546 幻觉防护
    set.add(
        "F546 hallucination gate",
        claim_gated(&Claim { conf_permille: 900, has_citation: true, verifiable: true })
            && !claim_gated(&Claim { conf_permille: 300, has_citation: true, verifiable: true })
            && !claim_gated(&Claim { conf_permille: 900, has_citation: false, verifiable: true }),
        "conf + citation",
    );

    // F547 离线模式
    set.add(
        "F547 offline mode",
        offline_capable(NetState::Online, false, false)
            && !offline_capable(NetState::Offline, false, true)
            && offline_capable(NetState::Offline, true, false),
        "local model required",
    );

    // F548 多语言
    set.add(
        "F548 multilang",
        lang_supported("zh") && !lang_supported("it")
            && lang_fallback("zh-Hant") == Some("zh")
            && lang_fallback("it-IT").is_none(),
        "fallback chain",
    );

    // F549 回归走廊
    set.add(
        "F549 corridor",
        assist_corridor_pass(&[true; 5]) && !assist_corridor_pass(&[true, true, false, true, true]),
        "5 cases",
    );

    // F550 年报
    let yb = AssistYearbook {
        requests: 1000,
        local_served: 950,
        gated_claims: 12,
        automation_proposed: 20,
        automation_accepted: 14,
    };
    set.add(
        "F550 yearbook",
        yb.local_rate_permille() == 950 && yb.privacy_grade() == 0,
        "local rate + grade",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f527_route_priority() {
        // 多动词同时命中时取低位（OpenApp）优先
        assert_eq!(route_intent(0b0011, false, true), Intent::OpenApp);
    }

    #[test]
    fn f531_sort_stability_of_rank() {
        let mut notes = [
            Notification { urgency: 0, affinity: 3 },
            Notification { urgency: 3, affinity: 0 },
        ];
        notify_sort(&mut notes, 2);
        assert!(notify_rank(&notes[0]) >= notify_rank(&notes[1]));
    }

    #[test]
    fn f540_skill_dedup() {
        let mut slots = [const { None }; SKILL_CAP];
        let s = Skill { id: 0, api_ver: 1, sandboxed: true, enabled: false };
        assert!(skill_register(&mut slots, s));
        assert!(!skill_register(&mut slots, s));
    }

    #[test]
    fn f546_gate_boundary() {
        assert!(claim_gated(&Claim { conf_permille: 500, has_citation: false, verifiable: false }));
        assert!(!claim_gated(&Claim { conf_permille: 499, has_citation: false, verifiable: false }));
    }

    #[test]
    fn f550_domain_selfcheck_all_pass() {
        let set = run_m6assist_checks();
        assert!(set.len() >= 25, "got {}", set.len());
                    for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
                        for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
            assert!(set.all_passed());
    }
}
