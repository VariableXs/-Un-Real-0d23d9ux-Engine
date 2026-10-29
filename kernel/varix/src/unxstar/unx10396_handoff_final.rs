//! UNX-F10396 handoff 终写（C3 块 finalized=40 终态）（0x1bc00 段）
//! 锚点：UNX-F10396-J1 handoff 终写（C3 块 finalized 字段 40 终态+校验链）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bc00;
pub const EV_BAD_FINAL: u32 = SEG_BASE + 1;
pub const EV_TWICE: u32 = SEG_BASE + 2;
pub const EV_SEALED: u32 = SEG_BASE + 3;
pub const EV_NO_BLOCK: u32 = SEG_BASE + 4;
pub const EV_BAD_FIELD: u32 = SEG_BASE + 5;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 6;

pub const FINAL_ANCHOR: u16 = 40;

/// C3 块：域标识 + finalized 字段 + 批清单
#[derive(Clone, Debug)]
pub struct C3Block {
    pub domain: String,
    pub finalized: u16,
    pub batches: Vec<u16>,
}

impl C3Block {
    pub fn new(domain: &str) -> Self {
        C3Block { domain: domain.to_string(), finalized: 0, batches: Vec::new() }
    }
    pub fn add_batch(&mut self, no: u16) -> Result<(), u32> {
        if no == 0 || no > FINAL_ANCHOR {
            return Err(EV_RANGE_OUT);
        }
        for b in self.batches.iter() {
            if *b == no {
                return Err(EV_TWICE);
            }
        }
        self.batches.push(no);
        Ok(())
    }
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }
    pub fn complete(&self) -> bool {
        self.finalized == FINAL_ANCHOR && self.batches.len() == FINAL_ANCHOR as usize
    }
}

/// handoff 终写器：C3 块终态写入 + 一致性校验
pub struct HandoffFinal {
    block: Option<C3Block>,
    written: bool,
    checksum: Option<u32>,
}

const FNV_SEED: u32 = 0x811c_9dc5;

impl HandoffFinal {
    pub fn new() -> Self {
        HandoffFinal { block: None, written: false, checksum: None }
    }

    pub fn load(&mut self, block: C3Block) -> Result<(), u32> {
        if self.written {
            return Err(EV_TWICE);
        }
        if self.block.is_some() {
            return Err(EV_NO_BLOCK);
        }
        if block.finalized > FINAL_ANCHOR {
            return Err(EV_BAD_FINAL);
        }
        self.block = Some(block);
        Ok(())
    }

    pub fn block_mut(&mut self) -> Option<&mut C3Block> {
        if self.written {
            return None;
        }
        self.block.as_mut()
    }

    pub fn is_written(&self) -> bool {
        self.written
    }

    /// 终写：finalized=40 且批清单齐备才可写，写入即封账
    pub fn finalize_write(&mut self) -> Result<u32, u32> {
        if self.written {
            return Err(EV_SEALED);
        }
        let b = match &self.block {
            None => return Err(EV_NO_BLOCK),
            Some(b) => b,
        };
        if b.finalized != FINAL_ANCHOR {
            self.written = true;
            return Err(EV_BAD_FINAL);
        }
        if b.batch_count() != FINAL_ANCHOR as usize {
            self.written = true;
            return Err(EV_BAD_FIELD);
        }
        // 校验和：域名字节 + finalized + 逐批号
        let mut h: u32 = FNV_SEED;
        let db = b.domain.as_bytes();
        let mut i = 0;
        while i < db.len() {
            h ^= db[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        h ^= b.finalized as u32;
        h = h.wrapping_mul(0x0100_0193);
        let mut i = 0;
        while i < b.batches.len() {
            h ^= b.batches[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        self.checksum = Some(h);
        self.written = true;
        Ok(h)
    }

    pub fn checksum_of(&self) -> Option<u32> {
        self.checksum
    }
}

/// 满批 C3 构造
fn full_block(domain: &str) -> C3Block {
    let mut b = C3Block::new(domain);
    let mut n = 1;
    while n <= FINAL_ANCHOR {
        b.add_batch(n).unwrap();
        n += 1;
    }
    b.finalized = FINAL_ANCHOR;
    b
}

pub fn run_unx10396_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10396");
    s.add("J1-段锚", SEG_BASE == 0x1bc00, "handoff 段基址 0x1bc00");
    s.add("J2-终态锚", FINAL_ANCHOR == 40, "finalized=40 锚");
    s.add("J3-新块零态", {
        let b = C3Block::new("C3");
        b.finalized == 0 && b.batch_count() == 0 && !b.complete()
    }, "新块 0/40");
    s.add("J4-批号越界", {
        let mut b = C3Block::new("C3");
        b.add_batch(0).unwrap_err() == EV_RANGE_OUT && b.add_batch(41).unwrap_err() == EV_RANGE_OUT
    }, "批 0/41 拒收");
    s.add("J5-重复批拒收", {
        let mut b = C3Block::new("C3");
        b.add_batch(7).unwrap();
        b.add_batch(7).unwrap_err() == EV_TWICE
    }, "重复批 EV_TWICE");
    s.add("J6-满块齐备", {
        let b = full_block("C3");
        b.complete() && b.batch_count() == 40
    }, "满块 complete 且 40 批");
    s.add("J7-终写成功", {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        hf.finalize_write().is_ok()
    }, "满块终写通过");
    s.add("J8-校验和存档", {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        let c = hf.finalize_write().unwrap();
        hf.checksum_of() == Some(c)
    }, "校验和可回读");
    s.add("J9-校验和确定", {
        let mut a = HandoffFinal::new();
        let mut b = HandoffFinal::new();
        a.load(full_block("C3")).unwrap();
        b.load(full_block("C3")).unwrap();
        a.finalize_write().unwrap() == b.finalize_write().unwrap()
    }, "同块两次终写同和");
    s.add("J10-域异和异", {
        let mut a = HandoffFinal::new();
        let mut b = HandoffFinal::new();
        a.load(full_block("C3")).unwrap();
        b.load(full_block("C4")).unwrap();
        a.finalize_write().unwrap() != b.finalize_write().unwrap()
    }, "域标识入校验和");
    s.add("J11-未满态拒写", {
        let mut hf = HandoffFinal::new();
        hf.load(C3Block::new("C3")).unwrap();
        hf.finalize_write().unwrap_err() == EV_BAD_FINAL
    }, "finalized=0 EV_BAD_FINAL");
    s.add("J12-空块拒载", {
        let mut hf = HandoffFinal::new();
        hf.finalize_write().unwrap_err() == EV_NO_BLOCK
    }, "无块终写 EV_NO_BLOCK");
    s.add("J13-二次载块拒", {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        hf.load(full_block("C3")).unwrap_err() == EV_NO_BLOCK
    }, "已载块再载 EV_NO_BLOCK");
    s.add("J14-终写封账", {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        hf.finalize_write().unwrap();
        hf.finalize_write().unwrap_err() == EV_SEALED
    }, "终写后再写 EV_SEALED");
    s.add("J15-终写后块冻结", {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        hf.finalize_write().unwrap();
        hf.block_mut().is_none()
    }, "终写后 block_mut None");
    s.add("J16-finalized 溢值拒载", {
        let mut b = full_block("C3");
        b.finalized = 41;
        let mut hf = HandoffFinal::new();
        hf.load(b).unwrap_err() == EV_BAD_FINAL
    }, "finalized=41 EV_BAD_FINAL");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_handoff_write() {
        let mut hf = HandoffFinal::new();
        hf.load(full_block("C3")).unwrap();
        let c = hf.finalize_write().unwrap();
        assert_eq!(hf.checksum_of(), Some(c));
        assert!(hf.is_written());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10396_checks();
        assert!(s.all_passed(), "UNX-F10396 判据全绿");
    }
}
