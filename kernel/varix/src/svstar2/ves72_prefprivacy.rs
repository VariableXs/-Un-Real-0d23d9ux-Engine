//! VE-F3672 · 个性化隐私（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3672`
//!
//! **职责定位（锚点原文）**：个性化隐私——
//! - **推断风险**：个性化含隐私（布局暗示作息/内容偏好暗示兴趣——本地画像红线执法化）；
//! - **数据最小化**：只存必要项（过度收集 = P0 红线）；
//! - **匿名化**：遥测中的个性化数据匿名聚合（复述）；
//! - **用户控制**：查看/修改/清除/导出四权（入口可达断言——入口深埋 = 形式主义）。
//!
//! **判据（锚点原文）**：过度收集 P0、四权可达、推断评估、匿名单源、本地画像、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 过度收集→P0（红线实测） | [`C72Code::OVER_COLLECT`] |
//! | 四权不可达→P1（红线实测） | [`C72Code::RIGHT_UNREACHABLE`] |
//! | 推断泄露→评估修正 | [`C72Code::INFER_LEAK`] |
//! | 匿名失效→单源 | [`C72Code::ANON_FAIL`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3672 诊断码段（独占 0x78xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C72Code(pub u16);

impl C72Code {
    /// 过度收集（P0 红线实测）。
    pub const OVER_COLLECT: C72Code = C72Code(0x7801);
    /// 四权不可达（P1 红线实测）。
    pub const RIGHT_UNREACHABLE: C72Code = C72Code(0x7802);
    /// 推断泄露。
    pub const INFER_LEAK: C72Code = C72Code(0x7803);
    /// 匿名失效。
    pub const ANON_FAIL: C72Code = C72Code(0x7804);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C72Code::OVER_COLLECT => "过度收集个性化数据（P0 红线实测）。建议：最小化收敛。",
            C72Code::RIGHT_UNREACHABLE => "四权入口不可达（P1 红线实测）。建议：入口深度 ≤2 跳。",
            C72Code::INFER_LEAK => "推断泄露。建议：评估修正。",
            C72Code::ANON_FAIL => "匿名失效。建议：单源匿名复用。",
            _ => "未知诊断码。",
        }
    }
}

// ── 最小化清单 ──────────────────────────────────────────────────────

/// 必要项（最小化清单：只存这些）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Necessary {
    /// 主题外观。
    Theme,
    /// 字号。
    FontSize,
    /// 语言。
    Lang,
    /// 无障碍项。
    A11y,
}

impl Necessary {
    pub const ALL: [Necessary; 4] = [
        Necessary::Theme,
        Necessary::FontSize,
        Necessary::Lang,
        Necessary::A11y,
    ];
}

/// 收集审计器（过度收集 = P0）。
pub struct CollectAudit {
    collected: Vec<(u16, bool)>,
    /// (键, is_necessary)
    over_hits: u32,
    last_code: C72Code,
    has_error: bool,
}

impl CollectAudit {
    pub fn new() -> Self {
        Self {
            collected: Vec::new(),
            over_hits: 0,
            last_code: C72Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C72Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 登记收集项。
    pub fn collect(&mut self, key: u16, necessary: bool) -> Result<bool, C72Code> {
        self.collected.push((key, necessary));
        if !necessary {
            self.over_hits += 1;
            self.last_code = C72Code::OVER_COLLECT;
            self.has_error = true;
            return Err(C72Code::OVER_COLLECT);
        }
        self.last_code = C72Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 清单审计：全部必要才绿。
    pub fn audit(&mut self) -> Result<bool, C72Code> {
        let all_ok = self.collected.iter().all(|(_, n)| *n);
        if all_ok {
            self.last_code = C72Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.over_hits += 1;
            self.last_code = C72Code::OVER_COLLECT;
            self.has_error = true;
            Err(C72Code::OVER_COLLECT)
        }
    }

    pub fn over_hits(&self) -> u32 {
        self.over_hits
    }

    pub fn collected_count(&self) -> usize {
        self.collected.len()
    }

    /// 移除不必要项（收敛动作）。
    pub fn minimize(&mut self) -> usize {
        let before = self.collected.len();
        self.collected.retain(|(_, n)| *n);
        before - self.collected.len()
    }
}

// ── 四权入口 ────────────────────────────────────────────────────────

/// 四权。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivacyRight {
    /// 查看。
    View,
    /// 修改。
    Modify,
    /// 清除。
    Erase,
    /// 导出。
    Export,
}

impl PrivacyRight {
    pub const ALL: [PrivacyRight; 4] = [
        PrivacyRight::View,
        PrivacyRight::Modify,
        PrivacyRight::Erase,
        PrivacyRight::Export,
    ];
}

/// 四权入口（可达断言：深度 ≤2 跳）。
pub struct RightsPortal {
    /// (权, 入口跳数)。
    hops: Vec<(PrivacyRight, u8)>,
    unreachable: u32,
    last_code: C72Code,
    has_error: bool,
}

impl RightsPortal {
    pub fn new() -> Self {
        Self {
            hops: Vec::new(),
            unreachable: 0,
            last_code: C72Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C72Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 登记一权的入口跳数。
    pub fn register(&mut self, right: PrivacyRight, hops: u8) {
        if let Some(e) = self.hops.iter_mut().find(|(r, _)| *r == right) {
            e.1 = hops;
        } else {
            self.hops.push((right, hops));
        }
    }

    /// 可达断言：四权全部注册且跳数 ≤2。
    pub fn assert_reachable(&mut self) -> Result<bool, C72Code> {
        let mut ok = true;
        for r in PrivacyRight::ALL {
            match self.hops.iter().find(|(x, _)| *x == r) {
                Some((_, h)) if *h <= 2 => {}
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            self.last_code = C72Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.unreachable += 1;
            self.last_code = C72Code::RIGHT_UNREACHABLE;
            self.has_error = true;
            Err(C72Code::RIGHT_UNREACHABLE)
        }
    }

    /// 四权齐备（不查跳数）。
    pub fn all_registered(&self) -> bool {
        PrivacyRight::ALL.iter().all(|r| self.hops.iter().any(|(x, _)| x == r))
    }

    pub fn unreachable(&self) -> u32 {
        self.unreachable
    }
}

// ── 匿名化 ──────────────────────────────────────────────────────────

/// 匿名化器（遥测聚合复述）。
pub struct Anonymizer {
    /// 原始样本是否含标识。
    raw_with_id: u32,
    /// 匿名后样本数。
    anon_samples: u32,
    fail_hits: u32,
    last_code: C72Code,
    has_error: bool,
}

impl Anonymizer {
    pub fn new() -> Self {
        Self {
            raw_with_id: 0,
            anon_samples: 0,
            fail_hits: 0,
            last_code: C72Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C72Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 送入遥测样本（含标识位）。
    pub fn ingest(&mut self, has_id: bool) {
        if has_id {
            self.raw_with_id += 1;
        }
        self.anon_samples += 1;
    }

    /// 匿名有效性：不得有标识外泄。
    pub fn assert_anon(&mut self) -> Result<bool, C72Code> {
        if self.raw_with_id == 0 {
            self.last_code = C72Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail_hits += 1;
            self.last_code = C72Code::ANON_FAIL;
            self.has_error = true;
            Err(C72Code::ANON_FAIL)
        }
    }

    pub fn fail_hits(&self) -> u32 {
        self.fail_hits
    }

    pub fn anon_samples(&self) -> u32 {
        self.anon_samples
    }
}

// ── 本地画像 ────────────────────────────────────────────────────────

/// 推断风险点。
#[derive(Clone, Copy)]
pub struct InferPoint {
    /// 布局/内容等来源面。
    pub surface: u8,
    /// 可推断敏感度（0-9）。
    pub sensitivity: u8,
    /// 是否已本地化（不上云）。
    pub local_only: bool,
}

/// 本地画像评估器（推断风险 + 本地红线）。
pub struct LocalProfile {
    points: Vec<InferPoint>,
    leaks: u32,
    last_code: C72Code,
    has_error: bool,
}

impl LocalProfile {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            leaks: 0,
            last_code: C72Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C72Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add_point(&mut self, p: InferPoint) {
        self.points.push(p);
    }

    /// 本地红线：高敏感点必须 local_only。
    pub fn audit_local(&mut self) -> Result<bool, C72Code> {
        let mut ok = true;
        for p in &self.points {
            if p.sensitivity >= 5 && !p.local_only {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C72Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.leaks += 1;
            self.last_code = C72Code::INFER_LEAK;
            self.has_error = true;
            Err(C72Code::INFER_LEAK)
        }
    }

    /// 修正动作：把非本地高敏感点改本地。
    pub fn localize_all(&mut self) -> usize {
        let mut n = 0;
        for p in &mut self.points {
            if p.sensitivity >= 5 && !p.local_only {
                p.local_only = true;
                n += 1;
            }
        }
        n
    }

    /// 风险评分：Σ(sensitivity) for non-local。
    pub fn risk_score(&self) -> u32 {
        self.points
            .iter()
            .filter(|p| !p.local_only)
            .map(|p| p.sensitivity as u32)
            .sum()
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves72_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves72_prefprivacy");
    {
        let mut a = CollectAudit::new();
        let res = a.collect(1, true);
        chk(&mut set, "C3672-01", res == Ok(true) && a.over_hits() == 0, "err");
    }
    {
        let mut a = CollectAudit::new();
        let res = a.collect(2, false);
        chk(&mut set, "C3672-02", res.err() == Some(C72Code::OVER_COLLECT), "err");
    }
    {
        let mut a = CollectAudit::new();
        a.collect(1, true);
        a.collect(2, false);
        let res = a.audit();
        chk(&mut set, "C3672-03", res.err() == Some(C72Code::OVER_COLLECT), "err");
    }
    {
        let mut a = CollectAudit::new();
        a.collect(1, true);
        a.collect(2, false);
        let n = a.minimize();
        chk(&mut set, "C3672-04", n == 1 && a.collected_count() == 1, "err");
    }
    {
        let mut p = RightsPortal::new();
        p.register(PrivacyRight::View, 1);
        p.register(PrivacyRight::Modify, 2);
        p.register(PrivacyRight::Erase, 1);
        p.register(PrivacyRight::Export, 2);
        let res = p.assert_reachable();
        chk(&mut set, "C3672-05", res == Ok(true), "err");
    }
    {
        let mut p = RightsPortal::new();
        p.register(PrivacyRight::View, 5);
        let res = p.assert_reachable();
        chk(&mut set, "C3672-06", res.err() == Some(C72Code::RIGHT_UNREACHABLE), "err");
    }
    {
        let mut p = RightsPortal::new();
        p.register(PrivacyRight::View, 1);
        chk(&mut set, "C3672-07", !p.all_registered(), "err");
    }
    {
        let mut n = Anonymizer::new();
        n.ingest(false);
        n.ingest(false);
        let res = n.assert_anon();
        chk(&mut set, "C3672-08", res == Ok(true) && n.anon_samples() == 2, "err");
    }
    {
        let mut n = Anonymizer::new();
        n.ingest(true);
        let res = n.assert_anon();
        chk(&mut set, "C3672-09", res.err() == Some(C72Code::ANON_FAIL), "err");
    }
    {
        let mut lp = LocalProfile::new();
        lp.add_point(InferPoint { surface: 1, sensitivity: 7, local_only: true });
        lp.add_point(InferPoint { surface: 2, sensitivity: 3, local_only: false });
        let res = lp.audit_local();
        chk(&mut set, "C3672-10", res == Ok(true), "err");
    }
    {
        let mut lp = LocalProfile::new();
        lp.add_point(InferPoint { surface: 1, sensitivity: 8, local_only: false });
        let res = lp.audit_local();
        chk(&mut set, "C3672-11", res.err() == Some(C72Code::INFER_LEAK), "err");
    }
    {
        let mut lp = LocalProfile::new();
        lp.add_point(InferPoint { surface: 1, sensitivity: 8, local_only: false });
        let n = lp.localize_all();
        chk(&mut set, "C3672-12", n == 1 && lp.risk_score() == 0, "err");
    }
    set
}
