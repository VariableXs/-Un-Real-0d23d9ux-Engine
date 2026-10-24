//! VE-F3627 · 主题验证与发布（VE-S 域 · 批次 R02 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3627`
//!
//! **判据（锚点原文六条）**：素材必填、审批单源、语义版本、回滚复用、
//! 流水增强、判据。逐条落位：
//! - **素材必填**：无展示素材不可上架（素材规范红线）——
//!   [`PublishCheck::audit_assets`]，缺素材 → [`V1BCode::ASSET_MISSING`]。
//! - **审批单源**：发布审批复用 F3109 治理（复述单源）。
//! - **语义版本**：语义化版本发布（复述）。
//! - **回滚复用**：发布回滚复用回滚通道。
//! - **流水增强**：发布流水 = F3607 验证复用 + 发布增强。
//! - **判据**：`run_ves27_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 无素材上架→阻断（红线实测） | [`V1BCode::ASSET_MISSING`] |
//! | 绕审批→P0（复述） | [`V1BCode::APPROVAL_BYPASS`] |
//! | 回滚失败→P1（复用红线） | [`V1BCode::ROLLBACK_FAIL`] |
//! | 版本混乱→语义修正（复用） | [`V1BCode::SEMVER_CONFUSE`] |
//!
//! **码段**：独占 **0x4bxx**。
//!
//! **性能逐项分解**：流水 O(段)；素材 O(规范)；审批 O(复用)；
//! 回滚 O(1)。
//!
//! **跨批对接点**：F3607/F3109/F3203 三单源复用；F3616 分发对端；
//! F3114 评级——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3627 诊断码段（独占 0x4bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1BCode(pub u16);

impl V1BCode {
    /// 展示素材缺失。
    pub const ASSET_MISSING: V1BCode = V1BCode(0x4b01);
    /// 绕过审批（P0）。
    pub const APPROVAL_BYPASS: V1BCode = V1BCode(0x4b02);
    /// 回滚失败。
    pub const ROLLBACK_FAIL: V1BCode = V1BCode(0x4b03);
    /// 语义版本混乱。
    pub const SEMVER_CONFUSE: V1BCode = V1BCode(0x4b04);
    /// 验证未过即发布。
    pub const VERIFY_PENDING: V1BCode = V1BCode(0x4b05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1BCode::ASSET_MISSING => {
                "展示素材缺失（素材必填红线）。无素材不可上架。"
            }
            V1BCode::APPROVAL_BYPASS => {
                "绕过发布审批（P0）。所有发布必须走 F3109 审批通道。"
            }
            V1BCode::ROLLBACK_FAIL => {
                "回滚失败（P1）。建议：核对回滚通道状态。"
            }
            V1BCode::SEMVER_CONFUSE => {
                "语义版本混乱。建议：按 Major/Minor/Patch 规范递进。"
            }
            V1BCode::VERIFY_PENDING => {
                "验证未过即发布。建议：先通过 F3607 验证。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 素材规范 ────────────────────────────────────────────────────────

/// 展示素材类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShowcaseAsset {
    /// 主截图。
    Screenshot,
    /// 缩略图。
    Thumbnail,
    /// 预览 GIF。
    PreviewAnim,
}

impl ShowcaseAsset {
    /// 必填素材全集。
    pub fn required() -> [ShowcaseAsset; 3] {
        [
            ShowcaseAsset::Screenshot,
            ShowcaseAsset::Thumbnail,
            ShowcaseAsset::PreviewAnim,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ShowcaseAsset::Screenshot => "主截图",
            ShowcaseAsset::Thumbnail => "缩略图",
            ShowcaseAsset::PreviewAnim => "预览动画",
        }
    }
}

/// 素材规范检查。
#[derive(Clone, Debug, Default)]
pub struct PublishCheck {
    assets: Vec<ShowcaseAsset>,
    /// 验证是否已过（F3607）。
    pub verified: bool,
    /// 是否已审批。
    pub approved: bool,
}

impl PublishCheck {
    /// 新建。
    pub fn new() -> PublishCheck {
        PublishCheck {
            assets: Vec::new(),
            verified: false,
            approved: false,
        }
    }

    /// 交付素材。
    pub fn deliver(&mut self, asset: ShowcaseAsset) {
        if !self.assets.contains(&asset) {
            self.assets.push(asset);
        }
    }

    /// 素材审计（必填缺一阻断）。
    pub fn audit_assets(&self) -> Result<(), V1BCode> {
        for a in ShowcaseAsset::required() {
            if !self.assets.contains(&a) {
                return Err(V1BCode::ASSET_MISSING);
            }
        }
        Ok(())
    }

    /// 发布前综合门。
    pub fn gate(&self) -> Result<(), V1BCode> {
        self.audit_assets()?;
        if !self.verified {
            return Err(V1BCode::VERIFY_PENDING);
        }
        if !self.approved {
            return Err(V1BCode::APPROVAL_BYPASS);
        }
        Ok(())
    }
}

// ── 语义版本发布 ────────────────────────────────────────────────────

/// 发布版本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseVer {
    /// 重大。
    pub major: u32,
    /// 次要。
    pub minor: u32,
    /// 补丁。
    pub patch: u32,
}

impl ReleaseVer {
    /// 合法递进（单级）。
    pub fn is_valid_step(self, prev: ReleaseVer) -> bool {
        let p = ReleaseVer {
            patch: prev.patch + 1,
            ..prev
        };
        let m = ReleaseVer {
            minor: prev.minor + 1,
            patch: 0,
            ..prev
        };
        let j = ReleaseVer {
            major: prev.major + 1,
            minor: 0,
            patch: 0,
            ..prev
        };
        self == p || self == m || self == j
    }

    /// 版本审计。
    pub fn audit(&self, prev: ReleaseVer) -> Result<(), V1BCode> {
        if self.is_valid_step(prev) {
            Ok(())
        } else {
            Err(V1BCode::SEMVER_CONFUSE)
        }
    }
}

// ── 回滚通道（复用）─────────────────────────────────────────────────

/// 回滚器。
#[derive(Clone, Debug, Default)]
pub struct Rollback {
    pub target: Option<u32>,
    pub executed: bool,
}

impl Rollback {
    /// 新建。
    pub fn new() -> Rollback {
        Rollback {
            target: None,
            executed: false,
        }
    }

    /// 执行回滚。
    pub fn execute(&mut self, target: u32) -> Result<(), V1BCode> {
        if target == 0 {
            return Err(V1BCode::ROLLBACK_FAIL);
        }
        self.target = Some(target);
        self.executed = true;
        Ok(())
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 4] = ["VE-F3607", "VE-F3109", "VE-F3616", "VE-F3114"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves27_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3627");

    // 0x4b01 素材齐
    let mut pc = PublishCheck::new();
    for a in ShowcaseAsset::required() {
        pc.deliver(a);
    }
    pc.verified = true;
    pc.approved = true;
    out.add("0x4b01-素材齐", pc.gate().is_ok(), "");

    // 0x4b02 素材缺阻断
    let mut pc2 = PublishCheck::new();
    pc2.deliver(ShowcaseAsset::Screenshot);
    out.add("0x4b02-素材缺阻断", pc2.audit_assets().is_err(), "");

    // 0x4b03 未验证阻断
    let mut pc3 = PublishCheck::new();
    for a in ShowcaseAsset::required() {
        pc3.deliver(a);
    }
    pc3.approved = true;
    out.add("0x4b03-未验证阻断", pc3.gate().is_err(), "");

    // 0x4b04 绕审批阻断
    let mut pc4 = PublishCheck::new();
    for a in ShowcaseAsset::required() {
        pc4.deliver(a);
    }
    pc4.verified = true;
    out.add("0x4b04-绕审批阻断", pc4.gate().is_err(), "");

    // 0x4b05 版本递进
    let prev = ReleaseVer { major: 1, minor: 2, patch: 3 };
    let next = ReleaseVer { major: 1, minor: 2, patch: 4 };
    out.add("0x4b05-版本递进", next.audit(prev).is_ok(), "");

    // 0x4b06 版本跳跃
    let jump = ReleaseVer { major: 2, minor: 5, patch: 0 };
    out.add("0x4b06-版本跳跃", jump.audit(prev).is_err(), "");

    // 0x4b07 回滚
    let mut rb = Rollback::new();
    out.add("0x4b07-回滚成功", rb.execute(5).is_ok() && rb.executed, "");

    // 0x4b08 回滚目标非法
    let mut rb2 = Rollback::new();
    out.add("0x4b08-回滚目标非法", rb2.execute(0).is_err(), "");

    out
}
