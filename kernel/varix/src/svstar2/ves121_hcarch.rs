//! VE-F3821 · 高对比引擎总架构
//!
//! 三层架构：主题层（高对比令牌集）→ 映射层（语义色→高对比色）→ 渲染层（后处理强化）。
//! 六域映射：forced-colors / 色弱 / 色盲 / 对比度实时 / 文字强化 / 图形替代。
//! 验证执行分工：F2895 门禁是验证者、S02 是执行者。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA9xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA9Code(pub u16);

impl CA9Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const LAYER_MISMATCH: Self = Self(SEG_BASE + 1);
    pub const DOMAIN_MISSING: Self = Self(SEG_BASE + 2);
    pub const ROLE_MIXED: Self = Self(SEG_BASE + 3);
    pub const SIGNATURE_DRIFT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LAYER_MISMATCH => "层间失配，需对拍",
            Self::DOMAIN_MISSING => "映射缺域，需补齐",
            Self::ROLE_MIXED => "验证执行混层（分工红线实测）",
            Self::SIGNATURE_DRIFT => "签名漂移，需冻结修正",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 三层架构段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Theme,
    Map,
    Render,
}

/// 三层签名（冻结）
#[derive(Debug, Clone, Copy)]
pub struct LayerSignature {
    pub layer: Layer,
    pub version: u16,
    pub frozen: bool,
}

/// 六域
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptDomain {
    ForcedColors,
    ColorWeak,
    ColorBlind,
    ContrastRealtime,
    TextBoost,
    GraphicAlt,
}

/// 验证执行分工
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Verifier,
    Executor,
}

/// 分工声明
#[derive(Debug, Clone, Copy)]
pub struct RoleDecl {
    pub f2895_role: Role,
    pub s02_role: Role,
}

impl RoleDecl {
    pub fn standard() -> Self {
        Self { f2895_role: Role::Verifier, s02_role: Role::Executor }
    }

    /// 分工红线：验证执行不得混层
    pub fn assert_separated(&self) -> Result<(), CA9Code> {
        if self.f2895_role == self.s02_role {
            Err(CA9Code::ROLE_MIXED)
        } else {
            Ok(())
        }
    }
}

/// 映射项
#[derive(Debug, Clone)]
pub struct MapEntry {
    pub domain: AdaptDomain,
    pub semantic_id: u16,
    pub hc_color: u32,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcArch {
    signatures: Vec<LayerSignature>,
    mappings: Vec<MapEntry>,
    role: RoleDecl,
}

impl HcArch {
    pub fn new(role: RoleDecl) -> Self {
        Self {
            signatures: Vec::new(),
            mappings: Vec::new(),
            role,
        }
    }

    pub fn freeze_layer(&mut self, layer: Layer, version: u16) {
        self.signatures.push(LayerSignature { layer, version, frozen: true });
    }

    /// 三层齐备断言
    pub fn assert_three_layers(&self) -> Result<(), CA9Code> {
        let has_theme = self.signatures.iter().any(|s| s.layer == Layer::Theme);
        let has_map = self.signatures.iter().any(|s| s.layer == Layer::Map);
        let has_render = self.signatures.iter().any(|s| s.layer == Layer::Render);
        if has_theme && has_map && has_render {
            Ok(())
        } else {
            Err(CA9Code::LAYER_MISMATCH)
        }
    }

    /// 签名冻结断言
    pub fn assert_frozen(&self) -> Result<(), CA9Code> {
        if self.signatures.iter().all(|s| s.frozen) {
            Ok(())
        } else {
            Err(CA9Code::SIGNATURE_DRIFT)
        }
    }

    pub fn add_mapping(&mut self, entry: MapEntry) {
        self.mappings.push(entry);
    }

    /// 六域覆盖断言
    pub fn assert_six_domains(&self) -> Result<(), CA9Code> {
        let domains = [
            AdaptDomain::ForcedColors,
            AdaptDomain::ColorWeak,
            AdaptDomain::ColorBlind,
            AdaptDomain::ContrastRealtime,
            AdaptDomain::TextBoost,
            AdaptDomain::GraphicAlt,
        ];
        for d in domains {
            if !self.mappings.iter().any(|m| m.domain == d) {
                return Err(CA9Code::DOMAIN_MISSING);
            }
        }
        Ok(())
    }

    /// 分工断言
    pub fn assert_role(&self) -> Result<(), CA9Code> {
        self.role.assert_separated()
    }

    /// 层间对拍：每层至少一个签名
    pub fn layer_count(&self) -> usize {
        self.signatures.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves121_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves121_hcarch");

    // 01 三层齐备
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.freeze_layer(Layer::Theme, 1);
        a.freeze_layer(Layer::Map, 1);
        a.freeze_layer(Layer::Render, 1);
        chk(&mut set, "C3821-01", a.assert_three_layers().is_ok(), "err");
    }
    // 02 缺层
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.freeze_layer(Layer::Theme, 1);
        chk(&mut set, "C3821-02", a.assert_three_layers() == Err(CA9Code::LAYER_MISMATCH), "err");
    }
    // 03 签名冻结
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.freeze_layer(Layer::Theme, 1);
        a.freeze_layer(Layer::Map, 1);
        a.freeze_layer(Layer::Render, 1);
        chk(&mut set, "C3821-03", a.assert_frozen().is_ok(), "err");
    }
    // 04 六域全
    {
        let mut a = HcArch::new(RoleDecl::standard());
        let domains = [
            AdaptDomain::ForcedColors, AdaptDomain::ColorWeak, AdaptDomain::ColorBlind,
            AdaptDomain::ContrastRealtime, AdaptDomain::TextBoost, AdaptDomain::GraphicAlt,
        ];
        for (i, d) in domains.iter().enumerate() {
            a.add_mapping(MapEntry { domain: *d, semantic_id: i as u16, hc_color: 0xFFFFFF });
        }
        chk(&mut set, "C3821-04", a.assert_six_domains().is_ok(), "err");
    }
    // 05 缺域
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.add_mapping(MapEntry { domain: AdaptDomain::ForcedColors, semantic_id: 0, hc_color: 0 });
        chk(&mut set, "C3821-05", a.assert_six_domains() == Err(CA9Code::DOMAIN_MISSING), "err");
    }
    // 06 分工混层
    {
        let role = RoleDecl { f2895_role: Role::Executor, s02_role: Role::Executor };
        let a = HcArch::new(role);
        chk(&mut set, "C3821-06", a.assert_role() == Err(CA9Code::ROLE_MIXED), "err");
    }
    // 07 分工正常
    {
        let a = HcArch::new(RoleDecl::standard());
        chk(&mut set, "C3821-07", a.assert_role().is_ok(), "err");
    }
    // 08 层计数
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.freeze_layer(Layer::Theme, 1);
        a.freeze_layer(Layer::Map, 1);
        chk(&mut set, "C3821-08", a.layer_count() == 2, "err");
    }
    // 09 映射添加
    {
        let mut a = HcArch::new(RoleDecl::standard());
        a.add_mapping(MapEntry { domain: AdaptDomain::ColorWeak, semantic_id: 5, hc_color: 0x00FF00 });
        chk(&mut set, "C3821-09", a.mappings.len() == 1, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3821-10", !CA9Code::ROLE_MIXED.explain().is_empty(), "err");
    }

    set
}
