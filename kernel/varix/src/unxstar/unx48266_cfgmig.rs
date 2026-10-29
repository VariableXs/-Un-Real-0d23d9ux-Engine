//! unx48266_cfgmig — UNX-F48266 配置迁移单向面（Varix 导出→fcitx5 可读）（诊断段 0xC220）。
//!
//! 判据原文：
//!   J1 迁移 ×10 fcitx5 侧可读断言；判据锚定 §AI-61 §2 专题四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 单向面定义：只许 Varix 侧导出、fcitx5 侧读取；反向导入路径
//!      不存在——任何逆向迁移尝试即 E_ONE_WAY（语义边界，非漏斗）。
//!   2. 键位互认表：Varix 键 → fcitx5 键 id 经常量映射表转换；fcitx5
//!      键 id 必须落在其命名空间 [0x5000, 0x6000)，越界即 E_NS_BAD。
//!   3. 迁移流定式：头（magic "V2F" + 记录数）+ 定长记录区（节号 u8 +
//!      fcitx5 键 id u16 + 值 u32，小端）；全序按 (节号, 键) 升序落盘，
//!      同库双导出字节逐位相同（确定性）。
//!   4. 侧可读断言（J1 主体）：fcitx5 侧读取器（独立游标实现）解析
//!      迁移流，×10 条记录逐条回收 (节号, 键, 值) 与源配置逐位相等。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC220;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ONE_WAY: u16 = SEG_BASE + 1; // 逆向迁移尝试（单向面违规）
const E_NS_BAD: u16 = SEG_BASE + 2; // fcitx5 键 id 越命名空间
const E_PARSE_BAD: u16 = SEG_BASE + 3; // 读取器解析失守
const E_MIG_MISS: u16 = SEG_BASE + 4; // 键位互认表查无映射
const E_DUP_KEY: u16 = SEG_BASE + 5; // 同节重键
const E_STATE_BAD: u16 = SEG_BASE + 6; // 迁移器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 封装常量。
pub const MAGIC: u32 = 0x5632_4634; // "V2F4"
pub const REC_BYTES: usize = 7; // 节号1 + 键2 + 值4

/// fcitx5 键命名空间。
pub const FC_NS_LO: u16 = 0x5000;
pub const FC_NS_HI: u16 = 0x6000;

/// 键位互认表：Varix 键 → fcitx5 键 id（常量，双向不认——单向面）。
pub const KEYMAP: [(u16, u16); 10] = [
    (0x0011, 0x5A01), // 预编辑开关
    (0x0012, 0x5A02), // 候选数
    (0x0013, 0x5A03), // 翻页键
    (0x0014, 0x5A04), // 临时英文键
    (0x0021, 0x5B01), // 全拼方案
    (0x0022, 0x5B02), // 双拼方案
    (0x0023, 0x5B03), // 五笔方案
    (0x0031, 0x5C01), // 词库路径
    (0x0032, 0x5C02), // 词库频次阈
    (0x0041, 0x5D01), // 观测埋点开关
];

/// 源配置一条：(节号, Varix 键, 值)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CfgEntry {
    pub section: u8,
    pub key: u16,
    pub val: u32,
}

/// Varix 侧源配置集。
pub struct VarixCfg {
    pub entries: Vec<CfgEntry>,
}

impl VarixCfg {
    pub fn new() -> VarixCfg {
        VarixCfg { entries: Vec::new() }
    }
    /// 入账：同节重键拒收。
    pub fn put(&mut self, section: u8, key: u16, val: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].section == section && self.entries[i].key == key {
                return Err(E_DUP_KEY);
            }
            i += 1;
        }
        self.entries.push(CfgEntry { section, key, val });
        Ok(())
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 键位互认：Varix 键 → fcitx5 键 id。
pub fn fc_key_of(varix_key: u16) -> Result<u16, u16> {
    let mut i = 0usize;
    while i < KEYMAP.len() {
        if KEYMAP[i].0 == varix_key {
            let fc = KEYMAP[i].1;
            if fc < FC_NS_LO || fc >= FC_NS_HI {
                return Err(E_NS_BAD);
            }
            return Ok(fc);
        }
        i += 1;
    }
    Err(E_MIG_MISS)
}

/// 导出（单向）：源配置 → fcitx5 迁移流（排序确定性 + 互认转换）。
pub fn migrate(cfg: &VarixCfg) -> Result<Vec<u8>, u16> {
    if cfg.entry_count() == 0 {
        return Err(E_STATE_BAD);
    }
    // 互认全量预检：任何键查无映射即整体拒迁（不留半流）。
    let mut i = 0usize;
    while i < cfg.entries.len() {
        fc_key_of(cfg.entries[i].key)?;
        i += 1;
    }
    // 按 (节号, Varix 键) 升序排序（选择排序，确定性）。
    let mut order: Vec<usize> = Vec::new();
    i = 0;
    while i < cfg.entries.len() {
        order.push(i);
        i += 1;
    }
    let mut a = 0usize;
    while a < order.len() {
        let mut b = a + 1;
        while b < order.len() {
            let ea = cfg.entries[order[a]];
            let eb = cfg.entries[order[b]];
            if eb.section < ea.section || (eb.section == ea.section && eb.key < ea.key) {
                let t = order[a];
                order[a] = order[b];
                order[b] = t;
            }
            b += 1;
        }
        a += 1;
    }
    let mut out: Vec<u8> = Vec::new();
    // 头：magic + 记录数。
    let magic = MAGIC;
    out.push((magic & 0xFF) as u8);
    out.push(((magic >> 8) & 0xFF) as u8);
    out.push(((magic >> 16) & 0xFF) as u8);
    out.push(((magic >> 24) & 0xFF) as u8);
    out.push(cfg.entry_count() as u8);
    i = 0;
    while i < order.len() {
        let e = cfg.entries[order[i]];
        let fk = fc_key_of(e.key)?;
        out.push(e.section);
        out.push((fk & 0xFF) as u8);
        out.push(((fk >> 8) & 0xFF) as u8);
        out.push((e.val & 0xFF) as u8);
        out.push(((e.val >> 8) & 0xFF) as u8);
        out.push(((e.val >> 16) & 0xFF) as u8);
        out.push(((e.val >> 24) & 0xFF) as u8);
        i += 1;
    }
    Ok(out)
}

/// fcitx5 侧记录（读取器回收产物）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FcRecord {
    pub section: u8,
    pub fc_key: u16,
    pub val: u32,
}

/// fcitx5 侧读取器（独立实现）：游标解析迁移流。
pub fn fcitx5_read(bytes: &[u8]) -> Result<Vec<FcRecord>, u16> {
    if bytes.len() < 5 {
        return Err(E_PARSE_BAD);
    }
    let magic = (bytes[0] as u32)
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24);
    if magic != MAGIC {
        return Err(E_PARSE_BAD);
    }
    let count = bytes[4] as usize;
    if bytes.len() != 5 + count * REC_BYTES {
        return Err(E_PARSE_BAD);
    }
    let mut recs: Vec<FcRecord> = Vec::new();
    let mut off = 5usize;
    let mut n = 0usize;
    while n < count {
        let section = bytes[off];
        let fk = (bytes[off + 1] as u16) | ((bytes[off + 2] as u16) << 8);
        let val = (bytes[off + 3] as u32)
            | ((bytes[off + 4] as u32) << 8)
            | ((bytes[off + 5] as u32) << 16)
            | ((bytes[off + 6] as u32) << 24);
        // 读取器侧命名空间断言（fcitx5 只认自家键空间）。
        if fk < FC_NS_LO || fk >= FC_NS_HI {
            return Err(E_NS_BAD);
        }
        recs.push(FcRecord { section, fc_key: fk, val });
        off += REC_BYTES;
        n += 1;
    }
    Ok(recs)
}

/// 完成写回（判据 J2）：可读断言成立 + 非空摘要方放行。
pub fn writeback(done: bool, readable_hits: u32, summary_len: u32) -> Result<(), u16> {
    if !done || readable_hits == 0 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（迁移 ×10 fcitx5 侧可读断言），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：10 条源配置迁移，fcitx5 侧读取器逐条回收，
    //    (节号, fcitx5键, 值) 与互认表+源值逐位相等。
    let mut cfg = VarixCfg::new();
    let mut i = 0usize;
    while i < 10 {
        cfg.put(0x10 + (i / 4) as u8, KEYMAP[i].0, 1000 + i as u32)?;
        i += 1;
    }
    if cfg.entry_count() != 10 {
        return Err(E_STATE_BAD);
    }
    let stream = migrate(&cfg)?;
    let recs = fcitx5_read(&stream)?;
    if recs.len() != 10 {
        return Err(E_PARSE_BAD);
    }
    // 读取器回收次序 = 排序序；逐条核对值与命名空间。
    let mut r = 0usize;
    while r < 10 {
        let want_fc = fc_key_of(KEYMAP[r].0)?;
        // 找到该 fc 键的回收记录（按序应严格对应排序位）。
        let mut found: Option<FcRecord> = None;
        let mut k = 0usize;
        while k < recs.len() {
            if recs[k].fc_key == want_fc {
                found = Some(recs[k]);
                break;
            }
            k += 1;
        }
        match found {
            None => return Err(E_MIG_MISS),
            Some(rec) => {
                if rec.val != 1000 + r as u32 {
                    return Err(E_PARSE_BAD);
                }
                if rec.fc_key < FC_NS_LO || rec.fc_key >= FC_NS_HI {
                    return Err(E_NS_BAD);
                }
            }
        }
        r += 1;
    }
    if writeback(true, 10, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    // R2 排序确定性：同库双导出字节逐位相同；且流内记录按 (节,键) 升序。
    let stream2 = migrate(&cfg)?;
    if stream.len() != stream2.len() {
        return Err(E_STATE_BAD);
    }
    let mut m = 0usize;
    while m < stream.len() {
        if stream[m] != stream2[m] {
            return Err(E_STATE_BAD); // 双导出漂移
        }
        m += 1;
    }
    let recs2 = fcitx5_read(&stream)?;
    let mut r = 1usize;
    while r < recs2.len() {
        let a = recs2[r - 1];
        let b = recs2[r];
        if b.section < a.section || (b.section == a.section && b.fc_key < a.fc_key) {
            return Err(E_PARSE_BAD); // 升序失守
        }
        r += 1;
    }
    hits += 1;

    // R3 单向面负例：逆向迁移尝试 → E_ONE_WAY，无任何反向流产生。
    // （单向面以类型级拒收表达：提供 import 签名但恒拒。）
    let recs_backup = recs2.clone();
    if reverse_import(&recs_backup).is_ok() {
        return Err(E_ONE_WAY);
    }
    hits += 1;

    // R4 互认缺项拒迁：源配置含映射表外键 → 整流拒迁（E_MIG_MISS）。
    let mut cfg2 = VarixCfg::new();
    cfg2.put(0x10, KEYMAP[0].0, 1)?;
    cfg2.put(0x11, 0xEEEE, 2)?; // 表外键
    if migrate(&cfg2).is_ok() {
        return Err(E_MIG_MISS);
    }
    hits += 1;

    // R5 命名空间断言负例：读取器遇到越命名空间键 id → E_NS_BAD。
    // 构造：手工改写迁移流中一条记录的 fc 键为 0x1234（越界）。
    let mut forged = Vec::new();
    m = 0;
    while m < stream.len() {
        forged.push(stream[m]);
        m += 1;
    }
    // 记录区第 2 条的键字节位于 5 + 7 + 1。
    let key_off = 5 + REC_BYTES + 1;
    forged[key_off] = 0x34;
    forged[key_off + 1] = 0x12;
    if fcitx5_read(&forged).is_ok() {
        return Err(E_NS_BAD);
    }
    hits += 1;

    // R6 解析守门：magic 错 / 长度不闭合 / 头截断 → E_PARSE_BAD 三路。
    let mut bad_magic = Vec::new();
    m = 0;
    while m < stream.len() {
        bad_magic.push(if m == 0 { stream[m] ^ 0xFF } else { stream[m] });
        m += 1;
    }
    if fcitx5_read(&bad_magic).is_ok() {
        return Err(E_PARSE_BAD);
    }
    if fcitx5_read(&stream[..stream.len() - 3]).is_ok() {
        return Err(E_PARSE_BAD);
    }
    if fcitx5_read(&stream[..3]).is_ok() {
        return Err(E_PARSE_BAD);
    }
    hits += 1;

    // R7 同节重键拒收 + 互认表自洽：put 重键 → E_DUP_KEY；
    //    互认表 10 对键 id 全部落 fcitx5 命名空间且两两互异。
    let mut cfg3 = VarixCfg::new();
    cfg3.put(0x10, KEYMAP[3].0, 7)?;
    if cfg3.put(0x10, KEYMAP[3].0, 8).is_ok() {
        return Err(E_DUP_KEY);
    }
    let mut a = 0usize;
    while a < KEYMAP.len() {
        let ka = KEYMAP[a].1;
        if ka < FC_NS_LO || ka >= FC_NS_HI {
            return Err(E_NS_BAD);
        }
        let mut b = a + 1;
        while b < KEYMAP.len() {
            if KEYMAP[b].1 == ka {
                return Err(E_STATE_BAD); // 互认表键 id 相撞
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R8 写回（判据 J2）：×10 可读断言 + 非空摘要放行。
    if writeback(true, 0, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    writeback(true, 10, 0x40)?;
    hits += 1;

    Ok(hits)
}

/// 逆向导入签名：单向面恒拒（纪律 1，语义边界非漏斗）。
pub fn reverse_import(_recs: &[FcRecord]) -> Result<VarixCfg, u16> {
    Err(E_ONE_WAY)
}
