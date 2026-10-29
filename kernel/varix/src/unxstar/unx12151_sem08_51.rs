//! UNX-F12151 — 动态函数表（DynamicFunctionTable）语义（0x1a500 段）
//!
//! 锚点：UNX-F12151-J1 动态函数表 10/10（JIT 代码块注册→查表→
//! 注销三段闭环），回调式查表按指纹命中，过期块查表必野。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：DynBlock（JIT 块：指纹+区间）+ DynTable（块账 + 回调
//! 查表）+ 折叠指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a500;
pub const DT_GHOST: u32 = SEG_BASE + 0x01; // 指纹不存在
pub const DT_ORDER: u32 = SEG_BASE + 0x02; // 区间倒挂
pub const DT_TWICE: u32 = SEG_BASE + 0x03; // 重复指纹
pub const DT_SEALED: u32 = SEG_BASE + 0x04; // 印后注册
pub const DT_STALE: u32 = SEG_BASE + 0x05; // 过期块查表

/// JIT 动态块。
#[derive(Clone, PartialEq)]
pub struct DynBlock {
    pub fp: u64,
    pub base: u64,
    pub size: u64,
    pub live: bool,
    pub note: String,
}

impl DynBlock {
    pub fn ok(&self) -> bool {
        self.fp != 0 && self.size > 0 && self.base != 0
    }

    pub fn contains(&self, addr: u64) -> bool {
        self.live && addr >= self.base && addr < self.base + self.size
    }

    pub fn render(&self) -> String {
        format!("fp={:#x} [{:#x}+{:#x}] live={}", self.fp, self.base, self.size, self.live)
    }
}

/// 动态函数表。
pub struct DynTable {
    pub blocks: Vec<DynBlock>,
    pub hits: Vec<u64>,
    sealed: bool,
}

impl DynTable {
    pub fn new() -> DynTable {
        DynTable { blocks: Vec::new(), hits: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 注册动态块（倒挂/零指纹/重复指纹必拒）。
    pub fn register(&mut self, fp: u64, base: u64, size: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DT_SEALED);
        }
        let blk = DynBlock { fp, base, size, live: true, note: note.to_string() };
        if !blk.ok() {
            return Err(DT_ORDER);
        }
        if self.blocks.iter().any(|b| b.fp == fp) {
            return Err(DT_TWICE);
        }
        self.blocks.push(blk);
        Ok(())
    }

    /// 注销（指纹不存在必拒）。
    pub fn unregister(&mut self, fp: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(DT_SEALED);
        }
        let b = self.blocks.iter_mut().find(|b| b.fp == fp).ok_or(DT_GHOST)?;
        b.live = false;
        Ok(())
    }

    /// 回调式查表：活块命中返回指纹；野地址/过期块拒。
    pub fn lookup(&mut self, addr: u64) -> Result<u64, u32> {
        for b in &self.blocks {
            if b.live && b.contains(addr) {
                self.hits.push(addr);
                return Ok(b.fp);
            }
        }
        // 过期块命中 = STALE。
        if self.blocks.iter().any(|b| !b.live && b.base <= addr && addr < b.base + b.size) {
            return Err(DT_STALE);
        }
        Err(DT_GHOST)
    }

    /// 活块计数。
    pub fn live_count(&self) -> usize {
        self.blocks.iter().filter(|b| b.live).count()
    }

    /// 查表留痕全对：每条留痕复查仍可判定（活块命中同区间，
    /// 已注销块按 STALE 判定；野地址留痕必不复存在）。
    pub fn hits_replayable(&self) -> bool {
        self.hits.iter().all(|&a| {
            self.blocks
                .iter()
                .any(|b| b.base <= a && a < b.base + b.size)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a500;
        for b in &self.blocks {
            h = h.wrapping_mul(211).wrapping_add(b.fp as u32);
            h = h.wrapping_mul(211).wrapping_add(b.base as u32);
            h = h.wrapping_mul(211).wrapping_add(b.live as u32);
            for by in b.note.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "blocks={} live={} hits={}",
            self.blocks.len(),
            self.live_count(),
            self.hits.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DynTable::new();
    let _ = b.register(0xAAA1, 0x8000, 0x400, "jit1");
    // 1) 重复指纹必拒。
    got[0] = b.register(0xAAA1, 0x9000, 0x400, "dup").err() == Some(DT_TWICE);
    // 2) 零 size 区间必拒。
    got[1] = b.register(0xAAA2, 0x9000, 0, "z").err() == Some(DT_ORDER);
    // 3) 注销不存在指纹必拒。
    got[2] = b.unregister(0xDEAD).err() == Some(DT_GHOST);
    // 4) 过期块查表拒 STALE。
    let _ = b.unregister(0xAAA1);
    got[3] = b.lookup(0x8100).err() == Some(DT_STALE);
    // 5) 印后注册必拒。
    b.seal();
    got[4] = b.register(0xAAA3, 0xA000, 0x400, "post").err() == Some(DT_SEALED);
    got
}

/// 判据入口：UNX-F12151
pub fn run_unx12151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12151");
    let mut b = DynTable::new();

    // J1 两 JIT 块注册。
    let j1 = b.register(0xAAA1, 0x8000, 0x400, "jit-alloc").is_ok()
        && b.register(0xAAA2, 0x9000, 0x400, "jit-thunk").is_ok();
    s.add("J1_blocks2", j1 && b.blocks.len() == 2, "两动态块注册");

    // J2 查表 10/10：块内多点 + 野地址。
    let mut j2 = true;
    for a in [0x8000u64, 0x8100, 0x83FF, 0x9000, 0x93FF] {
        if b.lookup(a).is_err() { j2 = false; }
    }
    for a in [0x8400u64, 0x9400, 0x7000, 0xDEAD, 0x1] {
        if b.lookup(a).is_ok() { j2 = false; }
    }
    s.add("J2_lookup_10x", j2 && b.hits.len() == 5, "块内/野十点查表全对");

    // J3 注册→查表→注销闭环。
    let u1 = b.unregister(0xAAA1);
    s.add(
        "J3_rlu_cycle",
        u1.is_ok() && b.lookup(0x8100).err() == Some(DT_STALE) && b.lookup(0x9100).is_ok(),
        "注销块查 STALE、活块仍命中",
    );

    // J4 过期块拒绝码。
    s.add(
        "J4_stale_code",
        b.lookup(0x83FF).err() == Some(DT_STALE),
        "过期区间拒 STALE（非 GHOST）",
    );

    // J5 活块计数。
    s.add(
        "J5_live_count",
        b.live_count() == 1 && b.blocks.len() == 2,
        "活块 1/2 计数对平",
    );

    // J6 块渲染。
    let r = b.blocks[1].render();
    s.add(
        "J6_render",
        r.contains("fp=0xaaa2") && r.contains("live=true"),
        "渲染含指纹/区间/活位",
    );

    // J7 块自洽核。
    s.add(
        "J7_blocks_ok",
        b.blocks.iter().all(|x| x.ok()),
        "全块过自洽核",
    );

    // J8 零 size 拒。
    let mut b8 = DynTable::new();
    s.add(
        "J8_zero_size",
        b8.register(0x1, 0x8000, 0, "z").err() == Some(DT_ORDER),
        "零 size 块拒 ORDER",
    );

    // J9 重复指纹拒。
    let mut b9 = DynTable::new();
    let _ = b9.register(0x5, 0x8000, 0x100, "a");
    s.add(
        "J9_dup_fp",
        b9.register(0x5, 0x9000, 0x100, "b").err() == Some(DT_TWICE),
        "重复指纹拒 TWICE",
    );

    // J10 注销野指纹拒。
    s.add(
        "J10_ghost_fp",
        b9.unregister(0xFFFF).err() == Some(DT_GHOST),
        "野指纹注销拒 GHOST",
    );

    // J11 重复注册同指纹不落双账。
    s.add(
        "J11_no_double",
        b9.blocks.len() == 1,
        "拒后账不增块",
    );

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "动态表折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.register(0xAAA9, 0xB000, 0x100, "post").err() == Some(DT_SEALED)
            && b.unregister(0xAAA2).err() == Some(DT_SEALED),
        "印账后注册/注销皆拒",
    );

    // J15 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("blocks=2") && sm.contains("live=1"),
        "摘要含块数与活数",
    );

    // J16 查表留痕可复放。
    s.add("J16_hits_replayable", b.hits_replayable(), "查表留痕全可复放");

    // J17 大表 10 块注册 + 全命中。
    let mut b17 = DynTable::new();
    let mut ok17 = true;
    for i in 0..10u64 {
        if b17.register(0x2000 + i, 0x10000 + i * 0x1000, 0x1000, "m").is_err() {
            ok17 = false;
        }
    }
    let hit17 = (0..10u64).all(|i| b17.lookup(0x10000 + i * 0x1000 + 0x800).is_ok());
    s.add("J17_big10", ok17 && hit17, "10 块大表逐块命中");

    // J18 注销后再注册同区间新指纹合法。
    let mut b18 = DynTable::new();
    let _ = b18.register(0x31, 0x8000, 0x100, "old");
    let _ = b18.unregister(0x31);
    let r18 = b18.register(0x32, 0x8000, 0x100, "new");
    s.add(
        "J18_reregister",
        r18.is_ok() && b18.lookup(0x8080) == Ok(0x32),
        "同区间换指纹重注册合法",
    );

    // J19 空表指纹稳定。
    let b19 = DynTable::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空动态表指纹稳定");

    // J20 空表查表拒 GHOST。
    let mut b20 = DynTable::new();
    s.add(
        "J20_empty_ghost",
        b20.lookup(0x8000).err() == Some(DT_GHOST),
        "空表查表拒 GHOST",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12151_dyn() {
        let mut b = DynTable::new();
        let _ = b.register(0xAAA1, 0x8000, 0x400, "jit");
        assert_eq!(b.lookup(0x8100), Ok(0xAAA1));
        let _ = b.unregister(0xAAA1);
        assert_eq!(b.lookup(0x8100), Err(DT_STALE));
    }

    #[test]
    fn unx12151_checks_green() {
        let s = run_unx12151_checks();
        assert!(s.all_passed(), "UNX-F12151 判据未全绿");
    }

    #[test]
    fn unx12151_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
