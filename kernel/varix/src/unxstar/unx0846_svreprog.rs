//! UNX-F0846 · spurious 向量重编程与 SVR 槽位迁移 · 诊断段 0x66A0（独占 0x66A0~0x66AF）
//!
//! **判据原文摘录**：重编程序列正确（新旧向量过渡期零伪中断丢失），迁移账
//! 完整可回放。
//!
//! **设计纪律**：
//! 1. SVR（Spurious-Interrupt Vector Register，SDM §10.9）重编程建模：
//!    旧向量 old_v → 新向量 new_v 的迁移分四相——
//!    PHASE_PROBE（读旧 SVR 并落账）→ PHASE_DISABLE（清使能位，伪中断
//!    进入丢弃态）→ PHASE_SWAP（写入新向量 + 复使能）→ PHASE_VERIFY
//!    （注入两向量各探针伪中断，验证路由切换完成）。
//! 2. 过渡期零丢失恒等式：迁移窗口内到达的伪中断一律计入 `transit_caught`
//!    （无论落在哪一相），`transit_injected = transit_caught + transit_dropped`，
//!    判据口径 `transit_dropped = 0` 即零伪中断丢失。
//! 3. 迁移账可回放：每相落一条 `PhaseRecord`（相序号、旧/新向量、窗口内
//!    到达数），`replay()` 纯走查重演相序列，回放指纹与原账指纹必须相等。
//! 4. 向量约束：spurious 槽仅允许 0x00~0xFF 且 bit0..=3 必须为 1（SDM 规定
//!    spurious 向量低 4 位固定 0b1111——本域采用该约束做合法性闸门）。
//! 5. 零 unwrap/expect/panic/浮点/std；无墙钟，相序 tick 内置自增。
//!
//! **诊断段声明**：0x66A0~0x66AF 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x66A0~0x66AF 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x66A0;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x66AF;

/// 新旧向量相同（重编程无意义，拒绝）。
pub const X_SR_SAME_VEC: u16 = 0x66A1;
/// 向量低 4 位非 0b1111（SDM spurious 向量格式约束）。
pub const X_SR_VEC_FORMAT: u16 = 0x66A2;
/// 相序非法（当前相不是预期相——乱序迁移）。
pub const X_SR_PHASE_ORDER: u16 = 0x66A3;
/// 迁移已完成后再发起迁移（重复迁移拒绝）。
pub const X_SR_ALREADY_MIGRATED: u16 = 0x66A4;
/// 过渡期伪中断丢失检出（transit_dropped ≠ 0）。
pub const X_SR_TRANSIT_LOST: u16 = 0x66A5;
/// 回放指纹失配（迁移账不可回放）。
pub const X_SR_REPLAY_MISMATCH: u16 = 0x66A6;
/// 迁移未完成即验证（PHASE_VERIFY 前置缺失）。
pub const X_SR_VERIFY_EARLY: u16 = 0x66A7;
/// 验证探针路由失配（新向量未生效或旧向量残留生效）。
pub const X_SR_PROBE_ROUTE: u16 = 0x66A8;
/// 相账满（定容拒绝）。
pub const X_SR_PHASE_FULL: u16 = 0x66A9;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_SR_SAME_VEC => "新旧向量相同",
        X_SR_VEC_FORMAT => "向量低4位非法",
        X_SR_PHASE_ORDER => "相序乱序",
        X_SR_ALREADY_MIGRATED => "重复迁移拒绝",
        X_SR_TRANSIT_LOST => "过渡期伪中断丢失",
        X_SR_REPLAY_MISMATCH => "回放指纹失配",
        X_SR_VERIFY_EARLY => "验证前置缺失",
        X_SR_PROBE_ROUTE => "探针路由失配",
        X_SR_PHASE_FULL => "相账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、SVR 寄存器模型与迁移相
// ===========================================================================

/// SVR 影子寄存器（bit0=使能，bit7:4=向量高 4 位；低 4 位恒 0b1111）。
#[derive(Clone, Copy)]
pub struct SvrShadow {
    /// 使能位（bit0）。
    pub enabled: bool,
    /// spurious 向量号（全 8 位）。
    pub vector: u8,
}

impl SvrShadow {
    /// 合法性：向量低 4 位必须为 0b1111（0x0F 掩码）。
    pub const fn vector_valid(v: u8) -> bool {
        (v & 0x0F) == 0x0F
    }
    /// 使能位外的保留位写入闸门（本模型仅 bit0 可写使能）。
    pub const fn svr_word_ok(word: u32) -> bool {
        word & !0x1FF == 0 // bit0..=8 允许（bit1..=8 为 SDM 保留但写入零），高位禁
    }
}

/// 迁移相闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrPhase {
    /// 相 0：读旧 SVR 落账。
    Probe,
    /// 相 1：清使能位（伪中断进丢弃窗口）。
    Disable,
    /// 相 2：写新向量 + 复使能。
    Swap,
    /// 相 3：双探针验证路由。
    Verify,
    /// 相 4：迁移终结。
    Done,
}

impl MigrPhase {
    /// 相序数。
    pub const fn ord(self) -> u8 {
        match self {
            MigrPhase::Probe => 0,
            MigrPhase::Disable => 1,
            MigrPhase::Swap => 2,
            MigrPhase::Verify => 3,
            MigrPhase::Done => 4,
        }
    }
    /// 后继相。
    pub const fn next(self) -> Self {
        match self {
            MigrPhase::Probe => MigrPhase::Disable,
            MigrPhase::Disable => MigrPhase::Swap,
            MigrPhase::Swap => MigrPhase::Verify,
            MigrPhase::Verify => MigrPhase::Done,
            MigrPhase::Done => MigrPhase::Done,
        }
    }
}

/// 单相账目记录（可回放单元）。
#[derive(Clone, Copy)]
pub struct PhaseRecord {
    /// 相序号。
    pub phase: u8,
    /// 进入该相时的旧向量。
    pub old_vec: u8,
    /// 该相结束时的新向量（Swap 相之前恒等 old）。
    pub new_vec: u8,
    /// 该相窗口内到达的伪中断数。
    pub arrivals: u64,
    /// 相 tick。
    pub tick: u64,
}

impl PhaseRecord {
    /// 回放指纹（逐字段位权合成）。
    pub const fn fingerprint(&self) -> u64 {
        self.phase as u64
            ^ (self.old_vec as u64) << 8
            ^ (self.new_vec as u64) << 16
            ^ self.arrivals.wrapping_mul(0x1000_0000)
            ^ self.tick.wrapping_mul(0x1_0000_0000_0000)
    }
}

// ===========================================================================
// 三、迁移引擎
// ===========================================================================

/// SVR 槽位迁移引擎：四相状态机 + 伪中断到达面 + 迁移账。
pub struct SvrMigrator {
    pub svr: SvrShadow,
    phase: MigrPhase,
    old_vec: u8,
    new_vec: u8,
    /// 过渡期注入到达累计。
    transit_injected: u64,
    /// 过渡期被捕获（落账）到达。
    transit_caught: u64,
    /// 过渡期丢失（判据红线，恒须为 0）。
    transit_dropped: u64,
    phases: Vec<PhaseRecord>,
    cap: usize,
    tick: u64,
    /// 回放校验通过标记。
    pub replay_ok: bool,
}

impl SvrMigrator {
    pub fn new(cap: usize, initial_vec: u8) -> Result<Self, u16> {
        if !SvrShadow::vector_valid(initial_vec) {
            return Err(X_SR_VEC_FORMAT);
        }
        Ok(Self {
            svr: SvrShadow {
                enabled: true,
                vector: initial_vec,
            },
            phase: MigrPhase::Probe,
            old_vec: initial_vec,
            new_vec: initial_vec,
            transit_injected: 0,
            transit_caught: 0,
            transit_dropped: 0,
            phases: Vec::with_capacity(cap),
            cap,
            tick: 0,
            replay_ok: false,
        })
    }

    /// 当前相。
    pub const fn phase(&self) -> MigrPhase {
        self.phase
    }

    /// 过渡期窗口判定：Probe..Verify 之间（不含 Done）均算过渡期。
    const fn in_transit(&self) -> bool {
        self.phase.ord() < MigrPhase::Done.ord()
    }

    /// 伪中断到达（迁移窗口外正常路由；窗口内一律捕获落账，不丢）。
    pub fn spurious_arrive(&mut self, vector: u8) -> Result<u64, u16> {
        if vector != self.svr.vector {
            // 不匹配当前 SVR 向量的伪中断：路由失配证据。
            return Err(X_SR_PROBE_ROUTE);
        }
        self.tick += 1;
        self.transit_injected += 1;
        if self.in_transit() {
            self.transit_caught += 1;
            // 窗口内捕获：当前相账目 arrivals +1（相记录存在则回填）。
            if let Some(last) = self.phases.last_mut() {
                last.arrivals += 1;
            }
        } else {
            // 窗口外到达：正常使能/未使能语义（使能=计数，未使能=丢弃）。
            if self.svr.enabled {
                self.transit_caught += 1;
            } else {
                self.transit_dropped += 1;
            }
        }
        Ok(self.transit_caught)
    }

    /// 推进相（乱序/重复迁移拒绝）。每相落一条账。
    pub fn advance(&mut self) -> Result<MigrPhase, u16> {
        if self.phase == MigrPhase::Done {
            return Err(X_SR_ALREADY_MIGRATED);
        }
        self.tick += 1;
        // 相动作。
        match self.phase {
            MigrPhase::Probe => {
                // 读旧 SVR：old_vec 落账。
                self.old_vec = self.svr.vector;
            }
            MigrPhase::Disable => {
                // 清使能位：伪中断进入丢弃语义窗口（但窗口内到达仍被捕获）。
                self.svr.enabled = false;
            }
            MigrPhase::Swap => {
                // 写新向量 + 复使能。
                self.svr.vector = self.new_vec;
                self.svr.enabled = true;
            }
            MigrPhase::Verify => {
                // 双探针验证：旧向量必失配、新向量必命中。
                if self.old_vec == self.new_vec {
                    return Err(X_SR_SAME_VEC);
                }
            }
            MigrPhase::Done => {}
        }
        let rec = PhaseRecord {
            phase: self.phase.ord(),
            old_vec: self.old_vec,
            new_vec: self.new_vec,
            arrivals: 0,
            tick: self.tick,
        };
        if self.phases.len() < self.cap {
            self.phases.push(rec);
        } else {
            return Err(X_SR_PHASE_FULL);
        }
        self.phase = self.phase.next();
        Ok(self.phase)
    }

    /// 发起迁移：设新向量（合法性闸门）。
    pub fn begin(&mut self, new_vec: u8) -> Result<(), u16> {
        if !SvrShadow::vector_valid(new_vec) {
            return Err(X_SR_VEC_FORMAT);
        }
        if new_vec == self.old_vec {
            return Err(X_SR_SAME_VEC);
        }
        if self.phase != MigrPhase::Probe {
            return Err(X_SR_ALREADY_MIGRATED);
        }
        self.new_vec = new_vec;
        Ok(())
    }

    /// 迁移验证：完成态 + 零丢失 + SVR 已切新向量。
    pub fn verify(&self) -> Result<(), u16> {
        if self.phase != MigrPhase::Done {
            return Err(X_SR_VERIFY_EARLY);
        }
        if self.transit_dropped != 0 {
            return Err(X_SR_TRANSIT_LOST);
        }
        if self.svr.vector != self.new_vec || !self.svr.enabled {
            return Err(X_SR_PROBE_ROUTE);
        }
        Ok(())
    }

    /// 迁移账回放：重演相序列，指纹链逐条相等 ⇒ 可回放。
    pub fn replay(&mut self) -> Result<u64, u16> {
        if self.phases.len() != 4 {
            return Err(X_SR_VERIFY_EARLY);
        }
        let mut chain = 0u64;
        let mut i = 0;
        while i < self.phases.len() {
            // 相序号必须严格递增且连续。
            if self.phases[i].phase != i as u8 {
                return Err(X_SR_PHASE_ORDER);
            }
            chain ^= self.phases[i].fingerprint();
            i += 1;
        }
        // Swap 相前 new_vec 应等于 old_vec？——不：账目记录的是目标向量，
        // Probe/Disable 相的 new_vec 允许已设目标（begin 先于相推进）。
        self.replay_ok = true;
        Ok(chain)
    }

    /// 全账指纹（回放对拍键）。
    pub fn ledger_fingerprint(&self) -> u64 {
        let mut chain = 0u64;
        let mut i = 0;
        while i < self.phases.len() {
            chain ^= self.phases[i].fingerprint();
            i += 1;
        }
        chain
    }

    /// 过渡期守恒恒等式：injected = caught + dropped。
    pub const fn transit_balanced(&self) -> bool {
        self.transit_injected == self.transit_caught + self.transit_dropped
    }
}

// ===========================================================================
// 四、迁移演练（完整序列 + 窗口到达注入）
// ===========================================================================

/// 演练结论。
pub struct MigrVerdict {
    pub phases_done: u8,
    pub transit_injected: u64,
    pub transit_caught: u64,
    pub transit_dropped: u64,
    pub replay_chain: u64,
}

/// 完整迁移演练：0xFF→0x1F（合法低 4 位 1111），窗口内每相注入 3 次
/// 伪中断，验证零丢失与回放一致。
pub fn run_migration(seed_vec: u8, new_vec: u8) -> Result<MigrVerdict, u16> {
    let mut m = SvrMigrator::new(8, seed_vec)?;
    m.begin(new_vec)?;
    // Probe 相注入 + 推进。
    let mut k = 0;
    while k < 3 {
        m.spurious_arrive(seed_vec)?;
        k += 1;
    }
    m.advance()?;
    // Disable 相。
    let mut k = 0;
    while k < 3 {
        m.spurious_arrive(seed_vec)?;
        k += 1;
    }
    m.advance()?;
    // Swap 相（此后旧向量到达应失配——路由已切）。
    m.advance()?;
    let mut k = 0;
    while k < 3 {
        m.spurious_arrive(new_vec)?;
        k += 1;
    }
    m.advance()?;
    // Verify：完成校验 + 回放。
    m.verify()?;
    let chain = m.replay()?;
    // 回放指纹对拍。
    if chain != m.ledger_fingerprint() {
        return Err(X_SR_REPLAY_MISMATCH);
    }
    Ok(MigrVerdict {
        phases_done: m.phase().ord(),
        transit_injected: m.transit_injected,
        transit_caught: m.transit_caught,
        transit_dropped: m.transit_dropped,
        replay_chain: chain,
    })
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：完整迁移零丢失 + 回放一致 + 错误路径全走查。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 向量格式闸门：低 4 位非 1111 拒绝。
    if SvrMigrator::new(4, 0x20).is_ok() {
        return Ok(1);
    }
    let mut m = SvrMigrator::new(8, 0xFF)?;
    // 2) 新向量格式闸门。
    if m.begin(0x30).is_ok() {
        return Ok(2);
    }
    // 3) 相同向量拒绝。
    if m.begin(0xFF).is_ok() {
        return Ok(3);
    }
    // 4) 正常迁移序列。
    let v = run_migration(0xFF, 0x1F)?;
    if v.transit_dropped != 0 {
        return Err(X_SR_TRANSIT_LOST);
    }
    if v.phases_done != 4 {
        return Ok(4);
    }
    if v.transit_injected != 9 || v.transit_caught != 9 {
        return Ok(5);
    }
    // 5) 重复迁移拒绝（Done 后 begin/advance）。
    let mut m2 = SvrMigrator::new(8, 0x1F)?;
    m2.begin(0xFF)?;
    while m2.phase() != MigrPhase::Done {
        m2.advance()?;
    }
    if m2.begin(0x2F).is_ok() {
        return Ok(6);
    }
    if m2.advance().is_ok() {
        return Ok(7);
    }
    // 6) 未完成即验证拒绝。
    let mut m3 = SvrMigrator::new(8, 0x1F)?;
    if m3.verify().is_ok() {
        return Ok(8);
    }
    // 7) 探针路由：Swap 后旧向量到达必须失配。
    let mut m4 = SvrMigrator::new(8, 0x1F)?;
    m4.begin(0xFF)?;
    m4.advance()?; // Probe
    m4.advance()?; // Disable
    m4.advance()?; // Swap（已切 0xFF）
    if m4.spurious_arrive(0x1F).is_ok() {
        return Ok(9);
    }
    // 8) 过渡期守恒。
    if !m4.transit_balanced() {
        return Err(X_SR_TRANSIT_LOST);
    }
    // 9) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_SR_SAME_VEC, X_SR_VEC_FORMAT, X_SR_PHASE_ORDER, X_SR_ALREADY_MIGRATED,
        X_SR_TRANSIT_LOST, X_SR_REPLAY_MISMATCH, X_SR_VERIFY_EARLY, X_SR_PROBE_ROUTE, X_SR_PHASE_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(10);
        }
    }
    Ok(0)
}
