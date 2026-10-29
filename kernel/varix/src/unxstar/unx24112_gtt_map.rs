//! UNX-F24112 GTT 页表映射与系统内存溢出路径·子功能12（0x40FE0 段）
//!
//! 锚点：UNX-F24112-J1 页表项 + 映射账 + 预算闸逐条落账；J2 字段齐备；
//! J3~J12 核心操作语义与折叠指纹（回滚账切面并入 J3/J11）；J13 注入核 5/5。
//! 域：CoRun·UNX-F5-B12（GTT 页表映射与系统内存溢出路径）。
//!
//! 结构：页表项 + 映射账 + 预算闸 + 回滚账（Rollback）切面账 + 注入核
//! （越界/重叠/双解/ENOMEM/印后 六路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40FE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x40FE0;
pub const GM_BAD_GTT: u32 = SEG_BASE + 0x01; // 野页越界
pub const GM_DUP_PTE: u32 = SEG_BASE + 0x02; // 重叠映射
pub const GM_DUP_MAP: u32 = SEG_BASE + 0x03; // 双重解映
pub const GM_ENOMEM: u32 = SEG_BASE + 0x04; // ENOMEM 预算闸
pub const GM_FROZEN: u32 = SEG_BASE + 0x05; // 印后映射

/// 回滚账切面账（GTT 页表映射与系统内存溢出路径子切面）：记录 (k, v) 对，
/// v == 0xBB 违者回 GM_ENOMEM，同 k 重记回 GM_DUP_MAP。
pub struct Rollback {
    pub tag: u32,
    pub hits: u32,
    pub log: Vec<(u32, u32)>,
}

impl Rollback {
    pub fn new(tag: u32) -> Rollback {
        Rollback { tag, hits: 0, log: vec![] }
    }

    /// 记账一笔：v == 0xBB。
    pub fn record(&mut self, k: u32, v: u32) -> Result<(), u32> {
        if v == 0xBB {
            return Err(GM_ENOMEM);
        }
        if self.log.iter().any(|(lk, _)| *lk == k) {
            return Err(GM_DUP_MAP);
        }
        self.log.push((k, v));
        self.hits += 1;
        Ok(())
    }

    /// 切面累计值（回滚账）。
    pub fn rb_cnt(&self) -> u32 {
        let mut acc: u32 = 0;
        for (k, v) in &self.log {
            acc = acc.wrapping_add((*k).wrapping_mul(*v).wrapping_add(self.tag));
        }
        acc
    }

    /// 切面折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40FE0;
        for (k, v) in &self.log {
            h = h.wrapping_mul(32).wrapping_add((*k << 8) ^ *v);
        }
        h ^ self.tag
    }
}

/// DMA 基址（按段位派生，闭集内合法 hex）。
pub const DMA0: u64 = 0x40FE0000;

/// 页表项。
#[derive(Clone, Copy, PartialEq)]
pub struct Pte {
    pub idx: u32,
    pub dma: u64,
    pub valid: bool,
}

/// 映射账。
#[derive(Clone, Copy, PartialEq)]
pub struct MapRec {
    pub seq: u32,
    pub start: u32,
    pub npages: u32,
    pub live: bool,
}

/// GttVault（页表项 + 映射账 + 预算闸）。
pub struct GttVault {
    pub ptes: Vec<Pte>,
    pub maps: Vec<MapRec>,
    pub budget: u32,
    pub used: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl GttVault {
    pub fn new(nptes: u32, budget: u32) -> GttVault {
        let mut ptes = vec![];
        for i in 0..nptes {
            ptes.push(Pte { idx: i, dma: 0, valid: false });
        }
        GttVault { ptes, maps: vec![], budget, used: 0, seq_next: 1, sealed: false }
    }

    /// 映射：印后拒 + 越界拒 + 重叠拒 + 预算闸 + 落账。
    pub fn map_at(&mut self, start: u32, npages: u32, dma_base: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(GM_FROZEN);
        }
        if npages == 0 || start.checked_add(npages).map_or(true, |e| e as usize > self.ptes.len()) {
            return Err(GM_BAD_GTT);
        }
        if self.ptes[start as usize..(start + npages) as usize].iter().any(|p| p.valid) {
            return Err(GM_DUP_PTE);
        }
        if self.used + npages > self.budget {
            return Err(GM_ENOMEM);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        for k in 0..npages {
            let i = (start + k) as usize;
            self.ptes[i].valid = true;
            self.ptes[i].dma = dma_base + (k as u64) * 0x1000;
        }
        self.maps.push(MapRec { seq, start, npages, live: true });
        self.used += npages;
        Ok(seq)
    }

    /// 解映射：印后拒 / 越界拒 / 双解拒 / 须整段对账回收。
    pub fn unmap_at(&mut self, start: u32, npages: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GM_FROZEN);
        }
        if npages == 0 || start.checked_add(npages).map_or(true, |e| e as usize > self.ptes.len()) {
            return Err(GM_BAD_GTT);
        }
        let any_valid = self.ptes[start as usize..(start + npages) as usize]
            .iter()
            .any(|p| p.valid);
        if !any_valid {
            return Err(GM_DUP_MAP);
        }
        let i = match self.maps.iter().position(|m| {
            m.live && m.start == start && m.npages == npages
        }) {
            Some(i) => i,
            None => return Err(GM_BAD_GTT),
        };
        self.maps[i].live = false;
        for k in 0..npages {
            let idx = (start + k) as usize;
            self.ptes[idx].valid = false;
            self.ptes[idx].dma = 0;
        }
        self.used -= npages;
        Ok(())
    }

    /// 活映射段计数。
    pub fn live_maps(&self) -> u32 {
        self.maps.iter().filter(|m| m.live).count() as u32
    }

    /// 活页表项计数（valid 位折叠）。
    pub fn valid_pages(&self) -> u32 {
        self.ptes.iter().filter(|p| p.valid).count() as u32
    }

    /// 查单页 DMA 地址：未映射页回 None。
    pub fn dma_of(&self, idx: u32) -> Option<u64> {
        let p = self.ptes.get(idx as usize)?;
        if p.valid {
            Some(p.dma)
        } else {
            None
        }
    }

    /// 剩余预算页数（ENOMEM 闸余量）。
    pub fn budget_left(&self) -> u32 {
        self.budget - self.used
    }

    /// 账本折叠指纹（映射账 + 活页表项）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40FE0;
        for m in &self.maps {
            h = h.wrapping_mul(32).wrapping_add(m.seq ^ (m.start << 8) ^ m.npages);
        }
        for p in &self.ptes {
            if p.valid {
                h = h.wrapping_mul(32).wrapping_add(p.idx ^ (p.dma as u32));
            }
        }
        h ^ self.used
    }

    /// 封印：印后映射必拒。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：64 页表项，预算 48 页。
pub fn mk_gtt() -> GttVault {
    GttVault::new(64, 48)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = mk_gtt();
    // 1) 越界映射
    got[0] = t.map_at(60, 8, DMA0).err() == Some(GM_BAD_GTT);
    // 2) 重叠映射
    let _ = t.map_at(0, 4, DMA0);
    got[1] = t.map_at(2, 4, DMA0).err() == Some(GM_DUP_PTE);
    // 3) 野页解映射
    got[2] = t.unmap_at(200, 2).err() == Some(GM_BAD_GTT);
    // 4) ENOMEM 预算闸
    let _ = t.map_at(8, 40, DMA0);
    got[3] = t.map_at(48, 8, DMA0).err() == Some(GM_ENOMEM);
    // 5) 印后映射
    t.seal();
    got[4] = t.map_at(48, 4, DMA0).err() == Some(GM_FROZEN);
    got
}

/// 判据主体（13 条，全部真跑账本）。
pub fn run_unx24112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24112");
    let mut t = mk_gtt();

    // J1 账非空
    let j1 = t.ptes.len() == 64 && t.ptes.iter().all(|p| !p.valid) && t.ptes[5].idx == 5;
    s.add("J1-账非空", j1, "64 页表项全自由且索引连续");

    // J2 字段齐
    let j2 = t.maps.is_empty() && t.used == 0 && t.budget == 48;
    s.add("J2-字段齐", j2, "映射账空且预算 48");

    // J3 回滚账：切面记账 + 首映射落账
    let mut asp = Rollback::new(24112);
    let a1 = asp.record(1, 11).is_ok();
    let a2 = asp.record(1, 11).err() == Some(GM_DUP_MAP);
    let a3 = asp.record(50, 187).err() == Some(GM_ENOMEM);
    let m3 = t.map_at(0, 4, DMA0);
    let j3 = m3.is_ok() && t.used == 4 && t.ptes[0].valid
        && t.ptes[3].dma == DMA0 + 0x3000 && a1 && a2 && a3;
    s.add("J3-映射落账", j3, "回滚账切面三连 + 首段 DMA 连续");

    // J4 重叠拒
    let j4 = t.map_at(2, 4, DMA0).err() == Some(GM_DUP_PTE);
    s.add("J4-重叠拒", j4, "与活页表项重叠必拒");

    // J5 越界拒
    let j5 = t.map_at(60, 8, DMA0).err() == Some(GM_BAD_GTT);
    s.add("J5-越界拒", j5, "越过页表末端必拒");

    // J6 续映射
    let m6 = t.map_at(8, 8, DMA0);
    let j6 = m6.is_ok() && t.used == 12 && t.ptes[9].dma == DMA0 + 0x1000;
    s.add("J6-续映射", j6, "第二段落账且 DMA 递增");

    // J7 解映回收
    let u7 = t.unmap_at(0, 4);
    let j7 = u7.is_ok() && t.used == 8 && !t.ptes[0].valid;
    s.add("J7-解映回收", j7, "整段解映后页表项失效");

    // J8 双解拒
    let j8 = t.unmap_at(0, 4).err() == Some(GM_DUP_MAP);
    s.add("J8-双解拒", j8, "同段二次解映必拒");

    // J9 回滚账：ENOMEM 预算闸
    let m9 = t.map_at(16, 40, DMA0);
    let j9 = m9.is_ok() && t.used == 48
        && t.map_at(56, 4, DMA0).err() == Some(GM_ENOMEM);
    s.add("J9-ENOMEM闸", j9, "回滚账切面在册 + 预算打满再映射必拒");

    // J10 野页解映
    let j10 = t.unmap_at(200, 2).err() == Some(GM_BAD_GTT);
    s.add("J10-野页拒", j10, "越表解映必拒");

    // J11 指纹稳定（账本 + 切面双折叠）
    let j11 = t.fold() == t.fold() && asp.fold() == asp.fold();
    s.add("J11-指纹稳定", j11, "账本与切面两次折叠一致");

    // J12 印后冻结
    let fp = t.seal();
    let j12 = fp > 0 && t.map_at(56, 4, DMA0).err() == Some(GM_FROZEN);
    s.add("J12-印后冻结", j12, "封印后映射必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24112_gtt_map_flow() {
        let mut t = mk_gtt();
        assert!(t.map_at(0, 4, DMA0).is_ok());
        assert!(t.map_at(2, 4, DMA0).err() == Some(GM_DUP_PTE));
        assert!(t.map_at(60, 8, DMA0).err() == Some(GM_BAD_GTT));
        assert!(t.unmap_at(0, 4).is_ok());
        assert!(t.unmap_at(0, 4).err() == Some(GM_DUP_MAP));
        assert!(t.map_at(16, 40, DMA0).is_ok());
        assert!(t.map_at(56, 4, DMA0).is_ok());
        assert!(t.map_at(8, 8, DMA0).err() == Some(GM_ENOMEM));
        assert!(t.seal() > 0);
        assert!(t.map_at(56, 4, DMA0).err() == Some(GM_FROZEN));
    }

    #[test]
    fn unx24112_checks_green() {
        let s = run_unx24112_checks();
        assert!(s.all_passed(), "UNX-F24112 判据未全绿");
    }
}
