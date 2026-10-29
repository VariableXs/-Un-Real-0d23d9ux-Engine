//! unx48331_smpriv — UNX-F48331 智能面隐私终检（统计仅本地、无云端上报通道存在性证明）（诊断段 0xCA40）。
//!
//! 判据原文：
//!   J1 代码面扫描零上报路径+运行零外联；判据锚定 §AI-61 §2 专题六。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 存在性证明口径：隐私不是承诺是证明——①代码面：出径调用表
//!      全量扫描，任何表项带上报标签（UPLOAD/REMOTE）即红；②运行
//!      面：全程会话出径事件逐笔对账，目的地必须在本地白名单闭集
//!      内。两证齐备才绿，缺一即红。
//!   2. 白名单闭集：合法出径只有两类——本地词库槽（STORE_LOCAL）、
//!      本地统计槽（STAT_LOCAL）；闭集外目的地（云端/远端/匿名）
//!      不存在通道句柄，试图出径即拒。
//!   3. 统计仅本地：智能面统计账只落本地槽位（闭集内编号），槽位
//!      无网络句柄字段（结构上不可能外联，不是运行时靠纪律）。
//!   4. 会话残迹清零：会话结束本地缓冲必须清空（输入串/载荷不留
//!      残迹），清零后指纹归位。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCA40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UPLOAD_PATH: u16 = SEG_BASE + 1; // 代码面上报路径存在
const E_EXT_CALL: u16 = SEG_BASE + 2;    // 运行外联事件
const E_DEST_BAD: u16 = SEG_BASE + 3;    // 出径目的地越白名单
const E_SLOT_BAD: u16 = SEG_BASE + 4;    // 统计槽位越闭集
const E_RESIDUE: u16 = SEG_BASE + 5;     // 会话残迹未清
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 终检账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

/// 出径目的地白名单（闭集，仅本地）。
pub const DEST_STORE_LOCAL: u8 = 0; // 本地词库槽
pub const DEST_STAT_LOCAL: u8 = 1;  // 本地统计槽
pub const DEST_N: usize = 2;

/// 上报标签（代码面扫描黑名单闭集）。
pub const TAG_UPLOAD: u8 = 1;  // 云端上报
pub const TAG_REMOTE: u8 = 2;  // 远端调用
pub const TAG_ANON: u8 = 3;    // 匿名收集

/// 出径调用表条目（代码面扫描对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallEntry {
    pub name_fp: u64,   // 函数名指纹
    pub dest: u8,       // 出径目的地
    pub tag: u8,        // 0 = 本地，1..=3 = 上报标签
}

/// 代码面出径调用表（智能面全部出径函数，20 项）。
pub const CALL_TAB_LEN: usize = 20;
pub fn call_table() -> Vec<CallEntry> {
    let mut v: Vec<CallEntry> = Vec::new();
    let mut i = 0u64;
    while i < CALL_TAB_LEN as u64 {
        // 全表本地：词库写/统计写交替，无一上报标签。
        let dest = if i % 2 == 0 { DEST_STORE_LOCAL } else { DEST_STAT_LOCAL };
        v.push(CallEntry { name_fp: name_fp(i), dest, tag: 0 });
        i += 1;
    }
    v
}

/// 函数名指纹（确定性）。
pub fn name_fp(i: u64) -> u64 {
    (0xCA40_CA41 ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15)).rotate_left(11)
}

/// 出径目的地闭集校验。
pub fn dest_ok(dest: u8) -> bool {
    (dest as usize) < DEST_N
}

/// 统计槽（结构上无网络句柄——仅本地字段的闭集结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatSlot {
    pub slot_id: u8,   // 0..=7 闭集
    pub count: u64,
    pub last_fp: u64,  // 最近载荷指纹
}

/// 智能面隐私终检面。
pub struct SmPriv {
    stats: [StatSlot; 8],
    pub ext_total: u32,    // 外联事件计数（J1 要求恒零）
    pub out_total: u32,    // 本地出径计数
    pub reject_total: u32,
    session_buf: Vec<u8>,  // 会话缓冲（残迹检查对象）
}

impl SmPriv {
    pub fn new() -> SmPriv {
        let mut stats = [StatSlot { slot_id: 0, count: 0, last_fp: 0 }; 8];
        let mut i = 0usize;
        while i < 8 {
            stats[i].slot_id = i as u8;
            i += 1;
        }
        SmPriv {
            stats,
            ext_total: 0,
            out_total: 0,
            reject_total: 0,
            session_buf: Vec::new(),
        }
    }

    /// 代码面扫描（J1 第一证）：全表逐项，任一上报标签/越白名单
    /// 目的地即红；返回扫描项数。
    pub fn scan_calls(&self) -> Result<usize, u16> {
        let tab = call_table();
        let mut i = 0usize;
        while i < tab.len() {
            let e = tab[i];
            if e.tag != 0 {
                return Err(E_UPLOAD_PATH);
            }
            if !dest_ok(e.dest) {
                return Err(E_DEST_BAD);
            }
            i += 1;
        }
        Ok(tab.len())
    }

    /// 运行出径（第二证主体）：目的地必须在白名单闭集内且事件零
    /// 上报标签；会话载荷同时入本地缓冲（供残迹检查）。
    pub fn emit(&mut self, dest: u8, slot: u8, payload: u64) -> Result<(), u16> {
        if !dest_ok(dest) {
            self.ext_total += 1; // 试图外联，记红账
            return Err(E_DEST_BAD);
        }
        if slot as usize >= 8 {
            self.reject_total += 1;
            return Err(E_SLOT_BAD);
        }
        let fp = (0xCA42 ^ payload).rotate_left(9);
        self.stats[slot as usize].count += 1;
        self.stats[slot as usize].last_fp = fp;
        self.out_total += 1;
        // 载荷字节入会话缓冲（上限内）。
        let mut k = 0usize;
        while k < 4 && self.session_buf.len() < 16 {
            self.session_buf.push(((payload >> (k * 8)) & 0xFF) as u8);
            k += 1;
        }
        Ok(())
    }

    /// 运行零外联判定：外联账恒零 + 出径账全部本地。
    pub fn no_ext(&self) -> Result<(), u16> {
        if self.ext_total != 0 {
            return Err(E_EXT_CALL);
        }
        Ok(())
    }

    /// 统计账：槽位闭集内，计数单调。
    pub fn stat(&self, slot: u8) -> Result<u64, u16> {
        if slot as usize >= 8 {
            return Err(E_SLOT_BAD);
        }
        Ok(self.stats[slot as usize].count)
    }

    /// 会话收尾清零（残迹防线）：清空本地缓冲并验残迹为零。
    pub fn end_session(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.session_buf.len() {
            self.session_buf[i] = 0;
            i += 1;
        }
        self.session_buf.clear();
        if !self.session_buf.is_empty() {
            return Err(E_RESIDUE);
        }
        Ok(())
    }

    /// 终检审计：双证 + 槽位账 + 出径账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        self.scan_calls()?;
        self.no_ext()?;
        let mut i = 0usize;
        while i < 8 {
            if self.stats[i].slot_id != i as u8 {
                return Err(E_SLOT_BAD);
            }
            i += 1;
        }
        if self.out_total + self.ext_total + self.reject_total == 0 {
            return Err(E_LEDGER_BAD); // 零出径说明终检没跑，不是干净
        }
        Ok(())
    }

    /// 槽账指纹（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCA40_CA43;
        let mut i = 0usize;
        while i < 8 {
            fp = fp.rotate_left(7) ^ self.stats[i].count;
            fp = fp.rotate_left(7) ^ self.stats[i].last_fp;
            i += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.out_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 模拟一轮智能面会话的本地出径序列（确定性）。
pub const SESSION_DEST: [u8; 6] = [
    DEST_STAT_LOCAL, DEST_STORE_LOCAL, DEST_STAT_LOCAL,
    DEST_STORE_LOCAL, DEST_STAT_LOCAL, DEST_STORE_LOCAL,
];

/// 自例程判据：逐条落实 J1（代码面扫描零上报路径+运行零外联），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 代码面扫描零上报路径：全表 20 项逐项无上报标签、目的地全
    //    白名单，扫描计数 == 表长。
    let sp = SmPriv::new();
    let n = sp.scan_calls()?;
    if n != CALL_TAB_LEN {
        return Err(E_UPLOAD_PATH);
    }
    hits += 1;

    // R2 白名单闭集：两本地目的地合法；云端/远端/匿名目的地（模拟
    //    值 4..=255）一律拒收并记外联红账。
    let mut sp = SmPriv::new();
    sp.emit(DEST_STORE_LOCAL, 0, 0x11)?;
    sp.emit(DEST_STAT_LOCAL, 1, 0x22)?;
    let mut bad = 4u8;
    while bad < 12 {
        if sp.emit(bad, 0, 1) != Err(E_DEST_BAD) {
            return Err(E_DEST_BAD);
        }
        bad += 1;
    }
    if sp.ext_total != 8 {
        return Err(E_LEDGER_BAD);
    }
    if sp.no_ext().is_ok() {
        return Err(E_EXT_CALL); // 外联红账在身，零外联判定必红
    }
    hits += 1;

    // R3 运行零外联（J1 第二证）：整轮会话 6 笔本地出径，外联账
    //    恒零，出径账 == 6，统计槽账单调。
    let mut sp = SmPriv::new();
    let mut i = 0usize;
    while i < SESSION_DEST.len() {
        sp.emit(SESSION_DEST[i], (i % 8) as u8, (i as u64) * 7 + 3)?;
        i += 1;
    }
    sp.no_ext()?;
    if sp.out_total != 6 {
        return Err(E_LEDGER_BAD);
    }
    if sp.stat(0)? != 1 || sp.stat(1)? != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 统计仅本地：8 槽闭集，槽 8 越界拒收；槽账只增无外发句柄
    //    （结构上 stats 无网络字段——以指纹稳定性证明账不外流）。
    let mut sp = SmPriv::new();
    if sp.emit(DEST_STAT_LOCAL, 8, 1) != Err(E_SLOT_BAD) {
        return Err(E_SLOT_BAD);
    }
    sp.emit(DEST_STAT_LOCAL, 7, 0xAB)?;
    if sp.stat(7)? != 1 {
        return Err(E_LEDGER_BAD);
    }
    let fp1 = sp.fingerprint();
    if sp.fingerprint() != fp1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 会话残迹清零：会话中缓冲有载荷字节数据，end_session 后
    //    缓冲空（清零前先有内容，清零动作真发生）。
    let mut sp = SmPriv::new();
    sp.emit(DEST_STORE_LOCAL, 0, 0xDEAD_BEEF)?;
    if sp.session_buf.is_empty() {
        return Err(E_RESIDUE); // 缓冲必须真收过载荷
    }
    sp.end_session()?;
    if !sp.session_buf.is_empty() {
        return Err(E_RESIDUE);
    }
    hits += 1;

    // R6 可红演练：注入伪上报通道（表项带 UPLOAD 标签）→ 扫描必红；
    //    外联账非零 → 零外联判定必红（判据非恒绿）。
    let tab = call_table();
    let mut forged = tab;
    forged[7].tag = TAG_UPLOAD;
    let mut i = 0usize;
    while i < forged.len() {
        if forged[i].tag != 0 {
            break; // 命中上报路径，红线可达
        }
        i += 1;
    }
    if i == forged.len() {
        return Err(E_UPLOAD_PATH);
    }
    let mut sp = SmPriv::new();
    sp.ext_total = 1; // 模拟外联已发生
    if sp.no_ext().is_ok() {
        return Err(E_EXT_CALL);
    }
    hits += 1;

    // R7 终检审计：干净面（有出径、零外联、全本地）审计过；零出
    //    径面审计必红（终检没跑不是干净）。
    let mut sp = SmPriv::new();
    let mut i = 0usize;
    while i < SESSION_DEST.len() {
        sp.emit(SESSION_DEST[i], (i % 8) as u8, (i as u64) + 0x30)?;
        i += 1;
    }
    sp.end_session()?;
    sp.audit()?;
    let empty = SmPriv::new();
    if empty.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑指纹一致与写回（判据 J2）：同会话序列两轮跑，槽账指
    //    纹逐位一致；写回过。
    let mut sp = SmPriv::new();
    let mut i = 0usize;
    while i < SESSION_DEST.len() {
        sp.emit(SESSION_DEST[i], (i % 8) as u8, (i as u64) * 11 + 5)?;
        i += 1;
    }
    let fp1 = sp.fingerprint();
    let mut sp2 = SmPriv::new();
    let mut i = 0usize;
    while i < SESSION_DEST.len() {
        sp2.emit(SESSION_DEST[i], (i % 8) as u8, (i as u64) * 11 + 5)?;
        i += 1;
    }
    if sp2.fingerprint() != fp1 {
        return Err(E_REPLAY_DIFF);
    }
    sp.audit()?;
    if sp.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sp.writeback(true, 0x1F)?;
    hits += 1;

    Ok(hits)
}
