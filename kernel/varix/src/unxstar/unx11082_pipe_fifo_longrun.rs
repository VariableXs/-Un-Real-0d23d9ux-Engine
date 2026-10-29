//! UNX-F11082 管道 FIFO 长稳段（多写多读万轮）（0x1ea40 段）
//!
//! FIFO 以 10 个窗口各 1000 轮推进；每轮先在原子边界上预排空到能整块写入，
//! 再执行一次普通读。所有占用深度都是轮内标量，十轮窗口只保留汇总，不把万轮
//! 样本堆进 Vec。每个窗口结束时强制排空并记账，因此全局不变式始终是
//! `bytes_in == bytes_out`，且 `partial_writes == 0`、峰值深度不超过容量。
//!
//! 结构：管道剖面（容量/写块/读块）+ 窗口账（轮数/写入/读出/峰值/边界次数）
//! + 长稳总账（万轮守恒、原子边界零漂、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11082 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1ea40;

/// 空账 —— 尚未跑任何窗口。
pub const PF_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 容量/块大小/窗口号/轮数越界。
pub const PF_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一窗口号重复推进。
pub const PF_DUP: u32 = SEG_BASE + 0x03;
/// 漂移 —— 写入与读出字节不相等，或残留深度非零。
pub const PF_DRIFT: u32 = SEG_BASE + 0x04;
/// 越界/半写 —— 深度超过容量或出现非原子部分写。
pub const PF_OVER: u32 = SEG_BASE + 0x05;
/// 封账后推进 —— 长稳账冻结后仍跑轮。
pub const PF_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（管道长稳账）。
pub const PF_TAG: u64 = 0x1ea_82;

/// 总轮数。
pub const PF_ROUNDS: u32 = 10_000;
/// 窗口数。
pub const PF_EPOCHS: usize = 10;
/// 每窗轮数。
pub const PF_EPOCH_ROUNDS: u32 = 1_000;

/// 管道运行剖面。
#[derive(Clone, Debug)]
pub struct PipeProfile {
    pub pid: u32,
    pub capacity: u32,
    pub write_chunk: u32,
    pub read_chunk: u32,
    pub owner: u8,
}

impl PipeProfile {
    /// 容量至少 1KiB，读写块均为正且写块不超过容量一半。
    pub fn validate(&self) -> Result<(), u32> {
        if self.pid == 0 || self.owner == 0 {
            return Err(PF_RANGE);
        }
        if self.capacity < 1_024 || self.capacity > 1_048_576 {
            return Err(PF_RANGE);
        }
        if self.write_chunk == 0 || self.read_chunk == 0 {
            return Err(PF_RANGE);
        }
        if self.write_chunk > self.capacity / 2 {
            return Err(PF_RANGE);
        }
        Ok(())
    }
}

/// 一个千轮窗口的汇总。
#[derive(Clone, Debug)]
pub struct FifoEpoch {
    pub epoch: u16,
    pub rounds: u16,
    pub wrote: u64,
    pub read: u64,
    pub peak: u32,
    pub boundary: u32,
    pub partial: u32,
}

impl FifoEpoch {
    /// 窗口内守恒。
    pub fn balanced(&self) -> bool {
        self.rounds != 0 && self.wrote == self.read && self.partial == 0
    }
}

/// 管道长稳总账。
#[derive(Clone, Debug)]
pub struct PipeSoak {
    pub profile: PipeProfile,
    pub epochs: Vec<FifoEpoch>,
    pub rounds_total: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub boundary_hits: u32,
    pub partial_writes: u32,
    pub drained: u32,
    pub residual: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PipeSoak {
    /// 带剖面建立长稳账；剖面必须先通过校验。
    pub fn with_profile(profile: PipeProfile) -> Result<PipeSoak, u32> {
        profile.validate()?;
        Ok(PipeSoak {
            profile,
            epochs: Vec::new(),
            rounds_total: 0,
            bytes_in: 0,
            bytes_out: 0,
            boundary_hits: 0,
            partial_writes: 0,
            drained: 0,
            residual: 0,
            frozen: false,
            root: None,
        })
    }

    fn epoch_at(&self, epoch: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.epochs.len() {
            if self.epochs[i].epoch == epoch {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 跑一个窗口。depth 是轮内标量，窗口结束前必被排空。
    pub fn run_epoch(&mut self, epoch: u16, rounds: u32) -> Result<FifoEpoch, u32> {
        if self.frozen {
            return Err(PF_FROZEN);
        }
        if epoch >= PF_EPOCHS as u16 || rounds == 0 || rounds > PF_EPOCH_ROUNDS {
            return Err(PF_RANGE);
        }
        if self.epoch_at(epoch).is_some() {
            return Err(PF_DUP);
        }
        let cap = self.profile.capacity;
        let wc = self.profile.write_chunk;
        let rc = self.profile.read_chunk;
        let mut depth = 0u32;
        let mut wrote = 0u64;
        let mut read = 0u64;
        let mut peak = 0u32;
        let mut boundary = 0u32;
        let mut partial = 0u32;
        let mut r = 0u32;
        while r < rounds {
            let idx = (epoch as u32).wrapping_mul(PF_EPOCH_ROUNDS).wrapping_add(r);
            // 写块 512..608，读块 384..416，二者错位制造边界压力。
            let want_w = wc.wrapping_add((r % 4).wrapping_mul(32));
            let occupied = depth as u64 + want_w as u64;
            let need = if occupied > cap as u64 {
                (occupied - cap as u64) as u32
            } else {
                0
            };
            if need > 0 {
                if need > depth {
                    partial = partial.wrapping_add(1);
                    return Err(PF_OVER);
                }
                depth = depth.wrapping_sub(need);
                read = read.wrapping_add(need as u64);
                boundary = boundary.wrapping_add(1);
            }
            if depth as u64 + want_w as u64 > cap as u64 {
                partial = partial.wrapping_add(1);
                return Err(PF_OVER);
            }
            depth = depth.wrapping_add(want_w);
            wrote = wrote.wrapping_add(want_w as u64);
            if depth > peak {
                peak = depth;
            }
            let want_r = rc.wrapping_add((r % 3).wrapping_mul(16));
            let take = if depth < want_r { depth } else { want_r };
            depth = depth.wrapping_sub(take);
            read = read.wrapping_add(take as u64);
            if depth > cap {
                partial = partial.wrapping_add(1);
                return Err(PF_OVER);
            }
            // idx 参与一个无副作用扰动，保证窗口样本不是彼此复制的常量。
            if idx % 97 == 0 && depth != 0 {
                boundary = boundary.wrapping_add(1);
            }
            r = r.wrapping_add(1);
        }
        // 窗口末排空，保证窗口守恒，也给下一窗口一个干净的起点。
        read = read.wrapping_add(depth as u64);
        self.drained = self.drained.wrapping_add(depth);
        depth = 0;
        self.residual = depth;
        self.rounds_total = self.rounds_total.wrapping_add(rounds);
        self.bytes_in = self.bytes_in.wrapping_add(wrote);
        self.bytes_out = self.bytes_out.wrapping_add(read);
        self.boundary_hits = self.boundary_hits.wrapping_add(boundary);
        self.partial_writes = self.partial_writes.wrapping_add(partial);
        let rec = FifoEpoch {
            epoch,
            rounds: rounds as u16,
            wrote,
            read,
            peak,
            boundary,
            partial,
        };
        self.epochs.push(rec.clone());
        Ok(rec)
    }

    /// 跑满十个窗口（万轮）。
    pub fn run_all(&mut self) -> Result<u64, u32> {
        let mut e = 0u16;
        while e < PF_EPOCHS as u16 {
            self.run_epoch(e, PF_EPOCH_ROUNDS)?;
            e = e.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 字节守恒：万轮写入等于读出，残留与半写均为零。
    pub fn conserved(&self) -> bool {
        self.rounds_total == PF_ROUNDS
            && self.bytes_in == self.bytes_out
            && self.residual == 0
            && self.partial_writes == 0
            && self.epochs.len() == PF_EPOCHS
    }

    /// 十窗账逐窗平衡且累计对平。
    pub fn epoch_chain_ok(&self) -> bool {
        if self.epochs.len() != PF_EPOCHS {
            return false;
        }
        let mut sw = 0u64;
        let mut sr = 0u64;
        let mut i = 0usize;
        while i < self.epochs.len() {
            if self.epochs[i].epoch as usize != i {
                return false;
            }
            if !self.epochs[i].balanced() {
                return false;
            }
            sw = sw.wrapping_add(self.epochs[i].wrote);
            sr = sr.wrapping_add(self.epochs[i].read);
            i += 1;
        }
        sw == self.bytes_in && sr == self.bytes_out
    }

    /// 审计：空、残留、漂移、半写分别拒绝。
    pub fn audit(&self) -> Result<(), u32> {
        if self.epochs.is_empty() {
            return Err(PF_EMPTY);
        }
        if self.residual > self.profile.capacity {
            return Err(PF_OVER);
        }
        if self.residual != 0 || self.bytes_in != self.bytes_out {
            return Err(PF_DRIFT);
        }
        if self.partial_writes != 0 {
            return Err(PF_OVER);
        }
        Ok(())
    }

    /// FNV-1a：剖面 + 每个窗口的轮数/字节/峰值/边界/半写全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PF_TAG;
        h = (h ^ self.profile.pid as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.profile.capacity as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.profile.write_chunk as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.profile.read_chunk as u64).wrapping_mul(0x100000001b3);
        let mut i = 0usize;
        while i < self.epochs.len() {
            let e = &self.epochs[i];
            h = (h ^ e.epoch as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.rounds as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.wrote).wrapping_mul(0x100000001b3);
            h = (h ^ e.read).wrapping_mul(0x100000001b3);
            h = (h ^ e.peak as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.boundary as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.partial as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h = (h ^ self.bytes_in).wrapping_mul(0x100000001b3);
        h = (h ^ self.bytes_out).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结长稳账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 万轮摘要。
    pub fn summary(&self) -> String {
        let label = "fifo".to_string();
        format!("{}|rounds={}|in={}|out={}|peak_boundary={}", label,
            self.rounds_total, self.bytes_in, self.bytes_out, self.boundary_hits)
    }
}

/// 四路坏样本：窗口重号、剖面越界、账目漂移、冻结推进。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let profile = PipeProfile {
        pid: 1,
        capacity: 4_096,
        write_chunk: 512,
        read_chunk: 384,
        owner: 1,
    };
    let mut a = match PipeSoak::with_profile(profile) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = a.run_epoch(0, 1_000);
    got[0] = a.run_epoch(0, 1_000).err() == Some(PF_DUP);

    let bad = PipeProfile {
        pid: 2,
        capacity: 128,
        write_chunk: 64,
        read_chunk: 16,
        owner: 1,
    };
    got[1] = PipeSoak::with_profile(bad).err() == Some(PF_RANGE);

    let mut c = match PipeSoak::with_profile(PipeProfile {
        pid: 3,
        capacity: 4_096,
        write_chunk: 512,
        read_chunk: 384,
        owner: 1,
    }) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = c.run_epoch(0, 1_000);
    c.bytes_in = c.bytes_in.wrapping_add(1);
    got[2] = c.audit().err() == Some(PF_DRIFT);

    let mut d = match PipeSoak::with_profile(PipeProfile {
        pid: 4,
        capacity: 4_096,
        write_chunk: 512,
        read_chunk: 384,
        owner: 1,
    }) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = d.run_epoch(0, 1_000);
    let _ = d.freeze();
    got[3] = d.run_epoch(1, 1_000).err() == Some(PF_FROZEN);
    got
}

/// 管道万轮长稳判据。
pub fn run_unx11082_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11082");

    let profile = PipeProfile {
        pid: 0x82,
        capacity: 4_096,
        write_chunk: 512,
        read_chunk: 384,
        owner: 2,
    };
    let mut b = match PipeSoak::with_profile(profile) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let run = b.run_all();

    // J1 两半：万轮字节守恒，原子边界万轮零漂。
    let j1a = b.bytes_in == b.bytes_out && b.rounds_total == 10_000 && b.conserved();
    let j1b = b.partial_writes == 0 && b.residual == 0 && b.boundary_hits > 0;
    s.add("J1-万轮字节守恒", j1a, "10000 轮写入字节等于读出字节");
    s.add("J1-原子边界零漂", j1b, "无半写且窗口末零残留");

    // J2 十窗链：逐窗平衡且累计与总账对平。
    let j2 = b.epoch_chain_ok() && run.is_ok() && b.epochs.len() == 10;
    s.add("J2-十窗总账对平", j2, "10 窗各自平衡并与全局字节对平");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同窗二次推进必拒");
    s.add("J3-剖面注入", inj[1], "容量不足剖面必拒");
    s.add("J3-漂移注入", inj[2], "字节账人为偏差必被审计抓住");
    s.add("J3-冻结注入", inj[3], "冻结后推进窗口必拒");

    // J4 冻结：根稳定且 run_epoch/run_all 双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.run_epoch(0, 1).err() == Some(PF_FROZEN)
        && f.run_all().err() == Some(PF_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后单窗/全跑均拒且根稳定");

    // J5 指纹敏感：单窗峰值变化即改根。
    let mut c = b.clone();
    c.epochs[4].peak = c.epochs[4].peak.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单窗峰值变化即改变 FIFO 账根");

    // J6 空账/范围/越界残留。
    let mut e = match PipeSoak::with_profile(PipeProfile {
        pid: 9,
        capacity: 2_048,
        write_chunk: 256,
        read_chunk: 128,
        owner: 1,
    }) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let empty = e.audit();
    let range = e.run_epoch(10, 1_000);
    let _ = e.run_epoch(0, 1_000);
    e.residual = 2_049;
    let over = e.audit();
    let j6 = empty.err() == Some(PF_EMPTY) && range.err() == Some(PF_RANGE)
        && over.err() == Some(PF_OVER);
    s.add("J6-空账越界", j6, "空账、越界窗口、超容残留均拒");

    // J7 窗口唯一有序：epoch 0..9 严格递增。
    let mut ordered = true;
    let mut k = 0usize;
    while k < b.epochs.len() {
        if b.epochs[k].epoch as usize != k {
            ordered = false;
        }
        k += 1;
    }
    s.add("J7-窗口有序", ordered, "10 个窗口号严格为 0..9");

    // J8 负语义：九窗时总账不得判全绿。
    let mut partial = match PipeSoak::with_profile(PipeProfile {
        pid: 10,
        capacity: 4_096,
        write_chunk: 512,
        read_chunk: 384,
        owner: 1,
    }) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut pe = 0u16;
    while pe < 9 {
        let _ = partial.run_epoch(pe, 1_000);
        pe = pe.wrapping_add(1);
    }
    let j8 = !partial.conserved() && !partial.epoch_chain_ok()
        && partial.rounds_total == 9_000;
    s.add("J8-九窗非绿", j8, "九窗账不得冒充万轮守恒");

    // J9 动态字节账。
    s.add_dyn("J9-万轮动态", j1a,
        &format!("in={} out={} drained={} boundary={}", b.bytes_in,
            b.bytes_out, b.drained, b.boundary_hits));

    // J10 峰值不越容量。
    let mut peak_ok = true;
    let mut pi = 0usize;
    while pi < b.epochs.len() {
        if b.epochs[pi].peak > b.profile.capacity {
            peak_ok = false;
        }
        pi += 1;
    }
    s.add("J10-峰值有界", peak_ok && b.epochs.iter().any(|x| x.peak > 0),
        "每窗峰值均不超过 4096 容量");

    // J11 审计与剖面校验。
    let j11 = b.audit().is_ok() && b.profile.validate().is_ok();
    s.add("J11-审计剖面", j11, "总审计通过且剖面合法");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11082_flow() {
        let mut b = match PipeSoak::with_profile(PipeProfile {
            pid: 1,
            capacity: 4_096,
            write_chunk: 512,
            read_chunk: 384,
            owner: 1,
        }) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(b.audit().err() == Some(PF_EMPTY));
        assert!(b.run_epoch(0, 1_000).is_ok());
        assert!(b.run_epoch(0, 1_000).err() == Some(PF_DUP));
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.run_epoch(1, 1_000).err() == Some(PF_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11082_checks_green() {
        assert!(run_unx11082_checks().all_passed(), "UNX-F11082 判据未全绿");
    }
}
