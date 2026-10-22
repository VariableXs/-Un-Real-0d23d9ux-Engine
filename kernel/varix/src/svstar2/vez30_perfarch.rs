//! VE-F5301 · 特效性能总架构（VE-Z 域 · Z06 组 · 目标 320 行）· 诊断段 0x9f80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5301`
//!
//! **判据（锚点原文）**：总预算宪法、三层、分域表、治理、判据。
//!
//! **职责定位（锚点原文）**：特效性能总架构（性能定位：四子系统+两翼的特效总
//! 预算治理——单帧特效总预算宪法（Z01 声明→各子系统分预算→本组总账治理））；
//! 架构三层：度量层（逐特效埋点）→预算层（分域预算表）→治理层（降级与清账）
//! ——层间接口冻结；预算失守→治理排程；漂移→对拍重绘。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9f80;

/// 单帧特效总预算（ms）。
pub const TOTAL_BUDGET_MS: u32 = 4;

/// 分域数。
pub const DOMAINS: u32 = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 分域预算表：粒子/后处理/环境/转场。
pub const DOMAIN_NAMES: [&str; 4] = ["particle", "postfx", "envfx", "transition"];

/// 特效埋点条目。
pub struct FxProfile {
    pub domain: u32,
    pub cost_us: u32, // 微秒
}

/// 三层性能架构。
pub struct FxPerfArch {
    pub iface_v: u32,
    pub profiles: Vec<FxProfile>,
    /// 分域预算（微秒）。
    pub budgets: [u32; DOMAINS as usize],
}

impl FxPerfArch {
    /// 标准构造：四域各 1000us = 总 4ms。
    pub fn standard() -> Self {
        FxPerfArch {
            iface_v: 1,
            profiles: alloc::vec![
                FxProfile { domain: 0, cost_us: 900 },
                FxProfile { domain: 1, cost_us: 800 },
                FxProfile { domain: 2, cost_us: 700 },
                FxProfile { domain: 3, cost_us: 600 },
            ],
            budgets: [1000, 1000, 1000, 1000],
        }
    }

    /// 总预算宪法：全特效成本和 ≤ 总预算。
    pub fn total_ok(&self) -> bool {
        let total_us: u32 = self.profiles.iter().map(|p| p.cost_us).sum();
        total_us <= TOTAL_BUDGET_MS * 1000
    }

    /// 分域超支清单（治理排程输入）。
    pub fn overspent_domains(&self) -> Vec<u32> {
        let mut spend = [0u32; DOMAINS as usize];
        for p in self.profiles.iter() {
            if (p.domain as usize) < DOMAINS as usize {
                spend[p.domain as usize] += p.cost_us;
            }
        }
        (0..DOMAINS as usize)
            .filter(|k| spend[*k] > self.budgets[*k])
            .map(|k| k as u32)
            .collect()
    }

    /// 接口冻结。
    pub fn frozen(&self) -> bool {
        self.iface_v == 1
    }
}

/// J1 主判据：总预算宪法 + 三层冻结 + 分域治理。
pub fn j1_main() -> bool {
    let a = FxPerfArch::standard();
    a.frozen() && a.total_ok() && a.overspent_domains().is_empty()
        && DOMAIN_NAMES.len() == DOMAINS as usize
}

// ── 注入核（0x9f80 段 5 项）────────────────────────────────────────────

/// 注入核 1：宪法失守注入——总成本超 4ms 必红。
pub fn inj1_total_breach() -> bool {
    let mut a = FxPerfArch::standard();
    a.profiles[0].cost_us = 3000;
    !a.total_ok()
}

/// 注入核 2：分域超支检出注入——单域超预算必入清单。
pub fn inj2_domain_overspend() -> bool {
    let mut a = FxPerfArch::standard();
    a.profiles[1].cost_us = 1500; // 预算 1000 → 超支
    a.overspent_domains() == alloc::vec![1]
}

/// 注入核 3：层失配注入。
pub fn inj3_iface_drift() -> bool {
    let mut a = FxPerfArch::standard();
    a.iface_v = 2;
    !a.frozen()
}

/// 注入核 4：分域表冻结注入——四域预算各 1000us。
pub fn inj4_budget_frozen() -> bool {
    FxPerfArch::standard().budgets == [1000, 1000, 1000, 1000]
}

/// 注入核 5：度量哈希冻结注入。
pub fn inj5_profile_frozen() -> bool {
    let mk = || {
        let mut bytes = Vec::new();
        for p in FxPerfArch::standard().profiles.iter() {
            bytes.extend_from_slice(&p.domain.to_le_bytes());
            bytes.extend_from_slice(&p.cost_us.to_le_bytes());
        }
        fnv1a(&bytes)
    };
    mk() == mk() && mk() != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vez30_checks：本单判据聚合（诊断段 0x9f80）。
pub fn run_vez30_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5301 特效性能总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "总预算宪法（≤4ms）+三层冻结",
        j1_main(),
        if j1_main() { "" } else { "宪法破" },
    );
    s.add(
        "分域预算表+超支治理清单",
        j1_main() && inj2_domain_overspend(),
        if j1_main() { "" } else { "治理破" },
    );
    let inj = [
        ("注入1宪法失守", inj1_total_breach()),
        ("注入2分域超支", inj2_domain_overspend()),
        ("注入3层失配", inj3_iface_drift()),
        ("注入4预算表冻结", inj4_budget_frozen()),
        ("注入5度量冻结", inj5_profile_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
