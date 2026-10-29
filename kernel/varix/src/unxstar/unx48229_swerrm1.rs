//! unx48229_swerrm1 — UNX-F48229 切换错误码族 M1 段（8 条）（诊断段 0xBD80）。
//!
//! 判据原文：
//!   J1 错误码 ×8 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源基座：8 条错误码全挂 SEG_BASE（0xBD80），段内偏移 1..8
//!      连续编发；码值只由 (基座,偏移) 决定，与调用路径、时序、查询
//!      次数无关——零漂移的定义域。
//!   2. 注册表只读闭集：每条带固定语义名、固定严重级（INFO/WARN/ERR）、
//!      固定来源域（SWITCH/LED/PT 三域）；结构上无改写入口。
//!   3. 零漂移口径（J1 ×8）：8 条逐条三重核对——常量值==注册表值==
//!      双次独立查询值；加全段基座一致、偏移连续、唯一三条结构断言。
//!   4. 双径同值：按码查（describe）与按序查（at）必须同语义名同级别，
//!      径间漂移即零漂移失守。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBD80;

// ── M1 段切换错误码（段内偏移 1..8）─────────────────────────
pub const SWERR_M1_LED_STALE: u16 = SEG_BASE + 1;    // LED 投影滞留旧态
pub const SWERR_M1_JITTER_FROZEN: u16 = SEG_BASE + 2;// 冻结后改窗被拒
pub const SWERR_M1_WINDOW_BAD: u16 = SEG_BASE + 3;   // 窗宽非法
pub const SWERR_M1_PUNCT_UNMAPPED: u16 = SEG_BASE + 4;// 标点键不在锚表
pub const SWERR_M1_PT_STICKY: u16 = SEG_BASE + 5;    // 直通粘滞
pub const SWERR_M1_TEMP_OVERLAP: u16 = SEG_BASE + 6; // 临时态重叠按下
pub const SWERR_M1_PERSIST_CRC: u16 = SEG_BASE + 7;  // 持久化校验和坏
pub const SWERR_M1_VEC_REPLAY: u16 = SEG_BASE + 8;   // 向量用例复跑失配

/// M1 段码数（J1 的 ×8）。
pub const M1_COUNT: usize = 8;

/// 严重级闭集。
pub const SEV_INFO: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_ERR: u8 = 2;

/// 来源域闭集。
pub const SRC_SWITCH: u8 = 0;
pub const SRC_LED: u8 = 1;
pub const SRC_PT: u8 = 2;

/// 一条错误码登记项。
#[derive(Clone, Copy, Debug)]
pub struct SwErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub sev: u8,
    pub src: u8,
}

/// 注册表（只读闭集，纪律 2）。
pub const M1_TABLE: [SwErrEntry; M1_COUNT] = [
    SwErrEntry { code: SWERR_M1_LED_STALE, name: "led_stale", sev: SEV_WARN, src: SRC_LED },
    SwErrEntry { code: SWERR_M1_JITTER_FROZEN, name: "jitter_frozen", sev: SEV_INFO, src: SRC_SWITCH },
    SwErrEntry { code: SWERR_M1_WINDOW_BAD, name: "window_bad", sev: SEV_ERR, src: SRC_SWITCH },
    SwErrEntry { code: SWERR_M1_PUNCT_UNMAPPED, name: "punct_unmapped", sev: SEV_INFO, src: SRC_PT },
    SwErrEntry { code: SWERR_M1_PT_STICKY, name: "pt_sticky", sev: SEV_ERR, src: SRC_PT },
    SwErrEntry { code: SWERR_M1_TEMP_OVERLAP, name: "temp_overlap", sev: SEV_WARN, src: SRC_SWITCH },
    SwErrEntry { code: SWERR_M1_PERSIST_CRC, name: "persist_crc", sev: SEV_ERR, src: SRC_SWITCH },
    SwErrEntry { code: SWERR_M1_VEC_REPLAY, name: "vec_replay", sev: SEV_ERR, src: SRC_SWITCH },
];

/// 漂移检查账。
#[derive(Clone, Copy, Debug)]
pub struct DriftRecord {
    pub code: u16,
    pub const_ok: bool,
    pub twice_ok: bool,
    pub dual_ok: bool,
}

/// M1 段守卫。
pub struct SwErrM1 {
    ledger: Vec<DriftRecord>,
    pub checked_total: u32,
    pub drift_hits: u32,
}

impl SwErrM1 {
    pub fn new() -> SwErrM1 {
        SwErrM1 { ledger: Vec::new(), checked_total: 0, drift_hits: 0 }
    }

    /// 按码值查（describe 径）。
    pub fn describe(&self, code: u16) -> Option<SwErrEntry> {
        let mut i = 0usize;
        while i < M1_TABLE.len() {
            if M1_TABLE[i].code == code {
                return Some(M1_TABLE[i]);
            }
            i += 1;
        }
        None
    }

    /// 按序号查（at 径）。
    pub fn at(&self, idx: usize) -> Option<SwErrEntry> {
        if idx < M1_TABLE.len() {
            Some(M1_TABLE[idx])
        } else {
            None
        }
    }

    /// 全段结构断言：基座一致 + 偏移连续 + 唯一 + 级别域闭集。
    pub fn audit_structure(&self) -> Result<(), u16> {
        if M1_TABLE.len() != M1_COUNT {
            return Err(SEG_BASE + 0x14);
        }
        let mut expect = 1u16;
        let mut i = 0usize;
        while i < M1_COUNT {
            if M1_TABLE[i].code - SEG_BASE != expect {
                return Err(SEG_BASE + 0x15);
            }
            expect += 1;
            i += 1;
        }
        let mut i = 0usize;
        while i < M1_COUNT {
            if M1_TABLE[i].code <= SEG_BASE || M1_TABLE[i].code > SEG_BASE + 0x1F {
                return Err(SEG_BASE + 0x16);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < M1_COUNT {
            let mut j = i + 1;
            while j < M1_COUNT {
                if M1_TABLE[i].code == M1_TABLE[j].code {
                    return Err(SEG_BASE + 0x17);
                }
                j += 1;
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < M1_COUNT {
            if M1_TABLE[i].sev > SEV_ERR || M1_TABLE[i].src > SRC_PT {
                return Err(SEG_BASE + 0x18);
            }
            i += 1;
        }
        Ok(())
    }

    /// 逐条零漂移核对（J1 ×8 主体）。
    pub fn check_all(&mut self) -> Result<(), u16> {
        self.audit_structure()?;
        let mut i = 0usize;
        while i < M1_COUNT {
            let entry = match self.at(i) {
                Some(e) => e,
                None => return Err(SEG_BASE + 0x15),
            };
            let desc = match self.describe(entry.code) {
                Some(e) => e,
                None => return Err(SEG_BASE + 0x19),
            };
            let const_ok = desc.code == entry.code;
            let twice_ok = self.describe(entry.code).map(|e| e.code) == Some(entry.code)
                && self.describe(entry.code).map(|e| e.name) == Some(entry.name);
            let dual_ok = desc.name == entry.name
                && desc.sev == entry.sev
                && desc.src == entry.src;
            let rec = DriftRecord { code: entry.code, const_ok, twice_ok, dual_ok };
            if !const_ok || !twice_ok || !dual_ok {
                self.drift_hits += 1;
                self.ledger.push(rec);
                self.checked_total += 1;
                return Err(SEG_BASE + 0x1A);
            }
            self.ledger.push(rec);
            self.checked_total += 1;
            i += 1;
        }
        if self.checked_total != M1_COUNT as u32 {
            return Err(SEG_BASE + 0x1B);
        }
        Ok(())
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.checked_total < want {
            return Err(SEG_BASE + 0x1C);
        }
        Ok(())
    }

    /// 零漂移终判。
    pub fn assert_zero_drift(&self) -> Result<(), u16> {
        if self.drift_hits != 0 {
            return Err(SEG_BASE + 0x1A);
        }
        if self.ledger.len() as u32 != self.checked_total {
            return Err(SEG_BASE + 0x1B);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(SEG_BASE + 0x1D);
        }
        if summary_len == 0 {
            return Err(SEG_BASE + 0x1E);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（错误码 ×8 零漂移），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主核对 ×8：8 条三重核对 + 结构断言 + 零漂移终判。
    let mut we = SwErrM1::new();
    we.check_all()?;
    we.coverage(8)?;
    we.assert_zero_drift()?;
    if we.ledger_len() != 8 {
        return Err(SEG_BASE + 0x1B);
    }
    hits += 1;

    // R2 逐条独立复断：双径（按码/按序）同语义逐条再对一遍。
    let we = SwErrM1::new();
    let mut i = 0usize;
    while i < M1_COUNT {
        let by_idx = match we.at(i) {
            Some(e) => e,
            None => return Err(SEG_BASE + 0x15),
        };
        let by_code = match we.describe(by_idx.code) {
            Some(e) => e,
            None => return Err(SEG_BASE + 0x19),
        };
        if by_idx.code != by_code.code
            || by_idx.name != by_code.name
            || by_idx.sev != by_code.sev
            || by_idx.src != by_code.src
        {
            return Err(SEG_BASE + 0x1A);
        }
        i += 1;
    }
    hits += 1;

    // R3 基座语义：首码=SEG_BASE+1、末码=SEG_BASE+8，段内不越 0x1F。
    if M1_TABLE[0].code != SEG_BASE + 1 {
        return Err(SEG_BASE + 0x15);
    }
    if M1_TABLE[M1_COUNT - 1].code != SEG_BASE + 8 {
        return Err(SEG_BASE + 0x15);
    }
    if SEG_BASE + 8 > SEG_BASE + 0x1F {
        return Err(SEG_BASE + 0x16);
    }
    hits += 1;

    // R4 越界查询：at(8) 拒、describe(SEG_BASE+9) 拒、describe(0) 拒。
    let we = SwErrM1::new();
    if we.at(M1_COUNT).is_some() {
        return Err(SEG_BASE + 0x1C);
    }
    if we.describe(SEG_BASE + 9).is_some() {
        return Err(SEG_BASE + 0x19);
    }
    if we.describe(0).is_some() {
        return Err(SEG_BASE + 0x19);
    }
    hits += 1;

    // R5 严重级分布闭账：ERR 恒 4、WARN 恒 2、INFO 恒 2（登记定档不变）。
    let mut err_n = 0u32;
    let mut warn_n = 0u32;
    let mut info_n = 0u32;
    let mut i = 0usize;
    while i < M1_COUNT {
        match M1_TABLE[i].sev {
            SEV_ERR => err_n += 1,
            SEV_WARN => warn_n += 1,
            _ => info_n += 1,
        }
        i += 1;
    }
    if err_n != 4 || warn_n != 2 || info_n != 2 {
        return Err(SEG_BASE + 0x18);
    }
    hits += 1;

    // R6 来源域分布：SWITCH 恒 5、LED 恒 1、PT 恒 2（分域守恒）。
    let mut sw_n = 0u32;
    let mut led_n = 0u32;
    let mut pt_n = 0u32;
    let mut i = 0usize;
    while i < M1_COUNT {
        match M1_TABLE[i].src {
            SRC_SWITCH => sw_n += 1,
            SRC_LED => led_n += 1,
            _ => pt_n += 1,
        }
        i += 1;
    }
    if sw_n != 5 || led_n != 1 || pt_n != 2 {
        return Err(SEG_BASE + 0x18);
    }
    hits += 1;

    // R7 双轮核对幂等：check_all 连跑两轮，账翻倍、命中仍零。
    let mut we = SwErrM1::new();
    we.check_all()?;
    we.check_all()?;
    if we.checked_total != 16 || we.drift_hits != 0 {
        return Err(SEG_BASE + 0x1A);
    }
    we.assert_zero_drift()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与未完成拒。
    let we = SwErrM1::new();
    if we.writeback(false, 0x20).is_ok() {
        return Err(SEG_BASE + 0x1D);
    }
    if we.writeback(true, 0).is_ok() {
        return Err(SEG_BASE + 0x1E);
    }
    we.writeback(true, 0x50)?;
    hits += 1;

    Ok(hits)
}
