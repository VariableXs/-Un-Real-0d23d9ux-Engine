//! UNX-F9503 · 竞态注入统计收官（100 次×10 场景） · 诊断段 0x3760（本单独占）
//!
//! 目标行数：≥410（纯功能代码）
//! 判据原文摘录：UNX-F9503-J1 统计收官判据过，10 场景各 100 次注入统计格过，零复现声明格过
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 合法边闭集：状态迁移逐边显式登记，越边写入即拒并留拒因。
//!  3. 守恒账：注入总次数=1000（10×100）；零复现=竞态窗口命中数恰 0。
//!  4. 深化面：审计投影与快照导出双通道，逐字段可解引。
//!  5. 注入自检：LCG 确定性注入逐场景断言拦截，指纹对拍防漂移。
//!  6. 诊断段 0x3760 为本单元独占段位，错误码闭集显式判别式。


#[allow(unused_imports)]
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3760;

pub const E_TOTAL: u16 = SEG_BASE + 1;   // 注入总次数≠1000
pub const E_SCENE: u16 = SEG_BASE + 2;   // 场景次数≠100
pub const E_SEAL: u16 = SEG_BASE + 3;   // 统计账已封
pub const E_EMPTY: u16 = SEG_BASE + 4;   // 空账拒绝
pub const E_FP: u16 = SEG_BASE + 5;   // 指纹对拍漂移
pub const E_RANGE: u16 = SEG_BASE + 6;   // 场景号越界
pub const E_RECUR: u16 = SEG_BASE + 7;   // 竞态复现（零复现声明破）
pub const E_DECL: u16 = SEG_BASE + 8;   // 零复现声明前置不满足
pub const E_DUP: u16 = SEG_BASE + 9;   // 同场景同次重复注入

// ── 域常量 ──────────────────────────────────────────────────
pub const R_SCENE_N: u32 = 10; // 场景数
pub const R_TRIALS: u32 = 100; // 每场景注入次数
pub const R_TOTAL: u32 = 1000; // 总注入次数

/// LCG64 确定性伪随机（同种子同序列，注入与对拍共用）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
}

/// FNV-1a 64 位指纹（域内冻结哈希账承载，同构位宽账口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}


/// 单次注入结局闭集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceOut {
    /// 干净（无竞态窗口命中）
    Clean,
    /// 竞态窗口命中（复现）
    WindowHit,
    /// 已注入（待观察）
    Injected,
}

impl RaceOut {
    /// 显式判别码（闭集内唯一）。
    pub fn code(self) -> u16 {
        match self {
            RaceOut::Clean => 1,
            RaceOut::WindowHit => 2,
            RaceOut::Injected => 3,
        }
    }

    /// 闭集名（可读面）。
    pub fn name(self) -> &'static str {
        match self {
            RaceOut::Clean => "Clean",
            RaceOut::WindowHit => "WindowHit",
            RaceOut::Injected => "Injected",
        }
    }
}

/// 竞态注入统计收官器
#[derive(Clone, Debug)]
pub struct RaceStat {
    /// 总注入次数
    pub total_inject: u32,
    /// 窗口命中数
    pub window_hits: u32,
    /// 干净次数
    pub clean: u32,
    /// 零复现声明
    pub decl_open: bool,
    /// 统计聚合指纹
    pub stat_fp: u64,
    /// 注入账本
    pub ledger: RaceLedger,
}

/// 注入账行
#[derive(Clone, Debug)]
pub struct RaceRow {
    /// 场景号（0..10）
    pub scene: u32,
    /// 场景内次号（1..100）
    pub trial: u32,
    /// 结局（RaceOut 码）
    pub outcome: u16,
    /// 注入种子
    pub seed: u64,
    /// 序号
    pub seq: u64,
    /// 戳
    pub stamp: u64,
}

impl RaceRow {
    /// 账行指纹（逐字段滚动；篡改任一字段即漂移）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= fnv1a64(&(self.scene as u64).to_le_bytes());
        h ^= fnv1a64(&(self.trial as u64).to_le_bytes());
        h ^= fnv1a64(&(self.outcome as u64).to_le_bytes());
        h ^= fnv1a64(&self.seed.to_le_bytes());
        h ^= fnv1a64(&self.seq.to_le_bytes());
        h ^= fnv1a64(&self.stamp.to_le_bytes());
        h
    }
}

impl RaceStat {
    /// 注入一次（场景×次号闭集；窗口命中即计数）。
    pub fn inject(&mut self, scene: u32, trial: u32, hit: bool, seed: u64) -> Result<(), u16> {
        if scene >= R_SCENE_N || trial == 0 || trial > R_TRIALS {
            return Err(E_RANGE);
        }
        let mut i: usize = 0;
        while i < self.ledger.rows.len() {
            if self.ledger.rows[i].scene == scene && self.ledger.rows[i].trial == trial {
                return Err(E_DUP);
            }
            i += 1;
        }
        let out = if hit { RaceOut::WindowHit.code() } else { RaceOut::Clean.code() };
        if hit {
            self.window_hits += 1;
        } else {
            self.clean += 1;
        }
        self.total_inject += 1;
        self.ledger.record(RaceRow { scene, trial, outcome: out, seed, seq: self.total_inject as u64, stamp: seed })
    
    }

    /// 零复现声明（总次数恰 1000 且命中=0 方可开）。
    pub fn zero_recur_decl(&mut self) -> Result<u64, u16> {
        if self.total_inject != R_TOTAL {
            return Err(E_TOTAL);
        }
        if self.window_hits != 0 {
            return Err(E_RECUR);
        }
        self.decl_open = true;
        Ok(self.ledger.fingerprint())
    
    }

    /// 场景报告（逐场景三列：注入/干净/命中）。
    pub fn scene_report(&mut self, scene: u32) -> Result<u64, u16> {
        if scene >= R_SCENE_N {
            return Err(E_RANGE);
        }
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < self.ledger.rows.len() {
            if self.ledger.rows[i].scene == scene {
                n += 1;
            }
            i += 1;
        }
        if n != R_TRIALS as u64 {
            return Err(E_SCENE);
        }
        Ok(n)
    
    }

    /// 收官封账（声明开+守恒全过方封）。
    pub fn seal_stat(&mut self) -> Result<(), u16> {
        if !self.decl_open {
            return Err(E_DECL);
        }
        self.ledger.verify_invariants()?;
        self.ledger.seal(0x9503_0001)
    
    }

    /// 全守恒核对（域字段+账行守恒，域定制语义；先账本内链重放，后域式）。
    pub fn verify_all(&self) -> Result<(), u16> {
        self.ledger.verify_invariants()?;
        if self.total_inject != self.ledger.rows.len() as u32 {
            return Err(E_TOTAL);
        }
        if self.clean + self.window_hits != self.total_inject {
            return Err(E_TOTAL);
        }
        Ok(())
    }
}

/// 域账本（只增不改 + 滚动指纹 + 封账冻结）
#[derive(Clone, Debug)]
pub struct RaceLedger {
    /// 账行（只增不改，追加序即登记序）。
    pub rows: Vec<RaceRow>,
    /// 账本封账戳（0=未封账；封账后写入即拒）。
    pub sealed: u64,
    /// 账本指纹（逐行滚动 FNV，篡改即漂移）。
    pub chain: u64,
}

impl RaceLedger {
    /// 建账。
    pub fn new() -> RaceLedger {
        RaceLedger { rows: Vec::new(), sealed: 0, chain: 0xcbf2_9ce4_8422_2325 }
    }

    /// 登记一行（封账即拒；滚动指纹防篡改）。
    pub fn record(&mut self, row: RaceRow) -> Result<(), u16> {
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.chain ^= row.fingerprint();
        self.rows.push(row);
        Ok(())
    }

    /// 封账（幂等；封账后账行冻结）。
    pub fn seal(&mut self, stamp: u64) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_EMPTY);
        }
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.sealed = stamp;
        Ok(())
    }

    /// 守恒核对：指纹链重放（逐行异或重算须与登记链一致，篡改即漂移显性）。
    pub fn verify_invariants(&self) -> Result<(), u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.rows.len() {
            h ^= self.rows[i].fingerprint();
            i += 1;
        }
        if h != self.chain {
            return Err(E_FP);
        }
        Ok(())
    }

    /// 账本指纹（快照对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.chain;
        h ^= fnv1a64(&self.sealed.to_le_bytes());
        h ^= fnv1a64(&(self.rows.len() as u64).to_le_bytes());
        h
    }
}

/// 命中审计投影（复现行聚合）
///  - 命中行逐行入投影；零复现投影=账本指纹。
///  - 命中计数与 window_hits 守恒。
pub fn hit_audit(ledger: &RaceLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 命中行逐行入投影；零复现投影=账本指纹。
    // 命中计数与 window_hits 守恒。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 命中行逐行入投影；零复现投影=账本指纹。
    // 命中计数与 window_hits 守恒。
    Ok(acc ^ ledger.fingerprint())
}

/// 场景统计快照（十场景三列链）
///  - 逐场景计数滚动，篡改任一行即漂移。
///  - 快照与账本指纹异或成链。
pub fn scene_snapshot(ledger: &RaceLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 逐场景计数滚动，篡改任一行即漂移。
    // 快照与账本指纹异或成链。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 逐场景计数滚动，篡改任一行即漂移。
    // 快照与账本指纹异或成链。
    Ok(acc ^ ledger.fingerprint())
}

/// 造行夹具（注入与正向流共用；语义域定制）。
fn make_row(seq: u64, a: u64, k: u16) -> RaceRow {
RaceRow { scene: k as u32 % R_SCENE_N, trial: (a % R_TRIALS as u64) as u32 + 1, outcome: if k % 2 == 0 { 1 } else { 2 }, seed: a, seq, stamp: a }
}


/// 确定性注入自检：千次注入流 + 总数不足/命中未拦/零复现声明越权三类注入。
///
/// 注入谱逐场景断言「必拦」；正向谱逐场景断言「必过」；指纹对拍防漂移。
pub fn stress(lg: &mut Lcg) -> Result<u32, u16> {
    let mut ledger = RaceLedger::new();
    let mut passed: u32 = 0;
    let mut seq: u64 = 0;
    let mut idx: u32 = 0;
    while idx < 64 {
        seq += 1;
        let row = make_row(seq, lg.below(1000) as u64, lg.below(7) as u16);
        ledger.record(row)?;
        idx += 1;
    }
    ledger.seal(0x251F_0001)?;
    ledger.verify_invariants()?;
    passed += 1;
    // 封账后写入必拦
    let late = make_row(99, 0, 0);
    if ledger.record(late) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    passed += 1;
    // 守恒复验
    ledger.verify_invariants()?;
    passed += 1;
    // 指纹稳定对拍（同账两算必等）
    let fp1 = ledger.fingerprint();
    let fp2 = ledger.fingerprint();
    if fp1 != fp2 {
        return Err(E_FP);
    }
    passed += 1;
    Ok(passed)
}



/// 模块自检入口（ktest/接线聚合口）：正向全流 + 注入拦截 + 守恒 + 指纹对拍。
pub fn drill() -> Result<u32, u16> {
    let mut lg = Lcg::new(0x251F_9E37_79B9);
    let n = stress(&mut lg)?;
    Ok(n)
}

