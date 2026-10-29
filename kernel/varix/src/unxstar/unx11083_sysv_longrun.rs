//! UNX-F11083 SysV IPC 长稳段（shm/sem/msg 万轮）（0x1ea60 段）
//!
//! 每个 SysV 周期是一套完整生命周期：create → attach → 可选二次 attach →
//! detach → remove。对象表在周期结束时必须回到空，释放计数必须追上创建计数。
//! 万轮循环只累积标量，实体表最多容纳一个周期内的对象，不随轮数增长。
//! 不变式：`rounds == creates == releases == 10000`，三种 IPC 的创建/释放
//! 向量逐项相等，且残活对象为 0。
//!
//! 结构：IPC 实体（键/类/引用/代次）+ 资源向量（三类计数）+ SysV 长稳账
//! （周期状态机、生命周期守恒、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11083 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1ea60;

/// 空账 —— 尚未跑任何 IPC 周期。
pub const SV_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 键为零、周期数为零或超出批量上限。
pub const SV_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同键对象仍存活时再次创建。
pub const SV_DUP: u32 = SEG_BASE + 0x03;
/// 泄漏 —— 周期结束后仍有残活对象，或创建/释放不相等。
pub const SV_LEAK: u32 = SEG_BASE + 0x04;
/// 状态错 —— 引用计数不为零时 remove，或 detach 次数多于 attach。
pub const SV_STATE: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 长稳账冻结后仍创建对象。
pub const SV_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（SysV 长稳账）。
pub const SV_TAG: u64 = 0x1ea_83;

/// 万轮总周期。
pub const SV_ROUNDS: u32 = 10_000;

/// SysV 对象类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcKind {
    Shm,
    Sem,
    Msg,
}

impl IpcKind {
    pub fn code(self) -> u8 {
        match self {
            IpcKind::Shm => 0,
            IpcKind::Sem => 1,
            IpcKind::Msg => 2,
        }
    }
}

/// 一个 SysV 对象。
#[derive(Clone, Debug)]
pub struct IpcEntity {
    pub key: u32,
    pub kind: IpcKind,
    pub refs: u16,
    pub generation: u16,
    pub live: bool,
}

/// 三类对象的计数向量。
#[derive(Clone, Debug)]
pub struct ResourceVector {
    pub shm: u32,
    pub sem: u32,
    pub msg: u32,
}

impl ResourceVector {
    pub fn zero() -> ResourceVector {
        ResourceVector { shm: 0, sem: 0, msg: 0 }
    }

    pub fn add(&mut self, kind: IpcKind) -> u32 {
        match kind {
            IpcKind::Shm => {
                self.shm = self.shm.wrapping_add(1);
                self.shm
            }
            IpcKind::Sem => {
                self.sem = self.sem.wrapping_add(1);
                self.sem
            }
            IpcKind::Msg => {
                self.msg = self.msg.wrapping_add(1);
                self.msg
            }
        }
    }

    pub fn get(&self, kind: IpcKind) -> u32 {
        match kind {
            IpcKind::Shm => self.shm,
            IpcKind::Sem => self.sem,
            IpcKind::Msg => self.msg,
        }
    }

    pub fn total(&self) -> u32 {
        self.shm.wrapping_add(self.sem).wrapping_add(self.msg)
    }

    pub fn matches(&self, other: &ResourceVector) -> bool {
        self.shm == other.shm && self.sem == other.sem && self.msg == other.msg
    }
}

/// SysV 长稳总账。
#[derive(Clone, Debug)]
pub struct SysvSoak {
    pub entities: Vec<IpcEntity>,
    pub created_by: ResourceVector,
    pub released_by: ResourceVector,
    pub rounds: u32,
    pub creates: u32,
    pub releases: u32,
    pub ops: u32,
    pub generations: u32,
    pub residual: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SysvSoak {
    pub fn new() -> SysvSoak {
        SysvSoak {
            entities: Vec::new(),
            created_by: ResourceVector::zero(),
            released_by: ResourceVector::zero(),
            rounds: 0,
            creates: 0,
            releases: 0,
            ops: 0,
            generations: 0,
            residual: 0,
            frozen: false,
            root: None,
        }
    }

    fn entity_at(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entities.len() {
            if self.entities[i].key == key && self.entities[i].live {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 创建对象。
    pub fn register(&mut self, key: u32, kind: IpcKind) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        if key == 0 {
            return Err(SV_RANGE);
        }
        if self.entity_at(key).is_some() {
            return Err(SV_DUP);
        }
        if self.entities.len() >= 8 {
            return Err(SV_LEAK);
        }
        self.generations = self.generations.wrapping_add(1);
        self.creates = self.creates.wrapping_add(1);
        self.entities.push(IpcEntity {
            key,
            kind,
            refs: 0,
            generation: (self.generations % u16::MAX as u32) as u16,
            live: true,
        });
        self.created_by.add(kind);
        Ok(())
    }

    /// attach：引用计数加一。
    pub fn attach(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        let ei = match self.entity_at(key) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        if self.entities[ei].refs == u16::MAX {
            return Err(SV_RANGE);
        }
        self.entities[ei].refs = self.entities[ei].refs.wrapping_add(1);
        self.ops = self.ops.wrapping_add(1);
        Ok(())
    }

    /// detach：引用计数减一，不能降到零以下。
    pub fn detach(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        let ei = match self.entity_at(key) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        if self.entities[ei].refs == 0 {
            return Err(SV_STATE);
        }
        self.entities[ei].refs = self.entities[ei].refs.wrapping_sub(1);
        self.ops = self.ops.wrapping_add(1);
        Ok(())
    }

    /// remove：引用必须归零，随后从对象表弹出。
    pub fn remove(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        let ei = match self.entity_at(key) {
            Some(i) => i,
            None => return Err(SV_EMPTY),
        };
        if self.entities[ei].refs != 0 {
            return Err(SV_STATE);
        }
        let kind = self.entities[ei].kind;
        self.entities[ei].live = false;
        self.entities.remove(ei);
        self.releases = self.releases.wrapping_add(1);
        self.released_by.add(kind);
        Ok(())
    }

    /// 一个完整生命周期；多余 attach 与 detach 成对，绝不遗留引用。
    pub fn cycle(&mut self, key: u32, kind: IpcKind, twice: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        self.register(key, kind)?;
        self.attach(key)?;
        if twice {
            self.attach(key)?;
        }
        self.detach(key)?;
        if twice {
            self.detach(key)?;
        }
        self.remove(key)?;
        self.rounds = self.rounds.wrapping_add(1);
        self.residual = self.entities.len() as u32;
        Ok(())
    }

    /// 批量推进万轮；每轮从空对象表开始。
    pub fn run_rounds(&mut self, rounds: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(SV_FROZEN);
        }
        if rounds == 0 || rounds > 100_000 {
            return Err(SV_RANGE);
        }
        let mut r = 0u32;
        while r < rounds {
            let kind = match r % 3 {
                0 => IpcKind::Shm,
                1 => IpcKind::Sem,
                _ => IpcKind::Msg,
            };
            let key = 0x8300u32.wrapping_add(r % 251);
            self.cycle(key, kind, r % 7 == 0)?;
            r = r.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 生命周期守恒：创建 = 释放 = 轮数，三类向量逐项相等。
    pub fn lifecycle_conserved(&self) -> bool {
        self.rounds == SV_ROUNDS
            && self.creates == SV_ROUNDS
            && self.releases == SV_ROUNDS
            && self.creates == self.releases
            && self.created_by.matches(&self.released_by)
            && self.created_by.total() == SV_ROUNDS
    }

    /// 残活审计：对象表必须为空且 residual 归零。
    pub fn zero_residual(&self) -> bool {
        self.entities.is_empty() && self.residual == 0
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rounds == 0 {
            return Err(SV_EMPTY);
        }
        if !self.entities.is_empty() || self.residual != 0 {
            return Err(SV_LEAK);
        }
        if self.creates != self.releases || !self.created_by.matches(&self.released_by) {
            return Err(SV_LEAK);
        }
        Ok(())
    }

    /// FNV-1a：全局周期/创建/释放/操作/代次 + 资源向量 + 对象表。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SV_TAG;
        h = (h ^ self.rounds as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.creates as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.releases as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ops as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.generations as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.created_by.shm as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.created_by.sem as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.created_by.msg as u64).wrapping_mul(0x100000001b3);
        let mut i = 0usize;
        while i < self.entities.len() {
            let e = &self.entities[i];
            h = (h ^ e.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.kind.code() as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.refs as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.generation as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 冻结长稳账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 三类计数摘要。
    pub fn summary(&self) -> String {
        let label = "sysv".to_string();
        format!("{}|rounds={}|shm={}|sem={}|msg={}|ops={}", label, self.rounds,
            self.created_by.shm, self.created_by.sem, self.created_by.msg, self.ops)
    }
}

/// 四路坏样本：重号、空对象 attach、带引用 remove、冻结后创建。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = SysvSoak::new();
    let _ = a.register(0x41, IpcKind::Shm);
    got[0] = a.register(0x41, IpcKind::Sem).err() == Some(SV_DUP);

    let mut b = SysvSoak::new();
    got[1] = b.attach(0x42).err() == Some(SV_EMPTY);

    let mut c = SysvSoak::new();
    let _ = c.register(0x43, IpcKind::Msg);
    let _ = c.attach(0x43);
    got[2] = c.remove(0x43).err() == Some(SV_STATE);

    let mut d = SysvSoak::new();
    let _ = d.register(0x44, IpcKind::Sem);
    let _ = d.freeze();
    got[3] = d.register(0x45, IpcKind::Shm).err() == Some(SV_FROZEN);
    got
}

/// SysV 万轮长稳判据。
pub fn run_unx11083_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11083");

    let mut b = SysvSoak::new();
    let run = b.run_rounds(SV_ROUNDS);

    // J1 两半：万轮账守恒，生命周期万轮零漏。
    let j1a = b.creates == 10_000 && b.releases == 10_000 && b.rounds == 10_000
        && b.lifecycle_conserved();
    let j1b = b.zero_residual() && b.audit().is_ok();
    s.add("J1-SysV万轮守恒", j1a, "10000 轮创建/释放/周期三账相等");
    s.add("J1-生命周期零漏", j1b, "对象表清空且残活计数为零");

    // J2 三类向量逐项对平。
    let j2 = b.created_by.matches(&b.released_by)
        && b.created_by.total() == 10_000
        && b.created_by.shm > 0 && b.created_by.sem > 0 && b.created_by.msg > 0;
    s.add("J2-三类对平", j2, "shm/sem/msg 创建与释放向量逐项相等");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同键存活时再创建必拒");
    s.add("J3-空对象注入", inj[1], "未知键 attach 必拒");
    s.add("J3-引用注入", inj[2], "带引用 remove 必拒");
    s.add("J3-冻结注入", inj[3], "冻结后创建必拒");

    // J4 冻结：根稳定且写路径关闭。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.register(1, IpcKind::Shm).err() == Some(SV_FROZEN)
        && f.run_rounds(1).err() == Some(SV_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后创建与批量推进均拒");

    // J5 指纹敏感：改一次操作计数即改根。
    let mut c = b.clone();
    c.ops = c.ops.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单次操作计数变化即改变 SysV 根");

    // J6 空账/范围/引用下限。
    let mut e = SysvSoak::new();
    let empty = e.audit();
    let range = e.register(0, IpcKind::Shm);
    let _ = e.register(0x51, IpcKind::Shm);
    let under = e.detach(0x51);
    let j6 = empty.err() == Some(SV_EMPTY) && range.err() == Some(SV_RANGE)
        && under.err() == Some(SV_STATE);
    s.add("J6-空账引用", j6, "空账、零键、零引用 detach 均拒");

    // J7 对象唯一：剩余表为空且代次随创建递增。
    let j7 = b.entities.is_empty() && b.generations == 10_000 && b.ops > 20_000;
    s.add("J7-代次唯一", j7, "万次创建代次递增且对象表回空");

    // J8 负语义：留一个活对象时审计必须报漏。
    let mut leak = SysvSoak::new();
    let _ = leak.run_rounds(9_999);
    let _ = leak.register(0x77, IpcKind::Msg);
    leak.residual = leak.entities.len() as u32;
    let j8 = !leak.lifecycle_conserved() && !leak.zero_residual()
        && leak.audit().err() == Some(SV_LEAK);
    s.add("J8-残活非绿", j8, "九千九百九十九轮加残活对象判漏");

    // J9 动态生命周期。
    s.add_dyn("J9-生命周期动态", j1a,
        &format!("c={} r={} ops={} gen={}", b.creates, b.releases, b.ops, b.generations));

    // J10 引用峰值：二次 attach 周期确实产生过 refs=2。
    let mut probe = SysvSoak::new();
    let _ = probe.register(0x61, IpcKind::Shm);
    let _ = probe.attach(0x61);
    let _ = probe.attach(0x61);
    let peak = probe.entities[0].refs;
    let _ = probe.detach(0x61);
    let _ = probe.detach(0x61);
    let _ = probe.remove(0x61);
    let j10 = peak == 2 && probe.entities.is_empty();
    s.add("J10-引用峰值", j10, "双 attach 峰值为 2 且最终可移除");

    // J11 总审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("sysv|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11083_flow() {
        let mut b = SysvSoak::new();
        assert!(b.audit().err() == Some(SV_EMPTY));
        assert!(b.register(0x31, IpcKind::Shm).is_ok());
        assert!(b.register(0x31, IpcKind::Sem).err() == Some(SV_DUP));
        assert!(b.attach(0x31).is_ok());
        assert!(b.remove(0x31).err() == Some(SV_STATE));
        assert!(b.detach(0x31).is_ok());
        assert!(b.detach(0x31).err() == Some(SV_STATE));
        assert!(b.remove(0x31).is_ok());
        assert!(b.run_rounds(9_999).is_ok());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.register(1, IpcKind::Msg).err() == Some(SV_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11083_checks_green() {
        assert!(run_unx11083_checks().all_passed(), "UNX-F11083 判据未全绿");
    }
}
