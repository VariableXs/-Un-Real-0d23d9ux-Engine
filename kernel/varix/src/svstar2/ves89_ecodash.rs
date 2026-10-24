//! VE-F3689 · 生态数据看板（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3689`
//!
//! **职责定位（锚点原文）**：生态数据看板——
//! - **双看板**：运营看板（四指标+健康度+治理）/ 创作者看板（下载/评分/收益+基线）；
//! - **数据口径**：全部口径公开（复述口径红线）；
//! - **看板权限**：运营看板限运营（权限复用）；
//! - **看板诚实**：不利数据同展示（粉饰红线全域化）。
//!
//! **判据（锚点原文）**：双看板、口径公开、权限复用、诚实断言、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 粉饰检出→P0（复述红线实测） | [`C89Code::WHITEWASH`] |
//! | 口径分叉→单源（复述） | [`C89Code::METRIC_FORK`] |
//! | 越权看板→P1（复述权限） | [`C89Code::UNAUTHORIZED`] |
//! | 创作者数据缺→补（复述明细） | [`C89Code::CREATOR_DATA_GAP`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3689 诊断码段（独占 0x89xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C89Code(pub u16);

impl C89Code {
    /// 粉饰检出（P0 红线实测）。
    pub const WHITEWASH: C89Code = C89Code(0x8901);
    /// 口径分叉。
    pub const METRIC_FORK: C89Code = C89Code(0x8902);
    /// 越权看板（P1）。
    pub const UNAUTHORIZED: C89Code = C89Code(0x8903);
    /// 创作者数据缺。
    pub const CREATOR_DATA_GAP: C89Code = C89Code(0x8904);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C89Code::WHITEWASH => "数据粉饰检出（P0 红线实测）。建议：补齐不利数据。",
            C89Code::METRIC_FORK => "口径分叉。建议：对齐单源。",
            C89Code::UNAUTHORIZED => "越权访问看板（P1）。建议：权限拦截。",
            C89Code::CREATOR_DATA_GAP => "创作者数据缺。建议：补明细。",
            _ => "未知诊断码。",
        }
    }
}

// ── 双看板 ──────────────────────────────────────────────────────────

/// 看板类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardKind {
    /// 运营看板。
    Ops,
    /// 创作者看板。
    Creator,
}

/// 指标格。
#[derive(Clone, Copy)]
pub struct MetricCell {
    pub key: u8,
    pub value: i64,
    /// 是否不利数据（粉饰红线）。
    pub adverse: bool,
    /// 是否已展示。
    pub shown: bool,
}

/// 看板。
pub struct Dashboard {
    kind: BoardKind,
    cells: Vec<MetricCell>,
    /// 口径版本（单源）。
    metric_version: u8,
    whitewash_hits: u32,
    gap_hits: u32,
    last_code: C89Code,
    has_error: bool,
}

impl Dashboard {
    pub fn new(kind: BoardKind) -> Self {
        Self {
            kind,
            cells: Vec::new(),
            metric_version: 1,
            whitewash_hits: 0,
            gap_hits: 0,
            last_code: C89Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C89Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn kind(&self) -> BoardKind {
        self.kind
    }

    pub fn put(&mut self, cell: MetricCell) -> usize {
        self.cells.push(cell);
        self.cells.len()
    }

    /// 诚实断言：不利数据必须展示。
    pub fn assert_honest(&mut self) -> Result<bool, C89Code> {
        let mut ok = true;
        for c in &self.cells {
            if c.adverse && !c.shown {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C89Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.whitewash_hits += 1;
            self.last_code = C89Code::WHITEWASH;
            self.has_error = true;
            Err(C89Code::WHITEWASH)
        }
    }

    /// 创作者数据齐备（下载/评分/收益三键）。
    pub fn creator_data_complete(&self) -> bool {
        if self.kind != BoardKind::Creator {
            return true;
        }
        (1..=3).all(|k| self.cells.iter().any(|c| c.key == k))
    }

    /// 缺数据审计。
    pub fn audit_gaps(&mut self) -> Result<bool, C89Code> {
        if self.creator_data_complete() {
            self.last_code = C89Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.gap_hits += 1;
            self.last_code = C89Code::CREATOR_DATA_GAP;
            self.has_error = true;
            Err(C89Code::CREATOR_DATA_GAP)
        }
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn whitewash_hits(&self) -> u32 {
        self.whitewash_hits
    }

    pub fn gap_hits(&self) -> u32 {
        self.gap_hits
    }

    pub fn metric_version(&self) -> u8 {
        self.metric_version
    }
}

// ── 口径单源 ────────────────────────────────────────────────────────

/// 口径注册（单源复述）。
pub struct MetricSource {
    version: u8,
    forks: u32,
    last_code: C89Code,
    has_error: bool,
}

impl MetricSource {
    pub fn new() -> Self {
        Self {
            version: 1,
            forks: 0,
            last_code: C89Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C89Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 对齐口径：版本必须一致。
    pub fn align(&mut self, board_version: u8) -> Result<bool, C89Code> {
        if board_version == self.version {
            self.last_code = C89Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.forks += 1;
            self.last_code = C89Code::METRIC_FORK;
            self.has_error = true;
            Err(C89Code::METRIC_FORK)
        }
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn forks(&self) -> u32 {
        self.forks
    }
}

// ── 权限 ────────────────────────────────────────────────────────────

/// 角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// 运营。
    Ops,
    /// 创作者。
    Creator,
    /// 游客。
    Guest,
}

/// 权限守卫（权限复用）。
pub struct AccessGuard {
    auth_hits: u32,
    last_code: C89Code,
    has_error: bool,
}

impl AccessGuard {
    pub fn new() -> Self {
        Self {
            auth_hits: 0,
            last_code: C89Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C89Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 访问闸：Ops 看板仅限 Ops。
    pub fn check(&mut self, role: Role, board: BoardKind) -> Result<bool, C89Code> {
        let allowed = match board {
            BoardKind::Ops => role == Role::Ops,
            BoardKind::Creator => role == Role::Ops || role == Role::Creator,
        };
        if allowed {
            self.last_code = C89Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.auth_hits += 1;
            self.last_code = C89Code::UNAUTHORIZED;
            self.has_error = true;
            Err(C89Code::UNAUTHORIZED)
        }
    }

    pub fn auth_hits(&self) -> u32 {
        self.auth_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves89_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves89_ecodash");
    {
        let d = Dashboard::new(BoardKind::Ops);
        chk(&mut set, "C3689-01", d.kind() == BoardKind::Ops && d.cell_count() == 0, "err");
    }
    {
        let mut d = Dashboard::new(BoardKind::Ops);
        d.put(MetricCell { key: 1, value: 100, adverse: false, shown: true });
        d.put(MetricCell { key: 2, value: -5, adverse: true, shown: true });
        chk(&mut set, "C3689-02", d.assert_honest() == Ok(true), "err");
    }
    {
        let mut d = Dashboard::new(BoardKind::Ops);
        d.put(MetricCell { key: 1, value: -5, adverse: true, shown: false });
        let res = d.assert_honest();
        chk(&mut set, "C3689-03", res.err() == Some(C89Code::WHITEWASH), "err");
    }
    {
        let mut d = Dashboard::new(BoardKind::Creator);
        d.put(MetricCell { key: 1, value: 10, adverse: false, shown: true });
        d.put(MetricCell { key: 2, value: 4, adverse: false, shown: true });
        d.put(MetricCell { key: 3, value: 99, adverse: false, shown: true });
        chk(&mut set, "C3689-04", d.creator_data_complete() && d.audit_gaps() == Ok(true), "err");
    }
    {
        let mut d = Dashboard::new(BoardKind::Creator);
        d.put(MetricCell { key: 1, value: 10, adverse: false, shown: true });
        let res = d.audit_gaps();
        chk(&mut set, "C3689-05", res.err() == Some(C89Code::CREATOR_DATA_GAP), "err");
    }
    {
        let mut s = MetricSource::new();
        let res = s.align(1);
        chk(&mut set, "C3689-06", res == Ok(true), "err");
    }
    {
        let mut s = MetricSource::new();
        let res = s.align(9);
        chk(&mut set, "C3689-07", res.err() == Some(C89Code::METRIC_FORK), "err");
    }
    {
        let mut g = AccessGuard::new();
        chk(&mut set, "C3689-08", g.check(Role::Ops, BoardKind::Ops) == Ok(true), "err");
    }
    {
        let mut g = AccessGuard::new();
        let res = g.check(Role::Creator, BoardKind::Ops);
        chk(&mut set, "C3689-09", res.err() == Some(C89Code::UNAUTHORIZED), "err");
    }
    {
        let mut g = AccessGuard::new();
        chk(&mut set, "C3689-10", g.check(Role::Creator, BoardKind::Creator) == Ok(true), "err");
    }
    {
        let mut g = AccessGuard::new();
        let res = g.check(Role::Guest, BoardKind::Creator);
        chk(&mut set, "C3689-11", res.err() == Some(C89Code::UNAUTHORIZED), "err");
    }
    {
        let d = Dashboard::new(BoardKind::Creator);
        chk(&mut set, "C3689-12", d.creator_data_complete() && d.metric_version() == 1, "err");
    }
    set
}
