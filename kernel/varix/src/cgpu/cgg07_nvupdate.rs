//! cgg07_nvupdate — CGPU-F0967 NVIDIA 驱动更新应对（驱动不是静态事实，是
//! 会变的事实——变的时候要接得住）。
//!
//! 判据原文（施工书锚点 CGPU-F0967）：更新应对：NVIDIA 驱动更新→重认证联
//! 动（F0807 触发规则 NVIDIA 落地）；更新预检（差异表预检+升级建议）；更新
//! 告知。判据：联动、预检、告知、判据。
//!
//! 职责定位：更新事件接收器（版本号进出+变更类型判定）+ 重认证联动器（按
//! F0807 触发规则落地 NVIDIA 语义：GameReady 与 Studio 双线各自触发——与
//! cgg03_nvdifftable 双驱动线对齐；产出受影响场景闭集的增量重认证计划）+
//! 更新预检器（新版本 vs 当前版本差异表预检——已知症状/规避开关是否失效+
//! 升级/暂缓建议与依据）+ 告知面（预检结果/联动计划/建议单一次发布）。红线：
//! 预检只能基于差异表已知事实，不确定处显性"未知"不猜（诚实纪律）；暂缓建
//! 议必须给理由，不给"建议暂缓"四个字了事。诊断码段 0x5701~0x5705。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const NV_UPDATE_VERSION: &str = "G07-nvupdate-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x57 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_VERSION_FORMAT_BAD: u32 = 0x5701; // 版本号格式非法（三段点分整数）
pub const XC_UPDATE_SAME_VERSION: u32 = 0x5702; // 同版本"更新"（无变更——拒绝空事件）
pub const XC_PREFLIGHT_UNKNOWN_SYMBOL: u32 = 0x5703; // 预检遇到差异表外符号（显性未知不猜）
pub const XC_TRIGGER_NO_PLAN: u32 = 0x5704; // 联动触发但无计划产出（联动失守）
pub const XC_ADVICE_NO_REASON: u32 = 0x5705; // 建议无依据（暂缓不带理由=空话违约）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 版本三段上限（major.minor.rev）。
pub const VER_SEGMENTS: usize = 3;
/// 受影响场景闭集上限（重认证计划容量）。
pub const SCENARIO_CAP: usize = 16;

/// NVIDIA 双驱动线（与 cgg03_nvdifftable 对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NvDriverLine {
    GameReady,
    Studio,
}

impl NvDriverLine {
    pub const ALL: [NvDriverLine; 2] = [NvDriverLine::GameReady, NvDriverLine::Studio];

    pub const fn zh(self) -> &'static str {
        match self {
            NvDriverLine::GameReady => "GameReady",
            NvDriverLine::Studio => "Studio",
        }
    }
}

/// 变更类型（F0807 触发规则的 NVIDIA 落地闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// 大版本（major 段变——重认证全量触发）。
    MajorBump,
    /// 小版本（minor/rev 变——受影响场景增量触发）。
    MinorBump,
    /// 无变更（拒收空事件）。
    None,
}

/// 三段版本号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NvVersion {
    pub major: u32,
    pub minor: u32,
    pub rev: u32,
}

impl NvVersion {
    /// 解析 "566.36.03" 式三段点分。
    pub fn parse(s: &str) -> Result<NvVersion, u32> {
        let mut segs = [0u32; VER_SEGMENTS];
        let bytes = s.as_bytes();
        let mut seg = 0usize;
        let mut acc: u32 = 0;
        let mut got = false;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'.' {
                if seg >= VER_SEGMENTS - 1 {
                    return Err(XC_VERSION_FORMAT_BAD);
                }
                segs[seg] = acc;
                seg += 1;
                acc = 0;
                got = false;
            } else if b >= b'0' && b <= b'9' {
                acc = acc.wrapping_mul(10).wrapping_add((b - b'0') as u32);
                got = true;
            } else {
                return Err(XC_VERSION_FORMAT_BAD);
            }
            i += 1;
        }
        if !got {
            return Err(XC_VERSION_FORMAT_BAD);
        }
        segs[seg] = acc;
        Ok(NvVersion {
            major: segs[0],
            minor: segs[1],
            rev: segs[2],
        })
    }

    /// 与旧版本比较得出变更类型。
    pub const fn diff_kind(&self, old: &NvVersion) -> ChangeKind {
        if self.major != old.major {
            ChangeKind::MajorBump
        } else if self.minor != old.minor || self.rev != old.rev {
            ChangeKind::MinorBump
        } else {
            ChangeKind::None
        }
    }

    /// 降序（新版本 > 旧版本）。
    pub const fn newer_than(&self, old: &NvVersion) -> bool {
        if self.major != old.major {
            return self.major > old.major;
        }
        if self.minor != old.minor {
            return self.minor > old.minor;
        }
        self.rev > old.rev
    }
}

/// 差异表条目（预检输入——从 cgg03_nvdifftable 导入的已知事实）。
#[derive(Clone)]
pub struct DiffEntry {
    /// 驱动线。
    pub line: NvDriverLine,
    /// 生效版本下限（含）。
    pub since: NvVersion,
    /// 症状描述。
    pub symptom: String,
    /// 规避开关名（空串=无规避——只能等修复）。
    pub workaround: String,
}

/// 预检结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advice {
    /// 建议升级（无已知阻断）。
    Upgrade,
    /// 建议暂缓（有已知症状未规避——必须附理由）。
    HoldReason,
    /// 未知（差异表覆盖不到该版本段——显性未知不猜）。
    Unknown,
}

/// 预检报告。
#[derive(Clone, PartialEq)]
pub struct PreflightReport {
    pub line: NvDriverLine,
    pub old_v: NvVersion,
    pub new_v: NvVersion,
    pub kind: ChangeKind,
    pub advice: Advice,
    /// 建议依据（空串+HoldReason = 违约 XC_ADVICE_NO_REASON）。
    pub reason: String,
    /// 新版本仍命中的差异条目（症状串表）。
    pub live_symptoms: Vec<String>,
}

/// 重认证计划（F0807 联动产物）。
#[derive(Clone, PartialEq)]
pub struct RecertPlan {
    pub line: NvDriverLine,
    pub kind: ChangeKind,
    /// 受影响场景闭集（MajorBump=全量；MinorBump=差异命中场景）。
    pub scenarios: Vec<String>,
    /// 全量触发标志。
    pub full_sweep: bool,
}

/// NVIDIA 更新应对器。
pub struct NvUpdateHandler {
    /// 差异表（预检知识库）。
    table: Vec<DiffEntry>,
    /// 各线当前版本。
    cur: [Option<NvVersion>; 2],
    /// 遥测。
    pub update_events: u64,
    pub plans_issued: u64,
    pub preflights: u64,
    pub unknown_advice: u64,
}

impl NvUpdateHandler {
    pub fn new() -> Self {
        NvUpdateHandler {
            table: Vec::new(),
            cur: [None, None],
            update_events: 0,
            plans_issued: 0,
            preflights: 0,
            unknown_advice: 0,
        }
    }

    /// 导入差异条目（知识库只增）。
    pub fn import_diff(&mut self, e: DiffEntry) {
        self.table.push(e);
    }

    /// 当前版本登记（基线）。
    pub fn set_current(&mut self, line: NvDriverLine, v: NvVersion) {
        self.cur[line as usize] = Some(v);
    }

    /// 更新事件全流程：接收→判定→预检→联动计划→告知。
    pub fn handle_update(
        &mut self,
        line: NvDriverLine,
        new_v: NvVersion,
    ) -> Result<(PreflightReport, RecertPlan), u32> {
        self.update_events += 1;
        let old = match self.cur[line as usize] {
            Some(v) => v,
            None => return Err(XC_VERSION_FORMAT_BAD), // 无基线不能判变更
        };
        let kind = new_v.diff_kind(&old);
        if kind == ChangeKind::None {
            return Err(XC_UPDATE_SAME_VERSION);
        }
        if !new_v.newer_than(&old) {
            // 降版不是"更新"——回滚走另一套流程，此处拒绝。
            return Err(XC_UPDATE_SAME_VERSION);
        }
        let report = self.preflight(line, old, new_v, kind)?;
        let plan = self.recert_plan(line, kind, &report)?;
        // 告知：基线推进（应对完成）。
        self.cur[line as usize] = Some(new_v);
        Ok((report, plan))
    }

    /// 更新预检（判据"差异表预检+升级建议"）。
    pub fn preflight(
        &mut self,
        line: NvDriverLine,
        old_v: NvVersion,
        new_v: NvVersion,
        kind: ChangeKind,
    ) -> Result<PreflightReport, u32> {
        self.preflights += 1;
        let mut live_symptoms = Vec::new();
        let mut known_hit = false;
        let mut unknown = false;
        let mut i = 0;
        while i < self.table.len() {
            let e = &self.table[i];
            if e.line == line && version_ge(new_v, e.since) {
                known_hit = true;
                // 规避开关存在 → 已知可控；无规避 → 阻断性症状。
                if e.workaround.len() == 0 {
                    live_symptoms.push(e.symptom.clone());
                } else {
                    live_symptoms.push(alloc::format!(
                        "{}（规避：{}）",
                        e.symptom,
                        e.workaround
                    ));
                }
            }
            i += 1;
        }
        // MajorBump 且差异表从未见过该 major 段 → 显性未知。
        if kind == ChangeKind::MajorBump && !major_known(line, new_v.major, &self.table) {
            unknown = true;
        }
        let (advice, reason) = if unknown {
            self.unknown_advice += 1;
            (
                Advice::Unknown,
                String::from("差异表未覆盖该大版本段——建议等待差异表更新后复检"),
            )
        } else if live_symptoms.len() > 0 && has_unmitigated(&live_symptoms) {
            (
                Advice::HoldReason,
                String::from("存在无规避开关的已知症状——暂缓升级等待驱动修复"),
            )
        } else if known_hit {
            (
                Advice::Upgrade,
                String::from("已知症状均有规避开关——可升级并保持开关生效"),
            )
        } else {
            (Advice::Upgrade, String::from("差异表无已知症状命中——可升级"))
        };
        if advice == Advice::HoldReason && reason.len() == 0 {
            return Err(XC_ADVICE_NO_REASON);
        }
        Ok(PreflightReport {
            line,
            old_v,
            new_v,
            kind,
            advice,
            reason,
            live_symptoms,
        })
    }

    /// 重认证联动（判据"F0807 触发规则 NVIDIA 落地"）。
    pub fn recert_plan(
        &mut self,
        line: NvDriverLine,
        kind: ChangeKind,
        report: &PreflightReport,
    ) -> Result<RecertPlan, u32> {
        let mut plan = RecertPlan {
            line,
            kind,
            scenarios: Vec::new(),
            full_sweep: kind == ChangeKind::MajorBump,
        };
        if plan.full_sweep {
            // 全量触发：受影响场景=全闭集占位（调用方按认证场景表填充）。
            plan.scenarios.push(String::from("all_scenarios"));
        } else {
            // 增量触发：差异命中条目逐条入计划（只重测受影响项——F0807）。
            let mut i = 0;
            while i < report.live_symptoms.len() {
                plan.scenarios.push(report.live_symptoms[i].clone());
                i += 1;
            }
        }
        if plan.scenarios.len() > SCENARIO_CAP {
            plan.scenarios.truncate(SCENARIO_CAP);
        }
        if plan.scenarios.len() == 0 {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        self.plans_issued += 1;
        Ok(plan)
    }

    /// 告知面：当前各线基线版本快照（判据"更新告知"）。
    pub fn bulletin(&self) -> [Option<NvVersion>; 2] {
        self.cur
    }
}

/// 版本 ≥ 判定。
pub const fn version_ge(a: NvVersion, b: NvVersion) -> bool {
    if a.major != b.major {
        return a.major > b.major;
    }
    if a.minor != b.minor {
        return a.minor > b.minor;
    }
    a.rev >= b.rev
}

/// 大版本是否在差异表出现过。
fn major_known(line: NvDriverLine, major: u32, table: &Vec<DiffEntry>) -> bool {
    let mut i = 0;
    while i < table.len() {
        if table[i].line == line && table[i].since.major == major {
            return true;
        }
        i += 1;
    }
    false
}

/// 症状列表中是否存在无"规避："标注的阻断项。
fn has_unmitigated(symptoms: &Vec<String>) -> bool {
    let mut i = 0;
    while i < symptoms.len() {
        if !contains_str(&symptoms[i], "规避：") {
            return true;
        }
        i += 1;
    }
    false
}

/// ASCII 子串包含。
pub fn contains_str(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.len() == 0 || hb.len() < nb.len() {
        return nb.len() == 0;
    }
    let mut i = 0;
    while i + nb.len() <= hb.len() {
        let mut m = true;
        let mut j = 0;
        while j < nb.len() {
            if hb[i + j] != nb[j] {
                m = false;
                break;
            }
            j += 1;
        }
        if m {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 自检（判据四组：联动 / 预检 / 告知 / 版本解析）
// ---------------------------------------------------------------------------

impl NvUpdateHandler {
    /// 组一：版本解析——合法三段解析、非法格式拒、变更类型判定。
    pub fn selfcheck_version(&mut self) -> Result<(), u32> {
        let v = NvVersion::parse("566.36.03")?;
        if v.major != 566 || v.minor != 36 || v.rev != 3 {
            return Err(XC_VERSION_FORMAT_BAD);
        }
        if NvVersion::parse("566.36").is_ok() {
            return Err(XC_VERSION_FORMAT_BAD); // 两段不足
        }
        if NvVersion::parse("a.b.c").is_ok() {
            return Err(XC_VERSION_FORMAT_BAD);
        }
        let old = NvVersion { major: 565, minor: 90, rev: 1 };
        if v.diff_kind(&old) != ChangeKind::MajorBump {
            return Err(XC_VERSION_FORMAT_BAD);
        }
        let old2 = NvVersion { major: 566, minor: 35, rev: 99 };
        if v.diff_kind(&old2) != ChangeKind::MinorBump {
            return Err(XC_VERSION_FORMAT_BAD);
        }
        Ok(())
    }

    /// 组二：联动——MajorBump 全量计划、MinorBump 增量计划、同版本拒。
    pub fn selfcheck_recert(&mut self) -> Result<(), u32> {
        let mut h = NvUpdateHandler::new();
        h.set_current(
            NvDriverLine::GameReady,
            NvVersion { major: 565, minor: 90, rev: 1 },
        );
        // MajorBump → 全量。
        let (r1, p1) = h.handle_update(
            NvDriverLine::GameReady,
            NvVersion::parse("566.36.03")?,
        )?;
        if !p1.full_sweep || p1.scenarios[0] != "all_scenarios" {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        if r1.kind != ChangeKind::MajorBump {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        // 基线已推进：再喂 MinorBump → 增量。
        h.import_diff(DiffEntry {
            line: NvDriverLine::GameReady,
            since: NvVersion::parse("566.0.0")?,
            symptom: String::from("网页字体渲染发虚"),
            workaround: String::from("nv_clear_type_tune=1"),
        });
        let (r2, p2) = h.handle_update(
            NvDriverLine::GameReady,
            NvVersion::parse("566.40.00")?,
        )?;
        if p2.full_sweep || p2.scenarios.len() != 1 {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        if r2.kind != ChangeKind::MinorBump {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        // 同版本拒。
        let r3 = h.handle_update(
            NvDriverLine::GameReady,
            NvVersion::parse("566.40.00")?,
        );
        if r3 != Err(XC_UPDATE_SAME_VERSION) {
            return Err(XC_UPDATE_SAME_VERSION);
        }
        // 降版拒。
        let r4 = h.handle_update(
            NvDriverLine::GameReady,
            NvVersion::parse("566.30.00")?,
        );
        if r4 != Err(XC_UPDATE_SAME_VERSION) {
            return Err(XC_UPDATE_SAME_VERSION);
        }
        Ok(())
    }

    /// 组三：预检——无规避症状→暂缓带理由、有规避→升级、未知段→Unknown。
    pub fn selfcheck_preflight(&mut self) -> Result<(), u32> {
        let mut h = NvUpdateHandler::new();
        h.set_current(
            NvDriverLine::Studio,
            NvVersion { major: 555, minor: 1, rev: 0 },
        );
        // 无规避阻断症状。
        h.import_diff(DiffEntry {
            line: NvDriverLine::Studio,
            since: NvVersion::parse("556.0.0")?,
            symptom: String::from("CUDA 上下文崩溃"),
            workaround: String::from(""),
        });
        let (r1, _) = h.handle_update(
            NvDriverLine::Studio,
            NvVersion::parse("556.10.00")?,
        )?;
        if r1.advice != Advice::HoldReason || r1.reason.len() == 0 {
            return Err(XC_ADVICE_NO_REASON);
        }
        // 有规避 → 升级。
        let mut h2 = NvUpdateHandler::new();
        h2.set_current(
            NvDriverLine::Studio,
            NvVersion { major: 555, minor: 1, rev: 0 },
        );
        h2.import_diff(DiffEntry {
            line: NvDriverLine::Studio,
            since: NvVersion::parse("556.0.0")?,
            symptom: String::from("字形发虚"),
            workaround: String::from("nv_ct_tune=1"),
        });
        let (r2, _) = h2.handle_update(
            NvDriverLine::Studio,
            NvVersion::parse("556.10.00")?,
        )?;
        if r2.advice != Advice::Upgrade {
            return Err(XC_ADVICE_NO_REASON);
        }
        // 未知大版本段 → Unknown。
        let (r3, _) = h2.handle_update(
            NvDriverLine::Studio,
            NvVersion::parse("600.0.0")?,
        )?;
        if r3.advice != Advice::Unknown || h2.unknown_advice < 1 {
            return Err(XC_PREFLIGHT_UNKNOWN_SYMBOL);
        }
        Ok(())
    }

    /// 组四：告知——基线推进与双线独立。
    pub fn selfcheck_bulletin(&mut self) -> Result<(), u32> {
        let mut h = NvUpdateHandler::new();
        h.set_current(
            NvDriverLine::GameReady,
            NvVersion { major: 560, minor: 0, rev: 0 },
        );
        h.set_current(
            NvDriverLine::Studio,
            NvVersion { major: 555, minor: 0, rev: 0 },
        );
        let _ = h.handle_update(
            NvDriverLine::GameReady,
            NvVersion::parse("561.0.0")?,
        )?;
        let b = h.bulletin();
        if b[0].unwrap_or(NvVersion { major: 0, minor: 0, rev: 0 }).major != 561 {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        // Studio 未动。
        if b[1].unwrap_or(NvVersion { major: 0, minor: 0, rev: 0 }).major != 555 {
            return Err(XC_TRIGGER_NO_PLAN);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_version()?;
        self.selfcheck_recert()?;
        self.selfcheck_preflight()?;
        self.selfcheck_bulletin()?;
        Ok(())
    }
}
