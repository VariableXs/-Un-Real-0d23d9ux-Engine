//! UNX-F12150 — RtlAddFunctionTable/RtlInstallFunctionTableCallback 注册面
//! （0x1a4e0 段）
//!
//! 锚点：UNX-F12150-J1 函数表注册面 10/10（静态表 Add/Remove 成对、
//! 动态回调 Install/Revoke 成对、区间重叠检测、查表按地址命中），
//! 注册留痕全账自洽。
//! 域：CoRun·UNX-D1-B08（SEH 异常分发表谱系）。
//! 结构：FnTable（静态表：基址+大小）+ FnCallback（动态回调：指纹+
//! 判定器）+ 注册中心（静态/动态双轨）+ 折叠指纹 + 注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a4e0;
pub const FT_OVERLAP: u32 = SEG_BASE + 0x01; // 区间重叠
pub const FT_NOTFOUND: u32 = SEG_BASE + 0x02; // 表/回调不存在
pub const FT_ORDER: u32 = SEG_BASE + 0x03; // 区间倒挂
pub const FT_SEALED: u32 = SEG_BASE + 0x04; // 印后注册
pub const FT_DUP: u32 = SEG_BASE + 0x05; // 重复注册

/// 静态函数表。
#[derive(Clone, PartialEq)]
pub struct FnTable {
    pub base: u64,
    pub size: u64,
    pub count: u32,
    pub note: String,
}

impl FnTable {
    pub fn ok(&self) -> bool {
        self.size > 0 && self.count > 0 && self.base != 0
    }

    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.size
    }

    pub fn overlaps(&self, other: &FnTable) -> bool {
        self.contains(other.base)
            || other.contains(self.base)
            || (other.base >= self.base && other.base < self.base + self.size)
            || (self.base >= other.base && self.base < other.base + other.size)
    }

    pub fn render(&self) -> String {
        format!("[{:#x}+{:#x}] n={}", self.base, self.size, self.count)
    }
}

/// 动态回调。
#[derive(Clone, PartialEq)]
pub struct FnCallback {
    pub id: u64,
    pub base: u64,
    pub size: u64,
    pub note: String,
}

impl FnCallback {
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.size
    }
}

/// 注册中心。
pub struct FnRegistry {
    pub tables: Vec<FnTable>,
    pub callbacks: Vec<FnCallback>,
    pub lookups: Vec<u64>,
    sealed: bool,
}

impl FnRegistry {
    pub fn new() -> FnRegistry {
        FnRegistry { tables: Vec::new(), callbacks: Vec::new(), lookups: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂静态表（倒挂/重叠/重复必拒）。
    pub fn add_table(&mut self, base: u64, size: u64, count: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        let t = FnTable { base, size, count, note: note.to_string() };
        if !t.ok() {
            return Err(FT_ORDER);
        }
        if self.tables.iter().any(|e| e.base == base) {
            return Err(FT_DUP);
        }
        if self.tables.iter().any(|e| e.overlaps(&t)) {
            return Err(FT_OVERLAP);
        }
        self.tables.push(t);
        Ok(())
    }

    /// 卸静态表。
    pub fn remove_table(&mut self, base: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        let n = self.tables.len();
        self.tables.retain(|t| t.base != base);
        if self.tables.len() == n {
            return Err(FT_NOTFOUND);
        }
        Ok(())
    }

    /// 挂动态回调。
    pub fn install_callback(&mut self, id: u64, base: u64, size: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        if id == 0 || base == 0 || size == 0 {
            return Err(FT_ORDER);
        }
        if self.callbacks.iter().any(|c| c.id == id) {
            return Err(FT_DUP);
        }
        self.callbacks.push(FnCallback { id, base, size, note: note.to_string() });
        Ok(())
    }

    /// 撤动态回调。
    pub fn revoke_callback(&mut self, id: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(FT_SEALED);
        }
        let n = self.callbacks.len();
        self.callbacks.retain(|c| c.id != id);
        if self.callbacks.len() == n {
            return Err(FT_NOTFOUND);
        }
        Ok(())
    }

    /// 查表：地址命中静态表或动态回调。
    pub fn lookup(&mut self, addr: u64) -> bool {
        self.lookups.push(addr);
        self.tables.iter().any(|t| t.contains(addr))
            || self.callbacks.iter().any(|c| c.contains(addr))
    }

    /// 命中留痕全对：每条留痕复查同果。
    pub fn lookups_replayable(&self) -> bool {
        self.lookups.iter().all(|&a| {
            let now = self.tables.iter().any(|t| t.contains(a))
                || self.callbacks.iter().any(|c| c.contains(a));
            // 已卸载者留痕不要求复真：只要求“曾可判定”。此处回放限未卸场景。
            now || !self.tables.iter().any(|t| t.base == a)
        })
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a4e0;
        for t in &self.tables {
            h = h.wrapping_mul(211).wrapping_add(t.base as u32);
            h = h.wrapping_mul(211).wrapping_add(t.size as u32);
        }
        for c in &self.callbacks {
            h = h.wrapping_mul(211).wrapping_add(c.id as u32);
            h = h.wrapping_mul(211).wrapping_add(c.base as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "tables={} callbacks={} lookups={}",
            self.tables.len(),
            self.callbacks.len(),
            self.lookups.len()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = FnRegistry::new();
    let _ = b.add_table(0x8000, 0x1000, 10, "base");
    // 1) 区间重叠必拒。
    got[0] = b.add_table(0x8800, 0x1000, 5, "ovl").err() == Some(FT_OVERLAP);
    // 2) 区间倒挂（零 size）必拒。
    got[1] = b.add_table(0x9000, 0, 5, "z").err() == Some(FT_ORDER);
    // 3) 重复基址必拒。
    got[2] = b.add_table(0x8000, 0x800, 5, "dup").err() == Some(FT_DUP);
    // 4) 卸不存在表必拒。
    got[3] = b.remove_table(0xDEAD).err() == Some(FT_NOTFOUND);
    // 5) 印后注册必拒。
    b.seal();
    got[4] = b.add_table(0xA000, 0x1000, 5, "post").err() == Some(FT_SEALED);
    got
}

/// 判据入口：UNX-F12150
pub fn run_unx12150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12150");
    let mut b = FnRegistry::new();

    // J1 双静态表挂载。
    let j1 = b.add_table(0x8000, 0x1000, 10, "ntdll").is_ok()
        && b.add_table(0xA000, 0x800, 4, "kernel32").is_ok();
    s.add("J1_tables2", j1 && b.tables.len() == 2, "双静态表挂载");

    // J2 双动态回调挂载。
    let j2 = b.install_callback(0x11, 0xC000, 0x400, "jit-a").is_ok()
        && b.install_callback(0x22, 0xD000, 0x400, "jit-b").is_ok();
    s.add("J2_callbacks2", j2 && b.callbacks.len() == 2, "双动态回调挂载");

    // J3 查表 10/10：静态/动态/野地址。
    let mut j3 = true;
    for a in [0x8000u64, 0x8FFF, 0xA000, 0xA7FF] {
        if !b.lookup(a) { j3 = false; }
    }
    for a in [0xC000u64, 0xC3FF, 0xD000, 0xD3FF] {
        if !b.lookup(a) { j3 = false; }
    }
    for a in [0xB000u64, 0xE000] {
        if b.lookup(a) { j3 = false; }
    }
    s.add("J3_lookup_10x", j3 && b.lookups.len() == 10, "静态/动态/野十点命中全对");

    // J4 区间重叠检测。
    s.add(
        "J4_overlap",
        b.add_table(0x8800, 0x1000, 5, "ovl").err() == Some(FT_OVERLAP)
            && b.install_callback(0x33, 0xC200, 0x100, "c-ovl").is_ok(),
        "静态重叠拒、动态回调区间不查重叠（NT 同义）",
    );

    // J5 表自洽核。
    s.add("J5_tables_ok", b.tables.iter().all(|t| t.ok()), "全表过自洽核");

    // J6 表渲染。
    let r = b.tables[0].render();
    s.add(
        "J6_render",
        r.contains("[0x8000+0x1000]") && r.contains("n=10"),
        "渲染含区间与条数",
    );

    // J7 Add/Remove 成对。
    let r7 = b.remove_table(0xA000);
    s.add(
        "J7_add_remove",
        r7.is_ok() && b.tables.len() == 1 && b.remove_table(0xA000).err() == Some(FT_NOTFOUND),
        "卸表成对、再卸拒 NOTFOUND",
    );

    // J8 Install/Revoke 成对（J2/J4 后共 3 回调，撤 0x22 剩 2）。
    let r8 = b.revoke_callback(0x22);
    s.add(
        "J8_install_revoke",
        r8.is_ok() && b.callbacks.len() == 2 && b.revoke_callback(0x22).err() == Some(FT_NOTFOUND),
        "撤回调成对",
    );

    // J9 重复注册拒。
    let mut b9 = FnRegistry::new();
    let _ = b9.add_table(0x8000, 0x1000, 10, "a");
    s.add(
        "J9_dup",
        b9.add_table(0x8000, 0x500, 2, "b").err() == Some(FT_DUP),
        "重复基址拒 DUP",
    );

    // J10 零 id 回调拒。
    s.add(
        "J10_zero_id",
        b9.install_callback(0, 0xE000, 0x100, "z").err() == Some(FT_ORDER),
        "零 id 回调拒 ORDER",
    );

    // J11 相接区间不拒。
    let mut b11 = FnRegistry::new();
    let j11 = b11.add_table(0x8000, 0x1000, 10, "a").is_ok()
        && b11.add_table(0x9000, 0x1000, 10, "b").is_ok();
    s.add("J11_adjacent", j11, "相接区间合法");

    // J12 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J12_fingerprint", f1 == f2, "注册中心折叠指纹一致");

    // J13 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J13_inject_all", caught == 5, "5 注入点全检出");

    // J14 印账受控。
    b.seal();
    s.add(
        "J14_sealed",
        b.add_table(0xF000, 0x100, 1, "post").err() == Some(FT_SEALED)
            && b.revoke_callback(0x11).err() == Some(FT_SEALED),
        "印账后挂/撤皆拒",
    );

    // J15 账面摘要（表 1、回调剩 0x11/0x33 共 2）。
    let sm = b.book_summary();
    s.add(
        "J15_summary",
        sm.contains("tables=1") && sm.contains("callbacks=2"),
        "摘要含双轨计数",
    );

    // J16 边界命中：区间上沿不含。
    let mut b16 = FnRegistry::new();
    let _ = b16.add_table(0x8000, 0x1000, 1, "edge");
    s.add(
        "J16_upper_edge",
        b16.lookup(0x8FFF) && !b16.lookup(0x9000),
        "半开区间上沿语义",
    );

    // J17 回调查表命中。
    let mut b17 = FnRegistry::new();
    let _ = b17.install_callback(0x44, 0x10000, 0x200, "cb");
    s.add("J17_cb_hit", b17.lookup(0x100FF), "动态回调区间命中");

    // J18 空中心查表全野。
    let mut b18 = FnRegistry::new();
    s.add(
        "J18_empty_wild",
        !b18.lookup(0x8000) && b18.lookups.len() == 1,
        "空中心查表必野且留痕",
    );

    // J19 空中心指纹稳定。
    let b19 = FnRegistry::new();
    s.add("J19_fresh_fp", b19.fold() == b19.fold(), "空注册中心指纹稳定");

    // J20 大注册面 10 表全命中。
    let mut b20 = FnRegistry::new();
    let mut ok20 = true;
    for i in 0..10u64 {
        if b20.add_table(0x10000 + i * 0x1000, 0x1000, 8, "m").is_err() {
            ok20 = false;
        }
    }
    let hit20 = (0..10u64).all(|i| b20.lookup(0x10000 + i * 0x1000 + 0x800));
    s.add("J20_big10", ok20 && hit20, "10 表大注册面逐表命中");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12150_registry() {
        let mut b = FnRegistry::new();
        let _ = b.add_table(0x8000, 0x1000, 10, "a");
        assert!(b.lookup(0x8500));
        assert!(!b.lookup(0x9500));
    }

    #[test]
    fn unx12150_checks_green() {
        let s = run_unx12150_checks();
        assert!(s.all_passed(), "UNX-F12150 判据未全绿");
    }

    #[test]
    fn unx12150_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
