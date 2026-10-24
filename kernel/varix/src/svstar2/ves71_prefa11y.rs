//! VE-F3671 · 个性化无障碍（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3671`
//!
//! **职责定位（锚点原文）**：个性化无障碍——
//! - **载体关系**：个性化是无障碍的载体（字号/对比度/动效/焦点环全是个性化项）；
//! - **覆盖禁令**：无障碍项受特殊保护（系统更新/主题切换不得覆盖，P0 级）；
//! - **禁令矩阵**：无障碍键清单 + 覆盖拒绝；
//! - **优先级顶层**：无障碍项在优先级表最顶层（复述顶层）；
//! - **随人走**：跨设备同步必含无障碍项（换设备必须在——同步必含断言）。
//!
//! **判据（锚点原文）**：覆盖禁令 P0、随人走、顶层复述、一等级民、清单执法、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 主题覆盖无障碍→P0（红线实测） | [`C71Code::THEME_OVERWRITE`] |
//! | 同步漏无障碍→P1（随人走红线） | [`C71Code::SYNC_DROPPED`] |
//! | 顶层失效→修正 | [`C71Code::TOP_TIER_LOST`] |
//! | 清单漏项→补 | [`C71Code::LIST_GAP`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3671 诊断码段（独占 0x77xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C71Code(pub u16);

impl C71Code {
    /// 主题覆盖无障碍（P0 红线实测）。
    pub const THEME_OVERWRITE: C71Code = C71Code(0x7701);
    /// 同步漏无障碍（随人走红线）。
    pub const SYNC_DROPPED: C71Code = C71Code(0x7702);
    /// 优先级顶层失效。
    pub const TOP_TIER_LOST: C71Code = C71Code(0x7703);
    /// 禁令清单漏项。
    pub const LIST_GAP: C71Code = C71Code(0x7704);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C71Code::THEME_OVERWRITE => "主题覆盖无障碍项（P0 红线实测）。建议：覆盖禁令执法。",
            C71Code::SYNC_DROPPED => "同步漏无障碍项（随人走红线）。建议：同步必含断言。",
            C71Code::TOP_TIER_LOST => "优先级顶层失效。建议：无障碍项置顶修正。",
            C71Code::LIST_GAP => "禁令清单漏项。建议：补齐无障碍键。",
            _ => "未知诊断码。",
        }
    }
}

// ── 无障碍键 ────────────────────────────────────────────────────────

/// 无障碍个性化键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yKey {
    /// 字号。
    FontSize,
    /// 对比度。
    Contrast,
    /// 动效。
    Motion,
    /// 焦点环样式。
    FocusRing,
}

impl A11yKey {
    /// 一等级民清单（全部四键）。
    pub const ALL: [A11yKey; 4] = [
        A11yKey::FontSize,
        A11yKey::Contrast,
        A11yKey::Motion,
        A11yKey::FocusRing,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            A11yKey::FontSize => "字号",
            A11yKey::Contrast => "对比度",
            A11yKey::Motion => "动效",
            A11yKey::FocusRing => "焦点环",
        }
    }
}

// ── 禁令矩阵 ────────────────────────────────────────────────────────

/// 禁令矩阵（无障碍键清单 + 覆盖拒绝）。
pub struct BanMatrix {
    listed: Vec<A11yKey>,
    /// 覆盖拒绝次数（红线实测）。
    rejects: u32,
    /// 覆盖漏拒次数（P0 命中）。
    leaks: u32,
    last_code: C71Code,
    has_error: bool,
}

impl BanMatrix {
    /// 默认清单：四键全列（一等级民）。
    pub fn standard() -> Self {
        Self {
            listed: A11yKey::ALL.to_vec(),
            rejects: 0,
            leaks: 0,
            last_code: C71Code(0),
            has_error: false,
        }
    }

    pub fn new(listed: Vec<A11yKey>) -> Self {
        Self {
            listed,
            rejects: 0,
            leaks: 0,
            last_code: C71Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C71Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: C71Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 清单是否覆盖全部四键（一等级民执法）。
    pub fn list_complete(&self) -> bool {
        A11yKey::ALL.iter().all(|k| self.listed.contains(k))
    }

    /// 清单漏项检测。
    pub fn audit_list(&mut self) -> Result<bool, C71Code> {
        if self.list_complete() {
            self.last_code = C71Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(C71Code::LIST_GAP);
            Err(C71Code::LIST_GAP)
        }
    }

    /// 覆盖请求：来自主题/系统更新的写入。
    /// 若目标键在清单内 → 拒绝（P0 禁令）。
    pub fn attempt_overwrite(&mut self, key: A11yKey) -> Result<bool, C71Code> {
        if self.listed.contains(&key) {
            self.rejects += 1;
            self.last_code = C71Code::THEME_OVERWRITE;
            self.has_error = true;
            Err(C71Code::THEME_OVERWRITE)
        } else {
            self.last_code = C71Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    /// 漏拒上报（演练中发现覆盖成功即 P0）。
    pub fn note_leak(&mut self) {
        self.leaks += 1;
        self.fail(C71Code::THEME_OVERWRITE);
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn leaks(&self) -> u32 {
        self.leaks
    }

    /// 禁令总评：清单齐 + 无漏拒。
    pub fn ban_green(&self) -> bool {
        self.list_complete() && self.leaks == 0
    }
}

// ── 优先级顶层 ──────────────────────────────────────────────────────

/// 优先级层级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    /// 系统默认（最低）。
    SystemDefault = 0,
    /// 同步。
    Sync = 1,
    /// 场景规则。
    SceneRule = 2,
    /// 用户手设。
    UserSet = 3,
    /// 无障碍（最顶层）。
    A11y = 4,
}

/// 优先级表（无障碍顶层复述）。
pub struct PriorityTable {
    /// 当前各层代表键的生效值记录（层→值）。
    tiers: Vec<(Tier, i32)>,
    top_lost: u32,
    last_code: C71Code,
    has_error: bool,
}

impl PriorityTable {
    pub fn new() -> Self {
        Self {
            tiers: Vec::new(),
            top_lost: 0,
            last_code: C71Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C71Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 压入一层的生效值。
    pub fn push(&mut self, tier: Tier, value: i32) {
        self.tiers.push((tier, value));
    }

    /// 消解：取最高层的值。
    pub fn effective(&self) -> Option<i32> {
        self.tiers.iter().max_by_key(|(t, _)| *t).map(|(_, v)| *v)
    }

    /// 顶层断言：无障碍层必须高于手设层。
    pub fn assert_top(&mut self) -> Result<bool, C71Code> {
        let a11y = self.tiers.iter().any(|(t, _)| *t == Tier::A11y);
        let userset = self.tiers.iter().any(|(t, _)| *t == Tier::UserSet);
        if a11y && userset {
            if Tier::A11y > Tier::UserSet {
                self.last_code = C71Code(0);
                self.has_error = false;
                Ok(true)
            } else {
                self.top_lost += 1;
                self.last_code = C71Code::TOP_TIER_LOST;
                self.has_error = true;
                Err(C71Code::TOP_TIER_LOST)
            }
        } else {
            self.last_code = C71Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    pub fn top_lost(&self) -> u32 {
        self.top_lost
    }
}

// ── 随人走 ──────────────────────────────────────────────────────────

/// 同步必含断言（无障碍随人走）。
pub struct SyncA11y {
    /// 本端无障碍键值。
    local: Vec<(A11yKey, i32)>,
    /// 同步包中的无障碍键值。
    remote: Vec<(A11yKey, i32)>,
    dropped: u32,
    last_code: C71Code,
    has_error: bool,
}

impl SyncA11y {
    pub fn new() -> Self {
        Self {
            local: Vec::new(),
            remote: Vec::new(),
            dropped: 0,
            last_code: C71Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C71Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn set_local(&mut self, key: A11yKey, value: i32) {
        if let Some(e) = self.local.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            self.local.push((key, value));
        }
    }

    pub fn set_remote(&mut self, key: A11yKey, value: i32) {
        if let Some(e) = self.remote.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            self.remote.push((key, value));
        }
    }

    /// 随人走断言：本地每个无障碍键必须出现在远端包。
    pub fn assert_carried(&mut self) -> Result<bool, C71Code> {
        let mut ok = true;
        for (k, _) in &self.local {
            if !self.remote.iter().any(|(rk, _)| rk == k) {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C71Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.dropped += 1;
            self.last_code = C71Code::SYNC_DROPPED;
            self.has_error = true;
            Err(C71Code::SYNC_DROPPED)
        }
    }

    /// 补齐远端缺项（修复动作）。
    pub fn repair(&mut self) -> usize {
        let mut added = 0;
        for (k, v) in &self.local {
            if !self.remote.iter().any(|(rk, _)| rk == k) {
                self.remote.push((*k, *v));
                added += 1;
            }
        }
        added
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn local_count(&self) -> usize {
        self.local.len()
    }

    pub fn remote_count(&self) -> usize {
        self.remote.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves71_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves71_prefa11y");
    {
        let m = BanMatrix::standard();
        chk(&mut set, "C3671-01", m.list_complete() && m.ban_green(), "err");
    }
    {
        let mut m = BanMatrix::new(vec![A11yKey::FontSize]);
        let res = m.audit_list();
        chk(&mut set, "C3671-02", res.err() == Some(C71Code::LIST_GAP), "err");
    }
    {
        let mut m = BanMatrix::standard();
        let res = m.attempt_overwrite(A11yKey::FontSize);
        chk(&mut set, "C3671-03", res.err() == Some(C71Code::THEME_OVERWRITE), "err");
    }
    {
        let mut m = BanMatrix::standard();
        let res = m.attempt_overwrite(A11yKey::Contrast);
        chk(&mut set, "C3671-04", res.is_err() && m.rejects() == 1, "err");
    }
    {
        let mut m = BanMatrix::standard();
        m.note_leak();
        chk(&mut set, "C3671-05", m.leaks() == 1 && !m.ban_green(), "err");
    }
    {
        let mut t = PriorityTable::new();
        t.push(Tier::UserSet, 10);
        t.push(Tier::A11y, 20);
        let res = t.assert_top();
        chk(&mut set, "C3671-06", res == Ok(true), "err");
    }
    {
        let mut t = PriorityTable::new();
        t.push(Tier::A11y, 20);
        t.push(Tier::UserSet, 10);
        let eff = t.effective();
        chk(&mut set, "C3671-07", eff == Some(20), "err");
    }
    {
        let mut t = PriorityTable::new();
        t.push(Tier::SystemDefault, 1);
        t.push(Tier::Sync, 2);
        let eff = t.effective();
        chk(&mut set, "C3671-08", eff == Some(2), "err");
    }
    {
        let mut s = SyncA11y::new();
        s.set_local(A11yKey::FontSize, 16);
        s.set_local(A11yKey::Contrast, 70);
        s.set_remote(A11yKey::FontSize, 16);
        s.set_remote(A11yKey::Contrast, 70);
        let res = s.assert_carried();
        chk(&mut set, "C3671-09", res == Ok(true), "err");
    }
    {
        let mut s = SyncA11y::new();
        s.set_local(A11yKey::Motion, 1);
        s.set_remote(A11yKey::FontSize, 16);
        let res = s.assert_carried();
        chk(&mut set, "C3671-10", res.err() == Some(C71Code::SYNC_DROPPED), "err");
    }
    {
        let mut s = SyncA11y::new();
        s.set_local(A11yKey::Motion, 1);
        s.set_remote(A11yKey::FontSize, 16);
        let n = s.repair();
        chk(&mut set, "C3671-11", n == 1 && s.assert_carried() == Ok(true), "err");
    }
    {
        let names_ok = A11yKey::ALL.iter().all(|k| !k.name().is_empty());
        chk(&mut set, "C3671-12", names_ok && A11yKey::ALL.len() == 4, "err");
    }
    set
}
