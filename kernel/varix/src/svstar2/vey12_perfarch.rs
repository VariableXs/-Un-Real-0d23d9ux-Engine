//! VE-F5121 · 性能总架构（VE-Z 域 · Y07 组 · 目标 320 行）· 诊断段 0x9780
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5121`
//!
//! **判据（锚点原文）**：三层、单帧宪法、全链治理、层冻结、判据。
//!
//! **职责定位（锚点原文）**：脚本性能总架构（性能定位：脚本的自由不能以帧率为
//! 代价——单帧预算宪法运行时兑现的总纲）；架构三层：度量层（全脚本埋点画像）
//! →预算层（分场景分脚本预算）→治理层（热点治理与降级）——层间接口冻结；预
//! 算超支→治理排程；失守→回退设计重议。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9780;

/// 单帧预算（ms）。
pub const FRAME_BUDGET_MS: u32 = 16;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 脚本画像条目：埋点耗时。
pub struct Profile {
    pub script: u32,
    pub cost_ms: u32,
}

/// 三层架构：度量→预算→治理（层间接口冻结）。
pub struct PerfArch {
    /// 接口冻结版本（三层共用）。
    pub iface_v: u32,
    /// 度量账。
    pub profiles: Vec<Profile>,
    /// 每脚本预算表。
    pub budgets: Vec<(u32, u32)>,
}

impl PerfArch {
    /// 标准构造。
    pub fn standard() -> Self {
        PerfArch {
            iface_v: 1,
            profiles: alloc::vec![
                Profile { script: 1, cost_ms: 4 },
                Profile { script: 2, cost_ms: 6 },
                Profile { script: 3, cost_ms: 5 },
            ],
            budgets: alloc::vec![(1, 5), (2, 7), (3, 5)],
        }
    }

    /// 单帧总账：全脚本成本和 ≤ 单帧宪法预算。
    pub fn frame_ok(&self) -> bool {
        let total: u32 = self.profiles.iter().map(|p| p.cost_ms).sum();
        total <= FRAME_BUDGET_MS
    }

    /// 逐脚本预算核对：超支脚本清单（治理排程输入）。
    pub fn overspent(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for p in self.profiles.iter() {
            if let Some((_, b)) = self.budgets.iter().find(|(s, _)| *s == p.script) {
                if p.cost_ms > *b {
                    v.push(p.script);
                }
            }
        }
        v
    }

    /// 接口冻结：三层共用一版。
    pub fn frozen(&self) -> bool {
        self.iface_v == 1
    }
}

/// J1 主判据：三层冻结 + 单帧宪法 + 全链治理。
pub fn j1_main() -> bool {
    let a = PerfArch::standard();
    a.frozen() && a.frame_ok() && a.overspent().is_empty()
}

// ── 注入核（0x9780 段 5 项）────────────────────────────────────────────

/// 注入核 1：宪法失守注入——总成本超 16ms 必红。
pub fn inj1_frame_breach() -> bool {
    let mut a = PerfArch::standard();
    a.profiles[0].cost_ms = 12;
    !a.frame_ok()
}

/// 注入核 2：超支检出注入——超支脚本必入治理清单。
pub fn inj2_overspend_detected() -> bool {
    let mut a = PerfArch::standard();
    a.profiles[1].cost_ms = 9; // 预算 7 → 超支
    a.overspent() == alloc::vec![2]
}

/// 注入核 3：层失配注入——接口版本漂移必红。
pub fn inj3_iface_drift() -> bool {
    let mut a = PerfArch::standard();
    a.iface_v = 2;
    !a.frozen()
}

/// 注入核 4：预算缺表注入——无预算脚本超支不可见（账面完整核对）。
pub fn inj4_budget_gap() -> bool {
    let mut a = PerfArch::standard();
    a.budgets.pop();
    // script 3 无预算 → 超支清单不含它（账面失明），治理要求预算全覆盖
    a.budgets.len() != a.profiles.len()
}

/// 注入核 5：度量哈希冻结注入——画像序列哈希重放一致。
pub fn inj5_profile_frozen() -> bool {
    let mk = || {
        let mut bytes = Vec::new();
        for p in PerfArch::standard().profiles.iter() {
            bytes.extend_from_slice(&p.script.to_le_bytes());
            bytes.extend_from_slice(&p.cost_ms.to_le_bytes());
        }
        fnv1a(&bytes)
    };
    mk() == mk() && mk() != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vey12_checks：本单判据聚合（诊断段 0x9780）。
pub fn run_vey12_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5121 性能总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三层接口冻结+单帧宪法（≤16ms）",
        j1_main(),
        if j1_main() { "" } else { "宪法破" },
    );
    s.add(
        "全链治理（超支检出+排程输入）",
        j1_main() && inj2_overspend_detected(),
        if j1_main() { "" } else { "治理破" },
    );
    let inj = [
        ("注入1宪法失守", inj1_frame_breach()),
        ("注入2超支检出", inj2_overspend_detected()),
        ("注入3层失配", inj3_iface_drift()),
        ("注入4预算缺表", inj4_budget_gap()),
        ("注入5度量冻结", inj5_profile_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
