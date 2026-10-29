//! UNX-F10393 域哈希链终冻结（四十批快照链终验+域级根哈希）（0x1bba0 段）
//! 锚点：UNX-F10393-J1 域哈希链终冻结（链式 hash(prev||cur) 40 环+根哈希单值）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bba0;
pub const EV_BAD_LINK: u32 = SEG_BASE + 1;
pub const EV_NOT_FORTY: u32 = SEG_BASE + 2;
pub const EV_SEALED: u32 = SEG_BASE + 3;
pub const EV_EMPTY_CHAIN: u32 = SEG_BASE + 4;
pub const EV_DUP_SNAP: u32 = SEG_BASE + 5;
pub const EV_FROZEN: u32 = SEG_BASE + 6;

pub const LINK_ANCHOR: usize = 40;
const FNV_SEED: u32 = 0x811c_9dc5;

/// FNV-1a 32 位哈希（no_std 纯函数）
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = FNV_SEED;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 链环：批号 + 快照哈希（前环哈希||本批摘要 混入）
#[derive(Clone, Debug)]
pub struct ChainLink {
    pub batch_no: usize,
    pub payload: String,
    pub snap: u32,
}

impl ChainLink {
    pub fn new(batch_no: usize, payload: &str, prev_hash: u32) -> Self {
        let mut buf: Vec<u8> = Vec::new();
        let prev_bytes = prev_hash.to_le_bytes();
        let mut j = 0;
        while j < 4 {
            buf.push(prev_bytes[j]);
            j += 1;
        }
        let p = payload.as_bytes();
        let mut k = 0;
        while k < p.len() {
            buf.push(p[k]);
            k += 1;
        }
        ChainLink { batch_no, payload: payload.to_string(), snap: fnv1a(&buf) }
    }
}

/// 域哈希链：40 环串联 + 域级根哈希冻结
pub struct HashChain {
    links: Vec<ChainLink>,
    root: Option<u32>,
    frozen: bool,
}

impl HashChain {
    pub fn new() -> Self {
        HashChain { links: Vec::new(), root: None, frozen: false }
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn tip_hash(&self) -> u32 {
        if self.links.is_empty() {
            0
        } else {
            self.links[self.links.len() - 1].snap
        }
    }

    pub fn push(&mut self, batch_no: usize, payload: &str) -> Result<u32, u32> {
        if self.frozen {
            return Err(EV_FROZEN);
        }
        if self.links.len() >= LINK_ANCHOR {
            return Err(EV_NOT_FORTY);
        }
        for e in self.links.iter() {
            if e.batch_no == batch_no {
                return Err(EV_DUP_SNAP);
            }
        }
        let link = ChainLink::new(batch_no, payload, self.tip_hash());
        let snap = link.snap;
        self.links.push(link);
        Ok(snap)
    }

    /// 链完整性：逐环重算（prev=0 起点或前一环 snap）
    pub fn verify_links(&self) -> Result<(), u32> {
        if self.links.is_empty() {
            return Err(EV_EMPTY_CHAIN);
        }
        let mut prev: u32 = 0;
        let mut i = 0;
        while i < self.links.len() {
            let expect = ChainLink::new(self.links[i].batch_no, &self.links[i].payload, prev);
            if expect.snap != self.links[i].snap {
                return Err(EV_BAD_LINK);
            }
            prev = self.links[i].snap;
            i += 1;
        }
        Ok(())
    }

    /// 终冻结：40 环链完整后根哈希入档
    pub fn freeze(&mut self) -> Result<u32, u32> {
        if self.frozen {
            return Err(EV_SEALED);
        }
        if self.links.len() != LINK_ANCHOR {
            self.frozen = true;
            return Err(EV_NOT_FORTY);
        }
        if self.verify_links().is_err() {
            self.frozen = true;
            return Err(EV_BAD_LINK);
        }
        // 根哈希 = tip 哈希与链长混入
        let mut buf: Vec<u8> = Vec::new();
        let tip = self.tip_hash().to_le_bytes();
        let mut j = 0;
        while j < 4 {
            buf.push(tip[j]);
            j += 1;
        }
        let n = (self.links.len() as u32).to_le_bytes();
        let mut k = 0;
        while k < 4 {
            buf.push(n[k]);
            k += 1;
        }
        let root = fnv1a(&buf);
        self.root = Some(root);
        self.frozen = true;
        Ok(root)
    }

    pub fn root_of(&self) -> Option<u32> {
        self.root
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

pub fn run_unx10393_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10393");
    s.add("J1-段锚", SEG_BASE == 0x1bba0, "哈希链段基址 0x1bba0");
    s.add("J2-链长锚", LINK_ANCHOR == 40, "四十环锚");
    s.add("J3-FNV 基线", fnv1a(b"") == FNV_SEED, "空输入=种子 0x811c9dc5");
    s.add("J4-FNV 已知值", fnv1a(b"a") == 0xe40c_292c, "FNV-1a('a')=0xe40c292c");
    s.add("J5-空链验链", HashChain::new().verify_links() == Err(EV_EMPTY_CHAIN), "空链 EV_EMPTY_CHAIN");
    s.add("J6-链环可推", {
        let mut c = HashChain::new();
        let h1 = c.push(1, "b1").unwrap();
        let h2 = c.push(2, "b2").unwrap();
        h1 != h2
    }, "两环哈希互异（链式混入 prev）");
    s.add("J7-短链验链真", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 10 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        c.verify_links().is_ok()
    }, "十环完整验链通过");
    s.add("J8-同 payload 链不同", {
        let mut c1 = HashChain::new();
        let mut c2 = HashChain::new();
        let a = c1.push(1, "same").unwrap();
        let b = c2.push(1, "same").unwrap();
        a == b
    }, "同起点同 payload 哈希确定一致");
    s.add("J9-重复批号拒收", {
        let mut c = HashChain::new();
        c.push(1, "a").unwrap();
        c.push(1, "b").unwrap_err() == EV_DUP_SNAP
    }, "重复批号 EV_DUP_SNAP");
    s.add("J10-缺环拒冻", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 39 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        c.freeze().unwrap_err() == EV_NOT_FORTY
    }, "39 环 EV_NOT_FORTY");
    s.add("J11-满链冻结", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        c.freeze().is_ok()
    }, "40 环冻结成功");
    s.add("J12-根哈希存档", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        let r = c.freeze().unwrap();
        c.root_of() == Some(r)
    }, "冻结后根哈希可回读");
    s.add("J13-冻结封账", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        c.freeze().unwrap();
        c.push(41, "late").unwrap_err() == EV_FROZEN
    }, "冻结后推环 EV_FROZEN");
    s.add("J14-根哈希确定性", {
        let mut c1 = HashChain::new();
        let mut c2 = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c1.push(n, "p").unwrap();
            c2.push(n, "p").unwrap();
            n += 1;
        }
        c1.freeze().unwrap() == c2.freeze().unwrap()
    }, "同链两次冻结根哈希一致");
    s.add("J15-链长决定根", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        let r1 = c.freeze().unwrap();
        let mut d = HashChain::new();
        n = 1;
        while n <= 40 {
            let p = if n == 40 { "q" } else { "p" };
            d.push(n, p).unwrap();
            n += 1;
        }
        let r2 = d.freeze().unwrap();
        r1 != r2
    }, "payload 变异根哈希变异");
    s.add_dyn("J16-冻结态验链仍真", {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "p").unwrap();
            n += 1;
        }
        c.freeze().unwrap();
        c.verify_links().is_ok() && c.is_frozen()
    }, "冻结后验链仍通过且 frozen 真");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_freeze_root() {
        let mut c = HashChain::new();
        let mut n = 1;
        while n <= 40 {
            c.push(n, "batch").unwrap();
            n += 1;
        }
        let r = c.freeze().unwrap();
        assert_eq!(c.root_of(), Some(r));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10393_checks();
        assert!(s.all_passed(), "UNX-F10393 判据全绿");
    }
}
