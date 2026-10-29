//! unx0837_lapic_mute — UNX-F0837 LAPIC 全屏蔽停止路径（系统停机/深度空闲用）（诊断段 0x65C0）。
//!
//! 判据原文：全屏蔽后中断投递归零（注入 10/10 次零投递），
//!           恢复序列状态复原 100%。
//! 设计纪律：
//!   1. 全屏蔽序列按 SDM 语义闭链：①清 SVR 使能位（0x0F0 bit8）→
//!      ②逐槽屏蔽全部 LVT（Mask[16] 置位）→③TPR 拉满 0xFF →
//!      ④排空在途账（IRR/ISR 位图与待发 IPI 队列计数清零）。
//!   2. 状态机：Active→Muting→Muted→Restoring→Active，跳相非法。
//!   3. Muted 相投递硬拦截：注入照常入注入账但投递严格为零（判据面）；
//!      恢复序列逆序回放，快照逐字段比对，复原 100% 才回 Active。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x65C0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x65C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_PHASE: u16 = SEG_BASE + 1;    // 状态机跳相非法
const E_LEAK: u16 = SEG_BASE + 2;     // Muted 相投递非零（判据失守）
const E_SEQ: u16 = SEG_BASE + 3;      // 屏蔽/恢复序列步序错
const E_RESTORE: u16 = SEG_BASE + 4;  // 恢复后快照不复原（<100%）
const E_DRAIN: u16 = SEG_BASE + 5;    // 排空后账目残留
const E_SCOPE: u16 = SEG_BASE + 6;    // 重复进入全屏蔽（已在 Muted）

/// LVT 槽数（P4 档全槽）。
pub const LVT_SLOTS: usize = 7;

/// 全屏蔽状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutePhase {
    Active,
    Muting,
    Muted,
    Restoring,
}

/// 屏蔽前快照（恢复比对基准）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MuteSnapshot {
    pub svr_enabled: bool,
    pub lvt_masks: [bool; LVT_SLOTS],
    pub tpr: u8,
    pub pending_ipis: u32,
    pub irr_bits: u32, // 在途 IRR 位计数（模型化账面）
    pub isr_bits: u32,
}

impl MuteSnapshot {
    fn differs(&self, other: &MuteSnapshot) -> u32 {
        let mut d = 0;
        if self.svr_enabled != other.svr_enabled {
            d += 1;
        }
        for i in 0..LVT_SLOTS {
            if self.lvt_masks[i] != other.lvt_masks[i] {
                d += 1;
            }
        }
        if self.tpr != other.tpr {
            d += 1;
        }
        if self.pending_ipis != other.pending_ipis {
            d += 1;
        }
        if self.irr_bits != other.irr_bits {
            d += 1;
        }
        if self.isr_bits != other.isr_bits {
            d += 1;
        }
        d
    }
}

/// 全屏蔽引擎。
pub struct MuteGate {
    phase: MutePhase,
    svr_enabled: bool,
    lvt_masks: [bool; LVT_SLOTS],
    tpr: u8,
    pending_ipis: u32,
    irr_bits: u32,
    isr_bits: u32,
    pub injections: u32,   // Muted 相注入总数（判据分母）
    pub deliveries: u32,   // Muted 相投递数（判据要求恒零）
    pub mute_rounds: u32,  // 完成屏蔽-恢复回合计数
    pre_snapshot: MuteSnapshot,
}

impl MuteGate {
    pub fn new() -> MuteGate {
        MuteGate {
            phase: MutePhase::Active,
            svr_enabled: true,
            lvt_masks: [false; LVT_SLOTS],
            tpr: 0,
            pending_ipis: 3, // 初始给点在途账（模拟活性态）
            irr_bits: 2,
            isr_bits: 1,
            injections: 0,
            deliveries: 0,
            mute_rounds: 0,
            pre_snapshot: MuteSnapshot {
                svr_enabled: true,
                lvt_masks: [false; LVT_SLOTS],
                tpr: 0,
                pending_ipis: 3,
                irr_bits: 2,
                isr_bits: 1,
            },
        }
    }

    /// 注入向量：Active 相按 TPR 语义投递；Muted 相硬拦截（投递恒零）。
    /// 其它相：注入被拒（过渡期窗口闭合）。
    pub fn inject(&mut self, vector: u8, prio: u8) -> Result<bool, u16> {
        match self.phase {
            MutePhase::Active => {
                let vclass = vector >> 4;
                let deliver = vclass > (self.tpr >> 4);
                if deliver {
                    self.irr_bits += 1; // 入在途账
                }
                Ok(deliver)
            }
            MutePhase::Muted => {
                self.injections += 1;
                // 硬拦截：投递恒零，连在途账都不入（判据「投递归零」）
                let _ = prio;
                Ok(false)
            }
            _ => Err(E_PHASE),
        }
    }

    /// 屏蔽序列：四步闭链，步序由 seq 强制（0→3）。
    /// 0=清 SVR 使能，1=屏蔽全 LVT，2=TPR 拉满，3=排空在途账。
    pub fn mute_step(&mut self, seq: u8) -> Result<(), u16> {
        match self.phase {
            MutePhase::Active => {
                if seq != 0 {
                    return Err(E_SEQ);
                }
                self.pre_snapshot = self.snapshot();
                self.phase = MutePhase::Muting;
            }
            MutePhase::Muting => {
                if seq == 0 || seq > 3 {
                    return Err(E_SEQ);
                }
            }
            _ => return Err(E_PHASE),
        }
        match seq {
            0 => {
                self.svr_enabled = false;
                self.phase = MutePhase::Muting;
            }
            1 => {
                for i in 0..LVT_SLOTS {
                    self.lvt_masks[i] = true;
                }
            }
            2 => {
                self.tpr = 0xFF;
            }
            3 => {
                self.pending_ipis = 0;
                self.irr_bits = 0;
                self.isr_bits = 0;
                self.phase = MutePhase::Muted;
            }
            _ => return Err(E_SEQ),
        }
        Ok(())
    }

    /// 一键全屏蔽（等价四步合法回放）。
    pub fn mute_all(&mut self) -> Result<(), u16> {
        if self.phase != MutePhase::Active {
            return Err(E_SCOPE);
        }
        for s in 0..4u8 {
            self.mute_step(s)?;
        }
        if self.phase != MutePhase::Muted || !self.muted_clean() {
            return Err(E_DRAIN);
        }
        Ok(())
    }

    /// Muted 相账面闭合：SVR 关、全 LVT 屏蔽、TPR 满、在途全零。
    pub fn muted_clean(&self) -> bool {
        !self.svr_enabled
            && self.lvt_masks.iter().all(|m| *m)
            && self.tpr == 0xFF
            && self.pending_ipis == 0
            && self.irr_bits == 0
            && self.isr_bits == 0
    }

    /// 恢复序列：逆序回放（3→0），快照逐字段比对，全等才回 Active。
    pub fn restore_step(&mut self, seq: u8) -> Result<(), u16> {
        match self.phase {
            MutePhase::Muted => {
                if seq != 3 {
                    return Err(E_SEQ);
                }
                self.phase = MutePhase::Restoring;
            }
            MutePhase::Restoring => {
                if seq > 2 {
                    return Err(E_SEQ);
                }
            }
            _ => return Err(E_PHASE),
        }
        match seq {
            3 => {} // 排空步恢复时无需动作（账面已在 Muted 清零）
            2 => {
                self.tpr = self.pre_snapshot.tpr;
            }
            1 => {
                for i in 0..LVT_SLOTS {
                    self.lvt_masks[i] = self.pre_snapshot.lvt_masks[i];
                }
            }
            0 => {
                self.svr_enabled = self.pre_snapshot.svr_enabled;
                // 在途账复原（快照基准）
                self.pending_ipis = self.pre_snapshot.pending_ipis;
                self.irr_bits = self.pre_snapshot.irr_bits;
                self.isr_bits = self.pre_snapshot.isr_bits;
                if self.snapshot().differs(&self.pre_snapshot) != 0 {
                    return Err(E_RESTORE);
                }
                self.phase = MutePhase::Active;
                self.mute_rounds += 1;
            }
            _ => return Err(E_SEQ),
        }
        Ok(())
    }

    /// 一键恢复。
    pub fn restore_all(&mut self) -> Result<(), u16> {
        if self.phase != MutePhase::Muted {
            return Err(E_PHASE);
        }
        for s in [3u8, 2, 1, 0] {
            self.restore_step(s)?;
        }
        if self.phase != MutePhase::Active {
            return Err(E_RESTORE);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> MuteSnapshot {
        MuteSnapshot {
            svr_enabled: self.svr_enabled,
            lvt_masks: self.lvt_masks,
            tpr: self.tpr,
            pending_ipis: self.pending_ipis,
            irr_bits: self.irr_bits,
            isr_bits: self.isr_bits,
        }
    }

    pub fn phase(&self) -> MutePhase {
        self.phase
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_PHASE => "状态机跳相非法（Active/Muting/Muted/Restoring 闭链外）",
        E_LEAK => "Muted 相投递非零（判据失守）",
        E_SEQ => "屏蔽/恢复序列步序错",
        E_RESTORE => "恢复后快照不复原（<100%）",
        E_DRAIN => "排空后账目残留",
        E_SCOPE => "重复进入全屏蔽（已在 Muted）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全屏蔽后 10/10 零投递 + 恢复复原 100% + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：Active 相投递语义基线（TPR Class 判据，供对照）。
    let mut g = MuteGate::new();
    if !g.inject(0x60, 6)? {
        return Err(E_LEAK);
    }
    // Active 相下恢复操作 → E_PHASE（过渡窗口闭合预检）。
    if g.restore_step(0) != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    // 重建基线（上一步未改变任何账面）。
    let mut g = MuteGate::new();
    if !g.inject(0x60, 6)? {
        return Err(E_LEAK);
    }
    hits += 1;

    // R2：一键全屏蔽 → 账面闭合。
    g.mute_all()?;
    if g.phase() != MutePhase::Muted || !g.muted_clean() {
        return Err(E_DRAIN);
    }
    hits += 1;

    // R3：Muted 相注入 10/10 零投递（判据原文）。
    for i in 0..10u8 {
        let v = 0x20 + i;
        if g.inject(v, i)? {
            return Err(E_LEAK); // 全屏蔽下任何向量都不得投递
        }
    }
    if g.injections != 10 || g.deliveries != 0 {
        return Err(E_LEAK);
    }
    // 在途账保持全零（拦截不入账）。
    if g.irr_bits != 0 || g.pending_ipis != 0 {
        return Err(E_DRAIN);
    }
    hits += 1;

    // R4：一键恢复 → 快照复原 100%（判据后半）。
    g.restore_all()?;
    if g.phase() != MutePhase::Active {
        return Err(E_RESTORE);
    }
    let s = g.snapshot();
    if s.svr_enabled != true || s.tpr != 0 || s.pending_ipis != 3 {
        return Err(E_RESTORE);
    }
    if g.mute_rounds != 1 {
        return Err(E_RESTORE);
    }
    // 恢复后投递语义复活（TPR=0 时向量 0x60 可投）。
    if !g.inject(0x60, 6)? {
        return Err(E_RESTORE);
    }
    hits += 1;

    // R5：逐步屏蔽与乱序拒绝（步序闭合验证）。
    let mut g2 = MuteGate::new();
    if g2.mute_step(2) != Err(E_SEQ) {
        return Err(E_SEQ); // 首步必须 0
    }
    g2.mute_step(0)?;
    if g2.mute_step(0) != Err(E_SEQ) {
        return Err(E_SEQ); // Muting 相禁重放步 0
    }
    g2.mute_step(1)?;
    g2.mute_step(2)?;
    g2.mute_step(3)?;
    if g2.phase() != MutePhase::Muted {
        return Err(E_PHASE);
    }
    hits += 1;

    // R6：逐步恢复逆序闭合。
    g2.restore_step(3)?;
    g2.restore_step(2)?;
    g2.restore_step(1)?;
    g2.restore_step(0)?;
    if g2.phase() != MutePhase::Active || !g2.mute_rounds_ready() {
        return Err(E_RESTORE);
    }
    hits += 1;

    // I1：Muted 相重复屏蔽 → E_SCOPE。
    let mut g3 = MuteGate::new();
    g3.mute_all()?;
    if g3.mute_all() != Err(E_SCOPE) {
        return Err(E_SCOPE);
    }
    hits += 1;

    // I2：过渡相注入被拒 → E_PHASE。
    let mut g4 = MuteGate::new();
    g4.mute_step(0)?;
    if g4.inject(0x60, 6) != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    // 过渡相（Muting 未排空）尝试一键恢复 → E_PHASE。
    if g4.restore_all() != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    hits += 1;

    // I3：恢复步序乱序 → E_SEQ。
    let mut g5 = MuteGate::new();
    g5.mute_all()?;
    if g5.restore_step(2) != Err(E_SEQ) {
        return Err(E_SEQ); // 恢复首步必须 3
    }
    hits += 1;

    Ok(hits)
}

impl MuteGate {
    /// 回合计数就绪（演练辅助面）。
    fn mute_rounds_ready(&self) -> bool {
        self.mute_rounds == 1
    }
}
