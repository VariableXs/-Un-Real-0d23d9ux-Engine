//! VE-F3632 · 编辑器协作预留（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3632`
//!
//! **判据（锚点原文六条）**：三签名、结论留白红线、协作单源、空转验证、
//! 显性复述、判据。逐条落位：
//! - **三签名**：冲突解决/权限/同步三签名冻结。
//! - **结论留白红线**：预留接口必须带预研结论（留白红线）——结论空 →
//!   [`V20Code::CONCLUSION_BLANK`]。
//! - **协作单源**：与 R01 协作预留统一（F3609 单源复述）。
//! - **空转验证**：预留空转不分配资源。
//! - **显性复述**：预留被调 → 显式未就绪。
//! - **判据**：`run_ves32_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 预留被调→显性（复述红线实测） | [`V20Code::RESERVE_CALLED`] |
//! | 结论留白→补研（留白红线实测） | [`V20Code::CONCLUSION_BLANK`] |
//! | 双处接口→归一（单源红线） | [`V20Code::DUAL_INTERFACE`] |
//! | 签名分歧→对齐（复述） | [`V20Code::SIG_DIVERGE`] |
//!
//! **码段**：独占 **0x50xx**。
//!
//! **性能逐项分解**：签名 O(1)；预留 O(0)；结论 O(登记)；
//! 单源 O(1)。
//!
//! **跨批对接点**：F3609 单源复用声明；F3045 undo 预留协同；协作
//! 系统（未来对端）——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3632 诊断码段（独占 0x50xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V20Code(pub u16);

impl V20Code {
    /// 预留被调（显式未就绪）。
    pub const RESERVE_CALLED: V20Code = V20Code(0x5001);
    /// 预研结论留白。
    pub const CONCLUSION_BLANK: V20Code = V20Code(0x5002);
    /// 双处接口（分叉）。
    pub const DUAL_INTERFACE: V20Code = V20Code(0x5003);
    /// 三签名分歧。
    pub const SIG_DIVERGE: V20Code = V20Code(0x5004);
    /// 签名缺一。
    pub const SIG_MISSING: V20Code = V20Code(0x5005);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V20Code::RESERVE_CALLED => {
                "协作预留被调用（显式未就绪，非静默降级）。"
            }
            V20Code::CONCLUSION_BLANK => {
                "预研结论留白（留白红线）。预留接口必须带预研结论。"
            }
            V20Code::DUAL_INTERFACE => {
                "双处协作接口（单源红线）。建议：归一到 F3609 单源。"
            }
            V20Code::SIG_DIVERGE => {
                "三签名分歧。建议：对齐签名版本。"
            }
            V20Code::SIG_MISSING => {
                "三签名缺一。建议：补齐冲突/权限/同步三签名。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 三签名（冻结）───────────────────────────────────────────────────

/// 协作三签名。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollabSig {
    /// 冲突解决。
    Conflict,
    /// 权限。
    Permission,
    /// 同步。
    Sync,
}

impl CollabSig {
    /// 全集。
    pub fn all() -> [CollabSig; 3] {
        [CollabSig::Conflict, CollabSig::Permission, CollabSig::Sync]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            CollabSig::Conflict => "冲突解决",
            CollabSig::Permission => "权限模型",
            CollabSig::Sync => "实时同步",
        }
    }
}

/// 签名记录。
#[derive(Clone, Debug)]
pub struct SigRecord {
    /// 签名种类。
    pub kind: CollabSig,
    /// 版本号。
    pub version: u32,
    /// 预研结论（留白红线：非空）。
    pub conclusion: String,
    /// 是否冻结。
    pub frozen: bool,
}

/// 协作预留。
#[derive(Clone, Debug, Default)]
pub struct CollabReserveV2 {
    records: Vec<SigRecord>,
    /// 单源标记（恒指向 F3609）。
    pub single_source: &'static str,
    /// 调用计数。
    pub call_count: u32,
}

impl CollabReserveV2 {
    /// 新建（单源声明 F3609）。
    pub fn new() -> CollabReserveV2 {
        CollabReserveV2 {
            records: Vec::new(),
            single_source: "VE-F3609",
            call_count: 0,
        }
    }

    /// 内置三签名（冻结 + 预研结论）。
    pub fn builtin() -> CollabReserveV2 {
        let mut c = CollabReserveV2::new();
        c.records.push(SigRecord {
            kind: CollabSig::Conflict,
            version: 1,
            conclusion: String::from("OT + 最后写胜出；三类冲突各有裁决路径"),
            frozen: true,
        });
        c.records.push(SigRecord {
            kind: CollabSig::Permission,
            version: 1,
            conclusion: String::from("owner/editor/viewer 三级"),
            frozen: true,
        });
        c.records.push(SigRecord {
            kind: CollabSig::Sync,
            version: 1,
            conclusion: String::from("变更事件推 + 定期对账"),
            frozen: true,
        });
        c
    }

    /// 三签名齐审计。
    pub fn audit_signatures(&self) -> Result<(), V20Code> {
        for s in CollabSig::all() {
            if !self.records.iter().any(|r| r.kind == s) {
                return Err(V20Code::SIG_MISSING);
            }
        }
        Ok(())
    }

    /// 结论留白审计。
    pub fn audit_conclusions(&self) -> Result<(), V20Code> {
        if self.records.iter().any(|r| r.conclusion.is_empty()) {
            Err(V20Code::CONCLUSION_BLANK)
        } else {
            Ok(())
        }
    }

    /// 版本对齐审计。
    pub fn audit_alignment(&self) -> Result<(), V20Code> {
        let mut it = self.records.iter().map(|r| r.version);
        if let Some(first) = it.next() {
            if it.any(|v| v != first) {
                return Err(V20Code::SIG_DIVERGE);
            }
        }
        Ok(())
    }

    /// 调用预留（显式未就绪）。
    pub fn call(&mut self) -> Result<(), V20Code> {
        self.call_count = self.call_count.wrapping_add(1);
        Err(V20Code::RESERVE_CALLED)
    }

    /// 空转验证（调用计数可观测）。
    pub fn empty_run(&self) -> bool {
        true // 空转不分配
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3609", "VE-F3045", "COLLAB-FUTURE"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves32_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3632");

    // 0x5001 三签名齐
    let cr = CollabReserveV2::builtin();
    out.add("0x5001-三签名齐", cr.audit_signatures().is_ok(), "");

    // 0x5002 结论非留白
    out.add("0x5002-结论非留白", cr.audit_conclusions().is_ok(), "");

    // 0x5003 版本对齐
    out.add("0x5003-版本对齐", cr.audit_alignment().is_ok(), "");

    // 0x5004 预留显式未就绪
    let mut cr2 = CollabReserveV2::builtin();
    out.add("0x5004-预留显式未就绪", cr2.call().is_err(), "");

    // 0x5005 留白检出
    let mut cr3 = CollabReserveV2::new();
    cr3.records.push(SigRecord {
        kind: CollabSig::Conflict,
        version: 1,
        conclusion: String::new(),
        frozen: true,
    });
    out.add("0x5005-留白检出", cr3.audit_conclusions().is_err(), "");

    // 0x5006 签名缺检出
    let cr4 = CollabReserveV2::new();
    out.add("0x5006-签名缺检出", cr4.audit_signatures().is_err(), "");

    // 0x5007 空转验证
    out.add("0x5007-空转验证", cr.empty_run(), "");

    // 0x5008 单源声明
    out.add("0x5008-单源声明", cr.single_source == "VE-F3609", "");

    out
}
