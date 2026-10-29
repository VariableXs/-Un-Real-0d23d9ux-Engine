//! UNX-F11089 内存账长稳监控（泄漏探针全网）（0x1eb20 段）
//!
//! 8 个分配站点各自维护 live/peak/alloc/free 四个计数；万轮循环每轮在一个
//! 站点上分配并立即释放，所有站点期末必须回到 live=0。每 1250 轮做一次全网
//! 泄漏扫描，探针记录扫描前后的 outstanding 字节与首个命中站点；8 个探针
//! 覆盖完整万轮。不变式：allocated == released、live == 0、8 个探针零命中。
//!
//! 结构：分配站点（类别/存活/峰值/次数）+ 泄漏探针（轮次/前后字节/命中位）
//! + 内存长稳账（零增长、探针全网、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11089 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1eb20;

/// 空账 —— 没有分配站点，或站点表为空时做泄漏扫描。
pub const ML_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 站点号越界、类别为零、分配字节为零或超上限。
pub const ML_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同站点重复登记，或探针槽已满仍追加。
pub const ML_DUP: u32 = SEG_BASE + 0x03;
/// 泄漏 —— 期末 live 非零，或 allocated != released，或探针命中。
pub const ML_LEAK: u32 = SEG_BASE + 0x04;
/// 样本不足 —— 释放字节大于 live，或长稳轮数未达 10000。
pub const ML_SHORT: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 内存账冻结后仍分配/释放/扫描。
pub const ML_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（内存长稳账）。
pub const ML_TAG: u64 = 0x1ea_89;

/// 站点数。
pub const ML_SITES: usize = 8;
/// 探针槽数。
pub const ML_PROBES: usize = 8;
/// 长稳轮数。
pub const ML_ROUNDS: u32 = 10_000;
/// 每多少轮做一次全网探针。
pub const ML_PROBE_EVERY: u32 = 1_250;

/// 一个分配站点。
#[derive(Clone, Debug)]
pub struct AllocSite {
    pub site: u8,
    pub class: u16,
    pub live: u64,
    pub peak: u64,
    pub allocations: u32,
    pub frees: u32,
}

impl AllocSite {
    /// 站点账是否闭合。
    pub fn settled(&self) -> bool {
        self.live == 0 && self.allocations == self.frees
    }
}

/// 一次全网泄漏探针。
#[derive(Clone, Debug)]
pub struct LeakProbe {
    pub round: u16,
    pub site: u8,
    pub live_before: u64,
    pub live_after: u64,
    pub hit: bool,
}

impl LeakProbe {
    /// 探针零命中且扫描前后无变化。
    pub fn clean(&self) -> bool {
        !self.hit && self.live_before == 0 && self.live_after == 0 && self.site == 0xff
    }
}

/// 内存长稳总账。
#[derive(Clone, Debug)]
pub struct MemSoak {
    pub sites: Vec<AllocSite>,
    pub probes: Vec<LeakProbe>,
    pub rounds: u32,
    pub allocated: u64,
    pub released: u64,
    pub live: u64,
    pub peak: u64,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MemSoak {
    pub fn new() -> MemSoak {
        MemSoak {
            sites: Vec::new(),
            probes: Vec::new(),
            rounds: 0,
            allocated: 0,
            released: 0,
            live: 0,
            peak: 0,
            frozen: false,
            root: None,
        }
    }

    fn site_at(&self, site: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.sites.len() {
            if self.sites[i].site == site {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记分配站点。
    pub fn register_site(&mut self, site: u8, class: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(ML_FROZEN);
        }
        if site >= ML_SITES as u8 || class == 0 || class > 3 {
            return Err(ML_RANGE);
        }
        if self.site_at(site).is_some() {
            return Err(ML_DUP);
        }
        self.sites.push(AllocSite {
            site,
            class,
            live: 0,
            peak: 0,
            allocations: 0,
            frees: 0,
        });
        Ok(())
    }

    /// 分配；字节数 1..=1MiB。
    pub fn alloc(&mut self, site: u8, bytes: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(ML_FROZEN);
        }
        if bytes == 0 || bytes > 1_048_576 {
            return Err(ML_RANGE);
        }
        let si = match self.site_at(site) {
            Some(i) => i,
            None => return Err(ML_RANGE),
        };
        self.sites[si].live = self.sites[si].live.wrapping_add(bytes);
        self.sites[si].allocations = self.sites[si].allocations.wrapping_add(1);
        if self.sites[si].live > self.sites[si].peak {
            self.sites[si].peak = self.sites[si].live;
        }
        self.live = self.live.wrapping_add(bytes);
        self.allocated = self.allocated.wrapping_add(bytes);
        if self.live > self.peak {
            self.peak = self.live;
        }
        Ok(())
    }

    /// 释放；释放量大于站点 live 时判样本不足，绝不把 live 减成负数。
    pub fn free(&mut self, site: u8, bytes: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(ML_FROZEN);
        }
        if bytes == 0 {
            return Err(ML_RANGE);
        }
        let si = match self.site_at(site) {
            Some(i) => i,
            None => return Err(ML_RANGE),
        };
        if bytes > self.sites[si].live {
            return Err(ML_SHORT);
        }
        self.sites[si].live = self.sites[si].live.wrapping_sub(bytes);
        self.sites[si].frees = self.sites[si].frees.wrapping_add(1);
        self.live = self.live.wrapping_sub(bytes);
        self.released = self.released.wrapping_add(bytes);
        Ok(())
    }

    /// 全网 outstanding 字节。
    pub fn scan_live(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.sites.len() {
            n = n.wrapping_add(self.sites[i].live);
            i += 1;
        }
        n
    }

    fn first_live_site(&self) -> u8 {
        let mut i = 0usize;
        while i < self.sites.len() {
            if self.sites[i].live != 0 {
                return self.sites[i].site;
            }
            i += 1;
        }
        0xff
    }

    /// 全网泄漏探针。
    pub fn probe(&mut self, round: u16) -> Result<bool, u32> {
        if self.frozen {
            return Err(ML_FROZEN);
        }
        if self.sites.is_empty() {
            return Err(ML_EMPTY);
        }
        if self.probes.len() >= ML_PROBES {
            return Err(ML_DUP);
        }
        let before = self.scan_live();
        let site = self.first_live_site();
        let after = self.scan_live();
        let hit = before != 0 || after != 0;
        self.probes.push(LeakProbe {
            round,
            site,
            live_before: before,
            live_after: after,
            hit,
        });
        Ok(hit)
    }

    /// 万轮分配/释放循环；每 1250 轮插入一次探针。
    pub fn run_rounds(&mut self, rounds: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(ML_FROZEN);
        }
        if self.sites.len() != ML_SITES {
            return Err(ML_EMPTY);
        }
        if rounds == 0 || rounds > 100_000 {
            return Err(ML_RANGE);
        }
        let mut r = 0u32;
        while r < rounds {
            let site = (r % ML_SITES as u32) as u8;
            let bytes = 64u64.wrapping_add((r % 7) as u64 * 32);
            self.alloc(site, bytes)?;
            self.free(site, bytes)?;
            if r % ML_PROBE_EVERY == 0 {
                let _ = self.probe(r as u16)?;
            }
            self.rounds = self.rounds.wrapping_add(1);
            r = r.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 万轮零增长：分配等于释放，期末 live=0，站点全闭合。
    pub fn zero_growth(&self) -> bool {
        if self.rounds != ML_ROUNDS || self.allocated != self.released || self.live != 0 {
            return false;
        }
        if self.peak == 0 || self.sites.len() != ML_SITES {
            return false;
        }
        let mut i = 0usize;
        while i < self.sites.len() {
            if !self.sites[i].settled() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 探针全网：8 个探针全部零命中。
    pub fn probes_clean(&self) -> bool {
        if self.probes.len() != ML_PROBES {
            return false;
        }
        let mut i = 0usize;
        while i < self.probes.len() {
            if !self.probes[i].clean() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.sites.is_empty() {
            return Err(ML_EMPTY);
        }
        if self.live != 0 || self.allocated != self.released
            || self.sites.iter().any(|s| s.live != 0)
        {
            return Err(ML_LEAK);
        }
        if self.rounds != ML_ROUNDS {
            return Err(ML_SHORT);
        }
        if self.probes.iter().any(|p| p.hit) {
            return Err(ML_LEAK);
        }
        Ok(())
    }

    /// FNV-1a：站点账、探针账与全局字节账全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = ML_TAG;
        let mut i = 0usize;
        while i < self.sites.len() {
            let s = &self.sites[i];
            h = (h ^ s.site as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.class as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.live).wrapping_mul(0x100000001b3);
            h = (h ^ s.peak).wrapping_mul(0x100000001b3);
            h = (h ^ s.allocations as u64).wrapping_mul(0x100000001b3);
            h = (h ^ s.frees as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.probes.len() {
            let p = &self.probes[k];
            h = (h ^ p.round as u64).wrapping_mul(0x100000001b3);
            h = (h ^ p.site as u64).wrapping_mul(0x100000001b3);
            h = (h ^ p.live_before).wrapping_mul(0x100000001b3);
            h = (h ^ if p.hit { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.allocated).wrapping_mul(0x100000001b3);
        h = (h ^ self.released).wrapping_mul(0x100000001b3);
        h = (h ^ self.live).wrapping_mul(0x100000001b3);
        h = (h ^ self.peak).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结内存账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 内存账摘要。
    pub fn summary(&self) -> String {
        let label = "memsoak".to_string();
        format!("{}|rounds={}|alloc={}|free={}|live={}|peak={}", label, self.rounds,
            self.allocated, self.released, self.live, self.peak)
    }
}

/// 四路坏样本：站点重号、越界分配、过量释放、冻结后分配。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = MemSoak::new();
    let _ = a.register_site(0, 1);
    got[0] = a.register_site(0, 2).err() == Some(ML_DUP);

    let mut b = MemSoak::new();
    let _ = b.register_site(1, 1);
    got[1] = b.alloc(8, 64).err() == Some(ML_RANGE);

    let mut c = MemSoak::new();
    let _ = c.register_site(2, 1);
    let _ = c.alloc(2, 64);
    got[2] = c.free(2, 65).err() == Some(ML_SHORT);

    let mut d = MemSoak::new();
    let _ = d.register_site(3, 1);
    let _ = d.freeze();
    got[3] = d.alloc(3, 64).err() == Some(ML_FROZEN);
    got
}

/// 内存账万轮判据。
pub fn run_unx11089_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11089");

    let mut b = MemSoak::new();
    let mut site = 0u8;
    while site < ML_SITES as u8 {
        let _ = b.register_site(site, (site % 3 + 1) as u16);
        site = site.wrapping_add(1);
    }
    let run = b.run_rounds(ML_ROUNDS);

    // J1 两半：内存账万轮零增长，泄漏探针零命中。
    let j1a = b.zero_growth() && b.live == 0 && b.allocated == b.released;
    let j1b = b.probes_clean() && b.probes.len() == 8;
    s.add("J1-万轮零增长", j1a, "10000 轮分配/释放相等且 live 归零");
    s.add("J1-探针零命中", j1b, "8 个全网探针全部未命中");

    // J2 站点账逐站闭合。
    let mut settled = true;
    let mut i = 0usize;
    while i < b.sites.len() {
        if !b.sites[i].settled() || b.sites[i].peak == 0 {
            settled = false;
        }
        i += 1;
    }
    let j2 = settled && b.scan_live() == 0;
    s.add("J2-站点闭合", j2, "8 站点 live 归零且分配次数等于释放次数");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同站点二次登记必拒");
    s.add("J3-越界注入", inj[1], "站点 8 分配必拒");
    s.add("J3-超释注入", inj[2], "释放量大于 live 必拒");
    s.add("J3-冻结注入", inj[3], "冻结后分配必拒");

    // J4 冻结：根稳定且分配/释放/探针全拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.alloc(0, 64).err() == Some(ML_FROZEN)
        && f.free(0, 64).err() == Some(ML_FROZEN)
        && f.probe(0).err() == Some(ML_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后分配/释放/探针三路均拒");

    // J5 指纹敏感：单站点峰值变化即改根。
    let mut c = b.clone();
    c.sites[2].peak = c.sites[2].peak.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单站点峰值变化即改变内存账根");

    // J6 空账/范围/样本不足。
    let mut e = MemSoak::new();
    let empty = e.probe(0);
    let range = e.alloc(0, 0);
    let _ = e.register_site(0, 1);
    let _ = e.alloc(0, 64);
    let over = e.free(0, 65);
    let j6 = empty.err() == Some(ML_EMPTY) && range.err() == Some(ML_RANGE)
        && over.err() == Some(ML_SHORT);
    s.add("J6-空账超释", j6, "无站点、零字节、过量释放均拒");

    // J7 站点唯一有序：站点号 0..7 严格递增。
    let mut ordered = true;
    let mut k = 0usize;
    while k < b.sites.len() {
        if b.sites[k].site as usize != k {
            ordered = false;
        }
        k += 1;
    }
    s.add("J7-站点有序", ordered && b.sites.len() == 8, "8 个站点号唯一且严格递增");

    // J8 负语义：九千九百九十九轮加残活分配不得判零增长。
    let mut partial = MemSoak::new();
    let mut ps = 0u8;
    while ps < 8 {
        let _ = partial.register_site(ps, 1);
        ps = ps.wrapping_add(1);
    }
    let _ = partial.run_rounds(9_999);
    let _ = partial.alloc(3, 512);
    let j8 = !partial.zero_growth() && partial.live == 512
        && partial.audit().err() == Some(ML_LEAK);
    s.add("J8-残活非绿", j8, "残活 512B 时内存账不得判零增长");

    // J9 动态内存账。
    s.add_dyn("J9-内存动态", j1a,
        &format!("rounds={} alloc={} free={} live={} peak={}", b.rounds,
            b.allocated, b.released, b.live, b.peak));

    // J10 峰值与最终归零：峰值为正且期末为零。
    let j10 = b.peak >= 64 && b.peak <= 512 && b.live == 0 && b.scan_live() == 0;
    s.add("J10-峰值归零", j10, "峰值有界且期末 outstanding 为零");

    // J11 审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("memsoak|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11089_flow() {
        let mut b = MemSoak::new();
        assert!(b.audit().err() == Some(ML_EMPTY));
        let mut site = 0u8;
        while site < 8 {
            let _ = b.register_site(site, 1);
            site = site.wrapping_add(1);
        }
        assert!(b.register_site(0, 1).err() == Some(ML_DUP));
        assert!(b.alloc(0, 64).is_ok());
        assert!(b.free(0, 65).err() == Some(ML_SHORT));
        assert!(b.free(0, 64).is_ok());
        let _ = b.run_rounds(10_000);
        assert!(b.zero_growth() && b.probes_clean());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.alloc(0, 64).err() == Some(ML_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11089_checks_green() {
        assert!(run_unx11089_checks().all_passed(), "UNX-F11089 判据未全绿");
    }
}
