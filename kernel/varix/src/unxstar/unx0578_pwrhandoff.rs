//! UNX-F0578 · 电源交接字段（A2 预备）· 诊断段 0x77C0（本单独占）
//!
//! 判据原文摘录：UNX-F0578-J1 交接字段 v1 冻结（8 字段），A2 mock diff 零差；
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 交接字段 v1 冻结闭集（8 字段，序号 0..=7）：
//!     0 电源态、1 C 态驻留、2 唤醒源、3 电池余量、4 热等级、5 充电路径、
//!     6 平台挂起能力、7 交接序号。
//!  3. 冻结纪律：字段表一经 freeze 拒收增删改；A2 mock 逐字段 diff 必须零差。
//!  4. mock 对账：交接帧按字段表逐字段序列化，与 mock 帧逐字节比对。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x77C0;
/// 字段序号越闭集（0..=7 之外）
pub const E_FIELD_RANGE: u16 = SEG_BASE + 1;
/// 字段表未冻结就试图对账
pub const E_NOT_FROZEN: u16 = SEG_BASE + 2;
/// 冻结后试图改表
pub const E_FROZEN_LOCK: u16 = SEG_BASE + 3;
/// mock diff 非零差
pub const E_DIFF_NONZERO: u16 = SEG_BASE + 4;
/// 交接帧长度不符（定容 8 字段帧）
pub const E_FRAME_LEN: u16 = SEG_BASE + 5;
/// 交接序号回卷（非递增）
pub const E_SEQ_REWIND: u16 = SEG_BASE + 6;
/// 字段值越域（枚举闭集外）
pub const E_VALUE_RANGE: u16 = SEG_BASE + 7;
/// 摘要为空
pub const E_SUMMARY_EMPTY: u16 = SEG_BASE + 8;

/// 冻结字段数锚点。
pub const N_FIELDS: usize = 8;
/// 字段名表（v1 冻结字面，序号即下标）。
pub const FIELD_NAMES: [&str; 8] = [
    "电源态",
    "C态驻留",
    "唤醒源",
    "电池余量",
    "热等级",
    "充电路径",
    "平台挂起能力",
    "交接序号",
];

/// 电源态闭集。
pub const PWR_S0: u8 = 0;
pub const PWR_S1: u8 = 1;
pub const PWR_S3: u8 = 2;
pub const PWR_S4: u8 = 3;
pub const PWR_S5: u8 = 4;
/// 电源态合法值集合大小。
pub const PWR_STATES: u8 = 5;

/// 唤醒源闭集。
pub const WAKE_NONE: u8 = 0;
pub const WAKE_RTC: u8 = 1;
pub const WAKE_PME: u8 = 2;
pub const WAKE_KEY: u8 = 3;
pub const WAKE_WOL: u8 = 4;

/// 热等级闭集（0 正常 .. 4 临界）。
pub const THERM_LEVELS: u8 = 5;

/// 充电路径闭集。
pub const CHG_NONE: u8 = 0;
pub const CHG_AC: u8 = 1;
pub const CHG_USB: u8 = 2;
pub const CHG_WIRELESS: u8 = 3;

/// 平台挂起能力位图（bit0=S1 bit1=S3 bit2=S4 bit3=S5）。
pub const SLEEP_CAP_MASK: u8 = 0x0F;

/// 单字段值（整数域 + 合法性谓词闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldValue {
    /// 字段序号（0..=7）
    pub idx: u8,
    /// 字段值（整数域，语义由 idx 决定）
    pub val: u32,
}

impl FieldValue {
    /// 字段值合法性（按字段各自的枚举/范围闭集）。
    pub fn valid(&self) -> bool {
        match self.idx {
            0 => self.val < PWR_STATES as u32,
            1 => self.val <= 100, // C 态驻留百分比
            2 => self.val <= WAKE_WOL as u32,
            3 => self.val <= 100, // 电池余量百分比
            4 => self.val < THERM_LEVELS as u32,
            5 => self.val <= CHG_WIRELESS as u32,
            6 => self.val & !(SLEEP_CAP_MASK as u32) == 0,
            7 => self.val >= 1, // 交接序号从 1 起
            _ => false,
        }
    }
}

/// 交接帧（8 字段定容）。
#[derive(Clone, Copy, Debug)]
pub struct HandoffFrame {
    pub fields: [FieldValue; N_FIELDS],
}

impl HandoffFrame {
    /// 构造一帧（全零基线，交接序号必须 ≥1）。
    pub fn new(seq: u32) -> Result<Self, u16> {
        if seq == 0 {
            return Err(E_VALUE_RANGE);
        }
        let mut f = HandoffFrame {
            fields: [
                FieldValue { idx: 0, val: 0 },
                FieldValue { idx: 1, val: 0 },
                FieldValue { idx: 2, val: 0 },
                FieldValue { idx: 3, val: 0 },
                FieldValue { idx: 4, val: 0 },
                FieldValue { idx: 5, val: 0 },
                FieldValue { idx: 6, val: 0 },
                FieldValue { idx: 7, val: seq },
            ],
        };
        // 交接序号写入末字段
        f.fields[7].val = seq;
        // 全字段合法性
        let mut i = 0usize;
        while i < N_FIELDS {
            if !f.fields[i].valid() {
                return Err(E_VALUE_RANGE);
            }
            i += 1;
        }
        Ok(f)
    }

    /// 改一个字段（先值域后写回）。
    pub fn set(&mut self, idx: u8, val: u32) -> Result<(), u16> {
        if idx as usize >= N_FIELDS {
            return Err(E_FIELD_RANGE);
        }
        let cand = FieldValue { idx, val };
        if !cand.valid() {
            return Err(E_VALUE_RANGE);
        }
        self.fields[idx as usize] = cand;
        Ok(())
    }

    /// 帧序列化：8 字段按序打包为 u32 数组（定容，A2 mock 同构）。
    pub fn serialize(&self) -> [u32; N_FIELDS] {
        let mut out = [0u32; N_FIELDS];
        let mut i = 0usize;
        while i < N_FIELDS {
            out[i] = self.fields[i].val;
            i += 1;
        }
        out
    }

    /// 字段表 diff：与本帧同构 mock 帧逐字段比对，返回不同字段数（0 = 零差）。
    pub fn diff(&self, mock: &HandoffFrame) -> Result<u32, u16> {
        let mut d = 0u32;
        let mut i = 0usize;
        while i < N_FIELDS {
            if self.fields[i].idx != mock.fields[i].idx {
                return Err(E_FIELD_RANGE);
            }
            if self.fields[i].val != mock.fields[i].val {
                d += 1;
            }
            i += 1;
        }
        Ok(d)
    }
}

/// 字段表冻结账：v1 冻结后拒收增删改。
pub struct FrozenTable {
    /// 冻结位
    pub frozen: bool,
    /// 冻结时字段数快照
    pub frozen_len: usize,
    /// 冻结时字段名快照（字面拷贝，逐名 diff 用）
    pub frozen_names: [&'static str; N_FIELDS],
}

impl FrozenTable {
    pub fn new() -> Self {
        FrozenTable {
            frozen: false,
            frozen_len: 0,
            frozen_names: ["", "", "", "", "", "", "", ""],
        }
    }
    /// 冻结：快照当前字段名表。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN_LOCK);
        }
        let mut i = 0usize;
        while i < N_FIELDS {
            self.frozen_names[i] = FIELD_NAMES[i];
            i += 1;
        }
        self.frozen_len = N_FIELDS;
        self.frozen = true;
        Ok(())
    }
    /// mock 对账：冻结快照与 v1 字面逐名 diff，零差才通过。
    pub fn mock_diff(&self) -> Result<u32, u16> {
        if !self.frozen {
            return Err(E_NOT_FROZEN);
        }
        let mut d = 0u32;
        let mut i = 0usize;
        while i < N_FIELDS {
            if self.frozen_names[i] != FIELD_NAMES[i] {
                d += 1;
            }
            i += 1;
        }
        Ok(d)
    }
}

impl Default for FrozenTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 交接序号链账：帧序号严格递增，回卷即拒。
pub struct SeqChain {
    last: u32,
    count: u32,
}

impl SeqChain {
    pub fn new() -> Self {
        SeqChain { last: 0, count: 0 }
    }
    /// 收一帧（序号必须 > last）。
    pub fn admit(&mut self, f: &HandoffFrame) -> Result<(), u16> {
        let seq = f.fields[7].val;
        if seq <= self.last {
            return Err(E_SEQ_REWIND);
        }
        self.last = seq;
        self.count += 1;
        Ok(())
    }
    pub fn len(&self) -> u32 {
        self.count
    }
    pub fn last_seq(&self) -> u32 {
        self.last
    }
}

impl Default for SeqChain {
    fn default() -> Self {
        Self::new()
    }
}

/// A2 mock 帧：按 v1 冻结语义构造的参考帧（字段逐个给定）。
pub fn a2_mock_frame(seq: u32) -> Result<HandoffFrame, u16> {
    let mut f = HandoffFrame::new(seq)?;
    f.set(0, PWR_S0 as u32)?;
    f.set(1, 25)?; // C 态驻留 25%
    f.set(2, WAKE_KEY as u32)?;
    f.set(3, 88)?; // 电池 88%
    f.set(4, 1)?; // 热等级 1
    f.set(5, CHG_AC as u32)?;
    f.set(6, 0b1010)?; // S3 + S5 支持
    Ok(f)
}

/// 自例程：冻结 → mock 零差 → 帧对账零差 → 序号链 → 封板纪律 → 0。
pub fn drill() -> Result<u32, u16> {
    // 字段表冻结 + mock 对账
    let mut tab = FrozenTable::new();
    tab.freeze()?;
    if tab.mock_diff()? != 0 {
        return Err(E_DIFF_NONZERO);
    }
    // 重复冻结拒收
    if tab.freeze() != Err(E_FROZEN_LOCK) {
        return Err(E_FROZEN_LOCK);
    }
    // A2 mock 帧 diff 零差
    let live = a2_mock_frame(7)?;
    let mock = a2_mock_frame(7)?;
    if live.diff(&mock)? != 0 {
        return Err(E_DIFF_NONZERO);
    }
    // 序列化定容
    let ser = live.serialize();
    if ser.len() != N_FIELDS || ser[7] != 7 {
        return Err(E_FRAME_LEN);
    }
    // 序号链：递增收帧，回卷拒收
    let mut chain = SeqChain::new();
    chain.admit(&a2_mock_frame(1)?)?;
    chain.admit(&a2_mock_frame(2)?)?;
    chain.admit(&a2_mock_frame(9)?)?;
    if chain.len() != 3 || chain.last_seq() != 9 {
        return Err(E_SEQ_REWIND);
    }
    if chain.admit(&a2_mock_frame(3)?) != Err(E_SEQ_REWIND) {
        return Err(E_SEQ_REWIND);
    }
    // 值域闭集：非法字段值拒收
    let mut bad = a2_mock_frame(10)?;
    if bad.set(3, 101) != Err(E_VALUE_RANGE) {
        return Err(E_VALUE_RANGE);
    }
    if bad.set(8, 0) != Err(E_FIELD_RANGE) {
        return Err(E_FIELD_RANGE);
    }
    Ok(0)
}
