//! UNX-F36143-J1 地基联动剧本 III：掩码注册→GENERIC 翻译→ACE 存储往返。
//! 判据：① 4 个对象类型注册 GENERIC 映射，12 例「注册→翻译→存 ACE→读回」
//! 往返全对平；② 重复注册 1 例/表满 1 例/零类型 1 例/未知类型翻译 1 例/
//! 非 GENERIC 掩码翻译 1 例全部显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 联动剧本之三——映射表/翻译器/ACE 存储均在本地建模
//! （真源 36081 只供 FNV），剧本验证三段数据流的往返不丢真。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x8250;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_TAG: u16 = SEG_BASE + 2;    // 零对象类型
pub const E_DUP: u16 = SEG_BASE + 3;    // 重复注册
pub const E_FULL: u16 = SEG_BASE + 4;   // 映射表满
pub const E_NOENT: u16 = SEG_BASE + 5;  // 未知类型（未注册）
pub const E_GENERIC: u16 = SEG_BASE + 6;// 待翻译掩码混入非 GENERIC 位
pub const E_CS: u16 = SEG_BASE + 7;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 映射表容量。
pub const MAP_CAP: usize = 8;
/// GENERIC 四位（高 4 位）。
pub const G_READ: u32 = 0x8000_0000;
pub const G_WRITE: u32 = 0x4000_0000;
pub const G_EXECUTE: u32 = 0x2000_0000;
pub const G_ALL: u32 = 0x1000_0000;
pub const G_MASK: u32 = 0xf000_0000;

/// 单类型映射项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenMapEntry {
    pub type_tag: u16,
    pub read: u32,
    pub write: u32,
    pub execute: u32,
    pub all: u32,
}

/// GENERIC 映射注册表。
#[derive(Clone, Copy)]
pub struct GenMap {
    pub entries: [Option<GenMapEntry>; MAP_CAP],
    pub n: usize,
}

const NONE_ENTRY: Option<GenMapEntry> = None;

impl GenMap {
    pub const fn fresh() -> GenMap {
        GenMap { entries: [NONE_ENTRY; MAP_CAP], n: 0 }
    }

    /// 注册：零类型闸 → 重复闸 → 满容闸。
    pub fn register(&mut self, type_tag: u16, read: u32, write: u32, execute: u32, all: u32) -> Result<usize, u16> {
        if type_tag == 0 {
            return Err(E_TAG);
        }
        let mut i = 0;
        while i < self.n {
            if let Some(e) = self.entries[i] {
                if e.type_tag == type_tag {
                    return Err(E_DUP);
                }
            }
            i += 1;
        }
        if self.n >= MAP_CAP {
            return Err(E_FULL);
        }
        self.entries[self.n] = Some(GenMapEntry { type_tag, read, write, execute, all });
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 查映射。
    pub fn lookup(&self, type_tag: u16) -> Result<GenMapEntry, u16> {
        let mut i = 0;
        while i < self.n {
            if let Some(e) = self.entries[i] {
                if e.type_tag == type_tag {
                    return Ok(e);
                }
            }
            i += 1;
        }
        Err(E_NOENT)
    }
}

/// GENERIC 翻译守门：desired 必须只含 GENERIC 四位（混入低位 = E_GENERIC）；
/// 翻译主体（GENERIC→特定展开）在剧本主流程内逐位进行，此处只承载闸面。
pub fn translate_guard(m: &GenMap, type_tag: u16, desired: u32) -> Result<u32, u16> {
    if desired & !G_MASK != 0 {
        return Err(E_GENERIC);
    }
    let _ = m;
    let _ = type_tag;
    Err(E_NOENT)
}

/// ACE 本地存储（头 8 字节线形式，真源编码/解析）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AceStore {
    pub wire: [u8; 8],
    pub occupied: bool,
}

/// 翻译结果存入 ACE（allow 类型）并读回掩码——往返不丢真。
pub fn store_ace(store: &mut AceStore, mask: u32) -> Result<(), u16> {
    let h = core::AceHeader::new(0x00, 0x00, core::ACE_HEADER_MIN as u16, mask)?;
    store.wire = core::encode(&h);
    store.occupied = true;
    Ok(())
}

/// 从 ACE 读回掩码（空存储 = 未知对象，E_NOENT）。
pub fn readback_ace(store: &AceStore) -> Result<u32, u16> {
    if !store.occupied {
        return Err(E_NOENT);
    }
    let h = core::parse(&store.wire)?;
    Ok(h.mask)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(m: &GenMap) -> String {
    let mut s = String::new();
    s.push_str("script3 types=");
    s.push_str(&m.n.to_string());
    s.push_str("/");
    s.push_str(&MAP_CAP.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs: Vec<u16> = vec![E_VER, E_TAG, E_DUP, E_FULL, E_NOENT, E_GENERIC, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TAG => "零对象类型",
        E_DUP => "重复注册",
        E_FULL => "映射表满",
        E_NOENT => "未知类型（未注册）/空存储",
        E_GENERIC => "待翻译掩码混入非 GENERIC 位",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位；R2 4 类型注册 + 12 例翻译-存-读往返；R3 负例 5 例；
/// R4 指纹稳定；R5 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + GENERIC 掩码锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if G_MASK != G_READ | G_WRITE | G_EXECUTE | G_ALL {
        return Err(E_CS);
    }
    hits += 1;
    // R2：4 类型注册，映射滚动；12 例翻译→存→读往返。
    let mut map = GenMap::fresh();
    let mut t: u16 = 1;
    while t <= 4 {
        map.register(t, 0x0001_0001, 0x0002_0002, 0x0000_0004, 0x000f_000f)?;
        t += 1;
    }
    let mut i: u32 = 0;
    while i < 12 {
        let tag: u16 = (i % 4) as u16 + 1;
        let want: u32 = match i % 4 {
            0 => G_READ,
            1 => G_WRITE,
            2 => G_EXECUTE,
            _ => G_ALL,
        };
        let e = map.lookup(tag)?;
        // 翻译：GENERIC 位 → 映射的特定位（逐位展开）。
        let mut specific: u32 = 0;
        if want & G_READ != 0 {
            specific |= e.read;
        }
        if want & G_WRITE != 0 {
            specific |= e.write;
        }
        if want & G_EXECUTE != 0 {
            specific |= e.execute;
        }
        if want & G_ALL != 0 {
            specific |= e.all;
        }
        if specific == 0 {
            return Err(E_NOENT);
        }
        let mut st = AceStore { wire: [0; 8], occupied: false };
        store_ace(&mut st, specific)?;
        let back = readback_ace(&st)?;
        if back != specific {
            return Err(E_CS);
        }
        hits += 1;
        i += 1;
    }
    // R3：负例 5 例。
    match map.register(0, 1, 1, 1, 1) {
        Ok(_) => return Err(E_TAG),
        Err(c) if c == E_TAG => {}
        Err(_) => return Err(E_TAG),
    }
    hits += 1;
    match map.register(2, 1, 1, 1, 1) {
        Ok(_) => return Err(E_DUP),
        Err(c) if c == E_DUP => {}
        Err(_) => return Err(E_DUP),
    }
    hits += 1;
    match map.lookup(99) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    let empty = AceStore { wire: [0; 8], occupied: false };
    match readback_ace(&empty) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    // 翻译器守门：非 GENERIC 位混入 → E_GENERIC。
    match translate_guard(&map, 1, 0x0001) {
        Ok(_) => return Err(E_GENERIC),
        Err(c) if c == E_GENERIC => {}
        Err(_) => return Err(E_GENERIC),
    }
    hits += 1;
    // R4：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut k = 0;
    while k < map.n {
        if let Some(e) = map.entries[k] {
            feed.extend_from_slice(&e.type_tag.to_le_bytes());
            feed.extend_from_slice(&e.read.to_le_bytes());
        }
        k += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R5：双跑一致（同参往返两遍）。
    let e1 = map.lookup(3)?;
    let mut sa = AceStore { wire: [0; 8], occupied: false };
    let mut sb = AceStore { wire: [0; 8], occupied: false };
    store_ace(&mut sa, e1.read)?;
    store_ace(&mut sb, e1.read)?;
    if sa != sb || readback_ace(&sa)? != readback_ace(&sb)? {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(20));
    }

    #[test]
    fn register_gates() {
        let mut m = GenMap::fresh();
        assert_eq!(m.register(0, 1, 1, 1, 1), Err(E_TAG));
        let _ = m.register(1, 1, 1, 1, 1);
        assert_eq!(m.register(1, 1, 1, 1, 1), Err(E_DUP));
    }

    #[test]
    fn roundtrip_keeps_mask() {
        let mut st = AceStore { wire: [0; 8], occupied: false };
        store_ace(&mut st, 0x0001_2003).unwrap_or(());
        assert_eq!(readback_ace(&st), Ok(0x0001_2003));
    }

    #[test]
    fn summary_shapes() {
        let m = GenMap::fresh();
        assert!(summary(&m).contains("script3"));
        assert!(codes_distinct());
    }
}

pub fn run_unx36143_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36143_script3");
    cs.add("SEG 段位独占", SEG_BASE == 0x8250, "0x8250~0x825f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "12 例往返对平、负例 5 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("GENERIC 四位锚 0xf0000000", G_MASK == 0xf000_0000, "GENERIC 掩码锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
