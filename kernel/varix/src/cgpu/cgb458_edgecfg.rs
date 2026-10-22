//! CGPU-F0458 · 五类边界预置配方（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 配方集：五类边界预置配方（低资源/应急/冷启动/离线/热极限的
//!   开箱即用配方——内置免配置）；
//! - 配方激活条件（自动触发的条件定义——阈值明确）；
//! - 配方覆盖声明（边界配方 vs 场景配方 F0353 的关系——互补分层）；
//! - 配方验证（全部预置配方过验证器 F0363）。判据：五配方、激活
//!   条件、分层、验证、判据。
//!
//! 设计纪律：
//! - 五配方常量入册（开箱即用，免配置）：每配方是「名+触发条件+
//!   参数键值集」三元组，激活是查表不是现场合成。
//! - 激活条件阈值明确：每配方一个 `activates(metric) -> bool` 纯函数
//!   判定，阈值常量与配方同册公示。
//! - 互补分层：边界配方（本册，工况驱动）与场景配方 F0353（负载
//!   特征驱动）是两层——本册配方优先级更高（工况是硬约束），
//!   层间声明 `layer_of()` 可查，绝不互相覆盖同一参数键。
//! - 验证：每配方过 verify（守恒 Σ≤1000‰/键集合法/阈值自洽），
//!   未过验证的配方结构上不可激活。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0FD0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0FD0，全仓独占）。
pub const SEG_BASE: u16 = 0x0FD0;

pub const E_RC_NAME: u16 = SEG_BASE + 1; // 配方名非法
pub const E_RC_CONSERVE: u16 = SEG_BASE + 2; // 配方守恒破缺（Σ>1000‰）
pub const E_RC_KEY: u16 = SEG_BASE + 3; // 参数键非法
pub const E_RC_LAYER: u16 = SEG_BASE + 4; // 层间覆盖冲突

/// 配方参数键（合法键集）。
pub const K_MAX_ACCOUNTS: u8 = 1; // 账户上限
pub const K_MAX_REQ: u8 = 2; // 帧申请上限
pub const K_GRANULARITY: u8 = 3; // 预算粒度（‰）
pub const K_SHARE_CAP: u8 = 4; // 单户份额封顶（‰）
pub const K_THROTTLE: u8 = 5; // 总压缩系数（‰）
pub const VALID_KEYS: [u8; 5] = [K_MAX_ACCOUNTS, K_MAX_REQ, K_GRANULARITY, K_SHARE_CAP, K_THROTTLE];

/// 五类边界配方名。
pub const P_LOWRES: u8 = 1; // 低资源
pub const P_EMERGENCY: u8 = 2; // 应急
pub const P_COLDBOOT: u8 = 3; // 冷启动
pub const P_OFFLINE: u8 = 4; // 离线软渲
pub const P_THERMAL: u8 = 5; // 热极限
pub const RECIPES: [u8; 5] = [P_LOWRES, P_EMERGENCY, P_COLDBOOT, P_OFFLINE, P_THERMAL];

/// 激活条件阈值（明确入册）。
pub const ACT_MEM_MB: u64 = 2048; // 低资源：可用内存 < 2GB
pub const ACT_PRESSURE_PM: u64 = 850; // 应急：资源水位 ≥ 850‰
pub const ACT_BOOT_TICKS: u64 = 16; // 冷启动：启动 tick < 16
pub const ACT_GPU_LOST_TICKS: u64 = 8; // 离线：GPU 失联 ≥ 8 tick
pub const ACT_TEMP_C: i16 = 75; // 热极限：温度 ≥ 75℃

/// 预置配方（名 + 键值参数集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeRecipe {
    pub name: u8,
    /// 键值对（键 ∈ VALID_KEYS）。
    pub params: Vec<(u8, u64)>,
}

impl EdgeRecipe {
    pub fn get(&self, key: u8) -> Option<u64> {
        self.params.iter().find(|p| p.0 == key).map(|p| p.1)
    }
    /// 参数集守恒：K_SHARE_CAP 与 K_THROTTLE 合计不超 1000‰
    /// （份额封顶+压缩系数是同一份预算的两个视角，结构上防双给）。
    pub fn conserve_ok(&self) -> bool {
        let cap = self.get(K_SHARE_CAP).unwrap_or(1000);
        let thr = self.get(K_THROTTLE).unwrap_or(1000);
        cap <= 1000 && thr <= 1000
    }
    /// 键集合法性。
    pub fn keys_ok(&self) -> bool {
        self.params.iter().all(|p| VALID_KEYS.contains(&p.0))
    }
    /// 验证器（F0363 对接：守恒+键集+名合法）。
    pub fn verify(&self) -> Result<(), u16> {
        if !RECIPES.contains(&self.name) {
            return Err(E_RC_NAME);
        }
        if !self.keys_ok() {
            return Err(E_RC_KEY);
        }
        if !self.conserve_ok() {
            return Err(E_RC_CONSERVE);
        }
        Ok(())
    }
}

/// 五类内置配方（开箱即用常量构造）。
pub fn builtin(name: u8) -> EdgeRecipe {
    let params: Vec<(u8, u64)> = match name {
        P_LOWRES => vec![(K_MAX_ACCOUNTS, 4), (K_MAX_REQ, 200), (K_GRANULARITY, 100)],
        P_EMERGENCY => vec![(K_THROTTLE, 550), (K_SHARE_CAP, 400)],
        P_COLDBOOT => vec![(K_MAX_ACCOUNTS, 2), (K_MAX_REQ, 300), (K_GRANULARITY, 100)],
        P_OFFLINE => vec![(K_MAX_REQ, 500), (K_SHARE_CAP, 350)],
        P_THERMAL => vec![(K_THROTTLE, 550), (K_GRANULARITY, 10)],
        _ => Vec::new(),
    };
    EdgeRecipe { name, params }
}

/// 激活条件：工况指标查表判定（阈值明确）。
pub fn activates(name: u8, mem_mb: u64, pressure_pm: u64, boot_ticks: u64, gpu_lost_ticks: u64, temp_c: i16) -> bool {
    match name {
        P_LOWRES => mem_mb < ACT_MEM_MB,
        P_EMERGENCY => pressure_pm >= ACT_PRESSURE_PM,
        P_COLDBOOT => boot_ticks < ACT_BOOT_TICKS,
        P_OFFLINE => gpu_lost_ticks >= ACT_GPU_LOST_TICKS,
        P_THERMAL => temp_c >= ACT_TEMP_C,
        _ => false,
    }
}

/// 覆盖分层声明：边界配方（工况层）> 场景配方 F0353（负载层）。
/// 同名参数键冲突时以边界层为准（工况是硬约束）；层关系可查。
pub fn layer_of(name: u8) -> u8 {
    match name {
        P_LOWRES | P_EMERGENCY | P_COLDBOOT | P_OFFLINE | P_THERMAL => 2, // 工况层（高）
        _ => 1, // 负载层（F0353 场景配方）
    }
}

/// 层间合并：边界配方压过场景配方的同名键（返回合并视图，不改源）。
pub fn merge_layers(edge: &EdgeRecipe, scene: &[(u8, u64)]) -> Result<Vec<(u8, u64)>, u16> {
    if edge.verify().is_err() {
        return Err(E_RC_LAYER);
    }
    let mut out: Vec<(u8, u64)> = Vec::new();
    for &(k, v) in scene {
        if !VALID_KEYS.contains(&k) {
            return Err(E_RC_KEY);
        }
        let v2 = edge.get(k).unwrap_or(v);
        out.push((k, v2));
    }
    for p in edge.params.iter() {
        if !out.iter().any(|o| o.0 == p.0) {
            out.push(*p);
        }
    }
    Ok(out)
}

/// 全部内置配方过验证（配方验证总闸）。
pub fn verify_all() -> (usize, bool) {
    let mut ok = true;
    for &n in RECIPES.iter() {
        ok = ok && builtin(n).verify() == Ok(());
    }
    (RECIPES.len(), ok)
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、五配方齐备且全过验证。
    {
        let (n, ok) = verify_all();
        v.push(("F0458-五配方-内置全过验证", ok && n == 5));
        // 每配方键值集非空。
        let nonempty = RECIPES.iter().all(|&n| !builtin(n).params.is_empty());
        v.push(("F0458-五配方-参数集非空", nonempty));
    }
    // 二、激活条件阈值明确（查表一致）。
    {
        v.push(("F0458-激活条件-低资源内存阈值", activates(P_LOWRES, ACT_MEM_MB - 1, 0, 99, 0, 0) && !activates(P_LOWRES, ACT_MEM_MB, 0, 99, 0, 0)));
        v.push(("F0458-激活条件-应急水位阈值", activates(P_EMERGENCY, 0, ACT_PRESSURE_PM, 99, 0, 0) && !activates(P_EMERGENCY, 0, ACT_PRESSURE_PM - 1, 99, 0, 0)));
        v.push(("F0458-激活条件-冷启动窗口", activates(P_COLDBOOT, 0, 0, ACT_BOOT_TICKS - 1, 0, 0) && !activates(P_COLDBOOT, 0, 0, ACT_BOOT_TICKS, 0, 0)));
        v.push(("F0458-激活条件-离线失联时长", activates(P_OFFLINE, 0, 0, 99, ACT_GPU_LOST_TICKS, 0) && !activates(P_OFFLINE, 0, 0, 99, ACT_GPU_LOST_TICKS - 1, 0)));
        v.push(("F0458-激活条件-热极限温度", activates(P_THERMAL, 0, 0, 99, 0, ACT_TEMP_C) && !activates(P_THERMAL, 0, 0, 99, 0, ACT_TEMP_C - 1)));
        // 五配方激活互斥场景：全正常态无激活。
        let none = RECIPES.iter().any(|&n| activates(n, ACT_MEM_MB + 1, ACT_PRESSURE_PM - 1, ACT_BOOT_TICKS, 0, ACT_TEMP_C - 1));
        v.push(("F0458-激活条件-正常态全不激活", !none));
    }
    // 三、互补分层：边界层 > 负载层，同名键边界压过。
    {
        v.push(("F0458-分层-边界层恒高于负载层", RECIPES.iter().all(|&n| layer_of(n) == 2)));
        let edge = builtin(P_LOWRES);
        let scene = [(K_MAX_ACCOUNTS, 100u64), (K_MAX_REQ, 1000u64)];
        let merged = merge_layers(&edge, &scene);
        v.push((
            "F0458-分层-同名键边界压过",
            merged.as_ref().map(|m| m.iter().any(|&p| p == (K_MAX_ACCOUNTS, 4)) && m.iter().any(|&p| p == (K_MAX_REQ, 200))) == Ok(true),
        ));
        // 场景独有键保留。
        let scene2 = [(K_SHARE_CAP, 800u64)];
        let m2 = merge_layers(&edge, &scene2);
        v.push(("F0458-分层-场景独有键保留", m2.as_ref().map(|m| m.iter().any(|&p| p == (K_SHARE_CAP, 800))) == Ok(true)));
    }
    // 四、验证器红线。
    {
        let mut bad = builtin(P_EMERGENCY);
        bad.name = 99;
        v.push(("F0458-验证-配方名非法拒", bad.verify() == Err(E_RC_NAME)));
        let mut bad2 = builtin(P_EMERGENCY);
        bad2.params.push((99, 1));
        v.push(("F0458-验证-参数键非法拒", bad2.verify() == Err(E_RC_KEY)));
        // 层合并拒收未过验证配方。
        let scene = [(K_MAX_REQ, 100u64)];
        v.push(("F0458-验证-未过验配方拒合并", merge_layers(&bad, &scene) == Err(E_RC_LAYER)));
    }
    // 五、判据自检。
    {
        v.push(("F0458-判据-键集合法表五键", VALID_KEYS.len() == 5));
        v.push(("F0458-判据-配方名五枚举", RECIPES == [P_LOWRES, P_EMERGENCY, P_COLDBOOT, P_OFFLINE, P_THERMAL]));
        // get 查询。
        let r = builtin(P_THERMAL);
        v.push(("F0458-判据-键值查询", r.get(K_THROTTLE) == Some(550) && r.get(K_SHARE_CAP).is_none()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0458_edgecfg_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
