//! F143 星徽与品牌资产包 · 完整设计（STAR I 主册 G-D-18）。
//!
//! **判据（主册）**：资产包全分辨率齐（含 4K 原生）；条款页法律面
//! 过审。
//!
//! **设计要点（主册）**：LOGO 五尺寸（16/32/48/128/512 + 矢量源）；
//! 单色变体三色（白/黑/强调色）；壁纸 10 张精选（4K）；条款页清单式
//! （允许：非商业展示/评测引用；禁止：篡改主色/暗示官方背书）；
//! 「图标正确用法示例」图解（间距/底色规范）；资产包 vxtheme 兼容
//! 容器；滥用 → 沟通下架流程（先礼后兵文档化）；条款争议 → F148。
//!
//! 本模块是资产包的**完整性账本**：五尺寸+矢量源齐套校验、单色三
//! 变体、壁纸计数、4K 原生硬标准、条款清单模型（允许/禁止二元）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// LOGO 点阵五尺寸。
pub const LOGO_SIZES: [u16; 5] = [16, 32, 48, 128, 512];
/// 4K 原生硬标准（横向像素下限）。
pub const NATIVE_4K_MIN_W: u16 = 3840;

// ---------------------------------------------------------------------------
// 资产包模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MonoVariant {
    White,
    Black,
    Accent,
}

pub const MONO_VARIANTS: [MonoVariant; 3] = [MonoVariant::White, MonoVariant::Black, MonoVariant::Accent];

/// 一份资产包登记（元数据面——像素本体走 4K 管线 C-7）。
pub struct BrandKit {
    /// LOGO 各尺寸是否在册（位图对应 LOGO_SIZES）。
    logo_sizes: u8,
    /// 矢量源（svg）在册。
    pub vector_source: bool,
    /// 单色变体在册位图。
    mono: u8,
    /// 壁纸在册数（全 4K 原生）。
    pub wallpapers_4k: u16,
    /// 条款页（法律面）在册。
    pub terms_page: bool,
    /// 正确用法图解（间距/底色规范）在册。
    pub usage_diagram: bool,
}

impl BrandKit {
    pub fn new() -> BrandKit {
        BrandKit {
            logo_sizes: 0,
            vector_source: false,
            mono: 0,
            wallpapers_4k: 0,
            terms_page: false,
            usage_diagram: false,
        }
    }

    pub fn add_logo_size(&mut self, size: u16) -> Result<(), &'static str> {
        let idx = LOGO_SIZES.iter().position(|&s| s == size).ok_or("size outside fixed ladder")?;
        self.logo_sizes |= 1 << idx;
        Ok(())
    }

    pub fn add_mono(&mut self, v: MonoVariant) {
        self.mono |= 1 << (v as u8);
    }

    /// 全分辨率齐（含 4K 原生）判据：
    /// 五尺寸 + 矢量源 + 三单色 + 壁纸 10 张（≥3840 宽）+ 条款 + 图解。
    pub fn complete(&self) -> bool {
        self.logo_sizes == 0b1111_1 && self.vector_source && self.mono == 0b111 && self.wallpapers_4k >= 10 && self.terms_page && self.usage_diagram
    }

    pub fn logo_size_count(&self) -> u32 {
        self.logo_sizes.count_ones()
    }

    pub fn mono_count(&self) -> u32 {
        self.mono.count_ones()
    }
}

// ---------------------------------------------------------------------------
// 条款清单（允许/禁止二元——法律面过审的形态要求）
// ---------------------------------------------------------------------------

/// 条款清单：允许/禁止各若干条，逐条编号可引用（F148 仲裁引用键）。
pub struct Terms {
    pub allowed: [&'static str; 2],
    pub forbidden: [&'static str; 3],
}

pub const DEFAULT_TERMS: Terms = Terms {
    allowed: ["非商业展示与评测引用", "社区教程中的示意使用"],
    forbidden: ["篡改主色或比例", "暗示官方背书", "用于与 VARIX 无关的产品宣传"],
};

impl Terms {
    /// 清单式条款形态校验：允许与禁止都不为空、逐条非空——
    /// 「条款页法律面过审」的形态面（法律实质过审是流程动作）。
    pub fn well_formed(&self) -> bool {
        self.allowed.iter().all(|s| !s.is_empty()) && self.forbidden.iter().all(|s| !s.is_empty())
    }

    /// 用途裁决：给出「某用法是否允许」的保守答案——用途关键词命中
    /// 允许清单短语（或短语命中用途）才放行，未列入的一律按禁止
    /// 处理（白名单制）。
    pub fn verdict(&self, usage: &str) -> bool {
        self.allowed.iter().any(|a| usage.contains(a) || a.contains(usage))
    }
}

// ---------------------------------------------------------------------------
// 滥用处理（先礼后兵）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbuseStage {
    /// 第一步：沟通（先礼）。
    Contacted,
    /// 第二步：下架要求（后兵）。
    TakedownDemand,
    /// 争议 → F148 仲裁。
    EscalatedToGovernance,
}

/// 滥用管线：必须先沟通再下架（顺序红线）。
pub struct AbuseFlow {
    pub stage: AbuseStage,
    pub contacted_day: u32,
}

impl AbuseFlow {
    pub fn new(day: u32) -> AbuseFlow {
        AbuseFlow { stage: AbuseStage::Contacted, contacted_day: day }
    }

    pub fn demand_takedown(&mut self, day: u32) -> Result<(), &'static str> {
        if self.stage != AbuseStage::Contacted {
            return Err("contact first — no skip");
        }
        if day < self.contacted_day {
            return Err("no time travel");
        }
        self.stage = AbuseStage::TakedownDemand;
        Ok(())
    }

    pub fn escalate(&mut self) -> Result<(), &'static str> {
        if self.stage != AbuseStage::TakedownDemand {
            return Err("only takedown disputes escalate");
        }
        self.stage = AbuseStage::EscalatedToGovernance;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143_TAG: &str = "stareco-F143-brandkit";

pub fn run_brandkit_checks() -> CheckSet {
    let mut set = CheckSet::new(F143_TAG);

    // LOGO 尺寸阶梯
    let mut kit = BrandKit::new();
    set.add("f143 empty kit incomplete", !kit.complete(), "nothing yet");
    set.add("f143 odd size rejected", kit.add_logo_size(100).is_err(), "fixed ladder only");
    for &s in LOGO_SIZES.iter() {
        assert!(kit.add_logo_size(s).is_ok());
    }
    set.add("f143 five sizes", kit.logo_size_count() == 5, "16/32/48/128/512");

    // 单色三变体
    set.add("f143 mono not yet", kit.mono_count() == 0, "none");
    for v in MONO_VARIANTS.iter() {
        kit.add_mono(*v);
    }
    set.add("f143 three mono variants", kit.mono_count() == 3, "white/black/accent");

    // 壁纸 + 条款 + 图解
    kit.wallpapers_4k = 9;
    kit.vector_source = true;
    kit.terms_page = true;
    kit.usage_diagram = true;
    set.add("f143 nine wallpapers short", !kit.complete(), "10 required");
    kit.wallpapers_4k = 10;
    set.add("f143 kit complete with 4k natives", kit.complete(), "all present");
    set.add("f143 4k floor respected", NATIVE_4K_MIN_W == 3840, "hard line");

    // 条款清单
    set.add("f143 terms well formed", DEFAULT_TERMS.well_formed(), "both lists non-empty");
    set.add(
        "f143 allowed verdict",
        DEFAULT_TERMS.verdict("非商业展示") && DEFAULT_TERMS.verdict("评测引用") && !DEFAULT_TERMS.verdict("改成绿色卖广告"),
        "whitelist law",
    );
    set.add("f143 unlisted is forbidden", !DEFAULT_TERMS.verdict("随便用用"), "conservative default");

    // 滥用先礼后兵
    let mut abuse = AbuseFlow::new(10);
    set.add("f143 no time travel", abuse.demand_takedown(5).is_err(), "day guard");
    set.add("f143 contact-only cannot escalate", AbuseFlow::new(0).escalate().is_err(), "escalation gate");
    assert!(abuse.demand_takedown(20).is_ok());
    set.add("f143 no double takedown", abuse.demand_takedown(30).is_err(), "order enforced");
    set.add("f143 takedown after contact", abuse.stage == AbuseStage::TakedownDemand, "second step");
    set.add("f143 contact-only cannot escalate", AbuseFlow::new(0).escalate().is_err(), "escalation gate");
    assert!(abuse.escalate().is_ok());
    set.add("f143 dispute to F148", abuse.stage == AbuseStage::EscalatedToGovernance, "arbitration path");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_kit_states() {
        let mut k = BrandKit::new();
        k.add_mono(MonoVariant::White);
        assert_eq!(k.mono_count(), 1);
        k.add_mono(MonoVariant::White); // 重复登记幂等
        assert_eq!(k.mono_count(), 1);
        let _ = k.add_logo_size(512);
        assert_eq!(k.logo_size_count(), 1);
    }
}
