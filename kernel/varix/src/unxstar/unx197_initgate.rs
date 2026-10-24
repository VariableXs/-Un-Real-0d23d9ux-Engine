//! unx197_initgate — UNX-F0197 init 自检清单（健康门禁）（0x2B80 段）。
//!
//! 锚点（CoRun Unxreal 施工书 UNX-F0197）：init 自检清单（健康门禁）。
//! 判据原文（UNX-F0197-J1）：全单元自检聚合报告生成，任何 FAIL 阻断
//! 进入 RUNTIME 并显性化。
//!
//! 设计三线：①自检清单账（init 各单元的 PASS/WARN/FAIL 三态自检结果
//! 逐项落账——WARN 不阻断但显性化、FAIL 即阻断）；②聚合报告生成
//! （单元数/通过数/告警数/失败数四账平 + 报告指纹可复现）；③健康
//! 门禁（FAIL 存在 → 阻断进入 RUNTIME 且阻断原因逐条显性化——
//! 静默放行是本域最高红线）。

use alloc::format;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const INITGATE_VERSION: &str = "197-initgate-v1";

/// 段基（0x2B80）。
pub const SEG_BASE: u16 = 0x2B80;

/// 自检三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Pass,
    Warn,
    Fail,
}

impl Health {
    /// 是否阻断。
    pub fn blocks(self) -> bool {
        self == Health::Fail
    }
    /// 字面量（报告指纹输入）。
    pub fn tag(self) -> &'static str {
        match self {
            Health::Pass => "PASS",
            Health::Warn => "WARN",
            Health::Fail => "FAIL",
        }
    }
}

/// 自检条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRecord {
    /// 单元名。
    pub unit: &'static str,
    pub health: Health,
    /// 明细（FAIL 必须非空——显性化红线）。
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 自检清单账
// ---------------------------------------------------------------------------

/// 清单账。
#[derive(Debug, Default)]
pub struct Checklist {
    pub recs: Vec<CheckRecord>,
}

impl Checklist {
    /// 落账：FAIL 无明细拒（显性化红线在入口处）。
    pub fn record(&mut self, unit: &'static str, h: Health, detail: &str) -> Result<(), u16> {
        if h.blocks() && detail.is_empty() {
            return Err(E_FAIL_NO_DETAIL);
        }
        if self.recs.iter().any(|r| r.unit == unit) {
            return Err(E_DUP_UNIT);
        }
        self.recs.push(CheckRecord {
            unit,
            health: h,
            detail: String::from(detail),
        });
        Ok(())
    }

    /// 单元级覆写：重检后更新状态（同一单元只留最新态，保留重检轨迹）。
    pub fn override_result(&mut self, unit: &'static str, h: Health, detail: &str) -> Result<(), u16> {
        if h.blocks() && detail.is_empty() {
            return Err(E_FAIL_NO_DETAIL);
        }
        let r = self
            .recs
            .iter_mut()
            .find(|r| r.unit == unit)
            .ok_or(E_NO_UNIT)?;
        r.health = h;
        r.detail = String::from(detail);
        Ok(())
    }

    /// 四账：(总数, PASS, WARN, FAIL)。
    pub fn tally(&self) -> (usize, usize, usize, usize) {
        let p = self.recs.iter().filter(|r| r.health == Health::Pass).count();
        let w = self.recs.iter().filter(|r| r.health == Health::Warn).count();
        let f = self.recs.iter().filter(|r| r.health == Health::Fail).count();
        (self.recs.len(), p, w, f)
    }

    /// 聚合报告（判据主体）：每单元一行 "unit=TAG"，FAIL 行附明细。
    /// 同输入必同报告（指纹可复现红线）。
    pub fn report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in self.recs.iter() {
            let mut line = alloc::format!("{}={}", r.unit, r.health.tag());
            if r.health.blocks() {
                line.push('(');
                line.push_str(&r.detail);
                line.push(')');
            }
            out.push(line);
        }
        out
    }

    /// 报告指纹：行依序 FNV-1a（可复现性对拍锚）。
    pub fn report_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for line in self.report() {
            for b in line.as_bytes() {
                h = (h ^ (*b as u64)).wrapping_mul(0x0000_0100_0000_01B3);
            }
            h = (h ^ (b'\n' as u64)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 健康门禁
// ---------------------------------------------------------------------------

/// 门禁裁决。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateVerdict {
    /// 是否放行进入 RUNTIME。
    pub allow: bool,
    /// 阻断原因清单（FAIL 单元逐条，显性化）。
    pub blockers: Vec<String>,
    /// 告警清单（不阻断但显性化）。
    pub warnings: Vec<String>,
}

/// 门禁判定（J1 主体）：FAIL 阻断 + 阻断原因逐条显性。
pub fn health_gate(list: &Checklist) -> GateVerdict {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    for r in list.recs.iter() {
        match r.health {
            Health::Fail => blockers.push(alloc::format!("{}:{}", r.unit, r.detail)),
            Health::Warn => warnings.push(String::from(r.unit)),
            Health::Pass => {}
        }
    }
    GateVerdict {
        allow: blockers.is_empty(),
        blockers,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// 错误码（0x2B80 段，显式判别式）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum InitGateErr {
    /// FAIL 无明细。
    FailNoDetail = SEG_BASE + 1,
    /// 单元重复落账。
    DupUnit = SEG_BASE + 2,
    /// 覆写未知单元。
    NoUnit = SEG_BASE + 3,
    /// 四账不平。
    TallyBroken = SEG_BASE + 4,
    /// 报告指纹漂移。
    FpDrift = SEG_BASE + 5,
    /// FAIL 被静默放行。
    SilentAllow = SEG_BASE + 6,
}

impl InitGateErr {
    pub const fn code(self) -> u16 {
        self as u16
    }
    pub fn detail(self) -> &'static str {
        match self {
            InitGateErr::FailNoDetail => "FAIL 无明细（显性化红线）",
            InitGateErr::DupUnit => "单元重复落账",
            InitGateErr::NoUnit => "覆写未知单元",
            InitGateErr::TallyBroken => "四账不平",
            InitGateErr::FpDrift => "同输入报告指纹漂移",
            InitGateErr::SilentAllow => "FAIL 被静默放行",
        }
    }
}

const E_FAIL_NO_DETAIL: u16 = InitGateErr::FailNoDetail.code();
const E_DUP_UNIT: u16 = InitGateErr::DupUnit.code();
const E_NO_UNIT: u16 = InitGateErr::NoUnit.code();
const E_TALLY_BROKEN: u16 = InitGateErr::TallyBroken.code();
const E_FP_DRIFT: u16 = InitGateErr::FpDrift.code();
const E_SILENT_ALLOW: u16 = InitGateErr::SilentAllow.code();

// ---------------------------------------------------------------------------
// 判据（自证）
// ---------------------------------------------------------------------------

/// 判据夹具：七单元健康清单（一 WARN 一 FAIL）。
fn fixture_list() -> Result<Checklist, u16> {
    let mut l = Checklist::default();
    l.record("cpu", Health::Pass, "")?;
    l.record("mtrr", Health::Pass, "")?;
    l.record("apic", Health::Pass, "")?;
    l.record("pcpu", Health::Warn, "size-uneven")?;
    l.record("gdt", Health::Pass, "")?;
    l.record("tss", Health::Fail, "IST1 未配置")?;
    l.record("ucode", Health::Pass, "")?;
    Ok(l)
}

/// 判据集（UNX-F0197-J1 + 结构红线）。自证七段。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：聚合报告生成 + FAIL 阻断 + 阻断原因显性。
    let list = fixture_list()?;
    let (n, p, w, f) = list.tally();
    if n != 7 || p != 5 || w != 1 || f != 1 {
        return Err(E_TALLY_BROKEN);
    }
    let gate = health_gate(&list);
    if gate.allow || gate.blockers.len() != 1 || !gate.blockers[0].contains("tss") {
        return Err(E_SILENT_ALLOW);
    }
    if gate.warnings != vec![String::from("pcpu")] {
        return Err(E_TALLY_BROKEN);
    }
    // 二、报告指纹可复现：同清单两次指纹一致，状态变则指纹变。
    let fp1 = list.report_fingerprint();
    let fp2 = list.report_fingerprint();
    if fp1 != fp2 {
        return Err(E_FP_DRIFT);
    }
    let mut list2 = fixture_list()?;
    list2.override_result("tss", Health::Pass, "")?;
    if list2.report_fingerprint() == fp1 {
        return Err(E_FP_DRIFT);
    }
    // 三、重检放行：tss 修复后门禁放行、WARN 仍在（不阻断）。
    let gate2 = health_gate(&list2);
    if !gate2.allow || !gate2.blockers.is_empty() || gate2.warnings.len() != 1 {
        return Err(E_SILENT_ALLOW);
    }
    // 四、红线逐项：FAIL 无明细 / 重复单元 / 覆写未知单元 各自拒。
    let mut bad = Checklist::default();
    if bad.record("x", Health::Fail, "").is_ok() {
        return Err(E_FAIL_NO_DETAIL);
    }
    bad.record("y", Health::Pass, "")?;
    if bad.record("y", Health::Pass, "").is_ok() {
        return Err(E_DUP_UNIT);
    }
    if bad.override_result("zz", Health::Pass, "").is_ok() {
        return Err(E_NO_UNIT);
    }
    // 入口拦截复核：FAIL+空明细从未进账（显性化在入口兑现）。
    if bad.recs.iter().any(|r| r.health.blocks() && r.detail.is_empty()) {
        return Err(E_FAIL_NO_DETAIL);
    }
    // 五、报告行形：FAIL 行带括号明细、PASS 行无括号（显性化形态锚）。
    let rep = list.report();
    let tss_line = rep.iter().find(|l| l.starts_with("tss=")).ok_or(E_FP_DRIFT)?;
    if !tss_line.contains("(IST1 未配置)") {
        return Err(E_FP_DRIFT);
    }
    let cpu_line = rep.iter().find(|l| l.starts_with("cpu=")).ok_or(E_FP_DRIFT)?;
    if cpu_line.contains('(') {
        return Err(E_FP_DRIFT);
    }
    // 六、判别式段位对拍。
    if E_SILENT_ALLOW != SEG_BASE + 6 || E_TALLY_BROKEN != SEG_BASE + 4 {
        return Err(E_SILENT_ALLOW);
    }
    Ok(())
}
