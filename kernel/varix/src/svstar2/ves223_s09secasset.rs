//! VE-F3969 · 安全资产汇总
//!
//! S 域安全资产汇总：八组安全断言/红线/审计项归集。汇总规则：每资产挂
//! 组/挂条/挂红线三指针。专项面三面复审：读屏语义注入面/色标配置注入面/
//! 工具插件面。
//! 错误路径：红线缺→立案补立；注入面未覆盖→补测；审计缺→回补。
//! 性能：归集 O(资产)；复审 O(面)；补测 O(缺项)。
//! 跨批对接：八组安全条目；F3964 上游；F3985 安全终审下游。
//! 无障碍与隐私：红线含无障碍配置不越权改用户偏好（隐私红线复述）；无
//! 新增隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3380）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3380;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9ICode(pub u16);

impl S9ICode {
    pub const OK: Self = Self(SEG_BASE);
    pub const REDLINE_MISSING: Self = Self(SEG_BASE + 1);
    pub const FACE_UNCOVERED: Self = Self(SEG_BASE + 2);
    pub const AUDIT_GAP: Self = Self(SEG_BASE + 3);
    pub const PREF_OVERWRITE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::REDLINE_MISSING => "红线缺→立案补立",
            Self::FACE_UNCOVERED => "注入面未覆盖→补测",
            Self::AUDIT_GAP => "审计缺→回补",
            Self::PREF_OVERWRITE => "越权改偏好→P0",
            _ => "未知",
        }
    }
}

// ── 安全资产条目 ─────────────────────────────────────────────────────

/// 资产种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecKind {
    /// 安全断言。
    Assert,
    /// 红线。
    Redline,
    /// 审计项。
    Audit,
}

pub const SEC_KINDS: [SecKind; 3] = [SecKind::Assert, SecKind::Redline, SecKind::Audit];

/// 安全资产行。
#[derive(Debug, Clone)]
pub struct SecRow {
    pub name: &'static str,
    pub kind: SecKind,
    /// 三指针：组 / 条 / 红线。
    pub group: u8,
    pub clause: u32,
    pub redline_ref: u32,
    /// 偏好越权红线锚（此资产是否校验偏好不可越权）。
    pub guards_pref: bool,
}

impl SecRow {
    pub fn pointers_ok(&self) -> bool {
        self.group >= 1 && self.group <= 8 && self.clause > 0 && self.redline_ref > 0
    }
}

/// 资产台账。
#[derive(Debug, Clone)]
pub struct SecLedger {
    pub rows: Vec<SecRow>,
}

impl SecLedger {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn admit(&mut self, r: SecRow) -> S9ICode {
        if !r.pointers_ok() {
            return S9ICode::REDLINE_MISSING;
        }
        self.rows.push(r);
        S9ICode::OK
    }

    /// 三种别覆盖缺口。
    pub fn kind_gaps(&self) -> Vec<SecKind> {
        SEC_KINDS.iter().filter(|k| !self.rows.iter().any(|r| r.kind == **k)).map(|k| *k).collect()
    }

    /// 审计缺（审计项数为 0）→ 回补。
    pub fn audit_count(&self) -> usize {
        self.rows.iter().filter(|r| r.kind == SecKind::Audit).count()
    }

    /// 偏好越权红线在册断言。
    pub fn pref_guarded(&self) -> bool {
        self.rows.iter().any(|r| r.guards_pref && r.kind == SecKind::Redline)
    }
}

impl Default for SecLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三面复审 ─────────────────────────────────────────────────────────

/// 注入面三面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackFace {
    /// 读屏语义注入。
    SpeakInject,
    /// 色标配置注入。
    ColorInject,
    /// 工具插件面。
    PluginFace,
}

pub const FACES: [AttackFace; 3] = [AttackFace::SpeakInject, AttackFace::ColorInject, AttackFace::PluginFace];

/// 面复审记录：面 → 用例数 → 是否覆盖。
#[derive(Debug, Clone)]
pub struct FaceReview {
    pub face: AttackFace,
    /// 覆盖用例数（0 = 未覆盖）。
    pub cases: u32,
}

/// 三面复审器。
#[derive(Debug, Clone)]
pub struct FaceAuditor {
    pub rows: Vec<FaceReview>,
}

impl FaceAuditor {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn record(&mut self, face: AttackFace, cases: u32) {
        match self.rows.iter_mut().find(|r| r.face == face) {
            Some(r) => r.cases = cases,
            None => self.rows.push(FaceReview { face, cases }),
        }
    }

    /// 注入面未覆盖清单。
    pub fn uncovered(&self) -> Vec<AttackFace> {
        FACES.iter().filter(|f| match self.rows.iter().find(|r| r.face == **f) {
            Some(r) => r.cases == 0,
            None => true,
        }).map(|f| *f).collect()
    }

    /// 补测：给未覆盖面登记用例。
    pub fn patch(&mut self, face: AttackFace, cases: u32) {
        self.record(face, cases);
    }

    pub fn all_covered(&self) -> bool {
        self.uncovered().is_empty() && self.rows.len() >= 3
    }
}

impl Default for FaceAuditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 越权偏好写入专项断言 ─────────────────────────────────────────────

/// 偏好写入请求：调用方 × 目标偏好。
#[derive(Debug, Clone)]
pub struct PrefWrite {
    pub caller: &'static str,
    pub pref: &'static str,
    /// 调用方是否有该偏好的写权限。
    pub authorized: bool,
}

/// 越权检出：未授权写入名单（P0）。
pub fn pref_violations(writes: &[PrefWrite]) -> Vec<&'static str> {
    writes.iter().filter(|w| !w.authorized).map(|w| w.caller).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves223_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves223_s09secasset");

    fn row(name: &'static str, kind: SecKind, group: u8) -> SecRow {
        SecRow { name, kind, group, clause: 3, redline_ref: 8, guards_pref: false }
    }

    // 01 三种别守恒
    {
        chk(&mut set, "C3969-01", SEC_KINDS.len() == 3 && SecKind::Assert as usize == 0 && SecKind::Audit as usize == 2, "err");
    }
    // 02 三种别全归集
    {
        let mut l = SecLedger::new();
        for (i, k) in SEC_KINDS.iter().enumerate() {
            l.admit(row("资产", *k, (i + 1) as u8));
        }
        chk(&mut set, "C3969-02", l.kind_gaps().is_empty() && l.audit_count() == 1, "err");
    }
    // 03 红线指针缺拒收
    {
        let mut l = SecLedger::new();
        let mut r = row("缺红线", SecKind::Redline, 1);
        r.redline_ref = 0;
        chk(&mut set, "C3969-03", l.admit(r) == S9ICode::REDLINE_MISSING, "err");
    }
    // 04 组号越界拒收
    {
        let mut l = SecLedger::new();
        chk(&mut set, "C3969-04", l.admit(row("越界", SecKind::Assert, 9)) == S9ICode::REDLINE_MISSING, "err");
    }
    // 05 审计缺→回补判定
    {
        let mut l = SecLedger::new();
        l.admit(row("断言", SecKind::Assert, 1));
        l.admit(row("红线", SecKind::Redline, 2));
        chk(&mut set, "C3969-05", l.audit_count() == 0 && l.kind_gaps() == vec![SecKind::Audit], "err");
    }
    // 06 三面清单守恒
    {
        chk(&mut set, "C3969-06", FACES.len() == 3 && AttackFace::SpeakInject as usize == 0 && AttackFace::PluginFace as usize == 2, "err");
    }
    // 07 注入面未覆盖检出
    {
        let mut a = FaceAuditor::new();
        a.record(AttackFace::SpeakInject, 12);
        a.record(AttackFace::ColorInject, 5);
        chk(&mut set, "C3969-07", a.uncovered() == vec![AttackFace::PluginFace] && !a.all_covered(), "err");
    }
    // 08 补测闭环
    {
        let mut a = FaceAuditor::new();
        for f in FACES.iter() {
            a.patch(*f, 8);
        }
        chk(&mut set, "C3969-08", a.all_covered() && a.uncovered().is_empty(), "err");
    }
    // 09 覆盖回零 = 未覆盖
    {
        let mut a = FaceAuditor::new();
        a.record(AttackFace::ColorInject, 0);
        chk(&mut set, "C3969-09", a.uncovered() == vec![AttackFace::ColorInject], "err");
    }
    // 10 越权偏好写入 P0 检出
    {
        let writes = vec![
            PrefWrite { caller: "内核策略", pref: "减弱动效", authorized: true },
            PrefWrite { caller: "第三方插件", pref: "色标", authorized: false },
        ];
        chk(&mut set, "C3969-10", pref_violations(&writes) == vec!["第三方插件"], "err");
    }
    // 11 授权写入零违例
    {
        let writes = vec![PrefWrite { caller: "用户面板", pref: "朗读语速", authorized: true }];
        chk(&mut set, "C3969-11", pref_violations(&writes).is_empty(), "err");
    }
    // 12 偏好越权红线在册断言
    {
        let mut l = SecLedger::new();
        let mut r = row("偏好红线", SecKind::Redline, 5);
        r.guards_pref = true;
        l.admit(r);
        chk(&mut set, "C3969-12", l.pref_guarded(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3969-13", S9ICode::PREF_OVERWRITE.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9ICode::OK, S9ICode::REDLINE_MISSING, S9ICode::FACE_UNCOVERED, S9ICode::AUDIT_GAP, S9ICode::PREF_OVERWRITE];
        chk(&mut set, "C3969-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_secasset_all_green() {
        let s = run_ves223_checks();
        assert!(s.all_passed(), "ves223 red: {}", s.red_items_text());
    }

    #[test]
    fn test_face_upsert() {
        let mut a = FaceAuditor::new();
        a.record(AttackFace::SpeakInject, 3);
        a.record(AttackFace::SpeakInject, 9);
        assert_eq!(a.rows.len(), 1);
        assert_eq!(a.rows[0].cases, 9);
    }

    #[test]
    fn test_pref_redline_full() {
        let writes = [
            PrefWrite { caller: "a", pref: "p", authorized: true },
            PrefWrite { caller: "b", pref: "p", authorized: true },
        ];
        assert!(pref_violations(&writes).is_empty());
    }
}
