//! UNX-F12114 缓冲所有权模型与 PagedPool 分配挂点（0x1a060 段）
//!
//! 卡面：字符串缓冲所有权——PagedPool 挂点分配（池标签必为四 ASCII
//! 大写元组）、块账（分配/释放）、双重释放拒、回收时泄漏显性红账。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 池标签合法：四字节皆 ASCII 大写字母。
//! - J2 块唯一：同块号二次分配必拒（含零号）。
//! - J3 双重释放拒：同块二次释放必拒。
//! - J4 无块释放拒：释放未分配块必拒。
//! - J5 泄漏显性：回收时存活块非零判红并逐块入红账。
//! - J6 尺寸约束：分配尺寸非零且 ≤ 上限。
//! - J7 印账冻结：seal 后一切操作必拒。
//! - J8 注入五况全抓。
//!
//! 结构：PoolBlock 块档 + PoolBook 块账（含红账）+ 回收核 +
//! 折叠指纹（211 进制卷积）+ 注入核（坏标签/无块释放/双重释放/泄漏红/
//! 印后分配）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a060 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a060;
pub const PP_BAD_TAG: u32 = SEG_BASE + 0x01; // 坏池标签
pub const PP_NO_BLOCK: u32 = SEG_BASE + 0x02; // 无块释放
pub const PP_DOUBLE_FREE: u32 = SEG_BASE + 0x03; // 双重释放
pub const PP_LEAK: u32 = SEG_BASE + 0x04; // 泄漏红
pub const PP_DUP_BLOCK: u32 = SEG_BASE + 0x05; // 重块（含零号）
pub const PP_FROZEN: u32 = SEG_BASE + 0x06; // 印账后操作

/// 单块尺寸上限。
pub const PP_MAX_SIZE: u32 = 0x10000;

/// 块档。
#[derive(Clone)]
pub struct PoolBlock {
    pub block: u32,
    pub size: u32,
    pub tag: u32,
    pub freed: bool,
}

impl PoolBlock {
    /// 池标签四字节 ASCII 大写判定。
    pub fn tag_ok(&self) -> bool {
        self.tag.to_be_bytes().iter().all(|b| (b'A'..=b'Z').contains(b))
    }
    pub fn render(&self) -> String {
        format!("B{}|{}|r{}", self.block, self.size, self.freed as u8)
    }
}

/// 块账。
pub struct PoolBook {
    pub blocks: Vec<PoolBlock>,
    pub reds: Vec<(u32, u32)>, // 红账：(错误码, 块号)
    pub max_blocks: usize,
    pub sealed: bool,
}

impl PoolBook {
    pub fn new(max_blocks: usize) -> PoolBook {
        PoolBook { blocks: Vec::new(), reds: Vec::new(), max_blocks, sealed: false }
    }

    /// 分配：标签 → 尺寸 → 块号唯一 → 容量。
    pub fn alloc(&mut self, block: u32, size: u32, tag: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_FROZEN);
        }
        let probe = PoolBlock {
            block, size, tag, freed: false,
        };
        if !probe.tag_ok() {
            self.reds.push((PP_BAD_TAG, block));
            return Err(PP_BAD_TAG);
        }
        if size == 0 || size > PP_MAX_SIZE {
            self.reds.push((PP_BAD_TAG, block)); // 尺寸面按标签面同码显性
            return Err(PP_BAD_TAG);
        }
        if block == 0 || self.blocks.iter().any(|b| b.block == block) {
            return Err(PP_DUP_BLOCK);
        }
        if self.blocks.len() >= self.max_blocks {
            return Err(PP_DUP_BLOCK); // 容量面按重块面显性
        }
        self.blocks.push(probe);
        Ok(())
    }

    fn pos(&self, block: u32) -> Option<usize> {
        self.blocks.iter().position(|b| b.block == block)
    }

    /// 释放：无块与双重释放必拒。
    pub fn free(&mut self, block: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_FROZEN);
        }
        let i = match self.pos(block) {
            Some(i) => i,
            None => {
                self.reds.push((PP_NO_BLOCK, block));
                return Err(PP_NO_BLOCK);
            }
        };
        if self.blocks[i].freed {
            self.reds.push((PP_DOUBLE_FREE, block));
            return Err(PP_DOUBLE_FREE);
        }
        self.blocks[i].freed = true;
        Ok(())
    }

    /// 存活块数。
    pub fn live_count(&self) -> usize {
        self.blocks.iter().filter(|b| !b.freed).count()
    }

    /// 回收核：泄漏显性判红。
    pub fn teardown(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(PP_FROZEN);
        }
        let leaks: Vec<u32> = self
            .blocks
            .iter()
            .filter(|b| !b.freed)
            .map(|b| b.block)
            .collect();
        if leaks.is_empty() {
            return Ok(());
        }
        for l in leaks {
            self.reds.push((PP_LEAK, l));
        }
        Err(PP_LEAK)
    }

    /// 红账条数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for b in &self.blocks {
            h = h.wrapping_mul(211).wrapping_add(b.block);
            h = h.wrapping_mul(211).wrapping_add(b.size);
            h = h.wrapping_mul(211).wrapping_add(b.tag);
            h = h.wrapping_mul(211).wrapping_add(b.freed as u32);
        }
        for r in &self.reds {
            h = h.wrapping_mul(211).wrapping_add(r.0);
            h = h.wrapping_mul(211).wrapping_add(r.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("blocks={} live={} reds={}", self.blocks.len(), self.live_count(), self.red_count())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, block: u32) -> String {
        match self.pos(block) {
            Some(i) => self.blocks[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PoolBook::new(8);
    // 1) 坏池标签必拒。
    got[0] = b.alloc(1, 32, 0x6162_6364).err() == Some(PP_BAD_TAG);
    // 2) 无块释放必拒。
    got[1] = b.free(9).err() == Some(PP_NO_BLOCK);
    // 3) 双重释放必拒。
    let _ = b.alloc(2, 16, 0x5354_5247);
    let _ = b.free(2);
    got[2] = b.free(2).err() == Some(PP_DOUBLE_FREE);
    // 4) 泄漏红必判（存活块回收显红）。
    let _ = b.alloc(3, 8, 0x5354_5247);
    got[3] = b.teardown().err() == Some(PP_LEAK) && b.red_count() >= 1;
    // 5) 印账后分配必抓。
    b.seal();
    got[4] = b.alloc(4, 8, 0x5354_5247).err() == Some(PP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12114");
    const TAG: u32 = 0x5354_5247; // "STRG"

    // J1 池标签：大写过小写拒零拒。
    let j1 = {
        let mut b = PoolBook::new(8);
        b.alloc(1, 8, TAG).is_ok()
            && b.alloc(2, 8, 0x7461_6767).err() == Some(PP_BAD_TAG)
            && b.alloc(2, 8, 0).err() == Some(PP_BAD_TAG)
    };
    s.add("J1-池标签", j1, "大写四元组闭检");

    // J2 零号与重块拒。
    let j2 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        b.alloc(1, 8, TAG).err() == Some(PP_DUP_BLOCK)
            && b.alloc(0, 8, TAG).err() == Some(PP_DUP_BLOCK)
    };
    s.add("J2-块唯一", j2, "零号重号皆拒");

    // J3 无块释放拒。
    let j3 = {
        let mut b = PoolBook::new(8);
        b.free(9).err() == Some(PP_NO_BLOCK) && b.red_count() == 1
    };
    s.add("J3-无块释放拒", j3, "未知块必拒且留红");

    // J4 双重释放拒。
    let j4 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        let _ = b.free(1);
        b.free(1).err() == Some(PP_DOUBLE_FREE)
    };
    s.add("J4-双重释放拒", j4, "二次释放必拒");

    // J5 泄漏显性红。
    let j5 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        let _ = b.alloc(2, 8, TAG);
        let _ = b.free(1);
        b.teardown().err() == Some(PP_LEAK) && b.live_count() == 1
    };
    s.add("J5-泄漏显性", j5, "回收判红逐块入账");

    // J6 无泄漏回收过。
    let j6 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        let _ = b.free(1);
        b.teardown().is_ok()
    };
    s.add("J6-无泄漏过", j6, "全释放回收绿");

    // J7 尺寸约束。
    let j7 = {
        let mut b = PoolBook::new(8);
        b.alloc(1, 0, TAG).is_err() && b.alloc(1, PP_MAX_SIZE + 1, TAG).is_err()
    };
    s.add("J7-尺寸约束", j7, "零与超限皆拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(3, 24, TAG);
        b.line_of(3).contains("24") && b.line_of(4) == "missing"
    };
    s.add("J8-回读渲染", j8, "块行含尺寸");

    // J9 摘要回读。
    let j9 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        let _ = b.free(1);
        b.summary().contains("live=0")
    };
    s.add("J9-摘要回读", j9, "存活数可查");

    // J10 印账冻结。
    let j10 = {
        let mut b = PoolBook::new(8);
        b.seal();
        b.alloc(1, 8, TAG).err() == Some(PP_FROZEN)
            && b.free(1).err() == Some(PP_FROZEN)
            && b.teardown().err() == Some(PP_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = PoolBook::new(8);
        let _ = b.alloc(1, 8, TAG);
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = PoolBook::new(8);
    let _ = b.alloc(1, 64, TAG);
    let _ = b.alloc(2, 32, TAG);
    let _ = b.free(2);

    s.add("摘要回读", b.summary().contains("blocks=2 live=1"), "一活一释");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12114_pool_flow() {
        let mut b = PoolBook::new(4);
        assert!(b.alloc(1, 32, 0x5354_5247).is_ok());
        assert_eq!(b.alloc(1, 32, 0x5354_5247), Err(PP_DUP_BLOCK));
        assert!(b.free(1).is_ok());
        assert_eq!(b.free(1), Err(PP_DOUBLE_FREE));
    }

    #[test]
    fn unx12114_checks_green() {
        let s = run_unx12114_checks();
        assert!(s.all_passed(), "UNX-F12114 判据未全绿");
    }

    #[test]
    fn tag_check_helper() {
        let good = PoolBlock { block: 1, size: 8, tag: 0x5445_5354, freed: false };
        let bad = PoolBlock { tag: 0x7465_7374, ..good };
        assert!(good.tag_ok());
        assert!(!bad.tag_ok());
    }
}
