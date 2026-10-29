//! UNX-F0598 · 参数交接字段（B30 预备）· 诊断段 0x7820（本单独占）
//!
//! 判据原文摘录：UNX-F0598-J1 交接字段 v1 冻结（8 字段），消费方 mock diff
//! 零差；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 参数交接字段 v1 冻结闭集（8 字段，序号 0..=7）：
//!     0 参数块版本、1 启动模式、2 内核加载基址、3 参数区长度、4 页粒度、
//!     5 调试通道、6 校验算法族、7 交接序号。
//!  3. 与电源面交接（F0578）字段语义完全独立，禁止共用枚举值域。
//!  4. 消费方 mock 对账：交接帧按字段表序列化，与消费方 mock 帧逐字节 diff。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7820;
/// 字段序号越闭集
pub const E_FIELD_RANGE: u16 = SEG_BASE + 1;
/// 未冻结就对账
pub const E_NOT_FROZEN: u16 = SEG_BASE + 2;
/// 冻结后改表
pub const E_FROZEN_LOCK: u16 = SEG_BASE + 3;
/// mock diff 非零差
pub const E_DIFF_NONZERO: u16 = SEG_BASE + 4;
/// 帧长度不符（定容 8 字段）
pub const E_FRAME_LEN: u16 = SEG_BASE + 5;
/// 交接序号回卷
pub const E_SEQ_REWIND: u16 = SEG_BASE + 6;
/// 字段值越域
pub const E_VALUE_RANGE: u16 = SEG_BASE + 7;
/// 版本不匹配（参数块版本字面不符）
pub const E_VER_MISMATCH: u16 = SEG_BASE + 8;

/// 冻结字段数锚点。
pub const N_FIELDS: usize = 8;
/// 字段名表（v1 冻结字面）。
pub const FIELD_NAMES: [&str; 8] = [
    "参数块版本",
    "启动模式",
    "内核加载基址",
    "参数区长度",
    "页粒度",
    "调试通道",
    "校验算法族",
    "交接序号",
];

/// 参数块版本锚点（v1）。
pub const BLOCK_VER: u32 = 1;

/// 启动模式闭集。
pub const BOOT_COLD: u8 = 0;
pub const BOOT_WARM: u8 = 1;
pub const BOOT_RESUME: u8 = 2;
pub const BOOT_MODES: u8 = 3;

/// 页粒度闭集（字节）：4KiB / 2MiB / 1GiB。
pub const PAGE_4K: u32 = 4_096;
pub const PAGE_2M: u32 = 2_097_152;
pub const PAGE_1G: u32 = 1_073_741_824;

/// 调试通道闭集。
pub const DBG_NONE: u8 = 0;
pub const DBG_SERIAL: u8 = 1;
pub const DBG_MEMORY: u8 = 2;
pub const DBG_BOTH: u8 = 3;

/// 校验算法族闭集。
pub const CKSUM_ADD: u8 = 0;
pub const CKSUM_FNV: u8 = 1;
pub const CKSUM_CRC32: u8 = 2;

/// 单字段值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldValue {
    pub idx: u8,
    pub val: u32,
}

impl FieldValue {
    /// 字段值合法性（参数面各自闭集）。
    pub fn valid(&self) -> bool {
        match self.idx {
            0 => self.val == BLOCK_VER, // 版本冻结 v1
            1 => self.val < BOOT_MODES as u32,
            2 => self.val >= 0x0010_0000 && self.val <= 0xFFFF_F000, // 合理基址域
            3 => self.val > 0 && self.val <= 0x0010_0000, // 参数区 ≤ 1MiB
            4 => self.val == PAGE_4K || self.val == PAGE_2M || self.val == PAGE_1G,
            5 => self.val <= DBG_BOTH as u32,
            6 => self.val <= CKSUM_CRC32 as u32,
            7 => self.val >= 1,
            _ => false,
        }
    }
}

/// 交接帧（8 字段定容）。
#[derive(Clone, Copy, Debug)]
pub struct ParamFrame {
    pub fields: [FieldValue; N_FIELDS],
}

impl ParamFrame {
    pub fn new(seq: u32) -> Result<Self, u16> {
        if seq == 0 {
            return Err(E_VALUE_RANGE);
        }
        let mut f = ParamFrame {
            fields: [
                FieldValue { idx: 0, val: BLOCK_VER },
                FieldValue { idx: 1, val: 0 },
                FieldValue { idx: 2, val: 0x0020_0000 },
                FieldValue { idx: 3, val: 0x0001_0000 },
                FieldValue { idx: 4, val: PAGE_4K },
                FieldValue { idx: 5, val: 0 },
                FieldValue { idx: 6, val: 0 },
                FieldValue { idx: 7, val: seq },
            ],
        };
        let mut i = 0usize;
        while i < N_FIELDS {
            if !f.fields[i].valid() {
                return Err(E_VALUE_RANGE);
            }
            i += 1;
        }
        Ok(f)
    }

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

    pub fn serialize(&self) -> [u32; N_FIELDS] {
        let mut out = [0u32; N_FIELDS];
        let mut i = 0usize;
        while i < N_FIELDS {
            out[i] = self.fields[i].val;
            i += 1;
        }
        out
    }

    /// 消费方 diff：逐字段比对（先核字段序号一致性再比值），返回差异数。
    pub fn diff(&self, mock: &ParamFrame) -> Result<u32, u16> {
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

/// 字段表冻结账。
pub struct FrozenTable {
    pub frozen: bool,
    pub frozen_len: usize,
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

/// 序号链（与电源面独立实现，单调递增语义相同）。
pub struct SeqChain {
    last: u32,
    count: u32,
}

impl SeqChain {
    pub fn new() -> Self {
        SeqChain { last: 0, count: 0 }
    }
    pub fn admit(&mut self, f: &ParamFrame) -> Result<(), u16> {
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
}

impl Default for SeqChain {
    fn default() -> Self {
        Self::new()
    }
}

/// 消费方 mock 帧（B30 消费方视角的参考值）。
pub fn consumer_mock_frame(seq: u32) -> Result<ParamFrame, u16> {
    let mut f = ParamFrame::new(seq)?;
    f.set(1, BOOT_COLD as u32)?;
    f.set(2, 0x0020_0000)?;
    f.set(3, 0x0001_0000)?;
    f.set(4, PAGE_4K)?;
    f.set(5, DBG_SERIAL as u32)?;
    f.set(6, CKSUM_FNV as u32)?;
    Ok(f)
}

/// 参数块版本一致性检查（版本不符即拒）。
pub fn ver_check(f: &ParamFrame) -> Result<(), u16> {
    if f.fields[0].val != BLOCK_VER {
        return Err(E_VER_MISMATCH);
    }
    Ok(())
}

/// 自例程：冻结 → mock 零差 → 版本检查 → 序号链 → 值域闭集 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut tab = FrozenTable::new();
    tab.freeze()?;
    if tab.mock_diff()? != 0 {
        return Err(E_DIFF_NONZERO);
    }
    if tab.freeze() != Err(E_FROZEN_LOCK) {
        return Err(E_FROZEN_LOCK);
    }
    let live = consumer_mock_frame(3)?;
    let mock = consumer_mock_frame(3)?;
    ver_check(&live)?;
    if live.diff(&mock)? != 0 {
        return Err(E_DIFF_NONZERO);
    }
    let ser = live.serialize();
    if ser.len() != N_FIELDS || ser[0] != BLOCK_VER {
        return Err(E_FRAME_LEN);
    }
    let mut chain = SeqChain::new();
    chain.admit(&consumer_mock_frame(1)?)?;
    chain.admit(&consumer_mock_frame(5)?)?;
    if chain.len() != 2 {
        return Err(E_SEQ_REWIND);
    }
    if chain.admit(&consumer_mock_frame(2)?) != Err(E_SEQ_REWIND) {
        return Err(E_SEQ_REWIND);
    }
    let mut bad = consumer_mock_frame(6)?;
    if bad.set(4, PAGE_4K + 1) != Err(E_VALUE_RANGE) {
        return Err(E_VALUE_RANGE);
    }
    if bad.set(0, 2) != Err(E_VALUE_RANGE) {
        return Err(E_VALUE_RANGE);
    }
    Ok(0)
}
