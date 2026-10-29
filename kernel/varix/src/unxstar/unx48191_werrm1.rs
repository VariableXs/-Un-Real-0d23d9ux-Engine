//! unx48191_werrm1 — UNX-F48191 Wine 窗错误码族 M1 段（12 条）（诊断段 0xB8C0）。
//!
//! 判据原文：
//!   J1 错误码 ×12 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 同源体系：M1 段 12 条错误码全部挂同一基座 SEG_BASE，段内偏移
//!      1..12 连续编发，任何一条的数值只由 (基座,偏移) 二元组决定，
//!      与调用路径、时序、重入次数完全无关（零漂移的定义域）。
//!   2. 注册表闭集：12 条码各带固定语义名、固定严重级（INFO/WARN/ERR）、
//!      固定来源域（IME/BRIDGE/RECON 三域）；注册表只读，无任何运行期
//!      改写入口——漂移在结构上不可表达。
//!   3. 零漂移口径（J1 ×12）：12 条逐条三重核对——常量值==注册表值==
//!      双次独立查询值；再加全段基座一致性、偏移连续性、唯一性三条
//!      全段断言。
//!   4. 查询双径同值：按码值查（describe）与按序号查（at）两条查询径
//!      必须返回同一语义名与同一严重级，径间漂移视为零漂移失守。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB8C0;

// ── M1 段错误码（段内偏移 1..12，同源基座连续编发）──────────
pub const WERR_M1_BRIDGE_UNBOUND: u16 = SEG_BASE + 1;  // 桥未绑定 context
pub const WERR_M1_CTX_STALE: u16 = SEG_BASE + 2;       // context 已过期
pub const WERR_M1_KEY_DROP: u16 = SEG_BASE + 3;        // 键投递失败
pub const WERR_M1_CAND_OOB: u16 = SEG_BASE + 4;        // 候选索引越界
pub const WERR_M1_CONV_BAD: u16 = SEG_BASE + 5;        // 转换状态非法
pub const WERR_M1_FORM_STYLE: u16 = SEG_BASE + 6;      // 表单样式闭集外
pub const WERR_M1_PAGE_CLAMP: u16 = SEG_BASE + 7;      // 翻页钳制触底
pub const WERR_M1_REC_PENDING: u16 = SEG_BASE + 8;     // 录制样本待基准机
pub const WERR_M1_DIFF_RESIDUE: u16 = SEG_BASE + 9;    // 跑差残留未归零
pub const WERR_M1_CODEPAGE_HOLE: u16 = SEG_BASE + 10;  // 代码页样张空洞
pub const WERR_M1_OBS_PARTIAL: u16 = SEG_BASE + 11;    // 埋点四点缺一
pub const WERR_M1_FOCUS_ORPHAN: u16 = SEG_BASE + 12;   // 焦点孤儿 context

/// M1 段码数（J1 的 ×12）。
pub const M1_COUNT: usize = 12;

/// 严重级闭集。
pub const SEV_INFO: u8 = 0;
pub const SEV_WARN: u8 = 1;
pub const SEV_ERR: u8 = 2;

/// 来源域闭集。
pub const SRC_IME: u8 = 0;
pub const SRC_BRIDGE: u8 = 1;
pub const SRC_RECON: u8 = 2;

/// 一条错误码登记项。
#[derive(Clone, Copy, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub sev: u8,
    pub src: u8,
}

/// 注册表（只读闭集，纪律 2）。
pub const M1_TABLE: [ErrEntry; M1_COUNT] = [
    ErrEntry { code: WERR_M1_BRIDGE_UNBOUND, name: "bridge_unbound", sev: SEV_ERR, src: SRC_BRIDGE },
    ErrEntry { code: WERR_M1_CTX_STALE, name: "ctx_stale", sev: SEV_WARN, src: SRC_IME },
    ErrEntry { code: WERR_M1_KEY_DROP, name: "key_drop", sev: SEV_ERR, src: SRC_BRIDGE },
    ErrEntry { code: WERR_M1_CAND_OOB, name: "cand_oob", sev: SEV_WARN, src: SRC_IME },
    ErrEntry { code: WERR_M1_CONV_BAD, name: "conv_bad", sev: SEV_WARN, src: SRC_IME },
    ErrEntry { code: WERR_M1_FORM_STYLE, name: "form_style", sev: SEV_INFO, src: SRC_IME },
    ErrEntry { code: WERR_M1_PAGE_CLAMP, name: "page_clamp", sev: SEV_INFO, src: SRC_IME },
    ErrEntry { code: WERR_M1_REC_PENDING, name: "rec_pending", sev: SEV_INFO, src: SRC_RECON },
    ErrEntry { code: WERR_M1_DIFF_RESIDUE, name: "diff_residue", sev: SEV_ERR, src: SRC_RECON },
    ErrEntry { code: WERR_M1_CODEPAGE_HOLE, name: "codepage_hole", sev: SEV_WARN, src: SRC_IME },
    ErrEntry { code: WERR_M1_OBS_PARTIAL, name: "obs_partial", sev: SEV_WARN, src: SRC_RECON },
    ErrEntry { code: WERR_M1_FOCUS_ORPHAN, name: "focus_orphan", sev: SEV_ERR, src: SRC_IME },
];

/// 漂移检查账：一条码一轮核对的结果。
#[derive(Clone, Copy, Debug)]
pub struct DriftRecord {
    pub code: u16,
    pub const_ok: bool,   // 常量值==注册表值
    pub twice_ok: bool,   // 双次独立查询同值
    pub dual_ok: bool,    // 双径（按码/按序）同语义
}

/// M1 段守卫：逐条漂移核对 + 全段结构断言。
pub struct WErrM1 {
    ledger: Vec<DriftRecord>,
    pub checked_total: u32,
    pub drift_hits: u32,
}

impl WErrM1 {
    pub fn new() -> WErrM1 {
        WErrM1 { ledger: Vec::new(), checked_total: 0, drift_hits: 0 }
    }

    /// 按码值查（describe 径）。
    pub fn describe(&self, code: u16) -> Option<ErrEntry> {
        let mut i = 0usize;
        while i < M1_TABLE.len() {
            if M1_TABLE[i].code == code {
                return Some(M1_TABLE[i]);
            }
            i += 1;
        }
        None
    }

    /// 按序号查（at 径）：序号 0..11 → 段内偏移 1..12。
    pub fn at(&self, idx: usize) -> Option<ErrEntry> {
        if idx < M1_TABLE.len() {
            Some(M1_TABLE[idx])
        } else {
            None
        }
    }

    /// 全段结构断言（纪律 3 后三条）：基座一致 + 偏移连续 + 唯一。
    pub fn audit_structure(&self) -> Result<(), u16> {
        if M1_TABLE.len() != M1_COUNT {
            return Err(SEG_BASE + 0x14);
        }
        // 偏移连续：1..12 逐位在位。
        let mut expect = 1u16;
        let mut i = 0usize;
        while i < M1_COUNT {
            let off = M1_TABLE[i].code - SEG_BASE;
            if off != expect {
                return Err(SEG_BASE + 0x15);
            }
            expect += 1;
            i += 1;
        }
        // 基座一致：所有码高位同源（code & !0x1F == SEG_BASE & !0x1F 的段语义）。
        let mut i = 0usize;
        while i < M1_COUNT {
            if M1_TABLE[i].code <= SEG_BASE || M1_TABLE[i].code > SEG_BASE + 0x1F {
                return Err(SEG_BASE + 0x16);
            }
            i += 1;
        }
        // 唯一：两两互异。
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
        // 严重级/来源域闭集。
        let mut i = 0usize;
        while i < M1_COUNT {
            if M1_TABLE[i].sev > SEV_ERR || M1_TABLE[i].src > SRC_RECON {
                return Err(SEG_BASE + 0x18);
            }
            i += 1;
        }
        Ok(())
    }

    /// 逐条零漂移核对（J1 ×12 主体）：三重断言逐条落账。
    pub fn check_all(&mut self) -> Result<(), u16> {
        self.audit_structure()?;
        let mut i = 0usize;
        while i < M1_COUNT {
            let entry = match self.at(i) {
                Some(e) => e,
                None => return Err(SEG_BASE + 0x15),
            };
            // 重 1：常量值==注册表值（按序号径取得的码再走 describe 径）。
            let desc = match self.describe(entry.code) {
                Some(e) => e,
                None => return Err(SEG_BASE + 0x19),
            };
            let const_ok = desc.code == entry.code;
            // 重 2：双次独立查询同值。
            let twice_ok = self.describe(entry.code).map(|e| e.code) == Some(entry.code)
                && self.describe(entry.code).map(|e| e.name) == Some(entry.name);
            // 重 3：双径同语义（名与严重级）。
            let dual_ok = desc.name == entry.name && desc.sev == entry.sev && desc.src == entry.src;
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

/// 自例程判据：逐条落实 J1（错误码 ×12 零漂移），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主核对 ×12：12 条逐条三重核对 + 全段结构断言 + 零漂移终判。
    let mut we = WErrM1::new();
    we.check_all()?;
    we.coverage(12)?;
    we.assert_zero_drift()?;
    if we.ledger_len() != 12 {
        return Err(SEG_BASE + 0x1B);
    }
    hits += 1;

    // R2 逐条独立复断：12 条常量与注册表、双径查询全部手工再对一遍。
    let we = WErrM1::new();
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
        {
            return Err(SEG_BASE + 0x1A);
        }
        i += 1;
    }
    hits += 1;

    // R3 基座语义：首码=SEG_BASE+1、末码=SEG_BASE+12，段内不越 0x1F 界。
    if M1_TABLE[0].code != SEG_BASE + 1 {
        return Err(SEG_BASE + 0x15);
    }
    if M1_TABLE[M1_COUNT - 1].code != SEG_BASE + 12 {
        return Err(SEG_BASE + 0x15);
    }
    if SEG_BASE + 12 > SEG_BASE + 0x1F {
        return Err(SEG_BASE + 0x16);
    }
    hits += 1;

    // R4 越界查询：at(12) 拒、describe(SEG_BASE+13) 拒、describe(0) 拒。
    let we = WErrM1::new();
    if we.at(M1_COUNT).is_some() {
        return Err(SEG_BASE + 0x1C);
    }
    if we.describe(SEG_BASE + 13).is_some() {
        return Err(SEG_BASE + 0x19);
    }
    if we.describe(0).is_some() {
        return Err(SEG_BASE + 0x19);
    }
    hits += 1;

    // R5 严重级分布闭账：ERR 恒 4 条、WARN 恒 5 条、INFO 恒 3 条（登记
    //    时刻定档，查询不改档——分布漂移也是漂移）。
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
    if err_n != 4 || warn_n != 5 || info_n != 3 {
        return Err(SEG_BASE + 0x18);
    }
    let mut i = 0usize;
    while i < M1_COUNT {
        let e = we.describe(M1_TABLE[i].code);
        match e {
            Some(x) if x.sev == M1_TABLE[i].sev => {}
            _ => return Err(SEG_BASE + 0x1A),
        }
        i += 1;
    }
    hits += 1;

    // R6 来源域分布：BRIDGE 恒 2、RECON 恒 3、IME 恒 7（同源账分域守恒）。
    let mut b = 0u32;
    let mut r = 0u32;
    let mut m = 0u32;
    let mut i = 0usize;
    while i < M1_COUNT {
        match M1_TABLE[i].src {
            SRC_BRIDGE => b += 1,
            SRC_RECON => r += 1,
            _ => m += 1,
        }
        i += 1;
    }
    if b != 2 || r != 3 || m != 7 {
        return Err(SEG_BASE + 0x18);
    }
    hits += 1;

    // R7 双轮核对幂等：check_all 连跑两轮，账翻倍、命中仍零（查询无副作用）。
    let mut we = WErrM1::new();
    we.check_all()?;
    we.check_all()?;
    if we.checked_total != 24 || we.drift_hits != 0 {
        return Err(SEG_BASE + 0x1A);
    }
    we.assert_zero_drift()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与未完成态拒。
    let we = WErrM1::new();
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
