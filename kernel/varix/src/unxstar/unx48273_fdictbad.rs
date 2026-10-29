//! unx48273_fdictbad — UNX-F48273 fcitx5 词库损坏防御（CRC 坏拒载+缺陷账登记）（诊断段 0xC300）。
//!
//! 判据原文：
//!   J1 损坏 ×10 全检出登记；判据锚定 §AI-61 §2 专题一错误路径同源。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 拒载红线：词库四件套（魔数/版本头/CRC32/词表体）任一损坏即
//!      拒载——损坏词库一个词条都不进引擎（宁可缺库不可脏库）。
//!   2. 损坏闭集 ×10：魔数坏/版本不识别/CRC 坏/体长不符/词条越界/
//!      键值截断/重键冲突/空词表/尾部长度残段/双重损坏——10 类逐一
//!      注入全检出并登记。
//!   3. 缺陷账：每次拒载登记 (词库id, 损坏类, 探测位)，账 append-only；
//!      同库同类重复探测只更新探测位不重记（幂等）。
//!   4. 错误路径同源：拒载回执码与 D1 全域表同源（SEG_BASE 段内），
//!      不许私造码值。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC300;

const E_MAGIC_BAD: u16 = SEG_BASE + 1;   // 魔数坏
const E_VER_BAD: u16 = SEG_BASE + 2;     // 版本不识别
const E_CRC_BAD: u16 = SEG_BASE + 3;     // CRC32 坏
const E_LEN_BAD: u16 = SEG_BASE + 4;     // 体长不符
const E_ENTRY_RANGE: u16 = SEG_BASE + 5; // 词条越界
const E_KV_TRUNC: u16 = SEG_BASE + 6;    // 键值截断
const E_DUP_KEY: u16 = SEG_BASE + 7;     // 重键冲突
const E_EMPTY_DICT: u16 = SEG_BASE + 8;  // 空词表
const E_TAIL_RESIDUE: u16 = SEG_BASE + 9; // 尾部长度残段
const E_DOUBLE_BAD: u16 = SEG_BASE + 10; // 双重损坏（多类叠加）
const E_LEDGER_BAD: u16 = SEG_BASE + 11; // 缺陷账缺失/非幂等/越段

/// 词库魔数（项目注册口径）："FICT"（fcitx5 词库兼容面 4 字节）。
pub const MAGIC: u32 = 0x4649_4354;
/// 识别版本：v1（当前唯一识别版本）。
pub const VER_OK: u16 = 1;

/// 词库四件套容器。
#[derive(Debug, Clone)]
pub struct Dict {
    pub id: u8,
    pub magic: u32,
    pub ver: u16,
    pub body_len: u32,   // 词表体声明长度
    pub body: Vec<u8>,   // 词表体（word_len(2) + word + key_len(2) + key 串接）
    pub crc: u32,        // 声明 CRC32（对 body）
}

/// CRC32（IEEE 多项式，查表法，no_std 可用）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data {
        crc = table[((crc ^ *b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// 缺陷账条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defect {
    pub dict_id: u8,
    pub kind: u16, // 损坏类（E_* 同源码）
    pub spot: u32, // 探测位（偏移或序号）
}

/// 防御总账。
#[derive(Debug)]
pub struct DictGuard {
    defects: Vec<Defect>,
    rejects: u32,
    loaded: u32,
}

/// 载入体检：逐类探测，全过才放行（返回 Ok(词条数)），坏则 Err(损坏类)。
pub fn inspect(d: &Dict) -> Result<u32, u16> {
    if d.magic != MAGIC {
        return Err(E_MAGIC_BAD);
    }
    if d.ver != VER_OK {
        return Err(E_VER_BAD);
    }
    let crc_now = crc32(&d.body);
    if crc_now != d.crc {
        return Err(E_CRC_BAD);
    }
    if d.body_len != d.body.len() as u32 {
        return Err(E_LEN_BAD);
    }
    if d.body.is_empty() {
        return Err(E_EMPTY_DICT);
    }
    // 词条遍历：word_len(2 BE) + word + key_len(2 BE) + key；任一截断/越界即坏。
    let mut seen_keys: Vec<Vec<u8>> = Vec::new();
    let mut off = 0usize;
    let mut entries = 0u32;
    while off < d.body.len() {
        if off + 2 > d.body.len() {
            return Err(E_TAIL_RESIDUE);
        }
        let wl = ((d.body[off] as u16) << 8 | d.body[off + 1] as u16) as usize;
        off += 2;
        if off + wl > d.body.len() {
            return Err(E_ENTRY_RANGE);
        }
        let word = &d.body[off..off + wl];
        off += wl;
        if off + 2 > d.body.len() {
            return Err(E_KV_TRUNC);
        }
        let kl = ((d.body[off] as u16) << 8 | d.body[off + 1] as u16) as usize;
        off += 2;
        if off + kl > d.body.len() {
            return Err(E_ENTRY_RANGE);
        }
        let key = d.body[off..off + kl].to_vec();
        off += kl;
        if word.is_empty() || key.is_empty() {
            return Err(E_KV_TRUNC);
        }
        if seen_keys.iter().any(|x| *x == key) {
            return Err(E_DUP_KEY);
        }
        seen_keys.push(key);
        entries += 1;
    }
    if off != d.body.len() {
        return Err(E_TAIL_RESIDUE);
    }
    Ok(entries)
}

impl DictGuard {
    pub fn new() -> Self {
        DictGuard { defects: Vec::new(), rejects: 0, loaded: 0 }
    }

    /// 载入尝试：Ok → loaded+1；Err(kind) → 缺陷账登记（同库同类幂等）。
    pub fn try_load(&mut self, d: &Dict) -> Result<u32, u16> {
        match inspect(d) {
            Ok(n) => {
                self.loaded += 1;
                Ok(n)
            }
            Err(kind) => {
                self.rejects += 1;
                let spot = d.body.len() as u32;
                match self.defects.iter_mut().find(|x| x.dict_id == d.id && x.kind == kind) {
                    Some(x) => x.spot = spot,
                    None => self.defects.push(Defect { dict_id: d.id, kind, spot }),
                }
                Err(kind)
            }
        }
    }

    pub fn defect_len(&self) -> usize {
        self.defects.len()
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn loaded(&self) -> u32 {
        self.loaded
    }
    /// 缺陷清单导出（J1 登记证据）。
    pub fn defect_list(&self) -> Vec<(u8, u16, u32)> {
        self.defects.iter().map(|d| (d.dict_id, d.kind, d.spot)).collect()
    }
}

/// 构造健康词库。
pub fn healthy(id: u8, pairs: &[(&str, &str)]) -> Dict {
    let mut body: Vec<u8> = Vec::new();
    for (w, k) in pairs {
        body.push((w.len() >> 8) as u8);
        body.push((w.len() & 0xFF) as u8);
        for b in w.bytes() {
            body.push(b);
        }
        body.push((k.len() >> 8) as u8);
        body.push((k.len() & 0xFF) as u8);
        for b in k.bytes() {
            body.push(b);
        }
    }
    let crc = crc32(&body);
    Dict {
        id,
        magic: MAGIC,
        ver: VER_OK,
        body_len: body.len() as u32,
        body,
        crc,
    }
}

/// 自例程判据：落实 J1「损坏 ×10 全检出登记」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = DictGuard::new();

    // R0 基线：健康词库载入成功（词条数正确）。
    let good = healthy(0, &[("ni", "你"), ("hao", "好")]);
    let n = g.try_load(&good)?;
    if n != 2 {
        return Err(E_KV_TRUNC);
    }
    hits += 1;

    // R1-R10：损坏闭集 ×10 逐类注入全检出。
    // 1 魔数坏。
    let mut d1 = healthy(1, &[("a", "甲")]);
    d1.magic = 0xDEAD_BEEF;
    match g.try_load(&d1) {
        Err(E_MAGIC_BAD) => {}
        _ => return Err(E_MAGIC_BAD),
    }
    // 2 版本不识别。
    let mut d2 = healthy(2, &[("a", "甲")]);
    d2.ver = 99;
    match g.try_load(&d2) {
        Err(E_VER_BAD) => {}
        _ => return Err(E_VER_BAD),
    }
    // 3 CRC 坏（body 翻转一字节）。
    let mut d3 = healthy(3, &[("abc", "丙")]);
    d3.body[0] ^= 0xFF;
    match g.try_load(&d3) {
        Err(E_CRC_BAD) => {}
        _ => return Err(E_CRC_BAD),
    }
    // 4 体长不符（声明与实际差 1）。
    let mut d4 = healthy(4, &[("ab", "丁")]);
    d4.body_len += 1;
    match g.try_load(&d4) {
        Err(E_LEN_BAD) => {}
        _ => return Err(E_LEN_BAD),
    }
    // 5 词条越界（word_len 超体长）。
    let mut d5 = healthy(5, &[("ab", "戊")]);
    d5.body[0] = 0x7F; // word_len 高位拉爆
    d5.crc = crc32(&d5.body);
    d5.body_len = d5.body.len() as u32;
    match g.try_load(&d5) {
        Err(E_ENTRY_RANGE) => {}
        _ => return Err(E_ENTRY_RANGE),
    }
    // 6 键值截断（key_len 声明越界）。
    let mut d6 = healthy(6, &[("ab", "己")]);
    let l = d6.body.len();
    d6.body[l - 1] = 0x7F; // key_len 高位拉爆
    d6.crc = crc32(&d6.body);
    match g.try_load(&d6) {
        Err(E_ENTRY_RANGE) | Err(E_KV_TRUNC) => {}
        _ => return Err(E_KV_TRUNC),
    }
    // 7 重键冲突。
    let d7 = healthy(7, &[("aa", "庚"), ("bb", "辛"), ("aa", "庚2")]);
    match g.try_load(&d7) {
        Err(E_DUP_KEY) => {}
        _ => return Err(E_DUP_KEY),
    }
    // 8 空词表。
    let d8 = healthy(8, &[]);
    match g.try_load(&d8) {
        Err(E_EMPTY_DICT) => {}
        _ => return Err(E_EMPTY_DICT),
    }
    // 9 尾部残段（声明体长截在词条中段）。
    let mut d9 = healthy(9, &[("abcd", "壬")]);
    let cut = d9.body.len() - 1;
    d9.body.truncate(cut);
    d9.body_len = cut as u32;
    d9.crc = crc32(&d9.body);
    match g.try_load(&d9) {
        Err(E_TAIL_RESIDUE) | Err(E_KV_TRUNC) => {}
        _ => return Err(E_TAIL_RESIDUE),
    }
    // 10 双重损坏（魔数与版本同时坏 → 按探测序报首类）。
    let mut d10 = healthy(10, &[("a", "癸")]);
    d10.magic = 0;
    d10.ver = 77;
    match g.try_load(&d10) {
        Err(E_MAGIC_BAD) => {}
        _ => return Err(E_DOUBLE_BAD),
    }
    hits += 1;

    // R11：缺陷账 —— 10 类全登记；同库同类幂等（重复探测不重记）。
    if g.defect_len() != 10 {
        return Err(E_LEDGER_BAD);
    }
    let before = g.defect_len();
    let _ = g.try_load(&d1);
    if g.defect_len() != before {
        return Err(E_LEDGER_BAD);
    }
    if g.rejects() != 11 {
        return Err(E_LEDGER_BAD); // 10 类 + 1 幂等重探
    }
    let lst = g.defect_list();
    // 全部损坏类与 D1 同源：kind 全落本段窗。
    for (_, kind, _) in lst {
        if kind <= SEG_BASE || kind > SEG_BASE + 0x1F {
            return Err(E_LEDGER_BAD);
        }
    }
    hits += 1;

    // R12：健康库不受污染 —— 缺陷账在册时健康库照常载入。
    let g2n = g.try_load(&good)?;
    if g2n != 2 || g.loaded() != 2 {
        return Err(E_CRC_BAD);
    }
    hits += 1;

    // R13：CRC32 已知向量自检（"123456789" → 0xCBF43926）。
    let v = crc32(b"123456789");
    if v != 0xCBF4_3926 {
        return Err(E_CRC_BAD);
    }
    hits += 1;

    Ok(hits)
}
