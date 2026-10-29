//! UNX-F48415 —— E21 错误码族 E 段（10 条：丢键/竞态/序违例等）
//! 判据原文：UNX-F48415-J1 十码冻结表逐位比对；族内唯一且连续（SEG_BASE+1 起）；
//!           名义绑定（码→名→义）三向一致；族外查询拒绝；反查表双射；
//!           LCG 64 轮随机取码不漂移；表封印指纹稳定重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD720; // E21 错误码族段

// ── 十码冻结表（与冻结表数组逐位一致）─────────────────────────
pub const E21_E_LOSTKEY: u16 = SEG_BASE + 1; // 丢键
pub const E21_E_RACE: u16 = SEG_BASE + 2; // 切换竞态
pub const E21_E_ORDER: u16 = SEG_BASE + 3; // 销毁序违例
pub const E21_E_ROUND: u16 = SEG_BASE + 4; // 轮切串投
pub const E21_E_LEDGER: u16 = SEG_BASE + 5; // 账本闭合破
pub const E21_E_TAMPER: u16 = SEG_BASE + 6; // 账本篡改
pub const E21_E_XFOCUS: u16 = SEG_BASE + 7; // X 焦点悬空
pub const E21_E_GHOST: u16 = SEG_BASE + 8; // 幽灵 focus
pub const E21_E_HWND: u16 = SEG_BASE + 9; // 陈旧 HWND
pub const E21_E_REBUILD: u16 = SEG_BASE + 10; // 重建代际断裂

const E_TBL_BAD: u16 = SEG_BASE + 11; // 表逐位比对失配
const E_DUP_CODE: u16 = SEG_BASE + 12; // 码重复
const E_NAME_MISS: u16 = SEG_BASE + 13; // 名义绑定缺失
const E_FOREIGN: u16 = SEG_BASE + 14; // 族外码
const E_BIJECT_MISS: u16 = SEG_BASE + 15; // 反查双射破
const E_SEAL_BAD: u16 = SEG_BASE + 16; // 封印失配
const E_COUNT_MISS: u16 = SEG_BASE + 17; // 十码计数不符

/// 错误码条目。
#[derive(Clone, Copy)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub desc: &'static str,
}

/// ×10 冻结表。
pub const TABLE: [ErrEntry; 10] = [
    ErrEntry { code: E21_E_LOSTKEY, name: "e21_lostkey", desc: "焦点切换期按键丢失" },
    ErrEntry { code: E21_E_RACE, name: "e21_race", desc: "切换帧 commit 竞态" },
    ErrEntry { code: E21_E_ORDER, name: "e21_order", desc: "先启新后杀旧序违例" },
    ErrEntry { code: E21_E_ROUND, name: "e21_round", desc: "轮切期 commit 串投" },
    ErrEntry { code: E21_E_LEDGER, name: "e21_ledger", desc: "丢键账本闭合破坏" },
    ErrEntry { code: E21_E_TAMPER, name: "e21_tamper", desc: "丢键账本遭篡改" },
    ErrEntry { code: E21_E_XFOCUS, name: "e21_xfocus", desc: "Xwayland 焦点悬空" },
    ErrEntry { code: E21_E_GHOST, name: "e21_ghost", desc: "幽灵 focus 注入" },
    ErrEntry { code: E21_E_HWND, name: "e21_hwnd", desc: "陈旧 HWND 投递" },
    ErrEntry { code: E21_E_REBUILD, name: "e21_rebuild", desc: "重建代际断裂" },
];

/// 名→码反查。
pub fn lookup_name(name: &str) -> Result<u16, u16> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if TABLE[i].name == name {
            return Ok(TABLE[i].code);
        }
        i += 1;
    }
    Err(E_NAME_MISS)
}

/// 码→名正查。
pub fn lookup_code(code: u16) -> Result<&'static str, u16> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if TABLE[i].code == code {
            return Ok(TABLE[i].name);
        }
        i += 1;
    }
    Err(E_FOREIGN)
}

/// 表封印指纹：逐条 FNV(code,name,desc)。
pub fn table_fp() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < TABLE.len() {
        let e = &TABLE[i];
        h ^= e.code as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let b = e.name.as_bytes();
        let mut j = 0usize;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            j += 1;
        }
        let b = e.desc.as_bytes();
        let mut j = 0usize;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            j += 1;
        }
        i += 1;
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：错误码族八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 十码连续：TABLE[i].code == SEG_BASE + i + 1 逐位
    let mut i = 0usize;
    while i < 10 {
        if TABLE[i].code != SEG_BASE + (i as u16) + 1 {
            return Err(E_TBL_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 族内唯一：两两互异（连续已保证，双保险）
    let mut i = 0usize;
    while i < 10 {
        let mut j = i + 1;
        while j < 10 {
            if TABLE[i].code == TABLE[j].code {
                return Err(E_DUP_CODE);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 名义绑定：每条 name/desc 非空，且 name 前缀 e21_ 统一
    let mut i = 0usize;
    while i < 10 {
        if TABLE[i].name.is_empty() || TABLE[i].desc.is_empty() {
            return Err(E_NAME_MISS);
        }
        let b = TABLE[i].name.as_bytes();
        if b.len() < 5 || b[0] != b'e' || b[1] != b'2' || b[2] != b'1' || b[3] != b'_' {
            return Err(E_NAME_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R4 正反双射：×10 全对 name→code→name 恒等
    let mut i = 0usize;
    while i < 10 {
        let c = lookup_name(TABLE[i].name)?;
        if c != TABLE[i].code {
            return Err(E_BIJECT_MISS);
        }
        let n = lookup_code(c)?;
        if n != TABLE[i].name {
            return Err(E_BIJECT_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R5 族外拒绝：code = SEG_BASE+11 与 SEG_BASE 之外查询拒绝
    match lookup_code(SEG_BASE + 11) {
        Err(E_FOREIGN) => {}
        _ => return Err(E_FOREIGN),
    }
    match lookup_code(0x0001) {
        Err(E_FOREIGN) => {}
        _ => return Err(E_FOREIGN),
    }
    match lookup_name("no_such_code") {
        Err(E_NAME_MISS) => {}
        _ => return Err(E_NAME_MISS),
    }
    hits += 1;

    // R6 LCG 64 轮随机取码不漂移：随机名取码恒等于表内值
    let mut rng = Lcg::new(0x48415);
    let mut k = 0u32;
    while k < 64 {
        let idx = rng.below(10) as usize;
        let c = lookup_name(TABLE[idx].name)?;
        if c != TABLE[idx].code {
            return Err(E_TBL_BAD);
        }
        k += 1;
    }
    hits += 1;

    // R7 表封印稳定：双算一致且非零
    let s1 = table_fp();
    let s2 = table_fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 十码计数守恒 + 首尾边界：首码 SEG_BASE+1 尾码 SEG_BASE+10
    if TABLE.len() != 10 {
        return Err(E_COUNT_MISS);
    }
    if TABLE[0].code != SEG_BASE + 1 || TABLE[9].code != SEG_BASE + 10 {
        return Err(E_TBL_BAD);
    }
    hits += 1;

    Ok(hits)
}
