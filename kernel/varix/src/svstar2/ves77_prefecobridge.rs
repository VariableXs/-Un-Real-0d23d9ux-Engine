//! VE-F3677 · 个性化与 R 域创作联动（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3677`
//!
//! **职责定位（锚点原文）**：个性化与 R 域创作联动——
//! - **创作偏好注入**：创作者工具读取个性化（默认画布/风格偏好）——偏好只做默认值，用户可改
//!   （注入可改红线：强制偏好 = 主权侵犯——可改断言）；
//! - **作品个性化**：创作者为作品定制默认个性化——用户可一键还原（还原断言）
//!   （作品默认 ≠ 用户绑定——锁死 P0）；
//! - **联动审计**：注入与默认的全留痕复述。
//!
//! **判据（锚点原文）**：注入可改、作品还原、锁死 P0、审计留痕、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 注入强制→P1（红线实测） | [`C77Code::FORCE_INJECT`] |
//! | 作品锁死→P0（红线最高级） | [`C77Code::WORK_LOCK`] |
//! | 还原失效→断言（复用） | [`C77Code::RESTORE_FAIL`] |
//! | 审计缺→补（复述） | [`C77Code::AUDIT_GAP`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3677 诊断码段（独占 0x7Dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C77Code(pub u16);

impl C77Code {
    /// 注入强制（P1 红线实测）。
    pub const FORCE_INJECT: C77Code = C77Code(0x7D01);
    /// 作品锁死（P0 红线最高级）。
    pub const WORK_LOCK: C77Code = C77Code(0x7D02);
    /// 还原失效。
    pub const RESTORE_FAIL: C77Code = C77Code(0x7D03);
    /// 审计缺痕。
    pub const AUDIT_GAP: C77Code = C77Code(0x7D04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C77Code::FORCE_INJECT => "偏好注入强制不可改（P1 红线实测）。建议：注入可改。",
            C77Code::WORK_LOCK => "作品锁死用户个性化（P0 红线最高级）。建议：一键还原。",
            C77Code::RESTORE_FAIL => "还原失效。建议：可逆断言修复。",
            C77Code::AUDIT_GAP => "联动审计缺痕。建议：补留痕。",
            _ => "未知诊断码。",
        }
    }
}

// ── 偏好注入 ────────────────────────────────────────────────────────

/// 注入项。
#[derive(Clone, Copy)]
pub struct InjectItem {
    pub key: u16,
    pub default_value: i32,
    /// 是否允许用户改（可改红线）。
    pub user_overridable: bool,
    /// 用户当前覆盖值。
    pub override_value: Option<i32>,
}

/// 偏好注入器（可改断言）。
pub struct PrefInjector {
    items: Vec<InjectItem>,
    force_hits: u32,
    last_code: C77Code,
    has_error: bool,
}

impl PrefInjector {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            force_hits: 0,
            last_code: C77Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C77Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 注入一项默认值。
    pub fn inject(&mut self, key: u16, default_value: i32) -> Result<usize, C77Code> {
        self.items.push(InjectItem {
            key,
            default_value,
            user_overridable: true,
            override_value: None,
        });
        self.last_code = C77Code(0);
        self.has_error = false;
        Ok(self.items.len())
    }

    /// 强制注入（不可改——违规路径）。
    pub fn inject_locked(&mut self, key: u16, default_value: i32) -> Result<usize, C77Code> {
        self.force_hits += 1;
        self.last_code = C77Code::FORCE_INJECT;
        self.has_error = true;
        Err(C77Code::FORCE_INJECT)
    }

    /// 用户覆盖（可改断言）。
    pub fn user_override(&mut self, key: u16, value: i32) -> Result<bool, C77Code> {
        match self.items.iter_mut().find(|e| e.key == key) {
            Some(e) if e.user_overridable => {
                e.override_value = Some(value);
                self.last_code = C77Code(0);
                self.has_error = false;
                Ok(true)
            }
            Some(_) => {
                self.force_hits += 1;
                self.last_code = C77Code::FORCE_INJECT;
                self.has_error = true;
                Err(C77Code::FORCE_INJECT)
            }
            None => {
                self.last_code = C77Code::AUDIT_GAP;
                self.has_error = true;
                Err(C77Code::AUDIT_GAP)
            }
        }
    }

    /// 生效值：有覆盖取覆盖，否则取默认。
    pub fn effective(&self, key: u16) -> Option<i32> {
        self.items
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.override_value.unwrap_or(e.default_value))
    }

    pub fn force_hits(&self) -> u32 {
        self.force_hits
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 全部可改。
    pub fn all_overridable(&self) -> bool {
        self.items.iter().all(|e| e.user_overridable)
    }
}

// ── 作品默认 ────────────────────────────────────────────────────────

/// 作品默认个性化（还原断言 + 锁死 P0）。
pub struct WorkDefault {
    /// 作品自带默认值。
    work_value: i32,
    /// 用户原值（还原用）。
    user_backup: Option<i32>,
    /// 是否处于作品默认态。
    applied: bool,
    /// 是否锁死（P0）。
    locked: bool,
    lock_hits: u32,
    restore_hits: u32,
    last_code: C77Code,
    has_error: bool,
}

impl WorkDefault {
    pub fn new(work_value: i32) -> Self {
        Self {
            work_value,
            user_backup: None,
            applied: false,
            locked: false,
            lock_hits: 0,
            restore_hits: 0,
            last_code: C77Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C77Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 应用作品默认（备份用户原值）。
    pub fn apply(&mut self, user_value: i32) -> Result<bool, C77Code> {
        if self.locked {
            self.lock_hits += 1;
            self.last_code = C77Code::WORK_LOCK;
            self.has_error = true;
            return Err(C77Code::WORK_LOCK);
        }
        self.user_backup = Some(user_value);
        self.applied = true;
        self.last_code = C77Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 一键还原（还原断言）。
    pub fn restore(&mut self) -> Result<i32, C77Code> {
        match self.user_backup {
            Some(v) => {
                self.applied = false;
                self.last_code = C77Code(0);
                self.has_error = false;
                Ok(v)
            }
            None => {
                self.restore_hits += 1;
                self.last_code = C77Code::RESTORE_FAIL;
                self.has_error = true;
                Err(C77Code::RESTORE_FAIL)
            }
        }
    }

    /// 锁死检测（作品不得锁死用户个性化）。
    pub fn note_lock(&mut self) {
        self.locked = true;
        self.lock_hits += 1;
        self.last_code = C77Code::WORK_LOCK;
        self.has_error = true;
    }

    pub fn is_applied(&self) -> bool {
        self.applied
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn lock_hits(&self) -> u32 {
        self.lock_hits
    }

    pub fn restore_hits(&self) -> u32 {
        self.restore_hits
    }

    pub fn work_value(&self) -> i32 {
        self.work_value
    }
}

// ── 联动审计 ────────────────────────────────────────────────────────

/// 审计事件。
#[derive(Clone, Copy)]
pub struct BridgeAuditEvent {
    /// 0=注入 1=覆盖 2=作品默认 3=还原
    pub kind: u8,
    pub key: u16,
}

/// 联动审计（全留痕复述）。
pub struct BridgeAudit {
    events: Vec<BridgeAuditEvent>,
    gap_hits: u32,
    last_code: C77Code,
    has_error: bool,
}

impl BridgeAudit {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            gap_hits: 0,
            last_code: C77Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C77Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn record(&mut self, kind: u8, key: u16) -> usize {
        self.events.push(BridgeAuditEvent { kind, key });
        self.events.len()
    }

    /// 留痕审计：全部事件 kind 合法。
    pub fn audit(&mut self) -> Result<bool, C77Code> {
        if self.events.iter().all(|e| e.kind <= 3) {
            self.last_code = C77Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.gap_hits += 1;
            self.last_code = C77Code::AUDIT_GAP;
            self.has_error = true;
            Err(C77Code::AUDIT_GAP)
        }
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn gap_hits(&self) -> u32 {
        self.gap_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves77_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves77_prefecobridge");
    {
        let mut i = PrefInjector::new();
        let n = i.inject(1, 10);
        chk(&mut set, "C3677-01", n == Ok(1) && i.all_overridable(), "err");
    }
    {
        let mut i = PrefInjector::new();
        let res = i.inject_locked(1, 10);
        chk(&mut set, "C3677-02", res.err() == Some(C77Code::FORCE_INJECT), "err");
    }
    {
        let mut i = PrefInjector::new();
        i.inject(1, 10);
        let res = i.user_override(1, 99);
        chk(&mut set, "C3677-03", res == Ok(true) && i.effective(1) == Some(99), "err");
    }
    {
        let mut i = PrefInjector::new();
        i.inject(1, 10);
        chk(&mut set, "C3677-04", i.effective(1) == Some(10), "err");
    }
    {
        let mut w = WorkDefault::new(200);
        let res = w.apply(100);
        chk(&mut set, "C3677-05", res == Ok(true) && w.is_applied(), "err");
    }
    {
        let mut w = WorkDefault::new(200);
        w.apply(100);
        let res = w.restore();
        chk(&mut set, "C3677-06", res == Ok(100) && !w.is_applied(), "err");
    }
    {
        let mut w = WorkDefault::new(200);
        let res = w.restore();
        chk(&mut set, "C3677-07", res.err() == Some(C77Code::RESTORE_FAIL), "err");
    }
    {
        let mut w = WorkDefault::new(200);
        w.note_lock();
        let res = w.apply(100);
        chk(&mut set, "C3677-08", res.err() == Some(C77Code::WORK_LOCK) && w.lock_hits() >= 1, "err");
    }
    {
        let mut a = BridgeAudit::new();
        a.record(0, 1);
        a.record(1, 1);
        a.record(2, 2);
        a.record(3, 1);
        chk(&mut set, "C3677-09", a.event_count() == 4 && a.audit() == Ok(true), "err");
    }
    {
        let mut a = BridgeAudit::new();
        a.record(9, 1);
        let res = a.audit();
        chk(&mut set, "C3677-10", res.err() == Some(C77Code::AUDIT_GAP), "err");
    }
    {
        let mut i = PrefInjector::new();
        i.inject(1, 1);
        let res = i.user_override(9, 1);
        chk(&mut set, "C3677-11", res.err() == Some(C77Code::AUDIT_GAP), "err");
    }
    {
        let mut w = WorkDefault::new(50);
        w.apply(10);
        w.restore();
        let res = w.apply(11);
        chk(&mut set, "C3677-12", res == Ok(true), "err");
    }
    set
}
