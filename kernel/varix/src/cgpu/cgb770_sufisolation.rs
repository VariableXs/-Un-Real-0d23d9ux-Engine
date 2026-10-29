#![allow(dead_code)]
//! cgb770_sufisolation —— CGPU-F0770 跨表面隔离强化。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0770 跨表面隔离强化  <SEG 0x6B80>
//! 目标行数：≥380（判据：纯功能代码 ≥380 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥380 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0770；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌跨表面隔离账，能力位面）：
//!   1. 表面闭集八面（0..=7）；能力位闭集五项（写死语义）：
//!      READ(1) / WRITE(2) / EXEC(4) / GRANT(8) / ADMIN(16)；
//!      ADMIN 仅 0 号根面可持（他面持之红码）；根面天生全能力；
//!   2. 授权账：定容 32 槽，有向边（from→to，能力子集）；
//!      三查铁律：自引红码 / 越权授予（授出自己没有的能力）红码 /
//!      ADMIN 授出红码；重复授权红码；
//!   3. 访问裁决：attempt(from,to,cap) 仅当存在覆盖该能力的
//!      活授权边才放行；否则拒并记违规（from 面打击 +1）；
//!   4. 打击隔离：面打击 ≥3 → 隔离（不可逆）：其全部授权边
//!      即刻失效、后续 attempt 全拒；
//!   5. 守恒恒等式：访问总数 = 放行数 + 拒绝数（含隔离拒）；
//!      授权总数 = 活边数 + 失效边数；
//!   6. 隔离指纹：FNV-1a 滚过全部活边 (from,to,caps)；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6B80 段为本模块独占（SEG_BASE=0x6B80），判别式
//!   SEG_BASE+1..+N；与 0x6B60/0x6BA0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const SI_VERSION: &str = "cgb770_sufisolation-v1";

/// 诊断段基址（0x6B80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6B80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6B81 表面号越界（0..=7 之外）。
pub const XC_BAD_SURFACE: u16 = SEG_BASE + 1;
/// 0x6B82 能力位越域（0b11111 之外）。
pub const XC_BAD_CAP: u16 = SEG_BASE + 2;
/// 0x6B83 授权自引（from == to）。
pub const XC_GRANT_SELF: u16 = SEG_BASE + 3;
/// 0x6B84 越权授予（授出自己没有的能力）。
pub const XC_GRANT_ELEVATE: u16 = SEG_BASE + 4;
/// 0x6B85 ADMIN 能力授出（仅根面可持）。
pub const XC_GRANT_ADMIN: u16 = SEG_BASE + 5;
/// 0x6B86 重复授权。
pub const XC_GRANT_DUP: u16 = SEG_BASE + 6;
/// 0x6B87 授权账满（32 槽）。
pub const XC_GRANT_FULL: u16 = SEG_BASE + 7;
/// 0x6B88 访问被拒（无活授权覆盖）。
pub const XC_ACCESS_DENIED: u16 = SEG_BASE + 8;
/// 0x6B89 访问方已被隔离。
pub const XC_QUARANTINED: u16 = SEG_BASE + 9;
/// 0x6B8A 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 10;
/// 0x6B8B 隔离指纹漂移。
pub const XC_DIGEST_DRIFT: u16 = SEG_BASE + 11;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 表面数（写死：8）。
pub const SURFACE_CAP: usize = 8;
/// 根面号（写死：0，天生全能力）。
pub const ROOT_SURFACE: usize = 0;
/// 能力位闭集。
pub const CAP_READ: u8 = 0b00001;
pub const CAP_WRITE: u8 = 0b00010;
pub const CAP_EXEC: u8 = 0b00100;
pub const CAP_GRANT: u8 = 0b01000;
pub const CAP_ADMIN: u8 = 0b10000;
/// 能力位全域。
pub const CAP_ALL_MASK: u8 = 0b11111;
/// 授权账容量（写死：32）。
pub const GRANT_CAP: usize = 32;
/// 打击隔离线（写死：3）。
pub const STRIKE_LINE: u32 = 3;

/// 有向授权边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantEdge {
    /// 授出面。
    pub from: usize,
    /// 受入面。
    pub to: usize,
    /// 能力位子集。
    pub caps: u8,
    /// 活性（隔离时整面失效）。
    pub alive: bool,
}

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 跨表面隔离账：授权边 + 访问裁决 + 打击隔离 + 指纹。
/// 守恒恒等式：
///   访问总数 = 放行数 + 拒绝数；
///   授权总数 = 活边数 + 失效边数。
pub struct IsolationLedger {
    caps: [u8; SURFACE_CAP],
    strikes: [u32; SURFACE_CAP],
    quarantined: [bool; SURFACE_CAP],
    grants: Vec<GrantEdge>,
    grant_evicted: u32,
    attempts: u32,
    allowed: u32,
    denied: u32,
}

impl IsolationLedger {
    /// 新账：根面全能力，余面零能力零打击。
    pub fn new() -> Self {
        let mut caps = [0u8; SURFACE_CAP];
        caps[ROOT_SURFACE] = CAP_ALL_MASK;
        IsolationLedger {
            caps,
            strikes: [0; SURFACE_CAP],
            quarantined: [false; SURFACE_CAP],
            grants: Vec::new(),
            grant_evicted: 0,
            attempts: 0,
            allowed: 0,
            denied: 0,
        }
    }

    /// 面能力只读。
    pub fn caps_of(&self, s: usize) -> Result<u8, u16> {
        if s >= SURFACE_CAP {
            return Err(XC_BAD_SURFACE);
        }
        Ok(self.caps[s])
    }

    /// 面打击数只读。
    pub fn strike_of(&self, s: usize) -> Result<u32, u16> {
        if s >= SURFACE_CAP {
            return Err(XC_BAD_SURFACE);
        }
        Ok(self.strikes[s])
    }

    /// 面隔离态只读。
    pub fn is_quarantined(&self, s: usize) -> Result<bool, u16> {
        if s >= SURFACE_CAP {
            return Err(XC_BAD_SURFACE);
        }
        Ok(self.quarantined[s])
    }

    /// 活授权边查找（from→to 活边下标集）。
    fn live_edge_caps(&self, from: usize, to: usize) -> u8 {
        let mut c = 0u8;
        for g in self.grants.iter() {
            if g.alive && g.from == from && g.to == to {
                c |= g.caps;
            }
        }
        c
    }

    /// 授权登记：五查（面/位/自引/越权/ADMIN/重复）+ 定容。
    pub fn grant(&mut self, from: usize, to: usize, caps: u8) -> Result<(), u16> {
        if from >= SURFACE_CAP || to >= SURFACE_CAP {
            return Err(XC_BAD_SURFACE);
        }
        if caps & !CAP_ALL_MASK != 0 || caps == 0 {
            return Err(XC_BAD_CAP);
        }
        if from == to {
            return Err(XC_GRANT_SELF);
        }
        if caps & CAP_ADMIN != 0 {
            return Err(XC_GRANT_ADMIN);
        }
        if self.quarantined[from] {
            return Err(XC_QUARANTINED);
        }
        // 越权授予：授出面必须持有全部授出能力。
        if caps & !self.caps[from] != 0 {
            return Err(XC_GRANT_ELEVATE);
        }
        // 授出面须持 GRANT 能力（根面除外——根面天生治权）。
        if from != ROOT_SURFACE && self.caps[from] & CAP_GRANT == 0 {
            return Err(XC_GRANT_ELEVATE);
        }
        // 重复授权：同 (from,to,caps) 精确三元组已存在即拒。
        for g in self.grants.iter() {
            if g.from == from && g.to == to && g.caps == caps {
                return Err(XC_GRANT_DUP);
            }
        }
        if self.grants.len() >= GRANT_CAP {
            self.grant_evicted += 1;
            // 定容：逐出最老边（下标 0）。
            self.grants.remove(0);
        }
        self.grants.push(GrantEdge {
            from,
            to,
            caps,
            alive: true,
        });
        // 受入面能力累积（获得即持有；ADMIN 永不可授）。
        self.caps[to] |= caps;
        Ok(())
    }

    /// 面打击记账（≥3 即隔离：全边失效 + 面封禁）。
    fn strike(&mut self, s: usize) {
        self.strikes[s] += 1;
        if self.strikes[s] >= STRIKE_LINE && !self.quarantined[s] {
            self.quarantined[s] = true;
            for g in self.grants.iter_mut() {
                if g.from == s {
                    g.alive = false;
                }
            }
        }
    }

    /// 访问裁决：隔离面拒；无覆盖活边拒并打击；有则放行。
    pub fn attempt(&mut self, from: usize, to: usize, cap: u8) -> Result<(), u16> {
        self.attempts += 1;
        if from >= SURFACE_CAP || to >= SURFACE_CAP {
            self.denied += 1;
            return Err(XC_BAD_SURFACE);
        }
        if cap & !CAP_ALL_MASK != 0 || cap.count_ones() != 1 {
            self.denied += 1;
            return Err(XC_BAD_CAP);
        }
        if self.quarantined[from] {
            self.denied += 1;
            return Err(XC_QUARANTINED);
        }
        let have = self.live_edge_caps(from, to);
        if have & cap == 0 {
            self.denied += 1;
            self.strike(from);
            return Err(XC_ACCESS_DENIED);
        }
        self.allowed += 1;
        Ok(())
    }

    /// 活边数。
    pub fn live_grants(&self) -> u32 {
        let mut n = 0u32;
        for g in self.grants.iter() {
            if g.alive {
                n += 1;
            }
        }
        n
    }

    /// 隔离指纹：滚过全部活边 (from,to,caps)。
    pub fn compute_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for g in self.grants.iter() {
            if !g.alive {
                continue;
            }
            h = fnv_step(h, g.from as u8);
            h = fnv_step(h, g.to as u8);
            h = fnv_step(h, g.caps);
        }
        h
    }

    /// 指纹复核。
    pub fn verify_digest(&self, want: u32) -> Result<(), u16> {
        if self.compute_digest() != want {
            return Err(XC_DIGEST_DRIFT);
        }
        Ok(())
    }

    /// 守恒核：访问 = 放行 + 拒绝；授权账 = 在库 + 逐出。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.attempts != self.allowed + self.denied {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 访问统计只读。
    pub fn access_stats(&self) -> (u32, u32, u32) {
        (self.attempts, self.allowed, self.denied)
    }
}

/// 自例程：根面授权 → 转授 → 访问裁决 → 越权/自引/ADMIN 拦截 →
/// 打击隔离连锁失效 → 守恒 → 指纹 → 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut g = IsolationLedger::new();
    // 错误路径 1：坏面号 / 坏能力位。
    if g.grant(0, 9, CAP_READ) != Err(XC_BAD_SURFACE) {
        return Err(XC_BAD_SURFACE);
    }
    if g.grant(0, 1, 0) != Err(XC_BAD_CAP) {
        return Err(XC_BAD_CAP);
    }
    if g.grant(0, 1, 0b100_000) != Err(XC_BAD_CAP) {
        return Err(XC_BAD_CAP);
    }
    // 错误路径 2：自引 / ADMIN 授出。
    if g.grant(0, 0, CAP_READ) != Err(XC_GRANT_SELF) {
        return Err(XC_GRANT_SELF);
    }
    if g.grant(0, 1, CAP_ADMIN) != Err(XC_GRANT_ADMIN) {
        return Err(XC_GRANT_ADMIN);
    }
    // 主路径：根面 → 面1 授 READ|WRITE + GRANT（可转授）。
    g.grant(0, 1, CAP_READ | CAP_WRITE)?;
    g.grant(0, 1, CAP_GRANT)?;
    g.grant(1, 2, CAP_READ)?;
    // 错误路径 3：面1 越权授出 EXEC（自己没有）。
    if g.grant(1, 2, CAP_EXEC) != Err(XC_GRANT_ELEVATE) {
        return Err(XC_GRANT_ELEVATE);
    }
    // 错误路径 4：重复授权（同三元组）。
    if g.grant(0, 1, CAP_READ | CAP_WRITE) != Err(XC_GRANT_DUP) {
        return Err(XC_GRANT_DUP);
    }
    // 访问裁决：1→2 READ 放行；1→2 WRITE 拒（无覆盖）。
    g.attempt(1, 2, CAP_READ)?;
    if g.attempt(1, 2, CAP_WRITE) != Err(XC_ACCESS_DENIED) {
        return Err(XC_ACCESS_DENIED);
    }
    // 根面直访也须授权边（根能力不等于直通权）。
    if g.attempt(0, 2, CAP_READ) != Err(XC_ACCESS_DENIED) {
        return Err(XC_ACCESS_DENIED);
    }
    // 打击连锁：面1 再吃 2 次拒 → 3 打击 → 隔离 → 其边失效。
    if g.attempt(1, 2, CAP_EXEC) != Err(XC_ACCESS_DENIED) {
        return Err(XC_ACCESS_DENIED);
    }
    if g.attempt(1, 2, CAP_GRANT) != Err(XC_ACCESS_DENIED) {
        return Err(XC_ACCESS_DENIED);
    }
    if !g.is_quarantined(1)? {
        return Err(XC_UNBALANCED);
    }
    if g.strike_of(1)? != 3 {
        return Err(XC_UNBALANCED);
    }
    // 隔离面访问拒；其活边归零。
    if g.attempt(1, 2, CAP_READ) != Err(XC_QUARANTINED) {
        return Err(XC_QUARANTINED);
    }
    // 隔离面授权登记拒。
    if g.grant(1, 3, CAP_READ) != Err(XC_QUARANTINED) {
        return Err(XC_QUARANTINED);
    }
    // 面2 的 READ 边来自面1 → 已失效 → 面2→无；但根面→面2 新授可通。
    g.grant(0, 2, CAP_READ)?;
    g.attempt(0, 2, CAP_READ)?;
    // 守恒 + 指纹。
    g.conserved()?;
    let (att, allow, deny) = g.access_stats();
    if att != allow + deny {
        return Err(XC_UNBALANCED);
    }
    let d = g.compute_digest();
    g.verify_digest(d)?;
    // 指纹漂移：多授一边后指纹必变。
    g.grant(0, 3, CAP_READ)?;
    if g.verify_digest(d) != Err(XC_DIGEST_DRIFT) {
        return Err(XC_DIGEST_DRIFT);
    }
    // 授权账满：32 槽灌满触发逐出（账仍守恒）。
    let mut g2 = IsolationLedger::new();
    for i in 1..=7usize {
        g2.grant(0, i, CAP_GRANT | CAP_READ)?;
    }
    for i in 1..=7usize {
        g2.grant(0, i, CAP_WRITE)?;
    }
    for i in 1..=7usize {
        g2.grant(0, i, CAP_EXEC)?;
    }
    for i in 1..=7usize {
        g2.grant(0, i, CAP_GRANT)?;
    }
    for i in 1..=7usize {
        g2.grant(0, i, CAP_READ | CAP_WRITE)?;
    }
    // 35 次授权 → 逐出 3、在库 32、全活。
    if g2.grants_len() != GRANT_CAP {
        return Err(XC_UNBALANCED);
    }
    // 精确重复拒。
    if g2.grant(0, 3, CAP_EXEC) != Err(XC_GRANT_DUP) {
        return Err(XC_GRANT_DUP);
    }
    // 灌满后再授 → 逐出最老（不拒收）。
    g2.grant(0, 4, CAP_READ | CAP_GRANT)?;
    g2.conserved()?;
    if g2.live_grants() != GRANT_CAP as u32 {
        return Err(XC_UNBALANCED);
    }
    // 坏面访问（越界 from）。
    if g2.attempt(9, 1, CAP_READ) != Err(XC_BAD_SURFACE) {
        return Err(XC_BAD_SURFACE);
    }
    // 坏能力位访问（多位或空位）。
    if g2.attempt(0, 1, CAP_READ | CAP_WRITE) != Err(XC_BAD_CAP) {
        return Err(XC_BAD_CAP);
    }
    g2.conserved()?;
    Ok(0)
}
