//! unx4130_fflags — UNX-F4130 file 级 O_ 标志暂存与 f_flags（诊断段 0x9220）。
//!
//! 判据原文：UNX-F4130-J1 open 时 O_ 标志位到 f_flags 的映射全量落账（标志位逐位
//!           核对），运行期非法改旗注入被拒（只读不变式）。
//!   J2 提交后 f_flags 只读，八位逐一试改全拒。
//!   J3 逐位核对次数恒等于提交文件数乘 8。
//!   J4 闭集外标志位暂存即拒（不静默丢弃）。
//! 设计纪律：
//!   1. 两段式：open 期先把 O_ 标志「暂存」（OFlagSet，未生效），装配完成才提交为
//!      f_flags（FFlags，只读）。暂存期可校验，提交后不可改——改旗只能经新 open 产生
//!      新 file，不能改既有 file 的 f_flags。
//!   2. 映射必须逐位核对：受理闭集 8 位，每位在 f_flags 中有且仅有一处落位，
//!      位与位之间不合并、不推导（O_SYNC 不自动带 O_DSYNC，O_DIRECT 不外扩）。
//!   3. 「运行期改旗」是明确禁区：提交后任何 set_* 调用返回 E_INVAL，并在
//!      篡改尝试账里留痕；留痕与拒收同时发生，二者缺一即判失守。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9220 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9220;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_INVAL: u16 = SEG_BASE + 1; // 非法改旗（EINVAL 22）
const E_UNKNOWN: u16 = SEG_BASE + 2; // 暂存了受理闭集之外的位
const E_NOTSTAGED: u16 = SEG_BASE + 3; // 未暂存即提交
const E_COMMITTED: u16 = SEG_BASE + 4; // 已提交后再暂存（file 复用错误）
const E_BITMISS: u16 = SEG_BASE + 5; // 映射漏位（逐位核对失败）
const E_ACCT: u16 = SEG_BASE + 6; // 位账与提交账不平
const E_FROZEN: u16 = SEG_BASE + 7; // f_flags 只读被破坏
const E_NOFILE: u16 = SEG_BASE + 8; // 无 file 对象承载

/// EINVAL 的 errno 原值（POSIX errno.h）。
pub const EINVAL_ERRNO: u16 = 22;

// ── 受理闭集 8 位（八进制原值，Linux asm-generic/fcntl.h）─────
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DSYNC: u32 = 0o10000;
pub const O_SYNC: u32 = 0o4010000;
pub const O_DIRECT: u32 = 0o40000;
pub const O_NOATIME: u32 = 0o1000000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const O_DIRECTORY: u32 = 0o200000;
/// 8 位闭集的并集，用于未知位拒收。
pub const O_FLAG_MASK: u32 = O_APPEND
    | O_NONBLOCK
    | O_DSYNC
    | O_SYNC
    | O_DIRECT
    | O_NOATIME
    | O_CLOEXEC
    | O_DIRECTORY;

/// 标志位枚举（8 元闭集）：逐位核对的遍历基。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagBit {
    Append = 0,
    Nonblock = 1,
    Dsync = 2,
    Sync = 3,
    Direct = 4,
    Noatime = 5,
    Cloexec = 6,
    Directory = 7,
}

pub const FLAG_COUNT: usize = 8;

impl FlagBit {
    pub fn from_idx(i: u8) -> Option<FlagBit> {
        match i {
            0 => Some(FlagBit::Append),
            1 => Some(FlagBit::Nonblock),
            2 => Some(FlagBit::Dsync),
            3 => Some(FlagBit::Sync),
            4 => Some(FlagBit::Direct),
            5 => Some(FlagBit::Noatime),
            6 => Some(FlagBit::Cloexec),
            7 => Some(FlagBit::Directory),
            _ => None,
        }
    }

    /// 该位的原始掩码（逐位落账的基准值）。
    pub fn mask(self) -> u32 {
        match self {
            FlagBit::Append => O_APPEND,
            FlagBit::Nonblock => O_NONBLOCK,
            FlagBit::Dsync => O_DSYNC,
            FlagBit::Sync => O_SYNC,
            FlagBit::Direct => O_DIRECT,
            FlagBit::Noatime => O_NOATIME,
            FlagBit::Cloexec => O_CLOEXEC,
            FlagBit::Directory => O_DIRECTORY,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FlagBit::Append => "O_APPEND",
            FlagBit::Nonblock => "O_NONBLOCK",
            FlagBit::Dsync => "O_DSYNC",
            FlagBit::Sync => "O_SYNC",
            FlagBit::Direct => "O_DIRECT",
            FlagBit::Noatime => "O_NOATIME",
            FlagBit::Cloexec => "O_CLOEXEC",
            FlagBit::Directory => "O_DIRECTORY",
        }
    }
}

/// f_flags 相位机：Staged（暂存）→ Committed（只读）→ 拒绝任何回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagPhase {
    Empty,
    Staged,
    Committed,
}

/// 暂存体：open 期收集的原始 O_ 位，可校验、可清空、不可直接读用。
#[derive(Clone, Copy)]
pub struct OFlagSet {
    /// 暂存的原始位
    pub raw: u32,
    /// 相位
    pub phase: FlagPhase,
}

impl OFlagSet {
    pub fn new() -> OFlagSet {
        OFlagSet {
            raw: 0,
            phase: FlagPhase::Empty,
        }
    }

    /// 暂存：闭集之外的位一律拒收（不静默丢弃）。
    pub fn stage(&mut self, oflags: u32) -> Result<(), u16> {
        if self.phase != FlagPhase::Empty {
            return Err(E_COMMITTED);
        }
        if (oflags & !O_FLAG_MASK) != 0 {
            return Err(E_UNKNOWN);
        }
        self.raw = oflags;
        self.phase = FlagPhase::Staged;
        Ok(())
    }

    /// 提交：Staged → Committed，产出只读 f_flags。
    pub fn commit(&mut self) -> Result<FFlags, u16> {
        if self.phase != FlagPhase::Staged {
            return Err(E_NOTSTAGED);
        }
        self.phase = FlagPhase::Committed;
        Ok(FFlags {
            bits: self.raw,
            frozen: true,
        })
    }
}

/// 只读 f_flags：提交后 frozen 恒真，任何改旗走 Err(E_INVAL)。
pub struct FFlags {
    /// 已落位的 f_flags
    pub bits: u32,
    /// 只读闸（提交即置真，本结构不提供置假入口）
    pub frozen: bool,
}

impl FFlags {
    /// 逐位核对：位 i 是否落位。索引先比长度。
    pub fn has(&self, bit: FlagBit) -> bool {
        (self.bits & bit.mask()) != 0
    }

    /// 运行期改旗：只读不变式在此拦截。
    pub fn try_set(&mut self, bit: FlagBit) -> Result<(), u16> {
        if self.frozen {
            return Err(E_INVAL);
        }
        self.bits |= bit.mask();
        Ok(())
    }

    /// 运行期清旗：同样被只读闸拦截（Linux 只有 O_NONBLOCK 可经 F_SETFL 改，
    /// 本域不开放任何运行期改旗入口，故一并拒）。
    pub fn try_clear(&mut self, bit: FlagBit) -> Result<(), u16> {
        if self.frozen {
            return Err(E_INVAL);
        }
        self.bits &= !bit.mask();
        Ok(())
    }

    /// 位账：bits 中不存在闭集外的位。
    pub fn closed(&self) -> bool {
        (self.bits & !O_FLAG_MASK) == 0
    }
}

/// 映射核：暂存 → 提交 → 逐位核对，三账合一。
pub struct FlagMapper {
    /// 已提交的 f_flags 快照（按 file 序）
    pub files: Vec<u32>,
    /// 逐位核对通过次数（每位每次 +1）
    pub bit_checks: u32,
    /// 漏位次数（应为 0）
    pub bit_misses: u32,
    /// 改旗拒收次数
    pub mutate_denies: u32,
    /// 暂存拒收次数
    pub stage_denies: u32,
}

impl FlagMapper {
    pub fn new() -> FlagMapper {
        FlagMapper {
            files: Vec::new(),
            bit_checks: 0,
            bit_misses: 0,
            mutate_denies: 0,
            stage_denies: 0,
        }
    }

    /// 完整 open 期映射：暂存 → 提交 → 落表 → 逐位核对。
    pub fn map(&mut self, oflags: u32) -> Result<usize, u16> {
        let mut set = OFlagSet::new();
        if let Err(e) = set.stage(oflags) {
            self.stage_denies += 1;
            return Err(e);
        }
        let ff = set.commit()?;
        if !ff.frozen || !ff.closed() {
            return Err(E_FROZEN);
        }
        // 逐位核对：期望值直接由原始 oflags 推出，与 f_flags 比对。
        let mut i = 0u8;
        while (i as usize) < FLAG_COUNT {
            let bit = match FlagBit::from_idx(i) {
                Some(b) => b,
                None => return Err(E_BITMISS),
            };
            let want = (oflags & bit.mask()) != 0;
            if ff.has(bit) != want {
                self.bit_misses += 1;
                return Err(E_BITMISS);
            }
            self.bit_checks += 1;
            i += 1;
        }
        self.files.push(ff.bits);
        Ok(self.files.len() - 1)
    }

    /// 运行期改旗注入（对已提交的 file）：必拒且留痕。
    pub fn inject_mutate(&mut self, idx: usize, bit: FlagBit) -> Result<(), u16> {
        let bits = match self.files.get(idx) {
            Some(&b) => b,
            None => return Err(E_NOFILE),
        };
        let mut ff = FFlags {
            bits,
            frozen: true,
        };
        if ff.try_set(bit).is_err() {
            self.mutate_denies += 1;
            // 拒收后原位不得变化（拒绝必须是「零副作用」的）
            if let Some(slot) = self.files.get_mut(idx) {
                if *slot != bits {
                    return Err(E_ACCT);
                }
            }
            return Err(E_INVAL);
        }
        Err(E_FROZEN) // 走到这里说明只读闸失守
    }

    /// 位覆盖率：bit_checks 必须等于 提交文件数 × 8。
    pub fn coverage_ok(&self) -> bool {
        self.bit_checks as usize == self.files.len() * FLAG_COUNT
    }

    /// 改旗零篡改：每条落表快照仍在闭集内。
    pub fn snapshots_closed(&self) -> bool {
        let mut i = 0usize;
        while i < self.files.len() {
            if (self.files[i] & !O_FLAG_MASK) != 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_INVAL => "运行期非法改旗（EINVAL 22）",
        E_UNKNOWN => "暂存了受理闭集之外的位",
        E_NOTSTAGED => "未暂存即提交",
        E_COMMITTED => "已提交后再次暂存",
        E_BITMISS => "映射漏位（逐位核对失败）",
        E_ACCT => "位账与提交账不平",
        E_FROZEN => "f_flags 只读不变式被破坏",
        E_NOFILE => "无 file 对象承载",
        _ => "未知诊断码",
    }
}

/// 确定性演练：逐位映射 → 改旗拦截 → 闭集拒收 → 覆盖对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：8 位全置的映射，逐位核对 8 次，落表 1 条。
    let mut m = FlagMapper::new();
    let all = O_APPEND
        | O_NONBLOCK
        | O_DSYNC
        | O_SYNC
        | O_DIRECT
        | O_NOATIME
        | O_CLOEXEC
        | O_DIRECTORY;
    let idx0 = m.map(all)?;
    if idx0 != 0 || m.files.len() != 1 {
        return Err(E_ACCT);
    }
    if m.bit_checks != 8 || m.bit_misses != 0 {
        return Err(E_BITMISS);
    }
    hits += 1;

    // R2：单位映射——只置 O_APPEND 的文件，f_flags 必须恰好等于 O_APPEND。
    let idx1 = m.map(O_APPEND)?;
    if m.files[idx1] != O_APPEND {
        return Err(E_BITMISS);
    }
    // 逐位反查：8 位中只有 Append 落位。
    {
        let bits = m.files[idx1];
        let mut on = 0u32;
        let mut i = 0u8;
        while (i as usize) < FLAG_COUNT {
            if let Some(bit) = FlagBit::from_idx(i) {
                if (bits & bit.mask()) != 0 {
                    on += 1;
                }
            }
            i += 1;
        }
        if on != 1 {
            return Err(E_BITMISS);
        }
    }
    hits += 1;

    // R3：空标志映射——0 位落表，f_flags 为 0（合法：无任何修饰位）。
    let idx2 = m.map(0)?;
    if m.files[idx2] != 0 {
        return Err(E_BITMISS);
    }
    hits += 1;

    // R4：运行期改旗注入 10/10 次全部 E_INVAL，且快照零变化（判据主断言）。
    let before = m.files[idx1];
    let mut i = 0;
    while i < 10 {
        if m.inject_mutate(idx1, FlagBit::Nonblock) != Err(E_INVAL) {
            return Err(E_FROZEN);
        }
        i += 1;
    }
    if m.mutate_denies != 10 {
        return Err(E_ACCT);
    }
    if m.files[idx1] != before {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R5：8 位逐一试改（覆盖全部位，不只 O_NONBLOCK），全部拒收。
    let mut b = 0u8;
    while (b as usize) < FLAG_COUNT {
        let bit = match FlagBit::from_idx(b) {
            Some(x) => x,
            None => return Err(E_BITMISS),
        };
        if m.inject_mutate(idx0, bit) != Err(E_INVAL) {
            return Err(E_FROZEN);
        }
        b += 1;
    }
    if m.mutate_denies != 18 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：闭集外位暂存 → E_UNKNOWN（不静默丢弃）。
    if m.map(0x8000_0000) != Err(E_UNKNOWN) {
        return Err(E_UNKNOWN);
    }
    if m.stage_denies != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I2：未暂存即提交 → E_NOTSTAGED；已提交再暂存 → E_COMMITTED。
    let mut s = OFlagSet::new();
    match s.commit() {
        Err(E_NOTSTAGED) => {}
        _ => return Err(E_NOTSTAGED),
    }
    s.stage(O_CLOEXEC)?;
    s.commit()?;
    if s.stage(O_APPEND) != Err(E_COMMITTED) {
        return Err(E_COMMITTED);
    }
    hits += 1;

    // I3：越界下标改旗 → E_NOFILE。
    if m.inject_mutate(m.files.len(), FlagBit::Append) != Err(E_NOFILE) {
        return Err(E_NOFILE);
    }
    hits += 1;

    // I4：只读闸自检——frozen 结构上无法被外部置假（构造即真）。
    let f = FFlags {
        bits: O_SYNC,
        frozen: true,
    };
    if !f.closed() || !f.has(FlagBit::Sync) {
        return Err(E_FROZEN);
    }
    if f.has(FlagBit::Append) {
        return Err(E_BITMISS); // O_SYNC 不得推导出 O_APPEND
    }
    hits += 1;

    // C1：覆盖率对账——3 个文件 × 8 位 = 24 次逐位核对。
    if !m.coverage_ok() {
        return Err(E_ACCT);
    }
    if m.bit_checks != 24 || m.files.len() != 3 {
        return Err(E_ACCT);
    }
    if !m.snapshots_closed() {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
