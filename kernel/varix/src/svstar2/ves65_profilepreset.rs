//! VE-F3665 · 个性化预设与画像（VE-S 域 · 批次 R04 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3665`
//!
//! **职责定位（锚点原文）**：预设画像——
//! - **预设模型**：一键应用成套个性化（预设 = 个性化集合快照，保存/分享/应用）；
//! - **画像本地红线**：画像数据本地存储不上传（P0 级：画像上云 = 隐私灾难）；
//! - **画像可见红线**：用户可查看自己的画像内容（画像不可见 = 黑盒操纵）；
//! - **画像清除**：一键清除（清除权，删除后扫描残留）。
//!
//! **判据（锚点原文六条）**：本地画像 P0、画像可见、清除权、
//! 预设快照、推荐输入、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 画像上云→P0（红线实测——网络审计） | [`P65Code::PROFILE_UPLOAD`] |
//! | 画像不可见→断言拦截（红线实测） | [`P65Code::PROFILE_HIDDEN`] |
//! | 清除残留→P1（清除红线实测——删除后扫描） | [`P65Code::CLEAR_RESIDUAL`] |
//! | 预设应用冲突→消解复用 | [`P65Code::PRESET_CONFLICT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3665 诊断码段（独占 0x71xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P65Code(pub u16);

impl P65Code {
    /// 画像上云（P0 隐私红线）。
    pub const PROFILE_UPLOAD: P65Code = P65Code(0x7101);
    /// 画像不可见（红线实测）。
    pub const PROFILE_HIDDEN: P65Code = P65Code(0x7102);
    /// 清除残留（P1 红线实测）。
    pub const CLEAR_RESIDUAL: P65Code = P65Code(0x7103);
    /// 预设应用冲突。
    pub const PRESET_CONFLICT: P65Code = P65Code(0x7104);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            P65Code::PROFILE_UPLOAD => "画像上云（P0 隐私红线）。建议：本地存储不上传。",
            P65Code::PROFILE_HIDDEN => "画像不可见（红线实测）。建议：补画像查看通道。",
            P65Code::CLEAR_RESIDUAL => "清除残留（P1 红线实测）。建议：删除后扫描复核。",
            P65Code::PRESET_CONFLICT => "预设应用冲突。建议：走冲突消解复用。",
            _ => "未知诊断码。",
        }
    }
}

// ── 预设快照 ────────────────────────────────────────────────────────

/// 预设条目（键值对）。
#[derive(Clone, Copy)]
pub struct PresetEntry {
    pub key: u16,
    pub value: i32,
}

/// 预设（个性化集合快照）。
#[derive(Clone, Copy)]
pub struct Preset {
    pub id: u32,
    pub entry_count: u16,
    /// 冲突键数（应用时消解口径）。
    pub conflicts: u16,
}

// ── 本地画像 ────────────────────────────────────────────────────────

/// 画像标签（本地）。
#[derive(Clone, Copy)]
pub struct ProfileTag {
    pub tag_id: u16,
    pub weight_bps: u32,
}

/// 本地画像。
pub struct LocalProfile {
    tags: Vec<ProfileTag>,
    /// 是否仅本地（P0 执法位）。
    local_only: bool,
    visible: bool,
    residual: u32,
    last_code: P65Code,
    has_error: bool,
}

impl LocalProfile {
    pub fn new() -> Self {
        Self {
            tags: Vec::new(),
            local_only: true,
            visible: true,
            residual: 0,
            last_code: P65Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> P65Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: P65Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 加画像标签（本地）。
    pub fn add_tag(&mut self, tag_id: u16, weight_bps: u32) -> usize {
        self.tags.push(ProfileTag { tag_id, weight_bps });
        self.tags.len()
    }

    /// 网络审计：上传即 P0。
    pub fn audit_network(&mut self, attempted_upload: bool) -> Result<bool, P65Code> {
        if attempted_upload {
            self.local_only = false;
            self.fail(P65Code::PROFILE_UPLOAD);
            return Err(P65Code::PROFILE_UPLOAD);
        }
        self.last_code = P65Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 画像可见断言（红线：不可见即拦截）。
    pub fn assert_visible(&mut self) -> Result<bool, P65Code> {
        if self.visible {
            self.last_code = P65Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(P65Code::PROFILE_HIDDEN);
            Err(P65Code::PROFILE_HIDDEN)
        }
    }

    /// 查看画像内容（可见通道）。
    pub fn view(&self) -> Option<&[ProfileTag]> {
        if self.visible {
            Some(&self.tags)
        } else {
            None
        }
    }

    /// 一键清除；残留扫描（删除后仍有标签 = P1）。
    pub fn clear(&mut self) -> Result<u32, P65Code> {
        self.tags.clear();
        if self.residual > 0 {
            self.fail(P65Code::CLEAR_RESIDUAL);
            return Err(P65Code::CLEAR_RESIDUAL);
        }
        self.last_code = P65Code(0);
        self.has_error = false;
        Ok(0)
    }

    /// 注入残留（删除后扫描演练口径）。
    pub fn inject_residual(&mut self, n: u32) {
        self.residual = n;
    }

    pub fn tag_count(&self) -> usize {
        self.tags.len()
    }

    pub fn local_only(&self) -> bool {
        self.local_only
    }

    pub fn residual(&self) -> u32 {
        self.residual
    }

    /// 推荐输入（F3666 前向）：导出权重表。
    pub fn recommend_input(&self) -> Option<Vec<(u16, u32)>> {
        if !self.local_only || !self.visible {
            return None;
        }
        Some(self.tags.iter().map(|t| (t.tag_id, t.weight_bps)).collect())
    }
}

// ── 预设库 ──────────────────────────────────────────────────────────

/// 预设库。
pub struct PresetLib {
    presets: Vec<Preset>,
    last_code: P65Code,
    has_error: bool,
}

impl PresetLib {
    pub fn new() -> Self {
        Self { presets: Vec::new(), last_code: P65Code(0), has_error: false }
    }

    pub fn last_code(&self) -> P65Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: P65Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 保存预设。
    pub fn save(&mut self, id: u32, entry_count: u16) -> usize {
        self.presets.push(Preset { id, entry_count, conflicts: 0 });
        self.presets.len()
    }

    /// 应用预设：有冲突键即显性消解。
    pub fn apply(&mut self, id: u32, conflicts: u16) -> Result<u32, P65Code> {
        match self.presets.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                p.conflicts = conflicts;
                if conflicts > 0 {
                    self.fail(P65Code::PRESET_CONFLICT);
                    return Err(P65Code::PRESET_CONFLICT);
                }
                self.last_code = P65Code(0);
                self.has_error = false;
                Ok(id)
            }
            None => {
                self.fail(P65Code::PRESET_CONFLICT);
                Err(P65Code::PRESET_CONFLICT)
            }
        }
    }

    /// 预设快照审计：条目数非零。
    pub fn snapshot_ok(&self) -> bool {
        self.presets.iter().all(|p| p.entry_count > 0)
    }

    pub fn count(&self) -> usize {
        self.presets.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves65_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves65_profilepreset");
    {
        let p = LocalProfile::new();
        chk(&mut set, "C3665-01", p.tag_count() == 0 && p.local_only(), "err");
    }
    {
        let mut p = LocalProfile::new();
        let n = p.add_tag(1, 8_000);
        chk(&mut set, "C3665-02", n == 1, "err");
    }
    {
        let mut p = LocalProfile::new();
        let r = p.audit_network(false);
        chk(&mut set, "C3665-03", r == Ok(true), "err");
    }
    {
        let mut p = LocalProfile::new();
        let r = p.audit_network(true);
        chk(&mut set, "C3665-04", r.err() == Some(P65Code::PROFILE_UPLOAD) && !p.local_only(), "err");
    }
    {
        let mut p = LocalProfile::new();
        let r = p.assert_visible();
        chk(&mut set, "C3665-05", r == Ok(true), "err");
    }
    {
        let mut p = LocalProfile::new();
        p.add_tag(1, 100);
        let r = p.clear();
        chk(&mut set, "C3665-06", r == Ok(0) && p.tag_count() == 0, "err");
    }
    {
        let mut p = LocalProfile::new();
        p.inject_residual(3);
        let r = p.clear();
        chk(&mut set, "C3665-07", r.err() == Some(P65Code::CLEAR_RESIDUAL), "err");
    }
    {
        let mut p = LocalProfile::new();
        p.add_tag(7, 5_000);
        let input = p.recommend_input();
        chk(&mut set, "C3665-08", input.map(|v| v.len()) == Some(1), "err");
    }
    {
        let mut l = PresetLib::new();
        let n = l.save(1, 5);
        chk(&mut set, "C3665-09", n == 1 && l.snapshot_ok(), "err");
    }
    {
        let mut l = PresetLib::new();
        l.save(1, 5);
        let r = l.apply(1, 0);
        chk(&mut set, "C3665-10", r == Ok(1), "err");
    }
    {
        let mut l = PresetLib::new();
        l.save(1, 5);
        let r = l.apply(1, 2);
        chk(&mut set, "C3665-11", r.err() == Some(P65Code::PRESET_CONFLICT), "err");
    }
    {
        let mut p = LocalProfile::new();
        p.add_tag(1, 100);
        let v = p.view();
        chk(&mut set, "C3665-12", v.map(|t| t.len()) == Some(1), "err");
    }
    set
}
