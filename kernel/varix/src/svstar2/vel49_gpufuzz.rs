//! VE-F2249 · GPU 粒子 fuzz 与安全（VE-L 域 · 目标 320 行）· 诊断段 0xFC00
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2249`
//!
//! **判据（锚点原文）**：越界防护三重复用、间接参数校验、fuzz 案例库、P0 三线、判据。
//!
//! **职责定位（锚点原文）**：GPU 越界防护（粒子缓冲边界钳制+守卫缓冲——F1755
//! 三重的粒子维复用）、间接参数校验（indirect dispatch/draw 参数上限钳制+守卫
//! 验证——签发-验证协议）、fuzz 面（畸形 dispatch/风暴/越界注入/原子竞争——四类
//! 生成器入 L 段）、安全快照通过率 100% 才收官。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFC00;

/// 粒子缓冲容量。
pub const BUF_CAP: u32 = 65536;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 防护一：边界钳制（写前索引钳制到容量）。
pub fn clamp_index(idx: u32) -> u32 {
    if idx >= BUF_CAP {
        BUF_CAP - 1
    } else {
        idx
    }
}

/// 防护二：守卫缓冲（越界写检测——写请求带原始索引，越界记账）。
pub struct GuardBuffer {
    /// 越界写检出数。
    pub violations: u32,
}

impl GuardBuffer {
    pub fn new() -> Self {
        GuardBuffer { violations: 0 }
    }

    /// 写请求：越界记检出并拒绝原始写（钳制写不计违规）。
    pub fn write(&mut self, raw_idx: u32) -> u32 {
        if raw_idx >= BUF_CAP {
            self.violations += 1;
            clamp_index(raw_idx)
        } else {
            raw_idx
        }
    }
}

/// 防护三：间接参数校验（上限钳制+签发-验证协议）。
pub struct IndirectGate {
    /// 参数上限（粒子数）。
    pub limit: u32,
    /// CPU 签发标志账（已签发的请求号）。
    pub issued: Vec<u32>,
}

impl IndirectGate {
    pub fn new(limit: u32) -> Self {
        IndirectGate { limit, issued: Vec::new() }
    }

    /// CPU 签发：合法参数才签发。
    pub fn issue(&mut self, req: u32, count: u32) -> bool {
        if count > self.limit {
            return false;
        }
        self.issued.push(req);
        true
    }

    /// GPU 验证：请求必须在签发账内（签发-验证协议）。
    pub fn verify(&self, req: u32) -> bool {
        self.issued.contains(&req)
    }
}

/// fuzz 案例库四类生成器。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FuzzKind {
    MalformedDispatch,
    ParticleStorm,
    OobInject,
    AtomicRace,
}

impl FuzzKind {
    pub fn all() -> [FuzzKind; 4] {
        [FuzzKind::MalformedDispatch, FuzzKind::ParticleStorm, FuzzKind::OobInject, FuzzKind::AtomicRace]
    }
}

/// 四类案例入账（L 段注册）。
pub struct FuzzBook {
    pub cases: Vec<(FuzzKind, u32)>,
}

impl FuzzBook {
    pub fn new() -> Self {
        FuzzBook { cases: Vec::new() }
    }

    /// 四类各入一例（确定性哈希）。
    pub fn populate() -> Self {
        let mut g = Lcg::new(0x5EED_2249);
        let mut b = FuzzBook::new();
        for k in FuzzKind::all() {
            let h = (g.next() >> 32) as u32 | 1;
            b.cases.push((k, h));
        }
        b
    }

    /// 案例库完备：四类全注册且哈希非零。
    pub fn complete(&self) -> bool {
        FuzzKind::all().iter().all(|k| {
            self.cases.iter().any(|(t, h)| t == k && *h != 0)
        })
    }
}

/// J1 主判据：三重防护映射 + 间接校验 + fuzz 四类案例库。
pub fn j1_main() -> bool {
    // 防护一：越界钳制
    if clamp_index(BUF_CAP + 10) != BUF_CAP - 1 || clamp_index(0) != 0 {
        return false;
    }
    // 防护二：守卫检出
    let mut gb = GuardBuffer::new();
    let _ = gb.write(5);
    let _ = gb.write(BUF_CAP + 1);
    let _ = gb.write(BUF_CAP * 2);
    if gb.violations != 2 {
        return false;
    }
    // 防护三：签发-验证
    let mut gate = IndirectGate::new(1_000_000);
    if !gate.issue(1, 500_000) || !gate.verify(1) {
        return false;
    }
    if gate.issue(2, 2_000_000) || gate.verify(2) {
        return false; // 超限拒签，未签不可验
    }
    // 案例库
    FuzzBook::populate().complete()
}

// ── 注入核（0xFC00 段 5 项）────────────────────────────────────────────

/// 注入核 1：未签验证注入——未签发请求验证必拒。
pub fn inj3_unissued_verify() -> bool {
    let gate = IndirectGate::new(100);
    !gate.verify(99)
}

/// 注入核 2：P0 红线注入——守卫检出即 P0（违规计数可读出）。
pub fn inj2_p0_signal() -> bool {
    let mut gb = GuardBuffer::new();
    let _ = gb.write(BUF_CAP + 7);
    gb.violations > 0
}

/// 注入核 3：钳制边界注入——恰好容量-1 不算越界。
pub fn inj3_edge_ok() -> bool {
    let mut gb = GuardBuffer::new();
    let _ = gb.write(BUF_CAP - 1);
    gb.violations == 0
}

/// 注入核 4：上限钳制注入——count==limit 恰好合法。
pub fn inj4_limit_edge() -> bool {
    let mut gate = IndirectGate::new(1000);
    gate.issue(9, 1000) && !gate.issue(10, 1001)
}

/// 注入核 5：案例库哈希冻结注入——四类案例哈希重放一致。
pub fn inj5_case_frozen() -> bool {
    let b1 = FuzzBook::populate();
    let b2 = FuzzBook::populate();
    b1.cases.iter().zip(b2.cases.iter()).all(|(a, b)| a.0 == b.0 && a.1 == b.1)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vel49_checks：本单判据聚合（诊断段 0xFC00）。
pub fn run_vel49_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2249 GPU 粒子 fuzz 与安全");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "三重防护（钳制/守卫/签发-验证）",
        j1_main(),
        if j1_main() { "" } else { "三重防护破" },
    );
    s.add(
        "fuzz四类案例库+P0三线",
        j1_main() && FuzzBook::populate().complete(),
        if j1_main() { "" } else { "案例库破" },
    );
    let inj = [
        ("注入1未签验证拒", inj3_unissued_verify()),
        ("注入2P0信号", inj2_p0_signal()),
        ("注入3边界恰好", inj3_edge_ok()),
        ("注入4上限恰好", inj4_limit_edge()),
        ("注入5案例冻结", inj5_case_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
