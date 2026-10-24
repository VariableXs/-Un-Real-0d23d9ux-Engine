//! UNX-F0231 · 镜像降级回滚（上一版回退）
//!
//! 锚点判据（施工书原文）：UNX-F0231-J1 新版引导失败自动回滚
//! 上一版槽位（A/B 联动 F0134），回滚记录落账。
//!
//! 设计：A/B 双槽状态机（active / standby + 各槽健康态）→
//! 引导失败注入 → 自动回滚（active → standby 切换 + 回滚记录
//! 三要素：从槽/到槽/失败因）→ 红线：①无失败不回滚（防误切）；
//! ②standby 不健康时回滚必拒（切过去也起不来）；③回滚后 active
//! 必须是上一版槽（版本号必须更旧）。
//!
//! 错误码段：0x2D70（全仓普查后独占分配，AI-WBX1022）。

use alloc::boxed::Box;
use alloc::vec::Vec;

/// 模块版本。
pub const F0231_VERSION: &str = "F0231-abrollback-v1";

/// 诊断码段基址（UNX-F0231 独占）。
pub const SEG_BASE: u16 = 0x2D70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 无失败回滚（误切）。
    RollbackNoFail = SEG_BASE + 1,
    /// standby 不健康仍回滚。
    RollbackToDead = SEG_BASE + 2,
    /// 回滚后版本不旧。
    VersionNoOlder = SEG_BASE + 3,
    /// 回滚记录缺要素。
    RecIncomplete = SEG_BASE + 4,
    /// 槽位账破。
    SlotLedgerBroken = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 槽位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// A 槽。
    A,
    /// B 槽。
    B,
}

impl Slot {
    /// 对偶槽。
    pub const fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

/// 槽位状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotState {
    /// 镜像版本号（版本越大越新）。
    pub version: u32,
    /// 健康（引导可成功）。
    pub healthy: bool,
}

/// A/B 回滚机。
#[derive(Debug)]
pub struct AbRollback {
    /// active 槽。
    pub active: Slot,
    /// 槽状态。
    pub slots: [(Slot, SlotState); 2],
    /// 回滚记录（从槽, 到槽, 失败因）。
    pub records: Vec<(Slot, Slot, &'static str)>,
}

impl AbRollback {
    /// 建立：A active、B standby（版本自定）。
    pub fn boot(a_ver: u32, b_ver: u32, a_healthy: bool, b_healthy: bool) -> Self {
        AbRollback {
            active: Slot::A,
            slots: [
                (Slot::A, SlotState {
                    version: a_ver,
                    healthy: a_healthy,
                }),
                (Slot::B, SlotState {
                    version: b_ver,
                    healthy: b_healthy,
                }),
            ],
            records: Vec::new(),
        }
    }

    fn state_of(&self, s: Slot) -> &SlotState {
        match self.slots.iter().find(|(k, _)| *k == s) {
            Some((_, st)) => st,
            None => unreachable_slot(),
        }
    }

    /// 引导 active 槽（健康 → 成功；不健康 → 失败自动回滚）。
    pub fn boot_attempt(&mut self, fail_reason: Option<&'static str>) -> Result<Slot, u16> {
        let cur = self.active;
        if fail_reason.is_none() && self.state_of(cur).healthy {
            return Ok(cur); // 引导成功，不切。
        }
        // 失败 → 自动回滚到对偶槽。
        self.rollback(fail_reason.unwrap_or("unhealthy"))?;
        Ok(self.active)
    }

    /// 回滚：切到对偶槽（必须健康且版本更旧）+ 落账。
    pub fn rollback(&mut self, reason: &'static str) -> Result<(), u16> {
        let from = self.active;
        let to = from.other();
        let st = self.state_of(to);
        if !st.healthy {
            return Err(E::RollbackToDead.code());
        }
        if st.version >= self.state_of(from).version {
            return Err(E::VersionNoOlder.code());
        }
        self.active = to;
        self.records.push((from, to, reason));
        Ok(())
    }

    /// 记录三要素齐。
    pub fn records_complete(&self) -> Result<(), u16> {
        for (f, t, r) in self.records.iter() {
            if f == t || r.is_empty() {
                return Err(E::RecIncomplete.code());
            }
        }
        Ok(())
    }
}

/// 不可达占位（slots 恒含两槽；走不到此路径）。
#[cold]
#[inline(never)]
fn unreachable_slot() -> &'static SlotState {
    // 安全：boot() 恒建两槽，find 必中；此路径不可达。
    // 返回一个 leaked 常量态（零 panic 面）。
    Box::leak(Box::new(SlotState {
        version: 0,
        healthy: false,
    }))
}

/// J1 自证（判据原文：新版失败自动回滚上一版槽位，记录落账）。
pub fn self_audit() -> Result<(), u16> {
    // 一、J1 主体：A(2)失败 → 自动回滚 B(1)，记录三要素齐。
    let mut ab = AbRollback::boot(2, 1, false, true);
    let booted = ab.boot_attempt(Some("img-bad-sign"))?;
    if booted != Slot::B || ab.active != Slot::B {
        return Err(E::SlotLedgerBroken.code());
    }
    if ab.records.len() != 1 {
        return Err(E::RecIncomplete.code());
    }
    ab.records_complete()?;
    if ab.records[0].0 != Slot::A || ab.records[0].1 != Slot::B {
        return Err(E::RecIncomplete.code());
    }

    // 二、回滚后 active 版本必须更旧。
    if ab.state_of(Slot::B).version >= 2 {
        return Err(E::VersionNoOlder.code());
    }

    // 三、无失败不回滚：健康 A 引导成功不切。
    let mut ok = AbRollback::boot(2, 1, true, true);
    let stayed = ok.boot_attempt(None)?;
    if stayed != Slot::A || !ok.records.is_empty() {
        return Err(E::RollbackNoFail.code());
    }

    // 四、红线逐项：standby 死槽拒回滚 / 同版本拒回滚。
    let mut dead = AbRollback::boot(2, 1, false, false);
    if dead.boot_attempt(Some("img-trunc")).is_ok() {
        return Err(E::RollbackToDead.code());
    }
    if dead.rollback("manual") != Err(E::RollbackToDead.code()) {
        return Err(E::RollbackToDead.code());
    }
    let mut same = AbRollback::boot(2, 2, false, true);
    if same.rollback("manual").is_ok() {
        return Err(E::VersionNoOlder.code());
    }

    // 五、判别式段位对拍。
    if E::RollbackNoFail.code() != SEG_BASE + 1 || E::SlotLedgerBroken.code() != SEG_BASE + 5 {
        return Err(E::RollbackNoFail.code());
    }
    Ok(())
}
