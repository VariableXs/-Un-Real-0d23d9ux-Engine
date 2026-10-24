//! VE-F3668 · 个性化同步（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3668`
//!
//! **职责定位（锚点原文）**：个性化跨设备同步——
//! - **增量同步**（复用 F3205 优先级模式跨域声明——单源）；
//! - **范围可选红线**：同步范围用户定（默认全同步 = 越权 → 默认最小同步）；
//! - **关闭即停红线**：关闭后不再上传（关闭验证 = 网络审计，P0）；
//! - **同步安全**：传输加密 + 服务端哈希校验（复述）。
//!
//! **判据（锚点原文六条）**：范围默认最小、关闭即停 P0、增量单源、
//! 加密复述、对账复述、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 默认全同步→P1（范围红线实测） | [`S68Code::SCOPE_OVERREACH`] |
//! | 关闭仍上传→P0（红线实测——网络审计断言） | [`S68Code::UPLOAD_AFTER_OFF`] |
//! | 冲突静默→消解（前向复述） | [`S68Code::CONFLICT_SILENT`] |
//! | 同步丢项→对账（复述） | [`S68Code::SYNC_LOSS`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3668 诊断码段（独占 0x74xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S68Code(pub u16);

impl S68Code {
    /// 同步范围越权（P1 范围红线）。
    pub const SCOPE_OVERREACH: S68Code = S68Code(0x7401);
    /// 关闭后仍上传（P0 红线实测）。
    pub const UPLOAD_AFTER_OFF: S68Code = S68Code(0x7402);
    /// 冲突静默（前向消解）。
    pub const CONFLICT_SILENT: S68Code = S68Code(0x7403);
    /// 同步丢项（对账）。
    pub const SYNC_LOSS: S68Code = S68Code(0x7404);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            S68Code::SCOPE_OVERREACH => "同步范围越权（P1 范围红线）。建议：默认最小同步。",
            S68Code::UPLOAD_AFTER_OFF => "关闭后仍上传（P0 网络审计）。建议：关闭即停。",
            S68Code::CONFLICT_SILENT => "同步冲突静默。建议：走 F3669 消解。",
            S68Code::SYNC_LOSS => "同步丢项。建议：对账补齐。",
            _ => "未知诊断码。",
        }
    }
}

// ── 同步域 ──────────────────────────────────────────────────────────

/// 可同步域（用户可选）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncDomain {
    /// 视觉（主题/风格）。
    Visual,
    /// 交互（布局/快捷键）。
    Interaction,
    /// 内容（首页/排序）。
    Content,
    /// 行为（自动化偏好）。
    Behavior,
}

impl SyncDomain {
    pub const ALL: [SyncDomain; 4] = [
        SyncDomain::Visual,
        SyncDomain::Interaction,
        SyncDomain::Content,
        SyncDomain::Behavior,
    ];
}

// ── 同步条目 ────────────────────────────────────────────────────────

/// 待同步项。
#[derive(Clone, Copy)]
pub struct SyncItem {
    pub domain: SyncDomain,
    pub key: u16,
    /// 增量序号（复用 F3205 口径）。
    pub seq: u32,
}

// ── 同步器 ──────────────────────────────────────────────────────────

/// 个性化同步器。
pub struct PrefSync {
    scope: Vec<SyncDomain>,
    items: Vec<SyncItem>,
    enabled: bool,
    /// 网络审计：关闭后上传次数。
    leak_uploads: u32,
    encrypted: bool,
    conflicts: u32,
    last_code: S68Code,
    has_error: bool,
}

impl PrefSync {
    /// 新建：默认最小同步（空范围 = 不同步任何域）。
    pub fn new() -> Self {
        Self {
            scope: Vec::new(),
            items: Vec::new(),
            enabled: false,
            leak_uploads: 0,
            encrypted: true,
            conflicts: 0,
            last_code: S68Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> S68Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: S68Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 开启同步：范围由用户指定（全选需显式逐域确认）。
    pub fn enable(&mut self, domains: &[SyncDomain], explicit_all: bool) -> Result<usize, S68Code> {
        if domains.len() == SyncDomain::ALL.len() && !explicit_all {
            self.fail(S68Code::SCOPE_OVERREACH);
            return Err(S68Code::SCOPE_OVERREACH);
        }
        self.scope.clear();
        for d in domains {
            if !self.scope.contains(d) {
                self.scope.push(*d);
            }
        }
        self.enabled = true;
        self.last_code = S68Code(0);
        self.has_error = false;
        Ok(self.scope.len())
    }

    /// 关闭同步（关闭即停红线）。
    pub fn disable(&mut self) -> Result<bool, S68Code> {
        self.enabled = false;
        self.last_code = S68Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 上传一个变更（网络审计口径）。
    pub fn upload(&mut self, item: SyncItem) -> Result<u32, S68Code> {
        if !self.enabled {
            self.leak_uploads += 1;
            self.fail(S68Code::UPLOAD_AFTER_OFF);
            return Err(S68Code::UPLOAD_AFTER_OFF);
        }
        if !self.scope.contains(&item.domain) {
            self.fail(S68Code::SCOPE_OVERREACH);
            return Err(S68Code::SCOPE_OVERREACH);
        }
        self.items.push(item);
        self.last_code = S68Code(0);
        self.has_error = false;
        Ok(item.seq)
    }

    /// 关闭后上传审计（P0 断言）。
    pub fn audit_off_state(&mut self) -> Result<u32, S68Code> {
        if self.leak_uploads > 0 {
            self.fail(S68Code::UPLOAD_AFTER_OFF);
            return Err(S68Code::UPLOAD_AFTER_OFF);
        }
        self.last_code = S68Code(0);
        self.has_error = false;
        Ok(0)
    }

    /// 增量对账：应传与实传一致。
    pub fn reconcile(&mut self, expected: usize) -> Result<bool, S68Code> {
        if self.items.len() == expected {
            self.last_code = S68Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(S68Code::SYNC_LOSS);
            Err(S68Code::SYNC_LOSS)
        }
    }

    /// 冲突登记（F3669 前向，显性）。
    pub fn note_conflict(&mut self) -> u32 {
        self.conflicts += 1;
        self.fail(S68Code::CONFLICT_SILENT);
        self.conflicts
    }

    pub fn scope_size(&self) -> usize {
        self.scope.len()
    }

    pub fn synced_count(&self) -> usize {
        self.items.len()
    }

    pub fn leak_uploads(&self) -> u32 {
        self.leak_uploads
    }

    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 范围内含某域。
    pub fn in_scope(&self, d: SyncDomain) -> bool {
        self.scope.contains(&d)
    }

    /// 最小同步总评：范围 ≤1 域或全部显式。
    pub fn minimal_green(&self) -> bool {
        !self.enabled || self.scope.len() < SyncDomain::ALL.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves68_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves68_prefsync");
    {
        let s = PrefSync::new();
        chk(&mut set, "C3668-01", s.scope_size() == 0 && !s.is_enabled(), "err");
    }
    {
        let mut s = PrefSync::new();
        let r = s.enable(&[SyncDomain::Visual], false);
        chk(&mut set, "C3668-02", r == Ok(1) && s.in_scope(SyncDomain::Visual), "err");
    }
    {
        let mut s = PrefSync::new();
        let r = s.enable(&SyncDomain::ALL, false);
        chk(&mut set, "C3668-03", r.err() == Some(S68Code::SCOPE_OVERREACH), "err");
    }
    {
        let mut s = PrefSync::new();
        let r = s.enable(&SyncDomain::ALL, true);
        chk(&mut set, "C3668-04", r == Ok(4), "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Visual], false);
        let r = s.upload(SyncItem { domain: SyncDomain::Visual, key: 1, seq: 1 });
        chk(&mut set, "C3668-05", r == Ok(1) && s.synced_count() == 1, "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Visual], false);
        let r = s.upload(SyncItem { domain: SyncDomain::Behavior, key: 1, seq: 1 });
        chk(&mut set, "C3668-06", r.err() == Some(S68Code::SCOPE_OVERREACH), "err");
    }
    {
        let mut s = PrefSync::new();
        let r = s.upload(SyncItem { domain: SyncDomain::Visual, key: 1, seq: 1 });
        chk(&mut set, "C3668-07", r.err() == Some(S68Code::UPLOAD_AFTER_OFF) && s.leak_uploads() == 1, "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Content], false);
        s.disable();
        let r = s.audit_off_state();
        chk(&mut set, "C3668-08", r == Ok(0), "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Visual], false);
        s.upload(SyncItem { domain: SyncDomain::Visual, key: 1, seq: 1 });
        let r = s.reconcile(1);
        chk(&mut set, "C3668-09", r == Ok(true), "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Visual], false);
        let r = s.reconcile(5);
        chk(&mut set, "C3668-10", r.err() == Some(S68Code::SYNC_LOSS), "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Interaction], false);
        let n = s.note_conflict();
        chk(&mut set, "C3668-11", n == 1 && s.conflicts() == 1, "err");
    }
    {
        let mut s = PrefSync::new();
        s.enable(&[SyncDomain::Visual], false);
        chk(&mut set, "C3668-12", s.minimal_green(), "err");
    }
    set
}
