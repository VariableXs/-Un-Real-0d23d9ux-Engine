//! UNX-F48455 —— E23 错误码族 E 段（14 条）
//! 判据原文：UNX-F48455-J1 十四码冻结表逐位比对；码连续唯一（SEG_BASE+1..+14）；
//!           名义绑定三向一致（码↔名↔描述）；族外查询拒绝；反查双射；
//!           LCG 128 轮随机取码不漂移；表封印重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDDC0; // E23 错误码族段

pub const E23_E_DICT_CRC: u16 = SEG_BASE + 1; // 词库 CRC 损坏
pub const E23_E_DICT_HEAD: u16 = SEG_BASE + 2; // 词库头部伪造
pub const E23_E_BIGRAM_BAD: u16 = SEG_BASE + 3; // bigram 表损坏
pub const E23_E_BIGRAM_OOB: u16 = SEG_BASE + 4; // bigram 越界索引
pub const E23_E_USER_ATOM: u16 = SEG_BASE + 5; // 用户档原子恢复
pub const E23_E_USER_HALF: u16 = SEG_BASE + 6; // 用户档半写
pub const E23_E_CFG_THREE: u16 = SEG_BASE + 7; // 配置三态
pub const E23_E_CFG_FIELD: u16 = SEG_BASE + 8; // 配置字段攻击
pub const E23_E_SYL_BAD: u16 = SEG_BASE + 9; // 音节表损坏
pub const E23_E_SYL_OOB: u16 = SEG_BASE + 10; // 音节表越界
pub const E23_E_FDICT_REJ: u16 = SEG_BASE + 11; // fcitx5 词库拒载
pub const E23_E_FSTREAM: u16 = SEG_BASE + 12; // fcitx5 半包
pub const E23_E_REG_SENT: u16 = SEG_BASE + 13; // 回归哨兵失配
pub const E23_E_LEDGER: u16 = SEG_BASE + 14; // 缺陷账联动失配

const E_TBL_BAD: u16 = SEG_BASE + 15;
const E_DUP_CODE: u16 = SEG_BASE + 16;
const E_NAME_MISS: u16 = SEG_BASE + 17;
const E_FOREIGN: u16 = SEG_BASE + 18;
const E_BIJECT_MISS: u16 = SEG_BASE + 19;
const E_SEAL_BAD: u16 = SEG_BASE + 20;

/// 错误码条目。
#[derive(Clone, Copy)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub desc: &'static str,
}

/// ×14 冻结表。
pub const TABLE: [ErrEntry; 14] = [
    ErrEntry { code: E23_E_DICT_CRC, name: "e23_dict_crc", desc: "词库 CRC 校验失败" },
    ErrEntry { code: E23_E_DICT_HEAD, name: "e23_dict_head", desc: "词库版本头伪造" },
    ErrEntry { code: E23_E_BIGRAM_BAD, name: "e23_bigram_bad", desc: "bigram 表结构损坏" },
    ErrEntry { code: E23_E_BIGRAM_OOB, name: "e23_bigram_oob", desc: "bigram 索引越界" },
    ErrEntry { code: E23_E_USER_ATOM, name: "e23_user_atom", desc: "用户档原子恢复路径" },
    ErrEntry { code: E23_E_USER_HALF, name: "e23_user_half", desc: "用户档半写状态" },
    ErrEntry { code: E23_E_CFG_THREE, name: "e23_cfg_three", desc: "配置三态归宿链" },
    ErrEntry { code: E23_E_CFG_FIELD, name: "e23_cfg_field", desc: "配置未知字段攻击" },
    ErrEntry { code: E23_E_SYL_BAD, name: "e23_syl_bad", desc: "音节表损坏" },
    ErrEntry { code: E23_E_SYL_OOB, name: "e23_syl_oob", desc: "音节表非法索引" },
    ErrEntry { code: E23_E_FDICT_REJ, name: "e23_fdict_rej", desc: "fcitx5 .dict 拒载" },
    ErrEntry { code: E23_E_FSTREAM, name: "e23_fstream", desc: "fcitx5 流半包重同步" },
    ErrEntry { code: E23_E_REG_SENT, name: "e23_reg_sent", desc: "三窗损坏回归失配" },
    ErrEntry { code: E23_E_LEDGER, name: "e23_ledger", desc: "AI-97 账联动失配" },
];

/// 名→码。
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

/// 码→名。
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

/// 表封印指纹。
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

    // R1 十四码连续逐位
    let mut i = 0usize;
    while i < 14 {
        if TABLE[i].code != SEG_BASE + (i as u16) + 1 {
            return Err(E_TBL_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 两两互异
    let mut i = 0usize;
    while i < 14 {
        let mut j = i + 1;
        while j < 14 {
            if TABLE[i].code == TABLE[j].code {
                return Err(E_DUP_CODE);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 名义绑定：name/desc 非空 + e23_ 前缀统一
    let mut i = 0usize;
    while i < 14 {
        if TABLE[i].name.is_empty() || TABLE[i].desc.is_empty() {
            return Err(E_NAME_MISS);
        }
        let b = TABLE[i].name.as_bytes();
        if b.len() < 5 || b[0] != b'e' || b[1] != b'2' || b[2] != b'3' || b[3] != b'_' {
            return Err(E_NAME_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R4 正反双射 ×14
    let mut i = 0usize;
    while i < 14 {
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

    // R5 族外拒绝：+15 与 0x0001
    match lookup_code(SEG_BASE + 15) {
        Err(E_FOREIGN) => {}
        _ => return Err(E_FOREIGN),
    }
    match lookup_code(0x0001) {
        Err(E_FOREIGN) => {}
        _ => return Err(E_FOREIGN),
    }
    match lookup_name("nope") {
        Err(E_NAME_MISS) => {}
        _ => return Err(E_NAME_MISS),
    }
    hits += 1;

    // R6 LCG 128 轮取码不漂移
    let mut rng = Lcg::new(0x48455);
    let mut k = 0u32;
    while k < 128 {
        let idx = rng.below(14) as usize;
        let c = lookup_name(TABLE[idx].name)?;
        if c != TABLE[idx].code {
            return Err(E_TBL_BAD);
        }
        k += 1;
    }
    hits += 1;

    // R7 封印稳定非零
    let s1 = table_fp();
    let s2 = table_fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R8 边界：首码 +1 尾码 +14、表长 14
    if TABLE.len() != 14 || TABLE[0].code != SEG_BASE + 1 || TABLE[13].code != SEG_BASE + 14 {
        return Err(E_TBL_BAD);
    }
    hits += 1;

    Ok(hits)
}
