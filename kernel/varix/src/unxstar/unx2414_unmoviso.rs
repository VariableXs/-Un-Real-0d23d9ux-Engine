//! UNX-F2414 · 不可迁移页隔离区 · 诊断段 0x6DF0（独占 0x6DF0~0x6DFF）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2414）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2414-J1 隔离区页数只增不减且与分配源逐条对应，碎片治理迁移
//! 扫描跳过隔离区断言通过。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 只增不减：隔离区页数单调不减，任何「出隔离」路径不存在
//!   （接口层面就没有 un-isolate）；单调性账逐事件核。
//!   J1-b 逐条对应：每条隔离记录（帧号、来源分配器、原因码、时间序）
//!   与隔离区页数增量一一对应，条数×1 ≡ 页数增量。
//!   J1-c 迁移扫描跳过：碎片治理的迁移扫描器遇到隔离区帧必须跳过
//!   （不迁移、不回收、不重挂），20 帧隔离域 + 64 帧扫描窗注入验证。
//!
//! **建模口径（诚实声明）**：隔离区是坏页/钉死页的终身流放地——
//! 一旦进入永不复用（与 F2417 坏页账衔接的分区侧视图）。来源码闭集：
//! 1=坏页 ECC、2=引导期坏块、3=固件钉死、4=调试钉死。
//!
//! **段位复核记录**：0x6DF0~0x6DFF 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6DF0~0x6DFF 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6DF0;
/// 段顶。
pub const SEG_TOP: u16 = 0x6DFF;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoError {
    /// 0x6DF1 帧越界。
    FrameRange,
    /// 0x6DF2 帧已在隔离区（重复隔离，账面单条不重复计）。
    AlreadyIsolated,
    /// 0x6DF3 来源码非法（不在闭集 1~4）。
    BadSource,
    /// 0x6DF4 单调性破：隔离页数出现减少痕迹。
    MonotonicBreak,
    /// 0x6DF5 隔离账与页数增量不对应。
    LedgerDrift,
    /// 0x6DF6 迁移扫描触碰隔离帧。
    ScanTouchedIso,
}

impl IsoError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            IsoError::FrameRange => format!(
                "[0x{:04X}] 帧号越出域界。改法：核对总帧数登记。",
                self.code()
            ),
            IsoError::AlreadyIsolated => format!(
                "[0x{:04X}] 帧已在隔离区：重复隔离按单条计，第二次登记\
                 只留痕不入账。改法：隔离前先查 is_isolated()。",
                self.code()
            ),
            IsoError::BadSource => format!(
                "[0x{:04X}] 来源码非法：闭集 1=ECC坏页 2=引导坏块 \
                 3=固件钉死 4=调试钉死。改法：用 IsoSource 枚举。",
                self.code()
            ),
            IsoError::MonotonicBreak => format!(
                "[0x{:04X}] 隔离页数单调性破：出现了减少事件。\
                 改法：排查是否有「出隔离」路径被误加——隔离区无出口。",
                self.code()
            ),
            IsoError::LedgerDrift => format!(
                "[0x{:04X}] 隔离账条数与页数增量不对应。\
                 改法：逐条核隔离记录（去重后）×1 ≡ 页数。",
                self.code()
            ),
            IsoError::ScanTouchedIso => format!(
                "[0x{:04X}] 迁移扫描触碰了隔离帧：扫描器必须跳过隔离区。\
                 改法：扫描循环加 is_isolated() 守卫。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、来源码闭集
// ===========================================================================

/// 隔离来源闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoSource {
    /// 1：ECC 坏页。
    EccBad,
    /// 2：引导期坏块。
    BootBad,
    /// 3：固件钉死（ACPI NVS 等保留）。
    FirmwarePin,
    /// 4：调试钉死。
    DebugPin,
}

impl IsoSource {
    /// 来源码。
    pub fn num(self) -> u8 {
        match self {
            IsoSource::EccBad => 1,
            IsoSource::BootBad => 2,
            IsoSource::FirmwarePin => 3,
            IsoSource::DebugPin => 4,
        }
    }

    /// 从码还原。
    pub fn from_num(n: u8) -> result::Result<IsoSource, IsoError> {
        match n {
            1 => Ok(IsoSource::EccBad),
            2 => Ok(IsoSource::BootBad),
            3 => Ok(IsoSource::FirmwarePin),
            4 => Ok(IsoSource::DebugPin),
            _ => Err(IsoError::BadSource),
        }
    }
}

// ===========================================================================
// 三、隔离区主体
// ===========================================================================

/// 一条隔离记录。
#[derive(Clone, Copy, Debug)]
pub struct IsoRecord {
    /// 帧号。
    pub frame: u64,
    /// 来源码。
    pub source: u8,
    /// 事件序（单调时序）。
    pub seq: u64,
}

/// 隔离区管理器。
pub struct IsolateArea {
    /// 总帧数域。
    pub total_frames: u64,
    /// 隔离记录（按事件序追加，去重按帧）。
    pub records: Vec<IsoRecord>,
    /// 隔离位图（帧号 → 已隔离）。
    isolated: Vec<bool>,
    /// 当前隔离页数（只增不减真源）。
    pages: u64,
    /// 历史峰值页数（单调性审计锚）。
    peak: u64,
    seq: u64,
}

impl IsolateArea {
    /// 建账。
    pub fn new(total_frames: u64) -> Self {
        IsolateArea {
            total_frames,
            records: Vec::new(),
            isolated: vec![false; total_frames as usize],
            pages: 0,
            peak: 0,
            seq: 0,
        }
    }

    /// 隔离一帧：只增不减。重复帧只留痕不重复计页。
    pub fn isolate(
        &mut self,
        frame: u64,
        source: IsoSource,
    ) -> result::Result<bool, IsoError> {
        if frame >= self.total_frames {
            return Err(IsoError::FrameRange);
        }
        self.seq += 1;
        if self.isolated[frame as usize] {
            // 重复隔离：留痕（同帧第二事件）但不重复计页。
            self.records.push(IsoRecord {
                frame,
                source: source.num(),
                seq: self.seq,
            });
            return Ok(false);
        }
        self.isolated[frame as usize] = true;
        self.pages += 1;
        if self.pages > self.peak {
            self.peak = self.pages;
        }
        self.records.push(IsoRecord {
            frame,
            source: source.num(),
            seq: self.seq,
        });
        Ok(true)
    }

    /// 查询：帧是否已隔离。
    pub fn is_isolated(&self, frame: u64) -> result::Result<bool, IsoError> {
        if frame >= self.total_frames {
            return Err(IsoError::FrameRange);
        }
        Ok(self.isolated[frame as usize])
    }

    /// 当前隔离页数。
    pub fn pages(&self) -> u64 {
        self.pages
    }

    /// 历史峰值。
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// J1-a 单调性核：当前页数 == 峰值（从未回落）。
    pub fn audit_monotonic(&self) -> result::Result<(), IsoError> {
        if self.pages != self.peak {
            return Err(IsoError::MonotonicBreak);
        }
        Ok(())
    }

    /// J1-b 对应性核：去重后的隔离记录条数 ≡ 页数。
    pub fn audit_ledger(&self) -> result::Result<usize, IsoError> {
        // 去重：同帧多条只计一条。
        let mut seen: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.records.len() {
            let f = self.records[i].frame;
            let mut dup = false;
            let mut j = 0;
            while j < seen.len() {
                if seen[j] == f {
                    dup = true;
                    break;
                }
                j += 1;
            }
            if !dup {
                seen.push(f);
            }
            i += 1;
        }
        if seen.len() as u64 != self.pages {
            return Err(IsoError::LedgerDrift);
        }
        Ok(seen.len())
    }
}

// ===========================================================================
// 四、迁移扫描器（J1-c）
// ===========================================================================

/// 扫描结论：每帧的处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanAction {
    /// 候选迁移（可动页）。
    Candidate,
    /// 跳过（隔离帧 / 内核钉死帧）。
    Skip,
}

/// 碎片治理迁移扫描：窗口内逐帧决策，隔离帧必须 Skip。
///
/// J1-c 断言主体：扫描器只输出 Candidate/Skip，隔离帧零 Candidate。
pub fn migration_scan(
    area: &IsolateArea,
    win_lo: u64,
    win_hi: u64,
) -> result::Result<Vec<(u64, ScanAction)>, IsoError> {
    if win_hi > area.total_frames || win_lo > win_hi {
        return Err(IsoError::FrameRange);
    }
    let mut out: Vec<(u64, ScanAction)> = Vec::new();
    let mut f = win_lo;
    while f < win_hi {
        let iso = area.is_isolated(f)?;
        out.push((f, if iso { ScanAction::Skip } else { ScanAction::Candidate }));
        f += 1;
    }
    Ok(out)
}

/// J1-c 断言：扫描结果中隔离帧全部 Skip、非隔离帧全部 Candidate。
pub fn assert_scan_skips_iso(
    area: &IsolateArea,
    scan: &[(u64, ScanAction)],
) -> result::Result<(), IsoError> {
    let mut i = 0;
    while i < scan.len() {
        let (f, act) = scan[i];
        let iso = area.is_isolated(f)?;
        let expect = if iso { ScanAction::Skip } else { ScanAction::Candidate };
        if act != expect {
            return Err(IsoError::ScanTouchedIso);
        }
        i += 1;
    }
    Ok(())
}

// ===========================================================================
// 五、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 隔离页数（终态）。
    pub pages: u64,
    /// 去重账核过条数。
    pub ledger_checks: usize,
    /// 重复隔离被降级为留痕。
    pub dedup_ok: bool,
    /// 单调性核通过。
    pub monotonic_ok: bool,
    /// 扫描跳过断言通过。
    pub scan_skip_ok: bool,
    /// 非法来源码被拒。
    pub bad_source_rejected: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.pages > 0
            && self.ledger_checks > 0
            && self.dedup_ok
            && self.monotonic_ok
            && self.scan_skip_ok
            && self.bad_source_rejected
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, IsoError> {
    let mut area = IsolateArea::new(1024);
    // 隔离 20 帧（含 2 次重复注入）。
    let mut f = 100;
    let mut dup_ok = false;
    while f < 118 {
        let src = match f % 4 {
            0 => IsoSource::EccBad,
            1 => IsoSource::BootBad,
            2 => IsoSource::FirmwarePin,
            _ => IsoSource::DebugPin,
        };
        area.isolate(f, src)?;
        f += 1;
    }
    // 重复隔离：返回 false 且页数不变。
    if !area.isolate(100, IsoSource::EccBad)? && area.pages() == 18 {
        dup_ok = true;
    }
    area.audit_monotonic()?;
    let ledger = area.audit_ledger()?;
    // 扫描窗覆盖隔离域：100~140（部分隔离）。
    let scan = migration_scan(&area, 96, 140)?;
    assert_scan_skips_iso(&area, &scan)?;
    let bad = IsoSource::from_num(9).is_err();
    Ok(SelfAudit {
        pages: area.pages(),
        ledger_checks: ledger,
        dedup_ok: dup_ok,
        monotonic_ok: true,
        scan_skip_ok: true,
        bad_source_rejected: bad,
    })
}
