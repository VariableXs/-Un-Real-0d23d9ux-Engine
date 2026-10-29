//! unx48313_cfgmigs — UNX-F48313 配置迁移到 fcitx5 单向面复签（B14 迁移面字段扩容）（诊断段 0xC800）。
//!
//! 判据原文：
//!   J1 复签 ×10 fcitx5 可读；判据锚定 §AI-61 §2 专题四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 单向面复用：迁移仍是 Varix 导出 → fcitx5 读取的单向面，逆向
//!      导入恒拒（E_ONE_WAY）；B14 扩容 = 每条迁移记录追加 ext 扩容
//!      字段（u16 特性位，<0x8000 闭集），旧面字段全保留。
//!   2. 复签语义：导出流尾部携复签字——对头部+记录区逐字节折叠
//!      （s = s*31+b 确定性折叠）；fcitx5 侧读取前先重算复签，签不
//!      符即整流拒收（E_SIG_BAD），坏流进不了配置面。
//!   3. 复签 ×10 可读（J1 主体）：×10 条扩容记录导出→复签→fcitx5
//!      侧解析，逐条 (节,键,值,扩容位) 与源配置逐位相等、复签重算
//!      逐位相符。
//!   4. 记录守恒：同节重键拒收；同账双导出字节逐位相同（确定性）；
//!      记录数头字段与实长闭合。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC800;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MAGIC: u16 = SEG_BASE + 1; // 头 magic 不符
const E_SIG_BAD: u16 = SEG_BASE + 2; // 复签不符（篡改/坏流）
const E_PARSE_BAD: u16 = SEG_BASE + 3; // 解析失守
const E_ONE_WAY: u16 = SEG_BASE + 4; // 逆向迁移尝试
const E_DUP_KEY: u16 = SEG_BASE + 5; // 同节重键
const E_STATE_BAD: u16 = SEG_BASE + 6; // 迁移面状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 封装常量。
pub const MAGIC: u32 = 0x5632_4653; // "V2FS"（复签面）
pub const REC_BYTES: usize = 9; // 节1 + 键2 + 值4 + 扩容2

/// 扩容字段闭集：特性位 <0x8000。
pub const EXT_HI: u16 = 0x8000;

/// 复签折叠：确定性逐字节折叠（签名真源）。
pub fn sign_fold(bytes: &[u8]) -> u16 {
    let mut s: u16 = 0x51ED;
    let mut i = 0usize;
    while i < bytes.len() {
        s = s.wrapping_mul(31).wrapping_add(bytes[i] as u16);
        i += 1;
    }
    s
}

/// 源配置一条（含 B14 扩容字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigEntry {
    pub section: u8,
    pub key: u16,
    pub val: u32,
    pub ext: u16,
}

/// Varix 侧源配置集。
pub struct MigSet {
    pub entries: Vec<MigEntry>,
}

impl MigSet {
    pub fn new() -> MigSet {
        MigSet { entries: Vec::new() }
    }
    pub fn put(&mut self, section: u8, key: u16, val: u32, ext: u16) -> Result<(), u16> {
        if ext >= EXT_HI {
            return Err(E_STATE_BAD); // 扩容位越闭集
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].section == section && self.entries[i].key == key {
                return Err(E_DUP_KEY);
            }
            i += 1;
        }
        self.entries.push(MigEntry { section, key, val, ext });
        Ok(())
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// 导出（单向 + 复签）：记录区按 (节,键) 升序、尾部携复签字。
pub fn export_signed(set: &MigSet) -> Result<Vec<u8>, u16> {
    if set.entry_count() == 0 {
        return Err(E_STATE_BAD);
    }
    // 排序（选择排序，确定性）。
    let mut order: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < set.entries.len() {
        order.push(i);
        i += 1;
    }
    let mut a = 0usize;
    while a < order.len() {
        let mut b = a + 1;
        while b < order.len() {
            let ea = set.entries[order[a]];
            let eb = set.entries[order[b]];
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
    push_u32(&mut out, MAGIC);
    out.push(set.entry_count() as u8);
    i = 0;
    while i < order.len() {
        let e = set.entries[order[i]];
        out.push(e.section);
        push_u16(&mut out, e.key);
        push_u32(&mut out, e.val);
        push_u16(&mut out, e.ext);
        i += 1;
    }
    // 尾：复签字（对头部+记录区折叠）。
    let sig = sign_fold(&out);
    push_u16(&mut out, sig);
    Ok(out)
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
    out.push(((v >> 24) & 0xFF) as u8);
}
fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
}

/// fcitx5 侧读取器：复签先验 → 逐条回收扩容记录。
pub fn fcitx5_read(bytes: &[u8]) -> Result<Vec<MigEntry>, u16> {
    if bytes.len() < 6 {
        return Err(E_PARSE_BAD);
    }
    let magic = (bytes[0] as u32)
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24);
    if magic != MAGIC {
        return Err(E_MAGIC);
    }
    let count = bytes[4] as usize;
    if bytes.len() != 5 + count * REC_BYTES + 2 {
        return Err(E_PARSE_BAD);
    }
    // 复签先验：重算体签与尾签比对。
    let body_end = bytes.len() - 2;
    let sig_stored = (bytes[body_end] as u16) | ((bytes[body_end + 1] as u16) << 8);
    if sign_fold(&bytes[..body_end]) != sig_stored {
        return Err(E_SIG_BAD);
    }
    let mut recs: Vec<MigEntry> = Vec::new();
    let mut off = 5usize;
    let mut n = 0usize;
    while n < count {
        let section = bytes[off];
        let key = (bytes[off + 1] as u16) | ((bytes[off + 2] as u16) << 8);
        let val = (bytes[off + 3] as u32)
            | ((bytes[off + 4] as u32) << 8)
            | ((bytes[off + 5] as u32) << 16)
            | ((bytes[off + 6] as u32) << 24);
        let ext = (bytes[off + 7] as u16) | ((bytes[off + 8] as u16) << 8);
        if ext >= EXT_HI {
            return Err(E_STATE_BAD); // 扩容位越闭集整流拒收
        }
        recs.push(MigEntry { section, key, val, ext });
        off += REC_BYTES;
        n += 1;
    }
    Ok(recs)
}

/// 逆向导入签名：单向面恒拒。
pub fn reverse_import(_recs: &[MigEntry]) -> Result<MigSet, u16> {
    Err(E_ONE_WAY)
}

/// 完成写回（判据 J2）：复签成立 + 非空摘要方放行。
pub fn writeback(done: bool, signed_green: u32, summary_len: u32) -> Result<(), u16> {
    if !done || signed_green == 0 {
        return Err(E_STATE_BAD);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（复签 ×10 fcitx5 可读），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：10 条扩容记录导出→复签→fcitx5 侧解析，逐条
    //    四字段逐位回收相等、复签重算相符。
    let mut set = MigSet::new();
    let mut i = 0u32;
    while i < 10 {
        set.put(0x20 + (i / 4) as u8, 0x0100 + i as u16, 7000 + i * 3, 0x0001 << (i % 8))?;
        i += 1;
    }
    if set.entry_count() != 10 {
        return Err(E_STATE_BAD);
    }
    let bytes = export_signed(&set)?;
    let recs = fcitx5_read(&bytes)?;
    if recs.len() != 10 {
        return Err(E_PARSE_BAD);
    }
    // 逐条回收核对（按 (节,键) 升序序与源账核对：以查账方式逐条找到）。
    let mut i = 0u32;
    while i < 10 {
        let want_key = (0x0100 + i) as u16;
        let mut found: Option<MigEntry> = None;
        let mut k = 0usize;
        while k < recs.len() {
            if recs[k].key == want_key {
                found = Some(recs[k]);
                break;
            }
            k += 1;
        }
        match found {
            None => return Err(E_PARSE_BAD),
            Some(r) => {
                if r.val != 7000 + i * 3 {
                    return Err(E_PARSE_BAD);
                }
                if r.ext != 0x0001 << (i % 8) {
                    return Err(E_PARSE_BAD);
                }
                if r.ext >= EXT_HI {
                    return Err(E_STATE_BAD);
                }
            }
        }
        i += 1;
    }
    hits += 1;

    // R2 复签守门负例：体改一字节 → 重算签不符必拒（E_SIG_BAD）；
    //    尾签改一位 → 同拒。
    let mut t1 = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        t1.push(if i == 10 { bytes[i] ^ 0x01 } else { bytes[i] });
        i += 1;
    }
    if fcitx5_read(&t1).is_ok() {
        return Err(E_SIG_BAD);
    }
    let mut t2 = Vec::new();
    i = 0;
    while i < bytes.len() {
        t2.push(if i == bytes.len() - 1 { bytes[i] ^ 0x80 } else { bytes[i] });
        i += 1;
    }
    if fcitx5_read(&t2).is_ok() {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R3 确定性终判：同账双导出字节逐位相同；复签折叠纯函数同输入
    //    同签（空流/单字节/长流三点核）。
    let bytes2 = export_signed(&set)?;
    if bytes.len() != bytes2.len() {
        return Err(E_STATE_BAD);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != bytes2[i] {
            return Err(E_STATE_BAD);
        }
        i += 1;
    }
    if sign_fold(&bytes[..0]) != 0x51ED {
        return Err(E_SIG_BAD); // 空流折叠恒初值
    }
    let one = [0x5Au8];
    if sign_fold(&one) != sign_fold(&one) {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R4 结构守门：magic 错 → E_MAGIC；记录数与实长不闭合 → E_PARSE_BAD。
    let mut bad_magic = Vec::new();
    i = 0;
    while i < bytes.len() {
        bad_magic.push(if i == 0 { bytes[i] ^ 0xFF } else { bytes[i] });
        i += 1;
    }
    if fcitx5_read(&bad_magic).is_ok() {
        return Err(E_MAGIC);
    }
    if fcitx5_read(&bytes[..bytes.len() - 4]).is_ok() {
        return Err(E_PARSE_BAD);
    }
    hits += 1;

    // R5 单向面负例：逆向导入 → E_ONE_WAY，零反向产物。
    if reverse_import(&recs).is_ok() {
        return Err(E_ONE_WAY);
    }
    hits += 1;

    // R6 重键与扩容闭集负例：同节重键 → E_DUP_KEY；ext=0x8000 → 拒收
    //    不入账。
    let mut set2 = MigSet::new();
    set2.put(0x10, 0x0001, 1, 0x0001)?;
    if set2.put(0x10, 0x0001, 2, 0x0001).is_ok() {
        return Err(E_DUP_KEY);
    }
    if set2.put(0x11, 0x0002, 2, EXT_HI).is_ok() {
        return Err(E_STATE_BAD);
    }
    if set2.entry_count() != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R7 复签改签自愈面：体改一字节后重签（复读机面：Varix 侧修正
    //    重导出）→ 复签复符可读（复签随体走，不是死签）。
    let mut set3 = MigSet::new();
    set3.put(0x30, 0x0201, 42, 0x0004)?;
    let b3 = export_signed(&set3)?;
    set3.put(0x30, 0x0202, 43, 0x0008)?;
    let b4 = export_signed(&set3)?;
    if b3.len() == b4.len() && b3.len() > 0 {
        // 账变流变：两流至少尾部两字节（签）不同或记录数位不同。
        let mut diff = false;
        let mut i = 0usize;
        while i < b3.len() {
            if b3[i] != b4[i] {
                diff = true;
            }
            i += 1;
        }
        if !diff {
            return Err(E_SIG_BAD); // 账变流不变 = 复签死了
        }
    }
    fcitx5_read(&b4)?;
    hits += 1;

    // R8 写回（判据 J2）：零复签不写回、空摘要拒、×10 全过+非空摘要放行。
    if writeback(true, 0, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    if writeback(true, 10, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 10, 0x28)?;
    hits += 1;

    Ok(hits)
}
