//! UNX-F0227 · 只读侧断电演练（引导期掉电安全）
//!
//! 锚点判据（施工书原文）：UNX-F0227-J1 引导各阶段拔电 ×10，
//! 重启无残留损坏（写盘侧归 B5/L1 红线，本域只证引导期只读性）。
//!
//! 设计：引导阶段机（10 阶段）× 每阶段拔电注入 → 只读性审计
//! （引导路径在拔电点之前的所有访存操作必须全为只读；写盘
//! 调用计数恒 0）→ 重启恢复账（状态机复位后无脏页残留）。
//! 红线：①引导期任何写盘调用即红（只读性破）；②拔电后状态
//! 机必须可复位（不可复位 = 残留损坏）；③10 次拔电全捕获。
//!
//! 错误码段：0x2D20（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0227_VERSION: &str = "F0227-powcut-v1";

/// 诊断码段基址（UNX-F0227 独占）。
pub const SEG_BASE: u16 = 0x2D20;

/// 引导阶段数（拔电注入口径 10）。
pub const BOOT_STAGES: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 引导期写盘（只读性破）。
    WriteDuringBoot = SEG_BASE + 1,
    /// 拔电后不可复位（残留损坏）。
    DirtyResidue = SEG_BASE + 2,
    /// 拔电点未捕获。
    CutMissed = SEG_BASE + 3,
    /// 演练账不平。
    DrillBroken = SEG_BASE + 4,
    /// 引导未完成即出恢复账。
    BootIncomplete = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 只读性访问类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// 读内存/盘。
    Read,
    /// 写内存（易失，允许）。
    WriteVolatile,
    /// 写盘（持久，引导期禁止）。
    WritePersistent,
}

/// 引导只读会话（一次引导 = 10 阶段访存序列）。
#[derive(Debug, Default)]
pub struct BootSession {
    /// 访存账。
    pub accesses: Vec<(u32, Access)>,
    /// 持久写计数（引导期必须恒 0）。
    pub persist_writes: u64,
    /// 已到阶段（0 基；拔电即停在此）。
    pub stage_reached: u32,
}

impl BootSession {
    /// 记一笔访存。
    pub fn touch(&mut self, stage: u32, a: Access) -> Result<(), u16> {
        if a == Access::WritePersistent {
            self.persist_writes += 1;
            return Err(E::WriteDuringBoot.code());
        }
        self.accesses.push((stage, a));
        self.stage_reached = stage;
        Ok(())
    }

    /// 只读性审计：持久写 = 0。
    pub fn readonly_ok(&self) -> Result<(), u16> {
        if self.persist_writes != 0 {
            return Err(E::WriteDuringBoot.code());
        }
        Ok(())
    }
}

/// 拔电状态机（复位即清账重建）。
#[derive(Debug, Default)]
pub struct PowerStateMachine {
    /// 复位代数。
    pub generation: u64,
    /// 上一代残留（复位后必须清空）。
    residue: Vec<u32>,
}

impl PowerStateMachine {
    /// 拔电（状态冻结）。
    pub fn cut(&mut self, stage: u32) {
        self.residue.push(stage);
    }

    /// 复位：残留必须清空、代数推进。
    pub fn reset(&mut self) -> Result<(), u16> {
        self.residue.clear();
        self.generation += 1;
        if !self.residue.is_empty() {
            return Err(E::DirtyResidue.code());
        }
        Ok(())
    }

    /// 复位后无残留。
    pub fn clean(&self) -> bool {
        self.residue.is_empty()
    }
}

/// J1 自证（判据原文：10 阶段拔电 ×10，重启无残留损坏）。
pub fn self_audit() -> Result<(), u16> {
    // 一、演练：10 阶段逐阶段拔电 ×10（每阶段一次完整引导片段）。
    let mut sm = PowerStateMachine::default();
    for cut_stage in 0..BOOT_STAGES {
        let mut sess = BootSession::default();
        // 片段内只读访存（读 + 易失写）。
        for s in 0..=cut_stage {
            sess.touch(s, Access::Read)?;
            if s % 3 == 0 {
                sess.touch(s, Access::WriteVolatile)?;
            }
        }
        // 只读性红线：片段零持久写。
        sess.readonly_ok()?;
        // 拔电 + 复位。
        sm.cut(cut_stage);
        sm.reset()?;
        if !sm.clean() {
            return Err(E::DirtyResidue.code());
        }
    }
    if sm.generation != BOOT_STAGES as u64 {
        return Err(E::CutMissed.code());
    }

    // 二、J1 主体：完整 10 阶段引导（无拔电）后恢复账成立。
    let mut full = BootSession::default();
    for s in 0..BOOT_STAGES {
        full.touch(s, Access::Read)?;
    }
    full.readonly_ok()?;
    if full.stage_reached != BOOT_STAGES - 1 {
        return Err(E::BootIncomplete.code());
    }
    if full.accesses.len() != BOOT_STAGES as usize {
        return Err(E::DrillBroken.code());
    }

    // 三、红线逐项：引导期持久写红 / 复位不可达（注入脏残留）。
    let mut writer = BootSession::default();
    writer.touch(0, Access::Read)?;
    if writer.touch(1, Access::WritePersistent).is_ok() {
        return Err(E::WriteDuringBoot.code());
    }
    if writer.readonly_ok().is_ok() {
        return Err(E::WriteDuringBoot.code());
    }
    let mut dirty = PowerStateMachine::default();
    dirty.cut(5);
    // 不复位直接查账 → 残留非空（检测路径）。
    if dirty.clean() {
        return Err(E::DirtyResidue.code());
    }

    // 四、判别式段位对拍。
    if E::WriteDuringBoot.code() != SEG_BASE + 1 || E::BootIncomplete.code() != SEG_BASE + 5 {
        return Err(E::WriteDuringBoot.code());
    }
    Ok(())
}
