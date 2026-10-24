//! VE-F0598 · C 域查重/验收/压测/安全四联审
//!
//! 职责：C 域四联审——
//! ① 查重审：域内 200 条承诺无重复——程序化相似度扫描 + 人工复核，
//!    命中项合并或显性区分边界；
//! ② 验收审：真实用户视角走查——编辑器内改一处着色器到看到结果的完整旅程
//!    （诊断可读、反馈及时、失败可恢复），体验日志挫败信号
//!    rage click（连续快速重复点击）+ dead click（点击无响应）对照核验；
//! ③ 压测审：四场景记录——大批量变体编译、深层 include 链、超大着色器单文件、
//!    队列洪峰——各场景资源水位与延迟曲线入档；
//! ④ 安全审：源码作为不可信输入的全链清洗、磁盘缓存防篡改 F0590、
//!    无 secrets 泄漏、遥测隐私红线 F0589 核验。
//! 任一审未过→阻断移交限期整改；压测异常→定位归因；安全项违规→最高级即时修。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 审次标识：四审。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditKind {
    Dedup,
    Accept,
    Stress,
    Security,
}

impl AuditKind {
    pub fn name(self) -> &'static str {
        match self {
            AuditKind::Dedup => "查重审",
            AuditKind::Accept => "验收审",
            AuditKind::Stress => "压测审",
            AuditKind::Security => "安全审",
        }
    }
}

/// 审查结论：通过 / 阻断（附整改要求与限期条目）。
#[derive(Clone, Debug, PartialEq)]
pub enum AuditVerdict {
    Pass,
    Block(String, u32),
}

/// ① 查重审：程序化相似度扫描 + 人工复核记录——命中未处置即阻断。
pub fn dedup_audit(unresolved_hits: usize, manual_reviewed: usize, total_hits: usize) -> AuditVerdict {
    if unresolved_hits == 0 && manual_reviewed == total_hits {
        AuditVerdict::Pass
    } else {
        AuditVerdict::Block(
            format!(
                "查重未清：{} 项未处置，人工复核 {}/{}——合并或显性区分后再审",
                unresolved_hits, manual_reviewed, total_hits
            ),
            5998,
        )
    }
}

/// ② 验收审：真实旅程走查清单——三关 + 挫败信号核验。
#[derive(Clone, Debug, PartialEq)]
pub struct JourneyCheck {
    /// 旅程步骤是否全部可走通。
    pub journey_ok: bool,
    /// 诊断可读：错误文本经屏幕阅读器可顺畅朗读（无障碍专项）。
    pub diag_readable: bool,
    /// 反馈及时：改一处着色器到看到结果在预算内。
    pub feedback_in_budget: bool,
    /// 失败可恢复：编译失败后编辑器回到可用态。
    pub recoverable: bool,
    /// rage click：5 秒内同点位连击 >= 5 次记挫败。
    pub rage_clicks: u32,
    /// dead click：点击后 2 秒无响应记 dead。
    pub dead_clicks: u32,
}

/// 挫败信号判定：rage 或 dead 任一非零即挫败命中。
pub fn frustration_hit(c: &JourneyCheck) -> bool {
    c.rage_clicks >= 5 || c.dead_clicks > 0
}

/// 验收审总判：旅程通 + 三体验关 + 无挫败信号，键盘全程可达诊断面板。
pub fn accept_audit(c: &JourneyCheck, keyboard_reachable: bool) -> AuditVerdict {
    if !c.journey_ok {
        return AuditVerdict::Block("完整旅程走不通".to_string(), 5998);
    }
    if !c.diag_readable || !c.feedback_in_budget || !c.recoverable {
        return AuditVerdict::Block("体验三关（可读/及时/可恢复）有未过".to_string(), 5998);
    }
    if !keyboard_reachable {
        return AuditVerdict::Block("无障碍专项：诊断面板键盘不可达".to_string(), 5998);
    }
    if frustration_hit(c) {
        return AuditVerdict::Block("挫败信号命中：rage click 或 dead click".to_string(), 5998);
    }
    AuditVerdict::Pass
}

/// ③ 压测审：四场景档案——每场景记录资源水位与延迟曲线，出归因。
#[derive(Clone, Debug, PartialEq)]
pub struct StressCase {
    pub scene: &'static str,
    /// 峰值内存水位（MiB）。
    pub peak_mem: u32,
    /// p99 延迟（毫秒）。
    pub p99_ms: u32,
    /// 是否触发资源保护（配额拒绝/队列丢弃）。
    pub guarded: bool,
}

/// 压测档案判定：四场景齐 + 各场景延迟曲线在阈值内（或被保护机制兜住）。
pub fn stress_audit(cases: &[StressCase], p99_budget: u32) -> AuditVerdict {
    if cases.len() != 4 {
        return AuditVerdict::Block(
            format!("压测场景 {} != 4：大批量变体/深层 include/超大单文件/队列洪峰", cases.len()),
            5998,
        );
    }
    let bad: Vec<&str> = cases
        .iter()
        .filter(|c| c.p99_ms > p99_budget && !c.guarded)
        .map(|c| c.scene)
        .collect();
    if bad.is_empty() {
        AuditVerdict::Pass
    } else {
        AuditVerdict::Block(
            format!("压测异常未兜住：{}——定位归因", bad.join(", ")),
            5998,
        )
    }
}

/// ④ 安全审：五红线逐项核验（安全项违规→最高级即时修）。
#[derive(Clone, Debug, PartialEq)]
pub struct SecurityCheck {
    /// 源码作为不可信输入：全链清洗完成。
    pub input_sanitized: bool,
    /// 磁盘缓存防篡改（F0590 校验和）在位。
    pub cache_tamper_guard: bool,
    /// 无 secrets 泄漏（日志/遥测/缓存产物全扫过）。
    pub no_secrets: bool,
    /// 遥测隐私红线（F0589：不采源码内容）核验通过。
    pub telemetry_privacy: bool,
    /// 错误文本不含路径等隐私（F0594 呈现层红线）。
    pub err_text_clean: bool,
}

pub fn security_audit(c: &SecurityCheck) -> AuditVerdict {
    let redlines = [
        ("源码不可信输入未全链清洗", c.input_sanitized),
        ("缓存防篡改缺失", c.cache_tamper_guard),
        ("secrets 泄漏风险", c.no_secrets),
        ("遥测隐私红线违规", c.telemetry_privacy),
        ("错误文本含隐私", c.err_text_clean),
    ];
    let bad: Vec<&str> = redlines.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    if bad.is_empty() {
        AuditVerdict::Pass
    } else {
        AuditVerdict::Block(
            format!("安全红线违规（最高级即时修）：{}", bad.join("；")),
            5998,
        )
    }
}

/// 四联审总闸：四审全过才放行移交；任一阻断→限期整改。
pub fn quad_gate(verdicts: &[(AuditKind, AuditVerdict)]) -> Result<(), String> {
    if verdicts.len() != 4 {
        return Err(format!("联审 {} != 4：查重/验收/压测/安全缺审", verdicts.len()));
    }
    let mut blocked: Vec<String> = Vec::new();
    for (k, v) in verdicts {
        if let AuditVerdict::Block(why, due) = v {
            blocked.push(format!("{} 阻断（限期整改 F{}）：{}", k.name(), due, why));
        }
    }
    if blocked.is_empty() {
        Ok(())
    } else {
        Err(blocked.join("\n"))
    }
}

pub fn run_vec198_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec198");
    // 族一：查重审
    s.add("查重全清通过", dedup_audit(0, 3, 3) == AuditVerdict::Pass, "dedup-pass");
    match dedup_audit(2, 1, 3) {
        AuditVerdict::Block(w, due) => {
            s.add("查重未清阻断", w.contains("2 项未处置") && due == 5998, "dedup-block");
        }
        _ => s.add("查重未清阻断", false, "expected block"),
    }
    s.add("漏复核也阻断", !matches!(dedup_audit(0, 2, 3), AuditVerdict::Pass), "dedup-partial");
    // 族二：验收审——真实旅程与挫败信号
    let good = JourneyCheck {
        journey_ok: true, diag_readable: true, feedback_in_budget: true,
        recoverable: true, rage_clicks: 0, dead_clicks: 0,
    };
    s.add("真实旅程全绿通过", accept_audit(&good, true) == AuditVerdict::Pass, "accept-pass");
    let rage = JourneyCheck { rage_clicks: 7, ..good.clone() };
    s.add("rage click 挫败命中", frustration_hit(&rage), "rage");
    let dead = JourneyCheck { dead_clicks: 1, ..good.clone() };
    s.add("dead click 挫败命中", frustration_hit(&dead), "dead");
    match accept_audit(&rage, true) {
        AuditVerdict::Block(w, _) => s.add("挫败信号阻断走查", w.contains("挫败信号"), "frustration"),
        _ => s.add("挫败信号阻断走查", false, "expected block"),
    }
    s.add("键盘不可达阻断", matches!(accept_audit(&good, false), AuditVerdict::Block(_, _)), "kbd");
    let unrecover = JourneyCheck { recoverable: false, ..good.clone() };
    s.add("不可恢复阻断", matches!(accept_audit(&unrecover, true), AuditVerdict::Block(_, _)), "recover");
    // 族三：压测审——四场景
    let cases = vec![
        StressCase { scene: "大批量变体编译", peak_mem: 700, p99_ms: 480, guarded: false },
        StressCase { scene: "深层 include 链", peak_mem: 300, p99_ms: 450, guarded: false },
        StressCase { scene: "超大着色器单文件", peak_mem: 900, p99_ms: 490, guarded: false },
        StressCase { scene: "队列洪峰", peak_mem: 200, p99_ms: 700, guarded: true },
    ];
    s.add("四场景全档", cases.len() == 4, "four-scenes");
    s.add("压测全过（洪峰被保护兜住）", stress_audit(&cases, 500) == AuditVerdict::Pass, "stress-pass");
    let mut bad_case = cases.clone();
    bad_case.push(StressCase { scene: "第五场景", peak_mem: 10, p99_ms: 10, guarded: false });
    match stress_audit(&bad_case, 500) {
        AuditVerdict::Block(w, _) => s.add("场景数不为 4 拒审", w.contains("5 != 4"), "scene-count"),
        _ => s.add("场景数不为 4 拒审", false, "expected block"),
    }
    let mut unprotected = cases.clone();
    unprotected[1].p99_ms = 900;
    unprotected[1].guarded = false;
    match stress_audit(&unprotected, 500) {
        AuditVerdict::Block(w, _) => s.add("延迟超标未兜住点名归因", w.contains("深层 include") && w.contains("定位归因"), "stress-red"),
        _ => s.add("延迟超标未兜住点名归因", false, "expected block"),
    }
    // 族四：安全审——五红线
    let sec = SecurityCheck {
        input_sanitized: true, cache_tamper_guard: true, no_secrets: true,
        telemetry_privacy: true, err_text_clean: true,
    };
    s.add("五红线全过", security_audit(&sec) == AuditVerdict::Pass, "sec-pass");
    let leak = SecurityCheck { no_secrets: false, ..sec.clone() };
    match security_audit(&leak) {
        AuditVerdict::Block(w, _) => {
            s.add("secrets 违规最高级", w.contains("最高级") && w.contains("secrets"), "sec-leak");
        }
        _ => s.add("secrets 违规最高级", false, "expected block"),
    }
    let privacy = SecurityCheck { telemetry_privacy: false, ..sec.clone() };
    s.add("遥测隐私红线违规点名", matches!(security_audit(&privacy), AuditVerdict::Block(_, _)), "sec-privacy");
    // 族五：四联审总闸
    let all_pass = vec![
        (AuditKind::Dedup, AuditVerdict::Pass),
        (AuditKind::Accept, AuditVerdict::Pass),
        (AuditKind::Stress, AuditVerdict::Pass),
        (AuditKind::Security, AuditVerdict::Pass),
    ];
    s.add("四审全过放行移交", quad_gate(&all_pass).is_ok(), "gate-pass");
    let one_block = vec![
        (AuditKind::Dedup, AuditVerdict::Pass),
        (AuditKind::Accept, AuditVerdict::Pass),
        (AuditKind::Stress, AuditVerdict::Block("洪峰未兜住".to_string(), 5998)),
        (AuditKind::Security, AuditVerdict::Pass),
    ];
    match quad_gate(&one_block) {
        Err(m) => {
            s.add("任一阻断限期整改", m.contains("压测审") && m.contains("限期整改") && m.contains("5998"), "gate-block");
        }
        Ok(()) => s.add("任一阻断限期整改", false, "expected err"),
    }
    let missing = vec![(AuditKind::Dedup, AuditVerdict::Pass), (AuditKind::Accept, AuditVerdict::Pass)];
    s.add("缺审拒总闸", quad_gate(&missing).is_err(), "gate-missing");
    s.add("四审名称互异", {
        let k = [AuditKind::Dedup, AuditKind::Accept, AuditKind::Stress, AuditKind::Security];
        k[0].name() != k[1].name() && k[1].name() != k[2].name() && k[2].name() != k[3].name()
    }, "names");
    s
}
