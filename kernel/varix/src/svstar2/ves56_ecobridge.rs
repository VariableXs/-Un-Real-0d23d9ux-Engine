//! VE-F3656 · 视觉创作与 E 域壁纸深化（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3656`
//!
//! **职责定位（锚点原文）**：工坊产物 × E 域壁纸运行时深化协同——
//! - **产消协议终版**：工坊产物 → E 域壁纸格式（消费格式双签）；
//! - **约束回传**：E 域运行时性能约束 → 工坊创作约束（约束回传红线：
//!   运行时约束不回传 = 创作出跑不动的壁纸）；
//! - **协同测试**：产物兼容矩阵（复述矩阵）；
//! - **联动调试**：工坊预览 × 运行时表现差异调试（复用保真）。
//!
//! **判据（锚点原文六条）**：产消双签、约束回传、兼容矩阵、
//! 保真复用、深化协同、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 约束不回传→P1（红线实测） | [`W56Code::CONSTRAINT_NOT_RETURNED`] |
//! | 产物不兼容→矩阵拦截（复述） | [`W56Code::INCOMPATIBLE`] |
//! | 双签缺→协议不冻结（复述） | [`W56Code::SIGN_MISSING`] |
//! | 保真分歧→对拍（复用） | [`W56Code::FIDELITY_DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3656 诊断码段（独占 0x68xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct W56Code(pub u16);

impl W56Code {
    /// 约束未回传（P1 红线实测）。
    pub const CONSTRAINT_NOT_RETURNED: W56Code = W56Code(0x6801);
    /// 产物不兼容（矩阵拦截）。
    pub const INCOMPATIBLE: W56Code = W56Code(0x6802);
    /// 双签缺失（协议不冻结）。
    pub const SIGN_MISSING: W56Code = W56Code(0x6803);
    /// 保真分歧（对拍）。
    pub const FIDELITY_DRIFT: W56Code = W56Code(0x6804);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            W56Code::CONSTRAINT_NOT_RETURNED => "运行时约束未回传（红线实测 P1）。建议：回传创作约束。",
            W56Code::INCOMPATIBLE => "产物不兼容（矩阵拦截）。建议：按兼容矩阵整改。",
            W56Code::SIGN_MISSING => "产消双签缺失。建议：双方签齐再冻结协议。",
            W56Code::FIDELITY_DRIFT => "保真分歧。建议：与运行时表现对拍。",
            _ => "未知诊断码。",
        }
    }
}

// ── 产消双签 ────────────────────────────────────────────────────────

/// 签署方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Party {
    /// 工坊（生产方）。
    Workshop,
    /// E 域壁纸运行时（消费方）。
    Runtime,
}

/// 双签结构。
#[derive(Clone, Copy)]
pub struct DualSign {
    pub workshop_signed: bool,
    pub runtime_signed: bool,
    pub format_rev: u16,
}

impl DualSign {
    /// 协议冻结条件：双方签齐。
    pub fn frozen(&self) -> bool {
        self.workshop_signed && self.runtime_signed
    }
}

// ── 约束回传 ────────────────────────────────────────────────────────

/// 运行时约束（性能）。
#[derive(Clone, Copy)]
pub struct RuntimeConstraint {
    /// GPU 预算（毫瓦）。
    pub power_mw: u32,
    /// 最大帧率。
    pub max_fps: u32,
    /// 显存预算（MiB）。
    pub vram_mib: u32,
}

/// 回传后的创作约束。
#[derive(Clone, Copy)]
pub struct AuthoringLimit {
    pub max_layers: u32,
    pub max_effects: u32,
    pub target_fps: u32,
}

impl AuthoringLimit {
    /// 从运行时约束推导创作限（回传协议）。
    pub fn from_runtime(c: &RuntimeConstraint) -> Self {
        let max_layers = if c.vram_mib >= 512 { 24 } else if c.vram_mib >= 256 { 12 } else { 6 };
        let max_effects = if c.power_mw >= 800 { 8 } else { 4 };
        Self { max_layers, max_effects, target_fps: c.max_fps.min(60) }
    }
}

// ── 兼容矩阵 ────────────────────────────────────────────────────────

/// 产物档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 小（百级资产）。
    Small,
    /// 中（千级资产）。
    Medium,
    /// 大（万级资产）。
    Large,
}

/// 兼容条目。
#[derive(Clone, Copy)]
pub struct CompatCell {
    pub tier: Tier,
    /// 是否兼容当前运行时。
    pub compatible: bool,
}

// ── 协同面板 ────────────────────────────────────────────────────────

/// 视觉创作 × E 域深化协同面板。
pub struct EcoBridge {
    sign: DualSign,
    constraint: Option<RuntimeConstraint>,
    limit: Option<AuthoringLimit>,
    matrix: Vec<CompatCell>,
    /// 保真对拍：预览指纹 vs 运行时指纹。
    fidelity_hits: u32,
    last_code: W56Code,
    has_error: bool,
}

impl EcoBridge {
    pub fn new(format_rev: u16) -> Self {
        Self {
            sign: DualSign { workshop_signed: false, runtime_signed: false, format_rev },
            constraint: None,
            limit: None,
            matrix: Vec::new(),
            fidelity_hits: 0,
            last_code: W56Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> W56Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: W56Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 签署。
    pub fn sign(&mut self, party: Party) -> Result<bool, W56Code> {
        match party {
            Party::Workshop => self.sign.workshop_signed = true,
            Party::Runtime => self.sign.runtime_signed = true,
        }
        self.last_code = W56Code(0);
        self.has_error = false;
        Ok(self.sign.frozen())
    }

    /// 协议冻结审计。
    pub fn frozen(&mut self) -> Result<bool, W56Code> {
        if self.sign.frozen() {
            self.last_code = W56Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(W56Code::SIGN_MISSING);
            Err(W56Code::SIGN_MISSING)
        }
    }

    /// 约束回传：运行时 → 创作限。
    pub fn return_constraint(&mut self, c: RuntimeConstraint) -> Result<AuthoringLimit, W56Code> {
        let limit = AuthoringLimit::from_runtime(&c);
        self.constraint = Some(c);
        self.limit = Some(limit);
        self.last_code = W56Code(0);
        self.has_error = false;
        Ok(limit)
    }

    /// 回传审计：限已推导（红线：不可缺回传）。
    pub fn constraint_returned(&mut self) -> Result<bool, W56Code> {
        if self.limit.is_some() {
            self.last_code = W56Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(W56Code::CONSTRAINT_NOT_RETURNED);
            Err(W56Code::CONSTRAINT_NOT_RETURNED)
        }
    }

    /// 填兼容矩阵行。
    pub fn add_compat(&mut self, tier: Tier, compatible: bool) -> usize {
        self.matrix.push(CompatCell { tier, compatible });
        self.matrix.len()
    }

    /// 矩阵拦截：不兼容即拒。
    pub fn admit(&mut self, tier: Tier) -> Result<bool, W56Code> {
        match self.matrix.iter().find(|c| c.tier == tier) {
            Some(c) if c.compatible => {
                self.last_code = W56Code(0);
                self.has_error = false;
                Ok(true)
            }
            _ => {
                self.fail(W56Code::INCOMPATIBLE);
                Err(W56Code::INCOMPATIBLE)
            }
        }
    }

    /// 保真对拍。
    pub fn verify_fidelity(&mut self, preview_fp: u64, runtime_fp: u64) -> Result<bool, W56Code> {
        if preview_fp != runtime_fp {
            self.fidelity_hits += 1;
            self.fail(W56Code::FIDELITY_DRIFT);
            return Err(W56Code::FIDELITY_DRIFT);
        }
        self.last_code = W56Code(0);
        self.has_error = false;
        Ok(true)
    }

    pub fn fidelity_hits(&self) -> u32 {
        self.fidelity_hits
    }

    pub fn matrix_len(&self) -> usize {
        self.matrix.len()
    }

    pub fn limit(&self) -> Option<AuthoringLimit> {
        self.limit
    }

    /// 深化协同总评：双签 + 回传 + 矩阵非空。
    pub fn bridge_green(&mut self) -> bool {
        self.sign.frozen() && self.limit.is_some() && !self.matrix.is_empty()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves56_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves56_ecobridge");
    {
        let b = EcoBridge::new(1);
        chk(&mut set, "C3656-01", b.matrix_len() == 0 && b.fidelity_hits() == 0, "err");
    }
    {
        let mut b = EcoBridge::new(1);
        let r = b.sign(Party::Workshop);
        chk(&mut set, "C3656-02", r == Ok(false), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.sign(Party::Workshop);
        b.sign(Party::Runtime);
        let r = b.frozen();
        chk(&mut set, "C3656-03", r == Ok(true), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.sign(Party::Workshop);
        let r = b.frozen();
        chk(&mut set, "C3656-04", r.err() == Some(W56Code::SIGN_MISSING), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        let c = RuntimeConstraint { power_mw: 900, max_fps: 60, vram_mib: 512 };
        let r = b.return_constraint(c);
        chk(&mut set, "C3656-05", r.unwrap().max_layers == 24, "err");
    }
    {
        let mut b = EcoBridge::new(1);
        let r = b.constraint_returned();
        chk(&mut set, "C3656-06", r.err() == Some(W56Code::CONSTRAINT_NOT_RETURNED), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.return_constraint(RuntimeConstraint { power_mw: 500, max_fps: 30, vram_mib: 128 });
        let r = b.constraint_returned();
        chk(&mut set, "C3656-07", r == Ok(true), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.add_compat(Tier::Small, true);
        let r = b.admit(Tier::Small);
        chk(&mut set, "C3656-08", r == Ok(true), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.add_compat(Tier::Large, false);
        let r = b.admit(Tier::Large);
        chk(&mut set, "C3656-09", r.err() == Some(W56Code::INCOMPATIBLE), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        let r = b.verify_fidelity(0xAA, 0xAA);
        chk(&mut set, "C3656-10", r == Ok(true), "err");
    }
    {
        let mut b = EcoBridge::new(1);
        let r = b.verify_fidelity(0xAA, 0xBB);
        chk(&mut set, "C3656-11", r.err() == Some(W56Code::FIDELITY_DRIFT) && b.fidelity_hits() == 1, "err");
    }
    {
        let mut b = EcoBridge::new(1);
        b.sign(Party::Workshop);
        b.sign(Party::Runtime);
        b.return_constraint(RuntimeConstraint { power_mw: 800, max_fps: 60, vram_mib: 256 });
        b.add_compat(Tier::Medium, true);
        chk(&mut set, "C3656-12", b.bridge_green(), "err");
    }
    set
}
