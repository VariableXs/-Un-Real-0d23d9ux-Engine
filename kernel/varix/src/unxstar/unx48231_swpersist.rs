//! unx48231_swpersist — UNX-F48231 状态持久化（重启后默认态声明与恢复策略）（诊断段 0xBDC0）。
//!
//! 判据原文：
//!   J1 重启 ×10 态符合声明；判据锚定 体验十二。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 声明式默认态：本卡声明的重启默认态为「中文态 + CapsLock 关 +
//!      抖动窗缺省（20/40ms）+ 临时英文态不跨重启」——声明即契约，
//!      恢复结果必须逐位符合声明，声明本身不做运行期改写。
//!   2. 快照账：保存 = (mode, caps, version, magic, checksum) 五元组
//!      落盘快照；checksum 为全字段异或折叠（整数可复算）；版本不符
//!      或 checksum 不符一律按损坏处理。
//!   3. 恢复策略三档：快照完好 → 恢复快照态（且临态强制归中文，纪律 1）；
//!      快照损坏 → 回落声明默认态并落损坏账；无快照 → 首启默认态。
//!      每次恢复落一档账，档位与实际行为逐位对应。
//!   4. 重启 ×10 口径（J1）：10 轮「保存→模拟重启（丢内存）→加载」，
//!      每轮加载后的可见态与声明/快照逐位比对；含 2 轮人为损坏注入，
//!      损坏轮必须走回落档且终态==声明默认态。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBDC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MAGIC_BAD: u16 = SEG_BASE + 1;   // 魔数不符
const E_VER_BAD: u16 = SEG_BASE + 2;     // 版本不符
const E_CRC_BAD: u16 = SEG_BASE + 3;     // 校验和不符
const E_MODE_BAD: u16 = SEG_BASE + 4;    // 恢复出越闭集态（不该发生）
const E_RESTART_MISS: u16 = SEG_BASE + 5;// ×10 重启不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 恢复档账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 快照魔数与版本。
pub const SNAP_MAGIC: u32 = 0x5357_5031; // "SWP1"
pub const SNAP_VER: u32 = 1;

/// 落盘快照五元组。
#[derive(Clone, Copy, Debug)]
pub struct SnapBlob {
    pub magic: u32,
    pub ver: u32,
    pub mode_raw: u8,
    pub caps: bool,
    pub crc: u32,
}

impl SnapBlob {
    /// 打包：crc = magic ^ ver ^ mode_raw ^ (caps as u32)（异或折叠）。
    pub fn pack(mode_raw: u8, caps: bool) -> SnapBlob {
        let crc = SNAP_MAGIC
            ^ SNAP_VER
            ^ mode_raw as u32
            ^ (caps as u32);
        SnapBlob { magic: SNAP_MAGIC, ver: SNAP_VER, mode_raw, caps, crc }
    }
    /// 校验：魔数/版本/校验和三关。
    pub fn validate(&self) -> Result<(), u16> {
        if self.magic != SNAP_MAGIC {
            return Err(E_MAGIC_BAD);
        }
        if self.ver != SNAP_VER {
            return Err(E_VER_BAD);
        }
        let want = SNAP_MAGIC ^ SNAP_VER ^ self.mode_raw as u32 ^ (self.caps as u32);
        if self.crc != want {
            return Err(E_CRC_BAD);
        }
        Ok(())
    }
    /// 人为损坏注入：翻转 mode 位（构造 CRC 不符）。
    pub fn corrupt(&mut self) {
        self.mode_raw ^= 0xFF;
    }
}

/// 三态（运行面，持久化只认中文/英文两档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PMode {
    Chinese,
    English,
    TempEnglish,
}

impl PMode {
    pub fn to_raw(self) -> u8 {
        match self {
            PMode::Chinese => 0,
            PMode::English => 1,
            PMode::TempEnglish => 2,
        }
    }
    pub fn from_raw(v: u8) -> Option<PMode> {
        match v {
            0 => Some(PMode::Chinese),
            1 => Some(PMode::English),
            _ => None, // 2 与越界值都不落盘（临态不跨重启）
        }
    }
}

/// 声明默认态（纪律 1，常量即契约）。
pub const DECL_MODE_RAW: u8 = 0; // 中文
pub const DECL_CAPS: bool = false;

/// 恢复档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreTier {
    FromSnap,  // 快照完好
    ToDefault, // 损坏回落声明默认态
    FirstBoot, // 无快照首启
}

/// 一次恢复账。
#[derive(Clone, Copy, Debug)]
pub struct RestoreRecord {
    pub tier: RestoreTier,
    pub mode_raw: u8,
    pub caps: bool,
}

/// 持久化枢纽。
pub struct SwPersist {
    ledger: Vec<RestoreRecord>,
    pub restore_total: u32,
    pub corrupt_total: u32,
}

impl SwPersist {
    pub fn new() -> SwPersist {
        SwPersist { ledger: Vec::new(), restore_total: 0, corrupt_total: 0 }
    }

    /// 保存：当前运行态 → 快照（临态不落盘，先归中文）。
    pub fn save(&self, mode: PMode, caps: bool) -> SnapBlob {
        let eff = if mode == PMode::TempEnglish { PMode::Chinese } else { mode };
        SnapBlob::pack(eff.to_raw(), caps)
    }

    /// 加载：三档恢复策略（纪律 3）。
    pub fn load(&mut self, blob: Option<SnapBlob>) -> Result<(PMode, bool, RestoreTier), u16> {
        let (mode, caps, tier) = match blob {
            None => (PMode::Chinese, DECL_CAPS, RestoreTier::FirstBoot),
            Some(b) => {
                match b.validate() {
                    Ok(()) => {
                        let m = match PMode::from_raw(b.mode_raw) {
                            Some(v) => v,
                            None => return Err(E_MODE_BAD),
                        };
                        (m, b.caps, RestoreTier::FromSnap)
                    }
                    Err(_) => {
                        self.corrupt_total += 1;
                        (PMode::Chinese, DECL_CAPS, RestoreTier::ToDefault)
                    }
                }
            }
        };
        self.ledger.push(RestoreRecord { tier, mode_raw: mode.to_raw(), caps });
        self.restore_total += 1;
        Ok((mode, caps, tier))
    }

    /// ×N 重启覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.restore_total < want {
            return Err(E_RESTART_MISS);
        }
        Ok(())
    }

    /// 恢复账审计：账目守恒 + 损坏档全部回落声明默认态。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.restore_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            if r.tier == RestoreTier::ToDefault {
                if r.mode_raw != DECL_MODE_RAW || r.caps != DECL_CAPS {
                    return Err(E_LEDGER_BAD);
                }
            }
            if r.mode_raw > 1 {
                return Err(E_MODE_BAD); // 恢复面不落临态
            }
            i += 1;
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（重启 ×10 态符合声明），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 声明锚：声明默认态常量逐位核对（中文 + Caps 关）。
    if DECL_MODE_RAW != 0 || DECL_CAPS {
        return Err(E_LEDGER_BAD);
    }
    if SNAP_MAGIC != 0x5357_5031 || SNAP_VER != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 重启 ×10（含 2 轮损坏注入）：每轮 保存→加载→逐位比对；
    //    完好轮恢复快照态，损坏轮回落声明默认态。
    let mut sp = SwPersist::new();
    // 轮序列：(保存态, 是否注入损坏)。
    let rounds: [(u8, bool); 10] = [
        (0, false), (1, false), (0, false), (1, true), (0, false),
        (1, false), (0, true), (1, false), (0, false), (1, false),
    ];
    let mut i = 0usize;
    while i < 10 {
        let (mode_raw, corrupt) = rounds[i];
        let mode = match PMode::from_raw(mode_raw) {
            Some(m) => m,
            None => return Err(E_MODE_BAD),
        };
        let caps = i % 2 == 0;
        let mut blob = sp.save(mode, caps);
        if corrupt {
            blob.corrupt();
        }
        let opt = Some(blob);
        let (rm, rc, tier) = sp.load(opt)?;
        if corrupt {
            if tier != RestoreTier::ToDefault {
                return Err(E_CRC_BAD);
            }
            if rm != PMode::Chinese || rc != DECL_CAPS {
                return Err(E_LEDGER_BAD); // 回落必须符合声明
            }
        } else {
            if tier != RestoreTier::FromSnap {
                return Err(E_LEDGER_BAD);
            }
            if rm.to_raw() != mode_raw || rc != caps {
                return Err(E_LEDGER_BAD); // 恢复必须等于保存
            }
        }
        i += 1;
    }
    sp.coverage(10)?;
    sp.audit()?;
    if sp.corrupt_total != 2 {
        return Err(E_CRC_BAD);
    }
    hits += 1;

    // R3 临态不跨重启：临态保存归中文（纪律 1 在 save 内强制）。
    let sp = SwPersist::new();
    let blob = sp.save(PMode::TempEnglish, true);
    if blob.mode_raw != 0 {
        return Err(E_LEDGER_BAD);
    }
    blob.validate()?;
    hits += 1;

    // R4 首启档：无快照加载 → FirstBoot 档 + 声明默认态。
    let mut sp = SwPersist::new();
    let (m, c, tier) = sp.load(None)?;
    if tier != RestoreTier::FirstBoot || m != PMode::Chinese || c != DECL_CAPS {
        return Err(E_LEDGER_BAD);
    }
    sp.audit()?;
    hits += 1;

    // R5 损坏三关：魔数翻、版本翻、CRC 翻各自独立击穿校验，全走回落档。
    let mut sp = SwPersist::new();
    let mut b1 = SnapBlob::pack(1, false);
    b1.magic ^= 1;
    let (_, _, t1) = sp.load(Some(b1))?;
    if t1 == RestoreTier::FromSnap {
        return Err(E_MAGIC_BAD);
    }
    let mut b2 = SnapBlob::pack(1, false);
    b2.ver = 9;
    let (_, _, t2) = sp.load(Some(b2))?;
    if t2 == RestoreTier::FromSnap {
        return Err(E_VER_BAD);
    }
    let mut b3 = SnapBlob::pack(1, false);
    b3.crc ^= 0xDEAD;
    let (_, _, t3) = sp.load(Some(b3))?;
    if t3 == RestoreTier::FromSnap {
        return Err(E_CRC_BAD);
    }
    if sp.corrupt_total != 3 {
        return Err(E_LEDGER_BAD);
    }
    sp.audit()?;
    hits += 1;

    // R6 打包自洽：pack 双参数四组合的 CRC 可复算且 validate 放行。
    let mut i = 0u8;
    while i < 2 {
        let mut j = 0u8;
        while j < 2 {
            let b = SnapBlob::pack(i, j == 1);
            b.validate()?;
            let want = SNAP_MAGIC ^ SNAP_VER ^ i as u32 ^ (j as u32);
            if b.crc != want {
                return Err(E_CRC_BAD);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R7 恢复账逐条回放：R2 段的账逐条核对档位与声明符合性（回放口径）。
    let mut sp = SwPersist::new();
    let mut i = 0usize;
    while i < 10 {
        let (mode_raw, corrupt) = rounds[i];
        let mut blob = SnapBlob::pack(mode_raw, i % 2 == 0);
        if corrupt {
            blob.corrupt();
        }
        sp.load(Some(blob))?;
        i += 1;
    }
    let mut i = 0usize;
    while i < sp.ledger_len() {
        let r = &sp.ledger[i];
        let was_corrupt = rounds[i].1;
        if was_corrupt {
            if r.tier != RestoreTier::ToDefault || r.mode_raw != DECL_MODE_RAW {
                return Err(E_LEDGER_BAD);
            }
        } else {
            if r.tier != RestoreTier::FromSnap || r.mode_raw != rounds[i].0 {
                return Err(E_LEDGER_BAD);
            }
        }
        i += 1;
    }
    sp.audit()?;
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与零恢复拒。
    let mut sp = SwPersist::new();
    if sp.writeback(true, 0x10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    sp.load(None)?;
    if sp.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sp.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
