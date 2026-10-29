//! unx48190_fwhwstat — UNX-F48190 Wine 窗全角/半角状态同步（ImmGetConversionStatus 语义）（诊断段 0xB8A0）。
//!
//! 判据原文：
//!   J1 同步 ×100 一致；判据锚定 §AI-61 §2 专题三类型二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 转换状态双字段：conversion mode（全角/半角、Native/Alphanumeric、
//!      Roman 开关）与 sentence mode（无/一般/名前），按 IMM 位形落账，
//!      全角位 FULLSHAPE=0x0008、Native 位=0x0001、Roman 位=0x0010。
//!   2. 双端同步模型：引擎端（set 侧）与查询端（get 侧）经共享状态槽
//!      同步，槽带单调版本号；set 写槽必升版，get 读槽返回 (mode,sent,ver)，
//!      两侧见到的版本漂移即失步。
//!   3. 同步一致性口径（J1 ×100）：100 组 set→get 对拍，get 出参必须
//!      与最近一次 set 入参逐位相等（含全/半角开关翻转序列），版本号
//!      严格单调递增且 get 见到的版本 == set 后版本。
//!   4. 开关位闭集：mode 只允许闭集位组合（0x0000 或含 Native/Roman/
//!      FULLSHAPE 的合法叠加），sentence 闭集 4 档；越位写拒收且槽不升版。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB8A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_BAD_MODE: u16 = SEG_BASE + 1;   // mode 位闭集外
const E_BAD_SENT: u16 = SEG_BASE + 2;   // sentence 闭集外
const E_SYNC_MISS: u16 = SEG_BASE + 3;  // set→get 对拍不一致
const E_VER_DRIFT: u16 = SEG_BASE + 4;  // 版本号漂移/不单调
const E_STALE_READ: u16 = SEG_BASE + 5; // get 读到旧版本
const E_PAIR_MISS: u16 = SEG_BASE + 6;  // ×100 对拍不足
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

// ── conversion mode 位（IMM 口径）───────────────────────────
pub const IME_CMODE_NATIVE: u32 = 0x0001;
pub const IME_CMODE_KATAKANA: u32 = 0x0002;
pub const IME_CMODE_FULLSHAPE: u32 = 0x0008; // 全角位
pub const IME_CMODE_ROMAN: u32 = 0x0010;
pub const IME_CMODE_CHARCODE: u32 = 0x0020;

// ── sentence mode 闭集 4 档 ─────────────────────────────────
pub const IME_SMODE_NONE: u32 = 0x0000;
pub const IME_SMODE_PLAURALCLAUSE: u32 = 0x0001;
pub const IME_SMODE_SINGLECONVERT: u32 = 0x0002;
pub const IME_SMODE_AUTOMATIC: u32 = 0x0004;

pub const SENT_SET: [u32; 4] = [
    IME_SMODE_NONE,
    IME_SMODE_PLAURALCLAUSE,
    IME_SMODE_SINGLECONVERT,
    IME_SMODE_AUTOMATIC,
];

/// 状态槽：共享的 (mode, sentence, version) 三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StatusSlot {
    pub mode: u32,
    pub sentence: u32,
    pub version: u64,
}

/// 同步枢纽：引擎端 set / 查询端 get / 对拍账。
pub struct FwHwStat {
    slot: StatusSlot,
    pub set_total: u32,
    pub get_total: u32,
    pub mismatches: u32,
    pairs: Vec<(u32, u32, u64)>, // 对拍账：(set 的 mode, get 的 mode, 版本)
}

impl FwHwStat {
    pub fn new() -> FwHwStat {
        FwHwStat {
            slot: StatusSlot { mode: 0, sentence: IME_SMODE_NONE, version: 0 },
            set_total: 0,
            get_total: 0,
            mismatches: 0,
            pairs: Vec::new(),
        }
    }

    fn mode_legal(mode: u32) -> bool {
        // 闭集位掩：超出已知位即非法；KATAKANA 仅可与 Native 同现。
        let known = IME_CMODE_NATIVE
            | IME_CMODE_KATAKANA
            | IME_CMODE_FULLSHAPE
            | IME_CMODE_ROMAN
            | IME_CMODE_CHARCODE;
        if mode & !known != 0 {
            return false;
        }
        if mode & IME_CMODE_KATAKANA != 0 && mode & IME_CMODE_NATIVE == 0 {
            return false;
        }
        true
    }

    fn sent_legal(s: u32) -> bool {
        let mut i = 0usize;
        while i < SENT_SET.len() {
            if SENT_SET[i] == s {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 引擎端写状态（ImmSetConversionStatus 语义）：闭集校验后写槽升版。
    pub fn set_status(&mut self, mode: u32, sentence: u32) -> Result<u64, u16> {
        if !Self::mode_legal(mode) {
            return Err(E_BAD_MODE);
        }
        if !Self::sent_legal(sentence) {
            return Err(E_BAD_SENT);
        }
        self.slot.mode = mode;
        self.slot.sentence = sentence;
        self.slot.version += 1;
        self.set_total += 1;
        Ok(self.slot.version)
    }

    /// 查询端读状态（ImmGetConversionStatus 语义）：返回槽快照。
    pub fn get_status(&self) -> StatusSlot {
        self.slot
    }

    /// 对拍一轮：set 入参 → get 出参逐位核对，对拍账落一条（J1 主体）。
    pub fn sync_pair(&mut self, mode: u32, sentence: u32) -> Result<(), u16> {
        let v = self.set_status(mode, sentence)?;
        let snap = self.get_status();
        self.get_total += 1;
        if snap.mode != mode || snap.sentence != sentence {
            self.mismatches += 1;
            return Err(E_SYNC_MISS);
        }
        if snap.version != v {
            return Err(E_VER_DRIFT);
        }
        self.pairs.push((mode, snap.mode, snap.version));
        Ok(())
    }

    /// 全角/半角翻转一步：保留其他位，仅翻 FULLSHAPE，返回新 mode。
    pub fn toggle_fullshape(&mut self) -> Result<u32, u16> {
        let cur = self.get_status().mode;
        let next = cur ^ IME_CMODE_FULLSHAPE;
        self.set_status(next, self.get_status().sentence)?;
        Ok(next)
    }

    /// 全角判定辅助。
    pub fn is_fullwidth(&self) -> bool {
        self.get_status().mode & IME_CMODE_FULLSHAPE != 0
    }

    /// 对拍审计：账条数与计数守恒、版本严格单调、无失步记录。
    pub fn audit(&self) -> Result<(), u16> {
        if self.pairs.len() as u32 != self.get_total {
            return Err(E_PAIR_MISS);
        }
        if self.mismatches != 0 {
            return Err(E_SYNC_MISS);
        }
        let mut i = 1usize;
        while i < self.pairs.len() {
            if self.pairs[i].2 <= self.pairs[i - 1].2 {
                return Err(E_VER_DRIFT);
            }
            if self.pairs[i].0 != self.pairs[i].1 {
                return Err(E_SYNC_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 对拍覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.get_total < want {
            return Err(E_PAIR_MISS);
        }
        Ok(())
    }

    pub fn slot_version(&self) -> u64 {
        self.slot.version
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_SYNC_MISS);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（同步 ×100 一致），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对拍 ×100：100 组 set→get，模式在全/半角、Native/Roman 组合
    //    中确定性轮转，逐组逐位一致，版本严格单调。
    let mut fs = FwHwStat::new();
    let modes: [u32; 8] = [
        IME_CMODE_NATIVE,
        IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE,
        IME_CMODE_NATIVE | IME_CMODE_ROMAN,
        IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE | IME_CMODE_ROMAN,
        0,
        IME_CMODE_FULLSHAPE,
        IME_CMODE_ROMAN,
        IME_CMODE_NATIVE | IME_CMODE_KATAKANA | IME_CMODE_FULLSHAPE,
    ];
    let mut i = 0u32;
    while i < 100 {
        let m = modes[(i % 8) as usize];
        let s = SENT_SET[(i % 4) as usize];
        fs.sync_pair(m, s)?;
        if fs.is_fullwidth() != (m & IME_CMODE_FULLSHAPE != 0) {
            return Err(E_SYNC_MISS);
        }
        i += 1;
    }
    fs.coverage(100)?;
    fs.audit()?;
    if fs.slot_version() != 100 {
        return Err(E_VER_DRIFT);
    }
    hits += 1;

    // R2 全/半角翻转链：从半角 Native 起连翻 4 次，奇偶态交替且逐次同步。
    let mut fs = FwHwStat::new();
    fs.set_status(IME_CMODE_NATIVE, IME_SMODE_AUTOMATIC)?;
    if fs.is_fullwidth() {
        return Err(E_SYNC_MISS);
    }
    let mut i = 0u32;
    while i < 4 {
        let m = fs.toggle_fullshape()?;
        let snap = fs.get_status();
        if snap.mode != m {
            return Err(E_SYNC_MISS);
        }
        if fs.is_fullwidth() != (i % 2 == 0) {
            return Err(E_SYNC_MISS);
        }
        i += 1;
    }
    fs.audit()?;
    hits += 1;

    // R3 位闭集拒收：越知位 0x8000 拒收、KATAKANA 孤立位拒收，槽不升版。
    let mut fs = FwHwStat::new();
    let v0 = fs.slot_version();
    if fs.set_status(0x8000, IME_SMODE_NONE).is_ok() {
        return Err(E_BAD_MODE);
    }
    if fs.set_status(IME_CMODE_KATAKANA, IME_SMODE_NONE).is_ok() {
        return Err(E_BAD_MODE);
    }
    if fs.set_status(IME_CMODE_NATIVE, 0x0100).is_ok() {
        return Err(E_BAD_SENT);
    }
    if fs.slot_version() != v0 || fs.set_total != 0 {
        return Err(E_VER_DRIFT); // 拒收不升版
    }
    hits += 1;

    // R4 版本单调：连续 set 版本 1..N 严格递增，get 恒见最新版。
    let mut fs = FwHwStat::new();
    let mut i = 0u32;
    while i < 20 {
        let v = fs.set_status(IME_CMODE_NATIVE, SENT_SET[(i % 4) as usize])?;
        if v != i as u64 + 1 {
            return Err(E_VER_DRIFT);
        }
        if fs.get_status().version != v {
            return Err(E_STALE_READ);
        }
        i += 1;
    }
    fs.audit()?;
    hits += 1;

    // R5 失步抓拍：直接篡改对拍口径（get 后误记账）由 audit 揭示——
    //    构造一条 mode 不一致的对拍账，审计必红。
    let mut fs = FwHwStat::new();
    fs.sync_pair(IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE, IME_SMODE_NONE)?;
    fs.pairs.push((0x1234, 0x5678, fs.slot_version() + 1)); // 伪造失步账
    if fs.audit().is_ok() {
        return Err(E_SYNC_MISS);
    }
    hits += 1;

    // R6 快照语义：get 返回副本，外部持有快照不受后续 set 影响。
    let mut fs = FwHwStat::new();
    fs.set_status(IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE, IME_SMODE_SINGLECONVERT)?;
    let snap = fs.get_status();
    fs.set_status(0, IME_SMODE_NONE)?;
    if snap.mode != IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE {
        return Err(E_SYNC_MISS);
    }
    if fs.get_status().mode != 0 {
        return Err(E_STALE_READ);
    }
    hits += 1;

    // R7 sentence 闭集轮转：4 档逐档写读一致，越档拒收已由 R3 兜底。
    let mut fs = FwHwStat::new();
    let mut i = 0usize;
    while i < SENT_SET.len() {
        fs.sync_pair(IME_CMODE_NATIVE, SENT_SET[i])?;
        if fs.get_status().sentence != SENT_SET[i] {
            return Err(E_SYNC_MISS);
        }
        i += 1;
    }
    fs.audit()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要拒。
    let mut fs = FwHwStat::new();
    fs.sync_pair(IME_CMODE_NATIVE, IME_SMODE_AUTOMATIC)?;
    if fs.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    fs.writeback(true, 0x30)?;
    hits += 1;

    Ok(hits)
}
