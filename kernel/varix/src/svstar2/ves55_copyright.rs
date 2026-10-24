//! VE-F3655 · 视觉创作版权检查（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3655`
//!
//! **职责定位（锚点原文）**：版权检查（创作素材版权合规）——
//! - **素材溯源**：来源登记（来源登记红线：无来源素材不可发布——来源必填）；
//! - **特征比对**：已发布素材特征库比对（盗用检测，复用 F3612 单源）；
//! - **许可链**：素材许可链验证（商用许可红线：个人许可素材不可商用分发
//!   ——许可分级断言）；
//! - **误报流程**：误报申诉（复述误拦白名单精神；误报率 >5% → 校准）。
//!
//! **判据（锚点原文六条）**：来源必填、商用分级、误报门、
//! 申诉时限、单源复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 无来源发布→阻断（红线实测） | [`C55Code::NO_SOURCE`] |
//! | 商用违规→下架（分级断言实测） | [`C55Code::LICENSE_VIOLATION`] |
//! | 误报率>5%→特征库校准（误报红线） | [`C55Code::FALSE_POSITIVE`] |
//! | 申诉超时→时限（复述） | [`C55Code::APPEAL_TIMEOUT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3655 诊断码段（独占 0x67xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C55Code(pub u16);

impl C55Code {
    /// 无来源发布（阻断红线）。
    pub const NO_SOURCE: C55Code = C55Code(0x6701);
    /// 商用许可违规（下架红线）。
    pub const LICENSE_VIOLATION: C55Code = C55Code(0x6702);
    /// 误报率超 5%（校准红线）。
    pub const FALSE_POSITIVE: C55Code = C55Code(0x6703);
    /// 申诉超时。
    pub const APPEAL_TIMEOUT: C55Code = C55Code(0x6704);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C55Code::NO_SOURCE => "无来源素材发布（阻断红线实测）。建议：来源必填登记。",
            C55Code::LICENSE_VIOLATION => "商用许可违规（下架红线实测）。建议：按许可分级撤回。",
            C55Code::FALSE_POSITIVE => "误报率超 5%（校准红线）。建议：特征库校准。",
            C55Code::APPEAL_TIMEOUT => "申诉超时。建议：按申诉时限处置。",
            _ => "未知诊断码。",
        }
    }
}

// ── 许可分级 ────────────────────────────────────────────────────────

/// 许可级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum License {
    /// 个人使用（不可商用分发）。
    Personal,
    /// 商用许可。
    Commercial,
    /// 公有领域。
    PublicDomain,
}

impl License {
    /// 是否允许商用分发（分级断言）。
    pub fn allows_commercial(self) -> bool {
        matches!(self, License::Commercial | License::PublicDomain)
    }
}

// ── 素材登记 ────────────────────────────────────────────────────────

/// 素材条目。
#[derive(Clone, Copy)]
pub struct Asset {
    pub id: u32,
    /// 来源串长度（0 = 无来源）。
    pub source_len: u32,
    pub license: License,
    /// 特征指纹（比对用）。
    pub fingerprint: u64,
}

impl Asset {
    /// 来源必填红线。
    pub fn has_source(&self) -> bool {
        self.source_len > 0
    }
}

// ── 特征库（F3612 单源复用口径） ────────────────────────────────────

/// 已发布特征库。
pub struct FeatureDb {
    known: Vec<u64>,
    /// 比对总数 / 误报数（误报门）。
    compares: u64,
    false_pos: u64,
}

impl FeatureDb {
    pub fn new() -> Self {
        Self { known: Vec::new(), compares: 0, false_pos: 0 }
    }

    /// 入库已发布特征。
    pub fn publish(&mut self, fp: u64) -> usize {
        if !self.known.contains(&fp) {
            self.known.push(fp);
        }
        self.known.len()
    }

    /// 盗用检测：命中已发布特征。
    pub fn match_stolen(&mut self, fp: u64) -> bool {
        self.compares += 1;
        self.known.contains(&fp)
    }

    /// 记误报（申诉成立）。
    pub fn note_false_positive(&mut self) {
        self.false_pos += 1;
    }

    /// 误报率（基点）。
    pub fn fp_rate_bps(&self) -> u32 {
        if self.compares == 0 {
            return 0;
        }
        (self.false_pos * 10_000 / self.compares) as u32
    }

    /// 误报门：超 500 基点（5%）即校准红线。
    pub fn needs_calibration(&mut self) -> Result<bool, C55Code> {
        if self.fp_rate_bps() > 500 {
            Err(C55Code::FALSE_POSITIVE)
        } else {
            Ok(false)
        }
    }

    pub fn size(&self) -> usize {
        self.known.len()
    }
}

// ── 申诉流程 ────────────────────────────────────────────────────────

/// 申诉单。
#[derive(Clone, Copy)]
pub struct Appeal {
    pub id: u32,
    /// 处理耗时（小时）。
    pub elapsed_h: u32,
    /// 时限（小时，超时判）。
    pub deadline_h: u32,
}

impl Appeal {
    pub fn timed_out(&self) -> bool {
        self.elapsed_h > self.deadline_h
    }
}

// ── 版权检查器 ──────────────────────────────────────────────────────

/// 视觉创作版权检查器。
pub struct CopyrightGate {
    db: FeatureDb,
    blocked: u32,
    takedowns: u32,
    appeals: Vec<Appeal>,
    last_code: C55Code,
    has_error: bool,
}

impl CopyrightGate {
    pub fn new() -> Self {
        Self {
            db: FeatureDb::new(),
            blocked: 0,
            takedowns: 0,
            appeals: Vec::new(),
            last_code: C55Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C55Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: C55Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 发布前检查：来源必填 + 许可分级 + 盗用比对。
    pub fn publish_gate(&mut self, asset: &Asset, commercial: bool) -> Result<bool, C55Code> {
        if !asset.has_source() {
            self.blocked += 1;
            self.fail(C55Code::NO_SOURCE);
            return Err(C55Code::NO_SOURCE);
        }
        if commercial && !asset.license.allows_commercial() {
            self.takedowns += 1;
            self.fail(C55Code::LICENSE_VIOLATION);
            return Err(C55Code::LICENSE_VIOLATION);
        }
        if self.db.match_stolen(asset.fingerprint) {
            self.blocked += 1;
            self.fail(C55Code::LICENSE_VIOLATION);
            return Err(C55Code::LICENSE_VIOLATION);
        }
        self.last_code = C55Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 特征库入库。
    pub fn publish_feature(&mut self, fp: u64) -> usize {
        self.db.publish(fp)
    }

    /// 提交申诉。
    pub fn file_appeal(&mut self, id: u32, elapsed_h: u32, deadline_h: u32) -> Result<usize, C55Code> {
        let a = Appeal { id, elapsed_h, deadline_h };
        self.appeals.push(a);
        if a.timed_out() {
            self.fail(C55Code::APPEAL_TIMEOUT);
            return Err(C55Code::APPEAL_TIMEOUT);
        }
        self.last_code = C55Code(0);
        self.has_error = false;
        Ok(self.appeals.len())
    }

    /// 申诉成立 → 记误报（白名单精神）。
    pub fn uphold_appeal(&mut self) {
        self.db.note_false_positive();
    }

    /// 误报门执法。
    pub fn check_fp_gate(&mut self) -> Result<bool, C55Code> {
        self.db.needs_calibration().map_err(|c| {
            self.fail(c);
            c
        })
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }

    pub fn takedowns(&self) -> u32 {
        self.takedowns
    }

    pub fn appeal_count(&self) -> usize {
        self.appeals.len()
    }

    pub fn db_size(&self) -> usize {
        self.db.size()
    }

    /// 单源复用总评：特征库非空即可查。
    pub fn single_source_green(&self) -> bool {
        self.db.size() > 0 || self.db.compares == 0
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves55_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves55_copyright");
    let good = Asset { id: 1, source_len: 10, license: License::Commercial, fingerprint: 0xAAAA };
    {
        let g = CopyrightGate::new();
        chk(&mut set, "C3655-01", g.blocked() == 0 && g.db_size() == 0, "err");
    }
    {
        let mut g = CopyrightGate::new();
        let r = g.publish_gate(&good, true);
        chk(&mut set, "C3655-02", r == Ok(true), "err");
    }
    {
        let mut g = CopyrightGate::new();
        let no_src = Asset { id: 1, source_len: 0, license: License::Commercial, fingerprint: 1 };
        let r = g.publish_gate(&no_src, true);
        chk(&mut set, "C3655-03", r.err() == Some(C55Code::NO_SOURCE) && g.blocked() == 1, "err");
    }
    {
        let mut g = CopyrightGate::new();
        let personal = Asset { id: 1, source_len: 5, license: License::Personal, fingerprint: 1 };
        let r = g.publish_gate(&personal, true);
        chk(&mut set, "C3655-04", r.err() == Some(C55Code::LICENSE_VIOLATION), "err");
    }
    {
        let mut g = CopyrightGate::new();
        let personal = Asset { id: 1, source_len: 5, license: License::Personal, fingerprint: 1 };
        let r = g.publish_gate(&personal, false);
        chk(&mut set, "C3655-05", r == Ok(true), "err");
    }
    {
        let mut g = CopyrightGate::new();
        g.publish_feature(0xBBBB);
        let stolen = Asset { id: 2, source_len: 5, license: License::Commercial, fingerprint: 0xBBBB };
        let r = g.publish_gate(&stolen, true);
        chk(&mut set, "C3655-06", r.err() == Some(C55Code::LICENSE_VIOLATION), "err");
    }
    {
        let mut g = CopyrightGate::new();
        let r = g.file_appeal(1, 10, 48);
        chk(&mut set, "C3655-07", r == Ok(1), "err");
    }
    {
        let mut g = CopyrightGate::new();
        let r = g.file_appeal(1, 100, 48);
        chk(&mut set, "C3655-08", r.err() == Some(C55Code::APPEAL_TIMEOUT), "err");
    }
    {
        let mut g = CopyrightGate::new();
        g.publish_gate(&good, true).unwrap();
        g.uphold_appeal();
        let r = g.check_fp_gate();
        chk(&mut set, "C3655-09", r == Ok(false), "err");
    }
    {
        let mut db = FeatureDb::new();
        for _ in 0..10 {
            db.match_stolen(9);
        }
        for _ in 0..2 {
            db.note_false_positive();
        }
        let r = db.needs_calibration();
        chk(&mut set, "C3655-10", r == Ok(false) && db.fp_rate_bps() == 2_000, "err");
    }
    {
        let mut db = FeatureDb::new();
        for _ in 0..10 {
            db.match_stolen(9);
        }
        for _ in 0..6 {
            db.note_false_positive();
        }
        let r = db.needs_calibration();
        chk(&mut set, "C3655-11", r.err() == Some(C55Code::FALSE_POSITIVE), "err");
    }
    {
        let l = License::Commercial;
        chk(&mut set, "C3655-12", l.allows_commercial() && !License::Personal.allows_commercial(), "err");
    }
    set
}
