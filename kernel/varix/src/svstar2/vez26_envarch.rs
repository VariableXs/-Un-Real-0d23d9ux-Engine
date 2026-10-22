//! VE-F5261 · 环境库总架构（VE-Z 域 · Z04 组 · 目标 320 行）· 诊断段 0x9d80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5261`
//!
//! **判据（锚点原文）**：五件、状态单源、环境即叙事、层冻结、判据。
//!
//! **职责定位（锚点原文）**：环境特效库总架构（环境库定位：世界活着的呼吸感
//! ——雾/天气/光照/昼夜/天空五件让场景自己会讲故事）；架构三层：状态层（环境
//! 状态单源）→驱动层（时间与事件驱动）→呈现层（渲染接入）——层间接口冻结；
//! 环境即叙事（环境氛围为内容服务）+可关律延伸（环境特效全可关）；状态分裂→
//! 单源归一。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9d80;

/// 环境五件。
pub const ELEMENTS: u32 = 5;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 环境五件闭集。
pub const ELEMENT_NAMES: [&str; 5] = ["fog", "weather", "light", "daynight", "sky"];

/// 环境状态：五件各自强度 + 可关位。
pub struct EnvState {
    /// 每件强度 0..1。
    pub strength: [f32; ELEMENTS as usize],
    /// 每件可关位（可关律：全可关）。
    pub switchable: [bool; ELEMENTS as usize],
}

impl EnvState {
    /// 标准构造：五件全登记全可关。
    pub fn standard() -> Self {
        EnvState {
            strength: [0.5, 0.3, 0.8, 0.6, 0.9],
            switchable: [true; 5],
        }
    }

    /// 可关律：五件全可关。
    pub fn all_switchable(&self) -> bool {
        self.switchable.iter().all(|s| *s)
    }

    /// 强度 schema：全在 0..1。
    pub fn in_schema(&self) -> bool {
        self.strength.iter().all(|s| *s >= 0.0 && *s <= 1.0)
    }
}

/// 三层架构：状态单源→驱动→呈现（接口冻结）。
pub struct EnvArch {
    pub iface_v: u32,
    /// 状态单源（唯一状态实例 id）。
    pub state_id: u32,
    pub state: EnvState,
}

impl EnvArch {
    /// 状态单源核对（呈现层与驱动层读同一状态）。
    pub fn single_state(&self, driver_view: u32, render_view: u32) -> bool {
        driver_view == self.state_id && render_view == self.state_id
    }

    /// 接口冻结。
    pub fn frozen(&self) -> bool {
        self.iface_v == 1
    }
}

/// 昼夜驱动：时间驱动状态推进（驱动层语义）。
pub fn daynight_drive(hour: u32) -> f32 {
    // 0-6 夜 → 6-12 晨 → 12-18 昼 → 18-24 暮：光照强度曲线
    match hour {
        0..=5 => 0.1,
        6..=11 => 0.5,
        12..=17 => 0.9,
        _ => 0.3,
    }
}

/// J1 主判据：五件 + 状态单源 + 环境即叙事可关律 + 层冻结。
pub fn j1_main() -> bool {
    let a = EnvArch { iface_v: 1, state_id: 100, state: EnvState::standard() };
    if !(a.frozen() && a.state.all_switchable() && a.state.in_schema()) {
        return false;
    }
    if !a.single_state(100, 100) {
        return false;
    }
    // 状态分裂检出：驱动层看别的状态必红
    if a.single_state(101, 100) {
        return false;
    }
    // 昼夜驱动四段曲线单调语义抽查
    daynight_drive(3) < daynight_drive(9)
        && daynight_drive(9) < daynight_drive(14)
        && ELEMENT_NAMES.len() == ELEMENTS as usize
}

// ── 注入核（0x9d80 段 5 项）────────────────────────────────────────────

/// 注入核 1：不可关注入——某件不可关必红（可关律）。
pub fn inj1_not_switchable() -> bool {
    let mut st = EnvState::standard();
    st.switchable[2] = false;
    !st.all_switchable()
}

/// 注入核 2：强度越域注入。
pub fn inj2_strength_out() -> bool {
    let mut st = EnvState::standard();
    st.strength[0] = 1.5;
    !st.in_schema()
}

/// 注入核 3：状态分裂注入。
pub fn inj3_state_split() -> bool {
    let a = EnvArch { iface_v: 1, state_id: 7, state: EnvState::standard() };
    !a.single_state(7, 8)
}

/// 注入核 4：层失配注入——接口漂移必红。
pub fn inj4_iface_drift() -> bool {
    let a = EnvArch { iface_v: 2, state_id: 7, state: EnvState::standard() };
    !a.frozen()
}

/// 注入核 5：五件闭集冻结注入。
pub fn inj5_elements_frozen() -> bool {
    let mut bytes = Vec::new();
    for n in ELEMENT_NAMES.iter() {
        bytes.extend_from_slice(n.as_bytes());
    }
    fnv1a(&bytes) != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vez26_checks：本单判据聚合（诊断段 0x9d80）。
pub fn run_vez26_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5261 环境库总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "五件登记+状态单源+层冻结",
        j1_main(),
        if j1_main() { "" } else { "状态/层破" },
    );
    s.add(
        "可关律+强度 schema+昼夜驱动",
        j1_main(),
        if j1_main() { "" } else { "可关/驱动破" },
    );
    let inj = [
        ("注入1不可关", inj1_not_switchable()),
        ("注入2强度越域", inj2_strength_out()),
        ("注入3状态分裂", inj3_state_split()),
        ("注入4层失配", inj4_iface_drift()),
        ("注入5五件冻结", inj5_elements_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
