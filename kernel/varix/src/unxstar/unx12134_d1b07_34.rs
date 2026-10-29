//! UNX-F12134 — 真实编译产物样本库制度（MSVC/MinGW 各钉一版）（0x1a2e0 段）
//!
//! 锚点：UNX-F12134-J1 样本库双族钉版（每族钉一版编译产物，版本号唯一锁定，混族/漂版必拒）。
//! 域：CoRun·UNX-D1-B07（.pdata 展开表面谱系）。
//! 结构：样本库（条目 = 族/钉版/函数名/RVA/长度/pdata 存在位）+ 族钉版锁（每族唯一版号）+
//! 入册受控（混族/漂版/重名三拒）+ 折叠指纹 + 注入核（漂版/混族/重名/零长样本/印后入册）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a2e0;
pub const SB_DRIFT: u32 = SEG_BASE + 0x01; // 钉版漂移
pub const SB_MIXED: u32 = SEG_BASE + 0x02; // 混族样本
pub const SB_DUP: u32 = SEG_BASE + 0x03; // 重名样本
pub const SB_ZERO: u32 = SEG_BASE + 0x04; // 零长样本
pub const SB_FROZEN: u32 = SEG_BASE + 0x05; // 印后入册

/// 编译器族闭集。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Toolchain {
    Msvc,
    MinGw,
}

impl Toolchain {
    /// 族名渲染。
    pub fn name(&self) -> &'static str {
        match self {
            Toolchain::Msvc => "MSVC",
            Toolchain::MinGw => "MinGW",
        }
    }
}

/// 族钉版：每族钉一版编译产物。
#[derive(Clone)]
pub struct PinnedVersion {
    pub chain: Toolchain,
    /// 钉版号（族内唯一，如 1938 / 13.2.0 折算整数）。
    pub ver: u32,
}

/// 一个样本条目：真实编译产物中的一个函数。
#[derive(Clone)]
pub struct SampleEntry {
    pub seq: u32,
    pub chain: Toolchain,
    pub func: String,
    /// 相对虚拟地址。
    pub rva: u32,
    /// 函数字节长度。
    pub len: u32,
    /// .pdata 条目存在位（叶函数为 false，联动 12136 卡）。
    pub has_pdata: bool,
}

impl SampleEntry {
    /// 条目合法性：名非空、长非零、RVA 对齐 4。
    pub fn ok(&self) -> bool {
        !self.func.is_empty() && self.len > 0 && self.rva % 4 == 0
    }

    pub fn render_line(&self) -> String {
        format!(
            "{}:{}+{:#x}({}B) pdata={}",
            self.chain.name(),
            self.func,
            self.rva,
            self.len,
            self.has_pdata
        )
    }
}

/// 样本库（双族钉版制度）。
pub struct SampleLibrary {
    pub pins: Vec<PinnedVersion>,
    pub entries: Vec<SampleEntry>,
    pub seq_next: u32,
    sealed: bool,
}

impl SampleLibrary {
    pub fn new() -> SampleLibrary {
        SampleLibrary { pins: Vec::new(), entries: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 族钉版（每族唯一：重复钉版拒、改钉漂版拒）。
    pub fn pin(&mut self, chain: Toolchain, ver: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SB_FROZEN);
        }
        if ver == 0 {
            return Err(SB_DRIFT);
        }
        if let Some(p) = self.pins.iter().find(|p| p.chain == chain) {
            if p.ver != ver {
                return Err(SB_DRIFT);
            }
            return Ok(());
        }
        self.pins.push(PinnedVersion { chain, ver });
        Ok(())
    }

    /// 族钉版读取。
    pub fn pin_of(&self, chain: Toolchain) -> Option<u32> {
        self.pins.iter().find(|p| p.chain == chain).map(|p| p.ver)
    }

    /// 入册：未钉版/重名/零长必拒。
    pub fn put(
        &mut self,
        chain: Toolchain,
        func: &str,
        rva: u32,
        len: u32,
        has_pdata: bool,
    ) -> Result<u32, u32> {
        if self.sealed {
            return Err(SB_FROZEN);
        }
        if func.is_empty() {
            return Err(SB_DUP);
        }
        if len == 0 {
            return Err(SB_ZERO);
        }
        if self.pin_of(chain).is_none() {
            return Err(SB_MIXED);
        }
        if self
            .entries
            .iter()
            .any(|e| e.chain == chain && e.func == func)
        {
            return Err(SB_DUP);
        }
        if self
            .entries
            .iter()
            .any(|e| e.rva < rva + len && rva < e.rva + e.len)
        {
            return Err(SB_ZERO);
        }
        let seq = self.seq_next;
        self.entries.push(SampleEntry {
            seq,
            chain,
            func: func.to_string(),
            rva,
            len,
            has_pdata,
        });
        self.seq_next += 1;
        Ok(seq)
    }

    /// 按族清点。
    pub fn count_of(&self, chain: Toolchain) -> usize {
        self.entries.iter().filter(|e| e.chain == chain).count()
    }

    /// 有 pdata 条目的样本数。
    pub fn pdata_count(&self) -> usize {
        self.entries.iter().filter(|e| e.has_pdata).count()
    }

    /// 叶函数数（无 .pdata 条目，联动 12136 卡）。
    pub fn leaf_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.has_pdata).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a2e0;
        for p in &self.pins {
            h = h.wrapping_mul(211).wrapping_add(p.ver);
        }
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.rva);
            h = h.wrapping_mul(211).wrapping_add(e.len);
            for by in e.func.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 全条目自洽复核。
    pub fn all_entries_ok(&self) -> bool {
        self.entries.iter().all(|e| e.ok())
    }

    /// 双族齐备判定。
    pub fn both_pinned(&self) -> bool {
        self.pins.len() == 2
    }

    /// 按名查样本。
    pub fn find(&self, chain: Toolchain, func: &str) -> Option<&SampleEntry> {
        self.entries
            .iter()
            .find(|e| e.chain == chain && e.func == func)
    }

    /// 账面摘要。
    pub fn book_summary(&self) -> String {
        format!(
            "pins={} entries={} pdata={} leaf={}",
            self.pins.len(),
            self.entries.len(),
            self.pdata_count(),
            self.leaf_count()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut lib = SampleLibrary::new();
    let _ = lib.pin(Toolchain::Msvc, 1938);
    // 1) 未钉版族入册必拒（混族）。
    got[0] = lib.put(Toolchain::MinGw, "inj_a", 0x1000, 16, true).err() == Some(SB_MIXED);
    // 2) 钉版漂移必拒。
    got[1] = lib.pin(Toolchain::Msvc, 1939).err() == Some(SB_DRIFT);
    // 3) 重名样本必拒。
    let _ = lib.put(Toolchain::Msvc, "dup_fn", 0x1000, 16, true);
    got[2] = lib.put(Toolchain::Msvc, "dup_fn", 0x2000, 16, true).err() == Some(SB_DUP);
    // 4) 零长样本必拒。
    got[3] = lib.put(Toolchain::Msvc, "zero_fn", 0x3000, 0, true).err() == Some(SB_ZERO);
    // 5) 印后入册必拒。
    lib.seal();
    got[4] = lib.put(Toolchain::Msvc, "post_fn", 0x4000, 16, true).err() == Some(SB_FROZEN);
    got
}

/// 判据入口：UNX-F12134
pub fn run_unx12134_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12134");
    let mut lib = SampleLibrary::new();

    // J1 双族钉版。
    let j1 = lib.pin(Toolchain::Msvc, 1938).is_ok() && lib.pin(Toolchain::MinGw, 1302).is_ok();
    s.add("J1_pin_both", j1 && lib.both_pinned(), "MSVC/MinGW 各钉一版");

    // J2 钉版幂等：同版重钉放行。
    s.add("J2_pin_idem", lib.pin(Toolchain::Msvc, 1938) == Ok(()), "同版重钉幂等放行");

    // J3 双族入册 10/10。
    let mut j3 = true;
    for i in 0..5usize {
        let rva = 0x1000 + (i as u32) * 0x40;
        if lib
            .put(Toolchain::Msvc, &format!("msvc_fn{}", i), rva, 32, true)
            .is_err()
        {
            j3 = false;
        }
        if lib
            .put(Toolchain::MinGw, &format!("mingw_fn{}", i), rva + 0x20, 24, true)
            .is_err()
        {
            j3 = false;
        }
    }
    s.add("J3_put_10x", j3 && lib.entries.len() == 10, "双族十样本入册");

    // J4 族清点对平。
    s.add(
        "J4_count",
        lib.count_of(Toolchain::Msvc) == 5 && lib.count_of(Toolchain::MinGw) == 5,
        "双族各 5 样本清点对平",
    );

    // J5 全条目自洽。
    s.add("J5_entries_ok", lib.all_entries_ok(), "十样本过合法性核");

    // J6 序号单调。
    let seqs: Vec<u32> = lib.entries.iter().map(|e| e.seq).collect();
    let mono = seqs.windows(2).all(|w| w[1] > w[0]);
    s.add("J6_seq_mono", mono && seqs.first() == Some(&1), "入册序号单调且首为 1");

    // J7 钉版漂移受控。
    s.add(
        "J7_drift",
        lib.pin(Toolchain::Msvc, 2000).err() == Some(SB_DRIFT),
        "改钉版号拒 DRIFT",
    );

    // J8 混族受控。
    let mut lib2 = SampleLibrary::new();
    let _ = lib2.pin(Toolchain::MinGw, 1302);
    s.add(
        "J8_mixed",
        lib2.put(Toolchain::Msvc, "x", 0x1000, 16, true).err() == Some(SB_MIXED),
        "未钉版族入册拒 MIXED",
    );

    // J9 重名受控。
    let _ = lib2.put(Toolchain::MinGw, "same", 0x1000, 16, true);
    s.add(
        "J9_dup",
        lib2.put(Toolchain::MinGw, "same", 0x2000, 16, true).err() == Some(SB_DUP),
        "族内重名拒 DUP",
    );

    // J10 跨族同名放行（重名以族为界）。
    let _ = lib2.pin(Toolchain::Msvc, 1938);
    s.add(
        "J10_cross_name_ok",
        lib2.put(Toolchain::Msvc, "same", 0x8000, 16, true).is_ok(),
        "跨族同名不判重",
    );

    // J11 RVA 重叠受控。
    s.add(
        "J11_overlap",
        lib2.put(Toolchain::Msvc, "ovl", 0x8004, 16, true).err() == Some(SB_ZERO),
        "RVA 区间重叠同码受控",
    );

    // J12 零长受控。
    s.add(
        "J12_zero",
        lib2.put(Toolchain::Msvc, "z", 0x9000, 0, true).err() == Some(SB_ZERO),
        "零长样本拒 ZERO",
    );

    // J13 指纹稳定。
    let f1 = lib.fold();
    let f2 = lib.fold();
    s.add("J13_fingerprint", f1 == f2, "样本库折叠指纹一致");

    // J14 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J14_inject_all", caught == 5, "5 注入点全检出");

    // J15 印账受控。
    let mut lib3 = SampleLibrary::new();
    let _ = lib3.pin(Toolchain::Msvc, 1938);
    lib3.seal();
    let j15 = lib3.pin(Toolchain::MinGw, 1302).err() == Some(SB_FROZEN)
        && lib3.put(Toolchain::Msvc, "post", 0x1000, 16, true).err() == Some(SB_FROZEN);
    s.add("J15_sealed", j15, "印账后钉版/入册皆拒 FROZEN");

    // J16 按名查命中与未命中。
    let hit = lib.find(Toolchain::Msvc, "msvc_fn0");
    s.add(
        "J16_find",
        hit.is_some() && hit.unwrap().len == 32 && lib.find(Toolchain::Msvc, "nope").is_none(),
        "按族按名查命中/未命中",
    );

    // J17 pdata 位与叶数联动。
    let _ = lib.put(Toolchain::Msvc, "leaf_fn", 0xA000, 8, false);
    s.add(
        "J17_leaf",
        lib.pdata_count() == 10 && lib.leaf_count() == 1,
        "pdata 位与叶函数计数联动对平",
    );

    // J18 渲染行含族名与 pdata 位。
    let r = lib.find(Toolchain::Msvc, "leaf_fn").unwrap().render_line();
    s.add(
        "J18_render",
        r.starts_with("MSVC:leaf_fn") && r.ends_with("pdata=false"),
        "渲染行含族名与 pdata 位",
    );

    // J19 账面摘要。
    let sm = lib.book_summary();
    s.add(
        "J19_summary",
        sm.contains("pins=2") && sm.contains("entries=11"),
        "摘要含钉版数与条目数",
    );

    // J20 空库指纹稳定且为零条目。
    let lib4 = SampleLibrary::new();
    s.add(
        "J20_fresh",
        lib4.entries.is_empty() && lib4.fold() == lib4.fold(),
        "空库指纹稳定",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12134_pin_put() {
        let mut lib = SampleLibrary::new();
        let _ = lib.pin(Toolchain::Msvc, 1938);
        assert!(lib.put(Toolchain::Msvc, "f", 0x1000, 16, true).is_ok());
        assert_eq!(lib.put(Toolchain::Msvc, "f", 0x2000, 16, true), Err(SB_DUP));
    }

    #[test]
    fn unx12134_leaf() {
        let mut lib = SampleLibrary::new();
        let _ = lib.pin(Toolchain::MinGw, 1302);
        let _ = lib.put(Toolchain::MinGw, "leaf", 0x1000, 8, false);
        assert_eq!(lib.leaf_count(), 1);
        assert_eq!(lib.pdata_count(), 0);
    }

    #[test]
    fn unx12134_checks_green() {
        let s = run_unx12134_checks();
        assert!(s.all_passed(), "UNX-F12134 判据未全绿");
    }

    #[test]
    fn unx12134_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
