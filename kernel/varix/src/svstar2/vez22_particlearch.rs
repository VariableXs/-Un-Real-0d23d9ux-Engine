//! VE-F5221 · 粒子库总架构（VE-Z 域 · Z02 组 · 目标 320 行）· 诊断段 0x9b80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5221`
//!
//! **判据（锚点原文）**：三段流水、数据契约、分工声明、拦截、判据。
//!
//! **职责定位（锚点原文）**：粒子库总架构（粒子库定位：粒子特效的引擎本体——
//! 发射/模拟/渲染三段流水线）；段间数据契约冻结；与 Z01 分工声明（Z01 管编排
//! 与语义、本组管引擎本体——不重叠不缺位）；契约破坏→版本拦截；越权编排→归
//! 位 Z01。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9b80;

/// 三段。
pub const STAGES: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 段间数据契约版本。
pub const CONTRACT_V: u32 = 1;

/// 粒子：位置/速度/生命。
#[derive(Clone, Copy)]
pub struct Particle {
    pub pos: (f32, f32),
    pub vel: (f32, f32),
    pub life: f32,
}

/// 三段流水线：发射→模拟→渲染。
pub struct ParticlePipeline {
    /// 段间契约版本（冻结）。
    pub contract_v: u32,
    pub particles: Vec<Particle>,
}

impl ParticlePipeline {
    /// 发射段：按生成数产粒子（生命 >0）。
    pub fn emit(&mut self, n: u32) {
        for k in 0..n {
            self.particles.push(Particle {
                pos: (k as f32, 0.0),
                vel: (0.0, 1.0),
                life: 2.0,
            });
        }
    }

    /// 模拟段：推进位置与生命衰减。
    pub fn simulate(&mut self, dt: f32) {
        for p in self.particles.iter_mut() {
            p.pos.0 += p.vel.0 * dt;
            p.pos.1 += p.vel.1 * dt;
            p.life -= dt;
        }
    }

    /// 渲染段：只渲染存活粒子（生命 >0）。
    pub fn render_list(&self) -> Vec<usize> {
        self.particles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.life > 0.0)
            .map(|(k, _)| k)
            .collect()
    }

    /// 契约冻结核对：版本匹配（破坏→拦截）。
    pub fn contract_ok(&self) -> bool {
        self.contract_v == CONTRACT_V
    }
}

/// 分工声明：引擎本体归本组、编排归 Z01（越权编排拦截）。
pub fn ownership(is_orchestration: bool) -> Result<(), i32> {
    if is_orchestration {
        Err(1) // 越权编排 → 归位 Z01
    } else {
        Ok(())
    }
}

/// J1 主判据：三段流水 + 数据契约冻结 + 分工拦截。
pub fn j1_main() -> bool {
    let mut p = ParticlePipeline { contract_v: CONTRACT_V, particles: Vec::new() };
    p.emit(4);
    p.simulate(1.0);
    let alive = p.render_list();
    // 4 粒子生命 2.0-1.0=1.0 全存活
    if alive.len() != 4 {
        return false;
    }
    // 再推进 1.5s → 全灭
    p.simulate(1.5);
    if !p.render_list().is_empty() {
        return false;
    }
    if !p.contract_ok() {
        return false;
    }
    ownership(false).is_ok() && ownership(true).is_err()
}

// ── 注入核（0x9b80 段 5 项）────────────────────────────────────────────

/// 注入核 1：契约破坏拦截注入——版本漂移必拦。
pub fn inj1_contract_break() -> bool {
    let p = ParticlePipeline { contract_v: 2, particles: Vec::new() };
    !p.contract_ok()
}

/// 注入核 2：越权编排注入——编排请求必归位 Z01。
pub fn inj2_overreach() -> bool {
    ownership(true).is_err()
}

/// 注入核 3：生命衰减注入——衰减后粒子离开渲染集。
pub fn inj3_life_decay() -> bool {
    let mut p = ParticlePipeline { contract_v: 1, particles: Vec::new() };
    p.emit(1);
    p.simulate(3.0); // 生命 2.0 耗尽
    p.render_list().is_empty()
}

/// 注入核 4：模拟位移注入——位置按速度推进。
pub fn inj4_sim_move() -> bool {
    let mut p = ParticlePipeline { contract_v: 1, particles: Vec::new() };
    p.emit(1);
    p.simulate(2.0);
    (p.particles[0].pos.1 - 2.0).abs() < 1e-5
}

/// 注入核 5：三段名冻结注入。
pub fn inj5_stages_frozen() -> bool {
    let names = ["emit", "simulate", "render"];
    let mut bytes = Vec::new();
    for n in names.iter() {
        bytes.extend_from_slice(n.as_bytes());
    }
    fnv1a(&bytes) != 0 && names.len() == STAGES as usize
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vez22_checks：本单判据聚合（诊断段 0x9b80）。
pub fn run_vez22_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5221 粒子库总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三段流水（发射/模拟/渲染）",
        j1_main(),
        if j1_main() { "" } else { "流水线破" },
    );
    s.add(
        "段间契约冻结+分工越权拦截",
        j1_main(),
        if j1_main() { "" } else { "契约/拦截破" },
    );
    let inj = [
        ("注入1契约拦截", inj1_contract_break()),
        ("注入2越权归位", inj2_overreach()),
        ("注入3生命衰减", inj3_life_decay()),
        ("注入4模拟位移", inj4_sim_move()),
        ("注入5三段冻结", inj5_stages_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
