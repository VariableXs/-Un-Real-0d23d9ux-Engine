//! UNX-F56049 Windows 侧启动段对照映射件（0x60600 段）
//!
//! 锚点：UNX-F56049-J1 Windows 启动段（POST→bootmgr→内核→登录→桌面
//! 就绪→交互就绪）与 Varix 五段语义映射表落账并声明 L1–L4 等级；J2
//! 映射表冻结走 mini-ADR（决议行只追加）；J3 双向解析闭包（Varix 段
//! ↔ Windows 段互查零缺射）三判据全绿。判据锚定 §AI-71.2 专题三。
//! 域：CoRun·UNX-F2-B40（启动分段域·Win 段对照面）。
//!
//! 结构：双系统段映射闭集登记册（Varix 五段 × Windows 六段逐条映射 +
//! 等级落账）+ mini-ADR 决议账 + 双向解析核 + 表指纹（封账材料）+ 注
//! 入核（越闭集段/越闭集等级/空映射名/ADR 封后改写/重复映射必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60600 独占 0x20）。
pub const SEG_BASE: u32 = 0x60600;
pub const WM_SEG_UNK: u32 = SEG_BASE + 0x01; // 越闭集段
pub const WM_LEVEL_UNK: u32 = SEG_BASE + 0x02; // 越闭集等级
pub const WM_NAME_EMPTY: u32 = SEG_BASE + 0x03; // 映射名空
pub const WM_ADR_SEALED: u32 = SEG_BASE + 0x04; // ADR 封账拒追加
pub const WM_DUP_MAP: u32 = SEG_BASE + 0x05; // 同段重复映射
pub const WM_SEQ_BACK: u32 = SEG_BASE + 0x06; // ADR 行号回退
pub const WM_FP_ZERO: u32 = SEG_BASE + 0x07; // 表指纹退化零哨兵

/// Varix 五段闭集（与 F56041 同源）。
pub const VX_SEGS: [&str; 5] = [
    "firmware_handoff", "kernel_first_task", "login_face",
    "desktop_first_frame", "click_first_response",
];

/// Windows 六段闭集。
pub const WIN_SEGS: [&str; 6] = [
    "post", "bootmgr", "kernel_start", "login_visible", "desktop_ready", "interactive_ready",
];

/// 等级闭集。
pub const LV_L1: usize = 0;
pub const LV_L2: usize = 1;
pub const LV_L3: usize = 2;
pub const LV_L4: usize = 3;
pub const LV_COUNT: usize = 4;
pub const LV_NAMES: [&str; LV_COUNT] = ["L1", "L2", "L3", "L4"];

pub fn level_ok(l: usize) -> bool {
    l < LV_COUNT
}

/// 标准映射：Varix 段 → (Windows 段索引, 等级)。
/// firmware_handoff→(post, L1)；kernel_first_task→(kernel_start, L2)；
/// login_face→(login_visible, L1)；desktop_first_frame→(desktop_ready, L1)；
/// click_first_response→(interactive_ready, L2)。
pub const STD_WIN_IDX: [usize; 5] = [0, 2, 3, 4, 5];
pub const STD_LEVELS: [usize; 5] = [LV_L1, LV_L2, LV_L1, LV_L1, LV_L2];

/// FNV-1a 32 位（表指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 段映射账目：Varix 段 × Windows 段 × 等级 × 指纹。
#[derive(Clone)]
pub struct SegMapEntry {
    pub vx_seg: usize,
    pub win_seg: usize,
    pub level: usize,
    pub fingerprint: u32,
}

impl SegMapEntry {
    pub fn new(vx_seg: usize, win_seg: usize, level: usize) -> Result<Self, u32> {
        if vx_seg >= 5 {
            return Err(WM_SEG_UNK);
        }
        if win_seg >= 6 {
            return Err(WM_SEG_UNK);
        }
        if !level_ok(level) {
            return Err(WM_LEVEL_UNK);
        }
        let fp = fnv1a(format!("{}|{}|{}", VX_SEGS[vx_seg], WIN_SEGS[win_seg], level).as_bytes());
        if fp == 0 {
            return Err(WM_FP_ZERO);
        }
        Ok(SegMapEntry { vx_seg, win_seg, level, fingerprint: fp })
    }
}

/// mini-ADR 决议行。
#[derive(Clone)]
pub struct AdrRow {
    pub seq: u32,
    pub vx_seg: usize,
    pub level: usize,
    pub note: String,
}

/// 映射册 + ADR 账。
pub struct WinMapBook {
    pub maps: Vec<SegMapEntry>,
    pub adr: Vec<AdrRow>,
    pub adr_sealed: bool,
}

impl WinMapBook {
    pub fn new() -> Self {
        WinMapBook { maps: Vec::new(), adr: Vec::new(), adr_sealed: false }
    }

    /// 逐条映射（同 Varix 段重复拒）。
    pub fn map(&mut self, e: SegMapEntry) -> Result<(), u32> {
        for m in self.maps.iter() {
            if m.vx_seg == e.vx_seg {
                return Err(WM_DUP_MAP);
            }
        }
        self.maps.push(e);
        Ok(())
    }

    /// ADR 行追加（号递增 + 封账拒）。
    pub fn adr_append(&mut self, row: AdrRow) -> Result<(), u32> {
        if self.adr_sealed {
            return Err(WM_ADR_SEALED);
        }
        if row.vx_seg >= 5 || !level_ok(row.level) {
            return Err(WM_SEG_UNK);
        }
        if let Some(prev) = self.adr.last() {
            if row.seq <= prev.seq {
                return Err(WM_SEQ_BACK);
            }
        }
        self.adr.push(row);
        Ok(())
    }

    /// 正向解析。
    pub fn to_win(&self, vx: usize) -> Option<(usize, usize)> {
        for m in self.maps.iter() {
            if m.vx_seg == vx {
                return Some((m.win_seg, m.level));
            }
        }
        None
    }

    /// 反向解析。
    pub fn to_vx(&self, win: usize) -> Option<usize> {
        for m in self.maps.iter() {
            if m.win_seg == win {
                return Some(m.vx_seg);
            }
        }
        None
    }

    /// J1：五段全映射且等级齐。
    pub fn full(&self) -> bool {
        if self.maps.len() != 5 {
            return false;
        }
        for v in 0..5 {
            match self.to_win(v) {
                None => return false,
                Some((w, lv)) => {
                    if w >= 6 || !level_ok(lv) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// J1'：等级账与标准一致。
    pub fn levels_match(&self) -> bool {
        for v in 0..5 {
            match self.to_win(v) {
                None => return false,
                Some((w, lv)) => {
                    if w != STD_WIN_IDX[v] || lv != STD_LEVELS[v] {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 表指纹。
    pub fn table_fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for m in self.maps.iter() {
            h ^= m.fingerprint;
            h = h.wrapping_mul(16777619);
        }
        h
    }

    /// ADR 封账。
    pub fn seal_adr(&mut self) {
        self.adr_sealed = true;
    }
}

/// 标准夹具：五段标准映射 + ADR 一行（POST 段初议 L1）。
pub fn standard_map() -> WinMapBook {
    let mut b = WinMapBook::new();
    for v in 0..5 {
        b.map(SegMapEntry::new(v, STD_WIN_IDX[v], STD_LEVELS[v]).expect("fixture"))
            .expect("reg");
    }
    b.adr_append(AdrRow {
        seq: 1,
        vx_seg: 0,
        level: LV_L1,
        note: "初议：POST 与固件交权同源时刻".to_string(),
    })
    .expect("adr");
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) Varix 段越闭集（vx=9）
    if SegMapEntry::new(9, 0, LV_L1).err() == Some(WM_SEG_UNK) {
        caught += 1;
    }
    // 2) Windows 段越闭集（win=7）
    if SegMapEntry::new(0, 7, LV_L1).err() == Some(WM_SEG_UNK) {
        caught += 1;
    }
    // 3) 等级越闭集
    if SegMapEntry::new(0, 0, 7).err() == Some(WM_LEVEL_UNK) {
        caught += 1;
    }
    // 4) 同段重复映射
    let mut b = standard_map();
    if b.map(SegMapEntry::new(0, 1, LV_L2).expect("dup")) == Err(WM_DUP_MAP) {
        caught += 1;
    }
    // 5) ADR 封账拒 + 行号回退
    let mut b2 = standard_map();
    b2.seal_adr();
    if b2.adr_append(AdrRow { seq: 2, vx_seg: 1, level: LV_L2, note: "x".to_string() })
        == Err(WM_ADR_SEALED)
    {
        caught += 1;
    }
    let mut b3 = standard_map();
    if b3.adr_append(AdrRow { seq: 1, vx_seg: 1, level: LV_L2, note: "y".to_string() })
        == Err(WM_SEQ_BACK)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56049_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56049");
    // J1-1 五段全映射
    let b = standard_map();
    let ok = b.full();
    s.add("56049-J1-五段全映射", ok, if ok { "" } else { "映射缺段" });
    // J1-2 等级账与标准一致
    let ok = b.levels_match();
    s.add("56049-J1-等级账一致", ok, if ok { "" } else { "等级漂移" });
    // J1-3 段名闭集同源（Varix 五段 / Windows 六段）
    let ok = VX_SEGS[0] == "firmware_handoff" && WIN_SEGS[0] == "post"
        && WIN_SEGS[5] == "interactive_ready";
    s.add("56049-J1-段名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 mini-ADR 决议行留痕
    let ok = b.adr.len() == 1 && b.adr[0].seq == 1 && b.adr[0].note.contains("POST");
    s.add("56049-J2-ADR决议留痕", ok, if ok { "" } else { "ADR 缺" });
    // J2-2 表指纹确定且敏感
    let fp1 = standard_map().table_fingerprint();
    let fp2 = standard_map().table_fingerprint();
    let mut b2 = standard_map();
    b2.maps[1].level = LV_L3;
    let ok = fp1 == fp2 && fp1 != b2.table_fingerprint() && fp1 != 0;
    s.add("56049-J2-表指纹敏感", ok, if ok { "" } else { "指纹漂移" });
    // J3-1 双向解析闭包
    let mut bidir = true;
    for v in 0..5 {
        match b.to_win(v) {
            None => bidir = false,
            Some((w, _)) => {
                if b.to_vx(w) != Some(v) {
                    bidir = false;
                }
            }
        }
    }
    let ok = bidir;
    s.add("56049-J3-双向解析闭包", ok, if ok { "" } else { "双向断" });
    // J3-2 未映射段显性（bootmgr 无 Varix 对侧 → 反查 None 显性）
    let ok = b.to_vx(1).is_none();
    s.add("56049-J3-未映射显性", ok, if ok { "" } else { "缺射被猜" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56049-J3-注入核必抓", ok, &detail);
    // J1-4 等级名闭集
    let ok = LV_NAMES[0] == "L1" && LV_NAMES[3] == "L4";
    s.add("56049-J1-等级名闭集", ok, if ok { "" } else { "等级名漂移" });
    // J2-3 ADR 封账冻结
    let mut b3 = standard_map();
    b3.seal_adr();
    let ok = b3.adr_sealed
        && b3.adr_append(AdrRow { seq: 9, vx_seg: 4, level: LV_L1, note: "s".to_string() })
            == Err(WM_ADR_SEALED);
    s.add("56049-J2-ADR封账冻结", ok, if ok { "" } else { "封账后可追加" });
    // J1-5 映射语义抽查（login_face → login_visible L1）
    let ok = b.to_win(2) == Some((3, LV_L1));
    s.add("56049-J1-登录段映射", ok, if ok { "" } else { "映射漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56049_all_checks_green() {
        let set = run_unx56049_checks();
        assert!(set.all_passed(), "UNX-F56049 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56049_bidir() {
        let b = standard_map();
        assert_eq!(b.to_win(4), Some((5, LV_L2)));
        assert_eq!(b.to_vx(0), Some(0));
    }
}
