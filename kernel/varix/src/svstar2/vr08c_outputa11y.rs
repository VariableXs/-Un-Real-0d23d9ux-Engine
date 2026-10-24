//! VE-F3743 · 创作产出无障碍验证（VE-S 域 · 批次 R08 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3743`
//!
//! **职责定位（锚点原文）**：创作产出无障碍验证——
//! - **产出验证**：创作者作品的自动无障碍验证
//!   （复用 F3607/F3704 引擎单源）——单源复用声明；
//! - **验证时机**：创作时实时提示 + 发布前门禁（双时机复述）；
//! - **产出分类验证**：主题/壁纸/图标/光标各有无障碍验证子项
//!   （主题对比度/壁纸语义描述/光标状态覆盖）——
//!   分类覆盖断言（每类资产有无障碍验证项）——
//!   覆盖红线：某类资产无验证 = 该类资产无障碍裸奔（裸奔红线）；
//! - **创作者教育**：验证失败 → 教育性提示（不是惩罚，教育复述）。
//!
//! **判据（锚点原文五条）**：分类覆盖、裸奔红线、教育导向、
//! 双时机、单源复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 裸奔类检出→补验证（红线实测） | [`O7Code::NUDITY_CLASS`] |
//! | 单源分叉→归一（复述红线实测） | [`O7Code::SOURCE_FORK`] |
//! | 验证惩罚化→教育修正（复述） | [`O7Code::PUNITIVE_VERIFY`] |
//! | 时机缺→补（复述） | [`O7Code::TIMING_MISS`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3743 诊断码段（独占 0x86xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct O7Code(pub u16);

impl O7Code {
    /// 裸奔类（补验证红线）。
    pub const NUDITY_CLASS: O7Code = O7Code(0x8601);
    /// 单源分叉（归一）。
    pub const SOURCE_FORK: O7Code = O7Code(0x8602);
    /// 验证惩罚化（教育修正）。
    pub const PUNITIVE_VERIFY: O7Code = O7Code(0x8603);
    /// 时机缺（补）。
    pub const TIMING_MISS: O7Code = O7Code(0x8604);
    /// 分类 ID 非法。
    pub const BAD_CLASS: O7Code = O7Code(0x8605);
    /// 验证失败。
    pub const VERIFY_FAIL: O7Code = O7Code(0x8606);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            O7Code::NUDITY_CLASS => "裸奔类（红线实测）。建议：补该类资产验证项。",
            O7Code::SOURCE_FORK => "单源分叉（复述红线实测）。建议：归一到 F3607/F3704。",
            O7Code::PUNITIVE_VERIFY => "验证惩罚化（复述）。建议：改为教育性提示。",
            O7Code::TIMING_MISS => "时机缺（复述）。建议：补双时机验证。",
            O7Code::BAD_CLASS => "分类 ID 非法。建议：0-3 四类。",
            O7Code::VERIFY_FAIL => "验证失败。建议：按教育提示整改。",
            _ => "未知诊断码。",
        }
    }
}

// ── 产出分类（覆盖红线） ────────────────────────────────────────────

/// 资产类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetClass {
    /// 主题（对比度）。
    Theme = 0,
    /// 壁纸（语义描述）。
    Wallpaper = 1,
    /// 图标（尺寸/对比）。
    Icon = 2,
    /// 光标（状态覆盖）。
    Cursor = 3,
}

impl AssetClass {
    pub const ALL: [AssetClass; 4] = [
        AssetClass::Theme,
        AssetClass::Wallpaper,
        AssetClass::Icon,
        AssetClass::Cursor,
    ];
}

/// 分类覆盖表。
pub struct ClassCoverage {
    covered: [bool; 4],
}

impl ClassCoverage {
    pub fn new() -> ClassCoverage {
        ClassCoverage {
            covered: [false; 4],
        }
    }

    /// 登记验证项。
    pub fn cover(&mut self, c: AssetClass) {
        self.covered[c as usize] = true;
    }

    /// 裸奔红线审计（任一类无验证即拦）。
    pub fn nudity_audit(&self) -> Result<(), O7Code> {
        for c in &self.covered {
            if !c {
                return Err(O7Code::NUDITY_CLASS);
            }
        }
        Ok(())
    }
}

// ── 双时机协议 ──────────────────────────────────────────────────────

/// 验证时机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Timing {
    /// 创作时实时提示。
    Realtime,
    /// 发布前门禁。
    PrePublish,
}

/// 时机登记。
pub struct TimingGate {
    realtime: bool,
    prepublish: bool,
}

impl TimingGate {
    pub fn new() -> TimingGate {
        TimingGate {
            realtime: false,
            prepublish: false,
        }
    }

    /// 启用时机。
    pub fn enable(&mut self, t: Timing) {
        match t {
            Timing::Realtime => self.realtime = true,
            Timing::PrePublish => self.prepublish = true,
        }
    }

    /// 双时机齐备。
    pub fn complete(&self) -> Result<(), O7Code> {
        if self.realtime && self.prepublish {
            Ok(())
        } else {
            Err(O7Code::TIMING_MISS)
        }
    }
}

// ── 教育性提示（教育复述） ──────────────────────────────────────────

/// 提示策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedbackStyle {
    /// 教育性（正确）。
    Educational,
    /// 惩罚性（错误）。
    Punitive,
}

/// 提示器。
pub struct Verifier {
    style: FeedbackStyle,
}

impl Verifier {
    /// 教育性构造。
    pub fn educational() -> Verifier {
        Verifier {
            style: FeedbackStyle::Educational,
        }
    }

    /// 惩罚性构造（违规）。
    pub fn punitive() -> Verifier {
        Verifier {
            style: FeedbackStyle::Punitive,
        }
    }

    /// 教育导向断言。
    pub fn assert_educational(&self) -> Result<(), O7Code> {
        if self.style == FeedbackStyle::Educational {
            Ok(())
        } else {
            Err(O7Code::PUNITIVE_VERIFY)
        }
    }

    /// 验证并出提示。
    pub fn verify(&self, pass: bool) -> Result<String, O7Code> {
        if pass {
            return Ok(String::new());
        }
        match self.style {
            FeedbackStyle::Educational => {
                Ok("建议：提升对比度至 4.5:1 以满足无障碍阅读。".to_string())
            }
            FeedbackStyle::Punitive => Err(O7Code::PUNITIVE_VERIFY),
        }
    }
}

// ── 单源复用声明 ────────────────────────────────────────────────────

/// 引擎引用。
pub struct EngineRef {
    /// 声明的源指纹。
    declared_fp: u16,
    /// 实际源指纹。
    actual_fp: u16,
}

impl EngineRef {
    pub const fn new(declared_fp: u16, actual_fp: u16) -> EngineRef {
        EngineRef {
            declared_fp,
            actual_fp,
        }
    }

    /// 单源一致（分叉即拦）。
    pub fn check(&self) -> Result<(), O7Code> {
        if self.declared_fp == self.actual_fp {
            Ok(())
        } else {
            Err(O7Code::SOURCE_FORK)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3743 判据集（锚点原文五条逐条落位）。
pub fn run_vr08c_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3743");

    // 1) 分类覆盖：齐过
    let mut cc = ClassCoverage::new();
    for c in AssetClass::ALL {
        cc.cover(c);
    }
    out.add(
        "0x8601-分类覆盖",
        cc.nudity_audit().is_ok(),
        "四类须齐",
    );

    // 2) 裸奔红线：缺被拦
    let mut cc2 = ClassCoverage::new();
    cc2.cover(AssetClass::Theme);
    out.add(
        "0x8601-裸奔红线",
        cc2.nudity_audit().is_err(),
        "裸奔须拦",
    );

    // 3) 双时机：齐过
    let mut tg = TimingGate::new();
    tg.enable(Timing::Realtime);
    tg.enable(Timing::PrePublish);
    out.add(
        "0x8604-双时机",
        tg.complete().is_ok(),
        "双时机须齐",
    );

    // 4) 时机缺：被拦
    let tg2 = TimingGate::new();
    out.add(
        "0x8604-缺拦",
        tg2.complete().is_err(),
        "缺须拦",
    );

    // 5) 教育导向：教育过
    let v = Verifier::educational();
    out.add(
        "0x8603-教育导向",
        v.assert_educational().is_ok(),
        "教育须过",
    );

    // 6) 惩罚化：被拦
    let v2 = Verifier::punitive();
    out.add(
        "0x8603-惩罚拦",
        v2.assert_educational().is_err(),
        "惩罚须拦",
    );

    // 7) 单源复用：一致过
    let er = EngineRef::new(0x1234, 0x1234);
    out.add(
        "0x8602-单源复用",
        er.check().is_ok(),
        "一致须过",
    );

    // 8) 单源分叉：被拦
    let er2 = EngineRef::new(0x1234, 0x5678);
    out.add(
        "0x8602-分叉拦",
        er2.check().is_err(),
        "分叉须拦",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x8606-判据",
        O7Code::NUDITY_CLASS.value() != O7Code::SOURCE_FORK.value(),
        "码段互异",
    );

    out
}
