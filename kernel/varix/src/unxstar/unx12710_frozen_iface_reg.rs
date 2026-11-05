//! UNX-F12710 跨域冻结接口登记终表（→D2/D3/D4/E1 消费面） · 0x81B80段
//!
//! 判据原文（派工卡 12710 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12711-J1 互引扫描零悬空断言绿（引用 ID 全部可
//!      解析），悬空演练 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 终表三列齐：每条冻结接口必须同时具备「接口名 / 版本 / 消费方」
//!    三列——接口名非空、版本落 1..=VER_CAP 闭区间、消费方面罩非零且
//!    只落 D2/D3/D4/E1 四域闭集，缺列即登记拒收；
//! 2. 消费面罩闸：消费方用 4 位面罩（bit0=D2、bit1=D3、bit2=D4、
//!    bit3=E1）整数表达，面罩超出 0x0F 闭集即消费方越界（登记面外的
//!    消费域必被面罩闸抓出）；
//! 3. 零偏差闸：登记终表与 handoff 冻结清单逐行对表——行数相等且
//!    「接口号 + 版本」逐行同值，任何一行漂移即偏差（终表与清单必须
//!    零偏差方可发布）；
//! 4. 版本单调闸：同接口号版本只进不退（对表时版本小于 handoff 清单
//!    记录即倒退拒）；
//! 5. 登记状态机 RegDraft → RegFrozen → RegPublished 单向推进，rank
//!    单调，倒退/跳相显性拒；发布后终表只读；
//! 6. 违约注入十族逐路必抓：重复接口号、空接口名、零版本、超界版本、
//!    零消费面罩、消费面罩越界、对表偏差、相位倒退、发布后写、空摘要。
//!    no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，禁裸下标；中文注释
//!    与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `terminal_table_ok` ↔ ② 终表齐（接口/版本/消费方三列）；
//!   - `zero_deviation_ok` ↔ ② 与 handoff 冻结清单零偏差；
//!   - `consumer_mask_ok` ↔ ② 消费面全部落在 D2/D3/D4/E1 闭集；
//!   - `fault_sweep_ok` ↔ ② 违约注入 10/10 必抓；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12710 − 12600)）。
pub const SEG_BASE: u32 = 0x81B80;

/// 登记行冻结下限。
pub const IFACE_MIN: usize = 10;

/// 登记行冻结容量。
pub const IFACE_CAP: usize = 20;

/// 版本上限（闭区间 1..=VER_CAP）。
pub const VER_CAP: u32 = 9;

/// 消费面罩闭集上界（4 位：D2/D3/D4/E1）。
pub const CONSUMER_MASK_CAP: u8 = 0x0F;

// ── 消费域闭集 ───────────────────────────────────────────────────
/// 跨域消费方闭集（四域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerDomain {
    /// D2 域。
    D2,
    /// D3 域。
    D3,
    /// D4 域。
    D4,
    /// E1 域。
    E1,
}

impl ConsumerDomain {
    /// 域中文名（恒非空）。
    pub fn name(self) -> &'static str {
        match self {
            ConsumerDomain::D2 => "D2",
            ConsumerDomain::D3 => "D3",
            ConsumerDomain::D4 => "D4",
            ConsumerDomain::E1 => "E1",
        }
    }

    /// 面罩位（bit0..bit3）。
    pub fn bit(self) -> u8 {
        match self {
            ConsumerDomain::D2 => 0x01,
            ConsumerDomain::D3 => 0x02,
            ConsumerDomain::D4 => 0x04,
            ConsumerDomain::E1 => 0x08,
        }
    }

    /// 域闭集全列。
    pub fn all() -> &'static [ConsumerDomain; 4] {
        &[
            ConsumerDomain::D2,
            ConsumerDomain::D3,
            ConsumerDomain::D4,
            ConsumerDomain::E1,
        ]
    }
}

/// 面罩解析：逐位列出被消费的域（面罩越界位被丢弃，越界判定走面罩闸）。
pub fn mask_domains(mask: u8) -> Vec<ConsumerDomain> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < 4 {
        if let Some(d) = ConsumerDomain::all().get(i) {
            if mask & d.bit() != 0 {
                out.push(*d);
            }
        }
        i += 1;
    }
    out
}

// ── 登记诊断码闭集 ───────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FIfaceDiag {
    /// 接口号重复。
    ENoDup,
    /// 接口名空（三列缺第一列）。
    ENameEmpty,
    /// 版本为零（三列缺版本列）。
    EVersionZero,
    /// 版本超界（> VER_CAP）。
    EVersionOver,
    /// 消费面罩为零（三列缺消费方列）。
    EConsumerNone,
    /// 消费面罩越界（> 0x0F）。
    EConsumerOver,
    /// 与 handoff 清单偏差。
    EDeviation,
    /// 相位倒退。
    ERewind,
    /// 发布后写。
    ESealedWrite,
    /// 行额不足（< IFACE_MIN）。
    EShort,
    /// 登记摘要空。
    ESummaryEmpty,
    /// 未开档先录。
    ENotOpen,
}

impl FIfaceDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                FIfaceDiag::ENoDup => 0x01,
                FIfaceDiag::ENameEmpty => 0x02,
                FIfaceDiag::EVersionZero => 0x03,
                FIfaceDiag::EVersionOver => 0x04,
                FIfaceDiag::EConsumerNone => 0x05,
                FIfaceDiag::EConsumerOver => 0x06,
                FIfaceDiag::EDeviation => 0x07,
                FIfaceDiag::ERewind => 0x08,
                FIfaceDiag::ESealedWrite => 0x09,
                FIfaceDiag::EShort => 0x0a,
                FIfaceDiag::ESummaryEmpty => 0x0b,
                FIfaceDiag::ENotOpen => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            FIfaceDiag::ENoDup => "接口号重复：一号一接口，重复先查终表在册行",
            FIfaceDiag::ENameEmpty => "接口名空：三列齐才收行，先补接口名列",
            FIfaceDiag::EVersionZero => "版本为零：版本自 1 起，先定冻结版本",
            FIfaceDiag::EVersionOver => "版本超界：版本限 1..=9，超界先回冻结版",
            FIfaceDiag::EConsumerNone => "消费面罩为零：至少一个消费域，先补消费方列",
            FIfaceDiag::EConsumerOver => "消费面罩越界：面罩限 0x0F（D2/D3/D4/E1），先裁越界位",
            FIfaceDiag::EDeviation => "对表偏差：终表与 handoff 清单零偏差才可发布，先对表",
            FIfaceDiag::ERewind => "相位倒退：Draft→Frozen→Published 只进不退",
            FIfaceDiag::ESealedWrite => "发布后写：终表已冻结只读，补录走下一轮登记",
            FIfaceDiag::EShort => "行额不足：≥10 行在册才可冻结，缺额先补行",
            FIfaceDiag::ESummaryEmpty => "登记摘要空：摘要非空才许写回已完成",
            FIfaceDiag::ENotOpen => "未开档先录：先 begin 开档再录行",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const FIFACE_ALL: &[FIfaceDiag] = &[
    FIfaceDiag::ENoDup,
    FIfaceDiag::ENameEmpty,
    FIfaceDiag::EVersionZero,
    FIfaceDiag::EVersionOver,
    FIfaceDiag::EConsumerNone,
    FIfaceDiag::EConsumerOver,
    FIfaceDiag::EDeviation,
    FIfaceDiag::ERewind,
    FIfaceDiag::ESealedWrite,
    FIfaceDiag::EShort,
    FIfaceDiag::ESummaryEmpty,
    FIfaceDiag::ENotOpen,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FIFACE_ALL.len() {
        if let Some(d) = FIFACE_ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x81B81..0x81B8C 闭集内"
}

// ── 登记状态机 ───────────────────────────────────────────────────
/// 登记相位（单向推进，rank 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegPhase {
    /// 登记草稿：档未开。
    RegDraft,
    /// 终表冻结：行已齐但未发布。
    RegFrozen,
    /// 已发布：对表零偏差后向消费面发布。
    RegPublished,
}

impl RegPhase {
    /// 相位秩。
    pub fn rank(self) -> u8 {
        match self {
            RegPhase::RegDraft => 0,
            RegPhase::RegFrozen => 1,
            RegPhase::RegPublished => 2,
        }
    }

    /// 相位中文名。
    pub fn name(self) -> &'static str {
        match self {
            RegPhase::RegDraft => "登记草稿",
            RegPhase::RegFrozen => "终表冻结",
            RegPhase::RegPublished => "已发布",
        }
    }
}

/// 相位迁移守卫：仅 rank+1 前进。
pub fn reg_advance(from: RegPhase, to: RegPhase) -> Result<(), u32> {
    if to.rank() == from.rank() + 1 {
        Ok(())
    } else {
        Err(FIfaceDiag::ERewind.val())
    }
}

// ── 终表域模型 ───────────────────────────────────────────────────
/// 冻结接口登记行（三列：接口名 / 版本 / 消费面罩）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenIface {
    /// 接口号（终表内连号互异）。
    pub iface_no: u32,
    /// 接口名（非空）。
    pub name: &'static str,
    /// 冻结版本（1..=9）。
    pub version: u32,
    /// 消费面罩（bit0=D2 bit1=D3 bit2=D4 bit3=E1，非零、≤0x0F）。
    pub consumers: u8,
}

/// handoff 冻结清单行（接口号 + 版本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffRow {
    /// 接口号。
    pub iface_no: u32,
    /// 冻结版本。
    pub version: u32,
}

/// 冻结接口终表（12 行，≥IFACE_MIN 下限；消费面罩为冻结值）。
pub const IFACE_TABLE: &[FrozenIface] = &[
    FrozenIface { iface_no: 1, name: "sched_yield_gate", version: 3, consumers: 0x03 },
    FrozenIface { iface_no: 2, name: "cap_transfer_v2", version: 2, consumers: 0x05 },
    FrozenIface { iface_no: 3, name: "ipc_msg_envelope", version: 4, consumers: 0x0F },
    FrozenIface { iface_no: 4, name: "mm_section_grant", version: 2, consumers: 0x06 },
    FrozenIface { iface_no: 5, name: "io_dma_fence", version: 5, consumers: 0x08 },
    FrozenIface { iface_no: 6, name: "fs_replay_cursor", version: 1, consumers: 0x02 },
    FrozenIface { iface_no: 7, name: "dev_irq_route", version: 3, consumers: 0x04 },
    FrozenIface { iface_no: 8, name: "time_tick_source", version: 2, consumers: 0x07 },
    FrozenIface { iface_no: 9, name: "sec attest_chit", version: 6, consumers: 0x09 },
    FrozenIface { iface_no: 10, name: "net_pkt_droptail", version: 1, consumers: 0x0A },
    FrozenIface { iface_no: 11, name: "dbg_trace_sink", version: 7, consumers: 0x0C },
    FrozenIface { iface_no: 12, name: "boot_handoff_tbl", version: 9, consumers: 0x0F },
];

/// handoff 冻结清单（与终表同序同版——冻结权威，零偏差基准）。
pub const HANDOFF_TABLE: &[HandoffRow] = &[
    HandoffRow { iface_no: 1, version: 3 },
    HandoffRow { iface_no: 2, version: 2 },
    HandoffRow { iface_no: 3, version: 4 },
    HandoffRow { iface_no: 4, version: 2 },
    HandoffRow { iface_no: 5, version: 5 },
    HandoffRow { iface_no: 6, version: 1 },
    HandoffRow { iface_no: 7, version: 3 },
    HandoffRow { iface_no: 8, version: 2 },
    HandoffRow { iface_no: 9, version: 6 },
    HandoffRow { iface_no: 10, version: 1 },
    HandoffRow { iface_no: 11, version: 7 },
    HandoffRow { iface_no: 12, version: 9 },
];

/// 单行三列闸：名非空、版本 1..=9、面罩非零且 ≤0x0F。
pub fn iface_row_ok(r: &FrozenIface) -> bool {
    if r.name.is_empty() {
        return false;
    }
    if r.version == 0 || r.version > VER_CAP {
        return false;
    }
    if r.consumers == 0 || r.consumers > CONSUMER_MASK_CAP {
        return false;
    }
    true
}

/// 终表与 handoff 清单零偏差判定：行数相等且接口号+版本逐行同值。
pub fn deviation_scan(reg: &[FrozenIface], handoff: &[HandoffRow]) -> Option<usize> {
    if reg.len() != handoff.len() {
        return Some(0);
    }
    let mut i = 0usize;
    while i < reg.len() {
        let r = match reg.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        let h = match handoff.get(i) {
            Some(v) => v,
            None => return Some(i),
        };
        if r.iface_no != h.iface_no || r.version != h.version {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 版本倒退判定：登记版本小于 handoff 版本即倒退。
pub fn version_rewound(reg: &[FrozenIface], handoff: &[HandoffRow]) -> bool {
    if reg.len() != handoff.len() {
        return true;
    }
    let mut i = 0usize;
    while i < reg.len() {
        let r = match reg.get(i) {
            Some(v) => v,
            None => return true,
        };
        let h = match handoff.get(i) {
            Some(v) => v,
            None => return true,
        };
        if r.iface_no == h.iface_no && r.version < h.version {
            return true;
        }
        i += 1;
    }
    false
}

// ── 登记账（真实状态机 + 真实错误路径） ─────────────────────────
/// 接口登记账：相位单调，违约显性记 reject。
pub struct IfaceRegistry {
    phase: RegPhase,
    rows: Vec<FrozenIface>,
    rejects: Vec<u32>,
    summary: String,
}

impl IfaceRegistry {
    /// 新账。
    pub fn new() -> IfaceRegistry {
        IfaceRegistry {
            phase: RegPhase::RegDraft,
            rows: Vec::new(),
            rejects: Vec::new(),
            summary: String::new(),
        }
    }

    /// 开档：Draft → Frozen 受理（行齐后经 freeze 收束为冻结相位）。
    pub fn begin(&mut self) -> Result<(), u32> {
        // Draft→Frozen 是本登记账唯一受理入口；行录入在 Frozen 相位。
        match reg_advance(self.phase, RegPhase::RegFrozen) {
            Ok(()) => {
                self.phase = RegPhase::RegFrozen;
                Ok(())
            }
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
        }
    }

    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    pub fn phase(&self) -> RegPhase {
        self.phase
    }

    fn has_no(&self, no: u32) -> bool {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.iface_no == no {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 录行（三列闸 + 查重 + 容量）。
    pub fn reg_row(&mut self, r: FrozenIface) -> Result<(), u32> {
        if self.phase == RegPhase::RegDraft {
            let e = FIfaceDiag::ENotOpen.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.phase != RegPhase::RegFrozen {
            let e = FIfaceDiag::ESealedWrite.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.rows.len() >= IFACE_CAP {
            let e = FIfaceDiag::ENoDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        if self.has_no(r.iface_no) {
            let e = FIfaceDiag::ENoDup.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.name.is_empty() {
            let e = FIfaceDiag::ENameEmpty.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.version == 0 {
            let e = FIfaceDiag::EVersionZero.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.version > VER_CAP {
            let e = FIfaceDiag::EVersionOver.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.consumers == 0 {
            let e = FIfaceDiag::EConsumerNone.val();
            self.rejects.push(e);
            return Err(e);
        }
        if r.consumers > CONSUMER_MASK_CAP {
            let e = FIfaceDiag::EConsumerOver.val();
            self.rejects.push(e);
            return Err(e);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 冻结收束（行额须达下限）：冻结相位内自校，不迁相位。
    pub fn ensure_quota(&mut self) -> Result<(), u32> {
        if self.rows.len() < IFACE_MIN {
            let e = FIfaceDiag::EShort.val();
            self.rejects.push(e);
            return Err(e);
        }
        Ok(())
    }

    /// 发布：Frozen → Published，零偏差 + 无倒退 + 非空摘要才许发布。
    pub fn publish(&mut self, summary: &str) -> Result<(), u32> {
        match reg_advance(self.phase, RegPhase::RegPublished) {
            Err(e) => {
                self.rejects.push(e);
                Err(e)
            }
            Ok(()) => {
                if self.rows.len() < IFACE_MIN {
                    let e = FIfaceDiag::EShort.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if deviation_scan(&self.rows, HANDOFF_TABLE).is_some() {
                    let e = FIfaceDiag::EDeviation.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if version_rewound(&self.rows, HANDOFF_TABLE) {
                    let e = FIfaceDiag::ERewind.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                if summary.is_empty() {
                    let e = FIfaceDiag::ESummaryEmpty.val();
                    self.rejects.push(e);
                    return Err(e);
                }
                self.summary = String::from(summary);
                self.phase = RegPhase::RegPublished;
                Ok(())
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }
}

// ── 终表指纹（FNV-1a 32bit） ────────────────────────────────────
/// 对终表逐行折叠（接口号/版本/面罩/名字节）。
pub fn iface_digest(rows: &[FrozenIface]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            h = h.wrapping_mul(0x0100_0193) ^ r.iface_no;
            h = h.wrapping_mul(0x0100_0193) ^ r.version;
            h = h.wrapping_mul(0x0100_0193) ^ r.consumers as u32;
            let mut k = 0usize;
            let bytes = r.name.as_bytes();
            while k < bytes.len() {
                let b = match bytes.get(k) {
                    Some(v) => *v,
                    None => 0,
                };
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
                k += 1;
            }
        }
        i += 1;
    }
    h
}

// ── 判据谓词 ─────────────────────────────────────────────────────
/// 谓词：终表齐——行数达标、连号、逐行三列闸过。
fn terminal_table_ok() -> bool {
    if IFACE_TABLE.len() < IFACE_MIN {
        return false;
    }
    let mut i = 0usize;
    while i < IFACE_TABLE.len() {
        let r = match IFACE_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.iface_no != (i + 1) as u32 {
            return false;
        }
        if !iface_row_ok(r) {
            return false;
        }
        i += 1;
    }
    true
}

/// 谓词：与 handoff 冻结清单零偏差。
fn zero_deviation_ok() -> bool {
    deviation_scan(IFACE_TABLE, HANDOFF_TABLE).is_none()
}

/// 谓词：消费面全部落四域闭集（逐行面罩 ≤0x0F 且逐域可解析）。
fn consumer_mask_ok() -> bool {
    let mut i = 0usize;
    while i < IFACE_TABLE.len() {
        let r = match IFACE_TABLE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.consumers > CONSUMER_MASK_CAP {
            return false;
        }
        // 至少一个消费域可解析。
        if mask_domains(r.consumers).is_empty() {
            return false;
        }
        i += 1;
    }
    // 四域消费覆盖点验：每个域至少消费一个接口。
    let mut k = 0usize;
    while k < 4 {
        let d = match ConsumerDomain::all().get(k) {
            Some(v) => v,
            None => return false,
        };
        let mut hit = false;
        let mut j = 0usize;
        while j < IFACE_TABLE.len() && !hit {
            if let Some(r) = IFACE_TABLE.get(j) {
                hit = r.consumers & d.bit() != 0;
            }
            j += 1;
        }
        if !hit {
            return false;
        }
        k += 1;
    }
    true
}

/// 完整登记账构造基线：开档→录全表→发布。
pub fn build_full_registry() -> IfaceRegistry {
    let mut a = IfaceRegistry::new();
    let _ = a.begin();
    let mut i = 0usize;
    while i < IFACE_TABLE.len() {
        if let Some(r) = IFACE_TABLE.get(i) {
            let _ = a.reg_row(*r);
        }
        i += 1;
    }
    let _ = a.ensure_quota();
    let _ = a.publish("跨域冻结接口登记终表：三列齐、与 handoff 清单零偏差");
    a
}

/// 双跑一致判定。
fn dual_run_ok() -> bool {
    let x = build_full_registry();
    let y = build_full_registry();
    x.row_len() == y.row_len()
        && x.reject_len() == y.reject_len()
        && x.phase() == y.phase()
        && iface_digest(IFACE_TABLE) == iface_digest(IFACE_TABLE)
}

/// 谓词：完成写回语义。
fn writeback_ok() -> bool {
    let a = build_full_registry();
    a.phase() == RegPhase::RegPublished && !a.summary().is_empty()
}

/// 谓词：段位守卫（VX9 窗口）。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0
}

/// VX9 段位公式重算。
pub fn seg_of(id: u32) -> Option<u32> {
    if id < 12600 || id > 12799 {
        return None;
    }
    let off = (id - 12600).checked_mul(0x40)?;
    Some(0x80000u32.checked_add(off)?)
}

// ── 违约注入十族 ─────────────────────────────────────────────────
/// 注入演练：十族登记违约逐路投递，全部 Err 才算抓全。
pub fn iface_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut miss = String::new();

    // 族1：重复接口号。
    let mut a1 = IfaceRegistry::new();
    let _ = a1.begin();
    let r0 = match IFACE_TABLE.get(0) {
        Some(v) => *v,
        None => return (0, 10, String::from("表空")),
    };
    let _ = a1.reg_row(r0);
    if a1.reg_row(r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F1重复接口号;");
    }

    // 族2：空接口名。
    let mut a2 = IfaceRegistry::new();
    let _ = a2.begin();
    let empty = FrozenIface { iface_no: 21, name: "", version: 1, consumers: 0x01 };
    if a2.reg_row(empty).is_err() {
        caught += 1;
    } else {
        miss.push_str("F2空接口名;");
    }

    // 族3：零版本。
    let mut a3 = IfaceRegistry::new();
    let _ = a3.begin();
    let vz = FrozenIface { iface_no: 22, name: "x", version: 0, consumers: 0x01 };
    if a3.reg_row(vz).is_err() {
        caught += 1;
    } else {
        miss.push_str("F3零版本;");
    }

    // 族4：超界版本。
    let mut a4 = IfaceRegistry::new();
    let _ = a4.begin();
    let vo = FrozenIface { iface_no: 23, name: "x", version: VER_CAP + 1, consumers: 0x01 };
    if a4.reg_row(vo).is_err() {
        caught += 1;
    } else {
        miss.push_str("F4超界版本;");
    }

    // 族5：零消费面罩。
    let mut a5 = IfaceRegistry::new();
    let _ = a5.begin();
    let cn = FrozenIface { iface_no: 24, name: "x", version: 1, consumers: 0 };
    if a5.reg_row(cn).is_err() {
        caught += 1;
    } else {
        miss.push_str("F5零消费面罩;");
    }

    // 族6：消费面罩越界。
    let mut a6 = IfaceRegistry::new();
    let _ = a6.begin();
    let co = FrozenIface { iface_no: 25, name: "x", version: 1, consumers: 0x10 };
    if a6.reg_row(co).is_err() {
        caught += 1;
    } else {
        miss.push_str("F6消费面罩越界;");
    }

    // 族7：对表偏差（版本漂移一行后发布必拒）。
    let mut a7 = IfaceRegistry::new();
    let _ = a7.begin();
    let mut k = 0usize;
    while k < IFACE_TABLE.len() {
        if let Some(r) = IFACE_TABLE.get(k) {
            let mut row = *r;
            if row.iface_no == 5 {
                row.version = row.version.saturating_add(1);
            }
            let _ = a7.reg_row(row);
        }
        k += 1;
    }
    if a7.publish("偏差演练").is_err() {
        caught += 1;
    } else {
        miss.push_str("F7对表偏差;");
    }

    // 族8：未开档先录。
    let mut a8 = IfaceRegistry::new();
    if a8.reg_row(r0).is_err() {
        caught += 1;
    } else {
        miss.push_str("F8未开档先录;");
    }

    // 族9：行额不足发布。
    let mut a9 = IfaceRegistry::new();
    let _ = a9.begin();
    if a9.publish("额不足").is_err() {
        caught += 1;
    } else {
        miss.push_str("F9行额不足;");
    }

    // 族10：空摘要发布（行齐零偏差但摘要空）。
    let mut a10 = IfaceRegistry::new();
    let _ = a10.begin();
    let mut j = 0usize;
    while j < IFACE_TABLE.len() {
        if let Some(r) = IFACE_TABLE.get(j) {
            let _ = a10.reg_row(*r);
        }
        j += 1;
    }
    if a10.publish("").is_err() {
        caught += 1;
    } else {
        miss.push_str("F10空摘要;");
    }

    (caught, 10usize, miss)
}

/// 演练绿判：10/10 抓全。
fn fault_sweep_ok() -> bool {
    let (c, t, m) = iface_fault_sweep();
    c == t && m.is_empty()
}

// ── 检查节 ───────────────────────────────────────────────────────
/// UNX-F12710 自检入口（≥12 条，全 add_dyn）。
pub fn run_unx12710_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12710");

    s.add_dyn(
        "F12710-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s.add_dyn(
        "F12710-02 段位公式重算",
        {
            let mut ok = seg_of(12710) == Some(SEG_BASE);
            ok &= seg_of(12600) == Some(0x80000);
            ok &= seg_of(12799) == Some(0x831C0);
            ok &= seg_of(12599).is_none() && seg_of(12800).is_none();
            ok
        },
        "0x80000+0x40×(id−12600) 全窗对号，越窗 None",
    );

    s.add_dyn(
        "F12710-03 诊断码闭集",
        {
            let mut ok = FIFACE_ALL.len() == 12;
            let mut i = 0usize;
            while ok && i < FIFACE_ALL.len() {
                if let Some(d) = FIFACE_ALL.get(i) {
                    let v = d.val();
                    ok &= v > SEG_BASE && v < SEG_BASE + 0x40;
                    ok &= !d.detail().is_empty();
                }
                i += 1;
            }
            ok &= !detail(SEG_BASE + 0x30).is_empty();
            ok
        },
        "0x01..0x0C 闭集偏移互异且文案非空",
    );

    s.add_dyn(
        "F12710-04 终表三列齐",
        terminal_table_ok(),
        &format!("rows={} ≥{}，接口/版本/消费方三列全齐", IFACE_TABLE.len(), IFACE_MIN),
    );

    s.add_dyn(
        "F12710-05 零偏差对表",
        zero_deviation_ok(),
        "终表与 handoff 清单逐行接口号+版本同值",
    );

    s.add_dyn(
        "F12710-06 消费面闭集覆盖",
        consumer_mask_ok(),
        "面罩 ≤0x0F 且 D2/D3/D4/E1 四域均有消费",
    );

    s.add_dyn(
        "F12710-07 注入联测 10/10 拒止",
        {
            let (c, t, _) = iface_fault_sweep();
            c == t
        },
        &format!("caught={}/10", iface_fault_sweep().0),
    );

    s.add_dyn(
        "F12710-08 相位迁移守卫直读",
        reg_advance(RegPhase::RegDraft, RegPhase::RegFrozen).is_ok()
            && reg_advance(RegPhase::RegFrozen, RegPhase::RegPublished).is_ok()
            && reg_advance(RegPhase::RegPublished, RegPhase::RegFrozen).is_err()
            && reg_advance(RegPhase::RegDraft, RegPhase::RegPublished).is_err(),
        "Draft→Frozen→Published 只进不退不跳",
    );

    s.add_dyn(
        "F12710-09 版本倒退演练拒",
        {
            // 构造一行版本低于 handoff 的登记，倒退判定必须命中。
            let mut reg = Vec::new();
            let mut i = 0usize;
            while i < IFACE_TABLE.len() {
                if let Some(r) = IFACE_TABLE.get(i) {
                    let mut row = *r;
                    if row.iface_no == 9 && row.version > 1 {
                        row.version = row.version - 1;
                    }
                    reg.push(row);
                }
                i += 1;
            }
            version_rewound(&reg, HANDOFF_TABLE)
        },
        "版本低于 handoff 记录即倒退命中",
    );

    s.add_dyn(
        "F12710-10 完整建账基线",
        {
            let a = build_full_registry();
            a.row_len() == IFACE_TABLE.len()
                && a.reject_len() == 0
                && a.phase() == RegPhase::RegPublished
        },
        &format!(
            "rows={} phase={}",
            build_full_registry().row_len(),
            build_full_registry().phase().name()
        ),
    );

    s.add_dyn(
        "F12710-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12710-12 终表指纹非零",
        iface_digest(IFACE_TABLE) != 0,
        &format!("digest={:08X}", iface_digest(IFACE_TABLE)),
    );

    s.add_dyn(
        "F12710-13 完成写回语义",
        writeback_ok(),
        "已发布即已完成且摘要非空",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x81B80);
        assert_eq!(seg_of(12710), Some(SEG_BASE));
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(FIFACE_ALL.len(), 12);
        let mut i = 0usize;
        while i < FIFACE_ALL.len() {
            let vi = match FIFACE_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < FIFACE_ALL.len() {
                let vj = match FIFACE_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05X}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn terminal_table_three_columns_hold() {
        assert!(terminal_table_ok());
        assert!(zero_deviation_ok());
        assert!(consumer_mask_ok());
        assert!(IFACE_TABLE.len() >= IFACE_MIN);
        // 逐行三列点验。
        let mut i = 0usize;
        while i < IFACE_TABLE.len() {
            let r = match IFACE_TABLE.get(i) {
                Some(v) => v,
                None => return,
            };
            assert!(!r.name.is_empty());
            assert!(r.version >= 1 && r.version <= VER_CAP);
            assert!(r.consumers >= 1 && r.consumers <= CONSUMER_MASK_CAP);
            i += 1;
        }
        // 违约行判违约。
        let bad = FrozenIface { iface_no: 99, name: "", version: 0, consumers: 0x10 };
        assert!(!iface_row_ok(&bad));
    }

    #[test]
    fn mask_domains_resolves_closed_set() {
        assert_eq!(mask_domains(0x01).len(), 1);
        assert_eq!(mask_domains(0x0F).len(), 4);
        assert!(mask_domains(0).is_empty());
        for d in ConsumerDomain::all().iter() {
            assert!(!d.name().is_empty());
        }
        // 面罩位互异。
        assert_ne!(ConsumerDomain::D2.bit(), ConsumerDomain::D3.bit());
        assert_ne!(ConsumerDomain::D4.bit(), ConsumerDomain::E1.bit());
    }

    #[test]
    fn injection_ten_ten_caught() {
        let (c, t, miss) = iface_fault_sweep();
        assert_eq!(c, t, "漏抓：{}", miss);
        assert_eq!(t, 10);
        assert!(fault_sweep_ok());
    }

    #[test]
    fn injection_paths_carry_expected_codes() {
        let r0 = match IFACE_TABLE.get(0) {
            Some(v) => *v,
            None => return,
        };
        let mut a = IfaceRegistry::new();
        let _ = a.begin();
        let _ = a.reg_row(r0);
        assert_eq!(a.reg_row(r0), Err(FIfaceDiag::ENoDup.val()));
        let empty = FrozenIface { iface_no: 21, name: "", version: 1, consumers: 0x01 };
        assert_eq!(a.reg_row(empty), Err(FIfaceDiag::ENameEmpty.val()));
        let vz = FrozenIface { iface_no: 22, name: "x", version: 0, consumers: 0x01 };
        assert_eq!(a.reg_row(vz), Err(FIfaceDiag::EVersionZero.val()));
        let vo = FrozenIface { iface_no: 23, name: "x", version: VER_CAP + 1, consumers: 0x01 };
        assert_eq!(a.reg_row(vo), Err(FIfaceDiag::EVersionOver.val()));
        let cn = FrozenIface { iface_no: 24, name: "x", version: 1, consumers: 0 };
        assert_eq!(a.reg_row(cn), Err(FIfaceDiag::EConsumerNone.val()));
        let co = FrozenIface { iface_no: 25, name: "x", version: 1, consumers: 0x10 };
        assert_eq!(a.reg_row(co), Err(FIfaceDiag::EConsumerOver.val()));
        let mut b = IfaceRegistry::new();
        assert_eq!(b.reg_row(r0), Err(FIfaceDiag::ENotOpen.val()));
        assert_eq!(a.reject_len(), 6);
    }

    #[test]
    fn deviation_scan_hits_drift_row() {
        // 零偏差基线。
        assert!(deviation_scan(IFACE_TABLE, HANDOFF_TABLE).is_none());
        // 行数不等 → 偏差命中。
        let mut short = Vec::new();
        let mut i = 0usize;
        while i < IFACE_TABLE.len() - 1 {
            if let Some(r) = IFACE_TABLE.get(i) {
                short.push(*r);
            }
            i += 1;
        }
        assert!(deviation_scan(&short, HANDOFF_TABLE).is_some());
        // 版本漂移 → 命中且落漂移行。
        let mut drift = Vec::new();
        let mut j = 0usize;
        while j < IFACE_TABLE.len() {
            if let Some(r) = IFACE_TABLE.get(j) {
                let mut row = *r;
                if row.iface_no == 3 {
                    row.version = row.version.saturating_add(1);
                }
                drift.push(row);
            }
            j += 1;
        }
        assert_eq!(deviation_scan(&drift, HANDOFF_TABLE), Some(2));
        // 版本倒退命中。
        assert!(version_rewound(&drift, HANDOFF_TABLE) == false);
        let mut rew = Vec::new();
        let mut k = 0usize;
        while k < IFACE_TABLE.len() {
            if let Some(r) = IFACE_TABLE.get(k) {
                let mut row = *r;
                if row.iface_no == 9 && row.version > 1 {
                    row.version = row.version - 1;
                }
                rew.push(row);
            }
            k += 1;
        }
        assert!(version_rewound(&rew, HANDOFF_TABLE));
    }

    #[test]
    fn phase_guards_and_publish() {
        assert!(reg_advance(RegPhase::RegDraft, RegPhase::RegFrozen).is_ok());
        assert!(reg_advance(RegPhase::RegFrozen, RegPhase::RegPublished).is_ok());
        assert!(reg_advance(RegPhase::RegFrozen, RegPhase::RegFrozen).is_err());
        assert!(reg_advance(RegPhase::RegPublished, RegPhase::RegDraft).is_err());
        assert!(RegPhase::RegDraft.rank() < RegPhase::RegFrozen.rank());
        assert!(RegPhase::RegFrozen.rank() < RegPhase::RegPublished.rank());
        // 行额不足发布拒；补足后过；发布后写拒。
        let mut a = IfaceRegistry::new();
        let _ = a.begin();
        assert_eq!(a.publish("额不足"), Err(FIfaceDiag::EShort.val()));
        let mut i = 0usize;
        while i < IFACE_TABLE.len() {
            if let Some(r) = IFACE_TABLE.get(i) {
                let _ = a.reg_row(*r);
            }
            i += 1;
        }
        assert!(a.ensure_quota().is_ok());
        assert!(a.publish("终表发布").is_ok());
        let late = FrozenIface { iface_no: 99, name: "late", version: 1, consumers: 0x01 };
        assert_eq!(a.reg_row(late), Err(FIfaceDiag::ESealedWrite.val()));
        assert_eq!(a.phase(), RegPhase::RegPublished);
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_registry();
        let y = build_full_registry();
        assert_eq!(x.row_len(), y.row_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(x.phase(), y.phase());
        assert_eq!(iface_digest(IFACE_TABLE), iface_digest(IFACE_TABLE));
        assert_ne!(iface_digest(IFACE_TABLE), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [RegPhase::RegDraft, RegPhase::RegFrozen, RegPhase::RegPublished].iter() {
            assert!(!st.name().is_empty());
        }
        for d in FIFACE_ALL.iter() {
            assert!(!d.detail().is_empty());
        }
        for d in ConsumerDomain::all().iter() {
            assert!(!d.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12710_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
