//! VE-F3669 · 个性化冲突消解（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3669`
//!
//! **职责定位（锚点原文）**：个性化同步冲突消解——
//! - **冲突类型**：双端同键异值 / 手设 vs 同步 / 删除 vs 修改；
//! - **消解策略**：用户手设优先（主权红线复述）+ 显性冲突提示；
//! - **静默红线**：冲突必须可见（静默覆盖 = 主权事故）；
//! - **消解可查**：每条冲突有裁决记录。
//!
//! **判据（锚点原文）**：冲突显性、手设优先、裁决可查、静默红线、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 静默覆盖→P1（红线实测） | [`C69Code::SILENT_OVERWRITE`] |
//! | 手设被压→P1（主权红线） | [`C69Code::USERSET_LOST`] |
//! | 裁决无痕→拦截 | [`C69Code::NO_RULING`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3669 诊断码段（独占 0x75xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C69Code(pub u16);

impl C69Code {
    /// 静默覆盖（P1 红线实测）。
    pub const SILENT_OVERWRITE: C69Code = C69Code(0x7501);
    /// 手设被压（主权红线）。
    pub const USERSET_LOST: C69Code = C69Code(0x7502);
    /// 裁决无痕。
    pub const NO_RULING: C69Code = C69Code(0x7503);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C69Code::SILENT_OVERWRITE => "冲突被静默覆盖（P1 红线实测）。建议：显性冲突提示。",
            C69Code::USERSET_LOST => "用户手设被压（主权红线）。建议：手设优先消解。",
            C69Code::NO_RULING => "裁决无痕。建议：每条冲突留裁决记录。",
            _ => "未知诊断码。",
        }
    }
}

// ── 冲突类型 ────────────────────────────────────────────────────────

/// 冲突类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictKind {
    /// 双端同键异值。
    BothEdited,
    /// 手设 vs 同步。
    UserVsSync,
    /// 删除 vs 修改。
    DeleteVsEdit,
}

/// 冲突条目。
#[derive(Clone, Copy)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub key: u16,
    pub local_value: i32,
    pub remote_value: i32,
    /// 本地是否用户手设。
    pub local_userset: bool,
}

// ── 裁决 ────────────────────────────────────────────────────────────

/// 裁决结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ruling {
    /// 取本地。
    KeepLocal,
    /// 取远端。
    TakeRemote,
    /// 手设优先（主权）。
    PreferUserSet,
}

/// 裁决记录（可查）。
#[derive(Clone, Copy)]
pub struct RulingRecord {
    pub key: u16,
    pub ruling: Ruling,
    pub final_value: i32,
}

// ── 消解器 ──────────────────────────────────────────────────────────

/// 个性化冲突消解器。
pub struct ConflictResolver {
    conflicts: Vec<Conflict>,
    rulings: Vec<RulingRecord>,
    silent_hits: u32,
    userset_lost: u32,
    last_code: C69Code,
    has_error: bool,
}

impl ConflictResolver {
    pub fn new() -> Self {
        Self {
            conflicts: Vec::new(),
            rulings: Vec::new(),
            silent_hits: 0,
            userset_lost: 0,
            last_code: C69Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C69Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: C69Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 登记冲突（显性提示口径）。
    pub fn raise(&mut self, c: Conflict) -> usize {
        self.conflicts.push(c);
        self.conflicts.len()
    }

    /// 消解一条：手设优先红线 + 必须留裁决。
    pub fn resolve(&mut self, index: usize, proposed: Ruling, final_value: i32) -> Result<Ruling, C69Code> {
        let c = match self.conflicts.get(index) {
            Some(c) => *c,
            None => {
                self.fail(C69Code::NO_RULING);
                return Err(C69Code::NO_RULING);
            }
        };
        // 主权红线：手设被压即报
        if c.local_userset && proposed == Ruling::TakeRemote {
            self.userset_lost += 1;
            self.fail(C69Code::USERSET_LOST);
            return Err(C69Code::USERSET_LOST);
        }
        let ruling = if c.local_userset { Ruling::PreferUserSet } else { proposed };
        self.rulings.push(RulingRecord {
            key: c.key,
            ruling,
            final_value: if c.local_userset { c.local_value } else { final_value },
        });
        self.last_code = C69Code(0);
        self.has_error = false;
        Ok(ruling)
    }

    /// 静默覆盖审计：有冲突无裁决即红线。
    pub fn audit_silence(&mut self) -> Result<bool, C69Code> {
        if self.conflicts.len() > self.rulings.len() {
            self.silent_hits += 1;
            self.fail(C69Code::SILENT_OVERWRITE);
            return Err(C69Code::SILENT_OVERWRITE);
        }
        self.last_code = C69Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 裁决可查（每条冲突有记录）。
    pub fn rulings_complete(&self) -> bool {
        self.rulings.len() >= self.conflicts.len()
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    pub fn ruling_count(&self) -> usize {
        self.rulings.len()
    }

    pub fn silent_hits(&self) -> u32 {
        self.silent_hits
    }

    pub fn userset_lost(&self) -> u32 {
        self.userset_lost
    }

    /// 按键查裁决。
    pub fn ruling_of(&self, key: u16) -> Option<&RulingRecord> {
        self.rulings.iter().find(|r| r.key == key)
    }

    /// 消解总评：无静默 + 无手设丢失 + 裁决齐。
    pub fn resolve_green(&self) -> bool {
        self.silent_hits == 0 && self.userset_lost == 0 && self.rulings_complete()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves69_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves69_conflictresolve");
    let mk = |key: u16, userset: bool| Conflict {
        kind: ConflictKind::UserVsSync,
        key,
        local_value: 10,
        remote_value: 20,
        local_userset: userset,
    };
    {
        let r = ConflictResolver::new();
        chk(&mut set, "C3669-01", r.conflict_count() == 0 && r.ruling_count() == 0, "err");
    }
    {
        let mut r = ConflictResolver::new();
        let n = r.raise(mk(1, true));
        chk(&mut set, "C3669-02", n == 1, "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, true));
        let res = r.resolve(0, Ruling::TakeRemote, 20);
        chk(&mut set, "C3669-03", res.err() == Some(C69Code::USERSET_LOST), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, true));
        let res = r.resolve(0, Ruling::KeepLocal, 10);
        chk(&mut set, "C3669-04", res == Ok(Ruling::PreferUserSet), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(2, false));
        let res = r.resolve(0, Ruling::TakeRemote, 20);
        chk(&mut set, "C3669-05", res == Ok(Ruling::TakeRemote), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, false));
        let res = r.resolve(5, Ruling::KeepLocal, 1);
        chk(&mut set, "C3669-06", res.err() == Some(C69Code::NO_RULING), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, false));
        let res = r.audit_silence();
        chk(&mut set, "C3669-07", res.err() == Some(C69Code::SILENT_OVERWRITE), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, false));
        r.resolve(0, Ruling::KeepLocal, 10);
        let res = r.audit_silence();
        chk(&mut set, "C3669-08", res == Ok(true), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(3, true));
        r.resolve(0, Ruling::KeepLocal, 10);
        let rec = r.ruling_of(3);
        chk(&mut set, "C3669-09", rec.map(|x| x.final_value) == Some(10), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, true));
        r.raise(mk(2, false));
        r.resolve(0, Ruling::KeepLocal, 1);
        r.resolve(1, Ruling::TakeRemote, 2);
        chk(&mut set, "C3669-10", r.rulings_complete(), "err");
    }
    {
        let mut r = ConflictResolver::new();
        r.raise(mk(1, false));
        r.resolve(0, Ruling::TakeRemote, 20);
        chk(&mut set, "C3669-11", r.resolve_green(), "err");
    }
    {
        let c = mk(1, true);
        chk(&mut set, "C3669-12", c.kind == ConflictKind::UserVsSync && c.local_userset, "err");
    }
    set
}
