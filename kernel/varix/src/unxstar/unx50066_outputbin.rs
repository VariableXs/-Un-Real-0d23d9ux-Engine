//! UNX-F50066 输出分仓语义（逐张 / 装订 / 打孔三通道落账）（0x43460 段）
//!
//! 锚点：UNX-F50066-J1 通道 ×3 ×10 落账与能力表一致；判据锚定 §AI-63 §2 专题六。
//!
//! 数据段：`SEG_BASE = 0x43460`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 17`，
//! 逐项见 [`BinError`]。
//!
//! 输出分仓的难点不在「找个仓丢进去」，而在三条通道的**计量口径**不同：
//! 1. 逐张（Stack）以「张」计量，一张占一个仓内张位；
//! 2. 装订（Staple）以「套」计量，每套固定 [`SET_SHEETS`] 张、压两道钉，
//!    所以 5 张纸在账上不是 5 而是「1 套 / 2 颗钉」；
//! 3. 打孔（Punch）以「孔」计量，同一叠纸落进 2 孔仓还是 3 孔仓，
//!    物理产出与损耗完全不同。
//!
//! 如果三通道都用「张数」记一本糊涂账，装订套数、钉数、孔数就永远对不上
//! 能力表，事后既无法补打也无法判废。因此本模块把「仓能力表」与「路由账本」
//! 分成两层：
//! * 能力表（[`OutputTable`]）只回答「这个仓支不支持该通道、静态容量多少、
//!   打孔几孔」，注册时拒绝自相矛盾的条目（支持打孔却 0 孔）；
//! * 路由（[`Router`]）在剩余容量上选择最低仓号，显式指定仓号时绝不静默
//!   改道；落账（[`RouteLedger`]）按通道换算单元数并逐项复核。
//!
//! J1 固定 30 个作业（3 通道 × 10），每作业 5 张：前 12 个自动选仓，后 18
//! 个显式指定仓号；最终账本必须与能力表逐项一致：逐张 50 张、装订 20 颗钉、
//! 打孔 115 孔，四个仓的占用分别为 100/25/25/0。
//!
//! 结构清单：
//! * [`Channel`]：三通道枚举（稳定码值 + 位掩码）；
//! * [`BinCap`] / [`OutputTable`]：单仓能力 + 能力表注册表（仓号唯一 + 定容）；
//! * [`JobSpec`] / [`RouteDecision`]：作业规格与路由决定；
//! * [`Router`]：带剩余容量的纯路由状态机（显式直达 / 自动最低仓号）；
//! * [`RouteLedger`]：逐作业落账（作业号严格递增 + 通道单元换算 + 封账）；
//! * [`reference_units`] / [`reference_route`]：独立参考实现（不经过账本换算）；
//! * [`injection_core`]：8 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43460 独占 0x20）。
pub const SEG_BASE: u32 = 0x43460;

/// 能力表仓数上限。
pub const MAX_BINS: usize = 8;

/// 单作业张数上限。
pub const MAX_JOB_SHEETS: u16 = 64;

/// 账本容量上限。
pub const MAX_ENTRIES: usize = 4096;

/// 一套装订固定张数。
pub const SET_SHEETS: u16 = 5;

/// J1 作业数（3 通道 × 10）。
pub const J1_JOBS: usize = 30;

/// J1 每作业张数。
pub const J1_SHEETS: u16 = 5;

const E_EMPTY_TABLE: u16 = 1;
const E_DUP_BIN: u16 = 2;
const E_BAD_BIN: u16 = 3;
const E_ZERO_SHEETS: u16 = 4;
const E_SHEET_MAX: u16 = 5;
const E_BAD_CHANNEL: u16 = 6;
const E_BAD_JOB: u16 = 7;
const E_NO_BIN: u16 = 8;
const E_UNSUPPORTED: u16 = 9;
const E_CAPACITY: u16 = 10;
const E_SET: u16 = 11;
const E_DUP: u16 = 12;
const E_ORDER: u16 = 13;
const E_FULL: u16 = 14;
const E_IMMUTABLE: u16 = 15;
const E_AUDIT: u16 = 16;
const E_TABLE: u16 = 17;

/// 输出分仓错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinError {
    /// 能力表为空。
    EmptyTable,
    /// 仓号重复注册。
    DuplicateBin,
    /// 仓号不存在或越界。
    BadBinId,
    /// 作业张数为零。
    ZeroSheets,
    /// 作业张数超过上限。
    SheetOverflow,
    /// 通道码值非法。
    BadChannel,
    /// 作业号为 0。
    BadJob,
    /// 没有任何仓支持该通道。
    NoBin,
    /// 指定仓不支持该通道。
    BinUnsupported,
    /// 仓剩余容量不足。
    CapacityShort,
    /// 装订张数不是整套。
    SetMismatch,
    /// 作业号重复。
    Duplicate,
    /// 作业号未严格递增。
    OrderGap,
    /// 账本容量已满。
    Full,
    /// 封账后只读。
    Immutable,
    /// 账本复核失败（未封账 / 单元数不符）。
    Audit,
    /// 能力表条目自相矛盾（打孔掩码与孔数不一致 / 容量为零）。
    TableAudit,
}

impl BinError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            BinError::EmptyTable => E_EMPTY_TABLE,
            BinError::DuplicateBin => E_DUP_BIN,
            BinError::BadBinId => E_BAD_BIN,
            BinError::ZeroSheets => E_ZERO_SHEETS,
            BinError::SheetOverflow => E_SHEET_MAX,
            BinError::BadChannel => E_BAD_CHANNEL,
            BinError::BadJob => E_BAD_JOB,
            BinError::NoBin => E_NO_BIN,
            BinError::BinUnsupported => E_UNSUPPORTED,
            BinError::CapacityShort => E_CAPACITY,
            BinError::SetMismatch => E_SET,
            BinError::Duplicate => E_DUP,
            BinError::OrderGap => E_ORDER,
            BinError::Full => E_FULL,
            BinError::Immutable => E_IMMUTABLE,
            BinError::Audit => E_AUDIT,
            BinError::TableAudit => E_TABLE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            BinError::EmptyTable => "output bin table is empty",
            BinError::DuplicateBin => "output bin id registered twice",
            BinError::BadBinId => "output bin id does not exist",
            BinError::ZeroSheets => "job has zero sheets",
            BinError::SheetOverflow => "job sheets exceed limit",
            BinError::BadChannel => "unknown output channel code",
            BinError::BadJob => "job id is zero",
            BinError::NoBin => "no bin supports the channel",
            BinError::BinUnsupported => "bin does not support the channel",
            BinError::CapacityShort => "bin remaining capacity is short",
            BinError::SetMismatch => "staple sheets are not a whole set",
            BinError::Duplicate => "job id recorded twice",
            BinError::OrderGap => "job ids are not increasing",
            BinError::Full => "ledger capacity exhausted",
            BinError::Immutable => "ledger is sealed",
            BinError::Audit => "ledger audit failed",
            BinError::TableAudit => "bin capability entry is inconsistent",
        }
    }
}

/// 输出通道：逐张 / 装订 / 打孔。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// 逐张堆叠。
    Stack,
    /// 装订（每套固定张数 + 两颗钉）。
    Staple,
    /// 打孔（孔数由仓能力决定）。
    Punch,
}

impl Channel {
    /// 全部通道（按码值顺序）。
    pub const ALL: [Channel; 3] = [Channel::Stack, Channel::Staple, Channel::Punch];

    /// 稳定码值（0/1/2）。
    pub fn code(self) -> u8 {
        match self {
            Channel::Stack => 0,
            Channel::Staple => 1,
            Channel::Punch => 2,
        }
    }

    /// 位掩码（能力表用）。
    pub fn bit(self) -> u8 {
        1u8 << self.code()
    }

    /// 码值转通道；越界给 [`BinError::BadChannel`]。
    pub fn from_code(code: u8) -> Result<Self, BinError> {
        match code {
            0 => Ok(Channel::Stack),
            1 => Ok(Channel::Staple),
            2 => Ok(Channel::Punch),
            _ => Err(BinError::BadChannel),
        }
    }

    /// 一行标签。
    pub fn label(self) -> &'static str {
        match self {
            Channel::Stack => "stack",
            Channel::Staple => "staple",
            Channel::Punch => "punch",
        }
    }

    /// 计量单位名。
    pub fn unit_label(self) -> &'static str {
        match self {
            Channel::Stack => "sheet",
            Channel::Staple => "staple",
            Channel::Punch => "hole",
        }
    }
}

/// 单仓能力。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinCap {
    /// 仓号（1..=MAX_BINS）。
    pub id: u8,
    /// 静态容量（张）。
    pub max_sheets: u16,
    /// 支持通道位掩码。
    pub channels: u8,
    /// 打孔孔数（2 或 3；不支持打孔时为 0）。
    pub punch_holes: u8,
}

impl BinCap {
    /// 构造单仓能力（一致性由 [`OutputTable::push`] 校验）。
    pub fn new(id: u8, max_sheets: u16, channels: u8, punch_holes: u8) -> Self {
        BinCap {
            id,
            max_sheets,
            channels,
            punch_holes,
        }
    }

    /// 是否支持某通道。
    pub fn supports(&self, channel: Channel) -> bool {
        self.channels & channel.bit() != 0
    }

    /// 静态校验：支持、整套（装订）、容量。
    pub fn check(&self, channel: Channel, sheets: u16) -> Result<(), BinError> {
        if !self.supports(channel) {
            return Err(BinError::BinUnsupported);
        }
        if channel == Channel::Staple && sheets % SET_SHEETS != 0 {
            return Err(BinError::SetMismatch);
        }
        if sheets > self.max_sheets {
            return Err(BinError::CapacityShort);
        }
        if channel == Channel::Punch && self.punch_holes == 0 {
            return Err(BinError::BinUnsupported);
        }
        Ok(())
    }

    /// 通道单元换算：逐张=张、装订=钉数（2/套）、打孔=孔数。
    pub fn units(&self, channel: Channel, sheets: u16) -> Result<u16, BinError> {
        match channel {
            Channel::Stack => Ok(sheets),
            Channel::Staple => {
                if sheets % SET_SHEETS != 0 {
                    Err(BinError::SetMismatch)
                } else {
                    Ok((sheets / SET_SHEETS).saturating_mul(2))
                }
            }
            Channel::Punch => {
                if self.punch_holes == 0 {
                    Err(BinError::BinUnsupported)
                } else {
                    Ok(sheets.saturating_mul(self.punch_holes as u16))
                }
            }
        }
    }
}

/// 输出能力表：仓号唯一、定容注册。
pub struct OutputTable {
    bins: Vec<BinCap>,
}

impl OutputTable {
    /// 新建空能力表。
    pub fn new() -> Self {
        OutputTable {
            bins: Vec::with_capacity(MAX_BINS),
        }
    }

    /// 注册一个仓：容量满 / 仓号越界 / 重复 / 掩码矛盾全部拒绝。
    pub fn push(&mut self, cap: BinCap) -> Result<(), BinError> {
        if self.bins.len() >= MAX_BINS {
            return Err(BinError::Full);
        }
        if cap.id == 0 || cap.id as usize > MAX_BINS {
            return Err(BinError::BadBinId);
        }
        if cap.max_sheets == 0 {
            return Err(BinError::TableAudit);
        }
        if cap.channels == 0 {
            return Err(BinError::BadChannel);
        }
        let punch = cap.channels & Channel::Punch.bit() != 0;
        if punch != (cap.punch_holes != 0) {
            return Err(BinError::TableAudit);
        }
        if cap.punch_holes > 3 {
            return Err(BinError::TableAudit);
        }
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                if b.id == cap.id {
                    return Err(BinError::DuplicateBin);
                }
            }
            i += 1;
        }
        self.bins.push(cap);
        Ok(())
    }

    /// 仓数。
    pub fn len(&self) -> usize {
        self.bins.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.bins.is_empty()
    }

    /// 按注册序取仓。
    pub fn get(&self, i: usize) -> Option<&BinCap> {
        self.bins.get(i)
    }

    /// 按仓号取仓。
    pub fn by_id(&self, id: u8) -> Option<&BinCap> {
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                if b.id == id {
                    return Some(b);
                }
            }
            i += 1;
        }
        None
    }

    /// 仓号在注册序中的下标。
    pub fn index_of(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                if b.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否有任何仓支持该通道。
    pub fn supports_any(&self, channel: Channel) -> bool {
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                if b.supports(channel) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 全表复核：非空、仓号唯一、条目一致。
    pub fn validate(&self) -> Result<usize, BinError> {
        if self.bins.is_empty() {
            return Err(BinError::EmptyTable);
        }
        let mut i = 0usize;
        while i < self.bins.len() {
            let a = match self.bins.get(i) {
                Some(v) => v,
                None => return Err(BinError::BadBinId),
            };
            if a.id == 0 || a.id as usize > MAX_BINS || a.max_sheets == 0 || a.channels == 0 {
                return Err(BinError::TableAudit);
            }
            let punch = a.channels & Channel::Punch.bit() != 0;
            if punch != (a.punch_holes != 0) {
                return Err(BinError::TableAudit);
            }
            let mut j = i + 1;
            while j < self.bins.len() {
                if let Some(b) = self.bins.get(j) {
                    if b.id == a.id {
                        return Err(BinError::DuplicateBin);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(self.bins.len())
    }

    /// 能力表指纹（FNV-1a）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_u32(h, self.bins.len() as u32);
        let mut i = 0usize;
        while i < self.bins.len() {
            if let Some(b) = self.bins.get(i) {
                h = fnv_u32(h, b.id as u32);
                h = fnv_u32(h, b.max_sheets as u32);
                h = fnv_u32(h, b.channels as u32);
                h = fnv_u32(h, b.punch_holes as u32);
            }
            i += 1;
        }
        h
    }
}

/// 一个输出作业的规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobSpec {
    /// 作业号（1 起，账本严格递增）。
    pub job_id: u16,
    /// 输出通道。
    pub channel: Channel,
    /// 张数。
    pub sheets: u16,
    /// 显式指定仓号；None 为自动。
    pub hint: Option<u8>,
}

impl JobSpec {
    /// 构造作业规格。
    pub fn new(job_id: u16, channel: Channel, sheets: u16, hint: Option<u8>) -> Self {
        JobSpec {
            job_id,
            channel,
            sheets,
            hint,
        }
    }

    /// 装载校验：作业号 / 张数 / 装订整套。
    pub fn validate(&self) -> Result<(), BinError> {
        if self.job_id == 0 {
            return Err(BinError::BadJob);
        }
        if self.sheets == 0 {
            return Err(BinError::ZeroSheets);
        }
        if self.sheets > MAX_JOB_SHEETS {
            return Err(BinError::SheetOverflow);
        }
        if self.channel == Channel::Staple && self.sheets % SET_SHEETS != 0 {
            return Err(BinError::SetMismatch);
        }
        Ok(())
    }

    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "j{}/c{}/s{}/hint{}",
            self.job_id,
            self.channel.code(),
            self.sheets,
            match self.hint {
                Some(v) => v,
                None => 0,
            }
        )
    }
}

/// 路由决定：作业落到哪个仓、换算成多少单元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecision {
    /// 作业号。
    pub job_id: u16,
    /// 通道。
    pub channel: Channel,
    /// 目标仓号。
    pub bin: u8,
    /// 张数。
    pub sheets: u16,
    /// 通道单元数（张 / 钉 / 孔）。
    pub units: u16,
    /// 是否显式指定仓号。
    pub explicit: bool,
}

impl RouteDecision {
    /// 一行文本。
    pub fn line(&self) -> String {
        format!(
            "j{}/{}->b{}/u{}",
            self.job_id,
            self.channel.label(),
            self.bin,
            self.units
        )
    }
}

/// 路由状态机：在能力表上维护各仓剩余容量。
pub struct Router {
    used: Vec<u16>,
    last_job: u16,
}

impl Router {
    /// 新建路由器（按能力表注册序记录占用）。
    pub fn new(table: &OutputTable) -> Self {
        Router {
            used: vec_with_zeros(table.len()),
            last_job: 0,
        }
    }

    /// 当前占用快照（按注册序）。
    pub fn snapshot(&self) -> Vec<u16> {
        self.used.clone()
    }

    /// 某仓已占张数。
    pub fn used_of(&self, table: &OutputTable, id: u8) -> Option<u16> {
        match table.index_of(id) {
            Some(i) => self.used.get(i).copied(),
            None => None,
        }
    }

    /// 最近一次提交的作业号。
    pub fn last_job(&self) -> u16 {
        self.last_job
    }

    /// 选择目标仓：显式直达不降级，自动档取最低仓号且剩余容量足够者。
    pub fn route(&self, table: &OutputTable, spec: &JobSpec) -> Result<RouteDecision, BinError> {
        if table.is_empty() {
            return Err(BinError::EmptyTable);
        }
        spec.validate()?;
        match spec.hint {
            Some(id) => {
                let idx = match table.index_of(id) {
                    Some(v) => v,
                    None => return Err(BinError::BadBinId),
                };
                let cap = match table.by_id(id) {
                    Some(v) => *v,
                    None => return Err(BinError::BadBinId),
                };
                cap.check(spec.channel, spec.sheets)?;
                let used = self.used.get(idx).copied().unwrap_or(0);
                if used.saturating_add(spec.sheets) > cap.max_sheets {
                    return Err(BinError::CapacityShort);
                }
                let units = cap.units(spec.channel, spec.sheets)?;
                Ok(RouteDecision {
                    job_id: spec.job_id,
                    channel: spec.channel,
                    bin: id,
                    sheets: spec.sheets,
                    units,
                    explicit: true,
                })
            }
            None => {
                let mut supported = false;
                let mut i = 0usize;
                while i < table.len() {
                    let cap = match table.get(i) {
                        Some(v) => *v,
                        None => return Err(BinError::BadBinId),
                    };
                    if cap.supports(spec.channel) {
                        supported = true;
                        let used = self.used.get(i).copied().unwrap_or(0);
                        if used.saturating_add(spec.sheets) <= cap.max_sheets
                            && cap.check(spec.channel, spec.sheets).is_ok()
                        {
                            let units = cap.units(spec.channel, spec.sheets)?;
                            return Ok(RouteDecision {
                                job_id: spec.job_id,
                                channel: spec.channel,
                                bin: cap.id,
                                sheets: spec.sheets,
                                units,
                                explicit: false,
                            });
                        }
                    }
                    i += 1;
                }
                if supported {
                    Err(BinError::CapacityShort)
                } else {
                    Err(BinError::NoBin)
                }
            }
        }
    }

    /// 提交路由决定：再次复核剩余容量，扣减占用并推进作业号。
    pub fn commit(&mut self, table: &OutputTable, d: &RouteDecision) -> Result<(), BinError> {
        if d.job_id == 0 {
            return Err(BinError::BadJob);
        }
        if d.job_id == self.last_job {
            return Err(BinError::Duplicate);
        }
        if d.job_id < self.last_job {
            return Err(BinError::OrderGap);
        }
        let idx = match table.index_of(d.bin) {
            Some(v) => v,
            None => return Err(BinError::BadBinId),
        };
        let cap = match table.by_id(d.bin) {
            Some(v) => *v,
            None => return Err(BinError::BadBinId),
        };
        if !cap.supports(d.channel) {
            return Err(BinError::BinUnsupported);
        }
        let used = self.used.get(idx).copied().unwrap_or(0);
        if used.saturating_add(d.sheets) > cap.max_sheets {
            return Err(BinError::CapacityShort);
        }
        match self.used.get_mut(idx) {
            Some(slot) => *slot = used.saturating_add(d.sheets),
            None => return Err(BinError::BadBinId),
        }
        self.last_job = d.job_id;
        Ok(())
    }
}

/// 落账账本：逐作业记录，作业号严格递增，封账后只读。
pub struct RouteLedger {
    entries: Vec<RouteDecision>,
    cap: usize,
    sealed: bool,
}

impl RouteLedger {
    /// 新建账本。
    pub fn new(cap: usize) -> Self {
        RouteLedger {
            entries: Vec::with_capacity(cap),
            cap,
            sealed: false,
        }
    }

    /// 入账一条路由决定：作业号为零 / 重复 / 逆序 / 满 / 封账分别给码。
    pub fn insert(&mut self, d: RouteDecision) -> Result<(), BinError> {
        if self.sealed {
            return Err(BinError::Immutable);
        }
        if self.entries.len() >= self.cap {
            return Err(BinError::Full);
        }
        if d.job_id == 0 {
            return Err(BinError::BadJob);
        }
        if let Some(prev) = self.entries.last() {
            if d.job_id == prev.job_id {
                return Err(BinError::Duplicate);
            }
            if d.job_id < prev.job_id {
                return Err(BinError::OrderGap);
            }
        }
        self.entries.push(d);
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按序取决定。
    pub fn get(&self, i: usize) -> Option<&RouteDecision> {
        self.entries.get(i)
    }

    /// 全账复核：封账、作业号递增、仓存在且支持通道、单元数与能力表一致、
    /// 各仓累计张数不超过静态容量。
    pub fn verify(&self, table: &OutputTable) -> Result<usize, BinError> {
        if !self.sealed {
            return Err(BinError::Audit);
        }
        let mut bin_sum = vec_with_zeros(table.len());
        let mut prev: u16 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            let d = match self.entries.get(i) {
                Some(v) => *v,
                None => return Err(BinError::Audit),
            };
            if d.job_id == 0 {
                return Err(BinError::BadJob);
            }
            if i > 0 {
                if d.job_id == prev {
                    return Err(BinError::Duplicate);
                }
                if d.job_id < prev {
                    return Err(BinError::OrderGap);
                }
            }
            prev = d.job_id;
            let cap = match table.by_id(d.bin) {
                Some(v) => *v,
                None => return Err(BinError::BadBinId),
            };
            if !cap.supports(d.channel) {
                return Err(BinError::BinUnsupported);
            }
            let want = reference_units(&cap, d.channel, d.sheets)?;
            if d.units != want {
                return Err(BinError::Audit);
            }
            let idx = match table.index_of(d.bin) {
                Some(v) => v,
                None => return Err(BinError::BadBinId),
            };
            if let Some(slot) = bin_sum.get_mut(idx) {
                *slot = slot.saturating_add(d.sheets);
                if *slot > cap.max_sheets {
                    return Err(BinError::CapacityShort);
                }
            } else {
                return Err(BinError::BadBinId);
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 账本指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_u32(h, self.entries.len() as u32);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(d) = self.entries.get(i) {
                h = fnv_u32(h, d.job_id as u32);
                h = fnv_u32(h, d.channel.code() as u32);
                h = fnv_u32(h, d.bin as u32);
                h = fnv_u32(h, d.sheets as u32);
                h = fnv_u32(h, d.units as u32);
            }
            i += 1;
        }
        h
    }

    /// 多行文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(d) = self.entries.get(i) {
                s.push_str(&d.line());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 独立参考：通道单元换算（按能力表原始字段，不经过账本逻辑）。
pub fn reference_units(cap: &BinCap, channel: Channel, sheets: u16) -> Result<u16, BinError> {
    match channel {
        Channel::Stack => Ok(sheets),
        Channel::Staple => {
            if sheets % SET_SHEETS != 0 {
                Err(BinError::SetMismatch)
            } else {
                Ok((sheets / SET_SHEETS) * 2)
            }
        }
        Channel::Punch => {
            if cap.punch_holes == 0 {
                Err(BinError::BinUnsupported)
            } else {
                Ok(sheets * cap.punch_holes as u16)
            }
        }
    }
}

/// 独立参考路由：在占用快照上取最低仓号（与 [`Router::route`] 无共享分支）。
pub fn reference_route(
    table: &OutputTable,
    used: &[u16],
    spec: &JobSpec,
) -> Result<u8, BinError> {
    if table.is_empty() {
        return Err(BinError::EmptyTable);
    }
    spec.validate()?;
    let mut i = 0usize;
    let mut supported = false;
    while i < table.len() {
        if let Some(cap) = table.get(i) {
            if cap.supports(spec.channel) {
                supported = true;
                let now = used.get(i).copied().unwrap_or(0);
                let fits = now + spec.sheets <= cap.max_sheets
                    && spec.sheets <= cap.max_sheets
                    && (spec.channel != Channel::Staple || spec.sheets % SET_SHEETS == 0);
                if fits {
                    match spec.hint {
                        Some(id) => {
                            if cap.id == id {
                                return Ok(id);
                            }
                        }
                        None => return Ok(cap.id),
                    }
                } else if spec.hint == Some(cap.id) {
                    return Err(BinError::CapacityShort);
                }
            }
            i += 1;
        } else {
            return Err(BinError::BadBinId);
        }
    }
    if let Some(id) = spec.hint {
        if table.by_id(id).is_none() {
            return Err(BinError::BadBinId);
        }
        return Err(BinError::BinUnsupported);
    }
    if supported {
        Err(BinError::CapacityShort)
    } else {
        Err(BinError::NoBin)
    }
}

const FNV_OFFSET: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_u32(mut h: u32, v: u32) -> u32 {
    h ^= v;
    h = h.wrapping_mul(FNV_PRIME);
    h
}

fn vec_with_zeros(n: usize) -> Vec<u16> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0);
        i += 1;
    }
    v
}

/// J1 能力表：四仓覆盖「全通道 / 装订 / 打孔 / 仅逐张」四种形态。
pub fn sample_table() -> OutputTable {
    let mut t = OutputTable::new();
    let _ = t.push(BinCap::new(
        1,
        100,
        Channel::Stack.bit() | Channel::Staple.bit() | Channel::Punch.bit(),
        2,
    ));
    let _ = t.push(BinCap::new(
        2,
        60,
        Channel::Stack.bit() | Channel::Staple.bit(),
        0,
    ));
    let _ = t.push(BinCap::new(
        3,
        40,
        Channel::Stack.bit() | Channel::Punch.bit(),
        3,
    ));
    let _ = t.push(BinCap::new(4, 20, Channel::Stack.bit(), 0));
    t
}

/// J1 作业序列：前 12 个自动选仓，后 18 个显式指定且均合法。
pub fn sample_jobs() -> Vec<JobSpec> {
    let mut out = Vec::with_capacity(J1_JOBS);
    let mut i = 0usize;
    while i < J1_JOBS {
        let channel = match i % 3 {
            0 => Channel::Stack,
            1 => Channel::Staple,
            _ => Channel::Punch,
        };
        let hint = if i < 12 {
            None
        } else {
            let j = (i / 3) as u8;
            match channel {
                Channel::Stack => Some(1 + (j % 3)),
                Channel::Staple => Some(if j % 2 == 0 { 2 } else { 1 }),
                Channel::Punch => Some(if j % 2 == 0 { 1 } else { 3 }),
            }
        };
        out.push(JobSpec::new(
            (i as u16).saturating_add(1),
            channel,
            J1_SHEETS,
            hint,
        ));
        i += 1;
    }
    out
}

/// 跑完整个作业序列：路由 → 提交 → 落账 → 封账 → 复核。
pub fn execute(
    table: &OutputTable,
    jobs: &[JobSpec],
) -> Result<(Router, RouteLedger), BinError> {
    let mut router = Router::new(table);
    let mut ledger = RouteLedger::new(MAX_ENTRIES);
    let mut i = 0usize;
    while i < jobs.len() {
        let spec = match jobs.get(i) {
            Some(v) => v,
            None => return Err(BinError::BadJob),
        };
        let d = router.route(table, spec)?;
        router.commit(table, &d)?;
        ledger.insert(d)?;
        i += 1;
    }
    ledger.seal();
    ledger.verify(table)?;
    Ok((router, ledger))
}

/// 8 类坏样本注入核：每类都必须命中各自的诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let table = sample_table();

    // 1) 空表：任何作业都无仓可选。
    let empty = OutputTable::new();
    let r0 = Router::new(&empty);
    if r0.route(&empty, &JobSpec::new(1, Channel::Stack, 5, None)).err()
        == Some(BinError::EmptyTable)
        && r0.route(&empty, &JobSpec::new(1, Channel::Stack, 0, None)).err()
            == Some(BinError::EmptyTable)
    {
        caught += 1;
    }

    // 2) 能力表注册：重复仓号 / 越界仓号 / 掩码矛盾。
    let mut reg = OutputTable::new();
    let _ = reg.push(BinCap::new(1, 10, Channel::Stack.bit() | Channel::Punch.bit(), 2));
    if reg
        .push(BinCap::new(1, 10, Channel::Stack.bit(), 0))
        .err()
        == Some(BinError::DuplicateBin)
        && reg.push(BinCap::new(0, 10, Channel::Stack.bit(), 0)).err()
            == Some(BinError::BadBinId)
        && reg
            .push(BinCap::new(2, 10, Channel::Punch.bit(), 0))
            .err()
            == Some(BinError::TableAudit)
    {
        caught += 1;
    }

    // 3) 作业装载：零张 / 超限 / 零作业号 / 坏通道。
    let zero = JobSpec::new(1, Channel::Stack, 0, None).validate();
    let over = JobSpec::new(1, Channel::Stack, MAX_JOB_SHEETS + 1, None).validate();
    let bad_job = JobSpec::new(0, Channel::Stack, 5, None).validate();
    let bad_ch = Channel::from_code(9);
    if zero.err() == Some(BinError::ZeroSheets)
        && over.err() == Some(BinError::SheetOverflow)
        && bad_job.err() == Some(BinError::BadJob)
        && bad_ch.err() == Some(BinError::BadChannel)
    {
        caught += 1;
    }

    // 4) 无仓支持 / 显式仓不支持。
    let mut stack_only = OutputTable::new();
    let _ = stack_only.push(BinCap::new(1, 50, Channel::Stack.bit(), 0));
    let no_bin = Router::new(&stack_only).route(
        &stack_only,
        &JobSpec::new(1, Channel::Punch, 5, None),
    );
    let unsupported = Router::new(&table).route(
        &table,
        &JobSpec::new(1, Channel::Punch, 5, Some(2)),
    );
    if no_bin.err() == Some(BinError::NoBin)
        && unsupported.err() == Some(BinError::BinUnsupported)
    {
        caught += 1;
    }

    // 5) 显式容量不足 / 装订非整套。
    let short = Router::new(&table).route(
        &table,
        &JobSpec::new(1, Channel::Stack, 25, Some(4)),
    );
    let set = JobSpec::new(1, Channel::Staple, 7, Some(1)).validate();
    if short.err() == Some(BinError::CapacityShort)
        && set.err() == Some(BinError::SetMismatch)
    {
        caught += 1;
    }

    // 6) 路由提交：重复提交 / 逆序提交。
    let mut rr = Router::new(&table);
    let d1 = RouteDecision {
        job_id: 1,
        channel: Channel::Stack,
        bin: 1,
        sheets: 5,
        units: 5,
        explicit: false,
    };
    let d2 = RouteDecision {
        job_id: 2,
        channel: Channel::Stack,
        bin: 1,
        sheets: 5,
        units: 5,
        explicit: false,
    };
    let ok1 = rr.commit(&table, &d1).is_ok();
    let dup = rr.commit(&table, &d1).err() == Some(BinError::Duplicate);
    let ok2 = rr.commit(&table, &d2).is_ok();
    let back = rr.commit(&table, &d1).err() == Some(BinError::OrderGap);
    if ok1 && dup && ok2 && back {
        caught += 1;
    }

    // 7) 账本纪律：重复作业号 / 逆序作业号。
    let mut led = RouteLedger::new(8);
    let first = led.insert(d1).is_ok();
    let dup_j = led.insert(d1).err() == Some(BinError::Duplicate);
    let _ = led.insert(d2);
    let order = led.insert(d1).err() == Some(BinError::OrderGap);
    if first && dup_j && order {
        caught += 1;
    }

    // 8) 封账 / 未封账复核；单元数篡改。
    let mut sealed = RouteLedger::new(4);
    let _ = sealed.insert(d1);
    let unsealed = sealed.verify(&table).err() == Some(BinError::Audit);
    sealed.seal();
    let mut tampered = RouteLedger::new(4);
    let wrong = RouteDecision {
        job_id: 1,
        channel: Channel::Punch,
        bin: 1,
        sheets: 5,
        units: 999,
        explicit: false,
    };
    let _ = tampered.insert(wrong);
    tampered.seal();
    if unsealed && tampered.verify(&table).err() == Some(BinError::Audit) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50066 自检：18 条判据（J1 原文：通道 ×3 ×10 落账与能力表一致）。
pub fn run_unx50066_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50066");
    let table = sample_table();
    let jobs = sample_jobs();
    let run = execute(&table, &jobs);

    // J1-1 三十作业全量落入账本，封账复核通过。
    let mut landed = false;
    let mut detail = String::new();
    if let Ok((_router, ledger)) = &run {
        landed = ledger.len() == J1_JOBS
            && ledger.is_sealed()
            && ledger.verify(&table).is_ok();
        detail = format!(
            "n={} sealed={}",
            ledger.len(),
            if ledger.is_sealed() { 1 } else { 0 }
        );
    }
    s.add_dyn("50066-J1-三十作业落账", landed, &detail);

    // J1-2 逐项对拍：每条决定的单元数必须等于能力表闭式换算。
    let mut unit_ok = false;
    if let Ok((_router, ledger)) = &run {
        unit_ok = ledger.len() == J1_JOBS;
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                let cap = match table.by_id(d.bin) {
                    Some(c) => *c,
                    None => {
                        unit_ok = false;
                        i += 1;
                        continue;
                    }
                };
                match reference_units(&cap, d.channel, d.sheets) {
                    Ok(u) => {
                        if u != d.units {
                            unit_ok = false;
                        }
                    }
                    Err(_) => unit_ok = false,
                }
            } else {
                unit_ok = false;
            }
            i += 1;
        }
    }
    s.add(
        "50066-J1-单元逐项对拍",
        unit_ok,
        if unit_ok { "" } else { "单元数与能力表不符" },
    );

    // J1-3 通道汇总：逐张 50 张 / 装订 20 钉 / 打孔 115 孔。
    let mut ch_sum = [0u32; 3];
    let mut ch_count = [0usize; 3];
    let mut sums_ok = true;
    if let Ok((_router, ledger)) = &run {
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                let c = d.channel.code() as usize;
                if c < 3 {
                    ch_sum[c] += d.units as u32;
                    ch_count[c] += 1;
                } else {
                    sums_ok = false;
                }
            }
            i += 1;
        }
        sums_ok = sums_ok
            && ch_sum[0] == 50
            && ch_sum[1] == 20
            && ch_sum[2] == 115
            && ch_count == [10usize, 10, 10];
    }
    let det = format!("s{}/st{}/p{}", ch_sum[0], ch_sum[1], ch_sum[2]);
    s.add(
        "50066-J1-通道汇总",
        sums_ok,
        if sums_ok { "" } else { "通道汇总偏离 50/20/115" },
    );
    let _ = det;

    // J1-4 分仓占用：路由占用与账本重算一致，且不超静态容量。
    let mut bins_ok = false;
    if let Ok((router, ledger)) = &run {
        bins_ok = table.validate() == Ok(4);
        let mut sums = vec_with_zeros(table.len());
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                if let Some(idx) = table.index_of(d.bin) {
                    if let Some(slot) = sums.get_mut(idx) {
                        *slot = slot.saturating_add(d.sheets);
                    }
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < table.len() {
            if let Some(cap) = table.get(k) {
                let used = match router.used_of(&table, cap.id) {
                    Some(v) => v,
                    None => 0,
                };
                let counted = sums.get(k).copied().unwrap_or(0);
                if used != counted || counted > cap.max_sheets {
                    bins_ok = false;
                }
            }
            k += 1;
        }
        bins_ok = bins_ok
            && router.used_of(&table, 1) == Some(100)
            && router.used_of(&table, 2) == Some(25)
            && router.used_of(&table, 3) == Some(25)
            && router.used_of(&table, 4) == Some(0);
    }
    s.add(
        "50066-J1-分仓占用",
        bins_ok,
        if bins_ok { "" } else { "分仓占用/容量不符" },
    );

    // J1-5 自动档：前 12 个作业显式位为假，全部命中最低仓号；
    // 并用占用快照跑独立参考路由逐项对拍。
    let mut auto_ok = false;
    if let Ok((_router, ledger)) = &run {
        auto_ok = ledger.len() == J1_JOBS;
        let mut i = 0usize;
        while i < 12 && i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                if d.explicit || d.bin != 1 {
                    auto_ok = false;
                }
            } else {
                auto_ok = false;
            }
            i += 1;
        }
        let mut replay = Router::new(&table);
        let mut k = 0usize;
        while k < 12 && k < ledger.len() {
            if let (Some(spec), Some(d)) = (jobs.get(k), ledger.get(k)) {
                let snap = replay.snapshot();
                match reference_route(&table, &snap, spec) {
                    Ok(id) => {
                        if id != d.bin {
                            auto_ok = false;
                        }
                    }
                    Err(_) => auto_ok = false,
                }
                let _ = replay.commit(&table, d);
            } else {
                auto_ok = false;
            }
            k += 1;
        }
    }
    s.add(
        "50066-J1-自动最低仓号",
        auto_ok,
        if auto_ok { "" } else { "自动档未取最低仓号" },
    );

    // J1-6 显式直达：后 18 个作业必须落在指定仓，不允许静默改道。
    let mut hint_ok = false;
    if let Ok((_router, ledger)) = &run {
        hint_ok = ledger.len() == J1_JOBS;
        let mut i = 12usize;
        while i < ledger.len() {
            if let (Some(d), Some(spec)) = (ledger.get(i), jobs.get(i)) {
                if !d.explicit || spec.hint != Some(d.bin) {
                    hint_ok = false;
                }
            } else {
                hint_ok = false;
            }
            i += 1;
        }
    }
    s.add(
        "50066-J1-显式直达",
        hint_ok,
        if hint_ok { "" } else { "显式仓号被静默改道" },
    );

    // J1-7 通道换算口径：逐张=张、装订=2 钉/套、打孔=张×孔数。
    let mut formula_ok = false;
    if let Ok((_router, ledger)) = &run {
        formula_ok = true;
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                let want = match d.channel {
                    Channel::Stack => d.sheets,
                    Channel::Staple => (d.sheets / SET_SHEETS) * 2,
                    Channel::Punch => match table.by_id(d.bin) {
                        Some(c) => d.sheets * c.punch_holes as u16,
                        None => 0,
                    },
                };
                if d.units != want {
                    formula_ok = false;
                }
            }
            i += 1;
        }
    }
    s.add(
        "50066-J1-三通道口径",
        formula_ok,
        if formula_ok { "" } else { "通道换算口径不一致" },
    );

    // J1-8 装订套数：10 个装订作业各 5 张 = 1 套 2 钉，合计 20 钉。
    let mut staple_ok = false;
    if let Ok((_router, ledger)) = &run {
        staple_ok = true;
        let mut staple_jobs = 0usize;
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                if d.channel == Channel::Staple {
                    staple_jobs += 1;
                    if d.sheets % SET_SHEETS != 0 || d.units != 2 {
                        staple_ok = false;
                    }
                }
            }
            i += 1;
        }
        staple_ok = staple_ok && staple_jobs == 10;
    }
    s.add(
        "50066-J1-装订套数",
        staple_ok,
        if staple_ok { "" } else { "装订套数/钉数不符" },
    );

    // J1-9 打孔孔数：2 孔仓 20 张→40 孔，3 孔仓 15 张→45 孔，合计 115。
    let mut punch_ok = false;
    if let Ok((_router, ledger)) = &run {
        let mut holes = 0u32;
        let mut punch_jobs = 0usize;
        let mut i = 0usize;
        while i < ledger.len() {
            if let Some(d) = ledger.get(i) {
                if d.channel == Channel::Punch {
                    punch_jobs += 1;
                    match table.by_id(d.bin) {
                        Some(c) => {
                            if c.punch_holes != 2 && c.punch_holes != 3 {
                                holes = u32::MAX;
                            } else {
                                holes += d.sheets as u32 * c.punch_holes as u32;
                            }
                        }
                        None => holes = u32::MAX,
                    }
                }
            }
            i += 1;
        }
        punch_ok = punch_jobs == 10 && holes == 115;
    }
    s.add(
        "50066-J1-打孔孔数",
        punch_ok,
        if punch_ok { "" } else { "打孔孔数不等于 115" },
    );

    // J1-10 路由状态：route 不扣容量，commit 才扣；重复/逆序提交给码。
    let mut router_ok = false;
    if let Ok(mut r) = Ok::<Router, BinError>(Router::new(&table)) {
        let spec = JobSpec::new(1, Channel::Stack, 5, None);
        let before = r.used_of(&table, 1) == Some(0);
        let d = r.route(&table, &spec);
        let still = r.used_of(&table, 1) == Some(0);
        if let Ok(dec) = d {
            let first = r.commit(&table, &dec).is_ok();
            let after = r.used_of(&table, 1) == Some(5);
            let dup = r.commit(&table, &dec).err() == Some(BinError::Duplicate);
            let d2 = RouteDecision {
                job_id: 2,
                channel: Channel::Stack,
                bin: 1,
                sheets: 5,
                units: 5,
                explicit: false,
            };
            let ok2 = r.commit(&table, &d2).is_ok();
            let back = r.commit(&table, &dec).err() == Some(BinError::OrderGap);
            router_ok = before && still && first && after && dup && ok2 && back;
        }
    }
    s.add(
        "50066-J1-路由状态",
        router_ok,
        if router_ok { "" } else { "路由提交状态不符" },
    );

    // J1-11 账本纪律：重复 / 逆序 / 容量满 / 封账各自给码。
    let d1 = RouteDecision {
        job_id: 1,
        channel: Channel::Stack,
        bin: 1,
        sheets: 5,
        units: 5,
        explicit: false,
    };
    let d2 = RouteDecision {
        job_id: 2,
        channel: Channel::Stack,
        bin: 1,
        sheets: 5,
        units: 5,
        explicit: false,
    };
    let mut led = RouteLedger::new(2);
    let first = led.insert(d1).is_ok();
    let dup = led.insert(d1).err() == Some(BinError::Duplicate);
    let second = led.insert(d2).is_ok();
    let full = led.insert(d2).err() == Some(BinError::Full);
    let mut led2 = RouteLedger::new(2);
    let _ = led2.insert(d2);
    let order = led2.insert(d1).err() == Some(BinError::OrderGap);
    led.seal();
    let imm = led.insert(d2).err() == Some(BinError::Immutable);
    let disc_ok = first && dup && second && full && order && imm;
    s.add(
        "50066-J1-账本纪律",
        disc_ok,
        if disc_ok { "" } else { "账本纪律缺口" },
    );

    // J1-12 封账复核：未封账→Audit；单元数篡改→Audit；封账后通过。
    let mut audit_ok = false;
    let mut tamper = RouteLedger::new(4);
    let wrong = RouteDecision {
        job_id: 1,
        channel: Channel::Punch,
        bin: 1,
        sheets: 5,
        units: 999,
        explicit: false,
    };
    let _ = tamper.insert(wrong);
    let unsealed = tamper.verify(&table).err() == Some(BinError::Audit);
    tamper.seal();
    let bad_units = tamper.verify(&table).err() == Some(BinError::Audit);
    if let Ok((_router, ledger)) = &run {
        audit_ok = unsealed
            && bad_units
            && ledger.verify(&table).is_ok()
            && !ledger.text().is_empty();
    }
    s.add(
        "50066-J1-封账复核",
        audit_ok,
        if audit_ok { "" } else { "封账/篡改复核缺口" },
    );

    // J1-13 诊断码闭集：17 个变体码值连续、互异、基址正确。
    let all = [
        BinError::EmptyTable,
        BinError::DuplicateBin,
        BinError::BadBinId,
        BinError::ZeroSheets,
        BinError::SheetOverflow,
        BinError::BadChannel,
        BinError::BadJob,
        BinError::NoBin,
        BinError::BinUnsupported,
        BinError::CapacityShort,
        BinError::SetMismatch,
        BinError::Duplicate,
        BinError::OrderGap,
        BinError::Full,
        BinError::Immutable,
        BinError::Audit,
        BinError::TableAudit,
    ];
    let mut codes_ok = all.len() == 17;
    let mut i = 0usize;
    while i < all.len() {
        if all[i].code() != SEG_BASE + (i as u32) + 1 || all[i].message().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[j].code() == all[i].code() {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "50066-J1-诊断码闭集",
        codes_ok,
        if codes_ok { "n=17" } else { "码值不连续/重复" },
    );

    // J1-14 坏样本注入：8 类逐类命中。
    let (caught, total) = injection_core();
    s.add(
        "50066-J1-坏样本注入",
        caught == total && total == 8,
        if caught == total { "8/8" } else { "注入未全命中" },
    );

    // J1-15 能力表注册：非空复核 / 重复 / 越界 / 掩码矛盾 / 容量为 0。
    let mut reg_ok = table.validate() == Ok(4);
    let mut reg = OutputTable::new();
    let _ = reg.push(BinCap::new(1, 10, Channel::Stack.bit() | Channel::Punch.bit(), 2));
    reg_ok = reg_ok
        && reg
            .push(BinCap::new(1, 10, Channel::Stack.bit(), 0))
            .err()
            == Some(BinError::DuplicateBin)
        && reg.push(BinCap::new(0, 10, Channel::Stack.bit(), 0)).err()
            == Some(BinError::BadBinId)
        && reg.push(BinCap::new(2, 0, Channel::Stack.bit(), 0)).err()
            == Some(BinError::TableAudit)
        && reg
            .push(BinCap::new(3, 10, Channel::Punch.bit(), 0))
            .err()
            == Some(BinError::TableAudit)
        && reg.push(BinCap::new(4, 10, 0, 0)).err() == Some(BinError::BadChannel);
    let mut full = OutputTable::new();
    let mut k = 1u8;
    while k as usize <= MAX_BINS {
        let _ = full.push(BinCap::new(k, 10, Channel::Stack.bit(), 0));
        k += 1;
    }
    reg_ok = reg_ok
        && full.push(BinCap::new(9, 10, Channel::Stack.bit(), 0)).err()
            == Some(BinError::Full);
    s.add(
        "50066-J1-能力表注册",
        reg_ok,
        if reg_ok { "" } else { "能力表注册校验缺口" },
    );

    // J1-16 装载边界：零张 / 超限 / 零作业号 / 坏通道码。
    let load_ok = JobSpec::new(1, Channel::Stack, 0, None).validate().err()
        == Some(BinError::ZeroSheets)
        && JobSpec::new(1, Channel::Stack, MAX_JOB_SHEETS + 1, None)
            .validate()
            .err()
            == Some(BinError::SheetOverflow)
        && JobSpec::new(0, Channel::Stack, 5, None).validate().err()
            == Some(BinError::BadJob)
        && Channel::from_code(3).err() == Some(BinError::BadChannel)
        && Channel::from_code(2) == Ok(Channel::Punch)
        && Channel::from_code(1) == Ok(Channel::Staple);
    s.add(
        "50066-J1-装载边界",
        load_ok,
        if load_ok { "" } else { "装载校验缺口" },
    );

    // J1-17 无仓 / 不支持 / 容量不足三类路由拒绝。
    let mut stack_only = OutputTable::new();
    let _ = stack_only.push(BinCap::new(1, 50, Channel::Stack.bit(), 0));
    let no_bin = Router::new(&stack_only).route(
        &stack_only,
        &JobSpec::new(1, Channel::Punch, 5, None),
    );
    let unsupported = Router::new(&table).route(
        &table,
        &JobSpec::new(1, Channel::Punch, 5, Some(2)),
    );
    let short = Router::new(&table).route(
        &table,
        &JobSpec::new(1, Channel::Stack, 25, Some(4)),
    );
    let bad_id = Router::new(&table).route(
        &table,
        &JobSpec::new(1, Channel::Stack, 5, Some(7)),
    );
    let reject_ok = no_bin.err() == Some(BinError::NoBin)
        && unsupported.err() == Some(BinError::BinUnsupported)
        && short.err() == Some(BinError::CapacityShort)
        && bad_id.err() == Some(BinError::BadBinId);
    s.add(
        "50066-J1-路由拒绝",
        reject_ok,
        if reject_ok { "" } else { "路由拒绝码不符" },
    );

    // J1-18 确定性：同一作业序列两次执行，账本指纹与占用一致。
    let again = execute(&table, &jobs);
    let mut det_ok = false;
    if let (Ok((ra, la)), Ok((rb, lb))) = (&run, &again) {
        det_ok = la.digest() == lb.digest()
            && ra.snapshot() == rb.snapshot()
            && ra.used_of(&table, 1) == rb.used_of(&table, 1);
    }
    s.add(
        "50066-J1-确定性",
        det_ok,
        if det_ok { "" } else { "两次执行落账不一致" },
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50066_thirty_jobs_accounting_matches_table() {
        let table = sample_table();
        let jobs = sample_jobs();
        let run = execute(&table, &jobs);
        assert!(run.is_ok());
        if let Ok((router, ledger)) = run {
            assert_eq!(ledger.len(), J1_JOBS);
            assert_eq!(ledger.verify(&table), Ok(J1_JOBS));
            assert_eq!(router.used_of(&table, 1), Some(100));
            assert_eq!(router.used_of(&table, 2), Some(25));
            assert_eq!(router.used_of(&table, 3), Some(25));
            assert_eq!(router.used_of(&table, 4), Some(0));
        }
    }

    #[test]
    fn unx50066_channel_units_are_distinct() {
        let table = sample_table();
        let jobs = sample_jobs();
        let run = execute(&table, &jobs);
        assert!(run.is_ok());
        if let Ok((_router, ledger)) = run {
            let mut stack = 0u32;
            let mut staple = 0u32;
            let mut punch = 0u32;
            let mut i = 0usize;
            while i < ledger.len() {
                if let Some(d) = ledger.get(i) {
                    match d.channel {
                        Channel::Stack => stack += d.units as u32,
                        Channel::Staple => staple += d.units as u32,
                        Channel::Punch => punch += d.units as u32,
                    }
                }
                i += 1;
            }
            assert_eq!(stack, 50);
            assert_eq!(staple, 20);
            assert_eq!(punch, 115);
        }
    }

    #[test]
    fn unx50066_explicit_hint_never_falls_back() {
        let table = sample_table();
        let r = Router::new(&table);
        let bad = JobSpec::new(1, Channel::Punch, 5, Some(2));
        assert_eq!(r.route(&table, &bad).err(), Some(BinError::BinUnsupported));
        let tight = JobSpec::new(2, Channel::Stack, 25, Some(4));
        assert_eq!(r.route(&table, &tight).err(), Some(BinError::CapacityShort));
    }

    #[test]
    fn unx50066_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
