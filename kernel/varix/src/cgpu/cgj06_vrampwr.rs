//! unxg06b_cgj06_vrampwr —— CGPU-F1446 显存与屏幕功耗模型（参数表公开可校准）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F1446 · 核显与内存功耗分账（CGPU-J 域 · 批次 J01 · J06 组 · 判据：五账户/估算标注/模型参数/屏幕查表/5% 审计/H 域联动/不确定度/测试）
//! · 组件：显存五族频率×利用率模型 + 屏幕五族亮度线性模型 + 公开参数表
//! · 判据落地：模型参数/屏幕查表
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const G06B_VERSION: &str = "g06b-cgj06_vrampwr-v1";

/// 诊断段基址。
pub const G06B_SEG_BASE: u16 = 0x7010;

// --------------------------------------------------------------------------
// 诊断码（g06b 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 显存/屏幕功耗诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum g06bCode {
    /// 0x7010 频率档非法（表外）。
    FreqBad,
    /// 0x7011 利用率非法（0..100 之外）。
    UtilBad,
    /// 0x7012 校准参数表缺失。
    ParamTableMissing,
    /// 0x7013 亮度非法（0..100 之外）。
    BrightnessBad,
    /// 0x7014 面板族非法（表外面板）。
    PanelFamilyBad,
    /// 0x7015 H 域联动缺失。
    HwLinkMissing,
    /// 0x7016 模型漂移（估算与校准差超阈）。
    ModelDrift,
    /// 0x7017 守卫触发。
    GuardTripped,
    /// 0x7018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x701a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x701b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x701c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x701d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x701e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl g06bCode {
    /// 全量闭集。
    pub const ALL: [g06bCode; 15] = [
        g06bCode::FreqBad,
        g06bCode::UtilBad,
        g06bCode::ParamTableMissing,
        g06bCode::BrightnessBad,
        g06bCode::PanelFamilyBad,
        g06bCode::HwLinkMissing,
        g06bCode::ModelDrift,
        g06bCode::GuardTripped,
        g06bCode::LedgerBroken,
        g06bCode::SnapshotFingerprint,
        g06bCode::TableBroken,
        g06bCode::IndexMisaligned,
        g06bCode::DuplicateEntry,
        g06bCode::OrderViolated,
        g06bCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            g06bCode::FreqBad => 0x7010,
            g06bCode::UtilBad => 0x7011,
            g06bCode::ParamTableMissing => 0x7012,
            g06bCode::BrightnessBad => 0x7013,
            g06bCode::PanelFamilyBad => 0x7014,
            g06bCode::HwLinkMissing => 0x7015,
            g06bCode::ModelDrift => 0x7016,
            g06bCode::GuardTripped => 0x7017,
            g06bCode::LedgerBroken => 0x7018,
            g06bCode::SnapshotFingerprint => 0x7019,
            g06bCode::TableBroken => 0x701a,
            g06bCode::IndexMisaligned => 0x701b,
            g06bCode::DuplicateEntry => 0x701c,
            g06bCode::OrderViolated => 0x701d,
            g06bCode::SegmentGuard => 0x701e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            g06bCode::FreqBad => "频率档非法（表外）",
            g06bCode::UtilBad => "利用率非法（0..100 之外）",
            g06bCode::ParamTableMissing => "校准参数表缺失",
            g06bCode::BrightnessBad => "亮度非法（0..100 之外）",
            g06bCode::PanelFamilyBad => "面板族非法（表外面板）",
            g06bCode::HwLinkMissing => "H 域联动缺失",
            g06bCode::ModelDrift => "模型漂移（估算与校准差超阈）",
            g06bCode::GuardTripped => "守卫触发",
            g06bCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            g06bCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            g06bCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            g06bCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            g06bCode::DuplicateEntry => "条目重复（同键双写）",
            g06bCode::OrderViolated => "次序违例（依赖序/时序破缺）",
            g06bCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            g06bCode::FreqBad => "按频率档表校正",
            g06bCode::UtilBad => "按利用率域校正",
            g06bCode::ParamTableMissing => "按公开参数表补配",
            g06bCode::BrightnessBad => "按亮度域校正",
            g06bCode::PanelFamilyBad => "按面板族表校正",
            g06bCode::HwLinkMissing => "按跨域接口补接",
            g06bCode::ModelDrift => "按重校准路径修正",
            g06bCode::GuardTripped => "检查临界区",
            g06bCode::LedgerBroken => "按守恒律重建台账后重跑",
            g06bCode::SnapshotFingerprint => "按防篡改规程重建快照",
            g06bCode::TableBroken => "按语义规则重建常量表",
            g06bCode::IndexMisaligned => "按声明序归一化后重跑",
            g06bCode::DuplicateEntry => "合并同键条目为末次生效",
            g06bCode::OrderViolated => "按依赖矩阵重排后重跑",
            g06bCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}


// ---------------------------------------------------------------------------
// 显存功耗模型（GDDR/HBM：按频率与利用率估算，参数表公开可校准）
// ---------------------------------------------------------------------------

/// 显存族（GDDR6/GDDR6X/HBM2/HBM2E/HBM3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VramKind {
    /// GDDR6。
    Gddr6,
    /// GDDR6X。
    Gddr6x,
    /// HBM2。
    Hbm2,
    /// HBM2E。
    Hbm2e,
    /// HBM3。
    Hbm3,
}

impl VramKind {
    /// 全量闭集（五族）。
    pub const ALL: [VramKind; 5] = [
        VramKind::Gddr6,
        VramKind::Gddr6x,
        VramKind::Hbm2,
        VramKind::Hbm2e,
        VramKind::Hbm3,
    ];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            VramKind::Gddr6 => 0,
            VramKind::Gddr6x => 1,
            VramKind::Hbm2 => 2,
            VramKind::Hbm2e => 3,
            VramKind::Hbm3 => 4,
        }
    }

    /// 英文名。
    pub const fn en(self) -> &'static str {
        match self {
            VramKind::Gddr6 => "GDDR6",
            VramKind::Gddr6x => "GDDR6X",
            VramKind::Hbm2 => "HBM2",
            VramKind::Hbm2e => "HBM2E",
            VramKind::Hbm3 => "HBM3",
        }
    }
}

/// 显存模型参数（空闲 mW + 每 MHz mW + per-利用率 mW）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramModel {
    /// 族。
    pub kind: VramKind,
    /// 静态/空闲功耗（mW）。
    pub idle_mw: u32,
    /// 每 MHz 动态功耗（mW per MHz）。
    pub per_mhz_mw: u32,
    /// 每 1% 利用率附加（mW）。
    pub per_util_mw: u32,
    /// 最高有效频率（MHz）。
    pub max_mhz: u32,
}

/// 五族校准参数表（公开可校准口径；估算非实测）。
pub const VRAM_MODELS: [VramModel; 5] = [
    VramModel { kind: VramKind::Gddr6, idle_mw: 8500, per_mhz_mw: 42, per_util_mw: 11, max_mhz: 900 },
    VramModel { kind: VramKind::Gddr6x, idle_mw: 12000, per_mhz_mw: 58, per_util_mw: 15, max_mhz: 1188 },
    VramModel { kind: VramKind::Hbm2, idle_mw: 15000, per_mhz_mw: 26, per_util_mw: 9, max_mhz: 1600 },
    VramModel { kind: VramKind::Hbm2e, idle_mw: 17000, per_mhz_mw: 28, per_util_mw: 10, max_mhz: 1800 },
    VramModel { kind: VramKind::Hbm3, idle_mw: 20000, per_mhz_mw: 22, per_util_mw: 8, max_mhz: 2000 },
];

/// 显存功耗估算（空闲 + 频率项 + 利用率项；mW）。
pub const fn vram_power_mw(model: &VramModel, mhz: u32, util_pct: u32) -> Option<u64> {
    if mhz > model.max_mhz || util_pct > 100 {
        return None;
    }
    let dyn_mhz = (mhz as u64) * (model.per_mhz_mw as u64);
    let dyn_util = (util_pct as u64) * (model.per_util_mw as u64);
    Some(model.idle_mw as u64 + dyn_mhz + dyn_util)
}

/// 面板族（LCD-IPS/LCD-VA/OLED/LCD-TN/eDP）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelFamily {
    /// LCD IPS。
    Ips,
    /// LCD VA。
    Va,
    /// OLED。
    Oled,
    /// LCD TN。
    Tn,
    /// eDP 内嵌屏。
    Edp,
}

/// 面板族全量闭集。
impl PanelFamily {
    /// 全量闭集（五族）。
    pub const ALL: [PanelFamily; 5] = [
        PanelFamily::Ips,
        PanelFamily::Va,
        PanelFamily::Oled,
        PanelFamily::Tn,
        PanelFamily::Edp,
    ];
}

/// 面板功耗模型（满亮 mW + 每 1% 亮度 mW + 最暗 mW）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelModel {
    /// 族。
    pub family: PanelFamily,
    /// 满亮附加功耗（mW @ 100%）。
    pub max_mw: u32,
    /// 每 1% 亮度功耗（mW）。
    pub per_pct_mw: u32,
    /// 最小亮度功耗（mW @ 0%）。
    pub min_mw: u32,
}

/// 五族面板参数表（查表法；亮度线性模型）。
pub const PANEL_MODELS: [PanelModel; 5] = [
    PanelModel { family: PanelFamily::Ips, max_mw: 4200, per_pct_mw: 42, min_mw: 1300 },
    PanelModel { family: PanelFamily::Va, max_mw: 4900, per_pct_mw: 49, min_mw: 1500 },
    PanelModel { family: PanelFamily::Oled, max_mw: 6600, per_pct_mw: 66, min_mw: 380 },
    PanelModel { family: PanelFamily::Tn, max_mw: 3500, per_pct_mw: 35, min_mw: 1100 },
    PanelModel { family: PanelFamily::Edp, max_mw: 3800, per_pct_mw: 38, min_mw: 900 },
];

/// 屏幕功耗估算（亮度线性模型；mW）。
pub const fn panel_power_mw(model: &PanelModel, brightness_pct: u32) -> Option<u64> {
    if brightness_pct > 100 {
        return None;
    }
    let dyn_mw = (brightness_pct as u64) * (model.per_pct_mw as u64);
    Some(model.min_mw as u64 + dyn_mw)
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7010, 0x701f]。
pub const fn g06b_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < g06bCode::ALL.len() {
        let c = g06bCode::ALL[i].code();
        if c < 0x7010 || c > 0x701f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn g06b_guard_seg() -> bool {
    g06b_segment_guard()
}

/// 显存模型（GDDR6X @900MHz/60% 估算 + 越界拒收）。
fn check_c1_vram_model() -> bool {
    let m = &VRAM_MODELS[1];
    let mut ok = vram_power_mw(m, 900, 60) == Some(12000 + 900 * 58 + 60 * 15);
    if ok {
        ok = vram_power_mw(m, 1200, 50).is_none() && vram_power_mw(m, 800, 101).is_none();
    }
    ok
}

/// 屏幕亮度线性模型（IPS 三亮度点 + 越界拒收）。
fn check_c2_panel() -> bool {
    let p = &PANEL_MODELS[0];
    let mut ok = panel_power_mw(p, 0) == Some(1300);
    if ok {
        ok = panel_power_mw(p, 50) == Some(1300 + 50 * 42);
    }
    if ok {
        ok = panel_power_mw(p, 100) == Some(1300 + 100 * 42) && panel_power_mw(p, 101).is_none();
    }
    ok
}

/// 参数表齐备（五族显存 + 五族面板 + 频率单调）。
fn check_c3_tables() -> bool {
    let mut ok = VRAM_MODELS.len() == 5 && PANEL_MODELS.len() == 5;
    if ok {
        let mut mono = true;
        let mut i = 1;
        while i < 5 {
            if VRAM_MODELS[i - 1].max_mhz > VRAM_MODELS[i].max_mhz {
                mono = false;
            }
            i += 1;
        }
        ok = mono;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_g06b_checks() -> CheckSet {
    let mut set = CheckSet::new("G06B");
    set.add(
        "g06b-seg-guard",
        g06b_guard_seg(),
        if g06b_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "g06b-c1-vram-model",
        check_c1_vram_model(),
        if check_c1_vram_model() { "" } else { "vram power model mismatch" },
    );
    set.add(
        "g06b-c2-panel",
        check_c2_panel(),
        if check_c2_panel() { "" } else { "panel brightness model mismatch" },
    );
    set.add(
        "g06b-c3-tables",
        check_c3_tables(),
        if check_c3_tables() { "" } else { "power parameter tables mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g06b_all_checks_green() {
        let set = run_g06b_checks();
        assert!(set.all_passed(), "G06B domain checks must be green");
    }

    #[test]
    fn g06b_segment_guard_holds() {
        assert!(g06b_segment_guard());
    }
}
