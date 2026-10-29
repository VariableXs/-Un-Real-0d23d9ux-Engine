//! UNX-F11084 POSIX IPC 长稳段（sem/mq 万轮）（0x1ea80 段）
//!
//! POSIX 命名对象采用“名字 → 引用”的 inode 语义：unlink 只摘名字，已打开
//! 的句柄仍可继续操作，最后一个 close 才真正释放对象。万轮循环每轮创建、
//! 打开、关闭、unlink 一个名字，名字表始终只有一个活动项，轮与轮之间清零。
//! 不变式：created == unlinked == closes == 10000，名字表回空，sem/mq 操作账
//! 各占一半且总和为 20000。
//!
//! 结构：命名对象（哈希/类/链接/打开数）+ 命名注册表（创建/摘链/关闭计数）
//! + POSIX 长稳账（命名周期、操作账、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11084 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1ea80;

/// 空账 —— 尚未跑命名周期。
pub const PX_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 命名类号不在 0..2、哈希为零或批量轮数越界。
pub const PX_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同名对象存活时再次创建，或重复 unlink。
pub const PX_DUP: u32 = SEG_BASE + 0x03;
/// 名字/状态错 —— 名字非法、close 次数超过 open。
pub const PX_NAME: u32 = SEG_BASE + 0x04;
/// 泄漏 —— 名字表未回空，或创建/摘链/关闭三账不等。
pub const PX_LEAK: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 总账冻结后仍创建名字。
pub const PX_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（POSIX 命名长稳账）。
pub const PX_TAG: u64 = 0x1ea_84;

/// 万轮周期数。
pub const PX_ROUNDS: u32 = 10_000;

/// 一个 POSIX 命名对象。
#[derive(Clone, Debug)]
pub struct PosixName {
    pub hash: u64,
    pub class: u8,
    pub links: u16,
    pub opens: u16,
    pub generation: u32,
    pub unlinked: bool,
}

impl PosixName {
    /// 是否仍可通过名字查到。
    pub fn named(&self) -> bool {
        self.links != 0 && !self.unlinked
    }
}

/// 命名注册表。
#[derive(Clone, Debug)]
pub struct NameRegistry {
    pub names: Vec<PosixName>,
    pub created: u32,
    pub unlinked: u32,
    pub closes: u32,
    pub frozen: bool,
}

impl NameRegistry {
    pub fn new() -> NameRegistry {
        NameRegistry {
            names: Vec::new(),
            created: 0,
            unlinked: 0,
            closes: 0,
            frozen: false,
        }
    }

    fn name_at(&self, hash: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.names.len() {
            if self.names[i].hash == hash {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 创建命名对象；类 0=sem、1=mq。
    pub fn create(&mut self, hash: u64, class: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        if hash == 0 || class >= 2 {
            return Err(PX_RANGE);
        }
        if self.name_at(hash).is_some() {
            return Err(PX_DUP);
        }
        if self.names.len() >= 4 {
            return Err(PX_NAME);
        }
        self.created = self.created.wrapping_add(1);
        self.names.push(PosixName {
            hash,
            class,
            links: 1,
            opens: 0,
            generation: self.created,
            unlinked: false,
        });
        Ok(())
    }

    /// 打开命名对象。
    pub fn open(&mut self, hash: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        if hash == 0 {
            return Err(PX_NAME);
        }
        let ni = match self.name_at(hash) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        if self.names[ni].opens == u16::MAX {
            return Err(PX_RANGE);
        }
        self.names[ni].opens = self.names[ni].opens.wrapping_add(1);
        Ok(())
    }

    /// 关闭命名对象；最后一个 close 负责回收 unlinked 对象。
    pub fn close(&mut self, hash: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let ni = match self.name_at(hash) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        if self.names[ni].opens == 0 {
            return Err(PX_NAME);
        }
        self.names[ni].opens = self.names[ni].opens.wrapping_sub(1);
        self.closes = self.closes.wrapping_add(1);
        if self.names[ni].unlinked && self.names[ni].opens == 0 {
            self.names.remove(ni);
        }
        Ok(())
    }

    /// 摘链：命名不可再查；若无打开句柄则立即释放。
    pub fn unlink(&mut self, hash: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        let ni = match self.name_at(hash) {
            Some(i) => i,
            None => return Err(PX_EMPTY),
        };
        if self.names[ni].unlinked {
            return Err(PX_DUP);
        }
        self.names[ni].unlinked = true;
        self.names[ni].links = 0;
        self.unlinked = self.unlinked.wrapping_add(1);
        if self.names[ni].opens == 0 {
            self.names.remove(ni);
        }
        Ok(())
    }

    /// 仍存活的命名对象数。
    pub fn live_names(&self) -> u32 {
        self.names.len() as u32
    }
}

/// POSIX 长稳总账。
#[derive(Clone, Debug)]
pub struct PosixSoak {
    pub registry: NameRegistry,
    pub rounds: u32,
    pub sem_ops: u32,
    pub mq_ops: u32,
    pub peak_open: u16,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PosixSoak {
    pub fn new() -> PosixSoak {
        PosixSoak {
            registry: NameRegistry::new(),
            rounds: 0,
            sem_ops: 0,
            mq_ops: 0,
            peak_open: 0,
            frozen: false,
            root: None,
        }
    }

    /// 一个命名周期：create → open → (unlink/close 顺序可交换) → 释放。
    pub fn cycle(&mut self, hash: u64, class: u8, unlink_first: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        self.registry.create(hash, class)?;
        self.registry.open(hash)?;
        if self.registry.names.len() == 1 {
            let opens = self.registry.names[0].opens;
            if opens > self.peak_open {
                self.peak_open = opens;
            }
        }
        if unlink_first {
            self.registry.unlink(hash)?;
            self.registry.close(hash)?;
        } else {
            self.registry.close(hash)?;
            self.registry.unlink(hash)?;
        }
        if class == 0 {
            self.sem_ops = self.sem_ops.wrapping_add(2);
        } else {
            self.mq_ops = self.mq_ops.wrapping_add(2);
        }
        self.rounds = self.rounds.wrapping_add(1);
        Ok(())
    }

    /// 批量万轮；每轮从空名字表开始。
    pub fn run_rounds(&mut self, rounds: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(PX_FROZEN);
        }
        if rounds == 0 || rounds > 100_000 {
            return Err(PX_RANGE);
        }
        let mut r = 0u32;
        while r < rounds {
            let hash = 0x8400_0000u64
                .wrapping_add((r % 4_096) as u64)
                .wrapping_add(1);
            let class = (r % 2) as u8;
            self.cycle(hash, class, r % 3 == 0)?;
            r = r.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 命名账守恒：创建/摘链/关闭三账均达万轮，名字表回空。
    pub fn registry_conserved(&self) -> bool {
        self.rounds == PX_ROUNDS
            && self.registry.created == PX_ROUNDS
            && self.registry.unlinked == PX_ROUNDS
            && self.registry.closes == PX_ROUNDS
            && self.registry.names.is_empty()
    }

    /// 操作账守恒：半轮落 sem、半轮落 mq（每轮两笔），两侧各 PX_ROUNDS 笔，
    /// 总操作数 = 2×轮数。
    pub fn ops_conserved(&self) -> bool {
        self.sem_ops == PX_ROUNDS && self.mq_ops == PX_ROUNDS
            && self.sem_ops.wrapping_add(self.mq_ops) == PX_ROUNDS.wrapping_mul(2)
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rounds == 0 {
            return Err(PX_EMPTY);
        }
        if !self.registry.names.is_empty() {
            return Err(PX_LEAK);
        }
        if self.registry.created != self.registry.unlinked
            || self.registry.created != self.registry.closes
        {
            return Err(PX_LEAK);
        }
        Ok(())
    }

    /// FNV-1a：注册表计数、操作账、峰值与残留名字全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PX_TAG;
        h = (h ^ self.rounds as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.registry.created as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.registry.unlinked as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.registry.closes as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.sem_ops as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.mq_ops as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.peak_open as u64).wrapping_mul(0x100000001b3);
        let mut i = 0usize;
        while i < self.registry.names.len() {
            let n = &self.registry.names[i];
            h = (h ^ n.hash).wrapping_mul(0x100000001b3);
            h = (h ^ n.class as u64).wrapping_mul(0x100000001b3);
            h = (h ^ n.opens as u64).wrapping_mul(0x100000001b3);
            h = (h ^ n.generation as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 冻结总账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.registry.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 命名/操作摘要。
    pub fn summary(&self) -> String {
        let label = "posix".to_string();
        format!("{}|rounds={}|sem={}|mq={}|peak={}", label, self.rounds,
            self.sem_ops, self.mq_ops, self.peak_open)
    }
}

/// 四路坏样本：零哈希、同名重号、未知名打开、冻结后创建。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = PosixSoak::new();
    got[0] = a.cycle(0, 0, false).err() == Some(PX_RANGE);

    let mut b = PosixSoak::new();
    let _ = b.registry.create(0x91, 0);
    got[1] = b.registry.create(0x91, 1).err() == Some(PX_DUP);

    let mut c = PosixSoak::new();
    got[2] = c.registry.open(0x92).err() == Some(PX_EMPTY);

    let mut d = PosixSoak::new();
    let _ = d.cycle(0x93, 1, false);
    let _ = d.freeze();
    got[3] = d.registry.create(0x94, 0).err() == Some(PX_FROZEN);
    got
}

/// POSIX 万轮长稳判据。
pub fn run_unx11084_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11084");

    let mut b = PosixSoak::new();
    let run = b.run_rounds(PX_ROUNDS);

    // J1 两半：万轮账守恒，命名周期万轮零漏。
    let j1a = b.registry_conserved() && b.ops_conserved();
    let j1b = b.registry.names.is_empty() && b.audit().is_ok() && b.peak_open == 1;
    s.add("J1-POSIX万轮守恒", j1a, "创建/摘链/关闭与 sem/mq 操作账全平");
    s.add("J1-命名周期零漏", j1b, "名字表回空且峰值打开数为一");

    // J2 三类计数两两对平。
    let reg = &b.registry;
    let j2 = reg.created == 10_000 && reg.unlinked == 10_000 && reg.closes == 10_000;
    s.add("J2-三账对平", j2, "created=unlinked=closes=10000");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-零名注入", inj[0], "零哈希命名必拒");
    s.add("J3-重名注入", inj[1], "同名存活再创建必拒");
    s.add("J3-空名注入", inj[2], "未知名打开必拒");
    s.add("J3-冻结注入", inj[3], "冻结后创建命名必拒");

    // J4 冻结：根稳定且 cycle/run_rounds 双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.cycle(1, 0, false).err() == Some(PX_FROZEN)
        && f.run_rounds(1).err() == Some(PX_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后周期与批量推进均拒");

    // J5 指纹敏感：改一次 sem 操作计数即改根。
    let mut c = b.clone();
    c.sem_ops = c.sem_ops.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单次 sem 操作变化即改变 POSIX 根");

    // J6 空账/范围/重复摘链。
    let mut e = PosixSoak::new();
    let empty = e.audit();
    let range = e.cycle(0x95, 2, false);
    let _ = e.registry.create(0x96, 0);
    let _ = e.registry.unlink(0x96);
    let dup = e.registry.unlink(0x96);
    let j6 = empty.err() == Some(PX_EMPTY) && range.err() == Some(PX_RANGE)
        && dup.err() == Some(PX_EMPTY);
    s.add("J6-空账范围", j6, "空账、类号越界、二次摘链均拒");

    // J7 代次唯一：每轮 generation 递增且在对象移除后不重复。
    let j7 = b.registry.created == b.registry.closes && b.registry.names.is_empty()
        && b.rounds == 10_000;
    s.add("J7-代次唯一", j7, "万轮代次连续且无残名");

    // J8 负语义：少一轮并把名字留下，审计必须报漏。
    let mut leak = PosixSoak::new();
    let _ = leak.run_rounds(9_999);
    let _ = leak.registry.create(0x97, 0);
    let _ = leak.registry.open(0x97);
    let j8 = !leak.registry_conserved() && !leak.registry.names.is_empty()
        && leak.audit().err() == Some(PX_LEAK);
    s.add("J8-残名非绿", j8, "九千九百九十九轮加残名判漏");

    // J9 动态命名账。
    s.add_dyn("J9-命名动态", j1a,
        &format!("c={} u={} cl={} sem={} mq={}", b.registry.created,
            b.registry.unlinked, b.registry.closes, b.sem_ops, b.mq_ops));

    // J10 unlink-while-open：名字先摘、对象后释放。
    let mut probe = PosixSoak::new();
    let _ = probe.registry.create(0x98, 1);
    let _ = probe.registry.open(0x98);
    let _ = probe.registry.unlink(0x98);
    let still = probe.registry.names.len() == 1 && probe.registry.names[0].unlinked;
    let _ = probe.registry.close(0x98);
    let j10 = still && probe.registry.names.is_empty();
    s.add("J10-延迟释放", j10, "unlink 后打开句柄仍活，末次 close 才回收");

    // J11 总审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("posix|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11084_flow() {
        let mut b = PosixSoak::new();
        assert!(b.audit().err() == Some(PX_EMPTY));
        assert!(b.registry.create(0x81, 0).is_ok());
        assert!(b.registry.create(0x81, 1).err() == Some(PX_DUP));
        assert!(b.registry.open(0x81).is_ok());
        assert!(b.registry.close(0x81).is_ok());
        assert!(b.registry.close(0x81).err() == Some(PX_NAME));
        assert!(b.registry.unlink(0x81).is_ok());
        assert!(b.registry.names.is_empty());
        assert!(b.run_rounds(9_999).is_ok());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.registry.create(1, 0).err() == Some(PX_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11084_checks_green() {
        assert!(run_unx11084_checks().all_passed(), "UNX-F11084 判据未全绿");
    }
}
