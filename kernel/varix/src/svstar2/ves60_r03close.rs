//! VE-F3660 · R03 组收口双签（VE-S 域 · 批次 R03 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3660`
//!
//! **职责定位（锚点原文）**：R03 组收口——
//! - **接口总账**：R03 冻结 API 十三族签名表（四核画布/壁纸编辑/动态时间轴/
//!   取色/图标网格/光标/锁屏/导出/4K 门/无障碍/隐写/版权……）；
//! - **映射断言 + 20 项自检**；
//! - **联动核验**：单源画布证据（零私造）/ 4K 门 + 伪检全绿 /
//!   隐写 + 元数据 P0 断言全绿 / 产消双签证据 / E 域回传协议证据。
//!
//! **判据（锚点原文六条）**：十三族、单源硬门、双 P0 硬门、
//! 产消双签、双签、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 映射缺→阻断 | [`S60Code::MAP_MISSING`] |
//! | 私造检出→P1 重构（复述硬门） | [`S60Code::PRIVATE_IMPL`] |
//! | P0 断言失败→阻断（精度/隐私硬门） | [`S60Code::P0_FAIL`] |
//! | 联动分歧→两轮上限（复述） | [`S60Code::LINK_DIVERGED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3660 诊断码段（独占 0x6Cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S60Code(pub u16);

impl S60Code {
    /// 映射缺失（阻断）。
    pub const MAP_MISSING: S60Code = S60Code(0x6C01);
    /// 私造实现检出（P1 硬门）。
    pub const PRIVATE_IMPL: S60Code = S60Code(0x6C02);
    /// P0 断言失败（阻断）。
    pub const P0_FAIL: S60Code = S60Code(0x6C03);
    /// 联动分歧（两轮上限）。
    pub const LINK_DIVERGED: S60Code = S60Code(0x6C04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            S60Code::MAP_MISSING => "接口映射缺失（阻断）。建议：补签名表映射。",
            S60Code::PRIVATE_IMPL => "私造实现检出（P1 硬门）。建议：重构回归单源。",
            S60Code::P0_FAIL => "P0 断言失败（精度/隐私硬门）。建议：阻断收口。",
            S60Code::LINK_DIVERGED => "联动分歧。建议：两轮内对齐。",
            _ => "未知诊断码。",
        }
    }
}

// ── 十三族签名表 ────────────────────────────────────────────────────

/// API 族（R03 冻结十三族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 四核画布。
    CanvasCore,
    /// 壁纸编辑。
    WallEdit,
    /// 动态时间轴。
    Timeline,
    /// 取色。
    ColorPick,
    /// 图标网格。
    IconGrid,
    /// 光标。
    Cursor,
    /// 锁屏。
    LockScreen,
    /// 导出。
    Export,
    /// 4K 门。
    Gate4k,
    /// 无障碍。
    A11y,
    /// 隐写。
    Stego,
    /// 版权。
    Copyright,
    /// 产消协议。
    ProduceConsume,
}

impl Family {
    pub const ALL: [Family; 13] = [
        Family::CanvasCore,
        Family::WallEdit,
        Family::Timeline,
        Family::ColorPick,
        Family::IconGrid,
        Family::Cursor,
        Family::LockScreen,
        Family::Export,
        Family::Gate4k,
        Family::A11y,
        Family::Stego,
        Family::Copyright,
        Family::ProduceConsume,
    ];
}

/// 签名表条目。
#[derive(Clone, Copy)]
pub struct ApiEntry {
    pub family: Family,
    /// 函数签名指纹。
    pub sig_fp: u32,
    /// 映射到判据 id（0 = 未映射）。
    pub mapped_to: u16,
    /// 是否私造（true = 红线）。
    pub private: bool,
}

// ── 联动证据 ────────────────────────────────────────────────────────

/// 联动证据位。
#[derive(Clone, Copy)]
pub struct LinkEvidence {
    pub canvas_single_source: bool,
    pub gate4k_green: bool,
    pub p0_privacy_green: bool,
    pub dual_sign_ok: bool,
    pub constraint_returned: bool,
}

impl LinkEvidence {
    /// 全绿条件。
    pub fn all_green(&self) -> bool {
        self.canvas_single_source
            && self.gate4k_green
            && self.p0_privacy_green
            && self.dual_sign_ok
            && self.constraint_returned
    }
}

// ── 双签 ────────────────────────────────────────────────────────────

/// 收口双签。
#[derive(Clone, Copy)]
pub struct CloseSign {
    pub author_signed: bool,
    pub reviewer_signed: bool,
}

impl CloseSign {
    pub fn complete(&self) -> bool {
        self.author_signed && self.reviewer_signed
    }
}

// ── 收口器 ──────────────────────────────────────────────────────────

/// R03 收口器。
pub struct R03Close {
    table: Vec<ApiEntry>,
    evidence: LinkEvidence,
    sign: CloseSign,
    link_rounds: u32,
    last_code: S60Code,
    has_error: bool,
}

impl R03Close {
    pub fn new() -> Self {
        Self {
            table: Vec::new(),
            evidence: LinkEvidence {
                canvas_single_source: false,
                gate4k_green: false,
                p0_privacy_green: false,
                dual_sign_ok: false,
                constraint_returned: false,
            },
            sign: CloseSign { author_signed: false, reviewer_signed: false },
            link_rounds: 0,
            last_code: S60Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> S60Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: S60Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 登记 API 条目；私造即红线。
    pub fn add_api(&mut self, entry: ApiEntry) -> Result<usize, S60Code> {
        if entry.private {
            self.fail(S60Code::PRIVATE_IMPL);
            return Err(S60Code::PRIVATE_IMPL);
        }
        self.table.push(entry);
        self.last_code = S60Code(0);
        self.has_error = false;
        Ok(self.table.len())
    }

    /// 十三族齐装。
    pub fn families_complete(&self) -> bool {
        Family::ALL
            .iter()
            .all(|f| self.table.iter().any(|e| e.family == *f))
    }

    /// 映射断言：全部条目映射到判据。
    pub fn assert_mappings(&mut self) -> Result<u32, S60Code> {
        let unmapped = self.table.iter().filter(|e| e.mapped_to == 0).count();
        if unmapped > 0 {
            self.fail(S60Code::MAP_MISSING);
            return Err(S60Code::MAP_MISSING);
        }
        self.last_code = S60Code(0);
        self.has_error = false;
        Ok(self.table.len() as u32)
    }

    /// 单源硬门：零私造。
    pub fn assert_single_source(&mut self) -> Result<bool, S60Code> {
        if self.table.iter().any(|e| e.private) {
            self.fail(S60Code::PRIVATE_IMPL);
            return Err(S60Code::PRIVATE_IMPL);
        }
        self.evidence.canvas_single_source = true;
        self.last_code = S60Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 双 P0 硬门（4K 精度 + 隐私）。
    pub fn assert_p0(&mut self, gate4k: bool, privacy: bool) -> Result<bool, S60Code> {
        if gate4k && privacy {
            self.evidence.gate4k_green = true;
            self.evidence.p0_privacy_green = true;
            self.last_code = S60Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S60Code::P0_FAIL);
            Err(S60Code::P0_FAIL)
        }
    }

    /// 产消双签 + 回传证据。
    pub fn assert_link(&mut self, dual_sign: bool, returned: bool) -> Result<bool, S60Code> {
        self.link_rounds += 1;
        if self.link_rounds > 2 {
            self.fail(S60Code::LINK_DIVERGED);
            return Err(S60Code::LINK_DIVERGED);
        }
        if dual_sign && returned {
            self.evidence.dual_sign_ok = true;
            self.evidence.constraint_returned = true;
            self.last_code = S60Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S60Code::LINK_DIVERGED);
            Err(S60Code::LINK_DIVERGED)
        }
    }

    /// 联动核验总评。
    pub fn evidence_green(&self) -> bool {
        self.evidence.all_green()
    }

    /// 双签落笔。
    pub fn countersign(&mut self, author: bool, reviewer: bool) -> Result<bool, S60Code> {
        self.sign = CloseSign { author_signed: author, reviewer_signed: reviewer };
        if self.sign.complete() {
            self.last_code = S60Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S60Code::MAP_MISSING);
            Err(S60Code::MAP_MISSING)
        }
    }

    pub fn api_count(&self) -> usize {
        self.table.len()
    }

    pub fn link_rounds(&self) -> u32 {
        self.link_rounds
    }

    /// 收口总评：十三族 + 映射 + 证据 + 双签。
    pub fn close_green(&mut self) -> bool {
        self.families_complete()
            && self.assert_mappings().is_ok()
            && self.evidence_green()
            && self.sign.complete()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves60_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves60_r03close");
    let mk = |family: Family, mapped: u16| ApiEntry {
        family,
        sig_fp: 0x51,
        mapped_to: mapped,
        private: false,
    };
    {
        let c = R03Close::new();
        chk(&mut set, "C3660-01", c.api_count() == 0 && !c.evidence_green(), "err");
    }
    {
        let mut c = R03Close::new();
        let r = c.add_api(mk(Family::CanvasCore, 1));
        chk(&mut set, "C3660-02", r == Ok(1), "err");
    }
    {
        let mut c = R03Close::new();
        let bad = ApiEntry { family: Family::Export, sig_fp: 1, mapped_to: 1, private: true };
        let r = c.add_api(bad);
        chk(&mut set, "C3660-03", r.err() == Some(S60Code::PRIVATE_IMPL), "err");
    }
    {
        let mut c = R03Close::new();
        for f in Family::ALL.iter() {
            c.add_api(mk(*f, 1));
        }
        chk(&mut set, "C3660-04", c.families_complete(), "err");
    }
    {
        let mut c = R03Close::new();
        c.add_api(mk(Family::Export, 0));
        let r = c.assert_mappings();
        chk(&mut set, "C3660-05", r.err() == Some(S60Code::MAP_MISSING), "err");
    }
    {
        let mut c = R03Close::new();
        c.add_api(mk(Family::Export, 5));
        let r = c.assert_mappings();
        chk(&mut set, "C3660-06", r == Ok(1), "err");
    }
    {
        let mut c = R03Close::new();
        c.add_api(mk(Family::CanvasCore, 1));
        let r = c.assert_single_source();
        chk(&mut set, "C3660-07", r == Ok(true), "err");
    }
    {
        let mut c = R03Close::new();
        let r = c.assert_p0(true, true);
        chk(&mut set, "C3660-08", r == Ok(true) && c.evidence_green() == false, "err");
    }
    {
        let mut c = R03Close::new();
        let r = c.assert_p0(true, false);
        chk(&mut set, "C3660-09", r.err() == Some(S60Code::P0_FAIL), "err");
    }
    {
        let mut c = R03Close::new();
        let r = c.assert_link(true, true);
        chk(&mut set, "C3660-10", r == Ok(true) && c.link_rounds() == 1, "err");
    }
    {
        let mut c = R03Close::new();
        c.assert_link(false, false);
        c.assert_link(false, false);
        let r = c.assert_link(false, false);
        chk(&mut set, "C3660-11", r.err() == Some(S60Code::LINK_DIVERGED), "err");
    }
    {
        let mut c = R03Close::new();
        for f in Family::ALL.iter() {
            c.add_api(mk(*f, 1));
        }
        c.assert_single_source();
        c.assert_p0(true, true);
        c.assert_link(true, true);
        c.countersign(true, true);
        chk(&mut set, "C3660-12", c.close_green(), "err");
    }
    set
}
