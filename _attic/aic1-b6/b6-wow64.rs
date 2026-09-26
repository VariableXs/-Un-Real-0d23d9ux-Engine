
// ---------------------------------------------------------------------------
// F004 · 深化批次六：混合包双轨路由（32 位安装器被拒 + 64 位主程序放行）
//
// 主册依据（G-A-04【状态与异常】）：「带 32 位安装器的混合包（32 位安装器装
// 64 位主程序）→ 安装器本身被拒时提示完整归因」——被拒的是安装器，主程序
// 的位数判定独立（拒绝不蔓延：双轨路由面）。
// ---------------------------------------------------------------------------

/// 混合包双轨路由：安装器轨（位数判定独立）与主程序轨互不牵连。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MixedTrack {
    /// 安装器轨：32 位 → 拒（诚实卡片）。
    InstallerRefused,
    /// 主程序轨：64 位 → 放行（按 F001 管线）。
    PayloadAllowed,
    /// 主程序也 32 位 → 拒（同卡片体系）。
    PayloadRefused,
}

/// 路由判定：两轨独立判定（installer_64/payload_64 分别来自各自 PE 头）。
pub fn mixed_route(installer_64: bool, payload_64: bool) -> MixedTrack {
    match (installer_64, payload_64) {
        (false, true) => MixedTrack::InstallerRefused,
        (false, false) => MixedTrack::PayloadRefused,
        (true, true) | (true, false) => MixedTrack::PayloadAllowed,
    }
}

/// F004 深化批次六自检。
pub fn run_wow64_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F004-wow64-deep5");
    // 1) 典型混合包：32 位安装器拒 + 64 位主程序放行（拒绝不蔓延——两轨独立）。
    cs.add(
        "mixed_route_installer_refused_payload_allowed",
        mixed_route(false, true) == MixedTrack::InstallerRefused,
        "",
    );
    // 2) 双 32 位：两轨全拒（同卡片体系——不给第二次不同话术）。
    cs.add(
        "mixed_route_both_refused_same_card",
        mixed_route(false, false) == MixedTrack::PayloadRefused,
        "",
    );
    // 3) 安装器 64 位（纯 64 位包）：主程序轨直接放行（无混合语义）。
    cs.add(
        "pure64_package_payload_allowed",
        mixed_route(true, true) == MixedTrack::PayloadAllowed,
        "",
    );
    cs
}
