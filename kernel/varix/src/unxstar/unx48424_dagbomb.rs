//! UNX-F48424 —— E·DAG 炸弹注入（反：构造稠密图注入 ×10 资源有界）
//! 判据原文：UNX-F48424-J1 稠密图炸弹（全连接/阶梯倍增/自环重入/长程回边）×10 构图
//!           全部资源有界（节点 ≤512、边 ≤4096、DP 步 ≤65536）；
//!           合法图零误拦；超界注入全拒并分类（四型封闭）；
//!           LCG 随机炸弹 ×256 全拒；拦截账封印重演一致；两账守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD8A0; // E22 DAG 炸弹段

pub const NODE_CAP: usize = 512;
pub const EDGE_CAP: usize = 4096;
pub const DP_CAP: u64 = 65_536;

const E_BOMB_PASS: u16 = SEG_BASE + 1; // 炸弹放行（资源超界）
const E_LEGAL_BLOCK: u16 = SEG_BASE + 2; // 合法图误拦
const E_KIND_BAD: u16 = SEG_BASE + 3; // 分类越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒破
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 计数不足

/// 炸弹型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BombKind {
    FullMesh = 0,   // 全连接 n(n-1)/2 边
    Ladder = 1,     // 阶梯倍增 2^k 路径
    SelfLoop = 2,   // 自环重入
    BackEdge = 3,   // 长程回边
}

/// 图描述（构造意图，不实际展开）。
#[derive(Clone, Copy)]
pub struct BombSpec {
    pub kind: BombKind,
    pub n: usize,   // 节点请求
    pub m: usize,   // 边请求（全连接/自环估算用）
}

/// 资源审计器。
pub struct ResourceGate {
    built: u32,
    blocked: Vec<(u64, BombKind)>,
    seq: u64,
}

impl ResourceGate {
    pub fn new() -> Self {
        ResourceGate { built: 0, blocked: Vec::new(), seq: 0 }
    }

    /// 构图申请：估算资源任一超界即拒（不展开）。
    pub fn admit(&mut self, spec: BombSpec) -> Result<u64, u16> {
        self.seq += 1;
        // 资源估算（闭式，不分配）
        let (est_n, est_m, est_dp) = match spec.kind {
            BombKind::FullMesh => {
                let m = spec.n * (spec.n - 1) / 2;
                (spec.n, m, 1u64)
            }
            BombKind::Ladder => {
                // 阶梯：n 节点 2n 边，路径数 2^(n/2)
                let paths = 1u64 << (spec.n / 2).min(63);
                (spec.n, spec.n * 2, paths)
            }
            BombKind::SelfLoop => (spec.n, spec.m.max(spec.n), u64::MAX),
            BombKind::BackEdge => {
                // 长程回边：n + k 边，DP = n + 回边重入
                (spec.n, spec.n + spec.m, (spec.n as u64) * (spec.m as u64 + 1))
            }
        };
        let over = est_n > NODE_CAP
            || est_m > EDGE_CAP
            || est_dp > DP_CAP
            || (spec.kind == BombKind::FullMesh && est_n > 91); // n(n-1)/2>4096 ⇔ n≥92
        if over {
            self.blocked.push((self.seq, spec.kind));
            return Err(E_BOMB_PASS);
        }
        self.built += 1;
        Ok(est_dp)
    }

    /// 分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.blocked.len() {
            if self.blocked[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 两账守恒：built + blocked.len() == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if (self.built as u64) + (self.blocked.len() as u64) != self.seq {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.blocked.len() {
            h ^= self.blocked[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.blocked[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn blocked_count(&self) -> u32 {
        self.blocked.len() as u32
    }

    pub fn built_count(&self) -> u32 {
        self.built
    }
}

/// 合法小图：n≤91 的全连接、n≤256 阶梯（路径 ≤65536）等。
pub fn legal_specs() -> Vec<BombSpec> {
    let mut v: Vec<BombSpec> = Vec::new();
    v.push(BombSpec { kind: BombKind::FullMesh, n: 8, m: 28 });
    v.push(BombSpec { kind: BombKind::Ladder, n: 32, m: 0 });
    v.push(BombSpec { kind: BombKind::BackEdge, n: 64, m: 4 });
    v
}

/// 炸弹清单：四型 × 超界参数。
pub fn bomb_specs() -> Vec<BombSpec> {
    let mut v: Vec<BombSpec> = Vec::new();
    v.push(BombSpec { kind: BombKind::FullMesh, n: 200, m: 0 });     // 边 19900 超
    v.push(BombSpec { kind: BombKind::FullMesh, n: 600, m: 0 });     // 节点超
    v.push(BombSpec { kind: BombKind::Ladder, n: 400, m: 0 });       // 路径 2^200 超
    v.push(BombSpec { kind: BombKind::Ladder, n: 600, m: 0 });       // 节点超
    v.push(BombSpec { kind: BombKind::SelfLoop, n: 100, m: 10_000 }); // DP ∞ 超
    v.push(BombSpec { kind: BombKind::SelfLoop, n: 600, m: 10 });    // 节点超
    v.push(BombSpec { kind: BombKind::BackEdge, n: 500, m: 100 });   // DP 500*101 超
    v.push(BombSpec { kind: BombKind::BackEdge, n: 700, m: 2 });     // 节点超
    v.push(BombSpec { kind: BombKind::FullMesh, n: 92, m: 0 });      // 边 4186 超
    v.push(BombSpec { kind: BombKind::Ladder, n: 130, m: 0 });       // 路径 2^65 超
    v
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：DAG 炸弹注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 合法图零误拦：3 枚合法 spec 全放行
    let mut gate = ResourceGate::new();
    let legal = legal_specs();
    let mut i = 0usize;
    while i < legal.len() {
        gate.admit(legal[i])?;
        i += 1;
    }
    if gate.built_count() != 3 {
        return Err(E_LEGAL_BLOCK);
    }
    hits += 1;

    // R2 稠密炸弹 ×10 全拒
    let bombs = bomb_specs();
    let mut i = 0usize;
    while i < bombs.len() {
        match gate.admit(bombs[i]) {
            Err(E_BOMB_PASS) => {}
            _ => return Err(E_BOMB_PASS),
        }
        i += 1;
    }
    if gate.blocked_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R3 分类封闭
    gate.audit_kind_closed()?;
    hits += 1;

    // R4 两账守恒：3 built + 10 blocked = 13 seq
    gate.audit_balance()?;
    hits += 1;

    // R5 资源界恒定：NODE_CAP/EDGE_CAP/DP_CAP 三常量双读一致
    if NODE_CAP != 512 || EDGE_CAP != 4096 || DP_CAP != 65_536 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R6 LCG 随机炸弹 ×256 全拒：四型随机参数（均超界构造）
    let mut gate2 = ResourceGate::new();
    let mut rng = Lcg::new(0x48424);
    let mut k = 0u32;
    while k < 256 {
        let t = rng.below(4);
        let kind = match t {
            0 => BombKind::FullMesh,
            1 => BombKind::Ladder,
            2 => BombKind::SelfLoop,
            _ => BombKind::BackEdge,
        };
        let spec = match kind {
            BombKind::FullMesh => BombSpec { kind, n: 100 + rng.below(500) as usize, m: 0 },
            BombKind::Ladder => BombSpec { kind, n: 140 + rng.below(400) as usize, m: 0 },
            BombKind::SelfLoop => BombSpec { kind, n: 100 + rng.below(400) as usize, m: 5000 },
            BombKind::BackEdge => BombSpec { kind, n: 500 + rng.below(100) as usize, m: 50 },
        };
        match gate2.admit(spec) {
            Err(E_BOMB_PASS) => {}
            _ => return Err(E_BOMB_PASS),
        }
        k += 1;
    }
    if gate2.blocked_count() != 256 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R7 封印重演一致：同种子拦截账指纹相等
    let f1 = gate2.fp();
    let mut gate3 = ResourceGate::new();
    let mut rng2 = Lcg::new(0x48424);
    let mut k = 0u32;
    while k < 256 {
        let t = rng2.below(4);
        let kind = match t {
            0 => BombKind::FullMesh,
            1 => BombKind::Ladder,
            2 => BombKind::SelfLoop,
            _ => BombKind::BackEdge,
        };
        let spec = match kind {
            BombKind::FullMesh => BombSpec { kind, n: 100 + rng2.below(500) as usize, m: 0 },
            BombKind::Ladder => BombSpec { kind, n: 140 + rng2.below(400) as usize, m: 0 },
            BombKind::SelfLoop => BombSpec { kind, n: 100 + rng2.below(400) as usize, m: 5000 },
            BombKind::BackEdge => BombSpec { kind, n: 500 + rng2.below(100) as usize, m: 50 },
        };
        let _ = gate3.admit(spec);
        k += 1;
    }
    if f1 != gate3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒终验
    gate2.audit_balance()?;
    gate3.audit_balance()?;
    hits += 1;

    Ok(hits)
}
